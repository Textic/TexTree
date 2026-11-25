use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::{
    BorderStyle, Bounds, ContentMask, Context, FontWeight, MouseButton, MouseDownEvent,
    MouseMoveEvent, PaintQuad, Pixels, Point, ShapedLine, SharedString, Size as GpuiSize,
    TextAlign, TextRun, canvas,
    component::{
        Icon, IconName, Sizable as _, Size, StyledExt as _,
        button::{Button, ButtonVariants as _},
        h_flex, v_flex,
    },
    div, fill, hsla, outline, point, prelude::*, px,
};

use crate::models::{DiskTree, NodeId, SizeUnit, TreemapLayout};
use crate::theme::{
    BG_BASE, BG_ELEVATED, BG_SURFACE, BORDER_SUBTLE, RADIUS_CONTROL, TEXT_MUTED, TEXT_PRIMARY,
    TEXT_SECONDARY, format_size, soft_tint, solid,
};
use crate::ui::workspace::Workspace;

pub struct ShapedFolderHeader {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub line: ShapedLine,
}

#[derive(Default)]
pub struct TreemapCanvasCache {
    pub tree_version: u64,
    pub root_id: NodeId,
    pub show_hidden: bool,
    pub size_unit: SizeUnit,
    pub bounds: Bounds<Pixels>,
    pub origin: Point<Pixels>,
    pub width: f32,
    pub height: f32,
    pub layout: TreemapLayout,
    pub shaped_headers: Vec<ShapedFolderHeader>,
}

const LEGEND_ITEMS: [(&str, u32); 7] = [
    ("Video", 0x4f46e5),
    ("Images", 0x0d9488),
    ("Archives", 0xd97706),
    ("Binaries", 0x0284c7),
    ("Code", 0x059669),
    ("Docs", 0x7c3aed),
    ("Disk/Sys", 0xdc2626),
];

