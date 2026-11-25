//! Flat Obsidian Dark floating context menu for Tree View, Top Files, Treemap,
//! Sidebar drive cards, and Extensions rows.

use gpui_kit::{
    Context, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Window,
    assets::IconName as LucideIcon,
    component::{Icon, IconName, Sizable as _, h_flex, v_flex},
    div, prelude::*, px,
};

use crate::models::{NodeFlags, NodeId, VolumeInfo};
use crate::scanner::ScanTarget;
use crate::theme::{
    BG_ELEVATED, BG_HOVER, BORDER, BORDER_SUBTLE, RADIUS_CONTROL, RADIUS_ROW, STATUS_ERROR,
    TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY, soft_tint, solid,
};
use crate::ui::workspace::{MainTab, Workspace};

#[derive(Debug, Clone)]
pub enum ContextMenuTarget {
    Node { node_id: NodeId, from_treemap: bool },
    Volume(VolumeInfo),
    Extension(String),
}

#[derive(Debug, Clone)]
pub struct ContextMenuState {
    pub position: Point<Pixels>,
    pub target: ContextMenuTarget,
}

enum MenuRow {
    Separator,
    Item {
        id: &'static str,
        label: SharedString,
        shortcut: Option<&'static str>,
        icon: MenuIcon,
        danger: bool,
        action: MenuAction,
    },
}

enum MenuIcon {
    Kit(IconName),
    Lucide(LucideIcon),
}

#[derive(Clone)]
enum MenuAction {
    RevealNode(NodeId),
    OpenTerminalNode(NodeId),
    CopyNodePath(NodeId),
    ShowNodeProperties(NodeId),
    ToggleExpandNode(NodeId),
    FocusTreemap(NodeId),
    ZoomOutTreemap,
    ScanFolderNode(NodeId),
    FilterByExtension(String),
    CopyText(String),
    RecheckLock(NodeId),
    TrashNode(NodeId),
    PermanentDeleteNode(NodeId),
    RescanVolume(VolumeInfo),
    RevealPath(std::path::PathBuf),
    OpenTerminalPath(std::path::PathBuf),
    ShowPathProperties(std::path::PathBuf),
}

pub fn render_context_menu(
    ws: &Workspace,
    menu: &ContextMenuState,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let rows = build_menu_rows(ws, &menu.target);
    if rows.is_empty() {
        return div().into_any_element();
    }

    const MENU_W: f32 = 256.0;
    const ITEM_H: f32 = 28.0;
    const SEP_H: f32 = 9.0;
    let est_h: f32 = 10.0
        + rows
            .iter()
            .map(|r| match r {
                MenuRow::Separator => SEP_H,
                MenuRow::Item { .. } => ITEM_H,
            })
            .sum::<f32>();

    let vp = window.viewport_size();
    let max_x = (f32::from(vp.width) - MENU_W - 8.0).max(8.0);
    let max_y = (f32::from(vp.height) - est_h - 8.0).max(8.0);
    let left = f32::from(menu.position.x).clamp(8.0, max_x);
    let top = f32::from(menu.position.y).clamp(8.0, max_y);

    div()
        .id("ctx-menu-overlay")
        .absolute()
        .inset_0()
        .child(
            div()
                .id("ctx-menu-backdrop")
                .absolute()
                .inset_0()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.dismiss_context_menu(cx);
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.dismiss_context_menu(cx);
                    }),
                ),
        )
        .child(
            v_flex()
                .id("ctx-menu-card")
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(MENU_W))
                .py_1()
                .px_1()
                .bg(solid(BG_ELEVATED))
                .border_1()
                .border_color(solid(BORDER))
                .rounded(px(RADIUS_ROW))
                .occlude()
                .children(rows.into_iter().enumerate().map(|(ix, row)| match row {
                    MenuRow::Separator => div()
                        .id(("ctx-sep", ix))
                        .w_full()
                        .my_1()
                        .h(px(1.))
                        .bg(solid(BORDER_SUBTLE))
                        .into_any_element(),
                    MenuRow::Item {
                        id,
                        label,
                        shortcut,
                        icon,
                        danger,
                        action,
                    } => {
                        let icon_el = match icon {
                            MenuIcon::Kit(i) => Icon::new(i).xsmall(),
                            MenuIcon::Lucide(i) => Icon::new(i).xsmall(),
                        };
                        let text_col = if danger { STATUS_ERROR } else { TEXT_PRIMARY };
                        let icon_col = if danger { STATUS_ERROR } else { TEXT_SECONDARY };

                        h_flex()
                            .id((id, ix))
                            .w_full()
                            .h(px(ITEM_H))
                            .px_2()
                            .gap_2()
                            .items_center()
                            .justify_between()
                            .rounded(px(RADIUS_CONTROL))
                            .cursor_pointer()
                            .hover(move |s| {
                                if danger {
                                    s.bg(soft_tint(STATUS_ERROR, 0.15))
                                } else {
                                    s.bg(solid(BG_HOVER))
                                }
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    execute_menu_action(this, action.clone(), cx);
                                }),
                            )
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .flex_1()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_color(solid(icon_col))
                                            .child(icon_el),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .truncate()
                                            .text_xs()
                                            .text_color(solid(text_col))
                                            .child(label),
                                    ),
                            )
                            .when_some(shortcut, |r, sc| {
                                r.child(
                                    div()
                                        .flex_shrink_0()
                                        .text_xs()
                                        .text_color(solid(TEXT_MUTED))
                                        .child(sc),
                                )
                            })
                            .into_any_element()
                    }
                })),
        )
        .into_any_element()
}

