//! Build script -- generates the application icon and embeds Windows
//! application manifest and icon resource.

fn main() {
    // Generate icon if it does not already exist.
    let icon_path = "assets/icon.ico";
    if !std::path::Path::new(icon_path).exists() {
        std::fs::create_dir_all("assets").ok();
        match generate_ico(&[48, 32, 16]) {
            Ok(data) => {
                if let Err(e) = std::fs::write(icon_path, &data) {
                    eprintln!("cargo:warning=Failed to write icon: {e}");
                }
            }
            Err(e) => eprintln!("cargo:warning=Failed to generate icon: {e}"),
        }
    }

    // Only embed resources on Windows.
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_manifest(
            r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#,
        );

        if std::path::Path::new(icon_path).exists() {
            res.set_icon(icon_path);
        }

        if let Err(e) = res.compile() {
            eprintln!("cargo:warning=Failed to compile Windows resources: {e}");
        }
    }
}

// ════════════════════════════════════════════════════════════════
// Icon rendering (self-contained — no workspace crate deps)
// ════════════════════════════════════════════════════════════════

/// Produce a multi-resolution ICO file as bytes.
fn generate_ico(sizes: &[u32]) -> Result<Vec<u8>, String> {
    let mut ico: Vec<u8> = Vec::new();

    // ICO header.
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type = ICO
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());

    // Pre-render all images.
    let images: Vec<(u32, Vec<u8>)> = sizes
        .iter()
        .map(|&sz| {
            let rgba = render_icon_rgba(sz);
            let bmp = rgba_to_ico_bmp(&rgba, sz);
            (sz, bmp)
        })
        .collect();

    // Directory entries.
    let header_len = 6 + 16 * sizes.len();
    let mut offset = header_len;
    for (sz, bmp) in &images {
        let w = if *sz >= 256 { 0u8 } else { *sz as u8 };
        let h = w;
        ico.push(w);
        ico.push(h);
        ico.push(0); // colour count
        ico.push(0); // reserved
        ico.extend_from_slice(&1u16.to_le_bytes()); // planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bpp
        ico.extend_from_slice(&(bmp.len() as u32).to_le_bytes());
        ico.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += bmp.len();
    }

    // Image data.
    for (_, bmp) in &images {
        ico.extend_from_slice(bmp);
    }

    Ok(ico)
}

/// Convert top-to-bottom RGBA pixels into a BMP blob for an ICO entry.
fn rgba_to_ico_bmp(rgba: &[u8], size: u32) -> Vec<u8> {
    let mut bmp: Vec<u8> = Vec::new();

    // BITMAPINFOHEADER (40 bytes).
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&(size as i32).to_le_bytes());
    bmp.extend_from_slice(&((size as i32) * 2).to_le_bytes()); // doubled for ICO
    bmp.extend_from_slice(&1u16.to_le_bytes()); // planes
    bmp.extend_from_slice(&32u16.to_le_bytes()); // bpp
    bmp.extend_from_slice(&0u32.to_le_bytes()); // compression
    bmp.extend_from_slice(&0u32.to_le_bytes()); // image size
    bmp.extend_from_slice(&0i32.to_le_bytes()); // x ppm
    bmp.extend_from_slice(&0i32.to_le_bytes()); // y ppm
    bmp.extend_from_slice(&0u32.to_le_bytes()); // colours used
    bmp.extend_from_slice(&0u32.to_le_bytes()); // important colours

    // Pixel data — bottom-to-top, BGRA.
    for y in (0..size).rev() {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let r = rgba[idx];
            let g = rgba[idx + 1];
            let b = rgba[idx + 2];
            let a = rgba[idx + 3];
            bmp.push(b);
            bmp.push(g);
            bmp.push(r);
            bmp.push(a);
        }
    }

    // AND mask (1 bpp, bottom-to-top, rows padded to 4-byte boundary).
    let row_bytes = size.div_ceil(32) * 4;
    for y in (0..size).rev() {
        let mut row = vec![0u8; row_bytes as usize];
        for x in 0..size {
            let alpha = rgba[((y * size + x) * 4 + 3) as usize];
            if alpha < 128 {
                let byte_idx = (x / 8) as usize;
                let bit_idx = 7 - (x % 8);
                row[byte_idx] |= 1 << bit_idx;
            }
        }
        bmp.extend_from_slice(&row);
    }

    bmp
}

