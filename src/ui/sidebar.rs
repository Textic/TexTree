use gpui_kit::{
    Context, SharedString,
    assets::IconName as LucideIcon,
    component::{
        Icon, IconName, Sizable as _, Size, StyledExt as _,
        button::{Button, ButtonVariants as _},
        h_flex,
        scroll::ScrollableElement as _,
        v_flex,
    },
    div, prelude::*, px, relative,
};

use crate::models::{DriveKind, ScanEngineKind};
use crate::scanner::ScanTarget;
use crate::theme::{
    BG_BASE, BG_ELEVATED, BG_HOVER, BG_SURFACE, BORDER, BORDER_SUBTLE, RADIUS_CONTROL, RADIUS_ROW,
    STATUS_SUCCESS, STATUS_WARNING, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY, format_count,
    format_duration_ms, format_size, soft_tint, solid, usage_bar_color,
};
use crate::ui::workspace::{ScanState, Workspace};

pub fn render_sidebar(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let accent = ws.settings.accent;
    let unit = ws.settings.size_unit;
    let is_scanning = matches!(ws.scan_state, ScanState::Scanning(_));

    let fixed_vols: Vec<_> = ws
        .volumes
        .iter()
        .filter(|v| v.kind == DriveKind::Fixed)
        .collect();
    let summary_vols = if fixed_vols.is_empty() {
        ws.volumes.iter().collect::<Vec<_>>()
    } else {
        fixed_vols
    };
    let all_total: u64 = summary_vols.iter().map(|v| v.total_bytes).sum();
    let all_used: u64 = summary_vols.iter().map(|v| v.used_bytes()).sum();
    let all_frac = if all_total == 0 {
        0.0
    } else {
        (all_used as f64 / all_total as f64).clamp(0.0, 1.0) as f32
    };
    let all_selected = matches!(ws.selected_target, ScanTarget::AllFixedDrives);

    v_flex()
        .size_full()
        .bg(solid(BG_SURFACE))
        .overflow_hidden()
        .child(
            h_flex()
                .w_full()
                .h(px(34.))
                .px_3()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(solid(BORDER_SUBTLE))
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(solid(TEXT_SECONDARY))
                        .child("STORAGE"),
                )
                .child(
                    Button::new("sidebar-refresh-drives")
                        .with_size(Size::XSmall)
                        .ghost()
                        .icon(IconName::RefreshCw)
                        .tooltip("Refresh Drives")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.refresh_volumes(cx);
                        })),
                ),
        )
        .child(
            v_flex()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .p_2p5()
                .gap_2()
                // Scan Entire PC card
                .child(
                    div()
                        .id("sidebar-scan-all-pc")
                        .w_full()
                        .p_2p5()
                        .rounded(px(RADIUS_ROW))
                        .cursor_pointer()
                        .bg(if all_selected {
                            solid(BG_ELEVATED)
                        } else {
                            solid(BG_BASE)
                        })
                        .border_1()
                        .border_color(if all_selected {
                            solid(accent.primary_hex())
                        } else {
                            solid(BORDER_SUBTLE)
                        })
                        .hover(|s| s.bg(solid(BG_ELEVATED)))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.start_scan(ScanTarget::AllFixedDrives, cx);
                        }))
                        .child(
                            v_flex()
                                .gap_1p5()
                                .child(
                                    h_flex()
                                        .items_center()
                                        .justify_between()
                                        .gap_1p5()
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .gap_1p5()
                                                .child(
                                                    Icon::new(LucideIcon::Monitor)
                                                        .small()
                                                        .text_color(solid(accent.primary_hex())),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_semibold()
                                                        .text_color(solid(TEXT_PRIMARY))
                                                        .child("Entire PC"),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded(px(RADIUS_CONTROL))
                                                .bg(soft_tint(accent.primary_hex(), 0.14))
                                                .text_xs()
                                                .text_color(solid(accent.primary_hex()))
                                                .child(SharedString::from(format!(
                                                    "{} drives",
                                                    summary_vols.len()
                                                ))),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .justify_between()
                                        .text_xs()
                                        .text_color(solid(TEXT_SECONDARY))
                                        .child(SharedString::from(format!(
                                            "{} / {}",
                                            format_size(all_used, unit),
                                            format_size(all_total, unit)
                                        )))
                                        .child(SharedString::from(format!(
                                            "{:.0}%",
                                            all_frac * 100.0
                                        ))),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .h(px(4.))
                                        .rounded(px(2.))
                                        .bg(solid(BG_SURFACE))
                                        .overflow_hidden()
                                        .child(
                                            div()
                                                .h_full()
                                                .w(relative(all_frac))
                                                .bg(solid(usage_bar_color(all_frac, accent))),
                                        ),
                                ),
                        ),
                )
                .child(
                    div()
                        .pt_1()
                        .px_0p5()
                        .text_xs()
                        .font_semibold()
                        .text_color(solid(TEXT_MUTED))
                        .child("DRIVES"),
                )
                .children(ws.volumes.iter().enumerate().map(|(idx, vol)| {
                    let is_active = matches!(
                        &ws.selected_target,
                        ScanTarget::Volume(v) if v.root_path == vol.root_path
                    );
                    let frac = vol.used_fraction();
                    let bar_color = usage_bar_color(frac, accent);
                    let vol_clone = vol.clone();
                    let title = if vol.label.is_empty() {
                        vol.kind.label().to_string()
                    } else {
                        vol.label.clone()
                    };

                    div()
                        .id(("sidebar-vol", idx))
                        .w_full()
                        .p_2()
                        .rounded(px(RADIUS_ROW))
                        .cursor_pointer()
                        .bg(if is_active {
                            solid(BG_ELEVATED)
                        } else {
                            solid(BG_SURFACE)
                        })
                        .border_1()
                        .border_color(if is_active {
                            solid(accent.primary_hex())
                        } else {
                            solid(BORDER_SUBTLE)
                        })
                        .hover(|s| s.bg(solid(BG_ELEVATED)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.start_scan(ScanTarget::Volume(vol_clone.clone()), cx);
                        }))
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    h_flex()
                                        .items_center()
                                        .justify_between()
                                        .gap_1p5()
                                        .child(
                                            h_flex()
                                                .min_w_0()
                                                .flex_1()
                                                .items_center()
                                                .gap_1p5()
                                                .child(
                                                    Icon::new(LucideIcon::HardDrive)
                                                        .small()
                                                        .text_color(if is_active {
                                                            solid(accent.primary_hex())
                                                        } else {
                                                            solid(TEXT_SECONDARY)
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_semibold()
                                                        .text_color(solid(TEXT_PRIMARY))
                                                        .child(SharedString::from(
                                                            vol.short_name.clone(),
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .min_w_0()
                                                        .flex_1()
                                                        .truncate()
                                                        .text_xs()
                                                        .text_color(solid(TEXT_SECONDARY))
                                                        .child(SharedString::from(title)),
                                                ),
                                        )
                                        .when(!vol.file_system.is_empty(), |row| {
                                            row.child(
                                                div()
                                                    .px_1()
                                                    .rounded(px(3.))
                                                    .bg(solid(BG_BASE))
                                                    .border_1()
                                                    .border_color(solid(BORDER_SUBTLE))
                                                    .text_xs()
                                                    .text_color(solid(TEXT_MUTED))
                                                    .child(SharedString::from(
                                                        vol.file_system.clone(),
                                                    )),
                                            )
                                        }),
                                )
                                .child(
                                    h_flex()
                                        .justify_between()
                                        .text_xs()
                                        .text_color(solid(TEXT_MUTED))
                                        .child(SharedString::from(format!(
                                            "{} / {}",
                                            format_size(vol.used_bytes(), unit),
                                            format_size(vol.total_bytes, unit)
                                        )))
                                        .child(SharedString::from(format!(
                                            "{:.0}%",
                                            frac * 100.0
                                        ))),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .h(px(3.))
                                        .rounded(px(2.))
                                        .bg(solid(BG_BASE))
                                        .overflow_hidden()
                                        .child(
                                            div()
                                                .h_full()
                                                .w(relative(frac))
                                                .bg(solid(bar_color)),
                                        ),
                                ),
                        )
                }))
                .child(
                    div()
                        .pt_1()
                        .child(
                            div()
                                .id("sidebar-pick-folder")
                                .w_full()
                                .h(px(32.))
                                .px_2p5()
                                .rounded(px(RADIUS_CONTROL))
                                .bg(solid(BG_BASE))
                                .border_1()
                                .border_color(solid(BORDER))
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap_1p5()
                                .cursor_pointer()
                                .text_xs()
                                .font_medium()
                                .text_color(solid(TEXT_PRIMARY))
                                .hover(|s| s.bg(solid(BG_HOVER)))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.pick_folder(window, cx);
                                }))
                                .child(
                                    Icon::new(LucideIcon::FolderSearch)
                                        .small()
                                        .text_color(solid(TEXT_SECONDARY)),
                                )
                                .child("Scan Custom Folder..."),
                        ),
                )
                .when(cfg!(target_os = "windows") && !ws.is_elevated, |col| {
                    col.child(
                        div()
                            .id("sidebar-relaunch-admin")
                            .w_full()
                            .h(px(30.))
                            .px_2p5()
                            .rounded(px(RADIUS_CONTROL))
                            .bg(soft_tint(accent.primary_hex(), 0.08))
                            .border_1()
                            .border_color(soft_tint(accent.primary_hex(), 0.25))
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_1p5()
                            .cursor_pointer()
                            .text_xs()
                            .font_medium()
                            .text_color(solid(accent.primary_hex()))
                            .hover(|s| s.bg(soft_tint(accent.primary_hex(), 0.16)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.relaunch_as_admin(cx);
                            }))
                            .child(Icon::new(LucideIcon::Zap).xsmall())
                            .child("Admin Mode (Fast MFT)"),
                    )
                }),
        )
        // Compact bottom scan summary card when a scan is finished or running
        .when(ws.tree.is_some() || is_scanning, |panel| {
            panel.child(render_scan_summary_footer(ws))
        })
}

