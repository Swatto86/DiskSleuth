/// File write monitor — watches a drive or directory for active write events.
///
/// Uses Windows `ReadDirectoryChangesW` with overlapped (async) I/O so that
/// the background thread can be cancelled cleanly without blocking.
///
/// # Usage
///
/// ```ignore
/// let handle = start_monitor(PathBuf::from("C:\\"));
/// // receive events on handle.receiver
/// handle.stop();
/// ```
///
/// # Cancellation
///
/// Set `handle.cancel` to `true` (via `handle.stop()`).  The background thread
/// polls the flag every 200 ms between I/O waits and exits gracefully.
use crossbeam_channel::{bounded, Receiver, Sender};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, warn};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadDirectoryChangesW, FILE_ACTION_ADDED, FILE_ACTION_MODIFIED,
    FILE_ACTION_RENAMED_NEW_NAME, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_NOTIFY_INFORMATION,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

/// Maximum number of unique file entries retained in the monitor state.
///
/// Older entries are evicted when this limit is reached to bound memory usage.
pub const MAX_MONITOR_ENTRIES: usize = 500;

/// A single file-write event record held in the monitor state.
#[derive(Clone, Debug)]
pub struct WriteEvent {
    /// Full path of the file that was written / created / renamed.
    pub path: String,
    /// Number of change events observed for this file since monitoring started.
    pub hit_count: u64,
    /// Wall-clock timestamp of the most recent observed event.
    pub last_seen: chrono::DateTime<chrono::Local>,
}

/// Message sent from the monitor background thread to the UI layer.
pub enum MonitorMessage {
    /// A file was written to, created, or renamed.
    FileChanged(String),
}

/// Handle to a running file-write monitor.
///
/// Call [`MonitorHandle::stop`] to shut down the background thread.
/// The thread exits within ~400 ms of the stop signal.
pub struct MonitorHandle {
    /// Write `true` to request the background thread to exit.
    pub cancel: Arc<AtomicBool>,
    /// Receive [`MonitorMessage`] events from the background thread.
    pub receiver: Receiver<MonitorMessage>,
}

