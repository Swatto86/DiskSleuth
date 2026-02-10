/// Treemap widget — SpaceSniffer-style nested, interactive disk usage visualisation.
///
/// Uses squarified treemap layout (Bruls, Huizing, van Wijk) where
/// rectangle areas are proportional to file/directory sizes. Directories
/// show a header bar with their name, and their children are recursively
/// laid out inside — creating nested, hierarchical boxes just like SpaceSniffer.
///
/// **Navigation**:
/// - Click a directory to drill in (becomes the new root).
/// - Click a file to open Explorer at that location.
/// - Back/Forward/Up buttons and breadcrumb for navigation history.
///
/// **Labels**: Shown when the rectangle is large enough.
/// **Hover**: Tooltip with name, size, percentage, type.

use crate::state::AppState;
use crate::theme::DiskSleuthTheme;
use disksleuth_core::model::size::format_size;
use disksleuth_core::model::{FileTree, NodeIndex};
use egui::{Color32, Rect, Sense, Ui, Vec2};

/// Maximum recursion depth for nested layout.
const MAX_NEST_DEPTH: usize = 6;

/// Minimum rectangle area (px²) to bother drawing.
const MIN_RECT_AREA: f32 = 24.0;

/// Height of the directory header bar.
const HEADER_H: f32 = 16.0;

/// Padding inside directory frames.
const FRAME_PAD: f32 = 1.5;

/// Minimum rectangle dimension to show a label.
const MIN_LABEL_W: f32 = 40.0;
const MIN_LABEL_H: f32 = 16.0;

/// Colour palette for top-level categories.
const PALETTE: &[(u8, u8, u8)] = &[
    (0x89, 0xb4, 0xfa), // blue
    (0xa6, 0xe3, 0xa1), // green
    (0xf9, 0xe2, 0xaf), // yellow
    (0xf3, 0x8b, 0xa8), // pink
    (0xfa, 0xb3, 0x87), // peach
    (0xcb, 0xa6, 0xf7), // mauve
    (0x94, 0xe2, 0xd5), // teal
    (0xf5, 0xc2, 0xe7), // flamingo
    (0x74, 0xc7, 0xec), // sapphire
    (0xb4, 0xbe, 0xfe), // lavender
];

/// A rectangle in the treemap ready for rendering and hit testing.
#[derive(Clone)]
struct TreemapRect {
    node_idx: NodeIndex,
    rect: Rect,
    color: Color32,
    name: String,
    size: u64,
    percent: f32,
    is_dir: bool,
    depth: usize,
    /// The header bar rect for directories (click target for drilling in).
    header_rect: Option<Rect>,
}

/// Action returned from the treemap widget.
pub enum TreemapAction {
    /// User clicked a directory — drill into it.
    NavigateDir(NodeIndex),
    /// User clicked a file — open in Explorer.
    OpenFile(String),
    /// User single-clicked an item — select it (sync with tree view).
    SelectNode(NodeIndex),
    /// Navigate back.
    Back,
    /// Navigate forward.
    Forward,
    /// Navigate up to parent.
    Up,
}