/// Render the DiskSleuth icon as top-to-bottom RGBA pixels.
///
/// This is a self-contained copy of the algorithm in
/// `crates/disksleuth-gui/src/icon.rs` so the build script
/// has no dependency on workspace crates.
fn render_icon_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut px_buf = vec![0u8; (size * size * 4) as usize];

    let cx = s * 0.42;
    let cy = s * 0.42;
    let radius = s * 0.34;
    let ring_width = s * 0.045;
    let ring_outer = radius + ring_width;

    let angle_45: f32 = std::f32::consts::FRAC_PI_4;
    let h_start_x = cx + ring_outer * angle_45.cos();
    let h_start_y = cy + ring_outer * angle_45.sin();
    let h_end_x = s * 0.91;
    let h_end_y = s * 0.91;
    let h_width_start = s * 0.055;
    let h_width_end = s * 0.075;

    let segments: &[(f32, f32, [u8; 3])] = &[
        (0.0, 144.0, [0x89, 0xb4, 0xfa]),
        (144.0, 245.0, [0xa6, 0xe3, 0xa1]),
        (245.0, 314.0, [0xf9, 0xe2, 0xaf]),
        (314.0, 360.0, [0xf3, 0x8b, 0xa8]),
    ];
    let boundaries: Vec<f32> = segments.iter().map(|seg| seg.0).collect();

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

            // Pie-chart circle.
            if dist < radius + 1.5 {
                let edge_aa = ico_smooth_edge(dist, radius);
                let angle_deg = dy.atan2(dx).to_degrees();
                let angle_deg = if angle_deg < 0.0 {
                    angle_deg + 360.0
                } else {
                    angle_deg
                };

                let mut seg_col = segments[0].2;
                for &(start, end, col) in segments {
                    if angle_deg >= start && angle_deg < end {
                        seg_col = col;
                        break;
                    }
                }

                let bd = ico_boundary_factor(angle_deg, &boundaries);
                cr = (seg_col[0] as f32 * (1.0 - 0.35 * bd)) as u8;
                cg = (seg_col[1] as f32 * (1.0 - 0.35 * bd)) as u8;
                cb = (seg_col[2] as f32 * (1.0 - 0.35 * bd)) as u8;
                ca = edge_aa;

                let shade = 1.0 - 0.12 * (dist / radius);
                cr = (cr as f32 * shade).min(255.0) as u8;
                cg = (cg as f32 * shade).min(255.0) as u8;
                cb = (cb as f32 * shade).min(255.0) as u8;

                let hx = dx + radius * 0.30;
                let hy = dy + radius * 0.30;
                let h_dist = (hx * hx + hy * hy).sqrt();
                let highlight = (1.0 - h_dist / (radius * 0.65)).max(0.0) * 0.18;
                cr = (cr as f32 + highlight * 255.0).min(255.0) as u8;
                cg = (cg as f32 + highlight * 255.0).min(255.0) as u8;
                cb = (cb as f32 + highlight * 255.0).min(255.0) as u8;
            }

            // Ring.
            let ring_inner = radius - 0.5;
            if dist > ring_inner && dist < ring_outer + 1.5 {
                let aa_in = ico_smooth_edge_inv(dist, ring_inner);
                let aa_out = ico_smooth_edge(dist, ring_outer);
                let ring_alpha = aa_in * aa_out;
                let grad = 0.5 + 0.5 * (1.0 - (dy / radius).clamp(-1.0, 1.0)) * 0.5;
                let rr = (0x70 as f32 * grad).min(255.0) as u8;
                let rg = (0x78 as f32 * grad).min(255.0) as u8;
                let rb = (0x85 as f32 * grad).min(255.0) as u8;
                cr = ico_lerp(cr, rr, ring_alpha);
                cg = ico_lerp(cg, rg, ring_alpha);
                cb = ico_lerp(cb, rb, ring_alpha);
                ca = ca + (1.0 - ca) * ring_alpha;
            }

            // Handle.
            let ld = ico_pt_seg_dist(px, py, h_start_x, h_start_y, h_end_x, h_end_y);
            let t = ico_project_t(px, py, h_start_x, h_start_y, h_end_x, h_end_y);
            if t > -0.05 && t < 1.05 {
                let half_w = h_width_start + (h_width_end - h_width_start) * t.clamp(0.0, 1.0);
                if ld < half_w + 1.5 {
                    let haa = ico_smooth_edge(ld, half_w);
                    let tt = t.clamp(0.0, 1.0);
                    let hr = ico_lerp(0x78, 0x50, tt);
                    let hg_c = ico_lerp(0x7d, 0x55, tt);
                    let hb = ico_lerp(0x88, 0x60, tt);
                    cr = ico_lerp(cr, hr, haa);
                    cg = ico_lerp(cg, hg_c, haa);
                    cb = ico_lerp(cb, hb, haa);
                    ca = ca + (1.0 - ca) * haa;
                }
            }

            let idx = ((y * size + x) * 4) as usize;
            px_buf[idx] = cr;
            px_buf[idx + 1] = cg;
            px_buf[idx + 2] = cb;
            px_buf[idx + 3] = (ca * 255.0).clamp(0.0, 255.0) as u8;
        }
    }

    px_buf
}

fn ico_smooth_edge(dist: f32, edge: f32) -> f32 {
    let d = dist - edge;
    if d < -1.0 {
        1.0
    } else if d > 1.0 {
        0.0
    } else {
        0.5 - d * 0.5
    }
}
fn ico_smooth_edge_inv(dist: f32, edge: f32) -> f32 {
    let d = dist - edge;
    if d < -1.0 {
        0.0
    } else if d > 1.0 {
        1.0
    } else {
        0.5 + d * 0.5
    }
}
fn ico_boundary_factor(angle: f32, boundaries: &[f32]) -> f32 {
    let gap = 2.5;
    let mut f = 0.0f32;
    for &b in boundaries {
        let mut d = (angle - b).abs();
        if d > 180.0 {
            d = 360.0 - d;
        }
        if d < gap {
            f = f.max(1.0 - d / gap);
        }
    }
    f
}
fn ico_pt_seg_dist(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let len_sq = abx * abx + aby * aby;
    if len_sq < 0.0001 {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = (((px - ax) * abx + (py - ay) * aby) / len_sq).clamp(0.0, 1.0);
    ((px - (ax + t * abx)).powi(2) + (py - (ay + t * aby)).powi(2)).sqrt()
}
fn ico_project_t(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let len_sq = abx * abx + aby * aby;
    if len_sq < 0.0001 {
        return 0.0;
    }
    ((px - ax) * abx + (py - ay) * aby) / len_sq
}
fn ico_lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 * (1.0 - t) + b as f32 * t).clamp(0.0, 255.0) as u8
}
