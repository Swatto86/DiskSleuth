/// Shell file operations — Recycle Bin deletion via `SHFileOperationW`.
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::UI::Shell::{
    SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FO_DELETE, SHFILEOPSTRUCTW,
};

/// Flags for the Recycle Bin delete.
///
/// `FOF_SILENT` is deliberately NOT set: the caller blocks the render thread
/// for the whole operation, so the shell's progress dialog is the only
/// feedback (and the only cancel) the user gets.
const DELETE_FLAGS: u16 = (FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_NOERRORUI.0) as u16;

/// Move a file or directory (recursively) to the Recycle Bin.
///
/// Uses the shell's `FOF_ALLOWUNDO` delete so the item can be restored by
/// the user. No confirmation prompt is shown — callers are expected to confirm
/// with the user first — but the shell's own progress dialog is allowed to
/// appear for operations long enough to need one. On volumes without a
/// Recycle Bin (e.g. network shares) the shell deletes permanently, mirroring
/// Explorer.
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
        fFlags: DELETE_FLAGS,
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

    /// The shell's progress dialog must not be suppressed: it is the only
    /// feedback and the only cancel a user gets while the render thread is
    /// blocked on a large recursive delete.
    #[test]
    fn progress_ui_is_not_suppressed() {
        use windows::Win32::UI::Shell::FOF_SILENT;
        assert_eq!(
            DELETE_FLAGS & FOF_SILENT.0 as u16,
            0,
            "FOF_SILENT must not be set — it hides the only progress UI"
        );
        assert_ne!(DELETE_FLAGS & FOF_ALLOWUNDO.0 as u16, 0);
        assert_ne!(DELETE_FLAGS & FOF_NOCONFIRMATION.0 as u16, 0);
        assert_ne!(DELETE_FLAGS & FOF_NOERRORUI.0 as u16, 0);
    }

    /// A missing path errors instead of silently succeeding.
    #[test]
    fn recycle_missing_path_errors() {
        assert!(move_to_recycle_bin(Path::new("Z:\\definitely\\not\\here.bin")).is_err());
    }
}
