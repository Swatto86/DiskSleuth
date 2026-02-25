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

    // Build application state *before* opening the window so the first
    // rendered frame arrives immediately and the OS never fills the window
    // with its default white background.  This is the same pattern as
    // LogSleuth and is why LogSleuth has no white flash on launch.
    let state = disksleuth_gui::DiskSleuthState::build();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("DiskSleuth -- Disk Space Analyser")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_icon(icon)
            // Prevents Windows from filling the window with white before the
            // first OpenGL frame is rendered. DWM compositing is used instead,
            // which starts transparent/black rather than white.
            .with_transparent(true),
        ..Default::default()
    };

    eframe::run_native(
        "DiskSleuth",
        options,
        Box::new(|cc| {
            Ok(Box::new(disksleuth_gui::DiskSleuthApp::with_state(
                cc, state,
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))?;

    Ok(())
}
