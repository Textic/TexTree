//! Squarified Treemap layout algorithm (Bruls, Huizing, van Wijk) & flat color mapping
//! tailored for Obsidian Dark GPU canvas rendering.

use crate::models::{DiskTree, NodeFlags, NodeId};

const MAX_TREEMAP_DEPTH: u8 = 12;
const MAX_LEAF_RECTS: usize = 8_000;
const MIN_SUBDIVIDE_SIDE: f32 = 10.0;
const MIN_LEAF_SIDE: f32 = 3.0;

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

#[derive(Debug, Clone, Default)]
pub struct TreemapLayout {
    pub root_id: NodeId,
    pub width: f32,
    pub height: f32,
    /// Renderable filled tiles (files or small collapsed folders).
    pub leaves: Vec<TreemapRect>,
    /// Subtle structural outlines around depth 1-2 folder clusters (also used to highlight selected folders).
    pub frames: Vec<TreemapRect>,
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
            width,
            height,
            leaves: Vec::with_capacity(512),
            frames: Vec::with_capacity(128),
        };
        if width < 4.0 || height < 4.0 {
            return layout;
        }
        let Some(root_node) = tree.get(root_id) else {
            return layout;
        };
        if root_node.logical_size == 0 {
            return layout;
        }

        if root_node.is_container() {
            layout.subdivide(tree, root_id, 0.0, 0.0, width, height, 0, show_hidden);
        } else {
            let color_hex = color_for_node(tree, root_id);
            layout.leaves.push(TreemapRect {
                node_id: root_id,
                x: 0.0,
                y: 0.0,
                w: width,
                h: height,
                color_hex,
                is_container: false,
                depth: 0,
            });
        }
        layout
    }

    pub fn hit_test(&self, px: f32, py: f32) -> Option<TreemapRect> {
        self.leaves.iter().rev().copied().find(|r| r.contains(px, py))
    }

    pub fn rect_for_node(&self, node_id: NodeId) -> Option<TreemapRect> {
        if let Some(r) = self.leaves.iter().find(|r| r.node_id == node_id) {
            return Some(*r);
        }
        if let Some(r) = self.frames.iter().find(|r| r.node_id == node_id) {
            return Some(*r);
        }
        if node_id == self.root_id && self.width > 0.0 && self.height > 0.0 {
            return Some(TreemapRect {
                node_id,
                x: 0.0,
                y: 0.0,
                w: self.width,
                h: self.height,
                color_hex: 0x34d399,
                is_container: true,
                depth: 0,
            });
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

        // Squarify requires items sorted descending by size.
        items.sort_unstable_by(|a, b| b.1.cmp(&a.1));

        let total_size: u64 = items.iter().map(|(_, s)| *s).sum();
        if total_size == 0 {
            return;
        }

        let total_area = (w * h) as f64;
        // Skip microscopic trailing items (< ~1.5 px^2) to keep layout blazing fast.
        let min_size_threshold = ((1.5 / total_area.max(1.0)) * total_size as f64) as u64;
        let mut positioned: Vec<(NodeId, f32, f32, f32, f32)> =
            Vec::with_capacity(items.len().min(256));
        squarify_into(
            &items,
            total_size,
            min_size_threshold,
            x,
            y,
            w,
            h,
            &mut positioned,
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
                // Record subtle frame for depth 1-2 folder boundaries & selection highlighting.
                if next_depth <= 2 {
                    self.frames.push(TreemapRect {
                        node_id: cid,
                        x: rx,
                        y: ry,
                        w: rw,
                        h: rh,
                        color_hex: color_for_node(tree, cid),
                        is_container: true,
                        depth: next_depth,
                    });
                }
                // Inset slightly only at drive/top-folder boundaries so deep nesting doesn't waste pixels.
                let pad = if next_depth == 1 {
                    1.5
                } else if next_depth == 2 {
                    0.75
                } else {
                    0.0
                };
                let iw = (rw - pad * 2.0).max(0.0);
                let ih = (rh - pad * 2.0).max(0.0);
                let before = self.leaves.len();
                self.subdivide(tree, cid, rx + pad, ry + pad, iw, ih, next_depth, show_hidden);
                if self.leaves.len() == before {
                    // Fallback tile if all children were below minimum pixel size.
                    self.leaves.push(TreemapRect {
                        node_id: cid,
                        x: rx,
                        y: ry,
                        w: rw,
                        h: rh,
                        color_hex: color_for_node(tree, cid),
                        is_container: true,
                        depth: next_depth,
                    });
                }
            } else if rw >= MIN_LEAF_SIDE && rh >= MIN_LEAF_SIDE {
                self.leaves.push(TreemapRect {
                    node_id: cid,
                    x: rx,
                    y: ry,
                    w: rw,
                    h: rh,
                    color_hex: color_for_node(tree, cid),
                    is_container,
                    depth: next_depth,
                });
            }
        }
    }
}

