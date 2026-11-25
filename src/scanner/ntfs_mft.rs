//! Adapter connecting `crate::mft_raw` (shared with `textree-service.exe`)
//! into `TexTree`'s `TempNode` scanner pipeline.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::mft_raw;
use crate::models::NodeFlags;
use crate::scanner::parallel_walk::{ScanCounters, TempNode};
use crate::service_protocol::WireNode;

/// Converts a flat DFS-ordered `Vec<WireNode>` (from direct MFT or MFT Service IPC)
/// into a nested `TempNode` tree in linear O(N) time without recursion.
pub fn wire_nodes_to_temp_node(volume_root_name: &str, nodes: Vec<WireNode>) -> TempNode {
    if nodes.is_empty() {
        return TempNode {
            name: volume_root_name.to_string().into_boxed_str(),
            logical_size: 0,
            allocated_size: 0,
            modified_secs: 0,
            flags: NodeFlags::IS_DIR | NodeFlags::IS_VOLUME,
            children: Vec::new(),
        };
    }

    let len = nodes.len();
    let mut children_buckets: Vec<Vec<TempNode>> = (0..len).map(|_| Vec::new()).collect();
    let mut root_opt: Option<TempNode> = None;

    for (idx, wn) in nodes.into_iter().enumerate().rev() {
        let mut kids = std::mem::take(&mut children_buckets[idx]);
        kids.reverse();
        let node = TempNode {
            name: wn.name.into_boxed_str(),
            logical_size: wn.logical_size,
            allocated_size: wn.allocated_size,
            modified_secs: wn.modified_secs,
            flags: NodeFlags::from_bits_truncate(wn.flags),
            children: kids,
        };
        if idx == 0 || wn.parent == u32::MAX {
            root_opt = Some(node);
        } else if (wn.parent as usize) < idx {
            children_buckets[wn.parent as usize].push(node);
        }
    }

    let mut root = root_opt.unwrap_or_else(|| TempNode {
        name: volume_root_name.to_string().into_boxed_str(),
        logical_size: 0,
        allocated_size: 0,
        modified_secs: 0,
        flags: NodeFlags::IS_DIR | NodeFlags::IS_VOLUME,
        children: Vec::new(),
    });
    root.name = volume_root_name.to_string().into_boxed_str();
    root.flags |= NodeFlags::IS_DIR | NodeFlags::IS_VOLUME;
    root
}

/// Attempts a direct in-process NTFS MFT scan of `short_name` (e.g. `"C:"`) on Windows.
pub fn scan_volume_mft(
    short_name: &str,
    root_path_str: &str,
    counters: &ScanCounters,
    cancel: &AtomicBool,
) -> Result<TempNode, String> {
    counters.maybe_set_path(Path::new(&format!("{root_path_str} [NTFS MFT]")));
    let wire_nodes = mft_raw::scan_volume_mft_wire(
        short_name,
        root_path_str,
        cancel,
        |delta_files, delta_dirs, delta_bytes, pct| {
            counters.files.fetch_add(delta_files, Ordering::Relaxed);
            counters.dirs.fetch_add(delta_dirs, Ordering::Relaxed);
            counters.bytes.fetch_add(delta_bytes, Ordering::Relaxed);
            counters.maybe_set_path(Path::new(&format!("{root_path_str} [MFT {pct}%]")));
        },
    )?;
    Ok(wire_nodes_to_temp_node(root_path_str, wire_nodes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service_protocol::{FLAG_IS_DIR, FLAG_IS_VOLUME};

    #[test]
    fn converts_wire_nodes_into_temp_node_hierarchy() {
        let wire = vec![
            WireNode {
                parent: u32::MAX,
                logical_size: 0,
                allocated_size: 0,
                modified_secs: 0,
                flags: FLAG_IS_DIR | FLAG_IS_VOLUME,
                name: "C:\\".to_string(),
            },
            WireNode {
                parent: 0,
                logical_size: 0,
                allocated_size: 0,
                modified_secs: 0,
                flags: FLAG_IS_DIR,
                name: "Games".to_string(),
            },
            WireNode {
                parent: 1,
                logical_size: 12_345_678,
                allocated_size: 12_349_440,
                modified_secs: 100,
                flags: 0,
                name: "world.pak".to_string(),
            },
        ];

        let tree = wire_nodes_to_temp_node("C:\\", wire);
        assert_eq!(tree.name.as_ref(), "C:\\");
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].name.as_ref(), "Games");
        assert_eq!(tree.children[0].children.len(), 1);
        assert_eq!(tree.children[0].children[0].name.as_ref(), "world.pak");
        assert_eq!(tree.children[0].children[0].logical_size, 12_345_678);
    }
}
