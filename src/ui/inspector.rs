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

use crate::models::NodeFlags;
use crate::platform::FileLockStatus;
use crate::theme::{
    BG_BASE, BG_ELEVATED, BG_HOVER, BG_SURFACE, BORDER, BORDER_SUBTLE, RADIUS_CONTROL, RADIUS_ROW,
    STATUS_ERROR, STATUS_INFO, STATUS_SUCCESS, STATUS_WARNING, TEXT_MUTED, TEXT_PRIMARY,
    TEXT_SECONDARY, format_count, format_size, format_timestamp, soft_tint, solid,
};
use crate::ui::workspace::Workspace;

pub fn render_inspector(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let accent = ws.settings.accent;
    let unit = ws.settings.size_unit;

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
                        .child("INSPECTOR"),
                )
                .child(
                    Button::new("inspector-close-btn")
                        .with_size(Size::XSmall)
                        .ghost()
                        .icon(IconName::Close)
                        .tooltip("Hide Inspector (Ctrl+I)")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_inspector = false;
                            cx.notify();
                        })),
                ),
        )
        .child(match (&ws.tree, ws.selected_node) {
            (Some(tree), Some(node_id)) if tree.get(node_id).is_some() => {
                let node = tree.get(node_id).unwrap();
                let has_fs_path = ws.selected_has_fs_path;
                let path_display = ws
                    .selected_full_path
                    .clone()
                    .unwrap_or_else(|| node.name.clone());
                let is_container = node.is_container();
                let is_virtual = node.flags.contains(NodeFlags::IS_VIRTUAL);
                let is_volume = node.flags.contains(NodeFlags::IS_VOLUME);
                let is_denied = node.flags.contains(NodeFlags::ACCESS_DENIED);
                let can_trash = !is_virtual && !is_volume && node_id != tree.root;
                let can_rescan = !is_virtual && is_container && has_fs_path;

                let kind_label = if is_virtual {
                    "Virtual Root"
                } else if is_volume {
                    "Drive"
                } else if node.flags.contains(NodeFlags::IS_SYMLINK) {
                    "Symlink / Junction"
                } else if is_container {
                    "Folder"
                } else {
                    "File"
                };

                let parent_pct = tree.parent_percent(node_id);
                let root_pct = tree.root_percent(node_id);
                let ext = tree.extension_of(node_id).to_string();
                let top_children = &ws.selected_top_children;

                v_flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p_3()
                    .gap_3()
                    // Title & Type header
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .truncate()
                                            .text_sm()
                                            .font_semibold()
                                            .text_color(solid(TEXT_PRIMARY))
                                            .child(node.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded(px(RADIUS_CONTROL))
                                            .bg(solid(BG_ELEVATED))
                                            .border_1()
                                            .border_color(solid(BORDER_SUBTLE))
                                            .text_xs()
                                            .text_color(solid(TEXT_SECONDARY))
                                            .child(kind_label),
                                    ),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .p_2()
                                    .rounded(px(RADIUS_CONTROL))
                                    .bg(solid(BG_BASE))
                                    .border_1()
                                    .border_color(solid(BORDER_SUBTLE))
                                    .text_xs()
                                    .text_color(solid(TEXT_SECONDARY))
                                    .child(path_display),
                            ),
                    )
                    // Action buttons
                    .when(has_fs_path, |col| {
                        col.child(
                            h_flex()
                                .w_full()
                                .flex_wrap()
                                .gap_1p5()
                                .child(
                                    Button::new("insp-reveal")
                                        .with_size(Size::XSmall)
                                        .secondary()
                                        .icon(LucideIcon::ExternalLink)
                                        .label("Reveal")
                                        .tooltip("Reveal in File Explorer")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.reveal_selected_in_explorer(cx);
                                        })),
                                )
                                .child(
                                    Button::new("insp-copy-path")
                                        .with_size(Size::XSmall)
                                        .secondary()
                                        .icon(IconName::Copy)
                                        .label("Copy Path")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.copy_selected_path(cx);
                                        })),
                                )
                                .when(can_rescan, |row| {
                                    row.child(
                                        Button::new("insp-rescan-branch")
                                            .with_size(Size::XSmall)
                                            .secondary()
                                            .icon(IconName::RefreshCw)
                                            .label("Scan Here")
                                            .tooltip("Scan only this folder")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.scan_selected_folder(cx);
                                            })),
                                    )
                                })
                                .when(can_trash, |row| {
                                    row.child(
                                        Button::new("insp-trash")
                                            .with_size(Size::XSmall)
                                            .danger()
                                            .icon(IconName::Delete)
                                            .label("Trash")
                                            .tooltip("Move to Recycle Bin (Delete)")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.request_trash_selected(cx);
                                            })),
                                    )
                                }),
                        )
                    })
                    // Storage metrics 2x2 grid
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_1p5()
                                    .child(metric_card(
                                        "Total Size",
                                        format_size(node.logical_size, unit),
                                        TEXT_PRIMARY,
                                    ))
                                    .child(metric_card(
                                        "Size on Disk",
                                        format_size(node.allocated_size, unit),
                                        TEXT_PRIMARY,
                                    )),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_1p5()
                                    .child(metric_card(
                                        "% of Parent",
                                        format!("{:.2}%", parent_pct * 100.0),
                                        accent.primary_hex(),
                                    ))
                                    .child(metric_card(
                                        "% of Scan",
                                        format!("{:.2}%", root_pct * 100.0),
                                        TEXT_SECONDARY,
                                    )),
                            ),
                    )
                    // Container breakdown OR File metadata + Lock Status
                    .when(is_container, |col| {
                        col.child(
                            v_flex()
                                .p_2p5()
                                .gap_1p5()
                                .rounded(px(RADIUS_ROW))
                                .bg(solid(BG_BASE))
                                .border_1()
                                .border_color(solid(BORDER_SUBTLE))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(solid(TEXT_SECONDARY))
                                        .child("CONTENTS"),
                                )
                                .child(kv_row("Files", format_count(node.file_count), TEXT_PRIMARY))
                                .child(kv_row("Folders", format_count(node.dir_count), TEXT_PRIMARY))
                                .child(kv_row(
                                    "Access Denied",
                                    format_count(u64::from(node.denied_count)),
                                    if node.denied_count > 0 || is_denied {
                                        STATUS_WARNING
                                    } else {
                                        TEXT_MUTED
                                    },
                                ))
                                .child(kv_row(
                                    "Modified",
                                    format_timestamp(node.modified_secs),
                                    TEXT_SECONDARY,
                                )),
                        )
                        .when(!top_children.is_empty(), |col| {
                            col.child(
                                v_flex()
                                    .p_2p5()
                                    .gap_2()
                                    .rounded(px(RADIUS_ROW))
                                    .bg(solid(BG_BASE))
                                    .border_1()
                                    .border_color(solid(BORDER_SUBTLE))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(solid(TEXT_SECONDARY))
                                            .child("LARGEST CHILDREN"),
                                    )
                                    .children(top_children.iter().copied().enumerate().filter_map(
                                        |(i, cid)| {
                                            let child = tree.get(cid)?;
                                            let c_pct = tree.parent_percent(cid);
                                            Some(
                                                div()
                                                    .id(("insp-child", i))
                                                    .w_full()
                                                    .p_1p5()
                                                    .rounded(px(RADIUS_CONTROL))
                                                    .cursor_pointer()
                                                    .hover(|s| s.bg(solid(BG_ELEVATED)))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.reveal_node_in_tree(cid, cx);
                                                    }))
                                                    .child(
                                                        v_flex()
                                                            .gap_1()
                                                            .child(
                                                                h_flex()
                                                                    .justify_between()
                                                                    .gap_2()
                                                                    .child(
                                                                        div()
                                                                            .min_w_0()
                                                                            .flex_1()
                                                                            .truncate()
                                                                            .text_xs()
                                                                            .text_color(solid(
                                                                                TEXT_PRIMARY,
                                                                            ))
                                                                            .child(child.name.clone()),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .flex_shrink_0()
                                                                            .text_xs()
                                                                            .text_color(solid(
                                                                                TEXT_SECONDARY,
                                                                            ))
                                                                            .child(
                                                                                SharedString::from(
                                                                                    format_size(
                                                                                        child
                                                                                            .logical_size,
                                                                                        unit,
                                                                                    ),
                                                                                ),
                                                                            ),
                                                                    ),
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
                                                                            .w(relative(c_pct))
                                                                            .bg(solid(
                                                                                accent
                                                                                    .primary_hex(),
                                                                            )),
                                                                    ),
                                                            ),
                                                    ),
                                            )
                                        },
                                    )),
                            )
                        })
                    })
                    .when(!is_container, |col| {
                        col.child(
                            v_flex()
                                .p_2p5()
                                .gap_1p5()
                                .rounded(px(RADIUS_ROW))
                                .bg(solid(BG_BASE))
                                .border_1()
                                .border_color(solid(BORDER_SUBTLE))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(solid(TEXT_SECONDARY))
                                        .child("FILE DETAILS"),
                                )
                                .child(kv_row(
                                    "Extension",
                                    if ext.is_empty() {
                                        "None".to_string()
                                    } else {
                                        format!(".{ext}")
                                    },
                                    TEXT_PRIMARY,
                                ))
                                .child(kv_row(
                                    "Modified",
                                    format_timestamp(node.modified_secs),
                                    TEXT_SECONDARY,
                                ))
                                .child(render_attribute_badges(node.flags)),
                        )
                        .child(render_lock_card(&ws.lock_status, cx))
                    })
                    .into_any_element()
            }
            _ => v_flex()
                .flex_1()
                .p_4()
                .items_center()
                .justify_center()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(solid(TEXT_PRIMARY))
                        .child("No Selection"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Select any folder or file to inspect details and locks."),
                )
                .into_any_element(),
        })
}

