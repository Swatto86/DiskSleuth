/// Floating window listing the largest individual files of the scan.
///
/// Clicking a path selects the file and reveals it in both the tree view
/// and the treemap (selection sync).
use crate::state::AppState;
use disksleuth_core::model::size::format_size;
use disksleuth_core::model::NodeIndex;

/// How many of the pre-computed largest files to display.
const SHOW_TOP_N: usize = 100;

pub fn largest_files_window(ctx: &egui::Context, state: &mut AppState) {
    if !state.show_largest_window {
        return;
    }
    let mut open = true;
    let mut select: Option<NodeIndex> = None;

    egui::Window::new("📊 Largest Files")
        .open(&mut open)
        .default_size([640.0, 420.0])
        .resizable(true)
        .show(ctx, |ui| {
            let Some(ref tree) = state.tree else {
                ui.label("Run a scan first.");
                return;
            };
            if tree.largest_files.is_empty() {
                ui.label("No files found in this scan.");
                return;
            }

            let muted = ui.visuals().weak_text_color();
            ui.label(
                egui::RichText::new("Click a path to locate it in the tree and treemap.")
                    .size(11.0)
                    .color(muted),
            );
            ui.add_space(4.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("largest_grid")
                        .num_columns(3)
                        .striped(true)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            for (rank, &idx) in
                                tree.largest_files.iter().take(SHOW_TOP_N).enumerate()
                            {
                                let node = tree.node(idx);
                                ui.label(
                                    egui::RichText::new(format!("{}", rank + 1))
                                        .size(11.0)
                                        .color(muted),
                                );
                                ui.label(
                                    egui::RichText::new(format_size(node.size))
                                        .strong()
                                        .size(12.0),
                                );
                                if ui.link(tree.full_path(idx)).clicked() {
                                    select = Some(idx);
                                }
                                ui.end_row();
                            }
                        });
                });
        });

    if let Some(idx) = select {
        state.selected_node = Some(idx);
        state.reveal_node_in_tree(idx);
    }
    state.show_largest_window = open;
}
