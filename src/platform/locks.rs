use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockingProcess {
    pub pid: u32,
    pub name: String,
    pub app_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FileLockStatus {
    #[default]
    NotChecked,
    #[allow(dead_code)]
    Checking,
    NotApplicable,
    Unlocked,
    LockedBy(Vec<LockingProcess>),
    LockedUnknown,
    AccessDenied,
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    Error(String),
}

#[cfg(target_os = "windows")]
pub fn inspect_file_lock(path: &Path) -> FileLockStatus {
    use std::collections::BTreeSet;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_LOCK_VIOLATION, ERROR_MORE_DATA,
        ERROR_SHARING_VIOLATION, GENERIC_READ, GENERIC_WRITE, GetLastError, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    use windows_sys::Win32::System::RestartManager::{
        CCH_RM_SESSION_KEY, RM_PROCESS_INFO, RmConsole, RmCritical, RmEndSession, RmExplorer,
        RmGetList, RmMainWindow, RmOtherWindow, RmRegisterResources, RmService, RmStartSession,
    };

    if path.is_dir() {
        return FileLockStatus::NotApplicable;
    }

    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();

    // 1. Query Windows Restart Manager to identify exact processes holding open handles.
    let mut rm_processes: Vec<LockingProcess> = Vec::new();
    let mut session_handle: u32 = 0;
    let mut session_key = [0u16; (CCH_RM_SESSION_KEY as usize) + 1];

    let start_rc = unsafe { RmStartSession(&mut session_handle, 0, session_key.as_mut_ptr()) };
    if start_rc == 0 {
        struct SessionGuard(u32);
        impl Drop for SessionGuard {
            fn drop(&mut self) {
                unsafe {
                    let _ = RmEndSession(self.0);
                }
            }
        }
        let _guard = SessionGuard(session_handle);

        let files = [wide_path.as_ptr()];
        let reg_rc = unsafe {
            RmRegisterResources(
                session_handle,
                1,
                files.as_ptr(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
            )
        };
        if reg_rc == 0 {
            let mut needed: u32 = 0;
            let mut count: u32 = 0;
            let mut reason: u32 = 0;

            let mut list_rc = unsafe {
                RmGetList(
                    session_handle,
                    &mut needed,
                    &mut count,
                    std::ptr::null_mut(),
                    &mut reason,
                )
            };

            if list_rc == ERROR_MORE_DATA && needed > 0 {
                let mut infos: Vec<RM_PROCESS_INFO> =
                    vec![unsafe { std::mem::zeroed() }; (needed as usize) + 4];
                count = infos.len() as u32;
                list_rc = unsafe {
                    RmGetList(
                        session_handle,
                        &mut needed,
                        &mut count,
                        infos.as_mut_ptr(),
                        &mut reason,
                    )
                };
                if list_rc == 0 {
                    let mut seen: BTreeSet<(u32, String)> = BTreeSet::new();
                    for info in infos.iter().take(count as usize) {
                        let pid = info.Process.dwProcessId;
                        let name_len = info
                            .strAppName
                            .iter()
                            .position(|&c| c == 0)
                            .unwrap_or(info.strAppName.len());
                        let name = String::from_utf16_lossy(&info.strAppName[..name_len])
                            .trim()
                            .to_string();
                        let display_name = if name.is_empty() {
                            format!("Process #{pid}")
                        } else {
                            name
                        };
                        if !seen.insert((pid, display_name.clone())) {
                            continue;
                        }
                        let app_type = match info.ApplicationType {
                            x if x == RmMainWindow => "Application",
                            x if x == RmOtherWindow => "Window",
                            x if x == RmService => "Service",
                            x if x == RmExplorer => "Explorer",
                            x if x == RmConsole => "Console",
                            x if x == RmCritical => "System Critical",
                            _ => "Process",
                        }
                        .to_string();

                        rm_processes.push(LockingProcess {
                            pid,
                            name: display_name,
                            app_type,
                        });
                    }
                    rm_processes.sort_by_key(|p| p.pid);
                }
            }
        }
    }

    if !rm_processes.is_empty() {
        return FileLockStatus::LockedBy(rm_processes);
    }

    // 2. Fallback non-destructive exclusive open probe to catch kernel/elevated locks & ACL blocks.
    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if handle != INVALID_HANDLE_VALUE {
        unsafe {
            CloseHandle(handle);
        }
        return FileLockStatus::Unlocked;
    }

    let err = unsafe { GetLastError() };
    if err == ERROR_SHARING_VIOLATION || err == ERROR_LOCK_VIOLATION {
        return FileLockStatus::LockedUnknown;
    }
    if err == ERROR_ACCESS_DENIED {
        // Check whether the file is merely read-only vs truly access-denied.
        let ro_handle = unsafe {
            CreateFileW(
                wide_path.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if ro_handle != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(ro_handle);
            }
            return FileLockStatus::Unlocked;
        }
        let ro_err = unsafe { GetLastError() };
        if ro_err == ERROR_SHARING_VIOLATION || ro_err == ERROR_LOCK_VIOLATION {
            return FileLockStatus::LockedUnknown;
        }
        return FileLockStatus::AccessDenied;
    }

    FileLockStatus::Unlocked
}

#[cfg(not(target_os = "windows"))]
pub fn inspect_file_lock(path: &Path) -> FileLockStatus {
    if path.is_dir() {
        return FileLockStatus::NotApplicable;
    }
    match std::fs::File::open(path) {
        Ok(_) => FileLockStatus::Unlocked,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => FileLockStatus::AccessDenied,
        Err(e) => FileLockStatus::Error(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_unlocked_and_exclusively_locked_file() {
        let tmp = std::env::temp_dir().join(format!("textree_lock_test_{}.tmp", std::process::id()));
        std::fs::write(&tmp, b"lock-probe").unwrap();

        assert_eq!(inspect_file_lock(&tmp), FileLockStatus::Unlocked);

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let locked = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(&tmp)
                .unwrap();

            let status = inspect_file_lock(&tmp);
            assert!(
                matches!(
                    status,
                    FileLockStatus::LockedBy(_) | FileLockStatus::LockedUnknown
                ),
                "expected locked status while exclusive handle is open, got {status:?}"
            );
            drop(locked);
            assert_eq!(inspect_file_lock(&tmp), FileLockStatus::Unlocked);
        }

        let _ = std::fs::remove_file(&tmp);
    }
}
