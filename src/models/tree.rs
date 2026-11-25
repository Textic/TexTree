use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use bitflags::bitflags;
use gpui_kit::SharedString;

pub type NodeId = u32;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct NodeFlags: u16 {
        const IS_DIR        = 1 << 0;
        const IS_VOLUME     = 1 << 1;
        const IS_VIRTUAL    = 1 << 2;
        const ACCESS_DENIED = 1 << 3;
        const IS_SYMLINK    = 1 << 4;
        const IS_HIDDEN     = 1 << 5;
        const IS_SYSTEM     = 1 << 6;
        const IS_READONLY   = 1 << 7;
        const IS_COMPRESSED = 1 << 8;
    }
}

#[derive(Debug, Clone)]
pub struct FileNode {
    pub name: SharedString,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub file_count: u64,
    pub dir_count: u64,
    pub denied_count: u32,
    pub modified_secs: i64,
    pub flags: NodeFlags,
    pub ext_id: u16,
    pub removed: bool,
}

impl FileNode {
    pub fn is_container(&self) -> bool {
        self.flags
            .intersects(NodeFlags::IS_DIR | NodeFlags::IS_VOLUME | NodeFlags::IS_VIRTUAL)
    }

    pub fn is_file(&self) -> bool {
        !self.is_container()
    }