pub fn render_treemap_pane(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let Some(tree) = ws.tree.clone() else {
        return div().size_full().bg(solid(BG_BASE)).into_any_element();
    };

    let accent = ws.settings.accent;
    let unit = ws.settings.size_unit;
    let show_hidden = ws.settings.show_hidden_files;
    let treemap_root = ws
        .treemap_root
        .filter(|&id| tree.get(id).is_some_and(|n| !n.removed))
        .unwrap_or(tree.root);
    let can_go_up = treemap_root != tree.root
        && tree.get(treemap_root).and_then(|n| n.parent).is_some();

    let mut crumbs: Vec<(NodeId, SharedString)> = Vec::new();
    let mut cursor = Some(treemap_root);
    while let Some(cid) = cursor {
        if let Some(n) = tree.get(cid) {
            crumbs.push((cid, n.name.clone()));
            if cid == tree.root {
                break;
            }
            cursor = n.parent;
        } else {
            break;
        }
    }
    crumbs.reverse();

    let active_hover = ws
        .treemap_hovered
        .or(ws.list_hovered)
        .filter(|&hid| hid != treemap_root);
    let hover_from_list = ws.treemap_hovered.is_none() && active_hover.is_some();

    let hover_info: Option<(Option<SharedString>, SharedString, SharedString)> =
        active_hover.and_then(|hid| {
            let node = tree.get(hid)?;
            let parent_prefix = tree.full_path(hid).and_then(|p| {
                let parent = p.parent()?;
                let s = parent.to_string_lossy();
                if s.is_empty() {
                    None
                } else {
                    Some(SharedString::from(format_parent_prefix(&s)))
                }
            });
            let item_name = node.name.clone();
            let root_sz = tree.get(treemap_root).map(|r| r.logical_size).unwrap_or(0);
            let pct = if root_sz == 0 {
                0.0
            } else {
                (node.logical_size as f64 / root_sz as f64 * 100.0).clamp(0.0, 100.0)
            };
            Some((
                parent_prefix,
                item_name,
                SharedString::from(format!(
                    "{} ({:.1}%)",
                    format_size(node.logical_size, unit),
                    pct
                )),
            ))
        });
    let has_hover_info = hover_info.is_some();

    let cache_for_canvas: Rc<RefCell<TreemapCanvasCache>> = ws.treemap_cache.clone();
    let cache_for_paint = ws.treemap_cache.clone();
    let tree_for_prepaint: Arc<DiskTree> = tree.clone();
    let tree_for_paint: Arc<DiskTree> = tree;
    let tree_version = ws.tree_version;
    let selected_node = ws.selected_node;
    let hovered_node = active_hover;
    let accent_hex = accent.primary_hex();

    v_flex()
        .size_full()
        .bg(solid(BG_BASE))
        .overflow_hidden()
        .child(
            h_flex()
                .w_full()
                .h(px(28.))
                .px_2p5()
                .gap_2()
                .items_center()
                .justify_between()
                .bg(solid(BG_SURFACE))
                .border_b_1()
                .border_color(solid(BORDER_SUBTLE))
                .child(
                    h_flex()
                        .min_w_0()
                        .when(!has_hover_info, |r| r.flex_1())
                        .when(has_hover_info, |r| r.flex_shrink_0().max_w(px(260.)))
                        .items_center()
                        .gap_1()
                        .overflow_hidden()
                        .when(can_go_up, |row| {
                            row.child(
                                Button::new("treemap-up-btn")
                                    .with_size(Size::XSmall)
                                    .ghost()
                                    .icon(IconName::ArrowUp)
                                    .tooltip("Zoom Out One Level (Backspace)")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.treemap_zoom_up(cx);
                                    })),
                            )
                        })
                        .children(crumbs.into_iter().enumerate().map(|(ix, (cid, label))| {
                            let is_last = cid == treemap_root;
                            h_flex()
                                .items_center()
                                .gap_0p5()
                                .flex_shrink_0()
                                .when(ix > 0, |el| {
                                    el.child(
                                        Icon::new(IconName::ChevronRight)
                                            .xsmall()
                                            .text_color(solid(TEXT_MUTED)),
                                    )
                                })
                                .child(
                                    div()
                                        .id(("tm-crumb", ix))
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(px(RADIUS_CONTROL))
                                        .cursor_pointer()
                                        .text_xs()
                                        .when(is_last, |d| {
                                            d.font_semibold().text_color(solid(TEXT_PRIMARY))
                                        })
                                        .when(!is_last, |d| {
                                            d.text_color(solid(TEXT_SECONDARY)).hover(|s| {
                                                s.bg(solid(BG_ELEVATED))
                                                    .text_color(solid(TEXT_PRIMARY))
                                            })
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_treemap_root(cid, cx);
                                        }))
                                        .child(label),
                                )
                        })),
                )
                .child(
                    h_flex()
                        .when(has_hover_info, |r| r.flex_1().min_w_0().justify_end())
                        .when(!has_hover_info, |r| r.flex_shrink_0())
                        .items_center()
                        .gap_2()
                        .child(if let Some((parent_prefix, item_name, size_pct)) = hover_info {
                            h_flex()
                                .flex_1()
                                .min_w_0()
                                .justify_end()
                                .items_center()
                                .gap_2()
                                .child(
                                    h_flex()
                                        .min_w_0()
                                        .flex_1()
                                        .justify_end()
                                        .items_center()
                                        .overflow_hidden()
                                        .when_some(parent_prefix, |row, prefix| {
                                            row.child(
                                                div()
                                                    .min_w_0()
                                                    .flex_shrink(1.0)
                                                    .truncate()
                                                    .text_xs()
                                                    .text_color(solid(TEXT_MUTED))
                                                    .child(prefix),
                                            )
                                        })
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .max_w(px(340.))
                                                .truncate()
                                                .text_xs()
                                                .font_medium()
                                                .text_color(solid(TEXT_PRIMARY))
                                                .child(item_name),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(solid(accent_hex))
                                        .child(size_pct),
                                )
                                .into_any_element()
                        } else {
                            h_flex()
                                .items_center()
                                .gap_2()
                                .children(LEGEND_ITEMS.into_iter().map(|(label, color)| {
                                    h_flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            div()
                                                .w(px(7.))
                                                .h(px(7.))
                                                .rounded(px(2.))
                                                .bg(solid(color)),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(solid(TEXT_MUTED))
                                                .child(label),
                                        )
                                }))
                                .into_any_element()
                        })
                        .child(
                            Button::new("treemap-hide-btn")
                                .with_size(Size::XSmall)
                                .ghost()
                                .icon(IconName::Close)
                                .tooltip("Hide Treemap (Ctrl+T)")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toggle_treemap(cx);
                                })),
                        ),
                ),
        )
        .child(
            div()
                .id("treemap-canvas-wrap")
                .flex_1()
                .min_h_0()
                .w_full()
                .bg(solid(BG_BASE))
                .cursor_pointer()
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    if !*hovered {
                        this.clear_treemap_hover(cx);
                    }
                }))
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    this.handle_treemap_hover(event.position, cx);
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        this.handle_treemap_click(event.position, event.click_count, cx);
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.handle_treemap_context_menu(event.position, cx);
                    }),
                )
                .child(
                    canvas(
                        move |bounds: Bounds<Pixels>, window, _cx| {
                            let w = f32::from(bounds.size.width).max(0.0);
                            let h = f32::from(bounds.size.height).max(0.0);
                            let mut cache = cache_for_canvas.borrow_mut();
                            cache.bounds = bounds;
                            cache.origin = bounds.origin;
                            let needs_recompute = cache.tree_version != tree_version
                                || cache.root_id != treemap_root
                                || cache.show_hidden != show_hidden
                                || cache.size_unit != unit
                                || (cache.width - w).abs() > 1.5
                                || (cache.height - h).abs() > 1.5;
                            if needs_recompute {
                                cache.layout = TreemapLayout::compute(
                                    &tree_for_prepaint,
                                    treemap_root,
                                    w,
                                    h,
                                    show_hidden,
                                );

                                // Pre-shape inline depth-1 folder header text ONCE per layout change.
                                let base_font = window.text_style().font();
                                let mut bold_font = base_font.clone();
                                bold_font.weight = FontWeight::SEMIBOLD;
                                let font_size = px(10.5);
                                let text_sys = window.text_system();

                                cache.shaped_headers = cache
                                    .layout
                                    .headers
                                    .iter()
                                    .filter_map(|hdr| {
                                        let node = tree_for_prepaint.get(hdr.node_id)?;
                                        let name_str = node.name.replace('\n', " ");
                                        let show_size = hdr.w >= 135.0;
                                        let (full_text, runs) = if show_size {
                                            let size_suffix = format!(
                                                "  {}",
                                                format_size(node.logical_size, unit)
                                            );
                                            let name_len = name_str.len();
                                            let suffix_len = size_suffix.len();
                                            let combined = format!("{name_str}{size_suffix}");
                                            (
                                                SharedString::from(combined),
                                                vec![
                                                    TextRun {
                                                        len: name_len,
                                                        font: bold_font.clone(),
                                                        color: solid(TEXT_PRIMARY),
                                                        background_color: None,
                                                        underline: None,
                                                        strikethrough: None,
                                                    },
                                                    TextRun {
                                                        len: suffix_len,
                                                        font: base_font.clone(),
                                                        color: solid(TEXT_MUTED),
                                                        background_color: None,
                                                        underline: None,
                                                        strikethrough: None,
                                                    },
                                                ],
                                            )
                                        } else {
                                            let name_len = name_str.len();
                                            (
                                                SharedString::from(name_str),
                                                vec![TextRun {
                                                    len: name_len,
                                                    font: bold_font.clone(),
                                                    color: solid(TEXT_PRIMARY),
                                                    background_color: None,
                                                    underline: None,
                                                    strikethrough: None,
                                                }],
                                            )
                                        };
                                        let line = text_sys.shape_line(
                                            full_text,
                                            font_size,
                                            &runs,
                                            None,
                                        );
                                        Some(ShapedFolderHeader {
                                            x: hdr.x,
                                            y: hdr.y,
                                            w: hdr.w,
                                            h: hdr.h,
                                            line,
                                        })
                                    })
                                    .collect();

                                cache.tree_version = tree_version;
                                cache.root_id = treemap_root;
                                cache.show_hidden = show_hidden;
                                cache.size_unit = unit;
                                cache.width = w;
                                cache.height = h;
                            }
                        },
                        move |bounds: Bounds<Pixels>, (), window, cx| {
                            window.paint_quad(fill(bounds, solid(BG_BASE)));

                            let cache = cache_for_paint.borrow();
                            let ox = f32::from(bounds.origin.x);
                            let oy = f32::from(bounds.origin.y);

                            // 1. Paint depth-1 and depth-2 folder container plates with crisp structural borders.
                            for frame in &cache.layout.frames {
                                let rect = Bounds {
                                    origin: point(px(ox + frame.x), px(oy + frame.y)),
                                    size: GpuiSize {
                                        width: px(frame.w),
                                        height: px(frame.h),
                                    },
                                };
                                let border_col = if frame.depth == 1 {
                                    solid(0x3f3f46)
                                } else {
                                    solid(0x2c2c34)
                                };
                                window.paint_quad(PaintQuad {
                                    bounds: rect,
                                    corner_radii: (0.).into(),
                                    background: solid(frame.color_hex).into(),
                                    border_widths: px(1.).into(),
                                    border_color: border_col,
                                    border_style: BorderStyle::Solid,
                                });
                            }

                            // 2. Paint inline depth-1 folder header bars and pre-shaped titles.
                            for hdr in &cache.shaped_headers {
                                let hdr_rect = Bounds {
                                    origin: point(px(ox + hdr.x), px(oy + hdr.y)),
                                    size: GpuiSize {
                                        width: px(hdr.w),
                                        height: px(hdr.h),
                                    },
                                };
                                window.paint_quad(fill(hdr_rect, solid(BG_ELEVATED)));
                                // Subtle 1px bottom divider under folder header
                                let div_rect = Bounds {
                                    origin: point(px(ox + hdr.x), px(oy + hdr.y + hdr.h - 1.0)),
                                    size: GpuiSize {
                                        width: px(hdr.w),
                                        height: px(1.0),
                                    },
                                };
                                window.paint_quad(fill(div_rect, solid(0x2e2e36)));

                                let clip_rect = Bounds {
                                    origin: point(px(ox + hdr.x + 4.0), px(oy + hdr.y)),
                                    size: GpuiSize {
                                        width: px((hdr.w - 8.0).max(0.0)),
                                        height: px(hdr.h),
                                    },
                                };
                                window.with_content_mask(
                                    Some(ContentMask { bounds: clip_rect }),
                                    |window| {
                                        let _ = hdr.line.paint(
                                            point(px(ox + hdr.x + 5.0), px(oy + hdr.y + 2.0)),
                                            px(14.0),
                                            TextAlign::Left,
                                            None,
                                            window,
                                            cx,
                                        );
                                    },
                                );
                            }

                            // 3. Paint leaf tiles inset by 0.5px for crisp 1px dark seams.
                            for tile in &cache.layout.leaves {
                                let inset = if tile.w > 4.0 && tile.h > 4.0 { 0.5 } else { 0.0 };
                                let tw = (tile.w - inset * 2.0).max(1.0);
                                let th = (tile.h - inset * 2.0).max(1.0);
                                let rect = Bounds {
                                    origin: point(px(ox + tile.x + inset), px(oy + tile.y + inset)),
                                    size: GpuiSize {
                                        width: px(tw),
                                        height: px(th),
                                    },
                                };
                                window.paint_quad(fill(rect, solid(tile.color_hex)));
                            }

                            // 4. Re-stroke depth-1 frames subtly on top so outer folder borders stay razor-sharp.
                            for frame in &cache.layout.frames {
                                if frame.depth == 1 {
                                    let rect = Bounds {
                                        origin: point(px(ox + frame.x), px(oy + frame.y)),
                                        size: GpuiSize {
                                            width: px(frame.w),
                                            height: px(frame.h),
                                        },
                                    };
                                    window.paint_quad(outline(
                                        rect,
                                        solid(0x3f3f46),
                                        BorderStyle::Solid,
                                    ));
                                }
                            }

                            // 5. Selected node highlight (2px accent border; skip outer rim when root is selected)
                            if let Some(sel_id) = selected_node
                                && sel_id != treemap_root
                                && let Some(sel) =
                                    cache.layout.highlight_rect_for_node(&tree_for_paint, sel_id)
                            {
                                let rect = Bounds {
                                    origin: point(px(ox + sel.x), px(oy + sel.y)),
                                    size: GpuiSize {
                                        width: px(sel.w.max(2.0)),
                                        height: px(sel.h.max(2.0)),
                                    },
                                };
                                window.paint_quad(PaintQuad {
                                    bounds: rect,
                                    corner_radii: (0.).into(),
                                    background: hsla(0., 0., 1., 0.08).into(),
                                    border_widths: px(2.).into(),
                                    border_color: solid(accent_hex),
                                    border_style: BorderStyle::Solid,
                                });
                            }

                            // 6. Hovered tile/folder highlight (works both from Treemap mouseover and Tree/Files list hover)
                            if let Some(hov_id) = hovered_node
                                && let Some(hov) =
                                    cache.layout.highlight_rect_for_node(&tree_for_paint, hov_id)
                            {
                                let rect = Bounds {
                                    origin: point(px(ox + hov.x), px(oy + hov.y)),
                                    size: GpuiSize {
                                        width: px(hov.w.max(2.0)),
                                        height: px(hov.h.max(2.0)),
                                    },
                                };
                                window.paint_quad(PaintQuad {
                                    bounds: rect,
                                    corner_radii: (0.).into(),
                                    background: if hover_from_list {
                                        soft_tint(accent_hex, 0.22).into()
                                    } else {
                                        hsla(0., 0., 1., 0.16).into()
                                    },
                                    border_widths: if hover_from_list {
                                        px(2.).into()
                                    } else {
                                        px(1.5).into()
                                    },
                                    border_color: solid(TEXT_PRIMARY),
                                    border_style: BorderStyle::Solid,
                                });
                            }
                        },
                    )
                    .size_full(),
                ),
        )
        .into_any_element()
}

/// Formats a parent directory prefix with a trailing separator and smart middle elision
/// when the path is very deep so the drive root, immediate parent folders, and full item name
/// stay clearly visible without cutting off the filename.
fn format_parent_prefix(parent: &str) -> String {
    let sep = if cfg!(target_os = "windows") { '\\' } else { '/' };
    let trimmed = parent.trim_end_matches(['\\', '/']);
    let with_sep = if trimmed.is_empty() {
        sep.to_string()
    } else {
        format!("{trimmed}{sep}")
    };

    if with_sep.chars().count() <= 54 {
        return with_sep;
    }

    let parts: Vec<&str> = trimmed.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
    if parts.len() <= 4 {
        return with_sep;
    }

    let head = if parent.starts_with('/') {
        format!("/{}/{}/", parts[0], parts[1])
    } else {
        format!("{}{sep}{}{sep}", parts[0], parts[1])
    };
    let tail_a = parts[parts.len() - 2];
    let tail_b = parts[parts.len() - 1];
    format!("{head}…{sep}{tail_a}{sep}{tail_b}{sep}")
}
