use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_channel::mpsc::UnboundedSender;
use rayon::prelude::*;

use crate::models::tree::ExtInterner;
use crate::models::{DiskTree, DriveKind, FileNode, NodeFlags, NodeId, SortSpec, VolumeInfo};
use crate::platform::cluster_size_for_path;
use crate::scanner::{ScanEvent, ScanProgress, ScanTarget};

const MAX_SCAN_DEPTH: u16 = 256;

struct ScanCounters {
    files: AtomicU64,
    dirs: AtomicU64,
    bytes: AtomicU64,
    denied: AtomicU32,
    last_path_update_ms: AtomicU64,
    current_path: Mutex<String>,
    started: Instant,
}

impl ScanCounters {
    fn new() -> Self {
        Self {
            files: AtomicU64::new(0),
            dirs: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            denied: AtomicU32::new(0),
            last_path_update_ms: AtomicU64::new(0),
            current_path: Mutex::new(String::new()),
            started: Instant::now(),
        }
    }

    fn maybe_set_path(&self, path: &Path) {
        let now_ms = self.started.elapsed().as_millis() as u64;
        let prev = self.last_path_update_ms.load(Ordering::Relaxed);
        if now_ms.saturating_sub(prev) >= 60
            && self
                .last_path_update_ms
                .compare_exchange(prev, now_ms, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            && let Ok(mut guard) = self.current_path.try_lock()
        {
            *guard = path.to_string_lossy().into_owned();
        }
    }

    fn snapshot(&self) -> ScanProgress {
        let current_path = self
            .current_path
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        ScanProgress {
            scanned_files: self.files.load(Ordering::Relaxed),
            scanned_dirs: self.dirs.load(Ordering::Relaxed),
            scanned_bytes: self.bytes.load(Ordering::Relaxed),
            denied_count: self.denied.load(Ordering::Relaxed),
            current_path,
            elapsed_ms: self.started.elapsed().as_millis() as u64,
        }
    }
}

struct TempNode {
    name: Box<str>,
    logical_size: u64,
    allocated_size: u64,
    modified_secs: i64,
    flags: NodeFlags,
    children: Vec<TempNode>,
}

pub fn run_scan(
    target: ScanTarget,
    volumes: Vec<VolumeInfo>,
    sort: SortSpec,
    cancel: Arc<AtomicBool>,
    tx: UnboundedSender<ScanEvent>,
) {
    let counters = Arc::new(ScanCounters::new());
    let done = Arc::new(AtomicBool::new(false));

    // Spawn lightweight progress ticker so GPUI stays smooth with zero lock contention on worker threads.
    let ticker_counters = counters.clone();
    let ticker_done = done.clone();
    let ticker_cancel = cancel.clone();
    let ticker_tx = tx.clone();
    let ticker = std::thread::spawn(move || {
        while !ticker_done.load(Ordering::Relaxed) && !ticker_cancel.load(Ordering::Relaxed) {
            let _ = ticker_tx.unbounded_send(ScanEvent::Progress(ticker_counters.snapshot()));
            std::thread::sleep(Duration::from_millis(80));
        }
    });

    let result = execute_scan(&target, &volumes, sort, &counters, &cancel);
    done.store(true, Ordering::Relaxed);
    let _ = ticker.join();

    if cancel.load(Ordering::Relaxed) {
        let _ = tx.unbounded_send(ScanEvent::Cancelled);
        return;
    }

    match result {
        Ok(tree) => {
            let _ = tx.unbounded_send(ScanEvent::Completed(tree));
        }
        Err(err) => {
            let _ = tx.unbounded_send(ScanEvent::Error(err));
        }
    }
}

/// Synchronous helper used by unit tests.
#[cfg(test)]
pub fn scan_directory_sync(path: &Path, sort: SortSpec) -> Result<DiskTree, String> {
    let counters = ScanCounters::new();
    let cancel = AtomicBool::new(false);
    execute_scan(
        &ScanTarget::Folder(path.to_path_buf()),
        &[],
        sort,
        &counters,
        &cancel,
    )
}

fn execute_scan(
    target: &ScanTarget,
    volumes: &[VolumeInfo],
    sort: SortSpec,
    counters: &ScanCounters,
    cancel: &AtomicBool,
) -> Result<DiskTree, String> {
    let started = Instant::now();
    let mut tree = match target {
        ScanTarget::AllFixedDrives => {
            let fixed: Vec<VolumeInfo> = volumes
                .iter()
                .filter(|v| v.kind == DriveKind::Fixed)
                .cloned()
                .collect();
            let scan_vols = if fixed.is_empty() {
                volumes.to_vec()
            } else {
                fixed
            };
            if scan_vols.is_empty() {
                return Err("No mounted drives found to scan.".to_string());
            }

            let subtrees: Vec<TempNode> = scan_vols
                .into_par_iter()
                .map(|vol| {
                    let cluster = vol.cluster_size.max(512);
                    let mut node = scan_dir_recursive(
                        &vol.root_path,
                        vol.root_path.to_string_lossy().into_owned().into_boxed_str(),
                        NodeFlags::IS_DIR | NodeFlags::IS_VOLUME,
                        cluster,
                        0,
                        counters,
                        cancel,
                    );
                    node.flags |= NodeFlags::IS_VOLUME;
                    node
                })
                .collect();

            if cancel.load(Ordering::Relaxed) {
                return Err("Scan cancelled".to_string());
            }

            let mut disk_tree = DiskTree::new(
                "This PC",
                NodeFlags::IS_DIR | NodeFlags::IS_VIRTUAL,
                None,
                target.display_label(),
            );
            let mut interner = ExtInterner::default();
            for sub in subtrees {
                append_temp_subtree(&mut disk_tree, &mut interner, 0, sub);
            }
            disk_tree.extensions = interner.into_vec();
            disk_tree
        }
        ScanTarget::Volume(vol) => {
            let cluster = vol.cluster_size.max(512);
            let root_name = vol.root_path.to_string_lossy().into_owned();
            let sub = scan_dir_recursive(
                &vol.root_path,
                root_name.clone().into_boxed_str(),
                NodeFlags::IS_DIR | NodeFlags::IS_VOLUME,
                cluster,
                0,
                counters,
                cancel,
            );
            if cancel.load(Ordering::Relaxed) {
                return Err("Scan cancelled".to_string());
            }
            build_tree_from_root(
                sub,
                Some(vol.root_path.clone()),
                target.display_label(),
            )
        }
        ScanTarget::Folder(path) => {
            if !path.exists() {
                return Err(format!("Folder does not exist: {}", path.display()));
            }
            let cluster = cluster_size_for_path(path);
            let root_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            let sub = scan_dir_recursive(
                path,
                root_name.into_boxed_str(),
                NodeFlags::IS_DIR,
                cluster,
                0,
                counters,
                cancel,
            );
            if cancel.load(Ordering::Relaxed) {
                return Err("Scan cancelled".to_string());
            }
            build_tree_from_root(sub, Some(path.clone()), target.display_label())
        }
    };

    tree.scan_duration_ms = started.elapsed().as_millis() as u64;
    tree.aggregate_and_sort(sort);
    Ok(tree)
}

fn build_tree_from_root(
    root_temp: TempNode,
    root_base_path: Option<PathBuf>,
    target_label: String,
) -> DiskTree {
    let mut tree = DiskTree::new(
        root_temp.name,
        root_temp.flags,
        root_base_path,
        target_label,
    );
    tree.nodes[0].modified_secs = root_temp.modified_secs;
    let mut interner = ExtInterner::default();
    for child in root_temp.children {
        append_temp_subtree(&mut tree, &mut interner, 0, child);
    }
    tree.extensions = interner.into_vec();
    tree
}

fn append_temp_subtree(
    tree: &mut DiskTree,
    interner: &mut ExtInterner,
    parent_id: NodeId,
    temp: TempNode,
) {
    let ext_id = if temp.flags.contains(NodeFlags::IS_DIR) {
        0
    } else {
        interner.intern_from_filename(&temp.name)
    };
    let my_id = tree.nodes.len() as NodeId;
    let children = temp.children;
    tree.nodes.push(FileNode {
        name: temp.name,
        parent: Some(parent_id),
        children: Vec::with_capacity(children.len()),
        logical_size: temp.logical_size,
        allocated_size: temp.allocated_size,
        file_count: 0,
        dir_count: 0,
        denied_count: 0,
        modified_secs: temp.modified_secs,
        flags: temp.flags,
        ext_id,
        removed: false,
    });
    tree.nodes[parent_id as usize].children.push(my_id);

    for child in children {
        append_temp_subtree(tree, interner, my_id, child);
    }
}

#[cfg(target_os = "windows")]
fn scan_dir_recursive(
    dir_path: &Path,
    name: Box<str>,
    mut base_flags: NodeFlags,
    cluster_size: u64,
    depth: u16,
    counters: &ScanCounters,
    cancel: &AtomicBool,
) -> TempNode {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_FILES, GetLastError,
        INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN,
        FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SPARSE_FILE,
        FILE_ATTRIBUTE_SYSTEM, FIND_FIRST_EX_LARGE_FETCH, FindClose, FindFirstFileExW,
        FindNextFileW, FindExInfoBasic, FindExSearchNameMatch, GetCompressedFileSizeW,
        WIN32_FIND_DATAW,
    };

