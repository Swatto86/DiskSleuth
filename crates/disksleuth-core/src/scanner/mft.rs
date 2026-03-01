/// MFT (Master File Table) direct reader — the Tier 1 (fastest) scanner.
///
/// Uses Windows IOCTLs to enumerate every file record in the NTFS Master File
/// Table without traversing directories. This gives scan times comparable to
/// WinDirStat/Everything (2–10 seconds for millions of files) because:
///
/// 1. **No directory traversal** — reads the flat MFT record array sequentially.
/// 2. **Minimal I/O** — the MFT is a contiguous on-disk structure; sequential
///    reads are far faster than random `FindFirstFile` / `FindNextFile` calls.
/// 3. **No per-file metadata round-trip** — file name, size, and parent ref are
///    all embedded in each USN record.
///
/// # Requirements
///
/// - The volume must be NTFS.
/// - The process must be running with **administrator** privileges (required to
///   open `\\.\C:` with `GENERIC_READ`).
///
/// # Algorithm
///
/// 1. Open the raw volume handle (`\\.\X:`).
/// 2. `FSCTL_GET_NTFS_VOLUME_DATA` → get MFT size metadata.
/// 3. `FSCTL_ENUM_USN_DATA` in a loop → yields `(FileReferenceNumber,
///    ParentFileReferenceNumber, FileName, FileAttributes)` for every record.
/// 4. Build a `HashMap<u64, NodeIndex>` mapping MFT reference → tree node.
/// 5. Wire up parent → child relationships.
/// 6. Stat files for sizes, then run `FileTree::aggregate_sizes()`.
use crate::model::{FileNode, FileTree, NodeIndex};
use crate::platform::permissions::is_elevated;
use crate::scanner::progress::ScanProgress;
use crate::scanner::LiveTree;
use compact_str::CompactString;
use crossbeam_channel::Sender;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetVolumeInformationW, FILE_ATTRIBUTE_DIRECTORY, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Ioctl::{
    FSCTL_ENUM_USN_DATA, FSCTL_GET_NTFS_VOLUME_DATA, NTFS_VOLUME_DATA_BUFFER,
};

/// Maximum number of MFT records buffered before the scan is truncated.
///
/// A typical consumer NTFS volume has 200k–2M records. Drives with tens of
/// millions of small files (mail servers, source-control repos, etc.) can
/// exceed this bound. When hit, the scan is terminated gracefully rather than
/// exhausting heap memory.
pub const MAX_MFT_RECORDS: usize = 15_000_000;

/// Check whether MFT direct reading is available for the given path.
///
/// Requirements:
/// 1. The path must be a drive root (e.g. `C:\`).
/// 2. The volume must be NTFS.
/// 3. The process must be running with administrative privileges.
pub fn is_mft_available(path: &Path) -> bool {
    if !is_elevated() {
        tracing::debug!("MFT not available: process is not elevated");
        return false;
    }

    let path_str = path.to_string_lossy();
    if path_str.len() < 2 || !path_str.as_bytes()[0].is_ascii_alphabetic() {
        return false;
    }

    // Check filesystem is NTFS.
    let root: String = format!("{}\\", &path_str[..2]);
    let root_wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    let mut fs_buf = [0u16; 64];

    let has_info = unsafe {
        GetVolumeInformationW(
            PCWSTR(root_wide.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut fs_buf),
        )
        .is_ok()
    };

    if !has_info {
        return false;
    }

    let fs_name =
        String::from_utf16_lossy(&fs_buf[..fs_buf.iter().position(|&c| c == 0).unwrap_or(0)]);

    if fs_name != "NTFS" {
        tracing::debug!("MFT not available: filesystem is {fs_name}, not NTFS");
        return false;
    }

    // Try to open the volume handle to confirm access.
    let vol_path = format!("\\\\.\\{}:", &path_str[..1]);
    let vol_wide: Vec<u16> = vol_path.encode_utf16().chain(std::iter::once(0)).collect();

    let handle = unsafe {
        CreateFileW(
            PCWSTR(vol_wide.as_ptr()),
            0x80000000, // GENERIC_READ
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            Default::default(),
            None,
        )
    };

    match handle {
        Ok(h) => {
            unsafe {
                let _ = CloseHandle(h);
            }
            tracing::info!("MFT available for volume {}", &path_str[..2]);
            true
        }
        Err(e) => {
            tracing::debug!("MFT not available: cannot open volume handle: {e}");
            false
        }
    }
}

