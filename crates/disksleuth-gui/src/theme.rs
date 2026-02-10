/// Colour scheme and visual theme for DiskSleuth.
///
/// Provides both dark and light themes with a professional appearance.
/// All colour constants are defined here so the rest of the UI code
/// references semantically-named values rather than raw hex codes.

use egui::{Color32, Stroke, Visuals};

/// Which theme is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

impl ThemeMode {
    /// Toggle between dark and light.
    pub fn toggle(&mut self) {
        *self = match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        };
    }
}

/// Semantic colour palette for DiskSleuth.
pub struct DiskSleuthTheme {
    pub background: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub folder_icon: Color32,
    pub file_icon: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub success: Color32,
    pub bar_small: Color32,
    pub bar_large: Color32,
    pub separator: Color32,
    pub selection: Color32,
    pub header_bg: Color32,
}

impl DiskSleuthTheme {
    /// Dark theme — the default.
    pub fn dark() -> Self {
        Self {
            background: Color32::from_rgb(0x1e, 0x1e, 0x2e),
            surface: Color32::from_rgb(0x2a, 0x2a, 0x3c),
            surface_hover: Color32::from_rgb(0x35, 0x35, 0x4a),
            text_primary: Color32::from_rgb(0xe4, 0xe4, 0xe8),
            text_secondary: Color32::from_rgb(0xb8, 0xb8, 0xc4),
            text_muted: Color32::from_rgb(0x6c, 0x70, 0x86),
            accent: Color32::from_rgb(0x89, 0xb4, 0xfa),
            accent_hover: Color32::from_rgb(0x6a, 0x9a, 0xf0),
            folder_icon: Color32::from_rgb(0xf9, 0xe2, 0xaf),
            file_icon: Color32::from_rgb(0x89, 0xb4, 0xfa),
            error: Color32::from_rgb(0xf3, 0x8b, 0xa8),
            warning: Color32::from_rgb(0xfa, 0xb3, 0x87),
            success: Color32::from_rgb(0xa6, 0xe3, 0xa1),
            bar_small: Color32::from_rgb(0xa6, 0xe3, 0xa1),
            bar_large: Color32::from_rgb(0xf3, 0x8b, 0xa8),
            separator: Color32::from_rgb(0x3a, 0x3a, 0x50),
            selection: Color32::from_rgb(0x28, 0x3a, 0x5c),
            header_bg: Color32::from_rgb(0x22, 0x22, 0x34),
        }
    }

    /// Light theme — optional toggle.
    pub fn light() -> Self {
        Self {
            background: Color32::from_rgb(0xf5, 0xf5, 0xf5),
            surface: Color32::from_rgb(0xff, 0xff, 0xff),
            surface_hover: Color32::from_rgb(0xe8, 0xe8, 0xef),
            text_primary: Color32::from_rgb(0x1e, 0x1e, 0x2e),
            text_secondary: Color32::from_rgb(0x4a, 0x4a, 0x5a),
            text_muted: Color32::from_rgb(0x8a, 0x8a, 0x9a),
            accent: Color32::from_rgb(0x3a, 0x6f, 0xd8),
            accent_hover: Color32::from_rgb(0x2a, 0x5a, 0xc0),
            folder_icon: Color32::from_rgb(0xc0, 0x98, 0x20),
            file_icon: Color32::from_rgb(0x3a, 0x6f, 0xd8),
            error: Color32::from_rgb(0xd0, 0x40, 0x50),
            warning: Color32::from_rgb(0xd0, 0x80, 0x20),
            success: Color32::from_rgb(0x30, 0x98, 0x30),
            bar_small: Color32::from_rgb(0x60, 0xb0, 0x60),
            bar_large: Color32::from_rgb(0xd0, 0x50, 0x60),
            separator: Color32::from_rgb(0xd0, 0xd0, 0xd8),
            selection: Color32::from_rgba_premultiplied(0x3a, 0x6f, 0xd8, 0x30),
            header_bg: Color32::from_rgb(0xe8, 0xe8, 0xf0),
        }
    }

    /// Get the theme for the given mode.
    pub fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self::dark(),
            ThemeMode::Light => Self::light(),
        }
    }

    /// Apply this theme to an egui context.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();

        let mut visuals = if self.background.r() < 128 {
            Visuals::dark()
        } else {
            Visuals::light()
        };

        visuals.panel_fill = self.background;
        visuals.window_fill = self.surface;
        visuals.extreme_bg_color = self.background;
        visuals.faint_bg_color = self.surface;
        visuals.selection.bg_fill = self.selection;
        visuals.selection.stroke = Stroke::new(1.0, self.accent);

        visuals.widgets.noninteractive.bg_fill = self.surface;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text_primary);

        visuals.widgets.inactive.bg_fill = self.surface;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text_secondary);

        visuals.widgets.hovered.bg_fill = self.surface_hover;
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, self.accent);

        visuals.widgets.active.bg_fill = self.accent;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, self.background);

        visuals.window_stroke = Stroke::new(1.0, self.separator);

        style.visuals = visuals;
        style.spacing.item_spacing = egui::vec2(8.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);

        ctx.set_style(style);
    }

    /// Compute a bar colour that interpolates between bar_small and bar_large
    /// based on the percentage (0.0 – 100.0).
    pub fn bar_color(&self, percent: f32) -> Color32 {
        let t = (percent / 100.0).clamp(0.0, 1.0);
        lerp_color(self.bar_small, self.bar_large, t)
    }
}

/// Linear interpolation between two colours.
fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgb(
        (a.r() as f32 * (1.0 - t) + b.r() as f32 * t) as u8,
        (a.g() as f32 * (1.0 - t) + b.g() as f32 * t) as u8,
        (a.b() as f32 * (1.0 - t) + b.b() as f32 * t) as u8,
    )
}
