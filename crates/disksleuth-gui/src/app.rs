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
}

impl DiskSleuthApp {
    /// Create a new application instance from pre-built state.
    ///
    /// The state should have been constructed by [`DiskSleuthState::build()`]
    /// *before* `eframe::run_native` is called.
    pub fn with_state(cc: &eframe::CreationContext<'_>, state: DiskSleuthState) -> Self {
        // ── Font: Segoe UI ────────────────────────────────────────────────
        // Load Segoe UI from the Windows fonts directory and register it as
        // the highest-priority proportional font so every widget uses it.
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let font_path = format!("{}\\Fonts\\segoeui.ttf", system_root);

        let mut fonts = egui::FontDefinitions::default();
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
        cc.egui_ctx.set_fonts(fonts);

        // Apply initial dark visuals.
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        Self { state: state.inner }
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
        // Called every frame so that toggling dark_mode takes effect
        // immediately on the next rendered frame.
        if self.state.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // ── Process background messages ───────────────────────────────────
        let _data_changed = self.state.process_scan_messages();
        let _monitor_changed = self.state.process_monitor_messages();

        // Request continuous repaint while scanning or monitoring.
        let needs_repaint =
            self.state.phase == crate::state::AppPhase::Scanning || self.state.monitor_active;
        if needs_repaint {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

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
                    panels::details_panel::details_panel(ui, &self.state);
                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(8.0);
                    panels::chart_panel::chart_panel(ui, &self.state);
                });
            });

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
                        if let Some(ref tree) = self.state.tree {
                            let tree = tree.clone();
                            self.state.treemap_go_up(&tree);
                        } else if let Some(ref lt) = self.state.live_tree {
                            let tree = lt.read().clone();
                            self.state.treemap_go_up(&tree);
                        }
                    }
                }
            }
        });
    }
}
