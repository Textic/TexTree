#[cfg(target_os = "windows")]
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token: HANDLE = std::ptr::null_mut();
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
    if ok == 0 || token.is_null() {
        return false;
    }

    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned: u32 = 0;
    let info_ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    unsafe {
        CloseHandle(token);
    }
    info_ok != 0 && elevation.TokenIsElevated != 0
}

#[cfg(not(target_os = "windows"))]
pub fn is_elevated() -> bool {
    false
}

/// Relaunches the current TexTree executable with Administrator privileges via UAC (`runas`).
#[cfg(target_os = "windows")]
pub fn relaunch_as_admin() -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let exe = std::env::current_exe()
        .map_err(|e| format!("Failed to resolve current executable path: {e}"))?;
    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    let file: Vec<u16> = exe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();

    let rc = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;

    if rc <= 32 {
        Err("Administrator elevation was cancelled or failed.".to_string())
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
pub fn relaunch_as_admin() -> Result<(), String> {
    Err("NTFS MFT elevation is only available on Windows.".to_string())
}