    pub fn total_items(&self) -> u64 {
        self.file_count.saturating_add(self.dir_count)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortColumn {
    Name,
    Percent,
    #[default]
    Size,
    Allocated,
    Items,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortSpec {
    pub column: SortColumn,
    pub descending: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            column: SortColumn::Size,
            descending: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleRow {
    pub node_id: NodeId,
    pub depth: u16,
    pub has_children: bool,
    pub is_expanded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionStat {
    pub extension: String,
    pub total_size: u64,
    pub allocated_size: u64,
    pub file_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScanEngineKind {
    #[default]
    ParallelWalk,
    NtfsMft,
    NtfsMftService,
    Hybrid,
}

impl ScanEngineKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ParallelWalk => "Parallel Walk",
            Self::NtfsMft => "NTFS MFT",
            Self::NtfsMftService => "MFT Service",
            Self::Hybrid => "MFT + Parallel",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiskTree {
    pub root: NodeId,
    /// Real filesystem path of `root` when scanning a single drive or folder;
    /// `None` when `root` is the virtual "This PC" container.
    pub root_base_path: Option<PathBuf>,
    pub nodes: Vec<FileNode>,
    /// Interned lowercase extensions without leading dot. Index 0 is `""`.
    pub extensions: Vec<Box<str>>,
    pub scan_duration_ms: u64,
    pub target_label: String,
    pub engine: ScanEngineKind,
}

impl DiskTree {
    pub fn new(
        root_name: impl Into<SharedString>,
        root_flags: NodeFlags,
        root_base_path: Option<PathBuf>,
        target_label: impl Into<String>,
    ) -> Self {
        let root_node = FileNode {
            name: root_name.into(),
            parent: None,
            children: Vec::new(),
            logical_size: 0,
            allocated_size: 0,
            file_count: 0,
            dir_count: 0,
            denied_count: 0,
            modified_secs: 0,
            flags: root_flags,
            ext_id: 0,
            removed: false,
        };
        Self {
            root: 0,
            root_base_path,
            nodes: vec![root_node],
            extensions: vec![Box::from("")],
            scan_duration_ms: 0,
            target_label: target_label.into(),
            engine: ScanEngineKind::ParallelWalk,
        }
    }

    #[inline]
    pub fn get(&self, id: NodeId) -> Option<&FileNode> {
        self.nodes.get(id as usize)
    }

    #[inline]
    pub fn root_node(&self) -> &FileNode {
        &self.nodes[self.root as usize]
    }

    pub fn extension_of(&self, id: NodeId) -> &str {
        let Some(node) = self.get(id) else {
            return "";
        };
        self.extensions
            .get(node.ext_id as usize)
            .map(|s| s.as_ref())
            .unwrap_or("")
    }

    /// Reconstructs the full filesystem path of a node by walking its parent chain.
    /// Returns `None` for virtual nodes such as `This PC`.
    pub fn full_path(&self, id: NodeId) -> Option<PathBuf> {
        let node = self.get(id)?;
        if node.flags.contains(NodeFlags::IS_VIRTUAL) {
            return None;
        }

        let mut chain = Vec::with_capacity(16);
        let mut cursor = Some(id);
        while let Some(cur_id) = cursor {
            let cur = self.get(cur_id)?;
            if cur.flags.contains(NodeFlags::IS_VIRTUAL) {
                break;
            }
            chain.push(cur_id);
            cursor = cur.parent;
        }
        chain.reverse();
        let top_id = *chain.first()?;

        let mut path = if top_id == self.root {
            self.root_base_path
                .clone()
                .unwrap_or_else(|| PathBuf::from(self.nodes[top_id as usize].name.as_ref()))
        } else {
            PathBuf::from(self.nodes[top_id as usize].name.as_ref())
        };

        for &seg_id in chain.iter().skip(1) {
            path.push(self.nodes[seg_id as usize].name.as_ref());
        }
        Some(path)
    }

    pub fn parent_percent(&self, id: NodeId) -> f32 {
        let Some(node) = self.get(id) else {
            return 0.0;
        };
        let parent_size = node
            .parent
            .and_then(|pid| self.get(pid))
            .map(|p| p.logical_size)
            .unwrap_or(node.logical_size);
        if parent_size == 0 {
            if id == self.root { 1.0 } else { 0.0 }
        } else {
            (node.logical_size as f64 / parent_size as f64).clamp(0.0, 1.0) as f32
        }
    }

    pub fn root_percent(&self, id: NodeId) -> f32 {
        let Some(node) = self.get(id) else {
            return 0.0;
        };
        let root_size = self.root_node().logical_size;
        if root_size == 0 {
            if id == self.root { 1.0 } else { 0.0 }
        } else {
            (node.logical_size as f64 / root_size as f64).clamp(0.0, 1.0) as f32
        }
    }

    /// Computes bottom-up recursive totals across the entire arena and sorts every
    /// container's children according to `sort`.
    pub fn aggregate_and_sort(&mut self, sort: SortSpec) {
        if self.nodes.is_empty() {
            return;
        }
        // Build iterative post-order traversal so even deeply nested trees never blow the stack.
        let mut order = Vec::with_capacity(self.nodes.len());
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            order.push(id);
            for &child in &self.nodes[id as usize].children {
                if !self.nodes[child as usize].removed {
                    stack.push(child);
                }
            }
        }

        for &id in order.iter().rev() {
            let is_container = self.nodes[id as usize].is_container();
            if !is_container {
                continue;
            }
            let children = std::mem::take(&mut self.nodes[id as usize].children);
            let mut logical_sum: u64 = 0;
            let mut allocated_sum: u64 = 0;
            let mut file_sum: u64 = 0;
            let mut dir_sum: u64 = 0;
            let mut denied_sum: u32 = u32::from(
                self.nodes[id as usize]
                    .flags
                    .contains(NodeFlags::ACCESS_DENIED),
            );
            let mut max_mod = self.nodes[id as usize].modified_secs;

            for &cid in &children {
                let c = &self.nodes[cid as usize];
                if c.removed {
                    continue;
                }
                logical_sum = logical_sum.saturating_add(c.logical_size);
                allocated_sum = allocated_sum.saturating_add(c.allocated_size);
                if c.is_container() {
                    file_sum = file_sum.saturating_add(c.file_count);
                    dir_sum = dir_sum.saturating_add(c.dir_count).saturating_add(1);
                    denied_sum = denied_sum.saturating_add(c.denied_count);
                } else {
                    file_sum = file_sum.saturating_add(1);
                    if c.flags.contains(NodeFlags::ACCESS_DENIED) {
                        denied_sum = denied_sum.saturating_add(1);
                    }
                }
                if c.modified_secs > max_mod {
                    max_mod = c.modified_secs;
                }
            }

            let node = &mut self.nodes[id as usize];
            node.logical_size = logical_sum;
            node.allocated_size = allocated_sum;
            node.file_count = file_sum;
            node.dir_count = dir_sum;
            node.denied_count = denied_sum;
            if node.modified_secs == 0 {
                node.modified_secs = max_mod;
            }
            node.children = children;
        }

        self.sort_children(sort);
    }

    pub fn sort_children(&mut self, sort: SortSpec) {
        let len = self.nodes.len();
        for idx in 0..len {
            if self.nodes[idx].children.len() <= 1 {
                continue;
            }
            let is_virtual = self.nodes[idx].flags.contains(NodeFlags::IS_VIRTUAL);
            let mut children = std::mem::take(&mut self.nodes[idx].children);
            if is_virtual {
                // Keep top-level drives under "This PC" ordered A -> Z to match the sidebar.
                children.sort_unstable_by(|&a, &b| {
                    self.nodes[a as usize]
                        .name
                        .to_ascii_lowercase()
                        .cmp(&self.nodes[b as usize].name.to_ascii_lowercase())
                });
            } else {
                children.sort_unstable_by(|&a, &b| compare_nodes(&self.nodes, a, b, sort));
            }
            self.nodes[idx].children = children;
        }
    }

    /// Produces a flat list of visible rows for virtualized rendering (`uniform_list`).
    pub fn flatten_visible(
        &self,
        expanded: &HashSet<NodeId>,
        filter: &str,
        show_hidden: bool,
    ) -> Vec<VisibleRow> {
        let query = filter.trim().to_ascii_lowercase();
        if query.is_empty() {
            let mut rows = Vec::with_capacity(256);
            self.dfs_unfiltered(self.root, 0, expanded, show_hidden, &mut rows);
            return rows;
        }

        // Filtered mode: mark nodes matching query + all their ancestors so the user sees context.
        const MAX_FILTERED_ROWS: usize = 40_000;
        let mut keep = vec![false; self.nodes.len()];
        let mut matched_count = 0usize;

        for (idx, node) in self.nodes.iter().enumerate() {
            if node.removed {
                continue;
            }
            if !show_hidden
                && idx as NodeId != self.root
                && node.flags.contains(NodeFlags::IS_HIDDEN)
            {
                continue;
            }
            let name_match = node.name.to_ascii_lowercase().contains(&query);
            let ext_match = !node.is_container()
                && self
                    .extensions
                    .get(node.ext_id as usize)
                    .is_some_and(|ext| !ext.is_empty() && ext.contains(&query));
            if name_match || ext_match {
                let mut cursor = Some(idx as NodeId);
                while let Some(cid) = cursor {
                    if keep[cid as usize] {
                        break;
                    }
                    keep[cid as usize] = true;
                    cursor = self.nodes[cid as usize].parent;
                }
                matched_count += 1;
                if matched_count >= MAX_FILTERED_ROWS {
                    break;
                }
            }
        }

        if !keep[self.root as usize] {
            return Vec::new();
        }

        let mut rows = Vec::with_capacity(matched_count.min(4096));
        self.dfs_filtered(self.root, 0, &keep, show_hidden, MAX_FILTERED_ROWS, &mut rows);
        rows
    }

    fn dfs_unfiltered(
        &self,
        id: NodeId,
        depth: u16,
        expanded: &HashSet<NodeId>,
        show_hidden: bool,
        out: &mut Vec<VisibleRow>,
    ) {
        let node = &self.nodes[id as usize];
        if node.removed {
            return;
        }
        if !show_hidden && id != self.root && node.flags.contains(NodeFlags::IS_HIDDEN) {
            return;
        }

        let has_children = node.children.iter().any(|&cid| {
            let c = &self.nodes[cid as usize];
            !c.removed && (show_hidden || !c.flags.contains(NodeFlags::IS_HIDDEN))
        });
        let is_expanded = has_children && expanded.contains(&id);

        out.push(VisibleRow {
            node_id: id,
            depth,
            has_children,
            is_expanded,
        });

        if is_expanded {
            for &cid in &node.children {
                self.dfs_unfiltered(cid, depth.saturating_add(1), expanded, show_hidden, out);
            }
        }
    }

    fn dfs_filtered(
        &self,
        id: NodeId,
        depth: u16,
        keep: &[bool],
        show_hidden: bool,
        cap: usize,
        out: &mut Vec<VisibleRow>,
    ) {
        if out.len() >= cap || !keep[id as usize] {
            return;
        }
        let node = &self.nodes[id as usize];
        if node.removed {
            return;
        }
        if !show_hidden && id != self.root && node.flags.contains(NodeFlags::IS_HIDDEN) {
            return;
        }
        let has_children = node.children.iter().any(|&cid| keep[cid as usize]);
        out.push(VisibleRow {
            node_id: id,
            depth,
            has_children,
            is_expanded: has_children,
        });
        if has_children {
            for &cid in &node.children {
                if out.len() >= cap {
                    break;
                }
                self.dfs_filtered(cid, depth.saturating_add(1), keep, show_hidden, cap, out);
            }
        }
    }

    /// Returns the heaviest matching files (or denied items when `denied_only` is true).
    pub fn top_files(
        &self,
        filter: &str,
        ext_filter: Option<&str>,
        denied_only: bool,
        show_hidden: bool,
        limit: usize,
    ) -> Vec<NodeId> {
        let query = filter.trim().to_ascii_lowercase();
        let ext_wanted = ext_filter.map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase());

        let mut matches: Vec<NodeId> = Vec::with_capacity(1024);
        for (idx, node) in self.nodes.iter().enumerate() {
            if node.removed {
                continue;
            }
            if !show_hidden && node.flags.contains(NodeFlags::IS_HIDDEN) {
                continue;
            }
            if denied_only {
                if !node.flags.contains(NodeFlags::ACCESS_DENIED) {
                    continue;
                }
            } else if !node.is_file() {
                continue;
            }

            if let Some(ref ext) = ext_wanted {
                let node_ext = self
                    .extensions
                    .get(node.ext_id as usize)
                    .map(|s| s.as_ref())
                    .unwrap_or("");
                if ext == "(no extension)" {
                    if !node_ext.is_empty() {
                        continue;
                    }
                } else if node_ext != ext.as_str() {
                    continue;
                }
            }

            if !query.is_empty() {
                let name_hit = node.name.to_ascii_lowercase().contains(&query);
                if !name_hit {
                    let path_hit = self
                        .full_path(idx as NodeId)
                        .is_some_and(|p| p.to_string_lossy().to_ascii_lowercase().contains(&query));
                    if !path_hit {
                        continue;
                    }
                }
            }

            matches.push(idx as NodeId);
        }

        let cmp = |&a: &NodeId, &b: &NodeId| {
            self.nodes[b as usize]
                .logical_size
                .cmp(&self.nodes[a as usize].logical_size)
                .then_with(|| self.nodes[a as usize].name.cmp(&self.nodes[b as usize].name))
        };

        if matches.len() > limit {
            matches.select_nth_unstable_by(limit, cmp);
            matches.truncate(limit);
        }
        matches.sort_unstable_by(cmp);
        matches
    }

    pub fn extension_stats(&self, show_hidden: bool) -> Vec<ExtensionStat> {
        let mut buckets: Vec<(u64, u64, u64)> = vec![(0, 0, 0); self.extensions.len()];
        for node in &self.nodes {
            if node.removed || !node.is_file() {
                continue;
            }
            if !show_hidden && node.flags.contains(NodeFlags::IS_HIDDEN) {
                continue;
            }
            if let Some(entry) = buckets.get_mut(node.ext_id as usize) {
                entry.0 = entry.0.saturating_add(node.logical_size);
                entry.1 = entry.1.saturating_add(node.allocated_size);
                entry.2 = entry.2.saturating_add(1);
            }
        }

        let mut stats: Vec<ExtensionStat> = buckets
            .into_iter()
            .enumerate()
            .filter(|(_, (_, _, count))| *count > 0)
            .map(|(ext_id, (total_size, allocated_size, file_count))| {
                let raw = self
                    .extensions
                    .get(ext_id)
                    .map(|s| s.as_ref())
                    .unwrap_or("");
                let extension = if raw.is_empty() {
                    "(no extension)".to_string()
                } else {
                    format!(".{raw}")
                };
                ExtensionStat {
                    extension,
                    total_size,
                    allocated_size,
                    file_count,
                }
            })
            .collect();

        stats.sort_unstable_by(|a, b| {
            b.total_size
                .cmp(&a.total_size)
                .then_with(|| b.file_count.cmp(&a.file_count))
                .then_with(|| a.extension.cmp(&b.extension))
        });
        stats
    }

    pub fn largest_children(&self, id: NodeId, limit: usize) -> Vec<NodeId> {
        let Some(node) = self.get(id) else {
            return Vec::new();
        };
        let mut kids: Vec<NodeId> = node
            .children
            .iter()
            .copied()
            .filter(|&cid| !self.nodes[cid as usize].removed)
            .collect();
        kids.sort_unstable_by(|&a, &b| {
            self.nodes[b as usize]
                .logical_size
                .cmp(&self.nodes[a as usize].logical_size)
                .then_with(|| self.nodes[a as usize].name.cmp(&self.nodes[b as usize].name))
        });
        kids.truncate(limit);
        kids
    }

    pub fn ancestors_of(&self, id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut cursor = self.get(id).and_then(|n| n.parent);
        while let Some(pid) = cursor {
            result.push(pid);
            cursor = self.get(pid).and_then(|n| n.parent);
        }
        result
    }

    /// Returns `true` if `ancestor` is an ancestor of (or equal to) `descendant`.
    #[inline]
    pub fn is_ancestor(&self, ancestor: NodeId, descendant: NodeId) -> bool {
        let mut cur = Some(descendant);
        let mut guard = 0usize;
        while let Some(id) = cur {
            if id == ancestor {
                return true;
            }
            guard += 1;
            if guard > 512 {
                break;
            }
            cur = self.get(id).and_then(|n| n.parent);
        }
        false
    }

    /// Finds a direct volume child matching `root_path` under a virtual root (e.g. `This PC`).
    pub fn find_volume_child(&self, root_path: &std::path::Path) -> Option<NodeId> {
        let needle = normalize_cmp_path(&root_path.to_string_lossy());
        for &cid in &self.root_node().children {
            let child = self.get(cid)?;
            if child.removed || !child.flags.contains(NodeFlags::IS_VOLUME) {
                continue;
            }
            if normalize_cmp_path(child.name.as_ref()) == needle {
                return Some(cid);
            }
        }
        None
    }

    /// Locates a non-removed node in this tree matching `target_path` by walking down segments.
    pub fn find_node_by_path(&self, target_path: &std::path::Path) -> Option<NodeId> {
        let target_str = target_path.to_string_lossy();
        let target_norm = normalize_cmp_path(&target_str);
        if target_norm.is_empty() {
            return None;
        }

        let (mut cur_id, remainder) = if self.root_node().flags.contains(NodeFlags::IS_VIRTUAL) {
            let mut matched = None;
            for &cid in &self.root_node().children {
                let Some(child) = self.get(cid) else {
                    continue;
                };
                if child.removed {
                    continue;
                }
                let vol_norm = normalize_cmp_path(child.name.as_ref());
                if target_norm == vol_norm {
                    return Some(cid);
                }
                if let Some(rest) = target_norm.strip_prefix(&vol_norm)
                    && (rest.starts_with('/') || vol_norm.ends_with('/'))
                {
                    matched = Some((cid, rest.trim_start_matches('/')));
                    break;
                }
            }
            matched?
        } else {
            let root_str = self
                .root_base_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.root_node().name.to_string());
            let root_norm = normalize_cmp_path(&root_str);
            if target_norm == root_norm {
                return Some(self.root);
            }
            let rest = target_norm.strip_prefix(&root_norm)?;
            if !rest.starts_with('/') && !root_norm.ends_with('/') {
                return None;
            }
            (self.root, rest.trim_start_matches('/'))
        };

        for seg in remainder.split('/').filter(|s| !s.is_empty()) {
            let next = self.get(cur_id)?.children.iter().copied().find(|&cid| {
                let Some(c) = self.get(cid) else {
                    return false;
                };
                !c.removed && c.name.as_ref().eq_ignore_ascii_case(seg)
            })?;
            cur_id = next;
        }
        Some(cur_id)
    }

    /// Extracts an independent `DiskTree` rooted at `sub_root` (used for instant cached
    /// switching from `Entire PC` into an individual drive without re-reading disk).
    pub fn extract_subtree(
        &self,
        sub_root: NodeId,
        target_label: impl Into<String>,
        root_base_path: Option<PathBuf>,
    ) -> Option<DiskTree> {
        let root_src = self.get(sub_root)?;
        if root_src.removed {
            return None;
        }

        let est_nodes = root_src
            .total_items()
            .saturating_add(1)
            .min(self.nodes.len() as u64) as usize;
        let mut new_nodes: Vec<FileNode> = Vec::with_capacity(est_nodes.max(16));
        let mut stack: Vec<(NodeId, Option<NodeId>)> = Vec::with_capacity(64);
        stack.push((sub_root, None));

        while let Some((old_id, new_parent)) = stack.pop() {
            let src = &self.nodes[old_id as usize];
            if src.removed {
                continue;
            }
            let new_id = new_nodes.len() as NodeId;
            if let Some(pid) = new_parent {
                new_nodes[pid as usize].children.push(new_id);
            }
            let child_count = src.children.len();
            new_nodes.push(FileNode {
                name: src.name.clone(),
                parent: new_parent,
                children: Vec::with_capacity(child_count),
                logical_size: src.logical_size,
                allocated_size: src.allocated_size,
                file_count: src.file_count,
                dir_count: src.dir_count,
                denied_count: src.denied_count,
                modified_secs: src.modified_secs,
                flags: src.flags,
                ext_id: src.ext_id,
                removed: false,
            });
            // Push in reverse so children are visited and pushed into `new_nodes[new_id].children` in original sorted order.
            for &cid in src.children.iter().rev() {
                if !self.nodes[cid as usize].removed {
                    stack.push((cid, Some(new_id)));
                }
            }
        }

        Some(DiskTree {
            root: 0,
            root_base_path,
            nodes: new_nodes,
            extensions: self.extensions.clone(),
            scan_duration_ms: self.scan_duration_ms,
            target_label: target_label.into(),
            engine: self.engine,
        })
    }

    /// Marks `id` and all its descendants as removed and subtracts their totals up the parent chain.
    pub fn remove_subtree_soft(&mut self, id: NodeId) -> bool {
        if id == self.root {
            return false;
        }
        let Some(node) = self.nodes.get(id as usize) else {
            return false;
        };
        if node.removed || node.flags.intersects(NodeFlags::IS_VOLUME | NodeFlags::IS_VIRTUAL) {
            return false;
        }

        let delta_logical = node.logical_size;
        let delta_allocated = node.allocated_size;
        let (delta_files, delta_dirs, delta_denied) = if node.is_container() {
            (
                node.file_count,
                node.dir_count.saturating_add(1),
                node.denied_count,
            )
        } else {
            (
                1,
                0,
                u32::from(node.flags.contains(NodeFlags::ACCESS_DENIED)),
            )
        };
        let parent_id = node.parent;

        // Mark entire subtree removed iteratively.
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            let n = &mut self.nodes[cur as usize];
            if n.removed {
                continue;
            }
            n.removed = true;
            stack.extend(n.children.iter().copied());
        }

        if let Some(pid) = parent_id {
            self.nodes[pid as usize].children.retain(|&cid| cid != id);
        }

        let mut cursor = parent_id;
        while let Some(pid) = cursor {
            let p = &mut self.nodes[pid as usize];
            p.logical_size = p.logical_size.saturating_sub(delta_logical);
            p.allocated_size = p.allocated_size.saturating_sub(delta_allocated);
            p.file_count = p.file_count.saturating_sub(delta_files);
            p.dir_count = p.dir_count.saturating_sub(delta_dirs);
            p.denied_count = p.denied_count.saturating_sub(delta_denied);
            cursor = p.parent;
        }

        true
    }
}

/// Extension interner used while constructing a `DiskTree`.
pub struct ExtInterner {
    map: HashMap<Box<str>, u16>,
    list: Vec<Box<str>>,
}

impl Default for ExtInterner {
    fn default() -> Self {
        let mut map = HashMap::with_capacity(128);
        let empty: Box<str> = Box::from("");
        map.insert(empty.clone(), 0);
        Self {
            map,
            list: vec![empty],
        }
    }
}

impl ExtInterner {
    pub fn intern_from_filename(&mut self, name: &str) -> u16 {
        let Some(dot_idx) = name.rfind('.') else {
            return 0;
        };
        if dot_idx == 0 || dot_idx + 1 >= name.len() {
            return 0;
        }
        let raw_ext = &name[dot_idx + 1..];
        if raw_ext.len() > 16 || !raw_ext.is_ascii() {
            return 0;
        }
        let lower = raw_ext.to_ascii_lowercase();
        if let Some(&id) = self.map.get(lower.as_str()) {
            return id;
        }
        if self.list.len() >= u16::MAX as usize {
            return 0;
        }
        let id = self.list.len() as u16;
        let boxed: Box<str> = lower.into_boxed_str();
        self.map.insert(boxed.clone(), id);
        self.list.push(boxed);
        id
    }

    pub fn into_vec(self) -> Vec<Box<str>> {
        self.list
    }
}

fn compare_nodes(nodes: &[FileNode], a_id: NodeId, b_id: NodeId, sort: SortSpec) -> Ordering {
    let a = &nodes[a_id as usize];
    let b = &nodes[b_id as usize];

    let ord = match sort.column {
        SortColumn::Name => a
            .name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase()),
        SortColumn::Percent | SortColumn::Size => a.logical_size.cmp(&b.logical_size),
        SortColumn::Allocated => a.allocated_size.cmp(&b.allocated_size),
        SortColumn::Items => a.total_items().cmp(&b.total_items()),
        SortColumn::Modified => a.modified_secs.cmp(&b.modified_secs),
    };

    let directed = if sort.descending { ord.reverse() } else { ord };
    directed
        .then_with(|| b.logical_size.cmp(&a.logical_size))
        .then_with(|| a.name.cmp(&b.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_sample_tree() -> DiskTree {
        let mut interner = ExtInterner::default();
        let mut tree = DiskTree::new(
            "X:\\",
            NodeFlags::IS_DIR | NodeFlags::IS_VOLUME,
            Some(PathBuf::from("X:\\")),
            "X:\\",
        );

        let dir_a = tree.nodes.len() as NodeId;
        tree.nodes.push(FileNode {
            name: SharedString::from("Projects"),
            parent: Some(0),
            children: Vec::new(),
            logical_size: 0,
            allocated_size: 0,
            file_count: 0,
            dir_count: 0,
            denied_count: 0,
            modified_secs: 100,
            flags: NodeFlags::IS_DIR,
            ext_id: 0,
            removed: false,
        });
        tree.nodes[0].children.push(dir_a);

        let file_big = tree.nodes.len() as NodeId;
        let ext_pak = interner.intern_from_filename("assets.pak");
        tree.nodes.push(FileNode {
            name: SharedString::from("assets.pak"),
            parent: Some(dir_a),
            children: Vec::new(),
            logical_size: 5000,
            allocated_size: 8192,
            file_count: 0,
            dir_count: 0,
            denied_count: 0,
            modified_secs: 200,
            flags: NodeFlags::empty(),
            ext_id: ext_pak,
            removed: false,
        });
        tree.nodes[dir_a as usize].children.push(file_big);

        let file_small = tree.nodes.len() as NodeId;
        let ext_rs = interner.intern_from_filename("main.rs");
        tree.nodes.push(FileNode {
            name: SharedString::from("main.rs"),
            parent: Some(dir_a),
            children: Vec::new(),
            logical_size: 500,
            allocated_size: 4096,
            file_count: 0,
            dir_count: 0,
            denied_count: 0,
            modified_secs: 300,
            flags: NodeFlags::empty(),
            ext_id: ext_rs,
            removed: false,
        });
        tree.nodes[dir_a as usize].children.push(file_small);

        let denied_dir = tree.nodes.len() as NodeId;
        tree.nodes.push(FileNode {
            name: SharedString::from("System Volume Information"),
            parent: Some(0),
            children: Vec::new(),
            logical_size: 0,
            allocated_size: 0,
            file_count: 0,
            dir_count: 0,
            denied_count: 0,
            modified_secs: 50,
            flags: NodeFlags::IS_DIR | NodeFlags::ACCESS_DENIED | NodeFlags::IS_SYSTEM,
            ext_id: 0,
            removed: false,
        });
        tree.nodes[0].children.push(denied_dir);

        tree.extensions = interner.into_vec();
        tree.aggregate_and_sort(SortSpec::default());
        tree
    }

    #[test]
    fn aggregates_sizes_counts_paths_and_soft_removal() {
        let mut tree = build_sample_tree();
        assert_eq!(tree.root_node().logical_size, 5500);
        assert_eq!(tree.root_node().allocated_size, 12288);
        assert_eq!(tree.root_node().file_count, 2);
        assert_eq!(tree.root_node().dir_count, 2);
        assert_eq!(tree.root_node().denied_count, 1);

        // Largest child of root should be Projects (node 1)
        assert_eq!(tree.nodes[0].children[0], 1);
        assert_eq!(
            tree.full_path(2),
            Some(PathBuf::from("X:\\").join("Projects").join("assets.pak"))
        );

        let stats = tree.extension_stats(true);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].extension, ".pak");
        assert_eq!(stats[0].total_size, 5000);

        let top = tree.top_files("", None, false, true, 10);
        assert_eq!(top, vec![2, 3]);

        let denied = tree.top_files("", None, true, true, 10);
        assert_eq!(denied, vec![4]);

        // Soft-remove assets.pak and check ancestor totals update immediately
        assert!(tree.remove_subtree_soft(2));
        assert_eq!(tree.root_node().logical_size, 500);
        assert_eq!(tree.root_node().allocated_size, 4096);
        assert_eq!(tree.root_node().file_count, 1);
    }

    #[test]
    fn flattens_expanded_and_filtered_rows() {
        let tree = build_sample_tree();
        let mut expanded = HashSet::new();
        expanded.insert(0);

        let rows = tree.flatten_visible(&expanded, "", true);
        assert_eq!(rows.len(), 3); // root + Projects + System Volume Information
        assert_eq!(rows[0].node_id, 0);
        assert_eq!(rows[1].node_id, 1);
        assert_eq!(rows[1].depth, 1);

        let filtered = tree.flatten_visible(&HashSet::new(), "main.rs", true);
        let ids: Vec<NodeId> = filtered.iter().map(|r| r.node_id).collect();
        assert_eq!(ids, vec![0, 1, 3]);
    }

    #[test]
    fn keeps_virtual_root_drives_sorted_a_to_z() {
        let mut tree = DiskTree::new(
            "This PC",
            NodeFlags::IS_DIR | NodeFlags::IS_VIRTUAL,
            None,
            "This PC",
        );
        for (drive, size) in [("X:\\", 100), ("E:\\", 900), ("C:\\", 300), ("D:\\", 500)] {
            let vol_id = tree.nodes.len() as NodeId;
            tree.nodes.push(FileNode {
                name: SharedString::from(drive),
                parent: Some(0),
                children: Vec::new(),
                logical_size: 0,
                allocated_size: 0,
                file_count: 0,
                dir_count: 0,
                denied_count: 0,
                modified_secs: 0,
                flags: NodeFlags::IS_DIR | NodeFlags::IS_VOLUME,
                ext_id: 0,
                removed: false,
            });
            let file_id = tree.nodes.len() as NodeId;
            tree.nodes.push(FileNode {
                name: SharedString::from("data.bin"),
                parent: Some(vol_id),
                children: Vec::new(),
                logical_size: size,
                allocated_size: size,
                file_count: 0,
                dir_count: 0,
                denied_count: 0,
                modified_secs: 0,
                flags: NodeFlags::empty(),
                ext_id: 0,
                removed: false,
            });
            tree.nodes[vol_id as usize].children.push(file_id);
            tree.nodes[0].children.push(vol_id);
        }

        tree.aggregate_and_sort(SortSpec {
            column: SortColumn::Size,
            descending: true,
        });

        let drive_names: Vec<&str> = tree.nodes[0]
            .children
            .iter()
            .map(|&id| tree.nodes[id as usize].name.as_ref())
            .collect();
        assert_eq!(drive_names, vec!["C:\\", "D:\\", "E:\\", "X:\\"]);

        // Test volume lookup, subtree extraction, path lookup, and ancestry.
        let e_id = tree.find_volume_child(std::path::Path::new("E:\\")).unwrap();
        let e_tree = tree
            .extract_subtree(e_id, "Local Disk (E:)", Some(PathBuf::from("E:\\")))
            .unwrap();
        assert_eq!(e_tree.root_node().name.as_ref(), "E:\\");
        assert_eq!(e_tree.root_node().logical_size, 900);
        assert_eq!(e_tree.root_node().file_count, 1);

        let e_file_in_all = tree
            .find_node_by_path(std::path::Path::new("E:\\data.bin"))
            .unwrap();
        assert!(tree.is_ancestor(e_id, e_file_in_all));
        assert!(!tree.is_ancestor(e_file_in_all, e_id));

        let e_file_in_sub = e_tree
            .find_node_by_path(std::path::Path::new("E:\\data.bin"))
            .unwrap();
        assert_eq!(e_tree.get(e_file_in_sub).unwrap().logical_size, 900);
    }
}

fn normalize_cmp_path(raw: &str) -> String {
    let mut s = raw.replace('\\', "/").to_ascii_lowercase();
    while s.len() > 1 && s.ends_with('/') && !s.ends_with(":/") {
        s.pop();
    }
    s
}
