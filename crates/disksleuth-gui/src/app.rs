/// Main `eframe::App` implementation for DiskSleuth.
///
/// This is the top-level UI layout that composes all panels and widgets.

use crate::panels;
use crate::state::AppState;
use crate::theme::DiskSleuthTheme;
use crate::widgets;

/// The DiskSleuth application.
pub struct DiskSleuthApp {
    state: AppState,
}

impl DiskSleuthApp {
    /// Create a new application instance.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Apply the initial dark theme.
        let theme = DiskSleuthTheme::dark();
        theme.apply(&cc.egui_ctx);

        let mut state = AppState::new();

        // Auto-scan the OS drive on startup.
        let os_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
        let os_drive_path = format!("{}\\", os_drive);
        if let Some(idx) = state
            .drives
            .iter()
            .position(|d| d.path.to_string_lossy().eq_ignore_ascii_case(&os_drive_path))
        {
            state.selected_drive_index = Some(idx);
            let path = state.drives[idx].path.clone();
            state.start_scan(path);
        }

        Self { state }
    }
}

impl eframe::App for DiskSleuthApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process any pending scan messages before rendering.
        let _data_changed = self.state.process_scan_messages();

        // Re-apply theme if it changed.
        let theme = DiskSleuthTheme::for_mode(self.state.theme_mode);
        theme.apply(ctx);

        // Request continuous repaint while scanning (for progress updates).
        if self.state.phase == crate::state::AppPhase::Scanning {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // ── Top toolbar ───────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar")
            .min_height(36.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                widgets::toolbar::toolbar(ui, &mut self.state, &theme);
                ui.add_space(4.0);
            });

        // ── Bottom status bar ─────────────────────────────────────
        egui::TopBottomPanel::bottom("status_bar")
            .min_height(24.0)
            .show(ctx, |ui| {
                ui.add_space(2.0);
                widgets::status_bar::status_bar(ui, &self.state, &theme);
                ui.add_space(2.0);
            });

        // ── Left sidebar ──────────────────────────────────────────
        egui::SidePanel::left("left_panel")
            .default_width(500.0)
            .min_width(300.0)
            .max_width(800.0)
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    panels::scan_panel::scan_panel(ui, &mut self.state, &theme);
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    panels::tree_panel::tree_panel(ui, &mut self.state, &theme);
                });
            });

        // ── Right details panel ───────────────────────────────────
        egui::SidePanel::right("right_panel")
            .default_width(220.0)
            .min_width(180.0)
            .max_width(350.0)
            .resizable(true)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    panels::details_panel::details_panel(ui, &self.state, &theme);
                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(8.0);
                    panels::chart_panel::chart_panel(ui, &self.state, &theme);
                });
            });

        // ── Central panel (Treemap) ───────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            use widgets::treemap::TreemapAction;
            if let Some(act) = widgets::treemap::treemap(ui, &self.state, &theme) {
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