    if cancel.load(Ordering::Relaxed) || depth >= MAX_SCAN_DEPTH {
        return TempNode {
            name,
            logical_size: 0,
            allocated_size: 0,
            modified_secs: 0,
            flags: base_flags,
            children: Vec::new(),
        };
    }

    counters.dirs.fetch_add(1, Ordering::Relaxed);
    counters.maybe_set_path(dir_path);

    let pattern_wide = build_win32_search_pattern(dir_path);
    let mut find_data: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
    let handle = unsafe {
        FindFirstFileExW(
            pattern_wide.as_ptr(),
            FindExInfoBasic,
            (&mut find_data as *mut WIN32_FIND_DATAW).cast(),
            FindExSearchNameMatch,
            std::ptr::null(),
            FIND_FIRST_EX_LARGE_FETCH,
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        let err = unsafe { GetLastError() };
        if err == ERROR_ACCESS_DENIED {
            base_flags |= NodeFlags::ACCESS_DENIED;
            counters.denied.fetch_add(1, Ordering::Relaxed);
        } else if err != ERROR_FILE_NOT_FOUND && err != ERROR_NO_MORE_FILES {
            // Treat unreadable system paths gracefully.
            base_flags |= NodeFlags::ACCESS_DENIED;
            counters.denied.fetch_add(1, Ordering::Relaxed);
        }
        return TempNode {
            name,
            logical_size: 0,
            allocated_size: 0,
            modified_secs: 0,
            flags: base_flags,
            children: Vec::new(),
        };
    }

    struct FindHandle(windows_sys::Win32::Foundation::HANDLE);
    impl Drop for FindHandle {
        fn drop(&mut self) {
            unsafe {
                FindClose(self.0);
            }
        }
    }
    let _guard = FindHandle(handle);

    let mut files: Vec<TempNode> = Vec::new();
    let mut subdirs: Vec<(PathBuf, Box<str>, NodeFlags, i64)> = Vec::new();

    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        let name_len = find_data
            .cFileName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(find_data.cFileName.len());
        let slice = &find_data.cFileName[..name_len];

        // Skip "." and ".."
        let is_dot = slice == [b'.' as u16] || slice == [b'.' as u16, b'.' as u16];
        if !is_dot && !slice.is_empty() {
            let entry_os = OsString::from_wide(slice);
            let entry_name = entry_os.to_string_lossy().into_owned().into_boxed_str();
            let attrs = find_data.dwFileAttributes;
            let modified_secs = filetime_to_unix_secs(
                find_data.ftLastWriteTime.dwHighDateTime,
                find_data.ftLastWriteTime.dwLowDateTime,
            );

            let mut flags = NodeFlags::empty();
            if attrs & FILE_ATTRIBUTE_DIRECTORY != 0 {
                flags |= NodeFlags::IS_DIR;
            }
            if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                flags |= NodeFlags::IS_SYMLINK;
            }
            if attrs & FILE_ATTRIBUTE_HIDDEN != 0 {
                flags |= NodeFlags::IS_HIDDEN;
            }
            if attrs & FILE_ATTRIBUTE_SYSTEM != 0 {
                flags |= NodeFlags::IS_SYSTEM;
            }
            if attrs & FILE_ATTRIBUTE_READONLY != 0 {
                flags |= NodeFlags::IS_READONLY;
            }
            if attrs & FILE_ATTRIBUTE_COMPRESSED != 0 {
                flags |= NodeFlags::IS_COMPRESSED;
            }

            if flags.contains(NodeFlags::IS_DIR) {
                if flags.contains(NodeFlags::IS_SYMLINK) {
                    // Never follow directory junctions or symlinks to avoid cycles and double counting.
                    files.push(TempNode {
                        name: entry_name,
                        logical_size: 0,
                        allocated_size: 0,
                        modified_secs,
                        flags,
                        children: Vec::new(),
                    });
                } else {
                    let child_path = dir_path.join(&entry_os);
                    subdirs.push((child_path, entry_name, flags, modified_secs));
                }
            } else {
                let logical_size = ((find_data.nFileSizeHigh as u64) << 32)
                    | (find_data.nFileSizeLow as u64);
                let allocated_size = if flags.contains(NodeFlags::IS_SYMLINK) || logical_size == 0 {
                    0
                } else if attrs & (FILE_ATTRIBUTE_COMPRESSED | FILE_ATTRIBUTE_SPARSE_FILE) != 0 {
                    let full_child = dir_path.join(&entry_os);
                    let wide = to_win32_extended_wide(&full_child);
                    let mut high: u32 = 0;
                    let low = unsafe { GetCompressedFileSizeW(wide.as_ptr(), &mut high) };
                    if low == u32::MAX && unsafe { GetLastError() } != 0 {
                        round_up_cluster(logical_size, cluster_size)
                    } else {
                        ((high as u64) << 32) | (low as u64)
                    }
                } else {
                    round_up_cluster(logical_size, cluster_size)
                };

                counters.files.fetch_add(1, Ordering::Relaxed);
                counters.bytes.fetch_add(logical_size, Ordering::Relaxed);

                files.push(TempNode {
                    name: entry_name,
                    logical_size,
                    allocated_size,
                    modified_secs,
                    flags,
                    children: Vec::new(),
                });
            }
        }

