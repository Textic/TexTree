pub mod elevation;
pub mod locks;
pub mod service_client;
pub mod volumes;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub use elevation::{is_elevated, relaunch_as_admin};
pub use locks::{FileLockStatus, inspect_file_lock};
pub use service_client::{
    ServiceStatus, find_service_exe, inspect_file_lock_via_service, install_service_elevated,
    query_service_status, scan_volume_via_service, start_service_elevated,
    uninstall_service_elevated,
};
pub use volumes::{cluster_size_for_path, list_volumes};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedPathWarning {
    pub category: &'static str,
    pub reason: &'static str,
}

/// Checks whether `path` targets or resides inside a critical OS/Windows system directory
/// (such as `Windows`, `Program Files`, `Program Files (x86)`, `ProgramData`,
/// `System Volume Information`, `Boot`, `Recovery`, `Users` root, or core boot/swap files).
///
/// Used to enforce a mandatory confirmation warning 100% of the time regardless of
/// "Don't ask again" user preferences.
pub fn check_protected_system_path(path: &Path) -> Option<ProtectedPathWarning> {
    let raw = path.to_string_lossy();
    let normalized = raw.replace('\\', "/");
    let trimmed = normalized.trim_end_matches('/');
    let lower = trimmed.to_ascii_lowercase();

    // Windows drive-rooted path (`c:/...`)
    let bytes = lower.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let rest = lower.get(2..).unwrap_or("").trim_start_matches('/');
        if rest.is_empty() {
            return Some(ProtectedPathWarning {
                category: "Drive Root",
                reason: "Deleting a drive root is protected to prevent catastrophic data loss.",
            });
        }
        let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
        let first = *segments.first()?;

        return match first {
            "windows" => Some(ProtectedPathWarning {
                category: "Windows System Folder",
                reason: "Deleting files inside Windows can make your operating system unbootable or unstable.",
            }),
            "program files" | "program files (x86)" => Some(ProtectedPathWarning {
                category: "Installed Applications (Program Files)",
                reason: "Deleting files inside Program Files can break installed applications. Use Windows Uninstall when possible.",
            }),
            "programdata" => Some(ProtectedPathWarning {
                category: "System Application Data (ProgramData)",
                reason: "ProgramData stores shared configuration, licenses, and service data for Windows and installed apps.",
            }),
            "system volume information" => Some(ProtectedPathWarning {
                category: "System Restore & Volume Metadata",
                reason: "System Volume Information stores Windows Restore Points, shadow copies, and NTFS metadata.",
            }),
            "boot" | "efi" | "recovery" | "$recycle.bin" => Some(ProtectedPathWarning {
                category: "Windows Boot & Recovery",
                reason: "Deleting boot, EFI, or recovery files can prevent Windows from starting.",
            }),
            "users" if segments.len() <= 2 => Some(ProtectedPathWarning {
                category: "Windows User Profile Folder",
                reason: "Deleting a Windows Users or account profile folder permanently removes Desktop, Documents, AppData, and account settings.",
            }),
            "pagefile.sys" | "hiberfil.sys" | "swapfile.sys" | "bootmgr" | "bootnxt" | "ntldr"
            | "bootsect.bak"
                if segments.len() == 1 =>
            {
                Some(ProtectedPathWarning {
                    category: "Core Windows System File",
                    reason: "Core root system files are managed by Windows for booting, hibernation, and virtual memory.",
                })
            }
            _ => None,
        };
    }

    // Unix / macOS critical OS paths
    if lower.starts_with('/') {
        let segments: Vec<&str> = lower.split('/').filter(|s| !s.is_empty()).collect();
        let first = *segments.first()?;
        return match first {
            "system" | "bin" | "sbin" | "usr" | "etc" | "boot" | "lib" | "lib64" => {
                Some(ProtectedPathWarning {
                    category: "Operating System Folder",
                    reason: "Deleting core operating system files can prevent your computer from booting.",
                })
            }
            _ => None,
        };
    }

    None
}

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

/// Returns `true` if a delete/trash error string indicates OS permission denial
/// (e.g. Windows `ERROR_ACCESS_DENIED` / `os error 5`) where elevating the session may help.
pub fn is_permission_denied_error(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("access is denied")
        || lower.contains("permission denied")
        || lower.contains("os error 5")
        || lower.contains("0x80070005")
        || lower.contains("privilege")
        || lower.contains("requires elevation")
}

/// Moves a file or directory to the OS Recycle Bin / Trash.
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| format!("Failed to move item to Trash: {e}"))
}

