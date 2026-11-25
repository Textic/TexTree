//! Direct NTFS Master File Table (MFT) scanner for Windows.
//!
//! When running elevated on an NTFS volume, this module opens `\\.\X:`, queries
//! `FSCTL_GET_NTFS_VOLUME_DATA`, reads MFT Record #0 (`$MFT`) to decode its
//! non-resident data runs (supporting fragmented MFTs), and sequentially streams
//! 4 MiB chunks of MFT records to reconstruct the full directory tree in a
//! fraction of the time required by directory-by-directory traversal.
//!
//! All binary parsing routines are pure Rust and cross-platform unit-tested.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::SharedString;

use crate::models::NodeFlags;
use crate::scanner::parallel_walk::{ScanCounters, TempNode};

const NTFS_ROOT_RECORD_ID: u64 = 5;
const MFT_RECORD_IN_USE: u16 = 0x0001;
const MFT_RECORD_IS_DIR: u16 = 0x0002;

const ATTR_STANDARD_INFO: u32 = 0x10;
const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
const ATTR_REPARSE_POINT: u32 = 0xC0;
const ATTR_END: u32 = 0xFFFF_FFFF;

const ATTR_FLAG_COMPRESSED: u16 = 0x0001;
const ATTR_FLAG_SPARSE: u16 = 0x8000;

const DOS_READONLY: u32 = 0x0001;
const DOS_HIDDEN: u32 = 0x0002;
const DOS_SYSTEM: u32 = 0x0004;
const DOS_REPARSE: u32 = 0x0400;
const DOS_COMPRESSED: u32 = 0x0800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MftExtent {
    pub lcn: Option<u64>,
    pub cluster_count: u64,
}

#[derive(Debug, Clone)]
pub struct ParsedMftRecord {
    pub record_id: u64,
    pub base_record_id: u64,
    pub parent_id: u64,
    pub name: Option<String>,
    /// Higher is better: Win32 (3) / Win32+DOS (3) / POSIX (2) > DOS 8.3 (1) > none (0).
    pub name_priority: u8,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub has_data_size: bool,
    pub modified_secs: i64,
    pub flags: NodeFlags,
}