        let next_ok = unsafe { FindNextFileW(handle, &mut find_data) };
        if next_ok == 0 {
            break;
        }
    }

    drop(_guard);

    let child_dirs: Vec<TempNode> = if subdirs.len() > 1 && depth < 16 {
        subdirs
            .into_par_iter()
            .map(|(child_path, child_name, child_flags, child_mod)| {
                let mut node = scan_dir_recursive(
                    &child_path,
                    child_name,
                    child_flags,
                    cluster_size,
                    depth + 1,
                    counters,
                    cancel,
                );
                if node.modified_secs == 0 {
                    node.modified_secs = child_mod;
                }
                node
            })
            .collect()
    } else {
        subdirs
            .into_iter()
            .map(|(child_path, child_name, child_flags, child_mod)| {
                let mut node = scan_dir_recursive(
                    &child_path,
                    child_name,
                    child_flags,
                    cluster_size,
                    depth + 1,
                    counters,
                    cancel,
                );
                if node.modified_secs == 0 {
                    node.modified_secs = child_mod;
                }
                node
            })
            .collect()
    };

    files.extend(child_dirs);

    TempNode {
        name,
        logical_size: 0,
        allocated_size: 0,
        modified_secs: 0,
        flags: base_flags,
        children: files,
    }
}

#[cfg(target_os = "windows")]
fn to_win32_extended_wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let raw = path.as_os_str();
    let raw_str = raw.to_string_lossy();
    if raw_str.starts_with(r"\\?\") || raw_str.len() < 230 {
        return raw.encode_wide().chain(std::iter::once(0)).collect();
    }
    let prefixed = if let Some(unc) = raw_str.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{unc}")
    } else {
        format!(r"\\?\{raw_str}")
    };
    prefixed.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn build_win32_search_pattern(dir: &Path) -> Vec<u16> {
    let s = dir.to_string_lossy();
    let trimmed = s.trim_end_matches(['\\', '/']);
    let joined = if s.starts_with(r"\\?\") || trimmed.len() < 230 {
        format!("{trimmed}\\*")
    } else if let Some(unc) = trimmed.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{unc}\*")
    } else {
        format!(r"\\?\{trimmed}\*")
    };
    joined.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