/// Draw the treemap widget. Returns an optional action for the caller to handle.
pub fn treemap(
    ui: &mut Ui,
    state: &AppState,
    theme: &DiskSleuthTheme,
) -> Option<TreemapAction> {
    // Obtain tree reference — final tree, then live tree.
    let live_guard;
    let tree: &FileTree;

    if let Some(ref t) = state.tree {
        tree = t;
    } else if let Some(ref lt) = state.live_tree {
        live_guard = lt.read();
        if live_guard.len() == 0 {
            return None;
        }
        tree = &*live_guard;
    } else {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("No scan results. Select a drive and click Scan.")
                    .color(theme.text_muted),
            );
        });
        return None;
    };

    if tree.roots.is_empty() {
        return None;
    }

    // Determine the treemap root.
    let root_node = state.treemap_root.unwrap_or_else(|| tree.roots[0]);

    // Validate the root index is in range.
    if root_node.idx() >= tree.nodes.len() {
        return None;
    }

    let root = &tree.nodes[root_node.idx()];
    let is_light = theme.background.r() > 128;

    let mut action: Option<TreemapAction> = None;

    // ── Navigation toolbar ─────────────────────────────────────────
    ui.horizontal(|ui| {
        let can_back = !state.treemap_back.is_empty();
        let can_forward = !state.treemap_forward.is_empty();
        let can_up = root.parent.is_some();

        if ui
            .add_enabled(can_back, egui::Button::new("◀").min_size(Vec2::new(28.0, 22.0)))
            .on_hover_text("Back")
            .clicked()
        {
            action = Some(TreemapAction::Back);
        }
        if ui
            .add_enabled(can_forward, egui::Button::new("▶").min_size(Vec2::new(28.0, 22.0)))
            .on_hover_text("Forward")
            .clicked()
        {
            action = Some(TreemapAction::Forward);
        }
        if ui
            .add_enabled(can_up, egui::Button::new("▲").min_size(Vec2::new(28.0, 22.0)))
            .on_hover_text("Up to parent")
            .clicked()
        {
            action = Some(TreemapAction::Up);
        }

        ui.add_space(8.0);

        // ── Breadcrumb ─────────────────────────────────────────────
        let mut breadcrumb_chain: Vec<NodeIndex> = Vec::new();
        let mut cursor = root_node;
        loop {
            breadcrumb_chain.push(cursor);
            match tree.nodes[cursor.idx()].parent {
                Some(p) => cursor = p,
                None => break,
            }
        }
        breadcrumb_chain.reverse();

        for (i, &bc_node) in breadcrumb_chain.iter().enumerate() {
            if i > 0 {
                ui.label(
                    egui::RichText::new(" › ")
                        .color(theme.text_muted)
                        .size(12.0),
                );
            }
            let name = &tree.nodes[bc_node.idx()].name;
            let is_current = bc_node == root_node;
            let text = if is_current {
                egui::RichText::new(name.as_str())
                    .color(theme.text_primary)
                    .size(12.0)
                    .strong()
            } else {
                egui::RichText::new(name.as_str())
                    .color(theme.accent)
                    .size(12.0)
            };
            let resp = ui.add(egui::Label::new(text).sense(Sense::click()));
            if resp.clicked() && !is_current {
                action = Some(TreemapAction::NavigateDir(bc_node));
            }
            if resp.hovered() && !is_current {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format_size(root.size))
                    .color(theme.accent)
                    .size(12.0),
            );
        });
    });

    ui.add_space(2.0);

    // ── Allocate the treemap area ──────────────────────────────────
    let available = ui.available_size();
    let map_rect = ui.allocate_rect(
        Rect::from_min_size(ui.cursor().min, available),
        Sense::hover(),
    );
    let bounds = map_rect.rect;

    if bounds.width() < 10.0 || bounds.height() < 10.0 {
        return action;
    }

    let painter = ui.painter_at(bounds);

    // Fill background.
    let bg = if is_light {
        Color32::from_rgb(0xe0, 0xe0, 0xe4)
    } else {
        Color32::from_rgb(0x20, 0x20, 0x24)
    };
    painter.rect_filled(bounds, 0.0, bg);

    // ── Build nested rectangles via recursive squarified layout ────
    let children = tree.children_sorted_by_size(root_node);
    if children.is_empty() || root.size == 0 {
        painter.text(
            bounds.center(),
            egui::Align2::CENTER_CENTER,
            if root.is_dir { "Empty directory" } else { &root.name },
            egui::FontId::proportional(14.0),
            theme.text_muted,
        );
        return action;
    }

    let mut rects: Vec<TreemapRect> = Vec::with_capacity(512);
    layout_nested(tree, &children, root.size, bounds, 0, 0, is_light, &mut rects);

    // Sort by depth ascending so deeper items are drawn (and hit-tested) on top.
    rects.sort_by_key(|r| r.depth);

    // ── Render ─────────────────────────────────────────────────────
    let hover_pos = ui.input(|i| i.pointer.hover_pos());
    let clicked = ui.input(|i| i.pointer.button_clicked(egui::PointerButton::Primary));
    let double_clicked = ui.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary));

    // Highlight the currently selected node from the tree view.
    let selected_node = state.selected_node;

    // Label colors.
    let label_color = if is_light {
        Color32::from_rgb(0x10, 0x10, 0x10)
    } else {
        Color32::WHITE
    };
    let label_color_dim = if is_light {
        Color32::from_rgb(0x30, 0x30, 0x30)
    } else {
        Color32::from_rgb(0xd0, 0xd0, 0xd0)
    };

    // Draw all rects.
    for tr in &rects {
        let fill = tr.color;

        if tr.is_dir {
            // Directory: draw frame background + header bar.
            let frame_bg = if is_light {
                darken(fill, 0.08)
            } else {
                darken(fill, 0.35)
            };
            painter.rect_filled(tr.rect, 0.0, frame_bg);

            // Header bar.
            if let Some(hdr) = tr.header_rect {
                let hdr_fill = if is_light {
                    darken(fill, 0.15)
                } else {
                    darken(fill, 0.20)
                };
                painter.rect_filled(hdr, 0.0, hdr_fill);

                // Header label.
                if hdr.width() >= MIN_LABEL_W {
                    let max_chars = ((hdr.width() - 8.0) / 6.0) as usize;
                    let display = truncate_name(&tr.name, max_chars);
                    painter.text(
                        hdr.left_top() + Vec2::new(3.0, 1.0),
                        egui::Align2::LEFT_TOP,
                        &display,
                        egui::FontId::proportional(10.0),
                        label_color,
                    );

                    // Size in header if there's room.
                    let size_text = format_size(tr.size);
                    let name_approx_w = display.len() as f32 * 6.0 + 8.0;
                    if hdr.width() > name_approx_w + size_text.len() as f32 * 6.0 + 8.0 {
                        painter.text(
                            hdr.right_top() + Vec2::new(-3.0, 1.0),
                            egui::Align2::RIGHT_TOP,
                            &size_text,
                            egui::FontId::proportional(10.0),
                            label_color_dim,
                        );
                    }
                }
            }

            // Frame border.
            let border_color = if is_light {
                Color32::from_rgba_premultiplied(0, 0, 0, 40)
            } else {
                Color32::from_rgba_premultiplied(200, 200, 200, 50)
            };
            painter.rect_stroke(
                tr.rect,
                0.0,
                egui::Stroke::new(1.0, border_color),
                egui::StrokeKind::Inside,
            );
        } else {
            // File: solid fill.
            painter.rect_filled(tr.rect, 0.0, fill);

            // Border.
            let border_color = if is_light {
                Color32::from_rgba_premultiplied(255, 255, 255, 180)
            } else {
                Color32::from_rgba_premultiplied(0, 0, 0, 80)
            };
            painter.rect_stroke(
                tr.rect,
                0.0,
                egui::Stroke::new(0.5, border_color),
                egui::StrokeKind::Inside,
            );

            // File label.
            let w = tr.rect.width();
            let h = tr.rect.height();
            if w >= MIN_LABEL_W && h >= MIN_LABEL_H {
                let max_chars = ((w - 6.0) / 6.0) as usize;
                let display = truncate_name(&tr.name, max_chars);
                painter.text(
                    tr.rect.left_top() + Vec2::new(3.0, 2.0),
                    egui::Align2::LEFT_TOP,
                    &display,
                    egui::FontId::proportional(10.0),
                    label_color,
                );

                if h >= 28.0 {
                    painter.text(
                        tr.rect.left_top() + Vec2::new(3.0, 14.0),
                        egui::Align2::LEFT_TOP,
                        format_size(tr.size),
                        egui::FontId::proportional(9.0),
                        label_color_dim,
                    );
                }
            }
        }
    }

    // ── Hover highlight + tooltip (deepest hovered item wins) ──────
    let mut hovered_rect: Option<&TreemapRect> = None;
    if let Some(pos) = hover_pos {
        if bounds.contains(pos) {
            // Find the deepest rect containing the cursor.
            for tr in rects.iter().rev() {
                // For directories, only consider the header bar as the primary hover target,
                // unless the rect has no children rendered inside it.
                let hit = if tr.is_dir {
                    if let Some(hdr) = tr.header_rect {
                        hdr.contains(pos)
                    } else {
                        tr.rect.contains(pos)
                    }
                } else {
                    tr.rect.contains(pos)
                };
                if hit {
                    hovered_rect = Some(tr);
                    break;
                }
            }

            // If no specific item hit but cursor is in bounds, check for any rect.
            if hovered_rect.is_none() {
                for tr in rects.iter().rev() {
                    if tr.rect.contains(pos) {
                        hovered_rect = Some(tr);
                        break;
                    }
                }
            }
        }
    }

    if let Some(tr) = hovered_rect {
        // Highlight: draw a bright border on the hovered item.
        let highlight_color = if is_light {
            Color32::from_rgb(0x00, 0x60, 0xff)
        } else {
            Color32::from_rgb(0x80, 0xc0, 0xff)
        };
        let highlight_rect = if tr.is_dir {
            tr.header_rect.unwrap_or(tr.rect)
        } else {
            tr.rect
        };
        painter.rect_stroke(
            highlight_rect,
            0.0,
            egui::Stroke::new(2.0, highlight_color),
            egui::StrokeKind::Inside,
        );

        if tr.is_dir {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        egui::show_tooltip_at_pointer(
            ui.ctx(),
            egui::LayerId::new(egui::Order::Tooltip, map_rect.id),
            map_rect.id.with("treemap_tip"),
            |ui| {
                ui.label(
                    egui::RichText::new(&tr.name)
                        .strong()
                        .size(12.0)
                        .color(label_color),
                );
                let kind = if tr.is_dir { "Directory" } else { "File" };
                ui.label(
                    egui::RichText::new(format!(
                        "{} — {:.1}%  ({})",
                        format_size(tr.size),
                        tr.percent,
                        kind,
                    ))
                    .color(label_color_dim),
                );
            },
        );

        // Handle click: single = select, double = navigate/open.
        // Only if no navigation button already set an action this frame.
        if action.is_none() {
            if double_clicked {
                if tr.is_dir {
                    action = Some(TreemapAction::NavigateDir(tr.node_idx));
                } else {
                    let full_path = tree.full_path(tr.node_idx);
                    action = Some(TreemapAction::OpenFile(full_path));
                }
            } else if clicked {
                action = Some(TreemapAction::SelectNode(tr.node_idx));
            }
        }
    }

    // ── Draw selection highlight for the tree-view-selected node ───
    if let Some(sel) = selected_node {
        for tr in &rects {
            if tr.node_idx == sel {
                let sel_color = if is_light {
                    Color32::from_rgb(0xff, 0xa0, 0x00)
                } else {
                    Color32::from_rgb(0xff, 0xd0, 0x40)
                };
                let sel_rect = if tr.is_dir {
                    tr.header_rect.unwrap_or(tr.rect)
                } else {
                    tr.rect
                };
                painter.rect_stroke(
                    sel_rect,
                    0.0,
                    egui::Stroke::new(2.5, sel_color),
                    egui::StrokeKind::Inside,
                );
                break;
            }
        }
    }

    action
}