fn metric_card(label: &'static str, value: String, value_color: u32) -> impl IntoElement {
    v_flex()
        .flex_1()
        .p_2()
        .gap_0p5()
        .rounded(px(RADIUS_CONTROL))
        .bg(solid(BG_BASE))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .child(
            div()
                .text_xs()
                .text_color(solid(TEXT_MUTED))
                .child(label),
        )
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(solid(value_color))
                .child(SharedString::from(value)),
        )
}

fn kv_row(label: &'static str, value: String, value_color: u32) -> impl IntoElement {
    h_flex()
        .w_full()
        .justify_between()
        .gap_2()
        .text_xs()
        .child(div().text_color(solid(TEXT_MUTED)).child(label))
        .child(
            div()
                .font_medium()
                .text_color(solid(value_color))
                .child(SharedString::from(value)),
        )
}

fn render_attribute_badges(flags: NodeFlags) -> impl IntoElement {
    let mut tags: Vec<(&'static str, u32)> = Vec::new();
    if flags.contains(NodeFlags::ACCESS_DENIED) {
        tags.push(("Access Denied", STATUS_ERROR));
    }
    if flags.contains(NodeFlags::IS_SYMLINK) {
        tags.push(("Symlink", STATUS_INFO));
    }
    if flags.contains(NodeFlags::IS_READONLY) {
        tags.push(("Read-Only", TEXT_SECONDARY));
    }
    if flags.contains(NodeFlags::IS_HIDDEN) {
        tags.push(("Hidden", TEXT_SECONDARY));
    }
    if flags.contains(NodeFlags::IS_SYSTEM) {
        tags.push(("System", STATUS_WARNING));
    }
    if flags.contains(NodeFlags::IS_COMPRESSED) {
        tags.push(("Compressed", STATUS_INFO));
    }

    h_flex()
        .pt_1()
        .flex_wrap()
        .gap_1()
        .when(tags.is_empty(), |row| {
            row.child(
                div()
                    .text_xs()
                    .text_color(solid(TEXT_MUTED))
                    .child("Standard attributes"),
            )
        })
        .children(tags.into_iter().map(|(label, color)| {
            div()
                .px_1p5()
                .py_0p5()
                .rounded(px(RADIUS_CONTROL))
                .bg(soft_tint(color, 0.14))
                .border_1()
                .border_color(soft_tint(color, 0.28))
                .text_xs()
                .text_color(solid(color))
                .child(label)
        }))
}

fn render_lock_card(status: &FileLockStatus, cx: &mut Context<Workspace>) -> impl IntoElement {
    v_flex()
        .p_2p5()
        .gap_2()
        .rounded(px(RADIUS_ROW))
        .bg(solid(BG_BASE))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(solid(TEXT_SECONDARY))
                        .child("LOCK STATUS"),
                )
                .child(
                    Button::new("insp-recheck-lock")
                        .with_size(Size::XSmall)
                        .ghost()
                        .icon(IconName::RefreshCw)
                        .tooltip("Recheck File Locks")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.recheck_selected_lock(cx);
                        })),
                ),
        )
        .child(match status {
            FileLockStatus::NotChecked | FileLockStatus::Checking => h_flex()
                .gap_1p5()
                .items_center()
                .text_xs()
                .text_color(solid(TEXT_MUTED))
                .child("Checking active file handles...")
                .into_any_element(),
            FileLockStatus::NotApplicable => div()
                .text_xs()
                .text_color(solid(TEXT_MUTED))
                .child("Lock inspection applies to individual files.")
                .into_any_element(),
            FileLockStatus::Unlocked => h_flex()
                .gap_1p5()
                .items_center()
                .child(
                    Icon::new(LucideIcon::LockOpen)
                        .xsmall()
                        .text_color(solid(STATUS_SUCCESS)),
                )
                .child(
                    div()
                        .text_xs()
                        .font_medium()
                        .text_color(solid(STATUS_SUCCESS))
                        .child("Unlocked — no active process locks"),
                )
                .into_any_element(),
            FileLockStatus::LockedBy(procs) => v_flex()
                .gap_1p5()
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .child(
                            Icon::new(LucideIcon::Lock)
                                .xsmall()
                                .text_color(solid(STATUS_ERROR)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(solid(STATUS_ERROR))
                                .child(SharedString::from(format!(
                                    "Locked by {} process(es)",
                                    procs.len()
                                ))),
                        ),
                )
                .children(procs.iter().map(|p| {
                    h_flex()
                        .w_full()
                        .px_2()
                        .py_1()
                        .justify_between()
                        .gap_2()
                        .rounded(px(RADIUS_CONTROL))
                        .bg(solid(BG_ELEVATED))
                        .border_1()
                        .border_color(solid(BORDER))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_xs()
                                .font_medium()
                                .text_color(solid(TEXT_PRIMARY))
                                .child(SharedString::from(p.name.clone())),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(solid(TEXT_MUTED))
                                .child(SharedString::from(format!("PID {} · {}", p.pid, p.app_type))),
                        )
                }))
                .into_any_element(),
            FileLockStatus::LockedUnknown => h_flex()
                .gap_1p5()
                .items_center()
                .child(
                    Icon::new(LucideIcon::Lock)
                        .xsmall()
                        .text_color(solid(STATUS_WARNING)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(STATUS_WARNING))
                        .child("Locked by an elevated or system process"),
                )
                .into_any_element(),
            FileLockStatus::AccessDenied => h_flex()
                .gap_1p5()
                .items_center()
                .child(
                    Icon::new(LucideIcon::ShieldAlert)
                        .xsmall()
                        .text_color(solid(STATUS_ERROR)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(STATUS_ERROR))
                        .child("Access denied by OS permissions"),
                )
                .into_any_element(),
            FileLockStatus::Error(err) => div()
                .text_xs()
                .text_color(solid(STATUS_ERROR))
                .child(SharedString::from(err.clone()))
                .into_any_element(),
        })
        .hover(|s| s.border_color(solid(BG_HOVER)))
}
