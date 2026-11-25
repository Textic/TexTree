use std::path::Path;

use crate::models::{DriveKind, VolumeInfo};

#[cfg(target_os = "windows")]
pub fn list_volumes() -> Vec<VolumeInfo> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::PathBuf;
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
    };

    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    const DRIVE_REMOTE: u32 = 4;
    const DRIVE_CDROM: u32 = 5;
    const DRIVE_RAMDISK: u32 = 6;

    let mut buf = vec![0u16; 512];
    let len = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) } as usize;
    if len == 0 {
        return Vec::new();
    }
    if len > buf.len() {
        buf.resize(len + 2, 0);
        let len2 = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) } as usize;
        buf.truncate(len2);
    } else {
        buf.truncate(len);
    }

    let mut volumes = Vec::new();
    for slice in buf.split(|&c| c == 0) {
        if slice.is_empty() {
            continue;
        }
        let root_os = OsString::from_wide(slice);
        let root_path = PathBuf::from(&root_os);
        let wide_root: Vec<u16> = root_os.encode_wide().chain(std::iter::once(0)).collect();

        let drive_type = unsafe { GetDriveTypeW(wide_root.as_ptr()) };
        let kind = match drive_type {
            DRIVE_FIXED => DriveKind::Fixed,
            DRIVE_REMOVABLE => DriveKind::Removable,
            DRIVE_REMOTE => DriveKind::Network,
            DRIVE_CDROM => DriveKind::Optical,
            DRIVE_RAMDISK => DriveKind::RamDisk,
            _ => DriveKind::Unknown,
        };

        let mut free_avail: u64 = 0;
        let mut total_bytes: u64 = 0;
        let mut total_free: u64 = 0;
        let ok_space = unsafe {
            GetDiskFreeSpaceExW(
                wide_root.as_ptr(),
                &mut free_avail,
                &mut total_bytes,
                &mut total_free,
            )
        };
        if ok_space == 0 || total_bytes == 0 {
            continue;
        }

        let mut vol_name = [0u16; 260];
        let mut fs_name = [0u16; 64];
        let ok_info = unsafe {
            GetVolumeInformationW(
                wide_root.as_ptr(),
                vol_name.as_mut_ptr(),
                vol_name.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                fs_name.as_mut_ptr(),
                fs_name.len() as u32,
            )
        };

        let (label, file_system) = if ok_info != 0 {
            (wide_nul_to_string(&vol_name), wide_nul_to_string(&fs_name))
        } else {
            (String::new(), String::new())
        };

        let cluster_size = query_cluster_size_wide(&wide_root);
        let short_name = root_path
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_string();

        volumes.push(VolumeInfo {
            root_path,
            short_name,
            label,
            file_system,
            kind,
            total_bytes,
            free_bytes: total_free,
            cluster_size,
        });
    }

    volumes.sort_by(|a, b| a.short_name.cmp(&b.short_name));
    volumes
}

#[cfg(target_os = "windows")]
fn query_cluster_size_wide(wide_root: &[u16]) -> u64 {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceW;
    let mut sectors_per_cluster: u32 = 0;
    let mut bytes_per_sector: u32 = 0;
    let mut free_clusters: u32 = 0;
    let mut total_clusters: u32 = 0;
    let ok = unsafe {
        GetDiskFreeSpaceW(
            wide_root.as_ptr(),
            &mut sectors_per_cluster,
            &mut bytes_per_sector,
            &mut free_clusters,
            &mut total_clusters,
        )
    };
    if ok != 0 && sectors_per_cluster > 0 && bytes_per_sector > 0 {
        u64::from(sectors_per_cluster) * u64::from(bytes_per_sector)
    } else {
        4096
    }
}

#[cfg(target_os = "windows")]
fn wide_nul_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).trim().to_string()
}

#[cfg(target_os = "windows")]
pub fn cluster_size_for_path(path: &Path) -> u64 {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Component;

    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return 4096;
    };
    let mut root = prefix.as_os_str().to_os_string();
    root.push("\\");
    let wide: Vec<u16> = root.encode_wide().chain(std::iter::once(0)).collect();
    query_cluster_size_wide(&wide)
}

#[cfg(not(target_os = "windows"))]
pub fn list_volumes() -> Vec<VolumeInfo> {
    use std::path::PathBuf;
    let (total_bytes, free_bytes) = unix_root_space();
    vec![VolumeInfo {
        root_path: PathBuf::from("/"),
        short_name: "/".to_string(),
        label: "Root Filesystem".to_string(),
        file_system: if cfg!(target_os = "macos") {
            "APFS".to_string()
        } else {
            "POSIX".to_string()
        },
        kind: DriveKind::Fixed,
        total_bytes,
        free_bytes,
        cluster_size: 4096,
    }]
}

#[cfg(not(target_os = "windows"))]
fn unix_root_space() -> (u64, u64) {
    use std::process::Command;
    let Ok(out) = Command::new("df").args(["-k", "/"]).output() else {
        return (0, 0);
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let Some(line) = text.lines().nth(1) else {
        return (0, 0);
    };
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 4 {
        return (0, 0);
    }
    let total_kb = cols[1].parse::<u64>().unwrap_or(0);
    let avail_kb = cols[3].parse::<u64>().unwrap_or(0);
    (total_kb.saturating_mul(1024), avail_kb.saturating_mul(1024))
}

#[cfg(not(target_os = "windows"))]
pub fn cluster_size_for_path(_path: &Path) -> u64 {
    4096
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerates_at_least_one_volume() {
        let vols = list_volumes();
        assert!(!vols.is_empty(), "expected at least one mounted volume");
        assert!(vols[0].cluster_size >= 512);
    }
}
