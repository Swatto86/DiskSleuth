/// Shell file operations — Recycle Bin deletion via `SHFileOperationW`.
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::UI::Shell::{
    SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
    SHFILEOPSTRUCTW,
};

/// Move a file or directory (recursively) to the Recycle Bin.
///
/// Uses the shell's `FOF_ALLOWUNDO` delete so the item can be restored by
/// the user. No confirmation or progress UI is shown — callers are expected
/// to confirm with the user first. On volumes without a Recycle Bin (e.g.
/// network shares) the shell deletes permanently, mirroring Explorer.
pub fn move_to_recycle_bin(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!("path does not exist: {}", path.display());
    }

    // SHFileOperationW expects a double-null-terminated UTF-16 list.
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    wide.push(0);

    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(wide.as_ptr()),
        fFlags: (FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_SILENT.0 | FOF_NOERRORUI.0) as u16,
        ..Default::default()
    };

    let result = unsafe { SHFileOperationW(&mut op) };
    if result != 0 {
        anyhow::bail!(
            "shell delete failed for {} (error 0x{result:X})",
            path.display()
        );
    }
    if op.fAnyOperationsAborted.as_bool() {
        anyhow::bail!("delete was aborted for {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recycling a real temp file succeeds and removes it from its location.
    #[test]
    fn recycle_removes_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("recycle_me.txt");
        std::fs::write(&file, b"bye").unwrap();

        move_to_recycle_bin(&file).expect("recycle must succeed");
        assert!(!file.exists(), "file must be gone from original location");
    }

    /// Recycling a directory removes it and its contents.
    #[test]
    fn recycle_removes_directory_recursively() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("dir_to_recycle");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("inner.txt"), b"x").unwrap();

        move_to_recycle_bin(&dir).expect("recycle must succeed");
        assert!(!dir.exists());
    }

    /// A missing path errors instead of silently succeeding.
    #[test]
    fn recycle_missing_path_errors() {
        assert!(move_to_recycle_bin(Path::new("Z:\\definitely\\not\\here.bin")).is_err());
    }
}
