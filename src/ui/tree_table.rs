use std::ops::Range;

use gpui_kit::{
    Context, MouseButton, MouseDownEvent, SharedString,
    assets::IconName as LucideIcon,
    component::{Icon, IconName, Sizable as _, StyledExt as _, h_flex, v_flex},
    div, prelude::*, px, relative, uniform_list,
};

use crate::models::{NodeFlags, SortColumn};
use crate::theme::{
    BG_BASE, BG_ELEVATED, BG_HOVER, BG_SURFACE, BORDER_SUBTLE, RADIUS_CONTROL, STATUS_ERROR,
    STATUS_INFO, STATUS_WARNING, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY, format_count,
    format_size, format_timestamp, soft_tint, solid,
};
use crate::ui::workspace::Workspace;

const ROW_HEIGHT: f32 = 28.0;

pub fn render_tree_table(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let row_count = ws.visible_rows.len();

    v_flex()
        .id("tree-table-root")
        .size_full()
        .bg(solid(BG_BASE))
        .overflow_hidden()
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            if !*hovered {
                this.clear_list_hovered(None, cx);
            }
        }))
        .child(render_table_header(ws, cx))
        .child(if row_count == 0 {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(solid(TEXT_PRIMARY))
                        .child("No matching items"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Clear the search filter or scan another drive."),
                )
                .into_any_element()
        } else {
            uniform_list(
                "tree-virtual-list",
                row_count,
                cx.processor(move |this, range: Range<usize>, _window, cx| {
                    render_tree_rows(this, range, cx)
                }),
            )
            .track_scroll(&ws.tree_scroll)
            .flex_1()
            .w_full()
            .into_any_element()
        })
}

fn render_table_header(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    h_flex()
        .w_full()
        .h(px(30.))
        .px_2()
        .items_center()
        .bg(solid(BG_ELEVATED))
        .border_b_1()
        .border_color(solid(BORDER_SUBTLE))
        .child(header_cell(
            "hdr-name",
            "Name",
            SortColumn::Name,
            ws.sort.column,
            ws.sort.descending,
            None,
            false,
            cx,
        ))
        .child(header_cell(
            "hdr-pct",
            "% of Parent",
            SortColumn::Percent,
            ws.sort.column,
            ws.sort.descending,
            Some(120.),
            false,
            cx,
        ))
        .child(header_cell(
            "hdr-size",
            "Total Size",
            SortColumn::Size,
            ws.sort.column,
            ws.sort.descending,
            Some(96.),
            true,
            cx,
        ))
        .child(header_cell(
            "hdr-alloc",
            "Allocated",
            SortColumn::Allocated,
            ws.sort.column,
            ws.sort.descending,
            Some(96.),
            true,
            cx,
        ))
        .child(header_cell(
            "hdr-items",
            "Items",
            SortColumn::Items,
            ws.sort.column,
            ws.sort.descending,
            Some(96.),
            true,
            cx,
        ))
        .child(header_cell(
            "hdr-mod",
            "Modified",
            SortColumn::Modified,
            ws.sort.column,
            ws.sort.descending,
            Some(124.),
            true,
            cx,
        ))
        .child(
            div()
                .w(px(90.))
                .flex_shrink_0()
                .px_1p5()
                .flex()
                .justify_end()
                .text_xs()
                .font_semibold()
                .text_color(solid(TEXT_SECONDARY))
                .child("Status"),
        )
}

#[allow(clippy::too_many_arguments)]
fn header_cell(
    id: &'static str,
    label: &'static str,
    col: SortColumn,
    active_col: SortColumn,
    descending: bool,
    fixed_width: Option<f32>,
    align_right: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let is_active = col == active_col;
    let base = div()
        .id(id)
        .h_full()
        .px_1p5()
        .flex()
        .items_center()
        .gap_1()
        .cursor_pointer()
        .text_xs()
        .font_semibold()
        .text_color(if is_active {
            solid(TEXT_PRIMARY)
        } else {
            solid(TEXT_SECONDARY)
        })
        .hover(|s| s.text_color(solid(TEXT_PRIMARY)))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_sort_column(col, cx);
        }));

    let sized = match fixed_width {
        Some(w) => base.w(px(w)).flex_shrink_0(),
        None => base.flex_1().min_w(px(200.)),
    };
    let aligned = if align_right {
        sized.justify_end()
    } else {
        sized.justify_start()
    };

    aligned.child(label).when(is_active, |el| {
        el.child(
            Icon::new(if descending {
                IconName::ArrowDown
            } else {
                IconName::ArrowUp
            })
            .xsmall(),
        )
    })
}