/// Scan using MFT direct reading via `FSCTL_ENUM_USN_DATA`.
///
/// Builds the full `FileTree` from MFT records and sends it via the progress
/// channel.
pub fn scan_mft(
    root_path: PathBuf,
    progress_tx: Sender<ScanProgress>,
    cancel_flag: Arc<AtomicBool>,
    live_tree: LiveTree,
) {
    let start = Instant::now();
    let path_str = root_path.to_string_lossy();
    let drive_letter = &path_str[..1];

    // Open raw volume handle.
    let vol_path = format!("\\\\.\\{drive_letter}:");
    let vol_wide: Vec<u16> = vol_path.encode_utf16().chain(std::iter::once(0)).collect();

    let handle = match unsafe {
        CreateFileW(
            PCWSTR(vol_wide.as_ptr()),
            0x80000000, // GENERIC_READ
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            Default::default(),
            None,
        )
    } {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("Failed to open volume {vol_path}: {e}");
            let _ = progress_tx.send(ScanProgress::Error {
                path: vol_path,
                message: format!("Failed to open volume: {e}"),
            });
            return;
        }
    };

    // Step 1: Get NTFS volume data.
    let vol_data = match get_ntfs_volume_data(handle) {
        Some(vd) => vd,
        None => {
            tracing::error!("Failed to get NTFS volume data");
            let _ = progress_tx.send(ScanProgress::Error {
                path: vol_path,
                message: "FSCTL_GET_NTFS_VOLUME_DATA failed".into(),
            });
            unsafe {
                let _ = CloseHandle(handle);
            }
            return;
        }
    };

    tracing::info!(
        "NTFS volume data: MFT records ~{}, bytes_per_file_record={}",
        vol_data.MftValidDataLength / vol_data.BytesPerFileRecordSegment as i64,
        vol_data.BytesPerFileRecordSegment,
    );

    // Step 2: Enumerate all USN records (MFT entries).
    let mut records: Vec<MftEntry> = Vec::with_capacity(2_000_000);
    let mut files_found: u64 = 0;
    let mut dirs_found: u64 = 0;

    // MFT_ENUM_DATA_V0: { StartFileReferenceNumber: u64, LowUsn: i64, HighUsn: i64 }
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct MftEnumDataV0 {
        start_file_reference_number: u64,
        low_usn: i64,
        high_usn: i64,
    }

    let mut enum_data = MftEnumDataV0 {
        start_file_reference_number: 0,
        low_usn: 0,
        high_usn: i64::MAX,
    };

    // 256 KB gives 4x more records per DeviceIoControl call vs the old 64 KB,
    // reducing syscall overhead on high-inode volumes by up to 75%.
    let mut output_buf = vec![0u8; 256 * 1024];
    let mut update_counter: u64 = 0;

    // The label allows the inner USN-record parsing loop to break all the way
    // out of the outer DeviceIoControl loop when the record cap is hit.
    'mft_enum: loop {
        if cancel_flag.load(Ordering::Relaxed) {
            unsafe {
                let _ = CloseHandle(handle);
            }
            let _ = progress_tx.send(ScanProgress::Cancelled);
            return;
        }

        let mut bytes_returned: u32 = 0;
        let ok = unsafe {
            windows::Win32::System::IO::DeviceIoControl(
                handle,
                FSCTL_ENUM_USN_DATA,
                Some(&enum_data as *const MftEnumDataV0 as *const std::ffi::c_void),
                std::mem::size_of::<MftEnumDataV0>() as u32,
                Some(output_buf.as_mut_ptr() as *mut std::ffi::c_void),
                output_buf.len() as u32,
                Some(&mut bytes_returned),
                None,
            )
        };

        if ok.is_err() || bytes_returned <= 8 {
            break; // End of MFT data (ERROR_HANDLE_EOF).
        }

        // First 8 bytes = next StartFileReferenceNumber.
        let next_start = u64::from_le_bytes(output_buf[0..8].try_into().unwrap());

        // Parse USN_RECORD_V2 entries after the 8-byte header.
        let mut offset = 8usize;
        while offset < bytes_returned as usize {
            if offset + 4 > bytes_returned as usize {
                break;
            }

            let record_len =
                u32::from_le_bytes(output_buf[offset..offset + 4].try_into().unwrap()) as usize;

            if record_len == 0 || offset + record_len > bytes_returned as usize {
                break;
            }

            // USN_RECORD_V2 layout:
            //   0: RecordLength (u32)
            //   4: MajorVersion (u16)
            //   6: MinorVersion (u16)
            //   8: FileReferenceNumber (u64)
            //  16: ParentFileReferenceNumber (u64)
            //  24: Usn (i64)
            //  32: TimeStamp (i64)
            //  40: Reason (u32)
            //  44: SourceInfo (u32)
            //  48: SecurityId (u32)
            //  52: FileAttributes (u32)
            //  56: FileNameLength (u16)
            //  58: FileNameOffset (u16)
            //  60: FileName (variable, UTF-16)
            if record_len < 60 {
                offset += record_len;
                continue;
            }

            let base = offset;
            let file_ref = u64::from_le_bytes(output_buf[base + 8..base + 16].try_into().unwrap());
            let parent_ref =
                u64::from_le_bytes(output_buf[base + 16..base + 24].try_into().unwrap());
            let file_attrs =
                u32::from_le_bytes(output_buf[base + 52..base + 56].try_into().unwrap());
            let name_len =
                u16::from_le_bytes(output_buf[base + 56..base + 58].try_into().unwrap()) as usize;
            let name_offset =
                u16::from_le_bytes(output_buf[base + 58..base + 60].try_into().unwrap()) as usize;

            let name_start = base + name_offset;
            let name_end = name_start + name_len;

            if name_end > base + record_len || name_len == 0 {
                offset += record_len;
                continue;
            }

            // Decode UTF-16 directly into a CompactString without an
            // intermediate Vec<u16> or String allocation.
            // For typical filenames (≤15 bytes UTF-8) CompactString stores
            // the result inline with no heap allocation at all.
            let file_name: CompactString = char::decode_utf16(
                output_buf[name_start..name_end]
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]])),
            )
            .map(|r| r.unwrap_or('\u{FFFD}'))
            .collect();

            let is_dir = (file_attrs & FILE_ATTRIBUTE_DIRECTORY.0) != 0;

            // Mask to 48-bit MFT reference (lower 48 bits = record number).
            let file_ref_48 = file_ref & 0x0000_FFFF_FFFF_FFFF;
            let parent_ref_48 = parent_ref & 0x0000_FFFF_FFFF_FFFF;

            // Guard against unbounded memory growth on pathologically large
            // volumes (e.g. mail servers with tens of millions of tiny files).
            if records.len() >= MAX_MFT_RECORDS {
                tracing::warn!("MFT record limit ({MAX_MFT_RECORDS}) reached — truncating scan");
                let _ = progress_tx.send(ScanProgress::Error {
                    path: root_path.to_string_lossy().to_string(),
                    message: format!(
                        "Scan truncated: volume contains more than {MAX_MFT_RECORDS} MFT records. \
                         Only the first {MAX_MFT_RECORDS} entries are shown."
                    ),
                });
                // Exit the outer DeviceIoControl loop, not just this inner
                // USN-record parsing loop.
                break 'mft_enum;
            }

            records.push(MftEntry {
                file_ref: file_ref_48,
                parent_ref: parent_ref_48,
                file_name,
                is_dir,
            });

            if is_dir {
                dirs_found += 1;
            } else {
                files_found += 1;
            }

            update_counter += 1;
            if update_counter.is_multiple_of(50000) {
                let _ = progress_tx.send(ScanProgress::Update {
                    files_found,
                    dirs_found,
                    total_size: 0,
                    current_path: format!("Reading MFT... {} records", update_counter),
                });
            }

            offset += record_len;
        }

        enum_data.start_file_reference_number = next_start;
    }

    unsafe {
        let _ = CloseHandle(handle);
    }

    tracing::info!(
        "MFT enumeration complete: {} records ({} files, {} dirs) in {:?}",
        records.len(),
        files_found,
        dirs_found,
        start.elapsed(),
    );

    let _ = progress_tx.send(ScanProgress::Update {
        files_found,
        dirs_found,
        total_size: 0,
        current_path: "Building file tree from MFT records...".into(),
    });

    // Step 3: Build the FileTree from MFT records.
    let root_display = format!("{}:", drive_letter.to_uppercase());
    let (tree, error_count) =
        build_tree_from_mft(&records, &root_display, &root_path, &progress_tx);

    if cancel_flag.load(Ordering::Relaxed) {
        let _ = progress_tx.send(ScanProgress::Cancelled);
        return;
    }

    let duration = start.elapsed();
    tracing::info!("MFT scan complete. {} nodes in {:?}", tree.len(), duration);

    // Move the completed tree into the shared LiveTree.
    {
        let mut shared = live_tree.write();
        *shared = tree;
    }

    let _ = progress_tx.send(ScanProgress::Complete {
        duration,
        error_count,
    });
}

