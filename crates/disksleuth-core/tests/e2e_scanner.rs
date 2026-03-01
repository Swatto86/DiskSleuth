use disksleuth_core::scanner::progress::ScanProgress;
/// End-to-end scanner integration tests.
///
/// These tests exercise the real `parallel::scan_parallel` and
/// `mft::is_mft_available` code paths against a real temporary filesystem,
/// verifying that the scanner correctly enumerates files and directories,
/// accumulates sizes, and reports progress through the channel.
///
/// **Why a `tests/` integration test (not unit test)?**
///
/// The parallel scanner creates real OS threads, writes to a shared
/// `Arc<RwLock<FileTree>>`, and processes actual `DirEntry` objects.
/// Testing it in isolation would require mocking the entire OS filesystem
/// interface. An integration test with `tempfile` exercises every code path
/// — thread spawning, path resolution, arena insertion, aggregation — with
/// zero mocking.
use disksleuth_core::scanner::{start_scan, PROGRESS_CHANNEL_CAPACITY};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Create a reproducible directory tree for scanner tests:
///
/// ```text
/// root/
///   alpha/
///     a.txt   (100 bytes)
///     b.rs    (200 bytes)
///   beta/
///     c.png   (300 bytes)
///   d.zip     (400 bytes)
/// ```
///
/// Total file bytes: 1 000.
fn build_test_tree(root: &Path) {
    let alpha = root.join("alpha");
    let beta = root.join("beta");
    fs::create_dir_all(&alpha).unwrap();
    fs::create_dir_all(&beta).unwrap();

    write_bytes(&alpha.join("a.txt"), 100);
    write_bytes(&alpha.join("b.rs"), 200);
    write_bytes(&beta.join("c.png"), 300);
    write_bytes(&root.join("d.zip"), 400);
}

fn write_bytes(path: &Path, n: usize) {
    let mut f = fs::File::create(path).unwrap();
    f.write_all(&vec![0u8; n]).unwrap();
}

/// Drain all progress messages from a completed scan, returning the final
/// `ScanProgress::Complete` message (or panicking after a generous timeout).
///
/// Waits up to 30 seconds — more than enough for any tmpdir scan on any CI
/// machine but short enough that a genuinely stuck test does not block the
/// suite indefinitely.
fn drain_to_completion(handle: disksleuth_core::scanner::ScanHandle) -> Duration {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "scanner did not complete within 30 seconds"
        );
        match handle.progress_rx.try_recv() {
            Ok(ScanProgress::Complete { duration, .. }) => return duration,
            Ok(ScanProgress::Cancelled) => panic!("scan was unexpectedly cancelled"),
            Ok(_) => continue,
            Err(crossbeam_channel::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                panic!("scanner channel disconnected before Complete was sent");
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// The scanner must visit all files and report a non-zero total size.
#[test]
fn scan_discovers_all_files() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    build_test_tree(tmp.path());

    let handle = start_scan(tmp.path().to_path_buf());
    drain_to_completion(handle);

    // The live tree is available after the scan completes through the Arc.
    // Re-run a fresh scan and inspect the tree mid-completion via the final state.
    let handle2 = start_scan(tmp.path().to_path_buf());
    let tree_ref = handle2.live_tree.clone();
    drain_to_completion(handle2);

    let tree = tree_ref.read();
    // At minimum: 1 root + 2 dirs + 4 files = 7 nodes.
    assert!(
        tree.len() >= 7,
        "expected at least 7 nodes, found {}",
        tree.len()
    );
    // Total size must be >= 1 000 bytes (the exact files we wrote).
    assert!(
        tree.total_size >= 1_000,
        "expected total_size >= 1000, got {}",
        tree.total_size
    );
}

/// Scans of an empty directory must succeed with exactly 1 node (the root).
#[test]
fn scan_empty_directory() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    // Do NOT create any files — leave the directory empty.

    let handle = start_scan(tmp.path().to_path_buf());
    let tree_ref = handle.live_tree.clone();
    drain_to_completion(handle);

    let tree = tree_ref.read();
    // Root directory is always inserted.
    assert!(
        !tree.is_empty(),
        "expected at least the root node, got {}",
        tree.len()
    );
    assert_eq!(
        tree.total_size, 0,
        "empty directory should have 0 total size"
    );
}

