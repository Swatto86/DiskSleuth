/// DiskSleuth Core — scanning, analysis, and data model.
///
/// This crate contains all business logic with zero UI dependencies.
/// It is designed to be reusable across different frontends (GUI, CLI, TUI).
///
/// # Modules
///
/// - [`model`] — Arena-allocated file tree and supporting types.
/// - [`scanner`] — Background filesystem scanning with progress reporting.
/// - [`analysis`] — Post-scan analysis algorithms (top files, file types, etc.).
/// - [`platform`] — Windows-specific drive enumeration and permission checks.
/// - [`monitor`] — Live file-write monitor using Windows ReadDirectoryChangesW.
pub mod analysis;
pub mod model;
pub mod monitor;
pub mod platform;
pub mod scanner;