// ── Internal types ──────────────────────────────────────────────────

/// A raw MFT entry before it's inserted into the FileTree.
///
/// `file_name` uses [`CompactString`] so that short filenames (the common
/// case, ≤15 bytes) are stored inline without a heap allocation.
struct MftEntry {
    file_ref: u64,
    parent_ref: u64,
    file_name: CompactString,
    is_dir: bool,
}

// ── Internal helpers ────────────────────────────────────────────────

/// Get NTFS volume metadata via `FSCTL_GET_NTFS_VOLUME_DATA`.
fn get_ntfs_volume_data(handle: HANDLE) -> Option<NTFS_VOLUME_DATA_BUFFER> {
    let mut vol_data: NTFS_VOLUME_DATA_BUFFER = unsafe { std::mem::zeroed() };
    let mut bytes_returned: u32 = 0;

    let ok = unsafe {
        windows::Win32::System::IO::DeviceIoControl(
            handle,
            FSCTL_GET_NTFS_VOLUME_DATA,
            None,
            0,
            Some(&mut vol_data as *mut NTFS_VOLUME_DATA_BUFFER as *mut std::ffi::c_void),
            std::mem::size_of::<NTFS_VOLUME_DATA_BUFFER>() as u32,
            Some(&mut bytes_returned),
            None,
        )
    };

    if ok.is_ok() {
        Some(vol_data)
    } else {
        None
    }
}

