/// Main `eframe::App` implementation for DiskSleuth.
///
/// This is the top-level UI layout that composes all panels and widgets.
use crate::panels;
use crate::state::AppState;
use crate::widgets;

/// Pre-built application state.
///
/// Construct this **before** calling `eframe::run_native` so that the
/// expensive work (drive enumeration, initial scan kick-off) completes
/// before the OS window is created.  This matches LogSleuth's startup
/// pattern and prevents the window from sitting on a white background
/// while setup runs.
pub struct DiskSleuthState {
    pub(crate) inner: AppState,
}

impl DiskSleuthState {
    /// Enumerate drives and start the auto-scan of the OS drive.
    /// Call this before `eframe::run_native`.
    pub fn build() -> Self {
        let mut state = AppState::new();

        // Auto-scan the OS drive on startup.
        let os_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
        let os_drive_path = format!("{}\\", os_drive);
        if let Some(idx) = state.drives.iter().position(|d| {
            d.path
                .to_string_lossy()
                .eq_ignore_ascii_case(&os_drive_path)
        }) {
            state.selected_drive_index = Some(idx);
            let path = state.drives[idx].path.clone();
            state.start_scan(path);
        }

        Self { inner: state }
    }
}

/// The DiskSleuth application.
pub struct DiskSleuthApp {
    state: AppState,
    /// Tracks the `dark_mode` value at the last `ctx.set_visuals()` call.
    /// `None` on first frame (→ always sets visuals once at startup).
    /// Avoids allocating a new `Visuals` struct every frame when the theme
    /// has not changed.
    last_dark_mode: Option<bool>,
}

impl DiskSleuthApp {
    /// Create a new application instance from pre-built state.
    ///
    /// The state should have been constructed by [`DiskSleuthState::build()`]
    /// *before* `eframe::run_native` is called.
    pub fn with_state(cc: &eframe::CreationContext<'_>, state: DiskSleuthState) -> Self {
        // ── Font: Segoe UI + Segoe UI Emoji fallback ──────────────────────
        // Load Segoe UI as the primary proportional font.  Then load Segoe UI
        // Emoji as a fallback so that every emoji glyph (📁 📄 🔍 ⏹ ☀ 🌙 …)
        // renders correctly.  egui walks the family list in order and uses the
        // first font that contains the requested glyph, so the emoji font is
        // only consulted when Segoe UI lacks a codepoint.
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let font_path = format!("{}\\Fonts\\segoeui.ttf", system_root);
        let emoji_font_path = format!("{}\\Fonts\\seguiemj.ttf", system_root);

        let mut fonts = egui::FontDefinitions::default();

        // Primary font — Segoe UI.
        match std::fs::read(&font_path) {
            Ok(bytes) => {
                fonts.font_data.insert(
                    "SegoeUI".to_owned(),
                    egui::FontData::from_owned(bytes).into(),
                );
                // Highest priority in proportional family.
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "SegoeUI".to_owned());
                // Also use for monospace labels (file paths, etc.).
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .insert(0, "SegoeUI".to_owned());
                tracing::info!("Loaded Segoe UI from {}", font_path);
            }
            Err(e) => {
                tracing::warn!(
                    "Could not load Segoe UI from {}: {} -- using default font",
                    font_path,
                    e
                );
            }
        }

        // Emoji fallback — Segoe UI Emoji (seguiemj.ttf).
        // Appended *after* Segoe UI so it is only used when the primary font
        // lacks a glyph (emoji codepoints).  If the file is absent the app
        // continues without it; emoji will fall back to egui's built-in font.
        match std::fs::read(&emoji_font_path) {
            Ok(bytes) => {
                fonts.font_data.insert(
                    "SegoeUIEmoji".to_owned(),
                    egui::FontData::from_owned(bytes).into(),
                );
                // Push to the end — lowest priority, only used as fallback.
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push("SegoeUIEmoji".to_owned());
                tracing::info!("Loaded Segoe UI Emoji from {}", emoji_font_path);
            }
            Err(e) => {
                tracing::warn!(
                    "Could not load Segoe UI Emoji from {}: {} -- emoji may show as placeholders",
                    emoji_font_path,
                    e
                );
            }
        }