// ── Helpers ─────────────────────────────────────────────────────────

fn truncate_name(name: &str, max_chars: usize) -> String {
    if name.len() > max_chars && max_chars > 3 {
        format!("{}…", &name[..max_chars.min(name.len()) - 1])
    } else {
        name.to_string()
    }
}

// ── Nested Squarified Layout ────────────────────────────────────────

/// Recursively lay out children, producing nested rectangles.
/// Directories get a header bar and their children are laid out inside.
fn layout_nested(
    tree: &FileTree,
    children: &[NodeIndex],
    parent_size: u64,
    bounds: Rect,
    depth: usize,
    base_color_idx: usize,
    is_light: bool,
    rects: &mut Vec<TreemapRect>,
) {
    if children.is_empty() || parent_size == 0 {
        return;
    }

    let total_area = bounds.width() * bounds.height();
    let items: Vec<(NodeIndex, f32)> = children
        .iter()
        .filter_map(|&idx| {
            let size = tree.nodes[idx.idx()].size;
            if size == 0 {
                return None;
            }
            let area = (size as f64 / parent_size as f64) as f32 * total_area;
            if area < MIN_RECT_AREA {
                return None;
            }
            Some((idx, area))
        })
        .collect();

    if items.is_empty() {
        return;
    }

    // Run squarified algorithm on this level, then recurse into directories.
    squarify_nested(&items, bounds, tree, parent_size, depth, base_color_idx, is_light, rects);
}

