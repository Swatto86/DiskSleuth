/// Virtualised TreeView widget — the core UI component.
///
/// Only renders rows visible in the viewport, giving O(1) rendering
/// cost regardless of tree size. Uses the flat `visible_rows` list
/// maintained by `AppState`.
///
/// During scanning, reads from the shared `LiveTree` via a read-lock
/// so the user can see the tree populate in real time.

use crate::state::{AppPhase, AppState};
use crate::theme::DiskSleuthTheme;
use disksleuth_core::model::size::format_size;
use disksleuth_core::model::FileTree;
use egui::{Rect, Response, Sense, Ui, Vec2};

/// Height of each row in pixels.
const ROW_HEIGHT: f32 = 24.0;

/// Indentation per depth level in pixels.
const INDENT_PX: f32 = 20.0;

/// Draw the virtualised tree view.
///
/// Returns the response for the outer scroll area so callers can
/// detect interactions (e.g. deselect on background click).
pub fn tree_view(ui: &mut Ui, state: &mut AppState, theme: &DiskSleuthTheme) -> Response {
    // ── Check if we have any tree data ─────────────────────────
    let have_final_tree = state.tree.is_some();
    let have_live_tree = state.live_tree.as_ref().map_or(false, |lt| lt.read().len() > 0);

    if !have_final_tree && !have_live_tree {
        if state.phase == AppPhase::Scanning {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("Scanning… waiting for results")
                        .color(theme.text_muted),
                );
            });
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("No scan results. Select a drive and click Scan.")
                        .color(theme.text_muted),
                );
            });
        }
        return ui.interact(ui.max_rect(), ui.id().with("empty_tree"), Sense::click());
    }

    let is_scanning = state.phase == AppPhase::Scanning;

    // Show scanning indicator above the tree during a live scan.
    if is_scanning {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(
                egui::RichText::new(format!(
                    " Scanning… {} files found",
                    disksleuth_core::model::size::format_count(state.scan_files_found)
                ))
                .color(theme.text_secondary)
                .size(12.0),
            );
        });
        ui.add_space(2.0);
    }

    // ── Render the tree and collect deferred actions ────────────
    // Scoped block so that tree references (including any RwLockReadGuard)
    // are dropped before we mutate state.
    let (toggle_row, new_selection) = {
        // Obtain tree reference inside the block.
        let live_guard;
        let tree: &FileTree;

        if let Some(ref t) = state.tree {
            tree = t;
        } else {
            live_guard = state.live_tree.as_ref().unwrap().read();
            tree = &*live_guard;
        }

        render_tree_rows(ui, state, theme, tree)
    };
    // tree / live_guard dropped here — safe to mutate state.

    // Apply deferred state mutations.
    if let Some(row_idx) = new_selection {
        if row_idx < state.visible_rows.len() {
            state.selected_node = Some(state.visible_rows[row_idx].node_index);
        }
    }
    if let Some(row_idx) = toggle_row {
        state.toggle_expand(row_idx);
    }

    ui.interact(ui.max_rect(), ui.id().with("tree_bg"), Sense::hover())
}