        cc.egui_ctx.set_fonts(fonts);

        // Apply initial dark visuals.
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        Self {
            state: state.inner,
            last_dark_mode: None,
        }
    }

    /// Global keyboard navigation for the tree view.
    ///
    /// Arrow keys move/fold the selection; vim keys (h/j/k/l) mirror them.
    /// Enter drills into the selected directory (treemap) or opens the file
    /// in Explorer; Delete asks to recycle the selection. Skipped whenever a
    /// widget owns the keyboard (e.g. a text field) or the delete dialog is
    /// open (it handles Enter/Escape itself).
    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() || self.state.pending_delete.is_some() {
            return;
        }
        use egui::Key;
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));

        if pressed(Key::ArrowDown) || pressed(Key::J) {
            self.state.move_selection(1);
        }
        if pressed(Key::ArrowUp) || pressed(Key::K) {
            self.state.move_selection(-1);
        }
        if pressed(Key::PageDown) {
            self.state.move_selection(20);
        }
        if pressed(Key::PageUp) {
            self.state.move_selection(-20);
        }
        if pressed(Key::Home) {
            self.state.move_selection(isize::MIN / 2);
        }
        if pressed(Key::End) {
            self.state.move_selection(isize::MAX / 2);
        }
        if pressed(Key::ArrowRight) || pressed(Key::L) {
            self.state.expand_selection();
        }
        if pressed(Key::ArrowLeft) || pressed(Key::H) {
            self.state.collapse_selection();
        }
        if pressed(Key::Enter) {
            self.activate_selection();
        }
        if pressed(Key::Delete) {
            if let Some(selected) = self.state.selected_node {
                self.state.request_delete(selected);
            }
        }
    }

    /// Enter on a selection: drill the treemap into a directory, or reveal
    /// a file in Explorer.
    fn activate_selection(&mut self) {
        let Some(selected) = self.state.selected_node else {
            return;
        };
        let target = {
            let Some(ref tree) = self.state.tree else {
                return;
            };
            if selected.idx() >= tree.len() {
                return;
            }
            if tree.node(selected).is_dir {
                None
            } else {
                Some(tree.full_path(selected))
            }
        };
        match target {
            None => self.state.treemap_navigate_to(selected),
            Some(path) => {
                let _ = std::process::Command::new("explorer.exe")
                    .arg(format!("/select,{}", path))
                    .spawn();
            }
        }
    }
}

impl eframe::App for DiskSleuthApp {
    /// Override the GPU clear colour to match the active theme background,
    /// preventing a colour mismatch flash between frames.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        let [r, g, b, a] = visuals.panel_fill.to_array();
        [
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        ]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── Apply theme ───────────────────────────────────────────────────
        // Only call set_visuals when dark_mode actually changes so we avoid
        // allocating a new Visuals struct (and diffing it) every frame.
        let dark = self.state.dark_mode;
        if self.last_dark_mode != Some(dark) {
            if dark {
                ctx.set_visuals(egui::Visuals::dark());
            } else {
                ctx.set_visuals(egui::Visuals::light());
            }
            self.last_dark_mode = Some(dark);
        }

        // ── Process background messages ───────────────────────────────────
        let _data_changed = self.state.process_scan_messages();
        let _monitor_changed = self.state.process_monitor_messages();
        let _duplicates_changed = self.state.process_duplicate_messages();

        // Request continuous repaint while background work is running.
        let needs_repaint = self.state.phase == crate::state::AppPhase::Scanning
            || self.state.monitor_active
            || self.state.duplicate_scan.is_some();
        if needs_repaint {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // ── Keyboard navigation ───────────────────────────────────────────
        self.handle_keyboard(ctx);

        // ── Top toolbar ───────────────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar")
            .min_height(36.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                widgets::toolbar::toolbar(ui, &mut self.state);
                ui.add_space(4.0);
            });