/// Build a `FileTree` from raw MFT records.
///
/// Strategy:
/// 1. Create all nodes upfront (one per MFT record).
/// 2. Map `file_ref → NodeIndex` in a HashMap.
/// 3. Wire parent→child relationships using `parent_ref` lookups.
/// 4. Stat files for sizes in parallel with rayon (USN records omit file size).
/// 5. Run `aggregate_sizes()`.
fn build_tree_from_mft(
    records: &[MftEntry],
    root_display: &str,
    root_path: &Path,
    progress_tx: &Sender<ScanProgress>,
) -> (FileTree, u64) {
    let mut tree = FileTree::with_capacity(records.len() + 1);
    let mut error_count: u64 = 0;

    let root_idx = tree.add_root(CompactString::new(root_display));

    let mut ref_to_idx: HashMap<u64, NodeIndex> = HashMap::with_capacity(records.len() + 1);

    // The NTFS root directory has MFT reference number 5.
    const NTFS_ROOT_MFT_REF: u64 = 5;
    ref_to_idx.insert(NTFS_ROOT_MFT_REF, root_idx);

    // Phase A: Create all nodes without parent wiring.
    for entry in records {
        // Skip NTFS metafiles (MFT refs 0–23 are reserved) and the
        // root directory itself (ref 5, already created).
        if entry.file_ref <= 23 {
            continue;
        }

        // Skip NTFS system streams ($MFT, $Bitmap, etc.).
        if entry.file_name.starts_with('$') {
            continue;
        }

        let node = if entry.is_dir {
            // Clone is cheap: CompactString clones inline for short names.
            FileNode::new_dir(entry.file_name.clone(), None)
        } else {
            FileNode::new_file(entry.file_name.clone(), 0, None)
        };

        let idx = tree.add_node(node);
        ref_to_idx.insert(entry.file_ref, idx);
    }

    // Phase B: Wire parent→child relationships.
    for entry in records {
        if entry.file_ref <= 23 || entry.file_name.starts_with('$') {
            continue;
        }

        let child_idx = match ref_to_idx.get(&entry.file_ref) {
            Some(&idx) => idx,
            None => continue,
        };

        let parent_idx = match ref_to_idx.get(&entry.parent_ref) {
            Some(&idx) => idx,
            None => root_idx, // orphan → attach to root
        };

        tree.nodes[child_idx.idx()].parent = Some(parent_idx);
        tree.add_child(parent_idx, child_idx);
    }

    // Phase C: Stat files for sizes. USN records don't include file size,
    // so we read metadata from the filesystem. This is still faster than
    // a full directory walk because we skip enumeration entirely.
    //
    // Parallelised with rayon: `full_path` is read-only, `fs::metadata` is
    // a syscall that benefits from concurrent execution on SSDs/NVMe.
    // Results are written back in a single sequential pass.
    let total_files = tree.nodes.iter().filter(|n| !n.is_dir).count();
    let _ = progress_tx.send(ScanProgress::Update {
        files_found: total_files as u64,
        dirs_found: 0,
        total_size: 0,
        current_path: format!("Reading file sizes... 0/{total_files}"),
    });

    // Collect file indices (read-only pass, no allocation per node).
    let file_indices: Vec<usize> = (0..tree.nodes.len())
        .filter(|&i| !tree.nodes[i].is_dir)
        .collect();

    // Parallel stat: (index, size, allocated_size, modified, is_error)
    // `tree` is borrowed immutably here; `full_path` only reads nodes.
    use rayon::prelude::*;
    let stats: Vec<(usize, u64, Option<std::time::SystemTime>, bool)> = file_indices
        .par_iter()
        .map(|&i| {
            let rel_path = tree.full_path(NodeIndex::new(i));
            let full_path = if let Some(remainder) = rel_path.strip_prefix(root_display) {
                let remainder = remainder.trim_start_matches('\\');
                if remainder.is_empty() {
                    format!("{}\\", root_display)
                } else {
                    format!("{}\\{}", root_display, remainder)
                }
            } else {
                format!(
                    "{}\\{}",
                    root_path.to_string_lossy().trim_end_matches('\\'),
                    &rel_path
                )
            };
            match std::fs::metadata(&full_path) {
                Ok(meta) => (i, meta.len(), meta.modified().ok(), false),
                Err(_) => (i, 0u64, None, true),
            }
        })
        .collect();

    // Sequential write-back pass.
    let mut phase_c_errors: u64 = 0;
    for (i, size, modified, is_error) in stats {
        tree.nodes[i].size = size;
        tree.nodes[i].allocated_size = size;
        tree.nodes[i].modified = modified;
        if is_error {
            // Mark the node so the UI can render the error icon and
            // the details panel can show the "access denied" badge.
            tree.nodes[i].is_error = true;
            phase_c_errors += 1;
        }
    }
    error_count += phase_c_errors;

    // Phase D: Aggregate sizes bottom-up.
    tree.aggregate_sizes();

    (tree, error_count)
}