/// Squarified layout that produces nested rectangles.
fn squarify_nested(
    items: &[(NodeIndex, f32)],
    bounds: Rect,
    tree: &FileTree,
    parent_size: u64,
    depth: usize,
    base_color_idx: usize,
    is_light: bool,
    rects: &mut Vec<TreemapRect>,
) {
    if items.is_empty() || bounds.width() < 2.0 || bounds.height() < 2.0 {
        return;
    }

    let mut remaining = bounds;
    let mut idx = 0;

    while idx < items.len() {
        let w = remaining.width();
        let h = remaining.height();
        if w < 2.0 || h < 2.0 {
            break;
        }

        let layout_vertical = w >= h;
        let side = if layout_vertical { h } else { w };

        // Greedy row building.
        let mut row: Vec<usize> = vec![idx];
        let mut row_area: f32 = items[idx].1;
        let mut best_worst = worst_ratio(&[items[idx].1], side, row_area);

        idx += 1;
        while idx < items.len() {
            let candidate = items[idx].1;
            let new_area = row_area + candidate;
            let mut trial: Vec<f32> = row.iter().map(|&i| items[i].1).collect();
            trial.push(candidate);
            let new_ratio = worst_ratio(&trial, side, new_area);

            if new_ratio <= best_worst {
                row.push(idx);
                row_area = new_area;
                best_worst = new_ratio;
                idx += 1;
            } else {
                break;
            }
        }

        // Row fraction of remaining.
        let remaining_total: f32 = items[row[0]..].iter().map(|(_, a)| a).sum();
        let row_fraction = if remaining_total > 0.0 {
            row_area / remaining_total
        } else {
            1.0
        };

        let (row_rect, new_remaining) = if layout_vertical {
            let row_w = remaining.width() * row_fraction;
            (
                Rect::from_min_size(remaining.left_top(), Vec2::new(row_w, remaining.height())),
                Rect::from_min_size(
                    remaining.left_top() + Vec2::new(row_w, 0.0),
                    Vec2::new(remaining.width() - row_w, remaining.height()),
                ),
            )
        } else {
            let row_h = remaining.height() * row_fraction;
            (
                Rect::from_min_size(remaining.left_top(), Vec2::new(remaining.width(), row_h)),
                Rect::from_min_size(
                    remaining.left_top() + Vec2::new(0.0, row_h),
                    Vec2::new(remaining.width(), remaining.height() - row_h),
                ),
            )
        };

        remaining = new_remaining;

        // Place items in row.
        let mut offset = 0.0;
        for &ri in &row {
            let (node_idx, area) = items[ri];
            let fraction = if row_area > 0.0 { area / row_area } else { 0.0 };

            let item_rect = if layout_vertical {
                let item_h = row_rect.height() * fraction;
                let r = Rect::from_min_size(
                    row_rect.left_top() + Vec2::new(0.0, offset),
                    Vec2::new(row_rect.width(), item_h),
                );
                offset += item_h;
                r
            } else {
                let item_w = row_rect.width() * fraction;
                let r = Rect::from_min_size(
                    row_rect.left_top() + Vec2::new(offset, 0.0),
                    Vec2::new(item_w, row_rect.height()),
                );
                offset += item_w;
                r
            };

            let child = &tree.nodes[node_idx.idx()];
            let ci = (base_color_idx + ri) % PALETTE.len();
            let (cr, cg, cb) = PALETTE[ci];
            let base_color = if is_light {
                Color32::from_rgb(
                    (cr as f32 * 0.75) as u8,
                    (cg as f32 * 0.75) as u8,
                    (cb as f32 * 0.75) as u8,
                )
            } else {
                Color32::from_rgb(cr, cg, cb)
            };

            // Darken/lighten by depth for visual distinction.
            let depth_factor = 0.06 * depth as f32;
            let color = if child.is_dir {
                darken(base_color, depth_factor)
            } else {
                darken(base_color, depth_factor + 0.10)
            };

            let pct = if parent_size > 0 {
                child.size as f64 / parent_size as f64 * 100.0
            } else {
                0.0
            };

            if child.is_dir {
                // Directory: add header bar, then recurse into children.
                let has_room_for_header = item_rect.height() > HEADER_H + 4.0
                    && item_rect.width() > 8.0;

                let header_rect = if has_room_for_header {
                    Some(Rect::from_min_size(
                        item_rect.left_top(),
                        Vec2::new(item_rect.width(), HEADER_H),
                    ))
                } else {
                    None
                };

                rects.push(TreemapRect {
                    node_idx,
                    rect: item_rect,
                    color,
                    name: child.name.to_string(),
                    size: child.size,
                    percent: pct as f32,
                    is_dir: true,
                    depth,
                    header_rect,
                });

                // Recurse into children if there's room and we aren't too deep.
                if depth < MAX_NEST_DEPTH && has_room_for_header {
                    let content_rect = Rect::from_min_size(
                        item_rect.left_top() + Vec2::new(FRAME_PAD, HEADER_H + FRAME_PAD),
                        Vec2::new(
                            item_rect.width() - FRAME_PAD * 2.0,
                            item_rect.height() - HEADER_H - FRAME_PAD * 2.0,
                        ),
                    );

                    if content_rect.width() > 4.0 && content_rect.height() > 4.0 {
                        let sub_children = tree.children_sorted_by_size(node_idx);
                        if !sub_children.is_empty() && child.size > 0 {
                            layout_nested(
                                tree,
                                &sub_children,
                                child.size,
                                content_rect,
                                depth + 1,
                                ci,
                                is_light,
                                rects,
                            );
                        }
                    }
                }
            } else {
                // File: just a leaf rectangle.
                rects.push(TreemapRect {
                    node_idx,
                    rect: item_rect,
                    color,
                    name: child.name.to_string(),
                    size: child.size,
                    percent: pct as f32,
                    is_dir: false,
                    depth,
                    header_rect: None,
                });
            }
        }
    }
}

