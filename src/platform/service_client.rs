//! Client and lifecycle manager for the read-only `TexTreeMftService` Windows service.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::platform::locks::FileLockStatus;
use crate::scanner::parallel_walk::{ScanCounters, TempNode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServiceStatus {
    Running,
    Stopped,
    NotInstalled,
    #[default]
    Unavailable,
}

impl ServiceStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Active",
            Self::Stopped => "Stopped",
            Self::NotInstalled => "Not Installed",
            Self::Unavailable => "Windows Only",
        }
    }
}

pub fn find_service_exe() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let current = std::env::current_exe().ok()?;
        let candidate = current.parent()?.join("textree-service.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
        None
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
pub fn query_service_status() -> ServiceStatus {
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_MANAGER_CONNECT,
        SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_STATUS,
    };

    let scm = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if scm.is_null() {
        return ServiceStatus::Unavailable;
    }
    let name: Vec<u16> = crate::service_protocol::SERVICE_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let svc = unsafe { OpenServiceW(scm, name.as_ptr(), SERVICE_QUERY_STATUS) };
    if svc.is_null() {
        unsafe {
            CloseServiceHandle(scm);
        }
        return ServiceStatus::NotInstalled;
    }

    let mut st: SERVICE_STATUS = unsafe { std::mem::zeroed() };
    let ok = unsafe { QueryServiceStatus(svc, &mut st) };
    unsafe {
        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
    }
    if ok == 0 {
        ServiceStatus::Stopped
    } else if st.dwCurrentState == SERVICE_RUNNING {
        ServiceStatus::Running
    } else {
        ServiceStatus::Stopped
    }
}

#[cfg(not(target_os = "windows"))]
pub fn query_service_status() -> ServiceStatus {
    ServiceStatus::Unavailable
}

