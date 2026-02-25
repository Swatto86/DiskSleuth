//! DiskSleuth application icon generator.
//!
//! Produces a procedural icon: a pie-chart (disk usage visualisation) with
//! a magnifying-glass ring and handle (the "sleuth" motif).  The icon is
//! rendered at an arbitrary resolution as RGBA pixel data suitable for use
//! as a window icon or for ICO file generation.

/// Generate a DiskSleuth icon as egui `IconData`.
pub fn generate_icon(size: u32) -> egui::IconData {
    let rgba = render_icon(size);
    egui::IconData {
        rgba,
        width: size,
        height: size,
    }
}

/// Render the icon into an RGBA pixel buffer (top-to-bottom row order).
pub fn render_icon(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut pixels = vec![0u8; (size * size * 4) as usize];

    // ── Layout ──────────────────────────────────────────────────
    // Lens centre is shifted up-left to leave room for the handle.
    let cx = s * 0.42;
    let cy = s * 0.42;
    let radius = s * 0.34;

    // Ring (magnifying-glass bezel).
    let ring_width = s * 0.045;
    let ring_outer = radius + ring_width;

    // Handle extends from circle edge at 45° toward the lower-right.
    let angle_45: f32 = std::f32::consts::FRAC_PI_4;
    let h_start_x = cx + ring_outer * angle_45.cos();
    let h_start_y = cy + ring_outer * angle_45.sin();
    let h_end_x = s * 0.91;
    let h_end_y = s * 0.91;
    let h_width_start = s * 0.055;
    let h_width_end = s * 0.075;

    // ── Pie chart segments ──────────────────────────────────────
    // Angles in degrees (from +X axis, clockwise in screen space).
    let segments: &[(f32, f32, [u8; 3])] = &[
        (0.0, 144.0, [0x89, 0xb4, 0xfa]),   // Blue  — documents
        (144.0, 245.0, [0xa6, 0xe3, 0xa1]), // Green — free / code
        (245.0, 314.0, [0xf9, 0xe2, 0xaf]), // Amber — media
        (314.0, 360.0, [0xf3, 0x8b, 0xa8]), // Pink  — system
    ];

    let boundaries: Vec<f32> = segments.iter().map(|seg| seg.0).collect();

    // ── Per-pixel rendering ─────────────────────────────────────
    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let dx = px - cx;
            let dy = py - cy;
            let dist = (dx * dx + dy * dy).sqrt();

            let mut cr: u8 = 0;
            let mut cg: u8 = 0;
            let mut cb: u8 = 0;
            let mut ca: f32 = 0.0;

            // 1. Pie-chart circle (the "lens" interior). ─────────
            if dist < radius + 1.5 {
                let edge_aa = smooth_edge(dist, radius);

                let angle_deg = dy.atan2(dx).to_degrees();
                let angle_deg = if angle_deg < 0.0 {
                    angle_deg + 360.0
                } else {
                    angle_deg
                };

                // Pick segment colour.
                let mut seg_col = segments[0].2;
                for &(start, end, col) in segments {
                    if angle_deg >= start && angle_deg < end {
                        seg_col = col;
                        break;
                    }
                }

                // Darken near segment boundaries for visual separation.
                let bd = boundary_factor(angle_deg, &boundaries);
                cr = (seg_col[0] as f32 * (1.0 - 0.35 * bd)) as u8;
                cg = (seg_col[1] as f32 * (1.0 - 0.35 * bd)) as u8;
                cb = (seg_col[2] as f32 * (1.0 - 0.35 * bd)) as u8;
                ca = edge_aa;

                // Subtle radial shading (darker toward edge).
                let shade = 1.0 - 0.12 * (dist / radius);
                cr = (cr as f32 * shade).min(255.0) as u8;
                cg = (cg as f32 * shade).min(255.0) as u8;
                cb = (cb as f32 * shade).min(255.0) as u8;

                // Glass-like highlight in the upper-left quadrant.
                let hx = dx + radius * 0.30;
                let hy = dy + radius * 0.30;
                let highlight_dist = (hx * hx + hy * hy).sqrt();
                let highlight = (1.0 - highlight_dist / (radius * 0.65)).max(0.0) * 0.18;
                cr = (cr as f32 + highlight * 255.0).min(255.0) as u8;
                cg = (cg as f32 + highlight * 255.0).min(255.0) as u8;
                cb = (cb as f32 + highlight * 255.0).min(255.0) as u8;
            }

            // 2. Magnifying-glass ring. ──────────────────────────
            let ring_inner = radius - 0.5;
            if dist > ring_inner && dist < ring_outer + 1.5 {
                let aa_in = smooth_edge_inv(dist, ring_inner);
                let aa_out = smooth_edge(dist, ring_outer);
                let ring_alpha = aa_in * aa_out;

                // Slight gradient: lighter at top, darker at bottom.
                let grad = 0.5 + 0.5 * (1.0 - (dy / radius).clamp(-1.0, 1.0)) * 0.5;
                let rr = (0x70 as f32 * grad).min(255.0) as u8;
                let rg = (0x78 as f32 * grad).min(255.0) as u8;
                let rb = (0x85 as f32 * grad).min(255.0) as u8;

                cr = lerp_c(cr, rr, ring_alpha);
                cg = lerp_c(cg, rg, ring_alpha);
                cb = lerp_c(cb, rb, ring_alpha);
                ca = ca + (1.0 - ca) * ring_alpha;
            }

            // 3. Handle. ─────────────────────────────────────────
            let ld = point_to_seg_dist(px, py, h_start_x, h_start_y, h_end_x, h_end_y);
            let t = project_t(px, py, h_start_x, h_start_y, h_end_x, h_end_y);

            if t > -0.05 && t < 1.05 {
                let half_w = h_width_start + (h_width_end - h_width_start) * t.clamp(0.0, 1.0);
                if ld < half_w + 1.5 {
                    let handle_aa = smooth_edge(ld, half_w);

                    // Slight gradient along the handle.
                    let tt = t.clamp(0.0, 1.0);
                    let hr = lerp_c(0x78, 0x50, tt);
                    let hg = lerp_c(0x7d, 0x55, tt);
                    let hb = lerp_c(0x88, 0x60, tt);

                    cr = lerp_c(cr, hr, handle_aa);
                    cg = lerp_c(cg, hg, handle_aa);
                    cb = lerp_c(cb, hb, handle_aa);
                    ca = ca + (1.0 - ca) * handle_aa;
                }
            }

            let idx = ((y * size + x) * 4) as usize;
            pixels[idx] = cr;
            pixels[idx + 1] = cg;
            pixels[idx + 2] = cb;
            pixels[idx + 3] = (ca * 255.0).clamp(0.0, 255.0) as u8;
        }
    }

    pixels
}