#[derive(Debug, Clone)]
pub struct DeleteProgress {
    pub deleted_items: u64,
    pub deleted_bytes: u64,
    pub current_path: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOutcome {
    Completed,
    Cancelled { deleted_items: u64 },
}

/// Permanently deletes a file, symlink, or directory without moving it to the Recycle Bin.
/// Symlinks and NTFS directory junctions are unlinked directly without deleting their target.
#[cfg_attr(not(test), allow(dead_code))]
pub fn delete_permanently(path: &Path) -> Result<(), String> {
    let cancel = AtomicBool::new(false);
    delete_permanently_with_progress(path, &cancel, |_| {}).map(|_| ())
}

/// Iteratively deletes `path` permanently while streaming live `(deleted_items, deleted_bytes, current_path)`
/// updates to `on_progress` and honoring `cancel`. Never follows symlinks or NTFS junctions.
pub fn delete_permanently_with_progress<F>(
    path: &Path,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<DeleteOutcome, String>
where
    F: FnMut(DeleteProgress),
{
    let started = Instant::now();
    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut deleted_items: u64 = 0;
    let mut deleted_bytes: u64 = 0;

    let mut stack: Vec<(PathBuf, bool)> = vec![(path.to_path_buf(), false)];
    let mut emit = |cur: &Path, items: u64, bytes: u64, force: bool| {
        if force || last_emit.elapsed() >= Duration::from_millis(60) {
            last_emit = Instant::now();
            on_progress(DeleteProgress {
                deleted_items: items,
                deleted_bytes: bytes,
                current_path: cur.to_string_lossy().into_owned(),
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        }
    };

    while let Some((cur, dir_ready)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(DeleteOutcome::Cancelled { deleted_items });
        }

        if dir_ready {
            remove_dir_handling_readonly(&cur)?;
            deleted_items = deleted_items.saturating_add(1);
            emit(&cur, deleted_items, deleted_bytes, false);
            continue;
        }

        let meta = std::fs::symlink_metadata(&cur)
            .map_err(|e| format!("Failed to inspect \"{}\" before deletion: {e}", cur.display()))?;
        let ft = meta.file_type();

        if ft.is_symlink() {
            if std::fs::remove_file(&cur).is_err() {
                remove_dir_handling_readonly(&cur)?;
            }
            deleted_items = deleted_items.saturating_add(1);
            emit(&cur, deleted_items, deleted_bytes, false);
        } else if ft.is_dir() {
            emit(&cur, deleted_items, deleted_bytes, false);
            stack.push((cur.clone(), true));
            let entries = std::fs::read_dir(&cur)
                .map_err(|e| format!("Failed to read folder \"{}\": {e}", cur.display()))?;
            for entry in entries {
                let entry = entry.map_err(|e| {
                    format!("Failed enumerating folder \"{}\": {e}", cur.display())
                })?;
                stack.push((entry.path(), false));
            }
        } else {
            let len = meta.len();
            remove_file_handling_readonly(&cur)?;
            deleted_items = deleted_items.saturating_add(1);
            deleted_bytes = deleted_bytes.saturating_add(len);
            emit(&cur, deleted_items, deleted_bytes, false);
        }
    }

    emit(path, deleted_items, deleted_bytes, true);
    Ok(DeleteOutcome::Completed)
}

fn clear_readonly(path: &Path) {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
}

fn remove_file_handling_readonly(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            clear_readonly(path);
            std::fs::remove_file(path)
                .map_err(|e| format!("Failed to permanently delete \"{}\": {e}", path.display()))
        }
        Err(err) => Err(format!(
            "Failed to permanently delete \"{}\": {err}",
            path.display()
        )),
    }
}

fn remove_dir_handling_readonly(path: &Path) -> Result<(), String> {
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            clear_readonly(path);
            std::fs::remove_dir(path)
                .map_err(|e| format!("Failed to permanently delete folder \"{}\": {e}", path.display()))
        }
        Err(err) => Err(format!(
            "Failed to permanently delete folder \"{}\": {err}",
            path.display()
        )),
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
        let cancel = AtomicBool::new(false);
        let mut last_items = 0u64;
        let mut last_bytes = 0u64;
        let outcome = delete_permanently_with_progress(&base, &cancel, |p| {
            last_items = p.deleted_items;
            last_bytes = p.deleted_bytes;
        })
        .unwrap();
        assert_eq!(outcome, DeleteOutcome::Completed);
        assert!(last_items >= 3); // b.bin + nested + base
        assert_eq!(last_bytes, 5);
        assert!(!base.exists());
    }

    #[test]
    fn detects_permission_denied_delete_errors() {
        assert!(is_permission_denied_error("Failed to permanently delete \"C:\\Windows\\foo\": Access is denied. (os error 5)"));
        assert!(is_permission_denied_error("Permission denied (0x80070005)"));
        assert!(!is_permission_denied_error("The process cannot access the file because it is being used by another process."));
    }

    #[test]
    fn detects_critical_windows_system_paths_without_false_positives() {
        assert!(check_protected_system_path(Path::new(r"C:\Windows")).is_some());
        assert!(check_protected_system_path(Path::new(r"C:\Windows\System32\kernel32.dll")).is_some());
        assert!(check_protected_system_path(Path::new(r"C:\Program Files\TexTree")).is_some());
        assert!(check_protected_system_path(Path::new(r"C:\Program Files (x86)\Steam\steam.exe")).is_some());
        assert!(check_protected_system_path(Path::new(r"C:\ProgramData\Microsoft")).is_some());
        assert!(check_protected_system_path(Path::new(r"C:\System Volume Information")).is_some());
        assert!(check_protected_system_path(Path::new(r"C:\Boot\BCD")).is_some());
        assert!(check_protected_system_path(Path::new(r"C:\Users")).is_some());
        assert!(check_protected_system_path(Path::new(r"C:\Users\danie")).is_some());
        assert!(check_protected_system_path(Path::new(r"C:\pagefile.sys")).is_some());

        // Normal user project/download/game paths must NOT be flagged
        assert!(check_protected_system_path(Path::new(r"C:\Users\danie\Downloads\archive.zip")).is_none());
        assert!(check_protected_system_path(Path::new(r"C:\Users\danie\Documents\project\target")).is_none());
        assert!(check_protected_system_path(Path::new(r"X:\TexTree\target")).is_none());
        assert!(check_protected_system_path(Path::new(r"D:\Games\OldGame")).is_none());
    }
}