fn render_tree_rows(
    ws: &mut Workspace,
    range: Range<usize>,
    cx: &mut Context<Workspace>,
) -> Vec<impl IntoElement + use<>> {
    let Some(tree) = &ws.tree else {
        return Vec::new();
    };
    let accent = ws.settings.accent;
    let unit = ws.settings.size_unit;
    let selected = ws.selected_node;

    range
        .filter_map(|ix| {
            let row = *ws.visible_rows.get(ix)?;
            let node = tree.get(row.node_id)?;
            let node_id = row.node_id;
            let is_selected = selected == Some(node_id);
            let pct = tree.parent_percent(node_id);
            let indent_px = (row.depth.min(18) as f32) * 16.0;

            let is_denied = node.flags.contains(NodeFlags::ACCESS_DENIED);
            let is_symlink = node.flags.contains(NodeFlags::IS_SYMLINK);
            let is_hidden = node.flags.contains(NodeFlags::IS_HIDDEN);
            let is_system = node.flags.contains(NodeFlags::IS_SYSTEM);

            let (icon_el, icon_color) = if node.flags.contains(NodeFlags::IS_VIRTUAL) {
                (Icon::new(LucideIcon::Monitor), accent.primary_hex())
            } else if node.flags.contains(NodeFlags::IS_VOLUME) {
                (Icon::new(LucideIcon::HardDrive), accent.primary_hex())
            } else if is_denied {
                (Icon::new(LucideIcon::Lock), STATUS_ERROR)
            } else if is_symlink {
                (Icon::new(LucideIcon::Link), STATUS_INFO)
            } else if node.flags.contains(NodeFlags::IS_DIR) {
                if row.is_expanded {
                    (Icon::new(LucideIcon::FolderOpen), STATUS_WARNING)
                } else {
                    (Icon::new(LucideIcon::Folder), STATUS_WARNING)
                }
            } else {
                (Icon::new(LucideIcon::File), TEXT_SECONDARY)
            };

            let items_label = if node.is_container() {
                format_count(node.total_items())
            } else {
                "—".to_string()
            };

            let status_badge: Option<(&'static str, u32)> = if is_denied {
                Some(("Denied", STATUS_ERROR))
            } else if node.denied_count > 0 && node.is_container() {
                Some(("Partial", STATUS_WARNING))
            } else if is_symlink {
                Some(("Symlink", STATUS_INFO))
            } else if is_system {
                Some(("System", TEXT_MUTED))
            } else if is_hidden {
                Some(("Hidden", TEXT_MUTED))
            } else {
                None
            };

            let row_bg = if is_selected {
                solid(BG_ELEVATED)
            } else if ix % 2 == 1 {
                solid(BG_SURFACE)
            } else {
                solid(BG_BASE)
            };

            let pct_bar_color = if is_denied {
                STATUS_ERROR
            } else {
                accent.primary_hex()
            };

            let has_children = row.has_children;
            let is_expanded = row.is_expanded;
            let volume_subtitle: Option<SharedString> = if node.flags.contains(NodeFlags::IS_VOLUME) {
                ws.volumes
                    .iter()
                    .find(|v| v.root_path.to_string_lossy().eq_ignore_ascii_case(node.name.as_ref()))
                    .map(|v| {
                        let lbl = if v.label.is_empty() {
                            v.kind.label().to_string()
                        } else {
                            v.label.clone()
                        };
                        SharedString::from(lbl)
                    })
            } else {
                None
            };

            Some(
                h_flex()
                    .id(("tree-row", ix))
                    .w_full()
                    .h(px(ROW_HEIGHT))
                    .relative()
                    .px_2()
                    .items_center()
                    .bg(row_bg)
                    .border_b_1()
                    .border_color(solid(BORDER_SUBTLE))
                    .when(is_selected, |r| {
                        r.child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(2.))
                                .bg(solid(accent.primary_hex())),
                        )
                    })
                    .hover(|s| s.bg(solid(BG_HOVER)))
                    .cursor_pointer()
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.set_list_hovered(node_id, cx);
                        } else {
                            this.clear_list_hovered(Some(node_id), cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, evt: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.open_node_context_menu(node_id, false, evt.position, cx);
                        }),
                    )
                    .on_click(cx.listener(move |this, evt: &gpui_kit::ClickEvent, _, cx| {
                        this.select_node(node_id, cx);
                        if evt.click_count() >= 2 && has_children {
                            this.toggle_expand(node_id, cx);
                        }
                    }))
                    // Name column
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w(px(200.))
                            .px_1p5()
                            .items_center()
                            .gap_1()
                            .overflow_hidden()
                            .child(div().w(px(indent_px)).flex_shrink_0())
                            .child(if has_children {
                                div()
                                    .id(("tree-chev", ix))
                                    .w(px(16.))
                                    .h(px(16.))
                                    .flex()
                                    .flex_shrink_0()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(3.))
                                    .text_color(solid(TEXT_SECONDARY))
                                    .hover(|s| s.bg(solid(BG_BASE)).text_color(solid(TEXT_PRIMARY)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.toggle_expand(node_id, cx);
                                    }))
                                    .child(
                                        Icon::new(if is_expanded {
                                            IconName::ChevronDown
                                        } else {
                                            IconName::ChevronRight
                                        })
                                        .xsmall(),
                                    )
                                    .into_any_element()
                            } else {
                                div().w(px(16.)).flex_shrink_0().into_any_element()
                            })
                            .child(icon_el.xsmall().text_color(solid(icon_color)))
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .flex_1()
                                    .items_center()
                                    .gap_1p5()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .when(volume_subtitle.is_none(), |d| d.min_w_0().flex_1())
                                            .truncate()
                                            .text_xs()
                                            .when(node.is_container(), |d| d.font_medium())
                                            .text_color(if is_denied {
                                                solid(STATUS_ERROR)
                                            } else if is_hidden {
                                                solid(TEXT_SECONDARY)
                                            } else {
                                                solid(TEXT_PRIMARY)
                                            })
                                            .child(node.name.clone()),
                                    )
                                    .when_some(volume_subtitle, |row, lbl| {
                                        row.child(
                                            div()
                                                .min_w_0()
                                                .flex_1()
                                                .truncate()
                                                .text_xs()
                                                .text_color(solid(TEXT_MUTED))
                                                .child(lbl),
                                        )
                                    }),
                            ),
                    )
                    // % of Parent bar + percentage
                    .child(
                        h_flex()
                            .w(px(120.))
                            .flex_shrink_0()
                            .px_1p5()
                            .items_center()
                            .gap_1p5()
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(5.))
                                    .rounded(px(2.))
                                    .bg(solid(BG_ELEVATED))
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative(pct))
                                            .bg(solid(pct_bar_color)),
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(40.))
                                    .flex_shrink_0()
                                    .flex()
                                    .justify_end()
                                    .text_xs()
                                    .text_color(solid(TEXT_SECONDARY))
                                    .child(SharedString::from(format!("{:.1}%", pct * 100.0))),
                            ),
                    )
                    // Total Size
                    .child(
                        div()
                            .w(px(96.))
                            .flex_shrink_0()
                            .px_1p5()
                            .flex()
                            .justify_end()
                            .text_xs()
                            .font_medium()
                            .text_color(solid(TEXT_PRIMARY))
                            .child(SharedString::from(if is_denied && node.logical_size == 0 {
                                "Denied".to_string()
                            } else {
                                format_size(node.logical_size, unit)
                            })),
                    )
                    // Allocated Size
                    .child(
                        div()
                            .w(px(96.))
                            .flex_shrink_0()
                            .px_1p5()
                            .flex()
                            .justify_end()
                            .text_xs()
                            .text_color(solid(TEXT_SECONDARY))
                            .child(SharedString::from(format_size(node.allocated_size, unit))),
                    )
                    // Items
                    .child(
                        div()
                            .w(px(96.))
                            .flex_shrink_0()
                            .px_1p5()
                            .flex()
                            .justify_end()
                            .text_xs()
                            .text_color(solid(TEXT_SECONDARY))
                            .child(SharedString::from(items_label)),
                    )
                    // Modified
                    .child(
                        div()
                            .w(px(124.))
                            .flex_shrink_0()
                            .px_1p5()
                            .flex()
                            .justify_end()
                            .text_xs()
                            .text_color(solid(TEXT_MUTED))
                            .child(SharedString::from(format_timestamp(node.modified_secs))),
                    )
                    // Status badge
                    .child(
                        div()
                            .w(px(90.))
                            .flex_shrink_0()
                            .px_1p5()
                            .flex()
                            .justify_end()
                            .when_some(status_badge, |cell, (label, hex)| {
                                cell.child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(px(RADIUS_CONTROL))
                                        .bg(soft_tint(hex, 0.14))
                                        .border_1()
                                        .border_color(soft_tint(hex, 0.30))
                                        .text_xs()
                                        .text_color(solid(hex))
                                        .child(label),
                                )
                            }),
                    ),
            )
        })
        .collect()
}
