/// Scanner module — orchestrates filesystem scanning.
///
/// Provides a two-tier scanning strategy:
/// - **Tier 1 (MFT):** Direct NTFS Master File Table reading (requires admin).
/// - **Tier 2 (Parallel walk):** `jwalk`-based parallel directory traversal (no admin).
///
/// Both tiers write into a **shared `LiveTree`** (`Arc<RwLock<FileTree>>`) so
/// the UI can render a real-time, incrementally-growing tree view while the
/// scan is running.
pub mod mft;
pub mod parallel;
pub mod progress;

use crate::model::FileTree;
use progress::ScanProgress;

use crossbeam_channel::Receiver;
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tracing::info;

/// A shared, concurrently-readable file tree.
///
/// The scanner holds a write lock briefly when inserting batches of nodes.
/// The UI holds a read lock each frame to render the live tree.
pub type LiveTree = Arc<RwLock<FileTree>>;

/// Handle to a running or completed scan. Allows cancellation and
/// receiving progress updates.
pub struct ScanHandle {
    /// Receiver for progress updates from the scan thread.
    pub progress_rx: Receiver<ScanProgress>,
    /// Shared tree that is populated incrementally during scanning.
    pub live_tree: LiveTree,
    /// Flag to request cancellation.
    cancel_flag: Arc<AtomicBool>,
    /// Join handle for the scan thread.
    _thread: Option<thread::JoinHandle<()>>,
}

impl ScanHandle {
    /// Request the scan to stop as soon as possible.
    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    /// Check whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancel_flag.load(Ordering::Relaxed)
    }
}

/// Start a new scan on a background thread.
///
/// Returns a `ScanHandle` for receiving progress, accessing the live tree,
/// and requesting cancellation.
/// Maximum number of progress messages that may queue up in the channel.
///
/// The UI drains this channel once per frame (~60 fps). At 60 fps a burst of
/// 4 096 messages gives the scanner >68 seconds of headroom before back-pressure
/// causes `send` to block. If the UI falls behind (hidden window, resizing)
/// the scanner stalls briefly rather than consuming unbounded heap.
pub const PROGRESS_CHANNEL_CAPACITY: usize = 4_096;

pub fn start_scan(root_path: PathBuf) -> ScanHandle {
    let (progress_tx, progress_rx) =
        crossbeam_channel::bounded::<ScanProgress>(PROGRESS_CHANNEL_CAPACITY);
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_clone = cancel_flag.clone();

    let live_tree: LiveTree = Arc::new(RwLock::new(FileTree::with_capacity(500_000)));
    let tree_clone = live_tree.clone();

    let thread = thread::Builder::new()
        .name("disksleuth-scanner".into())
        .spawn(move || {
            info!("Starting scan of {}", root_path.display());

            let is_elevated = crate::platform::is_elevated();

            // Tier selection: try MFT first, fall back to parallel walk.
            if mft::is_mft_available(&root_path) {
                info!("Using MFT direct reader (Tier 1)");
                let _ = progress_tx.send(ScanProgress::ScanTier {
                    is_mft: true,
                    is_elevated,
                });
                mft::scan_mft(
                    root_path.clone(),
                    progress_tx.clone(),
                    cancel_clone.clone(),
                    tree_clone.clone(),
                );

                // If the MFT scan failed (tree is empty), fall back to Tier 2.
                let tree_empty = tree_clone.read().is_empty();
                if tree_empty && !cancel_clone.load(Ordering::Relaxed) {
                    info!("MFT scan produced no results — falling back to parallel walk (Tier 2)");
                    let _ = progress_tx.send(ScanProgress::ScanTier {
                        is_mft: false,
                        is_elevated,
                    });
                    parallel::scan_parallel(root_path, progress_tx, cancel_clone, tree_clone);
                }
            } else {
                info!("Using parallel directory walker (Tier 2)");
                let _ = progress_tx.send(ScanProgress::ScanTier {
                    is_mft: false,
                    is_elevated,
                });
                parallel::scan_parallel(root_path, progress_tx, cancel_clone, tree_clone);
            }
        })
        .expect("failed to spawn scanner thread");

    ScanHandle {
        progress_rx,
        live_tree,
        cancel_flag,
        _thread: Some(thread),
    }
}
