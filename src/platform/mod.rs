pub mod elevation;
pub mod locks;
pub mod service_client;
pub mod volumes;

use std::path::Path;
use std::process::Command;

pub use elevation::{is_elevated, relaunch_as_admin};
pub use locks::{FileLockStatus, inspect_file_lock};
pub use service_client::{
    ServiceStatus, find_service_exe, inspect_file_lock_via_service, install_service_elevated,
    query_service_status, scan_volume_via_service, start_service_elevated,
    uninstall_service_elevated,
};
pub use volumes::{cluster_size_for_path, list_volumes};

/// Opens the OS file manager and highlights `path`.
pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let path_str = path.to_string_lossy();
        let arg = format!("/select,\"{path_str}\"");
        Command::new("explorer.exe")
            .raw_arg(arg)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open Explorer: {e}"))
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open Finder: {e}"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let target = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open file manager: {e}"))
    }
}

/// Moves a file or directory to the OS Recycle Bin / Trash.
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| format!("Failed to move item to Trash: {e}"))
}

/// Permanently deletes a file, symlink, or directory without moving it to the Recycle Bin.
/// Symlinks and NTFS directory junctions are unlinked directly without deleting their target.
pub fn delete_permanently(path: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to inspect item before deletion: {e}"))?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        // On Windows, directory symlinks/junctions require `remove_dir`, while file symlinks use `remove_file`.
        if std::fs::remove_file(path).is_ok() {
            return Ok(());
        }
        return std::fs::remove_dir(path)
            .map_err(|e| format!("Failed to permanently remove symlink: {e}"));
    }
    if ft.is_dir() {
        std::fs::remove_dir_all(path)
            .map_err(|e| format!("Failed to permanently delete folder: {e}"))
    } else {
        std::fs::remove_file(path)
            .map_err(|e| format!("Failed to permanently delete file: {e}"))
    }
}

/// Opens a native OS terminal in `path` (or its parent directory if `path` is a file).
pub fn open_terminal_here(path: &Path) -> Result<(), String> {
    let dir = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };

    #[cfg(target_os = "windows")]
    {
        // Prefer Windows Terminal (`wt.exe`) when installed; fall back to PowerShell.
        if Command::new("wt.exe")
            .arg("-d")
            .arg(dir)
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
        let escaped = dir.to_string_lossy().replace('\'', "''");
        Command::new("powershell.exe")
            .arg("-NoExit")
            .arg("-Command")
            .arg(format!("Set-Location -LiteralPath '{escaped}'"))
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open terminal: {e}"))
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg("-a")
            .arg("Terminal")
            .arg(dir)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open Terminal: {e}"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for (bin, args) in [
            ("x-terminal-emulator", vec!["--working-directory"]),
            ("gnome-terminal", vec!["--working-directory"]),
            ("konsole", vec!["--workdir"]),
        ] {
            let mut cmd = Command::new(bin);
            for a in args {
                cmd.arg(a);
            }
            cmd.arg(dir);
            if cmd.spawn().is_ok() {
                return Ok(());
            }
        }
        Err("No supported terminal emulator found in PATH.".to_string())
    }
}

/// Opens the native Windows Properties dialog for `path`.
pub fn show_properties_dialog(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::{
            SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW, ShellExecuteExW,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;

        let verb: Vec<u16> = "properties\0".encode_utf16().collect();
        let file: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut sei: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        sei.fMask = SEE_MASK_INVOKEIDLIST;
        sei.lpVerb = verb.as_ptr();
        sei.lpFile = file.as_ptr();
        sei.nShow = SW_SHOW;

        let ok = unsafe { ShellExecuteExW(&mut sei) };
        if ok == 0 {
            Err("Failed to open Windows Properties dialog.".to_string())
        } else {
            Ok(())
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        Err("Properties dialog is only supported on Windows.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permanently_deletes_files_and_folders() {
        let base = std::env::temp_dir().join(format!(
            "textree_perm_del_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let sub = base.join("nested");
        std::fs::create_dir_all(&sub).unwrap();
        let f1 = sub.join("a.bin");
        std::fs::write(&f1, b"hello").unwrap();

        delete_permanently(&f1).unwrap();
        assert!(!f1.exists());

        std::fs::write(sub.join("b.bin"), b"world").unwrap();
        delete_permanently(&base).unwrap();
        assert!(!base.exists());
    }
}
