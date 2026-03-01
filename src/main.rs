//! DiskSleuth — production-grade disk space analyser.
//!
//! Thin binary entry point. All logic lives in the `disksleuth-core`
//! and `disksleuth-gui` crates.
//!
//! # Debug / Verbose Logging
//!
//! Set the `DISKSLEUTH_LOG` environment variable before launching to enable
//! additional diagnostic output:
//!
//! ```powershell
//! $env:DISKSLEUTH_LOG = "debug"   # function-level tracing
//! $env:DISKSLEUTH_LOG = "trace"   # maximum verbosity
//! .\disksleuth.exe
//! ```
//!
//! Valid values (case-insensitive): `error`, `warn`, `info` (default),
//! `debug`, `trace`.  When the variable is absent the level defaults to
//! `info` so release builds incur no diagnostic overhead.  Log output
//! goes to stderr and is never written to a file or sent off-system.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> anyhow::Result<()> {
    // Initialise structured logging.
    //
    // The log level is runtime-selectable via the DISKSLEUTH_LOG environment
    // variable so operators can enable debug output without rebuilding.
    // Defence: unknown/malformed values fall back silently to INFO so the app
    // never fails to start due to a bad env var. Secrets/PII are never logged.
    let log_level = std::env::var("DISKSLEUTH_LOG")
        .ok()
        .and_then(|v| v.parse::<tracing::Level>().ok())
        .unwrap_or(tracing::Level::INFO);

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_writer(std::io::stderr)
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