impl MonitorHandle {
    /// Signal the background thread to stop.  Non-blocking.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Start monitoring `path` for file write activity.
///
/// Returns a [`MonitorHandle`] immediately; the monitor runs in a detached
/// background thread.  The caller receives events via `handle.receiver`.
pub fn start_monitor(path: PathBuf) -> MonitorHandle {
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_clone = Arc::clone(&cancel);
    let (tx, rx) = bounded::<MonitorMessage>(2048);

    std::thread::Builder::new()
        .name("disksleuth-monitor".to_owned())
        .spawn(move || {
            run_monitor(path, cancel_clone, tx);
        })
        .expect("failed to spawn monitor thread");

    MonitorHandle {
        cancel,
        receiver: rx,
    }
}

// ─── Background thread ──────────────────────────────────────────────────────

/// Open the directory handle and issue `ReadDirectoryChangesW` in a loop until
/// the cancel flag is set.
fn run_monitor(path: PathBuf, cancel: Arc<AtomicBool>, tx: Sender<MonitorMessage>) {
    debug!("Monitor: starting on {:?}", path);

    // Build a null-terminated UTF-16 path.
    let wide_path: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0u16))
        .collect();

    // Open the directory with FILE_FLAG_OVERLAPPED so IO can be cancelled.
    let dir_handle: HANDLE = unsafe {
        match CreateFileW(
            windows::core::PCWSTR(wide_path.as_ptr()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OVERLAPPED.0),
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                warn!("Monitor: failed to open {:?}: {}", path, e);
                return;
            }
        }
    };

    // Create a manual-reset event for the OVERLAPPED structure.
    let io_event: HANDLE = unsafe {
        match CreateEventW(None, true, false, None) {
            Ok(h) => h,
            Err(e) => {
                warn!("Monitor: failed to create event: {}", e);
                let _ = CloseHandle(dir_handle);
                return;
            }
        }
    };

    let filter =
        FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE;

    let mut buffer = vec![0u8; 65536];

    'outer: loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        // Prepare a fresh OVERLAPPED each iteration.  The kernel event lives
        // for the lifetime of the outer loop, so the OVERLAPPED only needs to
        // survive until the operation completes (or is cancelled) below.
        let mut overlapped = OVERLAPPED::default();
        overlapped.hEvent = io_event;

        // Reset the event before issuing the next request.
        unsafe {
            let _ = ResetEvent(io_event);
        }

        // Issue asynchronous directory-change notification.
        // ERROR_IO_PENDING is the expected "success" return for overlapped I/O;
        // the Result from windows-rs is intentionally discarded here.
        let _ = unsafe {
            ReadDirectoryChangesW(
                dir_handle,
                buffer.as_mut_ptr() as *mut core::ffi::c_void,
                buffer.len() as u32,
                true, // watch subdirectories recursively
                filter,
                None,
                Some(&mut overlapped),
                None,
            )
        };

        // Poll for completion, checking the cancel flag every 200 ms.
        let mut bytes_transferred: u32 = 0;
        loop {
            if cancel.load(Ordering::Relaxed) {
                // Cancel the outstanding IO and drain the event before exiting.
                unsafe {
                    let _ = CancelIoEx(dir_handle, Some(&overlapped));
                    // Wait for cancellation to complete so OVERLAPPED is no
                    // longer referenced by the kernel before it goes out of scope.
                    WaitForSingleObject(io_event, 5000);
                    let _ = CloseHandle(io_event);
                    let _ = CloseHandle(dir_handle);
                }
                debug!("Monitor: cancelled for {:?}", path);
                return;
            }

            let wait = unsafe { WaitForSingleObject(io_event, 200) };

            if wait.0 == 0 {
                // WAIT_OBJECT_0 — IO completed.
                let result = unsafe {
                    GetOverlappedResult(dir_handle, &mut overlapped, &mut bytes_transferred, false)
                };
                if result.is_err() {
                    // Typically overflow (buffer too small) — skip and retry.
                    bytes_transferred = 0;
                }
                break;
            } else if wait.0 == 0x102 {
                // WAIT_TIMEOUT — 200 ms elapsed with no activity; re-check cancel flag.
                continue;
            } else {
                // Error or handle closed unexpectedly.
                warn!("Monitor: WaitForSingleObject returned unexpected value");
                break 'outer;
            }
        }

        if bytes_transferred == 0 {
            // Overflow or spurious wakeup — re-issue the request.
            continue;
        }

        // ── Parse FILE_NOTIFY_INFORMATION records ──────────────────────
        parse_and_send_events(&buffer, bytes_transferred as usize, &path, &tx);
    }

    // Normal exit — clean up handles.
    unsafe {
        let _ = CloseHandle(io_event);
        let _ = CloseHandle(dir_handle);
    }

    debug!("Monitor: stopped for {:?}", path);
}

/// Parse a contiguous `FILE_NOTIFY_INFORMATION` chain from `buffer` and send
/// relevant events to `tx`.
fn parse_and_send_events(
    buffer: &[u8],
    total_bytes: usize,
    base_path: &PathBuf,
    tx: &Sender<MonitorMessage>,
) {
    let mut offset = 0usize;
    let base = base_path.to_string_lossy();
    let base = base.trim_end_matches(['\\', '/']);

    loop {
        // Bounds check before casting.
        let record_min = std::mem::size_of::<FILE_NOTIFY_INFORMATION>();
        if offset + record_min > total_bytes {
            break;
        }

        // SAFETY: `buffer` is a valid byte slice of at least `total_bytes` bytes
        // filled by the kernel with correctly aligned FILE_NOTIFY_INFORMATION records.
        let fni = unsafe { &*(buffer.as_ptr().add(offset) as *const FILE_NOTIFY_INFORMATION) };

        // Extract the variable-length UTF-16 filename that follows the struct.
        let name_chars = fni.FileNameLength as usize / 2;
        // SAFETY: `FileName` is immediately followed by (name_chars - 1) additional
        // u16 code units in the same kernel-allocated buffer.
        let name_slice =
            unsafe { std::slice::from_raw_parts(&fni.FileName as *const u16, name_chars) };
        let relative_name = String::from_utf16_lossy(name_slice);
        let full_path = format!("{}\\{}", base, relative_name.replace('/', "\\"));

        // Only report creations, modifications, and renames (new name).
        if fni.Action == FILE_ACTION_ADDED
            || fni.Action == FILE_ACTION_MODIFIED
            || fni.Action == FILE_ACTION_RENAMED_NEW_NAME
        {
            let _ = tx.try_send(MonitorMessage::FileChanged(full_path));
        }

        if fni.NextEntryOffset == 0 {
            break;
        }
        offset += fni.NextEntryOffset as usize;
    }
}
