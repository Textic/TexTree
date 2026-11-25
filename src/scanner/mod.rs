pub mod ntfs_mft;
pub mod parallel_walk;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use futures_channel::mpsc::UnboundedSender;

use crate::models::{DiskTree, ScanMode, SortSpec, VolumeInfo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanTarget {
    AllFixedDrives,
    Volume(VolumeInfo),
    Folder(PathBuf),
}

impl ScanTarget {
    pub fn display_label(&self) -> String {
        match self {
            Self::AllFixedDrives => "This PC (All Drives)".to_string(),
            Self::Volume(v) => v.display_title(),
            Self::Folder(p) => p.to_string_lossy().to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScanProgress {
    pub scanned_files: u64,
    pub scanned_dirs: u64,
    pub scanned_bytes: u64,
    pub denied_count: u32,
    pub current_path: String,
    pub elapsed_ms: u64,
}

#[derive(Debug)]
pub enum ScanEvent {
    Progress(ScanProgress),
    Completed(DiskTree),
    Cancelled,
    Error(String),
}

/// Launches a background scan (using `TexTreeMftService`, direct elevated NTFS MFT,
/// or parallel Win32/POSIX walking depending on `scan_mode`) and streams progress + final `DiskTree`.
pub fn spawn_scan(
    target: ScanTarget,
    volumes: Vec<VolumeInfo>,
    sort: SortSpec,
    scan_mode: ScanMode,
    prefer_ntfs_mft: bool,
    cancel: Arc<AtomicBool>,
    tx: UnboundedSender<ScanEvent>,
) {
    std::thread::Builder::new()
        .name("textree-scan".to_string())
        .spawn(move || {
            parallel_walk::run_scan(
                target,
                volumes,
                sort,
                scan_mode,
                prefer_ntfs_mft,
                cancel,
                tx,
            );
        })
        .expect("failed to spawn scanner thread");
}