fn build_menu_rows(ws: &Workspace, target: &ContextMenuTarget) -> Vec<MenuRow> {
    let mut rows = Vec::new();

    match target {
        ContextMenuTarget::Node {
            node_id,
            from_treemap,
        } => {
            let Some(tree) = &ws.tree else {
                return rows;
            };
            let Some(node) = tree.get(*node_id) else {
                return rows;
            };
            let is_container = node.is_container();
            let is_virtual = node.flags.contains(NodeFlags::IS_VIRTUAL);
            let is_volume = node.flags.contains(NodeFlags::IS_VOLUME);
            let has_fs_path = tree.full_path(*node_id).is_some();
            let can_delete = has_fs_path && *node_id != tree.root && !is_volume && !is_virtual;
            let can_zoom_out = ws
                .treemap_root
                .is_some_and(|r| r != tree.root && tree.get(r).and_then(|n| n.parent).is_some());

            if *from_treemap {
                if is_container {
                    rows.push(MenuRow::Item {
                        id: "ctx-tm-zoom-in",
                        label: "Zoom Into Folder".into(),
                        shortcut: Some("Dbl-Click"),
                        icon: MenuIcon::Lucide(LucideIcon::ZoomIn),
                        danger: false,
                        action: MenuAction::FocusTreemap(*node_id),
                    });
                }
                if can_zoom_out {
                    rows.push(MenuRow::Item {
                        id: "ctx-tm-zoom-out",
                        label: "Zoom Out One Level".into(),
                        shortcut: Some("Backspace"),
                        icon: MenuIcon::Lucide(LucideIcon::ZoomOut),
                        danger: false,
                        action: MenuAction::ZoomOutTreemap,
                    });
                }
                if !rows.is_empty() {
                    rows.push(MenuRow::Separator);
                }
            }

            if has_fs_path {
                rows.push(MenuRow::Item {
                    id: "ctx-reveal",
                    label: "Reveal in File Explorer".into(),
                    shortcut: None,
                    icon: MenuIcon::Lucide(LucideIcon::ExternalLink),
                    danger: false,
                    action: MenuAction::RevealNode(*node_id),
                });
                rows.push(MenuRow::Item {
                    id: "ctx-terminal",
                    label: "Open Terminal Here".into(),
                    shortcut: None,
                    icon: MenuIcon::Lucide(LucideIcon::Terminal),
                    danger: false,
                    action: MenuAction::OpenTerminalNode(*node_id),
                });
                rows.push(MenuRow::Item {
                    id: "ctx-copy-path",
                    label: "Copy Path".into(),
                    shortcut: None,
                    icon: MenuIcon::Kit(IconName::Copy),
                    danger: false,
                    action: MenuAction::CopyNodePath(*node_id),
                });
                if cfg!(target_os = "windows") {
                    rows.push(MenuRow::Item {
                        id: "ctx-properties",
                        label: "Properties".into(),
                        shortcut: None,
                        icon: MenuIcon::Kit(IconName::Info),
                        danger: false,
                        action: MenuAction::ShowNodeProperties(*node_id),
                    });
                }
            }

            let mut nav_rows = Vec::new();
            if is_container {
                if !*from_treemap && ws.active_tab == MainTab::Tree && !node.children.is_empty() {
                    let expanded = ws.expanded.contains(node_id);
                    nav_rows.push(MenuRow::Item {
                        id: "ctx-toggle-expand",
                        label: if expanded {
                            "Collapse Folder".into()
                        } else {
                            "Expand Folder".into()
                        },
                        shortcut: None,
                        icon: MenuIcon::Kit(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        }),
                        danger: false,
                        action: MenuAction::ToggleExpandNode(*node_id),
                    });
                }
                if !*from_treemap {
                    nav_rows.push(MenuRow::Item {
                        id: "ctx-focus-treemap",
                        label: "Focus in Treemap".into(),
                        shortcut: None,
                        icon: MenuIcon::Lucide(LucideIcon::ZoomIn),
                        danger: false,
                        action: MenuAction::FocusTreemap(*node_id),
                    });
                }
                if has_fs_path && !is_virtual {
                    nav_rows.push(MenuRow::Item {
                        id: "ctx-scan-here",
                        label: "Scan Only This Folder".into(),
                        shortcut: None,
                        icon: MenuIcon::Lucide(LucideIcon::FolderSearch),
                        danger: false,
                        action: MenuAction::ScanFolderNode(*node_id),
                    });
                }
            } else {
                let ext = tree.extension_of(*node_id);
                if !ext.is_empty() {
                    let dot_ext = format!(".{ext}");
                    nav_rows.push(MenuRow::Item {
                        id: "ctx-filter-ext",
                        label: SharedString::from(format!("Filter by {dot_ext}")),
                        shortcut: None,
                        icon: MenuIcon::Lucide(LucideIcon::FolderTree),
                        danger: false,
                        action: MenuAction::FilterByExtension(dot_ext),
                    });
                }
                if has_fs_path {
                    nav_rows.push(MenuRow::Item {
                        id: "ctx-recheck-lock",
                        label: "Check File Locks".into(),
                        shortcut: None,
                        icon: MenuIcon::Lucide(LucideIcon::Lock),
                        danger: false,
                        action: MenuAction::RecheckLock(*node_id),
                    });
                }
            }

            if !nav_rows.is_empty() {
                if !rows.is_empty() {
                    rows.push(MenuRow::Separator);
                }
                rows.extend(nav_rows);
            }

            if can_delete {
                if !rows.is_empty() {
                    rows.push(MenuRow::Separator);
                }
                rows.push(MenuRow::Item {
                    id: "ctx-perm-delete",
                    label: "Delete Permanently".into(),
                    shortcut: Some("Del"),
                    icon: MenuIcon::Lucide(LucideIcon::ShieldAlert),
                    danger: true,
                    action: MenuAction::PermanentDeleteNode(*node_id),
                });
                rows.push(MenuRow::Item {
                    id: "ctx-trash",
                    label: "Move to Recycle Bin".into(),
                    shortcut: Some("Shift+Del"),
                    icon: MenuIcon::Kit(IconName::Delete),
                    danger: true,
                    action: MenuAction::TrashNode(*node_id),
                });
            }
        }
        ContextMenuTarget::Volume(vol) => {
            rows.push(MenuRow::Item {
                id: "ctx-vol-rescan",
                label: SharedString::from(format!("Rescan {} (Fresh)", vol.short_name)),
                shortcut: None,
                icon: MenuIcon::Kit(IconName::RefreshCw),
                danger: false,
                action: MenuAction::RescanVolume(vol.clone()),
            });
            rows.push(MenuRow::Separator);
            rows.push(MenuRow::Item {
                id: "ctx-vol-reveal",
                label: "Open in File Explorer".into(),
                shortcut: None,
                icon: MenuIcon::Lucide(LucideIcon::ExternalLink),
                danger: false,
                action: MenuAction::RevealPath(vol.root_path.clone()),
            });
            rows.push(MenuRow::Item {
                id: "ctx-vol-terminal",
                label: "Open Terminal Here".into(),
                shortcut: None,
                icon: MenuIcon::Lucide(LucideIcon::Terminal),
                danger: false,
                action: MenuAction::OpenTerminalPath(vol.root_path.clone()),
            });
            rows.push(MenuRow::Item {
                id: "ctx-vol-copy",
                label: "Copy Drive Path".into(),
                shortcut: None,
                icon: MenuIcon::Kit(IconName::Copy),
                danger: false,
                action: MenuAction::CopyText(vol.root_path.to_string_lossy().into_owned()),
            });
            if cfg!(target_os = "windows") {
                rows.push(MenuRow::Item {
                    id: "ctx-vol-props",
                    label: "Properties".into(),
                    shortcut: None,
                    icon: MenuIcon::Kit(IconName::Info),
                    danger: false,
                    action: MenuAction::ShowPathProperties(vol.root_path.clone()),
                });
            }
        }
        ContextMenuTarget::Extension(ext) => {
            rows.push(MenuRow::Item {
                id: "ctx-ext-show",
                label: SharedString::from(format!("Show Files with {ext}")),
                shortcut: None,
                icon: MenuIcon::Lucide(LucideIcon::Files),
                danger: false,
                action: MenuAction::FilterByExtension(ext.clone()),
            });
            rows.push(MenuRow::Item {
                id: "ctx-ext-copy",
                label: "Copy Extension".into(),
                shortcut: None,
                icon: MenuIcon::Kit(IconName::Copy),
                danger: false,
                action: MenuAction::CopyText(ext.clone()),
            });
        }
    }

    rows
}

