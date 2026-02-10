/// Scan progress reporting — lightweight messages sent from the scan
/// thread to the UI thread via a crossbeam channel.

use std::time::Duration;

/// Commands sent from the UI thread to control the scanner.
#[derive(Debug)]
pub enum ScanCommand {
    /// Start scanning the given path (drive root or folder).
    Start(std::path::PathBuf),
    /// Cancel the current scan.
    Cancel,
}

/// Progress updates sent from the scan thread to the UI.
///
/// The actual tree data is in the shared `LiveTree`; these messages
/// carry only lightweight counters and status flags.
#[derive(Debug)]
pub enum ScanProgress {
    /// Notifies the UI which scanner tier is being used.
    ScanTier {
        /// `true` = MFT direct reader (Tier 1, admin), `false` = parallel walk (Tier 2).
        is_mft: bool,
        /// `true` if the process is running with admin privileges.
        is_elevated: bool,
    },
    /// Periodic update with running totals.
    Update {
        files_found: u64,
        dirs_found: u64,
        total_size: u64,
        current_path: String,
    },
    /// A non-fatal error (e.g. permission denied on one file).
    Error {
        path: String,
        message: String,
    },
    /// Scanning completed successfully.
    /// The finished tree is in the shared `LiveTree` (already aggregated).
    Complete {
        duration: Duration,
        error_count: u64,
    },
    /// Scan was cancelled by the user.
    Cancelled,
}
