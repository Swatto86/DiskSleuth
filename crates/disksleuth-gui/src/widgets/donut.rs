/// Donut (ring) chart widget for proportional breakdowns.
///
/// egui has no arc primitive, so each segment is tessellated into small
/// annular quads (convex polygons), which renders correctly at any sweep
/// angle. Hover is resolved analytically from radius + angle, highlighting
/// the segment and showing a tooltip.
use disksleuth_core::model::size::format_size;
use egui::{Color32, Pos2, Sense, Ui, Vec2};
use std::f32::consts::TAU;

/// One slice of the donut.
pub struct DonutSegment {
    pub label: &'static str,
    pub value: u64,
    pub color: Color32,
}

/// Angular size of each tessellation step (radians). ~5° keeps the ring
/// smooth while bounding the polygon count to ~72 quads per full circle.
const STEP: f32 = 0.09;

/// Draw a donut chart of `segments` with the formatted total in the centre.
///
/// Returns the index of the hovered segment, if any, so callers can sync
/// other UI (e.g. highlight a legend row).
pub fn donut_chart(ui: &mut Ui, segments: &[DonutSegment], diameter: f32) -> Option<usize> {
    let total: u64 = segments.iter().map(|s| s.value).sum();
    if total == 0 {
        return None;
    }

    let (rect, _response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), diameter), Sense::hover());
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let r_outer = (diameter / 2.0 - 4.0).max(10.0);
    let r_inner = r_outer * 0.62;

    // Resolve the hovered segment first so it can be drawn enlarged.
    let hovered = ui
        .input(|i| i.pointer.hover_pos())
        .filter(|pos| {
            let d = (*pos - center).length();
            d >= r_inner && d <= r_outer + 3.0
        })
        .map(|pos| {
            let v = pos - center;
            // Angle from 12 o'clock, clockwise, in [0, TAU).
            (v.x.atan2(-v.y)).rem_euclid(TAU)
        })
        .and_then(|angle| {
            let mut start = 0.0f32;
            for (i, seg) in segments.iter().enumerate() {
                let sweep = seg.value as f32 / total as f32 * TAU;
                if angle >= start && angle < start + sweep {
                    return Some(i);
                }
                start += sweep;
            }
            None
        });

    // Draw the segments.
    let mut start = 0.0f32;
    for (i, seg) in segments.iter().enumerate() {
        let sweep = seg.value as f32 / total as f32 * TAU;
        let (r_in, r_out) = if hovered == Some(i) {
            (r_inner - 2.0, r_outer + 3.0)
        } else {
            (r_inner, r_outer)
        };

        // Tessellate the annular sector into quads.
        let mut a = start;
        let end = start + sweep;
        while a < end {
            let b = (a + STEP).min(end);
            let quad = vec![
                polar(center, r_in, a),
                polar(center, r_out, a),
                polar(center, r_out, b),
                polar(center, r_in, b),
            ];
            painter.add(egui::Shape::convex_polygon(
                quad,
                seg.color,
                egui::Stroke::NONE,
            ));
            a = b;
        }
        start = end;
    }

    // Centre label: total size.
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        format_size(total),
        egui::FontId::proportional(13.0),
        ui.visuals().strong_text_color(),
    );

    // Tooltip for the hovered segment.
    if let Some(i) = hovered {
        let seg = &segments[i];
        let pct = seg.value as f64 / total as f64 * 100.0;
        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), ui.id().with("donut_tip"), |ui| {
            ui.label(egui::RichText::new(seg.label).strong());
            ui.label(format!("{} ({pct:.1}%)", format_size(seg.value)));
        });
    }

    hovered
}

/// Point at `radius` from `center`, `angle` measured clockwise from 12 o'clock.
#[inline]
fn polar(center: Pos2, radius: f32, angle: f32) -> Pos2 {
    center + Vec2::new(angle.sin(), -angle.cos()) * radius
}
