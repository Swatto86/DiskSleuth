//! DiskSleuth — production-grade disk space analyser.
//!
//! Thin binary entry point. All logic lives in the `disksleuth-core`
//! and `disksleuth-gui` crates.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> anyhow::Result<()> {
    // Initialise structured logging.
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("DiskSleuth starting");

    let icon = disksleuth_gui::icon::generate_icon(64);

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("DiskSleuth — Disk Space Analyser")
            .with_maximized(true)
            .with_min_inner_size([800.0, 500.0])
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "DiskSleuth",
        options,
        Box::new(|cc| Ok(Box::new(disksleuth_gui::DiskSleuthApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))?;

    Ok(())
}