        // ── About dialog ──────────────────────────────────────────────────
        let mut show_about = self.state.show_about;
        egui::Window::new("About DiskSleuth")
            .open(&mut show_about)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([340.0, 0.0])
            .show(ctx, |ui| {
                // Use theme-aware colours so the dialog looks correct in both
                // dark and light mode.
                let accent = ui.visuals().hyperlink_color;
                let muted = ui.visuals().weak_text_color();
                let normal = ui.visuals().text_color();
                let strong = ui.visuals().strong_text_color();

                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("🔍 DiskSleuth")
                            .size(24.0)
                            .strong()
                            .color(accent),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .size(13.0)
                            .color(muted),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(
                            "A fast, visual disk space analyser for Windows.\n\
                             Parallel scanning, SpaceSniffer-style treemap,\n\
                             and an interactive tree view.",
                        )
                        .size(12.0)
                        .color(normal),
                    );
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("Developed by Swatto")
                            .size(13.0)
                            .strong()
                            .color(strong),
                    );
                    ui.add_space(4.0);
                    ui.hyperlink_to(
                        "github.com/Swatto86/DiskSleuth",
                        "https://github.com/Swatto86/DiskSleuth",
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("MIT License - (c) 2026 Swatto")
                            .size(11.0)
                            .color(muted),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("Built with Rust & egui")
                            .size(11.0)
                            .color(muted),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("with a little sprinkling of help from SteveO")
                            .size(10.0)
                            .italics()
                            .color(muted),
                    );
                    ui.add_space(8.0);
                });
            });
        self.state.show_about = show_about;

        // ── Bottom status bar ─────────────────────────────────────────────
        egui::TopBottomPanel::bottom("status_bar")
            .min_height(24.0)
            .show(ctx, |ui| {
                ui.add_space(2.0);
                widgets::status_bar::status_bar(ui, &self.state);
                ui.add_space(2.0);
            });

        // ── Live write monitor panel (optional bottom panel) ──────────────
        if self.state.show_monitor_panel {
            egui::TopBottomPanel::bottom("monitor_panel")
                .resizable(true)
                .default_height(200.0)
                .min_height(120.0)
                .max_height(500.0)
                .show(ctx, |ui| {
                    ui.add_space(4.0);
                    panels::monitor_panel::monitor_panel(ui, &mut self.state);
                    ui.add_space(4.0);
                });
        }

        // ── Left sidebar ──────────────────────────────────────────────────
        egui::SidePanel::left("left_panel")
            .default_width(500.0)
            .min_width(300.0)
            .max_width(800.0)
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    panels::scan_panel::scan_panel(ui, &mut self.state);
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    panels::tree_panel::tree_panel(ui, &mut self.state);
                });
            });

        // ── Right details panel ───────────────────────────────────────────
        egui::SidePanel::right("right_panel")
            .default_width(220.0)
            .min_width(180.0)
            .max_width(350.0)
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    panels::details_panel::details_panel(ui, &mut self.state);
                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(8.0);
                    panels::chart_panel::chart_panel(ui, &self.state);
                });
            });

        // ── Floating analysis windows & dialogs ──────────────────────────
        panels::largest_files_window::largest_files_window(ctx, &mut self.state);
        panels::old_files_window::old_files_window(ctx, &mut self.state);
        panels::duplicates_window::duplicates_window(ctx, &mut self.state);
        panels::history_window::history_window(ctx, &mut self.state);
        panels::delete_dialog::delete_dialog(ctx, &mut self.state);

        // ── Central panel (Treemap) ───────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            use widgets::treemap::TreemapAction;
            if let Some(act) = widgets::treemap::treemap(ui, &self.state) {
                match act {
                    TreemapAction::NavigateDir(node) => {
                        self.state.treemap_navigate_to(node);
                        self.state.selected_node = Some(node);
                        self.state.reveal_node_in_tree(node);
                    }
                    TreemapAction::SelectNode(node) => {
                        self.state.selected_node = Some(node);
                        self.state.reveal_node_in_tree(node);
                    }
                    TreemapAction::OpenFile(path) => {
                        let _ = std::process::Command::new("explorer.exe")
                            .arg(format!("/select,{}", path))
                            .spawn();
                    }
                    TreemapAction::Back => {
                        self.state.treemap_go_back();
                    }
                    TreemapAction::Forward => {
                        self.state.treemap_go_forward();
                    }
                    TreemapAction::Up => {
                        // treemap_go_up reads the tree internally — no clone needed.
                        self.state.treemap_go_up();
                    }
                }
            }
        });
    }
}