fn execute_menu_action(ws: &mut Workspace, action: MenuAction, cx: &mut Context<Workspace>) {
    ws.dismiss_context_menu(cx);
    match action {
        MenuAction::RevealNode(id) => {
            ws.select_node(id, cx);
            ws.reveal_selected_in_explorer(cx);
        }
        MenuAction::OpenTerminalNode(id) => {
            ws.select_node(id, cx);
            ws.open_terminal_for_selected(cx);
        }
        MenuAction::CopyNodePath(id) => {
            ws.select_node(id, cx);
            ws.copy_selected_path(cx);
        }
        MenuAction::ShowNodeProperties(id) => {
            ws.select_node(id, cx);
            ws.show_properties_for_selected(cx);
        }
        MenuAction::ToggleExpandNode(id) => {
            ws.toggle_expand(id, cx);
        }
        MenuAction::FocusTreemap(id) => {
            ws.set_treemap_root(id, cx);
        }
        MenuAction::ZoomOutTreemap => {
            ws.treemap_zoom_up(cx);
        }
        MenuAction::ScanFolderNode(id) => {
            ws.select_node(id, cx);
            ws.scan_selected_folder(cx);
        }
        MenuAction::FilterByExtension(ext) => {
            ws.filter_by_extension(ext, cx);
        }
        MenuAction::CopyText(text) => {
            ws.copy_text_to_clipboard(text, cx);
        }
        MenuAction::RecheckLock(id) => {
            ws.select_node(id, cx);
            ws.recheck_selected_lock(cx);
        }
        MenuAction::TrashNode(id) => {
            ws.select_node(id, cx);
            ws.request_trash_selected(cx);
        }
        MenuAction::PermanentDeleteNode(id) => {
            ws.select_node(id, cx);
            ws.request_permanent_delete_selected(cx);
        }
        MenuAction::RescanVolume(vol) => {
            ws.start_scan(ScanTarget::Volume(vol), cx);
        }
        MenuAction::RevealPath(path) => {
            ws.reveal_path_in_explorer(&path, cx);
        }
        MenuAction::OpenTerminalPath(path) => {
            ws.open_terminal_for_path(&path, cx);
        }
        MenuAction::ShowPathProperties(path) => {
            ws.show_properties_for_path(&path, cx);
        }
    }
}