/// Render the virtualised tree rows. Returns (toggle_row, new_selection)
/// indices for deferred state mutation.
fn render_tree_rows(
    ui: &mut Ui,
    state: &AppState,
    theme: &DiskSleuthTheme,
    tree: &FileTree,
) -> (Option<usize>, Option<usize>) {
    let total_rows = state.visible_rows.len();
    let total_height = total_rows as f32 * ROW_HEIGHT;

    let mut toggle_row: Option<usize> = None;
    let mut new_selection: Option<usize> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Reserve the full virtual height so the scrollbar is correct.
            let (response, mut painter) = ui.allocate_painter(
                Vec2::new(ui.available_width(), total_height),
                Sense::click(),
            );

            let viewport = ui.clip_rect();
            let top_y = response.rect.top();

            // Determine visible range.
            let first_visible =
                ((viewport.top() - top_y) / ROW_HEIGHT).floor().max(0.0) as usize;
            let last_visible = ((viewport.bottom() - top_y) / ROW_HEIGHT)
                .ceil()
                .min(total_rows as f32) as usize;

            // Render only visible rows.
            for row_idx in first_visible..last_visible {
                if row_idx >= state.visible_rows.len() {
                    break;
                }

                let row = &state.visible_rows[row_idx];
                let node = tree.node(row.node_index);

                let row_rect = Rect::from_min_size(
                    egui::pos2(response.rect.left(), top_y + row_idx as f32 * ROW_HEIGHT),
                    Vec2::new(response.rect.width(), ROW_HEIGHT),
                );

                // Skip if completely outside viewport.
                if !viewport.intersects(row_rect) {
                    continue;
                }

                // Selection highlight.
                let is_selected = state.selected_node == Some(row.node_index);
                if is_selected {
                    painter.rect_filled(row_rect, 0.0, theme.selection);
                }

                // Hover highlight.
                let row_response = ui.interact(
                    row_rect,
                    ui.id().with(("tree_row", row_idx)),
                    Sense::click(),
                );

                if row_response.hovered() && !is_selected {
                    painter.rect_filled(row_rect, 0.0, theme.surface_hover);
                }

                // Click handling.
                if row_response.clicked() {
                    new_selection = Some(row_idx);
                }
                if row_response.double_clicked() && node.is_dir {
                    toggle_row = Some(row_idx);
                }

                // Context menu.
                row_response.context_menu(|ui| {
                    context_menu(ui, state, row.node_index);
                });

                // Tooltip with full name when hovered (useful for truncated names).
                if row_response.hovered() {
                    let tip_text = if node.is_error {
                        format!("{}\n⚠ Access denied", node.name)
                    } else {
                        format!(
                            "{}\n{} — {:.1}%",
                            node.name,
                            format_size(node.size),
                            node.percent_of_parent,
                        )
                    };
                    egui::show_tooltip_at_pointer(
                        ui.ctx(),
                        ui.layer_id(),
                        ui.id().with(("tree_tip", row_idx)),
                        |ui| {
                            ui.label(tip_text);
                        },
                    );
                }

                // Draw row content.
                let indent = INDENT_PX * row.depth as f32;
                let text_x = row_rect.left() + indent + 4.0;
                let text_y = row_rect.center().y;

                // Expand/collapse arrow for directories.
                if node.is_dir {
                    let arrow_text = if row.is_expanded { "▼" } else { "▶" };
                    let arrow_rect = Rect::from_min_size(
                        egui::pos2(row_rect.left() + indent - 14.0, row_rect.top()),
                        Vec2::new(16.0, ROW_HEIGHT),
                    );
                    let arrow_response = ui.interact(
                        arrow_rect,
                        ui.id().with(("arrow", row_idx)),
                        Sense::click(),
                    );
                    if arrow_response.clicked() {
                        toggle_row = Some(row_idx);
                    }
                    painter.text(
                        egui::pos2(row_rect.left() + indent - 12.0, text_y),
                        egui::Align2::LEFT_CENTER,
                        arrow_text,
                        egui::FontId::proportional(11.0),
                        theme.text_muted,
                    );
                }

                // Icon — error nodes get a warning icon.
                let (icon, icon_color) = if node.is_error {
                    ("⚠", theme.warning)
                } else if node.is_dir {
                    ("📁", theme.folder_icon)
                } else {
                    ("📄", theme.file_icon)
                };
                painter.text(
                    egui::pos2(text_x, text_y),
                    egui::Align2::LEFT_CENTER,
                    icon,
                    egui::FontId::proportional(13.0),
                    icon_color,
                );

                // File/directory name — rendered with proper text clipping.
                // Error nodes display in muted/warning colour.
                let name_x = text_x + 20.0;
                let right_area_start = row_rect.right() - 300.0;
                let max_name_w = (right_area_start - name_x - 4.0).max(20.0);
                let name_str = node.name.as_str();

                let name_font = egui::FontId::proportional(13.0);
                let name_color = if node.is_error {
                    theme.text_muted
                } else {
                    theme.text_primary
                };
                let name_galley = painter.layout_no_wrap(
                    name_str.to_string(),
                    name_font,
                    name_color,
                );

                // If the text fits, draw it directly. Otherwise, clip and add ellipsis.
                let text_width = name_galley.size().x;
                if text_width <= max_name_w {
                    painter.galley(
                        egui::pos2(name_x, text_y - name_galley.size().y / 2.0),
                        name_galley,
                        name_color,
                    );
                } else {
                    // Clip the name galley to the available width.
                    let clip = Rect::from_min_size(
                        egui::pos2(name_x, row_rect.top()),
                        Vec2::new(max_name_w - 12.0, ROW_HEIGHT),
                    );
                    let prev_clip = painter.clip_rect();
                    painter.set_clip_rect(prev_clip.intersect(clip));
                    painter.galley(
                        egui::pos2(name_x, text_y - name_galley.size().y / 2.0),
                        name_galley,
                        name_color,
                    );
                    painter.set_clip_rect(prev_clip);

                    // Draw ellipsis after the clipped text.
                    painter.text(
                        egui::pos2(name_x + max_name_w - 12.0, text_y),
                        egui::Align2::LEFT_CENTER,
                        "…",
                        egui::FontId::proportional(13.0),
                        theme.text_muted,
                    );
                }

                // Size (right-aligned area).
                let size_text = format_size(node.size);

                painter.text(
                    egui::pos2(right_area_start, text_y),
                    egui::Align2::LEFT_CENTER,
                    &size_text,
                    egui::FontId::proportional(12.0),
                    theme.text_secondary,
                );

                // Percentage.
                let pct_text = format!("{:.1}%", node.percent_of_parent);
                painter.text(
                    egui::pos2(right_area_start + 80.0, text_y),
                    egui::Align2::LEFT_CENTER,
                    &pct_text,
                    egui::FontId::proportional(12.0),
                    theme.text_muted,
                );

                // Size bar.
                let bar_x = right_area_start + 130.0;
                let bar_width = 100.0;
                let bar_height = 10.0;
                let bar_rect = Rect::from_min_size(
                    egui::pos2(bar_x, text_y - bar_height / 2.0),
                    Vec2::new(bar_width, bar_height),
                );

                // Bar background.
                painter.rect_filled(bar_rect, 2.0, theme.surface);

                // Bar fill.
                let fill_w = bar_width * (node.percent_of_parent / 100.0).clamp(0.0, 1.0);
                if fill_w > 0.5 {
                    let fill_rect = Rect::from_min_size(bar_rect.min, Vec2::new(fill_w, bar_height));
                    let bar_color = theme.bar_color(node.percent_of_parent);
                    painter.rect_filled(fill_rect, 2.0, bar_color);
                }

                // File count for directories.
                if node.is_dir && node.descendant_count > 0 {
                    let count_text = format!("{} files", disksleuth_core::model::size::format_count(node.descendant_count));
                    painter.text(
                        egui::pos2(bar_x + bar_width + 10.0, text_y),
                        egui::Align2::LEFT_CENTER,
                        &count_text,
                        egui::FontId::proportional(11.0),
                        theme.text_muted,
                    );
                }
            }

            response
        })
        .inner;

    (toggle_row, new_selection)
}