#[cfg(target_os = "windows")]
fn run_service_admin_command(flag: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_NO_CONSOLE, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let exe = find_service_exe().ok_or_else(|| {
        "textree-service.exe was not found next to textree.exe. Reinstall TexTree or build the service binary."
            .to_string()
    })?;

    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    let file: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let params: Vec<u16> = flag.encode_utf16().chain(std::iter::once(0)).collect();

    let mut sei: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    sei.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NO_CONSOLE;
    sei.lpVerb = verb.as_ptr();
    sei.lpFile = file.as_ptr();
    sei.lpParameters = params.as_ptr();
    sei.nShow = SW_HIDE;

    let ok = unsafe { ShellExecuteExW(&mut sei) };
    if ok == 0 || sei.hProcess.is_null() {
        return Err("Service action was cancelled or failed UAC elevation.".to_string());
    }

    let wait = unsafe { WaitForSingleObject(sei.hProcess, 20_000) };
    let mut exit_code: u32 = 1;
    unsafe {
        let _ = GetExitCodeProcess(sei.hProcess, &mut exit_code);
        CloseHandle(sei.hProcess);
    }
    if wait != WAIT_OBJECT_0 {
        return Err("Timed out waiting for TexTree MFT Service command.".to_string());
    }
    if exit_code != 0 {
        return Err(format!(
            "textree-service.exe {flag} failed with exit code {exit_code}."
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn run_service_admin_command(_flag: &str) -> Result<(), String> {
    Err("TexTree MFT Service is only available on Windows.".to_string())
}

pub fn install_service_elevated() -> Result<(), String> {
    run_service_admin_command("--install")
}

pub fn start_service_elevated() -> Result<(), String> {
    run_service_admin_command("--start")
}

pub fn uninstall_service_elevated() -> Result<(), String> {
    run_service_admin_command("--uninstall")
}

#[cfg(target_os = "windows")]
fn connect_pipe_with_retry() -> Result<std::fs::File, String> {
    use std::fs::OpenOptions;
    use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;
    use windows_sys::Win32::System::Pipes::WaitNamedPipeW;

    let wide_pipe: Vec<u16> = crate::service_protocol::PIPE_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    for attempt in 0..10 {
        match OpenOptions::new()
            .read(true)
            .write(true)
            .open(crate::service_protocol::PIPE_NAME)
        {
            Ok(f) => return Ok(f),
            Err(err) => {
                if err.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) && attempt < 9 {
                    unsafe {
                        let _ = WaitNamedPipeW(wide_pipe.as_ptr(), 1500);
                    }
                    continue;
                }
                return Err(format!("Failed connecting to TexTree MFT Service: {err}"));
            }
        }
    }
    Err("TexTree MFT Service pipe remained busy.".to_string())
}

#[cfg(target_os = "windows")]
pub fn scan_volume_via_service(
    short_name: &str,
    root_path_str: &str,
    counters: &ScanCounters,
    cancel: &AtomicBool,
) -> Result<TempNode, String> {
    use std::io::{BufReader, BufWriter};
    use std::sync::atomic::Ordering;

    use crate::scanner::ntfs_mft::wire_nodes_to_temp_node;
    use crate::service_protocol::{
        FRAME_BINARY_TREE, FRAME_JSON_MSG, FRAME_JSON_REQ, ServiceMessage, ServiceRequest,
        decode_wire_nodes, read_frame, write_json_frame,
    };

    let drive_letter = short_name
        .chars()
        .find(|c| c.is_ascii_alphabetic())
        .ok_or_else(|| "Volume has no NTFS drive letter.".to_string())?;

    let pipe = connect_pipe_with_retry()?;
    let pipe_read = pipe
        .try_clone()
        .map_err(|e| format!("Failed cloning pipe handle: {e}"))?;
    let mut writer = BufWriter::new(pipe);
    let mut reader = BufReader::with_capacity(256 * 1024, pipe_read);

    counters.maybe_set_path(Path::new(&format!("{root_path_str} [MFT Service]")));
    write_json_frame(
        &mut writer,
        FRAME_JSON_REQ,
        &ServiceRequest::ScanVolume { drive_letter },
    )?;

    let mut prev_files = 0u64;
    let mut prev_dirs = 0u64;
    let mut prev_bytes = 0u64;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Scan cancelled".to_string());
        }
        let (kind, payload) = read_frame(&mut reader)?;
        match kind {
            FRAME_JSON_MSG => {
                let msg: ServiceMessage = serde_json::from_slice(&payload)
                    .map_err(|e| format!("Invalid IPC message from service: {e}"))?;
                match msg {
                    ServiceMessage::Progress {
                        scanned_files,
                        scanned_dirs,
                        scanned_bytes,
                        percent,
                    } => {
                        let df = scanned_files.saturating_sub(prev_files);
                        let dd = scanned_dirs.saturating_sub(prev_dirs);
                        let db = scanned_bytes.saturating_sub(prev_bytes);
                        prev_files = scanned_files;
                        prev_dirs = scanned_dirs;
                        prev_bytes = scanned_bytes;
                        counters.files.fetch_add(df, Ordering::Relaxed);
                        counters.dirs.fetch_add(dd, Ordering::Relaxed);
                        counters.bytes.fetch_add(db, Ordering::Relaxed);
                        counters.maybe_set_path(Path::new(&format!(
                            "{root_path_str} [MFT Service {percent}%]"
                        )));
                    }
                    ServiceMessage::Error(err) => return Err(err),
                    _ => {}
                }
            }
            FRAME_BINARY_TREE => {
                let wire_nodes = decode_wire_nodes(&payload)?;
                return Ok(wire_nodes_to_temp_node(root_path_str, wire_nodes));
            }
            _ => {
                return Err("Unexpected frame type from TexTree MFT Service.".to_string());
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn scan_volume_via_service(
    _short_name: &str,
    _root_path_str: &str,
    _counters: &ScanCounters,
    _cancel: &AtomicBool,
) -> Result<TempNode, String> {
    Err("TexTree MFT Service is only supported on Windows.".to_string())
}

#[cfg(target_os = "windows")]
pub fn inspect_file_lock_via_service(path: &Path) -> Option<FileLockStatus> {
    use std::io::{BufReader, BufWriter};

    use crate::platform::locks::LockingProcess;
    use crate::service_protocol::{
        FRAME_JSON_MSG, FRAME_JSON_REQ, ServiceMessage, ServiceRequest, WireLockStatus, read_frame,
        write_json_frame,
    };

    let pipe = connect_pipe_with_retry().ok()?;
    let pipe_read = pipe.try_clone().ok()?;
    let mut writer = BufWriter::new(pipe);
    let mut reader = BufReader::new(pipe_read);

    write_json_frame(
        &mut writer,
        FRAME_JSON_REQ,
        &ServiceRequest::InspectFileLock {
            path: path.to_string_lossy().into_owned(),
        },
    )
    .ok()?;

    let (kind, payload) = read_frame(&mut reader).ok()?;
    if kind != FRAME_JSON_MSG {
        return None;
    }
    let msg: ServiceMessage = serde_json::from_slice(&payload).ok()?;
    match msg {
        ServiceMessage::LockResult(wire) => Some(match wire {
            WireLockStatus::NotApplicable => FileLockStatus::NotApplicable,
            WireLockStatus::Unlocked => FileLockStatus::Unlocked,
            WireLockStatus::LockedBy(procs) => FileLockStatus::LockedBy(
                procs
                    .into_iter()
                    .map(|p| LockingProcess {
                        pid: p.pid,
                        name: p.name,
                        app_type: p.app_type,
                    })
                    .collect(),
            ),
            WireLockStatus::LockedUnknown => FileLockStatus::LockedUnknown,
            WireLockStatus::AccessDenied => FileLockStatus::AccessDenied,
            WireLockStatus::Error(e) => FileLockStatus::Error(e),
        }),
        _ => None,
    }
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn inspect_file_lock_via_service(_path: &Path) -> Option<FileLockStatus> {
    None
}