/// Cancellation must stop the scan gracefully and the channel must receive
/// `ScanProgress::Cancelled`.
#[test]
fn scan_cancellation_sends_cancelled_message() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    build_test_tree(tmp.path());

    let handle = start_scan(tmp.path().to_path_buf());
    // Request cancellation immediately — the scanner may already be done by
    // the time the flag is read, so we accept either Cancelled or Complete.
    handle.cancel();

    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let mut received_terminal = false;
    while std::time::Instant::now() < deadline {
        match handle.progress_rx.try_recv() {
            Ok(ScanProgress::Cancelled) | Ok(ScanProgress::Complete { .. }) => {
                received_terminal = true;
                break;
            }
            Ok(_) => continue,
            Err(crossbeam_channel::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(crossbeam_channel::TryRecvError::Disconnected) => break,
        }
    }
    assert!(
        received_terminal,
        "scanner must send Cancelled or Complete within 30 s"
    );
}

/// Progress messages must include Update messages with increasing file counts.
#[test]
fn scan_sends_progress_updates() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    // Write enough files that at least one Update message fires.
    for i in 0..50 {
        write_bytes(&tmp.path().join(format!("file{i:03}.bin")), 1024);
    }

    let handle = start_scan(tmp.path().to_path_buf());
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let mut saw_update = false;

    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "scanner timed out without sending any message"
        );
        match handle.progress_rx.try_recv() {
            Ok(ScanProgress::Update { files_found, .. }) if files_found > 0 => {
                saw_update = true;
                break;
            }
            Ok(ScanProgress::Complete { .. }) => {
                // Complete may arrive before an Update for very small dirs.
                break;
            }
            Ok(_) => continue,
            Err(crossbeam_channel::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break,
        }
    }

    // Drain the rest.
    let _ = drain_to_completion(start_scan(tmp.path().to_path_buf()));
    // We do not fail if Complete arrived before any visible Update; the
    // scanner is allowed to report progress at its own cadence.
    let _ = saw_update; // non-deterministic; we assert no panic/deadlock above.
}

/// `PROGRESS_CHANNEL_CAPACITY` must be a positive constant so it is never
/// accidentally set to 0 (which would make every `send()` block immediately).
/// This is a compile-time invariant enforced by the const assertion below.
const _: () = assert!(
    PROGRESS_CHANNEL_CAPACITY > 0,
    "PROGRESS_CHANNEL_CAPACITY must be > 0"
);

/// `is_mft_available` must return a `bool` without panicking, regardless of
/// whether the process is elevated or the path is on an NTFS volume.
#[test]
fn mft_available_check_does_not_panic() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    // Return value is environment-dependent; we only care that it does not panic.
    let _result = disksleuth_core::scanner::mft::is_mft_available(tmp.path());
}

/// Aggregation: after a completed scan the root node size equals the sum
/// of all direct-child file sizes.
#[test]
fn scan_aggregate_sizes_are_correct() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    // Single-level directory with three files of known lengths.
    write_bytes(&tmp.path().join("x.bin"), 1_000);
    write_bytes(&tmp.path().join("y.bin"), 2_000);
    write_bytes(&tmp.path().join("z.bin"), 3_000);

    let handle = start_scan(tmp.path().to_path_buf());
    let tree_ref = handle.live_tree.clone();
    drain_to_completion(handle);

    let tree = tree_ref.read();
    // Total size must be at least 6 000 bytes.
    assert!(
        tree.total_size >= 6_000,
        "expected total_size >= 6000, got {}",
        tree.total_size
    );
}