#[inline]
fn filetime_to_unix_secs(high: u32, low: u32) -> i64 {
    const WINDOWS_TO_UNIX_EPOCH_SECS: i64 = 11_644_473_600;
    let ticks = ((high as u64) << 32) | (low as u64);
    if ticks == 0 {
        return 0;
    }
    ((ticks / 10_000_000) as i64).saturating_sub(WINDOWS_TO_UNIX_EPOCH_SECS)
}

#[inline]
fn round_up_cluster(size: u64, cluster: u64) -> u64 {
    if size == 0 {
        return 0;
    }
    let c = cluster.max(512);
    size.div_ceil(c).saturating_mul(c)
}

#[cfg(not(target_os = "windows"))]
fn scan_dir_recursive(
    dir_path: &Path,
    name: Box<str>,
    mut base_flags: NodeFlags,
    cluster_size: u64,
    depth: u16,
    counters: &ScanCounters,
    cancel: &AtomicBool,
) -> TempNode {
    use std::os::unix::fs::MetadataExt;
    use std::time::UNIX_EPOCH;

    if cancel.load(Ordering::Relaxed) || depth >= MAX_SCAN_DEPTH {
        return TempNode {
            name,
            logical_size: 0,
            allocated_size: 0,
            modified_secs: 0,
            flags: base_flags,
            children: Vec::new(),
        };
    }

    counters.dirs.fetch_add(1, Ordering::Relaxed);
    counters.maybe_set_path(dir_path);

    let read_dir = match std::fs::read_dir(dir_path) {
        Ok(rd) => rd,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                base_flags |= NodeFlags::ACCESS_DENIED;
                counters.denied.fetch_add(1, Ordering::Relaxed);
            }
            return TempNode {
                name,
                logical_size: 0,
                allocated_size: 0,
                modified_secs: 0,
                flags: base_flags,
                children: Vec::new(),
            };
        }
    };

    let mut files = Vec::new();
    let mut subdirs = Vec::new();

    for entry_res in read_dir {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(entry) = entry_res else {
            continue;
        };
        let entry_path = entry.path();
        let entry_name_str = entry.file_name().to_string_lossy().into_owned();
        let mut flags = NodeFlags::empty();
        if entry_name_str.starts_with('.') {
            flags |= NodeFlags::IS_HIDDEN;
        }

        let Ok(meta) = std::fs::symlink_metadata(&entry_path) else {
            continue;
        };
        let ft = meta.file_type();
        let modified_secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        if ft.is_symlink() {
            flags |= NodeFlags::IS_SYMLINK;
            files.push(TempNode {
                name: entry_name_str.into_boxed_str(),
                logical_size: 0,
                allocated_size: 0,
                modified_secs,
                flags,
                children: Vec::new(),
            });
        } else if ft.is_dir() {
            flags |= NodeFlags::IS_DIR;
            subdirs.push((entry_path, entry_name_str.into_boxed_str(), flags, modified_secs));
        } else {
            let logical_size = meta.len();
            let blocks_bytes = meta.blocks().saturating_mul(512);
            let allocated_size = if blocks_bytes > 0 {
                blocks_bytes
            } else {
                round_up_cluster(logical_size, cluster_size)
            };
            counters.files.fetch_add(1, Ordering::Relaxed);
            counters.bytes.fetch_add(logical_size, Ordering::Relaxed);
            files.push(TempNode {
                name: entry_name_str.into_boxed_str(),
                logical_size,
                allocated_size,
                modified_secs,
                flags,
                children: Vec::new(),
            });
        }
    }

    let child_dirs: Vec<TempNode> = subdirs
        .into_par_iter()
        .map(|(child_path, child_name, child_flags, child_mod)| {
            let mut node = scan_dir_recursive(
                &child_path,
                child_name,
                child_flags,
                cluster_size,
                depth + 1,
                counters,
                cancel,
            );
            if node.modified_secs == 0 {
                node.modified_secs = child_mod;
            }
            node
        })
        .collect();

    files.extend(child_dirs);

    TempNode {
        name,
        logical_size: 0,
        allocated_size: 0,
        modified_secs: 0,
        flags: base_flags,
        children: files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_temp_directory_accurately() {
        let base = std::env::temp_dir().join(format!("textree_scan_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("sub_a")).unwrap();
        std::fs::create_dir_all(base.join("sub_b/nested")).unwrap();

        std::fs::write(base.join("root.bin"), vec![1u8; 4096]).unwrap();
        std::fs::write(base.join("sub_a/big.dat"), vec![2u8; 10_000]).unwrap();
        std::fs::write(base.join("sub_b/nested/small.txt"), b"hello").unwrap();

        let tree = scan_directory_sync(&base, SortSpec::default()).unwrap();
        assert_eq!(tree.root_node().logical_size, 4096 + 10_000 + 5);
        assert_eq!(tree.root_node().file_count, 3);
        assert_eq!(tree.root_node().dir_count, 3); // sub_a, sub_b, nested

        // sub_a (10,000 B) should be sorted first under root
        let first_child = tree.nodes[0].children[0];
        assert_eq!(tree.nodes[first_child as usize].name.as_ref(), "sub_a");

        let _ = std::fs::remove_dir_all(&base);
    }
}
