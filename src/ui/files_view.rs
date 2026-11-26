use std::ops::Range;

use gpui_kit::{
    Context, MouseButton, MouseDownEvent, SharedString,
    assets::IconName as LucideIcon,
    component::{
        Icon, IconName, Sizable as _, Size, StyledExt as _,
        button::{Button, ButtonVariants as _},
        h_flex, v_flex,
    },
    div, prelude::*, px, relative, uniform_list,
};

use crate::models::NodeFlags;
use crate::theme::{
    BG_BASE, BG_ELEVATED, BG_HOVER, BG_SURFACE, BORDER_SUBTLE, RADIUS_CONTROL, STATUS_ERROR,
    TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY, format_count, format_size, format_timestamp,
    soft_tint, solid,
};
use crate::ui::workspace::Workspace;

const ROW_HEIGHT: f32 = 28.0;

pub fn render_files_view(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let accent = ws.settings.accent;
    let count = ws.top_file_rows.len();

    v_flex()
        .id("top-files-root")
        .size_full()
        .bg(solid(BG_BASE))
        .overflow_hidden()
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            if !*hovered {
                this.clear_list_hovered(None, cx);
            }
        }))
        // Filter strip for extension pill + inaccessible-only toggle
        .child(
            h_flex()
                .w_full()
                .h(px(34.))
                .px_3()
                .items_center()
                .justify_between()
                .gap_2()
                .bg(solid(BG_SURFACE))
                .border_b_1()
                .border_color(solid(BORDER_SUBTLE))
                .child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(solid(TEXT_SECONDARY))
                                .child(SharedString::from(format!(
                                    "Showing top {} items",
                                    format_count(count as u64)
                                ))),
                        )
                        .when_some(ws.ext_filter.clone(), |row, ext| {
                            row.child(
                                h_flex()
                                    .px_2()
                                    .py_0p5()
                                    .gap_1()
                                    .items_center()
                                    .rounded(px(RADIUS_CONTROL))
                                    .bg(soft_tint(accent.primary_hex(), 0.14))
                                    .border_1()
                                    .border_color(soft_tint(accent.primary_hex(), 0.30))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(solid(accent.primary_hex()))
                                            .child(SharedString::from(format!("Ext: {ext}"))),
                                    )
                                    .child(
                                        div()
                                            .id("clear-ext-filter")
                                            .cursor_pointer()
                                            .text_color(solid(accent.primary_hex()))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.clear_extension_filter(cx);
                                            }))
                                            .child(Icon::new(IconName::Close).xsmall()),
                                    ),
                            )
                        }),
                )
                .child(
                    div()
                        .id("toggle-denied-only")
                        .px_2()
                        .py_0p5()
                        .rounded(px(RADIUS_CONTROL))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap_1()
                        .bg(if ws.denied_only_filter {
                            soft_tint(STATUS_ERROR, 0.16)
                        } else {
                            solid(BG_ELEVATED)
                        })
                        .border_1()
                        .border_color(if ws.denied_only_filter {
                            solid(STATUS_ERROR)
                        } else {
                            solid(BORDER_SUBTLE)
                        })
                        .text_xs()
                        .text_color(if ws.denied_only_filter {
                            solid(STATUS_ERROR)
                        } else {
                            solid(TEXT_SECONDARY)
                        })
                        .hover(|s| s.bg(solid(BG_HOVER)))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_denied_only_filter(cx);
                        }))
                        .child(Icon::new(LucideIcon::Lock).xsmall())
                        .child("Access Denied Only"),
                ),
        )
        // Sticky column header
        .child(
            h_flex()
                .w_full()
                .h(px(30.))
                .px_2()
                .items_center()
                .bg(solid(BG_ELEVATED))
                .border_b_1()
                .border_color(solid(BORDER_SUBTLE))
                .text_xs()
                .font_semibold()
                .text_color(solid(TEXT_SECONDARY))
                .child(div().flex_1().min_w(px(260.)).px_1p5().child("File & Location"))
                .child(div().w(px(110.)).flex_shrink_0().px_1p5().child("% of Scan"))
                .child(
                    div()
                        .w(px(96.))
                        .flex_shrink_0()
                        .px_1p5()
                        .flex()
                        .justify_end()
                        .child("Size"),
                )
                .child(
                    div()
                        .w(px(96.))
                        .flex_shrink_0()
                        .px_1p5()
                        .flex()
                        .justify_end()
                        .child("Allocated"),
                )
                .child(
                    div()
                        .w(px(124.))
                        .flex_shrink_0()
                        .px_1p5()
                        .flex()
                        .justify_end()
                        .child("Modified"),
                )
                .child(div().w(px(74.)).flex_shrink_0().px_1p5()),
        )
        .child(if count == 0 {
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
                        .child("No files match the current filter"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Adjust your search query, extension filter, or access filter."),
                )
                .into_any_element()
        } else {
            uniform_list(
                "top-files-virtual-list",
                count,
                cx.processor(move |this, range: Range<usize>, _window, cx| {
                    render_file_rows(this, range, cx)
                }),
            )
            .track_scroll(&ws.files_scroll)
            .flex_1()
            .w_full()
            .into_any_element()
        })
}

