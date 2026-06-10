/// Modal confirmation dialog shown before a Recycle Bin deletion.
///
/// Enter confirms, Escape cancels. The actual deletion (shell call + tree
/// update) lives in `AppState::confirm_pending_delete`.
use crate::state::AppState;
use disksleuth_core::model::size::{format_count, format_size};

pub fn delete_dialog(ctx: &egui::Context, state: &mut AppState) {
    let Some(target) = state.pending_delete else {
        return;
    };
    // The pending node must still be valid on the completed tree.
    let Some(ref tree) = state.tree else {
        state.pending_delete = None;
        return;
    };
    if target.idx() >= tree.len() || tree.node(target).is_deleted {
        state.pending_delete = None;
        return;
    }

    let node = tree.node(target);
    let name = node.name.to_string();
    let path = tree.full_path(target);
    let size = node.size;
    let is_dir = node.is_dir;
    let file_count = node.descendant_count;

    let mut confirm = false;
    let mut cancel = false;

    egui::Window::new("🗑 Move to Recycle Bin?")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([380.0, 0.0])
        .show(ctx, |ui| {
            let muted = ui.visuals().weak_text_color();

            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("{} {name}", if is_dir { "📁" } else { "📄" }))
                    .size(14.0)
                    .strong(),
            );
            ui.label(egui::RichText::new(&path).size(11.0).color(muted));
            ui.add_space(6.0);

            let what = if is_dir {
                format!(
                    "This folder and all {} files inside it ({}) will be moved to the Recycle Bin.",
                    format_count(file_count),
                    format_size(size)
                )
            } else {
                format!(
                    "This file ({}) will be moved to the Recycle Bin.",
                    format_size(size)
                )
            };
            ui.label(egui::RichText::new(what).size(12.0));
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                if ui.button("Cancel  (Esc)").clicked() {
                    cancel = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let delete_btn = egui::Button::new(
                        egui::RichText::new("🗑 Move to Recycle Bin  (Enter)")
                            .color(egui::Color32::WHITE),
                    )
                    .fill(egui::Color32::from_rgb(0xd0, 0x45, 0x60));
                    if ui.add(delete_btn).clicked() {
                        confirm = true;
                    }
                });
            });
            ui.add_space(2.0);
        });

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        cancel = true;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
        confirm = true;
    }

    if cancel {
        state.pending_delete = None;
    } else if confirm {
        state.confirm_pending_delete();
    }
}