#[inline]
fn read_u16(buf: &[u8], off: usize) -> Option<u16> {
    let slice = buf.get(off..off + 2)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

#[inline]
fn read_u32(buf: &[u8], off: usize) -> Option<u32> {
    let slice = buf.get(off..off + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

#[inline]
fn read_u64(buf: &[u8], off: usize) -> Option<u64> {
    let slice = buf.get(off..off + 8)?;
    Some(u64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

#[inline]
fn ntfs_filetime_to_unix_secs(ticks: u64) -> i64 {
    const WINDOWS_TO_UNIX_EPOCH_SECS: i64 = 11_644_473_600;
    if ticks == 0 {
        return 0;
    }
    ((ticks / 10_000_000) as i64).saturating_sub(WINDOWS_TO_UNIX_EPOCH_SECS)
}

#[inline]
fn namespace_priority(ns: u8) -> u8 {
    match ns {
        1 | 3 => 3, // Win32 or Win32+DOS
        0 => 2,     // POSIX
        2 => 1,     // DOS 8.3 short name
        _ => 0,
    }
}

/// Applies the NTFS Update Sequence Array (USA) fixup in place on a single MFT record buffer.
/// Returns `false` if the signature is not `"FILE"` or the USA header/sector words are invalid.
pub fn apply_usa_fixup(rec: &mut [u8], bytes_per_sector: usize) -> bool {
    if rec.len() < 48 || bytes_per_sector < 512 || &rec[0..4] != b"FILE" {
        return false;
    }
    let Some(usa_offset) = read_u16(rec, 4).map(usize::from) else {
        return false;
    };
    let Some(usa_count) = read_u16(rec, 6).map(usize::from) else {
        return false;
    };
    if usa_count < 2 || usa_offset + usa_count * 2 > rec.len() {
        return false;
    }
    let usn0 = rec[usa_offset];
    let usn1 = rec[usa_offset + 1];

    for sector_idx in 1..usa_count {
        let sector_end = sector_idx.saturating_mul(bytes_per_sector);
        if sector_end < 2 || sector_end > rec.len() {
            return false;
        }
        let end_off = sector_end - 2;
        if rec[end_off] != usn0 || rec[end_off + 1] != usn1 {
            return false;
        }
        let fix_off = usa_offset + sector_idx * 2;
        rec[end_off] = rec[fix_off];
        rec[end_off + 1] = rec[fix_off + 1];
    }
    true
}

/// Decodes NTFS mapping pairs (data runs) into physical `(Option<lcn>, cluster_count)` extents.
pub fn decode_data_runs(runs: &[u8]) -> Result<Vec<MftExtent>, String> {
    let mut extents = Vec::new();
    let mut pos = 0usize;
    let mut current_lcn: i64 = 0;

    while pos < runs.len() {
        let header = runs[pos];
        if header == 0 {
            break;
        }
        pos += 1;

        let len_bytes = (header & 0x0F) as usize;
        let off_bytes = ((header >> 4) & 0x0F) as usize;
        if len_bytes == 0 || len_bytes > 8 || off_bytes > 8 {
            return Err("Invalid NTFS data run descriptor.".to_string());
        }
        if pos + len_bytes + off_bytes > runs.len() {
            return Err("Truncated NTFS data run.".to_string());
        }

        let mut cluster_count: u64 = 0;
        for i in 0..len_bytes {
            cluster_count |= u64::from(runs[pos + i]) << (i * 8);
        }
        pos += len_bytes;

        if off_bytes == 0 {
            extents.push(MftExtent {
                lcn: None,
                cluster_count,
            });
            continue;
        }

        // Sign-extend little-endian delta LCN
        let mut delta_u64: u64 = 0;
        for i in 0..off_bytes {
            delta_u64 |= u64::from(runs[pos + i]) << (i * 8);
        }
        let sign_bit = 1u64 << (off_bytes * 8 - 1);
        let delta: i64 = if off_bytes < 8 && (delta_u64 & sign_bit) != 0 {
            let mask = !((1u64 << (off_bytes * 8)) - 1);
            (delta_u64 | mask) as i64
        } else {
            delta_u64 as i64
        };
        pos += off_bytes;

        current_lcn = current_lcn
            .checked_add(delta)
            .ok_or_else(|| "NTFS LCN overflow in data runs.".to_string())?;
        if current_lcn < 0 {
            return Err("Negative NTFS LCN in data runs.".to_string());
        }
        extents.push(MftExtent {
            lcn: Some(current_lcn as u64),
            cluster_count,
        });
    }

    if extents.is_empty() {
        return Err("No data runs found in $MFT record.".to_string());
    }
    Ok(extents)
}

/// Extracts `$MFT` data runs from Record #0 (after USA fixup).
/// Returns `Err` if `$MFT` uses an `$ATTRIBUTE_LIST` (caller transparently falls back).
pub fn extract_mft_extents_from_record0(rec: &[u8]) -> Result<Vec<MftExtent>, String> {
    let mut off = read_u16(rec, 20).ok_or("Missing first_attr_offset")? as usize;
    while off + 16 <= rec.len() {
        let attr_type = read_u32(rec, off).unwrap_or(ATTR_END);
        if attr_type == ATTR_END || attr_type == 0 {
            break;
        }
        let attr_len = read_u32(rec, off + 4).unwrap_or(0) as usize;
        if attr_len < 16 || off + attr_len > rec.len() {
            break;
        }
        if attr_type == ATTR_ATTRIBUTE_LIST {
            return Err("Fragmented $MFT uses $ATTRIBUTE_LIST; falling back to parallel walk.".to_string());
        }
        if attr_type == ATTR_DATA {
            let non_resident = rec[off + 8] != 0;
            let name_len = rec[off + 9];
            if non_resident && name_len == 0 && attr_len >= 64 {
                let run_off = read_u16(rec, off + 32).unwrap_or(0) as usize;
                if run_off < attr_len {
                    return decode_data_runs(&rec[off + run_off..off + attr_len]);
                }
            }
        }
        off += attr_len;
    }
    Err("Could not locate non-resident $DATA attribute in $MFT record #0.".to_string())
}

/// Parses a fixup-applied NTFS MFT record into `ParsedMftRecord`.
/// Returns `None` for deleted/unused records.
pub fn parse_mft_record(rec: &[u8], record_id: u64) -> Option<ParsedMftRecord> {
    if rec.len() < 48 || &rec[0..4] != b"FILE" {
        return None;
    }
    let rec_flags = read_u16(rec, 22)?;
    if rec_flags & MFT_RECORD_IN_USE == 0 {
        return None;
    }

    let base_ref = read_u64(rec, 32)?;
    let base_record_id = base_ref & 0x0000_FFFF_FFFF_FFFF;

    let mut flags = NodeFlags::empty();
    if rec_flags & MFT_RECORD_IS_DIR != 0 {
        flags |= NodeFlags::IS_DIR;
    }

    let mut parent_id: u64 = 0;
    let mut name: Option<String> = None;
    let mut name_priority: u8 = 0;
    let mut logical_size: u64 = 0;
    let mut allocated_size: u64 = 0;
    let mut has_data_size = false;
    let mut modified_secs: i64 = 0;
    let mut std_info_modified = false;

    let mut off = read_u16(rec, 20)? as usize;
    while off + 16 <= rec.len() {
        let attr_type = read_u32(rec, off)?;
        if attr_type == ATTR_END || attr_type == 0 {
            break;
        }
        let attr_len = read_u32(rec, off + 4)? as usize;
        if attr_len < 16 || off + attr_len > rec.len() {
            break;
        }
        let non_resident = rec[off + 8] != 0;
        let attr_name_len = rec[off + 9];
        let attr_flags = read_u16(rec, off + 12).unwrap_or(0);

        match attr_type {
            ATTR_STANDARD_INFO if !non_resident && attr_len >= 24 => {
                let val_len = read_u32(rec, off + 16).unwrap_or(0) as usize;
                let val_off = read_u16(rec, off + 20).unwrap_or(0) as usize;
                if val_len >= 36 && val_off + val_len <= attr_len {
                    let v = &rec[off + val_off..off + val_off + val_len];
                    if let Some(ft) = read_u64(v, 8) {
                        modified_secs = ntfs_filetime_to_unix_secs(ft);
                        std_info_modified = true;
                    }
                    if let Some(dos) = read_u32(v, 32) {
                        apply_dos_flags(dos, &mut flags);
                    }
                }
            }
            ATTR_FILE_NAME if !non_resident && attr_len >= 24 => {
                let val_len = read_u32(rec, off + 16).unwrap_or(0) as usize;
                let val_off = read_u16(rec, off + 20).unwrap_or(0) as usize;
                if val_len >= 66 && val_off + val_len <= attr_len {
                    let v = &rec[off + val_off..off + val_off + val_len];
                    let ns = v[65];
                    let prio = namespace_priority(ns);
                    let fname_units = v[64] as usize;
                    let byte_end = 66 + fname_units * 2;
                    if byte_end <= v.len() && (prio > name_priority || name.is_none()) {
                        let p_ref = read_u64(v, 0).unwrap_or(0) & 0x0000_FFFF_FFFF_FFFF;
                        let utf16: Vec<u16> = v[66..byte_end]
                            .chunks_exact(2)
                            .map(|c| u16::from_le_bytes([c[0], c[1]]))
                            .collect();
                        let decoded = String::from_utf16_lossy(&utf16);
                        parent_id = p_ref;
                        name = Some(decoded);
                        name_priority = prio;
                        if !std_info_modified
                            && let Some(ft) = read_u64(v, 16)
                        {
                            modified_secs = ntfs_filetime_to_unix_secs(ft);
                        }
                        if let Some(dos) = read_u32(v, 56) {
                            apply_dos_flags(dos, &mut flags);
                        }
                    }
                }
            }
            ATTR_DATA if attr_name_len == 0 && !flags.contains(NodeFlags::IS_DIR) => {
                if !non_resident && attr_len >= 24 {
                    let val_len = read_u32(rec, off + 16).unwrap_or(0) as u64;
                    logical_size = val_len;
                    allocated_size = 0; // Resident in MFT record
                    has_data_size = true;
                } else if non_resident && attr_len >= 64 {
                    let lowest_vcn = read_u64(rec, off + 16).unwrap_or(1);
                    if lowest_vcn == 0 {
                        let alloc = read_u64(rec, off + 40).unwrap_or(0);
                        let real = read_u64(rec, off + 48).unwrap_or(0);
                        let phys = if (attr_flags & (ATTR_FLAG_COMPRESSED | ATTR_FLAG_SPARSE)) != 0
                            && attr_len >= 72
                        {
                            flags |= NodeFlags::IS_COMPRESSED;
                            read_u64(rec, off + 64).unwrap_or(alloc)
                        } else {
                            alloc
                        };
                        logical_size = real;
                        allocated_size = phys;
                        has_data_size = true;
                    }
                }
            }
            ATTR_REPARSE_POINT => {
                flags |= NodeFlags::IS_SYMLINK;
            }
            _ => {}
        }

        off += attr_len;
    }

    Some(ParsedMftRecord {
        record_id,
        base_record_id,
        parent_id,
        name,
        name_priority,
        logical_size,
        allocated_size,
        has_data_size,
        modified_secs,
        flags,
    })
}

fn apply_dos_flags(dos: u32, flags: &mut NodeFlags) {
    if dos & DOS_READONLY != 0 {
        *flags |= NodeFlags::IS_READONLY;
    }
    if dos & DOS_HIDDEN != 0 {
        *flags |= NodeFlags::IS_HIDDEN;
    }
    if dos & DOS_SYSTEM != 0 {
        *flags |= NodeFlags::IS_SYSTEM;
    }
    if dos & DOS_REPARSE != 0 {
        *flags |= NodeFlags::IS_SYMLINK;
    }
    if dos & DOS_COMPRESSED != 0 {
        *flags |= NodeFlags::IS_COMPRESSED;
    }
}

struct MftEntry {
    parent_id: u64,
    name: String,
    name_priority: u8,
    logical_size: u64,
    allocated_size: u64,
    modified_secs: i64,
    flags: NodeFlags,
}

/// Assembles a `TempNode` volume subtree from parsed MFT base and extension records.
pub fn assemble_mft_tree(
    volume_root_name: SharedString,
    records: Vec<ParsedMftRecord>,
) -> TempNode {
    let mut base_entries: HashMap<u64, MftEntry> = HashMap::with_capacity(records.len());
    let mut extensions: Vec<ParsedMftRecord> = Vec::new();

    for rec in records {
        if rec.base_record_id != 0 {
            extensions.push(rec);
            continue;
        }
        // Skip internal NTFS metafiles (< 16) except root directory (#5).
        if rec.record_id < 16 && rec.record_id != NTFS_ROOT_RECORD_ID {
            continue;
        }
        let name = if rec.record_id == NTFS_ROOT_RECORD_ID {
            volume_root_name.to_string()
        } else {
            rec.name.unwrap_or_default()
        };
        base_entries.insert(
            rec.record_id,
            MftEntry {
                parent_id: rec.parent_id,
                name,
                name_priority: rec.name_priority,
                logical_size: rec.logical_size,
                allocated_size: rec.allocated_size,
                modified_secs: rec.modified_secs,
                flags: rec.flags,
            },
        );
    }

    // Merge extension records ($DATA sizes, better Win32 names, reparse flags) into base records.
    for ext in extensions {
        if let Some(base) = base_entries.get_mut(&ext.base_record_id) {
            if ext.has_data_size && base.logical_size == 0 && !base.flags.contains(NodeFlags::IS_DIR)
            {
                base.logical_size = ext.logical_size;
                base.allocated_size = ext.allocated_size;
            }
            if let Some(ext_name) = ext.name
                && (ext.name_priority > base.name_priority || base.name.is_empty())
            {
                base.name = ext_name;
                base.name_priority = ext.name_priority;
                base.parent_id = ext.parent_id;
            }
            base.flags |= ext.flags;
            if base.modified_secs == 0 {
                base.modified_secs = ext.modified_secs;
            }
        }
    }

    let mut children_map: HashMap<u64, Vec<u64>> = HashMap::with_capacity(base_entries.len() / 6);
    for (&rec_id, entry) in &base_entries {
        if rec_id == NTFS_ROOT_RECORD_ID || entry.name.is_empty() {
            continue;
        }
        // Avoid self-parent cycles
        if entry.parent_id == rec_id {
            continue;
        }
        children_map.entry(entry.parent_id).or_default().push(rec_id);
    }

    fn build_node(
        rec_id: u64,
        entries: &HashMap<u64, MftEntry>,
        children_map: &HashMap<u64, Vec<u64>>,
        depth: u16,
    ) -> Option<TempNode> {
        let entry = entries.get(&rec_id)?;
        let is_symlink = entry.flags.contains(NodeFlags::IS_SYMLINK);
        let is_dir = entry.flags.contains(NodeFlags::IS_DIR);

        let children = if is_dir && !is_symlink && depth < 256 {
            children_map
                .get(&rec_id)
                .map(|kids| {
                    kids.iter()
                        .filter_map(|&cid| build_node(cid, entries, children_map, depth + 1))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let (logical_size, allocated_size) = if is_dir || is_symlink {
            (0, 0)
        } else {
            (entry.logical_size, entry.allocated_size)
        };

        Some(TempNode {
            name: entry.name.clone().into_boxed_str(),
            logical_size,
            allocated_size,
            modified_secs: entry.modified_secs,
            flags: entry.flags,
            children,
        })
    }

    let mut root = build_node(NTFS_ROOT_RECORD_ID, &base_entries, &children_map, 0).unwrap_or_else(
        || TempNode {
            name: volume_root_name.to_string().into_boxed_str(),
            logical_size: 0,
            allocated_size: 0,
            modified_secs: 0,
            flags: NodeFlags::IS_DIR | NodeFlags::IS_VOLUME,
            children: Vec::new(),
        },
    );
    root.name = volume_root_name.to_string().into_boxed_str();
    root.flags |= NodeFlags::IS_DIR | NodeFlags::IS_VOLUME;
    root
}

/// Attempts a direct NTFS MFT scan of `short_name` (e.g. `"C:"`) on Windows.
/// Returns `Err` if elevated volume access is unavailable or `$MFT` layout requires fallback.
#[cfg(target_os = "windows")]
pub fn scan_volume_mft(
    short_name: &str,
    root_path_str: &str,
    counters: &ScanCounters,
    cancel: &AtomicBool,
) -> Result<TempNode, String> {
    use std::path::Path;
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_BEGIN, FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile, SetFilePointerEx,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::{FSCTL_GET_NTFS_VOLUME_DATA, NTFS_VOLUME_DATA_BUFFER};

    let dev_path = format!(r"\\.\{}", short_name.trim_end_matches(['\\', '/']));
    let wide_dev: Vec<u16> = dev_path.encode_utf16().chain(std::iter::once(0)).collect();

    let handle: HANDLE = unsafe {
        CreateFileW(
            wide_dev.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_SEQUENTIAL_SCAN,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err("Failed to open raw NTFS volume handle (requires Administrator).".to_string());
    }

    struct HandleGuard(HANDLE);
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let _guard = HandleGuard(handle);

    let mut vol_data: NTFS_VOLUME_DATA_BUFFER = unsafe { std::mem::zeroed() };
    let mut bytes_returned: u32 = 0;
    let ioctl_ok = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_GET_NTFS_VOLUME_DATA,
            std::ptr::null(),
            0,
            (&mut vol_data as *mut NTFS_VOLUME_DATA_BUFFER).cast(),
            std::mem::size_of::<NTFS_VOLUME_DATA_BUFFER>() as u32,
            &mut bytes_returned,
            std::ptr::null_mut(),
        )
    };
    if ioctl_ok == 0 {
        return Err("FSCTL_GET_NTFS_VOLUME_DATA failed.".to_string());
    }

    let bytes_per_sector = vol_data.BytesPerSector as usize;
    let bytes_per_cluster = vol_data.BytesPerCluster as u64;
    let record_size = vol_data.BytesPerFileRecordSegment as usize;
    let mft_valid_len = vol_data.MftValidDataLength.max(0) as u64;
    let mft_start_lcn = vol_data.MftStartLcn.max(0) as u64;

    if bytes_per_sector < 512
        || bytes_per_cluster == 0
        || record_size < 512
        || !record_size.is_power_of_two()
        || mft_valid_len < record_size as u64
    {
        return Err("Unexpected NTFS volume geometry.".to_string());
    }

    // Read Record #0 ($MFT) to decode physical disk extents of the MFT.
    let mft_start_byte = mft_start_lcn
        .checked_mul(bytes_per_cluster)
        .ok_or("MFT start offset overflow")?;
    let mut rec0 = vec![0u8; record_size.max(bytes_per_sector)];
    seek_and_read_exact(handle, mft_start_byte, &mut rec0)?;
    if !apply_usa_fixup(&mut rec0[..record_size], bytes_per_sector) {
        return Err("Invalid USA fixup on $MFT record #0.".to_string());
    }
    let extents = extract_mft_extents_from_record0(&rec0[..record_size])?;

    let total_records = (mft_valid_len / record_size as u64) as usize;
    let mut parsed_records: Vec<ParsedMftRecord> = Vec::with_capacity(total_records / 2);

    counters.maybe_set_path(Path::new(&format!("{root_path_str} [NTFS MFT]")));

    // Stream each MFT extent in aligned 4 MiB chunks.
    const CHUNK_TARGET: usize = 4 * 1024 * 1024;
    let chunk_size = (CHUNK_TARGET / record_size).max(1) * record_size;
    let mut buf = vec![0u8; chunk_size];

    let mut bytes_processed: u64 = 0;
    let mut current_record_id: u64 = 0;

    'extents: for extent in extents {
        let extent_bytes = extent.cluster_count.saturating_mul(bytes_per_cluster);
        let remaining_valid = mft_valid_len.saturating_sub(bytes_processed);
        if remaining_valid == 0 {
            break;
        }
        let to_read_extent = extent_bytes.min(remaining_valid);

        let Some(lcn) = extent.lcn else {
            // Sparse extent inside MFT: advance record counter without reading disk.
            bytes_processed += to_read_extent;
            current_record_id += to_read_extent / record_size as u64;
            continue;
        };

        let mut disk_offset = lcn
            .checked_mul(bytes_per_cluster)
            .ok_or("MFT extent offset overflow")?;
        let mut extent_done: u64 = 0;

        while extent_done < to_read_extent {
            if cancel.load(Ordering::Relaxed) {
                return Err("Scan cancelled".to_string());
            }
            let want = ((to_read_extent - extent_done) as usize).min(buf.len());
            // Align read size down to sector/record boundary.
            let aligned_want = (want / record_size) * record_size;
            if aligned_want == 0 {
                break;
            }

            let mut new_pos: i64 = 0;
            let seek_ok = unsafe {
                SetFilePointerEx(handle, disk_offset as i64, &mut new_pos, FILE_BEGIN)
            };
            if seek_ok == 0 {
                return Err("Failed seeking raw NTFS volume.".to_string());
            }

            let mut got: u32 = 0;
            let read_ok = unsafe {
                ReadFile(
                    handle,
                    buf.as_mut_ptr(),
                    aligned_want as u32,
                    &mut got,
                    std::ptr::null_mut(),
                )
            };
            if read_ok == 0 || got == 0 {
                break 'extents;
            }

            let usable = ((got as usize) / record_size) * record_size;
            let mut local_files: u64 = 0;
            let mut local_dirs: u64 = 0;
            let mut local_bytes: u64 = 0;

            for chunk in buf[..usable].chunks_exact_mut(record_size) {
                let rec_id = current_record_id;
                current_record_id += 1;

                if !apply_usa_fixup(chunk, bytes_per_sector) {
                    continue;
                }
                if let Some(parsed) = parse_mft_record(chunk, rec_id) {
                    if parsed.base_record_id == 0 && (rec_id >= 16 || rec_id == NTFS_ROOT_RECORD_ID)
                    {
                        if parsed.flags.contains(NodeFlags::IS_DIR) {
                            local_dirs += 1;
                        } else {
                            local_files += 1;
                            local_bytes = local_bytes.saturating_add(parsed.logical_size);
                        }
                    }
                    parsed_records.push(parsed);
                }
            }

            counters.files.fetch_add(local_files, Ordering::Relaxed);
            counters.dirs.fetch_add(local_dirs, Ordering::Relaxed);
            counters.bytes.fetch_add(local_bytes, Ordering::Relaxed);
            counters.maybe_set_path(Path::new(&format!(
                "{root_path_str} [MFT {:.0}%]",
                ((bytes_processed + usable as u64) as f64 / mft_valid_len.max(1) as f64 * 100.0)
                    .clamp(0.0, 100.0)
            )));

            disk_offset += usable as u64;
            extent_done += usable as u64;
            bytes_processed += usable as u64;
        }
    }

    if cancel.load(Ordering::Relaxed) {
        return Err("Scan cancelled".to_string());
    }

    Ok(assemble_mft_tree(
        SharedString::from(root_path_str.to_string()),
        parsed_records,
    ))
}

#[cfg(target_os = "windows")]
fn seek_and_read_exact(
    handle: windows_sys::Win32::Foundation::HANDLE,
    offset: u64,
    buf: &mut [u8],
) -> Result<(), String> {
    use windows_sys::Win32::Storage::FileSystem::{FILE_BEGIN, ReadFile, SetFilePointerEx};
    let mut new_pos: i64 = 0;
    let seek_ok = unsafe { SetFilePointerEx(handle, offset as i64, &mut new_pos, FILE_BEGIN) };
    if seek_ok == 0 {
        return Err("Failed seeking to $MFT record #0.".to_string());
    }
    let mut got: u32 = 0;
    let read_ok = unsafe {
        ReadFile(
            handle,
            buf.as_mut_ptr(),
            buf.len() as u32,
            &mut got,
            std::ptr::null_mut(),
        )
    };
    if read_ok == 0 || (got as usize) < buf.len() {
        return Err("Failed reading $MFT record #0.".to_string());
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn scan_volume_mft(
    _short_name: &str,
    _root_path_str: &str,
    _counters: &ScanCounters,
    _cancel: &AtomicBool,
) -> Result<TempNode, String> {
    Err("NTFS MFT scanning is only supported on Windows.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_synthetic_mft_record(
        is_dir: bool,
        parent_id: u64,
        name: &str,
        logical_size: u64,
        allocated_size: u64,
    ) -> Vec<u8> {
        let mut rec = vec![0u8; 1024];
        rec[0..4].copy_from_slice(b"FILE");
        // USA at offset 48, 3 entries (1 USN + 2 sectors of 512 bytes)
        rec[4..6].copy_from_slice(&48u16.to_le_bytes());
        rec[6..8].copy_from_slice(&3u16.to_le_bytes());
        // First attribute at offset 56
        rec[20..22].copy_from_slice(&56u16.to_le_bytes());
        let flags = MFT_RECORD_IN_USE | if is_dir { MFT_RECORD_IS_DIR } else { 0 };
        rec[22..24].copy_from_slice(&flags.to_le_bytes());

        // Write $FILE_NAME attribute at offset 56
        let mut off = 56usize;
        let utf16: Vec<u16> = name.encode_utf16().collect();
        let fn_val_len = 66 + utf16.len() * 2;
        let fn_attr_len = (24 + fn_val_len + 7) & !7;
        rec[off..off + 4].copy_from_slice(&ATTR_FILE_NAME.to_le_bytes());
        rec[off + 4..off + 8].copy_from_slice(&(fn_attr_len as u32).to_le_bytes());
        rec[off + 8] = 0; // resident
        rec[off + 16..off + 20].copy_from_slice(&(fn_val_len as u32).to_le_bytes());
        rec[off + 20..off + 22].copy_from_slice(&24u16.to_le_bytes());
        let v = off + 24;
        rec[v..v + 8].copy_from_slice(&parent_id.to_le_bytes());
        rec[v + 64] = utf16.len() as u8;
        rec[v + 65] = 1; // Win32 namespace
        for (i, &u) in utf16.iter().enumerate() {
            rec[v + 66 + i * 2..v + 68 + i * 2].copy_from_slice(&u.to_le_bytes());
        }
        off += fn_attr_len;

        // Write non-resident $DATA attribute for files
        if !is_dir {
            let data_attr_len = 72usize;
            rec[off..off + 4].copy_from_slice(&ATTR_DATA.to_le_bytes());
            rec[off + 4..off + 8].copy_from_slice(&(data_attr_len as u32).to_le_bytes());
            rec[off + 8] = 1; // non-resident
            rec[off + 9] = 0; // unnamed default stream
            rec[off + 16..off + 24].copy_from_slice(&0u64.to_le_bytes()); // lowest_vcn = 0
            rec[off + 40..off + 48].copy_from_slice(&allocated_size.to_le_bytes());
            rec[off + 48..off + 56].copy_from_slice(&logical_size.to_le_bytes());
            off += data_attr_len;
        }

        // End marker
        rec[off..off + 4].copy_from_slice(&ATTR_END.to_le_bytes());

        // Apply USA fixup words (USN = 0x1234)
        let usn = 0x1234u16.to_le_bytes();
        let s1 = [rec[510], rec[511]];
        let s2 = [rec[1022], rec[1023]];
        rec[48..50].copy_from_slice(&usn);
        rec[50..52].copy_from_slice(&s1);
        rec[52..54].copy_from_slice(&s2);
        rec[510..512].copy_from_slice(&usn);
        rec[1022..1024].copy_from_slice(&usn);
        rec
    }

    #[test]
    fn decodes_positive_negative_and_sparse_data_runs() {
        // Run 1: len=0x10 (1 byte), lcn=+0x0100 (2 bytes) -> header 0x21
        // Run 2: len=0x05 (1 byte), lcn=-0x10 (1 byte signed 0xF0) -> header 0x11
        // Run 3: sparse len=0x04 (1 byte), off=0 -> header 0x01
        let runs = [0x21, 0x10, 0x00, 0x01, 0x11, 0x05, 0xF0, 0x01, 0x04, 0x00];
        let extents = decode_data_runs(&runs).unwrap();
        assert_eq!(
            extents,
            vec![
                MftExtent {
                    lcn: Some(0x0100),
                    cluster_count: 16
                },
                MftExtent {
                    lcn: Some(0x00F0),
                    cluster_count: 5
                },
                MftExtent {
                    lcn: None,
                    cluster_count: 4
                },
            ]
        );
    }

    #[test]
    fn parses_synthetic_mft_records_and_assembles_tree() {
        let mut root_rec = make_synthetic_mft_record(true, 5, ".", 0, 0);
        let mut dir_rec = make_synthetic_mft_record(true, 5, "Games", 0, 0);
        let mut file_rec = make_synthetic_mft_record(false, 42, "world.pak", 12_345_678, 12_349_440);

        assert!(apply_usa_fixup(&mut root_rec, 512));
        assert!(apply_usa_fixup(&mut dir_rec, 512));
        assert!(apply_usa_fixup(&mut file_rec, 512));

        let parsed = vec![
            parse_mft_record(&root_rec, 5).unwrap(),
            parse_mft_record(&dir_rec, 42).unwrap(),
            parse_mft_record(&file_rec, 100).unwrap(),
        ];

        let tree = assemble_mft_tree(SharedString::from("C:\\"), parsed);
        assert_eq!(tree.name.as_ref(), "C:\\");
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].name.as_ref(), "Games");
        assert_eq!(tree.children[0].children.len(), 1);
        assert_eq!(tree.children[0].children[0].name.as_ref(), "world.pak");
        assert_eq!(tree.children[0].children[0].logical_size, 12_345_678);
        assert_eq!(tree.children[0].children[0].allocated_size, 12_349_440);
    }
}
