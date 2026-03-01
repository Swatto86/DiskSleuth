/// Drive selection panel widget.
///
/// Displays all available drives with usage bars and allows
/// the user to select which drive to scan.
use crate::state::AppState;
use egui::{Rect, Sense, Ui, Vec2};

/// Draw the drive picker panel.
pub fn drive_picker(ui: &mut Ui, state: &mut AppState) {
    ui.heading("Drives");
    ui.add_space(4.0);

    // Derive card colours from the active theme so the widget looks correct
    // in both dark and light mode.
    let is_dark = ui.visuals().dark_mode;
    let card_bg = if is_dark {
        egui::Color32::from_rgb(0x28, 0x3a, 0x5c)
    } else {
        egui::Color32::from_rgb(0xd0, 0xdc, 0xf0)
    };
    let card_bg_selected = if is_dark {
        egui::Color32::from_rgb(0x36, 0x50, 0x78)
    } else {
        egui::Color32::from_rgb(0xa8, 0xc0, 0xe8)
    };
    let card_bg_hover = if is_dark {
        egui::Color32::from_rgb(0x32, 0x48, 0x6e)
    } else {
        egui::Color32::from_rgb(0xbc, 0xd0, 0xec)
    };
    let border_color = if is_dark {
        egui::Color32::from_rgb(0x3a, 0x50, 0x72)
    } else {
        egui::Color32::from_rgb(0x90, 0xa8, 0xcc)
    };
    let bar_track_bg = if is_dark {
        egui::Color32::from_rgb(0x1e, 0x1e, 0x2e)
    } else {
        egui::Color32::from_rgb(0xb0, 0xbc, 0xd8)
    };
    // Text on cards: white on dark cards, near-black on light cards.
    let card_text = ui.visuals().text_color();

    let mut new_selection = state.selected_drive_index;

    for (i, drive) in state.drives.iter().enumerate() {
        let is_selected = state.selected_drive_index == Some(i);

        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::click());

        if response.clicked() {
            new_selection = Some(i);
        }

        let painter = ui.painter_at(rect);

        // Background — colour adapts to theme and selection state.
        let bg = if is_selected {
            card_bg_selected
        } else if response.hovered() {
            card_bg_hover
        } else {
            card_bg
        };
        painter.rect_filled(rect, 4.0, bg);

        // Subtle border.
        painter.rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.0, border_color),
            egui::StrokeKind::Outside,
        );

        // Drive letter and label.
        let label = if drive.label.is_empty() {
            drive.letter.clone()
        } else {
            format!("{} ({})", drive.letter, drive.label)
        };

        painter.text(
            egui::pos2(rect.left() + 8.0, rect.top() + 12.0),
            egui::Align2::LEFT_CENTER,
            &label,
            egui::FontId::proportional(13.0),
            card_text,
        );

        // Filesystem and type.
        let info = format!("{} · {}", drive.filesystem, drive.drive_type.label());
        painter.text(
            egui::pos2(rect.right() - 8.0, rect.top() + 12.0),
            egui::Align2::RIGHT_CENTER,
            &info,
            egui::FontId::proportional(11.0),
            card_text,
        );

        // Usage bar.
        let bar_y = rect.top() + 26.0;
        let bar_width = rect.width() - 16.0;
        let bar_height = 8.0;
        let bar_rect = Rect::from_min_size(
            egui::pos2(rect.left() + 8.0, bar_y),
            Vec2::new(bar_width, bar_height),
        );
        painter.rect_filled(bar_rect, 3.0, bar_track_bg);

        let fill_width = bar_width * (drive.usage_percent / 100.0).clamp(0.0, 1.0);
        if fill_width > 0.5 {
            let fill_rect = Rect::from_min_size(bar_rect.min, Vec2::new(fill_width, bar_height));
            let bar_color = drive_bar_color(drive.usage_percent);
            painter.rect_filled(fill_rect, 3.0, bar_color);
        }

        // Size summary — "X used · Y free" is unambiguous where "X / Y free" is not.
        let size_text = format!(
            "{} used \u{00b7} {} free",
            drive.used_display, drive.free_display
        );
        painter.text(
            egui::pos2(rect.left() + 8.0, bar_y + bar_height + 10.0),
            egui::Align2::LEFT_CENTER,
            &size_text,
            egui::FontId::proportional(11.0),
            card_text,
        );

        ui.add_space(2.0);
    }

    state.selected_drive_index = new_selection;
}

fn drive_bar_color(percent: f32) -> egui::Color32 {
    let t = (percent / 100.0).clamp(0.0, 1.0);
    let r = (0xa6_u8 as f32 + (0xf3_u8 as f32 - 0xa6_u8 as f32) * t) as u8;
    let g = (0xe3_u8 as f32 + (0x8b_u8 as f32 - 0xe3_u8 as f32) * t) as u8;
    let b = (0xa1_u8 as f32 + (0xa8_u8 as f32 - 0xa1_u8 as f32) * t) as u8;
    egui::Color32::from_rgb(r, g, b)
}