fn render_scan_summary_footer(ws: &Workspace) -> impl IntoElement {
    let unit = ws.settings.size_unit;
    let (title, files, dirs, bytes, denied, elapsed_ms, engine_opt) = match &ws.scan_state {
        ScanState::Scanning(p) => (
            "Scanning...".to_string(),
            p.scanned_files,
            p.scanned_dirs,
            p.scanned_bytes,
            p.denied_count,
            p.elapsed_ms,
            None,
        ),
        _ => {
            if let Some(tree) = &ws.tree {
                let r = tree.root_node();
                (
                    format!("Scan: {}", tree.target_label),
                    r.file_count,
                    r.dir_count,
                    r.logical_size,
                    r.denied_count,
                    tree.scan_duration_ms,
                    Some(tree.engine),
                )
            } else {
                ("Idle".to_string(), 0, 0, 0, 0, 0, None)
            }
        }
    };

    v_flex()
        .w_full()
        .p_2p5()
        .gap_1()
        .bg(solid(BG_BASE))
        .border_t_1()
        .border_color(solid(BORDER_SUBTLE))
        .child(
            h_flex()
                .justify_between()
                .gap_2()
                .text_xs()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .font_semibold()
                        .text_color(solid(TEXT_SECONDARY))
                        .child(SharedString::from(title)),
                )
                .child(
                    div()
                        .text_color(solid(TEXT_MUTED))
                        .child(SharedString::from(format_duration_ms(elapsed_ms))),
                ),
        )
        .when_some(engine_opt, |col, eng| {
            let is_mft = matches!(eng, ScanEngineKind::NtfsMft | ScanEngineKind::Hybrid);
            col.child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .text_xs()
                    .child(div().text_color(solid(TEXT_MUTED)).child("Engine"))
                    .child(
                        div()
                            .px_1p5()
                            .rounded(px(3.))
                            .bg(if is_mft {
                                soft_tint(STATUS_SUCCESS, 0.14)
                            } else {
                                solid(BG_ELEVATED)
                            })
                            .text_color(if is_mft {
                                solid(STATUS_SUCCESS)
                            } else {
                                solid(TEXT_SECONDARY)
                            })
                            .child(eng.label()),
                    ),
            )
        })
        .child(
            h_flex()
                .justify_between()
                .text_xs()
                .text_color(solid(TEXT_SECONDARY))
                .child(SharedString::from(format!("{} files", format_count(files))))
                .child(SharedString::from(format!("{} folders", format_count(dirs)))),
        )
        .child(
            h_flex()
                .justify_between()
                .text_xs()
                .child(
                    div()
                        .font_medium()
                        .text_color(solid(TEXT_PRIMARY))
                        .child(SharedString::from(format_size(bytes, unit))),
                )
                .when(denied > 0, |row| {
                    row.child(
                        div()
                            .text_color(solid(STATUS_WARNING))
                            .child(SharedString::from(format!("{denied} denied"))),
                    )
                }),
        )
}