fn render_file_rows(
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
            let node_id = *ws.top_file_rows.get(ix)?;
            let node = tree.get(node_id)?;
            let is_selected = selected == Some(node_id);
            let is_denied = node.flags.contains(NodeFlags::ACCESS_DENIED);
            let pct = tree.root_percent(node_id);
            let file_name = node.name.clone();
            let parent_dir: Option<SharedString> = tree.full_path(node_id).and_then(|p| {
                let parent = p.parent()?;
                let s = parent.to_string_lossy();
                if s.is_empty() {
                    None
                } else {
                    Some(SharedString::from(s.into_owned()))
                }
            });

            let row_bg = if is_selected {
                solid(BG_ELEVATED)
            } else if ix % 2 == 1 {
                solid(BG_SURFACE)
            } else {
                solid(BG_BASE)
            };

            Some(
                h_flex()
                    .id(("top-file-row", ix))
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
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_node(node_id, cx);
                    }))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w(px(260.))
                            .px_1p5()
                            .items_center()
                            .gap_1p5()
                            .overflow_hidden()
                            .child(
                                Icon::new(if is_denied {
                                    LucideIcon::Lock
                                } else {
                                    LucideIcon::File
                                })
                                .xsmall()
                                .text_color(if is_denied {
                                    solid(STATUS_ERROR)
                                } else {
                                    solid(TEXT_SECONDARY)
                                }),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .max_w(px(320.))
                                    .truncate()
                                    .text_xs()
                                    .font_medium()
                                    .text_color(if is_denied {
                                        solid(STATUS_ERROR)
                                    } else {
                                        solid(TEXT_PRIMARY)
                                    })
                                    .child(file_name),
                            )
                            .when_some(parent_dir, |row, dir| {
                                row.child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .truncate()
                                        .text_xs()
                                        .text_color(solid(TEXT_MUTED))
                                        .child(dir),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .w(px(110.))
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
                                            .bg(solid(accent.primary_hex())),
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(38.))
                                    .flex_shrink_0()
                                    .flex()
                                    .justify_end()
                                    .text_xs()
                                    .text_color(solid(TEXT_SECONDARY))
                                    .child(SharedString::from(format!("{:.1}%", pct * 100.0))),
                            ),
                    )
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
                            .child(SharedString::from(format_size(node.logical_size, unit))),
                    )
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
                    .child(
                        div()
                            .w(px(74.))
                            .flex_shrink_0()
                            .px_1()
                            .flex()
                            .justify_end()
                            .child(
                                Button::new(("reveal-in-tree", ix))
                                    .with_size(Size::XSmall)
                                    .ghost()
                                    .label("Tree")
                                    .tooltip("Reveal in Tree View")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.reveal_node_in_tree(node_id, cx);
                                    })),
                            ),
                    ),
            )
        })
        .collect()
}
