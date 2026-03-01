/// Live file write monitor panel.
///
/// Displays a continuously-updated table of files being written to on the
/// selected drive, helping identify what is consuming disk space rapidly.
///
/// The panel is rendered as a bottom panel when `state.show_monitor_panel`
/// is `true`.  Start/Stop is controlled directly from the panel.
use crate::state::AppState;
use egui::Ui;

/// Draw the live write monitor panel.
pub fn monitor_panel(ui: &mut Ui, state: &mut AppState) {
    ui.vertical(|ui| {
        // ── Header row ────────────────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("👁 Live Write Monitor")
                    .strong()
                    .color(ui.visuals().hyperlink_color),
            );

            ui.separator();

            // Drive/path being monitored (or prompt to select one).
            if state.monitor_active {
                ui.label(
                    egui::RichText::new(format!("Watching: {}", state.monitor_path))
                        .size(11.0)
                        .color(ui.visuals().weak_text_color()),
                );
            } else if !state.monitor_path.is_empty() {
                ui.label(
                    egui::RichText::new(format!("Last: {}", state.monitor_path))
                        .size(11.0)
                        .color(ui.visuals().weak_text_color()),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Clear button.
                if ui
                    .button("🗑 Clear")
                    .on_hover_text("Clear all recorded events")
                    .clicked()
                {
                    state.monitor_entries.clear();
                }

                ui.separator();

                // Start / Stop button.
                if state.monitor_active {
                    if ui
                        .button(
                            egui::RichText::new("⏹ Stop")
                                .color(egui::Color32::from_rgb(0xf3, 0x8b, 0xa8)),
                        )
                        .on_hover_text("Stop the write monitor")
                        .clicked()
                    {
                        state.stop_monitor();
                    }
                } else {
                    // Only allow start when a drive is selected.
                    let can_start = state.selected_drive_index.is_some();
                    if ui
                        .add_enabled(
                            can_start,
                            egui::Button::new(
                                egui::RichText::new("▶ Start")
                                    .color(egui::Color32::from_rgb(0xa6, 0xe3, 0xa1)),
                            ),
                        )
                        .on_hover_text("Start monitoring the selected drive for file writes")
                        .clicked()
                    {
                        if let Some(idx) = state.selected_drive_index {
                            let path = state.drives[idx].path.clone();
                            state.start_monitor(path);
                        }
                    }
                }
            });
        });

        ui.separator();

        // ── Content ──────────────────────────────────────────────────────
        if state.monitor_entries.is_empty() {
            ui.centered_and_justified(|ui| {
                let msg = if state.monitor_active {
                    "Waiting for file write activity..."
                } else {
                    "Press ▶ Start to begin monitoring the selected drive."
                };
                ui.label(
                    egui::RichText::new(msg)
                        .size(12.0)
                        .color(ui.visuals().weak_text_color()),
                );
            });
        } else {
            // Sort entries by hit count (descending) for the most active files first.
            let mut sorted: Vec<&disksleuth_core::monitor::WriteEvent> =
                state.monitor_entries.iter().collect();
            sorted.sort_by(|a, b| {
                b.hit_count
                    .cmp(&a.hit_count)
                    .then(b.last_seen.cmp(&a.last_seen))
            });

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    // Table header.
                    let accent = ui.visuals().hyperlink_color;
                    let muted = ui.visuals().weak_text_color();
                    let text_col = ui.visuals().text_color();

                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [50.0, 16.0],
                            egui::Label::new(
                                egui::RichText::new("Changes").size(11.0).color(accent),
                            ),
                        );
                        ui.add_sized(
                            [160.0, 16.0],
                            egui::Label::new(
                                egui::RichText::new("Last Seen").size(11.0).color(accent),
                            ),
                        );
                        ui.label(egui::RichText::new("File Path").size(11.0).color(accent));
                    });

                    ui.separator();

                    for event in sorted.iter().take(200) {
                        ui.horizontal(|ui| {
                            // Hit count — colour-coded by intensity.
                            let count_color = if event.hit_count >= 100 {
                                egui::Color32::from_rgb(0xf3, 0x8b, 0xa8) // red/hot
                            } else if event.hit_count >= 20 {
                                egui::Color32::from_rgb(0xfa, 0xb3, 0x87) // orange/warm
                            } else if event.hit_count >= 5 {
                                egui::Color32::from_rgb(0xf9, 0xe2, 0xaf) // yellow/moderate
                            } else {
                                text_col
                            };
                            ui.add_sized(
                                [50.0, 18.0],
                                egui::Label::new(
                                    egui::RichText::new(format!("{}", event.hit_count))
                                        .size(12.0)
                                        .strong()
                                        .color(count_color),
                                ),
                            );

                            // Last seen timestamp.
                            let time_str = event.last_seen.format("%H:%M:%S").to_string();
                            ui.add_sized(
                                [160.0, 18.0],
                                egui::Label::new(
                                    egui::RichText::new(time_str).size(11.0).color(muted),
                                ),
                            );

                            // File path — truncated from the left based on the
                            // available column width so the filename is always visible.
                            // Approximate 7 px per character at 12 px proportional size.
                            let avail_chars = ((ui.available_width() / 7.0).max(20.0)) as usize;
                            let path = truncate_path_left(&event.path, avail_chars);
                            ui.label(egui::RichText::new(path).size(12.0).color(text_col))
                                .on_hover_text(&event.path);
                        });
                    }

                    // Show overflow indicator.
                    if sorted.len() > 200 {
                        ui.label(
                            egui::RichText::new(format!(
                                "… and {} more entries (showing top 200 by activity)",
                                sorted.len() - 200
                            ))
                            .size(11.0)
                            .color(muted),
                        );
                    }
                });
        }
    });
}

/// Truncate a path from the left so it fits within `max_chars` characters.
///
/// Returns the original string if it already fits, or `"...\\<end>"` if not.
/// Uses character counts rather than byte lengths to avoid panicking on
/// multi-byte UTF-8 paths (Cyrillic, CJK, accented characters, etc.).
fn truncate_path_left(path: &str, max_chars: usize) -> String {
    let char_count = path.chars().count();
    if char_count <= max_chars {
        return path.to_owned();
    }
    // Keep the rightmost (max_chars - 4) chars so the "...\" prefix fits.
    let keep = max_chars.saturating_sub(4);
    let skip = char_count.saturating_sub(keep);
    let byte_start = path
        .char_indices()
        .nth(skip)
        .map(|(i, _)| i)
        .unwrap_or(path.len());
    format!("...\\{}", &path[byte_start..])
}
