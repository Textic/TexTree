//! Squarified Treemap layout algorithm (Bruls, Huizing, van Wijk) & flat color mapping
//! tailored for Obsidian Dark GPU canvas rendering with crisp folder framing,
//! inline depth-1 folder headers, and zero dead black voids.

use crate::models::{DiskTree, NodeFlags, NodeId};

const MAX_TREEMAP_DEPTH: u8 = 12;
const MAX_LEAF_RECTS: usize = 8_000;
const MIN_SUBDIVIDE_SIDE: f32 = 10.0;
const MIN_LEAF_SIDE: f32 = 3.0;

pub const FOLDER_HEADER_HEIGHT: f32 = 18.0;
const MIN_HEADER_WIDTH: f32 = 76.0;
const MIN_HEADER_CONTAINER_HEIGHT: f32 = 42.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreemapRect {
    pub node_id: NodeId,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color_hex: u32,
    pub is_container: bool,
    pub depth: u8,
}

impl TreemapRect {
    #[inline]
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreemapHeader {
    pub node_id: NodeId,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl TreemapHeader {
    #[inline]
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

#[derive(Debug, Clone, Default)]
pub struct TreemapLayout {
    pub root_id: NodeId,
    /// Renderable filled tiles (files, collapsed folders, and grouped small-file remainders).
    pub leaves: Vec<TreemapRect>,
    /// Structural container frames for depth 1-2 folder clusters (rendered with elevated plate + border).
    pub frames: Vec<TreemapRect>,
    /// Inline title headers drawn at the top of large depth-1 folder boxes.
    pub headers: Vec<TreemapHeader>,
}

impl TreemapLayout {
    pub fn compute(
        tree: &DiskTree,
        root_id: NodeId,
        width: f32,
        height: f32,
        show_hidden: bool,
    ) -> Self {
        let mut layout = Self {
            root_id,
            leaves: Vec::with_capacity(512),
            frames: Vec::with_capacity(128),
            headers: Vec::with_capacity(32),
        };
        if width < 6.0 || height < 6.0 {
            return layout;
        }
        let Some(root_node) = tree.get(root_id) else {
            return layout;
        };
        if root_node.logical_size == 0 {
            return layout;
        }

        if root_node.is_container() {
            layout.subdivide(
                tree,
                root_id,
                2.0,
                2.0,
                (width - 4.0).max(0.0),
                (height - 4.0).max(0.0),
                0,
                show_hidden,
            );
        } else {
            let color_hex = color_for_node(tree, root_id);
            layout.leaves.push(TreemapRect {
                node_id: root_id,
                x: 2.0,
                y: 2.0,
                w: (width - 4.0).max(1.0),
                h: (height - 4.0).max(1.0),
                color_hex,
                is_container: false,
                depth: 0,
            });
        }
        layout
    }

    pub fn hit_test(&self, px: f32, py: f32) -> Option<TreemapRect> {
        // 1. Check inline folder header strips first so clicking/hovering a folder title hits that folder.
        if let Some(hdr) = self.headers.iter().rev().find(|h| h.contains(px, py)) {
            if let Some(frame) = self.frames.iter().find(|f| f.node_id == hdr.node_id) {
                return Some(*frame);
            }
            return Some(TreemapRect {
                node_id: hdr.node_id,
                x: hdr.x,
                y: hdr.y,
                w: hdr.w,
                h: hdr.h,
                color_hex: 0x27272a,
                is_container: true,
                depth: 1,
            });
        }
        // 2. Check leaf tiles (files & remainder tiles).
        if let Some(leaf) = self.leaves.iter().rev().copied().find(|r| r.contains(px, py)) {
            return Some(leaf);
        }
        // 3. Fallback to container frames (e.g. clicking inside a folder margin).
        self.frames.iter().rev().copied().find(|r| r.contains(px, py))
    }

    /// Resolves the highlight bounding rectangle for any `node_id` hovered in the Tree View,
    /// Top Files list, or Treemap itself:
    /// 1. Direct frame (depth 1–2 folder/drive) or leaf tile match.
    /// 2. Tight union bounding box of all rendered descendant leaves for deeper folders (depth 3–12).
    /// 3. Immediate parent's small-file remainder tile if a tiny file was grouped into it.
    pub fn highlight_rect_for_node(&self, tree: &DiskTree, node_id: NodeId) -> Option<TreemapRect> {
        if node_id == self.root_id {
            return None;
        }
        if let Some(r) = self.frames.iter().find(|r| r.node_id == node_id) {
            return Some(*r);
        }
        if let Some(r) = self.leaves.iter().find(|r| r.node_id == node_id) {
            return Some(*r);
        }

        let node = tree.get(node_id)?;
        if node.is_container() {
            let mut min_x = f32::INFINITY;
            let mut min_y = f32::INFINITY;
            let mut max_x = f32::NEG_INFINITY;
            let mut max_y = f32::NEG_INFINITY;
            let mut found = false;

            for leaf in &self.leaves {
                if tree.is_ancestor(node_id, leaf.node_id) {
                    min_x = min_x.min(leaf.x);
                    min_y = min_y.min(leaf.y);
                    max_x = max_x.max(leaf.x + leaf.w);
                    max_y = max_y.max(leaf.y + leaf.h);
                    found = true;
                }
            }
            if found {
                return Some(TreemapRect {
                    node_id,
                    x: min_x,
                    y: min_y,
                    w: (max_x - min_x).max(2.0),
                    h: (max_y - min_y).max(2.0),
                    color_hex: 0,
                    is_container: true,
                    depth: 3,
                });
            }
        } else if let Some(pid) = node.parent {
            // Tiny file folded into its parent's remainder bucket.
            if let Some(rem) = self
                .leaves
                .iter()
                .find(|r| r.is_container && r.node_id == pid)
            {
                return Some(*rem);
            }
        }

        None
    }

    #[allow(clippy::too_many_arguments)]
    fn subdivide(
        &mut self,
        tree: &DiskTree,
        container_id: NodeId,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        depth: u8,
        show_hidden: bool,
    ) {
        if self.leaves.len() >= MAX_LEAF_RECTS || w < MIN_LEAF_SIDE || h < MIN_LEAF_SIDE {
            return;
        }
        let Some(node) = tree.get(container_id) else {
            return;
        };

        let mut items: Vec<(NodeId, u64)> = node
            .children
            .iter()
            .copied()
            .filter_map(|cid| {
                let c = tree.get(cid)?;
                if c.removed || c.logical_size == 0 {
                    return None;
                }
                if !show_hidden && c.flags.contains(NodeFlags::IS_HIDDEN) {
                    return None;
                }
                Some((cid, c.logical_size))
            })
            .collect();

        if items.is_empty() {
            return;
        }

        items.sort_unstable_by(|a, b| b.1.cmp(&a.1));

        let total_size: u64 = items.iter().map(|(_, s)| *s).sum();
        if total_size == 0 {
            return;
        }

        let total_area = (w * h) as f64;
        // Any file whose area is below MIN_LEAF_SIDE^2 (~12 px^2) cannot render cleanly;
        // aggregate all such trailing small files into a single subtle remainder tile.
        let min_tile_area = (MIN_LEAF_SIDE * MIN_LEAF_SIDE).max(12.0) as f64;
        let min_size_threshold = ((min_tile_area / total_area.max(1.0)) * total_size as f64) as u64;
        let mut positioned: Vec<(NodeId, f32, f32, f32, f32)> =
            Vec::with_capacity(items.len().min(256));
        let mut remainders: Vec<(f32, f32, f32, f32)> = Vec::new();
        squarify_into(
            &items,
            total_size,
            min_size_threshold,
            x,
            y,
            w,
            h,
            &mut positioned,
            &mut remainders,
        );

        let next_depth = depth + 1;
        for (cid, rx, ry, rw, rh) in positioned {
            if self.leaves.len() >= MAX_LEAF_RECTS {
                break;
            }
            let Some(child) = tree.get(cid) else {
                continue;
            };
            let is_container = child.is_container();

            let can_recurse = is_container
                && next_depth < MAX_TREEMAP_DEPTH
                && rw >= MIN_SUBDIVIDE_SIDE
                && rh >= MIN_SUBDIVIDE_SIDE
                && !child.children.is_empty();

            if can_recurse {
                if next_depth == 1 {
                    // Depth-1 folder: distinct 4px dark gutter (2px each side) + crisp card frame + optional header
                    let outer_pad = if rw > 18.0 && rh > 18.0 { 2.0 } else { 1.0 };
                    let fx = rx + outer_pad;
                    let fy = ry + outer_pad;
                    let fw = (rw - outer_pad * 2.0).max(0.0);
                    let fh = (rh - outer_pad * 2.0).max(0.0);

                    if fw >= MIN_LEAF_SIDE && fh >= MIN_LEAF_SIDE {
                        self.frames.push(TreemapRect {
                            node_id: cid,
                            x: fx,
                            y: fy,
                            w: fw,
                            h: fh,
                            color_hex: 0x121216,
                            is_container: true,
                            depth: 1,
                        });

                        let (ix, iy, iw, ih) =
                            if fw >= MIN_HEADER_WIDTH && fh >= MIN_HEADER_CONTAINER_HEIGHT {
                                self.headers.push(TreemapHeader {
                                    node_id: cid,
                                    x: fx + 1.0,
                                    y: fy + 1.0,
                                    w: (fw - 2.0).max(0.0),
                                    h: FOLDER_HEADER_HEIGHT,
                                });
                                (
                                    fx + 2.0,
                                    fy + FOLDER_HEADER_HEIGHT + 2.0,
                                    (fw - 4.0).max(0.0),
                                    (fh - FOLDER_HEADER_HEIGHT - 4.0).max(0.0),
                                )
                            } else {
                                (
                                    fx + 2.0,
                                    fy + 2.0,
                                    (fw - 4.0).max(0.0),
                                    (fh - 4.0).max(0.0),
                                )
                            };

                        let before = self.leaves.len();
                        self.subdivide(tree, cid, ix, iy, iw, ih, next_depth, show_hidden);
                        if self.leaves.len() == before && iw >= MIN_LEAF_SIDE && ih >= MIN_LEAF_SIDE
                        {
                            self.leaves.push(TreemapRect {
                                node_id: cid,
                                x: ix,
                                y: iy,
                                w: iw,
                                h: ih,
                                color_hex: 0x222229,
                                is_container: true,
                                depth: next_depth,
                            });
                        }
                    }
                } else if next_depth == 2 {
                    // Depth-2 subfolder: crisp 2px gutter + subtle inner frame
                    let outer_pad = if rw > 14.0 && rh > 14.0 { 1.0 } else { 0.5 };
                    let fx = rx + outer_pad;
                    let fy = ry + outer_pad;
                    let fw = (rw - outer_pad * 2.0).max(0.0);
                    let fh = (rh - outer_pad * 2.0).max(0.0);

                    if fw >= MIN_LEAF_SIDE && fh >= MIN_LEAF_SIDE {
                        self.frames.push(TreemapRect {
                            node_id: cid,
                            x: fx,
                            y: fy,
                            w: fw,
                            h: fh,
                            color_hex: 0x16161b,
                            is_container: true,
                            depth: 2,
                        });
                        let ix = fx + 1.5;
                        let iy = fy + 1.5;
                        let iw = (fw - 3.0).max(0.0);
                        let ih = (fh - 3.0).max(0.0);
                        let before = self.leaves.len();
                        self.subdivide(tree, cid, ix, iy, iw, ih, next_depth, show_hidden);
                        if self.leaves.len() == before && iw >= MIN_LEAF_SIDE && ih >= MIN_LEAF_SIDE
                        {
                            self.leaves.push(TreemapRect {
                                node_id: cid,
                                x: ix,
                                y: iy,
                                w: iw,
                                h: ih,
                                color_hex: 0x24242b,
                                is_container: true,
                                depth: next_depth,
                            });
                        }
                    }
                } else {
                    let before = self.leaves.len();
                    self.subdivide(tree, cid, rx, ry, rw, rh, next_depth, show_hidden);
                    if self.leaves.len() == before && rw >= MIN_LEAF_SIDE && rh >= MIN_LEAF_SIDE {
                        self.leaves.push(TreemapRect {
                            node_id: cid,
                            x: rx,
                            y: ry,
                            w: rw,
                            h: rh,
                            color_hex: 0x24242b,
                            is_container: true,
                            depth: next_depth,
                        });
                    }
                }
            } else if rw >= MIN_LEAF_SIDE && rh >= MIN_LEAF_SIDE {
                let (lx, ly, lw, lh) = if next_depth == 1 && rw > 12.0 && rh > 12.0 {
                    (rx + 2.0, ry + 2.0, rw - 4.0, rh - 4.0)
                } else {
                    (rx, ry, rw, rh)
                };
                self.leaves.push(TreemapRect {
                    node_id: cid,
                    x: lx,
                    y: ly,
                    w: lw,
                    h: lh,
                    color_hex: color_for_node(tree, cid),
                    is_container,
                    depth: next_depth,
                });
            }
        }

        // Fill remaining space occupied by thousands of tiny omitted files with a subtle
        // container tile so there are never unexplained black holes inside folders.
        for (rem_x, rem_y, rem_w, rem_h) in remainders {
            if self.leaves.len() >= MAX_LEAF_RECTS {
                break;
            }
            if rem_w >= MIN_LEAF_SIDE && rem_h >= MIN_LEAF_SIDE {
                self.leaves.push(TreemapRect {
                    node_id: container_id,
                    x: rem_x,
                    y: rem_y,
                    w: rem_w,
                    h: rem_h,
                    color_hex: 0x222229,
                    is_container: true,
                    depth: next_depth,
                });
            }
        }
    }
}

/// Iterative Squarified Treemap partitioner (Bruls et al., 2000).
/// Pushes any unfilled areas belonging to omitted tiny files into `remainders`.
#[allow(clippy::too_many_arguments)]
fn squarify_into(
    items: &[(NodeId, u64)],
    total_size: u64,
    min_size_threshold: u64,
    mut rx: f32,
    mut ry: f32,
    mut rw: f32,
    mut rh: f32,
    out: &mut Vec<(NodeId, f32, f32, f32, f32)>,
    remainders: &mut Vec<(f32, f32, f32, f32)>,
) {
    let mut remaining_size = total_size as f64;
    let mut idx = 0usize;

    while idx < items.len() && rw >= MIN_LEAF_SIDE && rh >= MIN_LEAF_SIDE && remaining_size > 0.0 {
        if items[idx].1 < min_size_threshold && idx > 0 {
            remainders.push((rx, ry, rw, rh));
            return;
        }

        let rem_area = (rw * rh) as f64;
        let short_side = rw.min(rh) as f64;
        let scale = rem_area / remaining_size;

        let row_start = idx;
        let first_area = (items[idx].1 as f64) * scale;
        let mut row_area = first_area;
        let mut row_min = first_area;
        let mut row_max = first_area;
        let mut row_size_sum = items[idx].1 as f64;
        idx += 1;

        while idx < items.len() {
            let next_sz = items[idx].1;
            if next_sz < min_size_threshold {
                break;
            }
            let next_area = (next_sz as f64) * scale;
            let cur_worst = worst_aspect(row_min, row_max, row_area, short_side);
            let new_area = row_area + next_area;
            let new_min = row_min.min(next_area);
            let new_max = row_max.max(next_area);
            let new_worst = worst_aspect(new_min, new_max, new_area, short_side);

            if new_worst <= cur_worst {
                row_area = new_area;
                row_min = new_min;
                row_max = new_max;
                row_size_sum += next_sz as f64;
                idx += 1;
            } else {
                break;
            }
        }

        let lay_vertical = rw >= rh;
        if lay_vertical {
            let strip_w = ((row_area / (rh as f64)).min(rw as f64)) as f32;
            if strip_w < MIN_LEAF_SIDE {
                remainders.push((rx, ry, rw, rh));
                return;
            }
            let mut cursor_y = ry;
            let mut skipped_tail_y: Option<f32> = None;
            for &(nid, sz) in &items[row_start..idx] {
                let frac = (sz as f64 / row_size_sum.max(1.0)) as f32;
                let item_h = (rh * frac).min((ry + rh) - cursor_y).max(0.0);
                if item_h >= MIN_LEAF_SIDE && skipped_tail_y.is_none() {
                    out.push((nid, rx, cursor_y, strip_w, item_h));
                    cursor_y += item_h;
                } else {
                    skipped_tail_y.get_or_insert(cursor_y);
                    break;
                }
            }
            if let Some(sy) = skipped_tail_y {
                let rem_h = (ry + rh - sy).max(0.0);
                if rem_h >= MIN_LEAF_SIDE {
                    remainders.push((rx, sy, strip_w, rem_h));
                }
            }
            rx += strip_w;
            rw = (rw - strip_w).max(0.0);
        } else {
            let strip_h = ((row_area / (rw as f64)).min(rh as f64)) as f32;
            if strip_h < MIN_LEAF_SIDE {
                remainders.push((rx, ry, rw, rh));
                return;
            }
            let mut cursor_x = rx;
            let mut skipped_tail_x: Option<f32> = None;
            for &(nid, sz) in &items[row_start..idx] {
                let frac = (sz as f64 / row_size_sum.max(1.0)) as f32;
                let item_w = (rw * frac).min((rx + rw) - cursor_x).max(0.0);
                if item_w >= MIN_LEAF_SIDE && skipped_tail_x.is_none() {
                    out.push((nid, cursor_x, ry, item_w, strip_h));
                    cursor_x += item_w;
                } else {
                    skipped_tail_x.get_or_insert(cursor_x);
                    break;
                }
            }
            if let Some(sx) = skipped_tail_x {
                let rem_w = (rx + rw - sx).max(0.0);
                if rem_w >= MIN_LEAF_SIDE {
                    remainders.push((sx, ry, rem_w, strip_h));
                }
            }
            ry += strip_h;
            rh = (rh - strip_h).max(0.0);
        }

        remaining_size -= row_size_sum;
    }

    if idx < items.len() && rw >= MIN_LEAF_SIDE && rh >= MIN_LEAF_SIDE {
        remainders.push((rx, ry, rw, rh));
    }
}

#[inline]
fn worst_aspect(min_a: f64, max_a: f64, sum_a: f64, side: f64) -> f64 {
    if min_a <= 0.0 || sum_a <= 0.0 || side <= 0.0 {
        return f64::INFINITY;
    }
    let s2 = side * side;
    let sum2 = sum_a * sum_a;
    let r1 = (s2 * max_a) / sum2;
    let r2 = sum2 / (s2 * min_a);
    r1.max(r2)
}

/// Curated flat color palette for Obsidian Dark Treemap tiles grouped by file category.
pub fn color_for_node(tree: &DiskTree, node_id: NodeId) -> u32 {
    let Some(node) = tree.get(node_id) else {
        return 0x3f3f46;
    };
    if node.flags.contains(NodeFlags::ACCESS_DENIED) {
        return 0x991b1b;
    }
    if node.is_container() {
        return 0x24242b;
    }
    color_for_extension(tree.extension_of(node_id))
}

pub fn color_for_extension(ext: &str) -> u32 {
    match ext {
        // Video / Cutscenes / Media — Deep Indigo
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v" | "m2ts" | "bik" | "bk2" => {
            0x4f46e5
        }
        // Audio / Game Sound Banks — Rose / Pink
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" | "opus" | "wma" | "bnk" | "wem" => {
            0xdb2777
        }
        // Images / Textures / 3D Models — Teal
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "psd" | "tga" | "dds" | "hdr" | "exr"
        | "svg" | "ico" | "blend" | "fbx" | "glb" | "obj" | "uasset" | "umap" => 0x0d9488,
        // Archives / Game Packages / Installers — Amber
        "zip" | "7z" | "rar" | "tar" | "gz" | "xz" | "zst" | "bz2" | "pak" | "vpk" | "ucas"
        | "utoc" | "rpf" | "bsa" | "ba2" | "cas" | "forge" | "cab" | "msi" | "jar" | "whl"
        | "nupkg" | "apk" | "deb" | "dmg" | "assets" | "bundle" | "unity3d" => 0xd97706,
        // Executables / Libraries / Build Artifacts — Sky Blue
        "exe" | "dll" | "sys" | "so" | "dylib" | "pdb" | "lib" | "rlib" | "a" | "o" | "bin"
        | "dat" | "wasm" | "class" | "pyc" => 0x0284c7,
        // Source Code / Web / Config — Emerald
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "c" | "cpp" | "cc" | "h" | "hpp" | "cs"
        | "go" | "java" | "kt" | "lua" | "luau" | "sh" | "ps1" | "json" | "toml" | "yaml"
        | "yml" | "xml" | "html" | "css" | "md" | "txt" | "sql" | "log" => 0x059669,
        // Documents / Databases — Violet
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "db" | "sqlite" | "sqlite3"
        | "parquet" | "csv" | "epub" => 0x7c3aed,
        // Disk Images / System / Memory Dumps — Coral Red
        "iso" | "img" | "vhdx" | "vhd" | "vmdk" | "vdi" | "qcow2" | "wim" | "esd" | "dmp" => {
            0xdc2626
        }
        // Fallback deterministic muted flat tone by extension hash
        other => {
            const FALLBACKS: [u32; 8] = [
                0x334155, 0x3b4252, 0x3f4756, 0x1e3a5f, 0x312e81, 0x134e4a, 0x3f2e4f, 0x4a342e,
            ];
            let hash = other
                .bytes()
                .fold(2166136261u32, |acc, b| acc.wrapping_mul(16777619) ^ u32::from(b));
            FALLBACKS[(hash as usize) % FALLBACKS.len()]
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui_kit::SharedString;

    use super::*;
    use crate::models::tree::ExtInterner;
    use crate::models::{FileNode, SortSpec};

    #[test]
    fn computes_bounded_squarified_layout_and_hit_tests() {
        let mut interner = ExtInterner::default();
        let mut tree = DiskTree::new(
            "X:\\",
            NodeFlags::IS_DIR | NodeFlags::IS_VOLUME,
            Some(PathBuf::from("X:\\")),
            "X:\\",
        );

        let files = [
            ("movie.mp4", 6000u64),
            ("archive.pak", 2500u64),
            ("engine.dll", 1000u64),
            ("main.rs", 500u64),
        ];
        for (name, size) in files {
            let id = tree.nodes.len() as NodeId;
            let ext_id = interner.intern_from_filename(name);
            tree.nodes.push(FileNode {
                name: SharedString::from(name),
                parent: Some(0),
                children: Vec::new(),
                logical_size: size,
                allocated_size: size,
                file_count: 0,
                dir_count: 0,
                denied_count: 0,
                modified_secs: 0,
                flags: NodeFlags::empty(),
                ext_id,
                removed: false,
            });
            tree.nodes[0].children.push(id);
        }
        tree.extensions = interner.into_vec();
        tree.aggregate_and_sort(SortSpec::default());

        let layout = TreemapLayout::compute(&tree, 0, 800.0, 400.0, true);
        assert_eq!(layout.leaves.len(), 4);

        for r in &layout.leaves {
            assert!(r.x >= 0.0 && r.y >= 0.0);
            assert!(r.x + r.w <= 800.05 && r.y + r.h <= 400.05);
            assert!(r.w > 0.0 && r.h > 0.0);

            let hit = layout.hit_test(r.x + r.w * 0.5, r.y + r.h * 0.5).unwrap();
            assert_eq!(hit.node_id, r.node_id);
        }
    }

    #[test]
    fn generates_depth1_folder_headers_and_fills_small_file_remainders() {
        let mut interner = ExtInterner::default();
        let mut tree = DiskTree::new(
            "X:\\",
            NodeFlags::IS_DIR | NodeFlags::IS_VOLUME,
            Some(PathBuf::from("X:\\")),
            "X:\\",
        );

        let folder_id = tree.nodes.len() as NodeId;
        tree.nodes.push(FileNode {
            name: SharedString::from("TexTree"),
            parent: Some(0),
            children: Vec::new(),
            logical_size: 0,
            allocated_size: 0,
            file_count: 0,
            dir_count: 0,
            denied_count: 0,
            modified_secs: 0,
            flags: NodeFlags::IS_DIR,
            ext_id: 0,
            removed: false,
        });
        tree.nodes[0].children.push(folder_id);

        // One big binary (1 MB) + 5000 tiny 20-byte files (100 KB total -> < 12 px^2 each)
        let ext_exe = interner.intern_from_filename("app.exe");
        let big_id = tree.nodes.len() as NodeId;
        tree.nodes.push(FileNode {
            name: SharedString::from("app.exe"),
            parent: Some(folder_id),
            children: Vec::new(),
            logical_size: 1_000_000,
            allocated_size: 1_000_000,
            file_count: 0,
            dir_count: 0,
            denied_count: 0,
            modified_secs: 0,
            flags: NodeFlags::empty(),
            ext_id: ext_exe,
            removed: false,
        });
        tree.nodes[folder_id as usize].children.push(big_id);

        for i in 0..5000 {
            let tid = tree.nodes.len() as NodeId;
            tree.nodes.push(FileNode {
                name: SharedString::from(format!("f{i}.tmp")),
                parent: Some(folder_id),
                children: Vec::new(),
                logical_size: 20,
                allocated_size: 20,
                file_count: 0,
                dir_count: 0,
                denied_count: 0,
                modified_secs: 0,
                flags: NodeFlags::empty(),
                ext_id: 0,
                removed: false,
            });
            tree.nodes[folder_id as usize].children.push(tid);
        }

        tree.extensions = interner.into_vec();
        tree.aggregate_and_sort(SortSpec::default());

        let layout = TreemapLayout::compute(&tree, 0, 800.0, 300.0, true);
        assert_eq!(layout.headers.len(), 1);
        assert_eq!(layout.headers[0].node_id, folder_id);
        // Should have big_id tile PLUS remainder tile for the 2000 tiny files
        assert!(layout.leaves.iter().any(|l| l.node_id == big_id));
        assert!(layout.leaves.iter().any(|l| l.node_id == folder_id && l.is_container));

        // Direct frame, direct leaf, and tiny-file remainder highlight resolution
        let folder_rect = layout.highlight_rect_for_node(&tree, folder_id).unwrap();
        assert!(folder_rect.w > 100.0 && folder_rect.h > 100.0);
        let big_rect = layout.highlight_rect_for_node(&tree, big_id).unwrap();
        assert_eq!(big_rect.node_id, big_id);
        let tiny_file_id = big_id + 1;
        let rem_rect = layout.highlight_rect_for_node(&tree, tiny_file_id).unwrap();
        assert!(rem_rect.is_container && rem_rect.node_id == folder_id);
        assert!(layout.highlight_rect_for_node(&tree, 0).is_none());
    }
}