/// Compute the worst (highest) aspect ratio among items in a row.
fn worst_ratio(areas: &[f32], side: f32, total: f32) -> f32 {
    if side <= 0.0 || total <= 0.0 {
        return f32::MAX;
    }

    let other = total / side;
    let mut worst = 0.0_f32;

    for &area in areas {
        let dim = area / other;
        let ratio = if dim > other {
            dim / other
        } else if dim > 0.0 {
            other / dim
        } else {
            f32::MAX
        };
        worst = worst.max(ratio);
    }
    worst
}

/// Lighten a colour by blending towards white.
#[allow(dead_code)]
fn lighten(c: Color32, amount: f32) -> Color32 {
    let a = amount.clamp(0.0, 1.0);
    Color32::from_rgb(
        (c.r() as f32 + (255.0 - c.r() as f32) * a) as u8,
        (c.g() as f32 + (255.0 - c.g() as f32) * a) as u8,
        (c.b() as f32 + (255.0 - c.b() as f32) * a) as u8,
    )
}

/// Darken a colour by scaling towards black.
fn darken(c: Color32, amount: f32) -> Color32 {
    let f = 1.0 - amount.clamp(0.0, 1.0);
    Color32::from_rgb(
        (c.r() as f32 * f) as u8,
        (c.g() as f32 * f) as u8,
        (c.b() as f32 * f) as u8,
    )
}