/// Iterative Squarified Treemap partitioner (Bruls et al., 2000).
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
) {
    let mut remaining_size = total_size as f64;
    let mut idx = 0usize;

    while idx < items.len() && rw >= MIN_LEAF_SIDE && rh >= MIN_LEAF_SIDE && remaining_size > 0.0 {
        if items[idx].1 < min_size_threshold && idx > 0 {
            break;
        }

        let rem_area = (rw * rh) as f64;
        let short_side = rw.min(rh) as f64;
        let scale = rem_area / remaining_size;

        // Greedily add items to the current row while worst aspect ratio improves.
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

        // Lay out items[row_start..idx] along the shorter side of the remaining rectangle.
        let lay_vertical = rw >= rh; // Stack items vertically in a left strip of width `thickness`
        if lay_vertical {
            let strip_w = ((row_area / (rh as f64)).min(rw as f64)) as f32;
            let mut cursor_y = ry;
            for &(nid, sz) in &items[row_start..idx] {
                let frac = (sz as f64 / row_size_sum.max(1.0)) as f32;
                let item_h = (rh * frac).min((ry + rh) - cursor_y).max(0.0);
                if strip_w >= MIN_LEAF_SIDE && item_h >= MIN_LEAF_SIDE {
                    out.push((nid, rx, cursor_y, strip_w, item_h));
                }
                cursor_y += item_h;
            }
            rx += strip_w;
            rw = (rw - strip_w).max(0.0);
        } else {
            let strip_h = ((row_area / (rw as f64)).min(rh as f64)) as f32;
            let mut cursor_x = rx;
            for &(nid, sz) in &items[row_start..idx] {
                let frac = (sz as f64 / row_size_sum.max(1.0)) as f32;
                let item_w = (rw * frac).min((rx + rw) - cursor_x).max(0.0);
                if item_w >= MIN_LEAF_SIDE && strip_h >= MIN_LEAF_SIDE {
                    out.push((nid, cursor_x, ry, item_w, strip_h));
                }
                cursor_x += item_w;
            }
            ry += strip_h;
            rh = (rh - strip_h).max(0.0);
        }

        remaining_size -= row_size_sum;
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
        return 0x27272a;
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

        let mut total_area = 0.0f32;
        for r in &layout.leaves {
            assert!(r.x >= -0.01 && r.y >= -0.01);
            assert!(r.x + r.w <= 800.05 && r.y + r.h <= 400.05);
            assert!(r.w > 0.0 && r.h > 0.0);
            total_area += r.w * r.h;

            let hit = layout.hit_test(r.x + r.w * 0.5, r.y + r.h * 0.5).unwrap();
            assert_eq!(hit.node_id, r.node_id);
        }
        assert!(
            (total_area - 800.0 * 400.0).abs() < 200.0,
            "expected layout area close to viewport, got {total_area}"
        );
    }
}
