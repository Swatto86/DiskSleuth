/// Drive selection panel widget.
///
/// Displays all available drives with usage bars and allows
/// the user to select which drive to scan.

use crate::state::AppState;
use crate::theme::DiskSleuthTheme;
use egui::{Rect, Sense, Ui, Vec2};

/// Draw the drive picker panel.
pub fn drive_picker(ui: &mut Ui, state: &mut AppState, theme: &DiskSleuthTheme) {
    ui.heading("Drives");
    ui.add_space(4.0);

    let mut new_selection = state.selected_drive_index;

    for (i, drive) in state.drives.iter().enumerate() {
        let is_selected = state.selected_drive_index == Some(i);

        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 52.0),
            Sense::click(),
        );

        if response.clicked() {
            new_selection = Some(i);
        }

        let painter = ui.painter_at(rect);

        // Background — tinted with the accent blue so the card matches
        // the DiskSleuth title colour family.
        let card_bg = if theme.background.r() < 128 {
            // Dark mode: deep navy matching the accent blue hue.
            egui::Color32::from_rgb(0x28, 0x3a, 0x5c)
        } else {
            // Light mode: light blue tint.
            egui::Color32::from_rgb(0xd8, 0xe4, 0xf4)
        };
        let bg = if is_selected {
            if theme.background.r() < 128 {
                egui::Color32::from_rgb(0x36, 0x50, 0x78)
            } else {
                egui::Color32::from_rgb(0xc0, 0xd4, 0xf0)
            }
        } else if response.hovered() {
            if theme.background.r() < 128 {
                egui::Color32::from_rgb(0x32, 0x48, 0x6e)
            } else {
                egui::Color32::from_rgb(0xc8, 0xd8, 0xec)
            }
        } else {
            card_bg
        };
        painter.rect_filled(rect, 4.0, bg);

        // Subtle border for extra definition.
        let border_color = if theme.background.r() < 128 {
            egui::Color32::from_rgb(0x3a, 0x50, 0x72)
        } else {
            egui::Color32::from_rgb(0xb0, 0xc0, 0xd8)
        };
        painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, border_color), egui::StrokeKind::Outside);

        // Drive letter and label.
        let label = if drive.label.is_empty() {
            drive.letter.clone()
        } else {
            format!("{} ({})", drive.letter, drive.label)
        };

        // Drive letter and label — use explicit high-contrast white in dark mode
        // since theme.text_primary is a light blue that can be hard to read.
        let card_text = if theme.background.r() < 128 {
            egui::Color32::WHITE
        } else {
            theme.text_primary
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
        painter.rect_filled(bar_rect, 3.0, theme.background);

        let fill_width = bar_width * (drive.usage_percent / 100.0).clamp(0.0, 1.0);
        if fill_width > 0.5 {
            let fill_rect =
                Rect::from_min_size(bar_rect.min, Vec2::new(fill_width, bar_height));
            let bar_color = theme.bar_color(drive.usage_percent);
            painter.rect_filled(fill_rect, 3.0, bar_color);
        }

        // Size text.
        let size_text = format!(
            "{} / {} free",
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