/// Right-click context menu for a tree node.
fn context_menu(ui: &mut Ui, state: &AppState, node_index: disksleuth_core::model::NodeIndex) {
    // Get tree reference — final tree first, then live tree.
    let live_guard;
    let tree: &FileTree;

    if let Some(ref t) = state.tree {
        tree = t;
    } else if let Some(ref lt) = state.live_tree {
        live_guard = lt.read();
        tree = &*live_guard;
    } else {
        return;
    };

    let full_path = tree.full_path(node_index);
    let node = tree.node(node_index);

    if ui.button("📂 Open in Explorer").clicked() {
        let path = if node.is_dir {
            full_path.clone()
        } else {
            // Open the parent directory with the file selected.
            std::path::Path::new(&full_path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or(full_path.clone())
        };
        let _ = std::process::Command::new("explorer.exe")
            .arg(if node.is_dir {
                path
            } else {
                format!("/select,{}", full_path)
            })
            .spawn();
        ui.close_menu();
    }

    if ui.button("📋 Copy Path").clicked() {
        ui.ctx().copy_text(full_path);
        ui.close_menu();
    }

    ui.separator();

    ui.label(format!("Size: {}", format_size(node.size)));
    if node.is_dir {
        ui.label(format!(
            "Files: {}",
            disksleuth_core::model::size::format_count(node.descendant_count)
        ));
    }
}
