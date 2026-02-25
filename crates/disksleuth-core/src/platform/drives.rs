/// Drive enumeration using the Windows API.
///
/// Lists all available drives with their type, label, total/free space,
/// and filesystem name.
use crate::model::size;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
};

// Drive type constants from the Windows API.
const DRIVE_REMOVABLE_VAL: u32 = 2;
const DRIVE_FIXED_VAL: u32 = 3;
const DRIVE_REMOTE_VAL: u32 = 4;
const DRIVE_CDROM_VAL: u32 = 5;

/// Information about a single drive.
#[derive(Debug, Clone)]
pub struct DriveInfo {
    /// Mount point path, e.g. "C:\".
    pub path: PathBuf,
    /// Drive letter, e.g. "C:".
    pub letter: String,
    /// Human-readable drive type.
    pub drive_type: DriveType,
    /// Volume label (e.g. "Windows", "Data").
    pub label: String,
    /// Filesystem name (e.g. "NTFS", "FAT32").
    pub filesystem: String,
    /// Total capacity in bytes.
    pub total_bytes: u64,
    /// Free space in bytes.
    pub free_bytes: u64,
    /// Used space in bytes.
    pub used_bytes: u64,
    /// Usage percentage (0.0–100.0).
    pub usage_percent: f32,
    /// Formatted total size string.
    pub total_display: String,
    /// Formatted free size string.
    pub free_display: String,
    /// Formatted used size string.
    pub used_display: String,
}

/// Drive type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveType {
    Fixed,
    Removable,
    Network,
    CdRom,
    Unknown,
}

impl DriveType {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Fixed => "Fixed",
            Self::Removable => "Removable",
            Self::Network => "Network",
            Self::CdRom => "CD-ROM",
            Self::Unknown => "Unknown",
        }
    }
}

/// Enumerate all available local drives on the system.
///
/// Network/remote drives are excluded — only fixed, removable, and
/// optical drives are returned.
///
/// Returns an empty vec if the Windows API call fails (should not happen
/// on any supported Windows version).
pub fn enumerate_drives() -> Vec<DriveInfo> {
    let mut drives = Vec::new();

    // GetLogicalDriveStringsW returns null-separated drive root strings.
    let mut buffer = [0u16; 256];
    let len = unsafe { GetLogicalDriveStringsW(Some(&mut buffer)) };

    if len == 0 {
        tracing::warn!("GetLogicalDriveStringsW returned 0");
        return drives;
    }

    // Parse the null-separated list of drive roots.
    let full = OsString::from_wide(&buffer[..len as usize]);
    let full_str = full.to_string_lossy();

    for root in full_str.split('\0').filter(|s| !s.is_empty()) {
        let root_wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let root_pcwstr = windows::core::PCWSTR(root_wide.as_ptr());

        // Drive type.
        let raw_type = unsafe { GetDriveTypeW(root_pcwstr) };
        let drive_type = match raw_type {
            DRIVE_FIXED_VAL => DriveType::Fixed,
            DRIVE_REMOVABLE_VAL => DriveType::Removable,
            DRIVE_REMOTE_VAL => DriveType::Network,
            DRIVE_CDROM_VAL => DriveType::CdRom,
            _ => DriveType::Unknown,
        };

        // Skip network/remote drives — only enumerate local drives.
        if drive_type == DriveType::Network {
            continue;
        }

        // Volume information.
        let mut label_buf = [0u16; 256];
        let mut fs_buf = [0u16; 256];
        let has_volume_info = unsafe {
            GetVolumeInformationW(
                root_pcwstr,
                Some(&mut label_buf),
                None,
                None,
                None,
                Some(&mut fs_buf),
            )
            .is_ok()
        };

        let label = if has_volume_info {
            String::from_utf16_lossy(
                &label_buf[..label_buf.iter().position(|&c| c == 0).unwrap_or(0)],
            )
        } else {
            String::new()
        };

        let filesystem = if has_volume_info {
            String::from_utf16_lossy(&fs_buf[..fs_buf.iter().position(|&c| c == 0).unwrap_or(0)])
        } else {
            String::new()
        };

        // Disk space.
        let mut free_caller: u64 = 0;
        let mut total: u64 = 0;
        let mut free_total: u64 = 0;
        let has_space = unsafe {
            GetDiskFreeSpaceExW(
                root_pcwstr,
                Some(&mut free_caller as *mut u64),
                Some(&mut total as *mut u64),
                Some(&mut free_total as *mut u64),
            )
            .is_ok()
        };

        let (total_bytes, free_bytes) = if has_space {
            (total, free_caller)
        } else {
            (0, 0)
        };
        let used_bytes = total_bytes.saturating_sub(free_bytes);
        let usage_percent = if total_bytes > 0 {
            (used_bytes as f64 / total_bytes as f64 * 100.0) as f32
        } else {
            0.0
        };

        let letter = root.trim_end_matches('\\').to_string();

        drives.push(DriveInfo {
            path: PathBuf::from(root),
            letter,
            drive_type,
            label,
            filesystem,
            total_bytes,
            free_bytes,
            used_bytes,
            usage_percent,
            total_display: size::format_size(total_bytes),
            free_display: size::format_size(free_bytes),
            used_display: size::format_size(used_bytes),
        });
    }

    drives
}
