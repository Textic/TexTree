use std::ops::Range;

use gpui_kit::{
    Context, MouseButton, MouseDownEvent, SharedString,
    component::{StyledExt as _, h_flex, v_flex},
    div, prelude::*, px, relative, uniform_list,
};

use crate::theme::{
    BG_BASE, BG_ELEVATED, BG_HOVER, BG_SURFACE, BORDER_SUBTLE, RADIUS_CONTROL, TEXT_MUTED,
    TEXT_PRIMARY, TEXT_SECONDARY, format_count, format_size, soft_tint, solid,
};
use crate::ui::workspace::Workspace;

const ROW_HEIGHT: f32 = 30.0;

pub fn render_extensions_view(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let count = ws.extension_rows.len();

    v_flex()
        .size_full()
        .bg(solid(BG_BASE))
        .overflow_hidden()
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
                .child(div().w(px(170.)).flex_shrink_0().px_2().child("Extension"))
                .child(div().flex_1().min_w(px(160.)).px_2().child("% of Scan"))
                .child(
                    div()
                        .w(px(110.))
                        .flex_shrink_0()
                        .px_2()
                        .flex()
                        .justify_end()
                        .child("Total Size"),
                )
                .child(
                    div()
                        .w(px(110.))
                        .flex_shrink_0()
                        .px_2()
                        .flex()
                        .justify_end()
                        .child("Allocated"),
                )
                .child(
                    div()
                        .w(px(110.))
                        .flex_shrink_0()
                        .px_2()
                        .flex()
                        .justify_end()
                        .child("Files"),
                ),
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
                        .child("No file types recorded"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Scan a drive or folder to inspect storage grouped by extension."),
                )
                .into_any_element()
        } else {
            uniform_list(
                "extensions-virtual-list",
                count,
                cx.processor(move |this, range: Range<usize>, _window, cx| {
                    render_extension_rows(this, range, cx)
                }),
            )
            .track_scroll(&ws.ext_scroll)
            .flex_1()
            .w_full()
            .into_any_element()
        })
}

fn render_extension_rows(
    ws: &mut Workspace,
    range: Range<usize>,
    cx: &mut Context<Workspace>,
) -> Vec<impl IntoElement + use<>> {
    let total_scan_bytes = ws
        .tree
        .as_ref()
        .map(|t| t.root_node().logical_size)
        .unwrap_or(0);
    let accent = ws.settings.accent;
    let unit = ws.settings.size_unit;

    range
        .filter_map(|ix| {
            let stat = ws.extension_rows.get(ix)?;
            let frac = if total_scan_bytes == 0 {
                0.0
            } else {
                (stat.total_size as f64 / total_scan_bytes as f64).clamp(0.0, 1.0) as f32
            };
            let ext_for_click = stat.extension.clone();
            let ext_for_ctx = stat.extension.clone();
            let row_bg = if ix % 2 == 1 {
                solid(BG_SURFACE)
            } else {
                solid(BG_BASE)
            };

            Some(
                h_flex()
                    .id(("ext-row", ix))
                    .w_full()
                    .h(px(ROW_HEIGHT))
                    .px_2()
                    .items_center()
                    .bg(row_bg)
                    .border_b_1()
                    .border_color(solid(BORDER_SUBTLE))
                    .hover(|s| s.bg(solid(BG_HOVER)))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, evt: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.open_extension_context_menu(ext_for_ctx.clone(), evt.position, cx);
                        }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.filter_by_extension(ext_for_click.clone(), cx);
                    }))
                    .child(
                        div()
                            .w(px(170.))
                            .flex_shrink_0()
                            .px_2()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded(px(RADIUS_CONTROL))
                                    .bg(soft_tint(accent.primary_hex(), 0.12))
                                    .text_xs()
                                    .font_medium()
                                    .text_color(solid(TEXT_PRIMARY))
                                    .child(SharedString::from(stat.extension.clone())),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w(px(160.))
                            .px_2()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(6.))
                                    .rounded(px(2.))
                                    .bg(solid(BG_BASE))
                                    .border_1()
                                    .border_color(solid(BORDER_SUBTLE))
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative(frac))
                                            .bg(solid(accent.primary_hex())),
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(46.))
                                    .flex_shrink_0()
                                    .flex()
                                    .justify_end()
                                    .text_xs()
                                    .text_color(solid(TEXT_SECONDARY))
                                    .child(SharedString::from(format!("{:.1}%", frac * 100.0))),
                            ),
                    )
                    .child(
                        div()
                            .w(px(110.))
                            .flex_shrink_0()
                            .px_2()
                            .flex()
                            .justify_end()
                            .text_xs()
                            .font_medium()
                            .text_color(solid(TEXT_PRIMARY))
                            .child(SharedString::from(format_size(stat.total_size, unit))),
                    )
                    .child(
                        div()
                            .w(px(110.))
                            .flex_shrink_0()
                            .px_2()
                            .flex()
                            .justify_end()
                            .text_xs()
                            .text_color(solid(TEXT_SECONDARY))
                            .child(SharedString::from(format_size(stat.allocated_size, unit))),
                    )
                    .child(
                        div()
                            .w(px(110.))
                            .flex_shrink_0()
                            .px_2()
                            .flex()
                            .justify_end()
                            .text_xs()
                            .text_color(solid(TEXT_SECONDARY))
                            .child(SharedString::from(format_count(stat.file_count))),
                    ),
            )
        })
        .collect()
}