// ── Helpers ─────────────────────────────────────────────────────

/// Smooth anti-aliased edge (1 → 0 as `dist` crosses `edge`).
fn smooth_edge(dist: f32, edge: f32) -> f32 {
    let d = dist - edge;
    if d < -1.0 {
        1.0
    } else if d > 1.0 {
        0.0
    } else {
        0.5 - d * 0.5
    }
}

/// Inverted smooth edge (0 → 1 as `dist` crosses `edge` going up).
fn smooth_edge_inv(dist: f32, edge: f32) -> f32 {
    let d = dist - edge;
    if d < -1.0 {
        0.0
    } else if d > 1.0 {
        1.0
    } else {
        0.5 + d * 0.5
    }
}

/// Darkening factor near segment boundary angles.
fn boundary_factor(angle: f32, boundaries: &[f32]) -> f32 {
    let gap_half = 2.5; // degrees
    let mut factor = 0.0f32;
    for &b in boundaries {
        let mut d = (angle - b).abs();
        if d > 180.0 {
            d = 360.0 - d;
        }
        if d < gap_half {
            factor = factor.max(1.0 - d / gap_half);
        }
    }
    factor
}

/// Perpendicular distance from a point to a line segment.
fn point_to_seg_dist(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let len_sq = abx * abx + aby * aby;
    if len_sq < 0.0001 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = (((px - ax) * abx + (py - ay) * aby) / len_sq).clamp(0.0, 1.0);
    let proj_x = ax + t * abx;
    let proj_y = ay + t * aby;
    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

/// Project point onto line segment, returning parameter t ∈ [0, 1].
fn project_t(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let len_sq = abx * abx + aby * aby;
    if len_sq < 0.0001 {
        return 0.0;
    }
    ((px - ax) * abx + (py - ay) * aby) / len_sq
}

/// Linear interpolation for a single colour channel.
fn lerp_c(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 * (1.0 - t) + b as f32 * t).clamp(0.0, 255.0) as u8
}
