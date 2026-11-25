use gpui_kit::{
    Context, SharedString,
    component::{
        Disableable as _, Icon, IconName, Sizable as _, Size, StyledExt as _,
        button::{Button, ButtonVariants as _},
        checkbox::Checkbox,
        h_flex,
        scroll::ScrollableElement as _,
        v_flex,
    },
    div, hsla, prelude::*, px, relative,
};

use crate::models::{AccentPreset, NodeId, SizeUnit};
use crate::theme::{
    BG_BASE, BG_ELEVATED, BG_HOVER, BG_SURFACE, BORDER, BORDER_SUBTLE, RADIUS_CONTROL, RADIUS_ROW,
    STATUS_ERROR, STATUS_SUCCESS, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY, format_count,
    format_size, render_app_badge, soft_tint, solid,
};
use crate::ui::workspace::Workspace;
use crate::updater::{CURRENT_VERSION, REPO_URL, UpdateState, format_download_bytes};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsCategory {
    #[default]
    Appearance,
    Scanning,
    UpdatesAbout,
}

impl SettingsCategory {
    pub const ALL: [Self; 3] = [Self::Appearance, Self::Scanning, Self::UpdatesAbout];

    pub fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Scanning => "Scanning & Safety",
            Self::UpdatesAbout => "Updates & About",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Appearance => "Choose your Obsidian Dark accent color and unit display.",
            Self::Scanning => "Hidden files visibility, trash confirmation, and startup checks.",
            Self::UpdatesAbout => "Application version, GitHub repository, and automatic updates.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingConfirm {
    TrashItem {
        node_id: NodeId,
        name: String,
        path: std::path::PathBuf,
        size: u64,
        is_dir: bool,
        item_count: u64,
    },
}

fn modal_backdrop(id: &'static str) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .absolute()
        .inset_0()
        .bg(hsla(0., 0., 0., 0.65))
        .occlude()
}

pub fn render_confirm_modal(
    action: &PendingConfirm,
    unit: SizeUnit,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let PendingConfirm::TrashItem {
        name,
        path,
        size,
        is_dir,
        item_count,
        ..
    } = action;

    let summary = if *is_dir {
        format!(
            "Move folder \"{name}\" ({}, {} items) to the Recycle Bin?",
            format_size(*size, unit),
            format_count(*item_count)
        )
    } else {
        format!(
            "Move file \"{name}\" ({}) to the Recycle Bin?",
            format_size(*size, unit)
        )
    };

    div()
        .id("confirm-dialog-root")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            modal_backdrop("confirm-dialog-backdrop").on_click(cx.listener(|this, _, _, cx| {
                this.dismiss_confirm(cx);
            })),
        )
        .child(
            v_flex()
                .id("confirm-dialog-card")
                .relative()
                .occlude()
                .w(px(460.))
                .max_w(relative(0.90))
                .bg(solid(BG_SURFACE))
                .border_1()
                .border_color(solid(BORDER))
                .rounded(px(RADIUS_ROW))
                .overflow_hidden()
                .child(
                    h_flex()
                        .w_full()
                        .h(px(42.))
                        .px_4()
                        .items_center()
                        .justify_between()
                        .bg(solid(BG_BASE))
                        .border_b_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .text_color(solid(TEXT_PRIMARY))
                                .child("Move to Recycle Bin"),
                        )
                        .child(
                            Button::new("confirm-dialog-close")
                                .with_size(Size::XSmall)
                                .ghost()
                                .icon(IconName::Close)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.dismiss_confirm(cx);
                                })),
                        ),
                )
                .child(
                    v_flex()
                        .p_4()
                        .gap_3()
                        .child(
                            div()
                                .text_xs()
                                .font_medium()
                                .text_color(solid(TEXT_PRIMARY))
                                .child(SharedString::from(summary)),
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
                                .child(SharedString::from(path.to_string_lossy().into_owned())),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(solid(TEXT_MUTED))
                                .child("Items moved to the Recycle Bin can be restored from your operating system."),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .pt_1()
                                .justify_end()
                                .gap_2()
                                .child(
                                    Button::new("confirm-dialog-cancel")
                                        .with_size(Size::Small)
                                        .secondary()
                                        .label("Cancel")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.dismiss_confirm(cx);
                                        })),
                                )
                                .child(
                                    Button::new("confirm-dialog-accept")
                                        .with_size(Size::Small)
                                        .danger()
                                        .icon(IconName::Delete)
                                        .label("Move to Trash")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.accept_confirm(cx);
                                        })),
                                ),
                        ),
                ),
        )
}

pub fn render_settings_modal(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let active_cat = ws.settings_category;
    let accent = ws.settings.accent;
    let update_available = ws.update_state.has_pending_update();

    div()
        .id("settings-modal-root")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            modal_backdrop("settings-modal-backdrop").on_click(cx.listener(|this, _, _, cx| {
                this.show_settings = false;
                cx.notify();
            })),
        )
        .child(
            v_flex()
                .id("settings-modal-card")
                .relative()
                .occlude()
                .w(px(720.))
                .h(px(510.))
                .max_w(relative(0.92))
                .max_h(relative(0.88))
                .bg(solid(BG_SURFACE))
                .border_1()
                .border_color(solid(BORDER))
                .rounded(px(RADIUS_ROW))
                .overflow_hidden()
                .child(
                    h_flex()
                        .w_full()
                        .h(px(44.))
                        .px_4()
                        .items_center()
                        .justify_between()
                        .bg(solid(BG_BASE))
                        .border_b_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    Icon::new(IconName::Settings)
                                        .small()
                                        .text_color(solid(accent.primary_hex())),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_semibold()
                                        .text_color(solid(TEXT_PRIMARY))
                                        .child("Settings"),
                                ),
                        )
                        .child(
                            Button::new("settings-modal-close")
                                .with_size(Size::XSmall)
                                .ghost()
                                .icon(IconName::Close)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_settings = false;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        // Left categories rail
                        .child(
                            v_flex()
                                .w(px(190.))
                                .h_full()
                                .flex_shrink_0()
                                .p_2p5()
                                .gap_1()
                                .bg(solid(BG_BASE))
                                .border_r_1()
                                .border_color(solid(BORDER_SUBTLE))
                                .children(SettingsCategory::ALL.into_iter().enumerate().map(
                                    |(ix, cat)| {
                                        let is_active = cat == active_cat;
                                        let show_dot = cat == SettingsCategory::UpdatesAbout
                                            && update_available;
                                        h_flex()
                                            .id(("settings-cat", ix))
                                            .w_full()
                                            .px_2p5()
                                            .py_1p5()
                                            .rounded(px(RADIUS_CONTROL))
                                            .items_center()
                                            .justify_between()
                                            .cursor_pointer()
                                            .bg(if is_active {
                                                solid(BG_ELEVATED)
                                            } else {
                                                solid(BG_BASE)
                                            })
                                            .text_xs()
                                            .font_medium()
                                            .text_color(if is_active {
                                                solid(TEXT_PRIMARY)
                                            } else {
                                                solid(TEXT_SECONDARY)
                                            })
                                            .hover(|s| s.bg(solid(BG_ELEVATED)))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.settings_category = cat;
                                                cx.notify();
                                            }))
                                            .child(cat.label())
                                            .when(show_dot, |row| {
                                                row.child(
                                                    div()
                                                        .w(px(6.))
                                                        .h(px(6.))
                                                        .rounded(px(999.))
                                                        .bg(solid(accent.primary_hex())),
                                                )
                                            })
                                    },
                                )),
                        )
                        // Right content pane
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .overflow_y_scrollbar()
                                .p_4()
                                .gap_3()
                                .child(
                                    v_flex()
                                        .gap_0p5()
                                        .pb_2()
                                        .border_b_1()
                                        .border_color(solid(BORDER_SUBTLE))
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_semibold()
                                                .text_color(solid(TEXT_PRIMARY))
                                                .child(active_cat.label()),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(solid(TEXT_MUTED))
                                                .child(active_cat.subtitle()),
                                        ),
                                )
                                .child(match active_cat {
                                    SettingsCategory::Appearance => {
                                        render_appearance_pane(ws, cx).into_any_element()
                                    }
                                    SettingsCategory::Scanning => {
                                        render_scanning_pane(ws, cx).into_any_element()
                                    }
                                    SettingsCategory::UpdatesAbout => {
                                        render_updates_about_pane(ws, cx).into_any_element()
                                    }
                                }),
                        ),
                ),
        )
}

fn section_card(title: &'static str) -> gpui_kit::Div {
    v_flex()
        .w_full()
        .p_3()
        .gap_2p5()
        .bg(solid(BG_ELEVATED))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .rounded(px(RADIUS_ROW))
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(solid(TEXT_SECONDARY))
                .child(title),
        )
}

fn render_appearance_pane(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let active_accent = ws.settings.accent;
    let active_unit = ws.settings.size_unit;

    v_flex()
        .w_full()
        .gap_3()
        .child(
            section_card("Obsidian Dark Accent Preset").children(
                AccentPreset::ALL.into_iter().enumerate().map(|(ix, preset)| {
                    let is_selected = preset == active_accent;
                    h_flex()
                        .id(("accent-preset-opt", ix))
                        .w_full()
                        .p_2p5()
                        .gap_3()
                        .items_center()
                        .justify_between()
                        .rounded(px(RADIUS_CONTROL))
                        .cursor_pointer()
                        .bg(if is_selected {
                            soft_tint(preset.primary_hex(), 0.12)
                        } else {
                            solid(BG_BASE)
                        })
                        .border_1()
                        .border_color(if is_selected {
                            solid(preset.primary_hex())
                        } else {
                            solid(BORDER_SUBTLE)
                        })
                        .hover(|s| s.bg(solid(BG_HOVER)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_accent_preset(preset, cx);
                        }))
                        .child(
                            h_flex()
                                .min_w_0()
                                .flex_1()
                                .items_center()
                                .gap_2p5()
                                .child(
                                    div()
                                        .w(px(18.))
                                        .h(px(18.))
                                        .flex_shrink_0()
                                        .rounded(px(RADIUS_CONTROL))
                                        .bg(solid(preset.primary_hex()))
                                        .border_1()
                                        .border_color(solid(BORDER)),
                                )
                                .child(
                                    v_flex()
                                        .min_w_0()
                                        .flex_1()
                                        .gap_0p5()
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_semibold()
                                                        .text_color(solid(TEXT_PRIMARY))
                                                        .child(preset.label()),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(solid(TEXT_MUTED))
                                                        .child(preset.hex_tag()),
                                                )
                                                .when(preset == AccentPreset::Emerald, |row| {
                                                    row.child(
                                                        div()
                                                            .px_1p5()
                                                            .rounded(px(3.))
                                                            .bg(solid(BG_SURFACE))
                                                            .border_1()
                                                            .border_color(solid(BORDER_SUBTLE))
                                                            .text_xs()
                                                            .text_color(solid(TEXT_MUTED))
                                                            .child("Default"),
                                                    )
                                                }),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(solid(TEXT_MUTED))
                                                .child(preset.description()),
                                        ),
                                ),
                        )
                        .when(is_selected, |row| {
                            row.child(
                                Icon::new(IconName::Check)
                                    .small()
                                    .text_color(solid(preset.primary_hex())),
                            )
                        })
                }),
            ),
        )
        .child(
            section_card("Size Display Unit").child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .children([SizeUnit::Binary, SizeUnit::Decimal].into_iter().enumerate().map(
                        |(ix, unit)| {
                            let is_selected = unit == active_unit;
                            div()
                                .id(("size-unit-opt", ix))
                                .flex_1()
                                .p_2()
                                .rounded(px(RADIUS_CONTROL))
                                .cursor_pointer()
                                .bg(if is_selected {
                                    soft_tint(active_accent.primary_hex(), 0.12)
                                } else {
                                    solid(BG_BASE)
                                })
                                .border_1()
                                .border_color(if is_selected {
                                    solid(active_accent.primary_hex())
                                } else {
                                    solid(BORDER_SUBTLE)
                                })
                                .text_xs()
                                .font_medium()
                                .text_color(if is_selected {
                                    solid(TEXT_PRIMARY)
                                } else {
                                    solid(TEXT_SECONDARY)
                                })
                                .hover(|s| s.bg(solid(BG_HOVER)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_size_unit(unit, cx);
                                }))
                                .child(unit.label())
                        },
                    )),
            ),
        )
}

fn render_scanning_pane(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let show_hidden = ws.settings.show_hidden_files;
    let confirm_trash = ws.settings.confirm_before_trash;
    let auto_scan = ws.settings.auto_scan_on_launch;
    let show_treemap = ws.settings.show_treemap;
    let prefer_mft = ws.settings.prefer_ntfs_mft;
    let auto_check = ws.settings.auto_check_updates;
    let is_elevated = ws.is_elevated;

    v_flex()
        .w_full()
        .gap_3()
        .child(
            section_card("Behavior & Safety")
                .child(
                    Checkbox::new("set-auto-scan")
                        .checked(auto_scan)
                        .label("Automatically scan Entire PC on launch")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_auto_scan_on_launch(cx);
                        })),
                )
                .child(
                    Checkbox::new("set-show-treemap")
                        .checked(show_treemap)
                        .label("Show interactive visual Treemap panel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_treemap(cx);
                        })),
                )
                .child(
                    Checkbox::new("set-prefer-mft")
                        .checked(prefer_mft)
                        .label("Use fast direct NTFS MFT scanning when running as Administrator")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_prefer_ntfs_mft(cx);
                        })),
                )
                .child(
                    Checkbox::new("set-show-hidden")
                        .checked(show_hidden)
                        .label("Show hidden and system items in scan views")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_show_hidden(cx);
                        })),
                )
                .child(
                    Checkbox::new("set-confirm-trash")
                        .checked(confirm_trash)
                        .label("Ask for confirmation before moving items to the Recycle Bin")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_confirm_before_trash(cx);
                        })),
                )
                .child(
                    Checkbox::new("set-auto-updates")
                        .checked(auto_check)
                        .label("Automatically check GitHub Releases for updates on launch")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toggle_auto_check_updates(cx);
                        })),
                ),
        )
        .when(cfg!(target_os = "windows"), |col| {
            col.child(
                section_card("NTFS MFT Privileges").child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            v_flex()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(if is_elevated {
                                            solid(STATUS_SUCCESS)
                                        } else {
                                            solid(TEXT_PRIMARY)
                                        })
                                        .child(if is_elevated {
                                            "Running as Administrator — Fast NTFS MFT active"
                                        } else {
                                            "Running as Standard User — Parallel Win32 mode"
                                        }),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(solid(TEXT_MUTED))
                                        .child("Reading raw NTFS Master File Tables requires elevated Windows privileges."),
                                ),
                        )
                        .when(!is_elevated, |row| {
                            row.child(
                                Button::new("set-relaunch-admin")
                                    .with_size(Size::XSmall)
                                    .primary()
                                    .label("Relaunch as Admin")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.relaunch_as_admin(cx);
                                    })),
                            )
                        }),
                ),
            )
        })
}

fn render_updates_about_pane(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    v_flex()
        .w_full()
        .gap_3()
        .child(
            section_card("About TexTree").child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        h_flex()
                            .gap_2p5()
                            .items_center()
                            .child(render_app_badge(36.))
                            .child(
                                v_flex()
                                    .gap_0p5()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_semibold()
                                                    .text_color(solid(TEXT_PRIMARY))
                                                    .child("TexTree"),
                                            )
                                            .child(
                                                div()
                                                    .px_1p5()
                                                    .py_0p5()
                                                    .rounded(px(3.))
                                                    .bg(solid(BG_BASE))
                                                    .border_1()
                                                    .border_color(solid(BORDER_SUBTLE))
                                                    .text_xs()
                                                    .text_color(solid(TEXT_MUTED))
                                                    .child(SharedString::from(format!(
                                                        "v{CURRENT_VERSION}"
                                                    ))),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(solid(TEXT_MUTED))
                                            .child("Minimalist flat-dark disk space analyzer built with GPUI-Kit."),
                                    ),
                            ),
                    )
                    .child(
                        Button::new("info-open-github")
                            .with_size(Size::XSmall)
                            .secondary()
                            .label("GitHub")
                            .tooltip(REPO_URL)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_github_repo(cx);
                            })),
                    ),
            ),
        )
        .child(render_updates_card(ws, cx))
}

fn render_updates_card(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let state = &ws.update_state;
    let accent = ws.settings.accent;
    let is_checking = matches!(state, UpdateState::Checking);
    let is_busy = state.is_busy();
    let can_retry = ws
        .last_update_info
        .as_ref()
        .is_some_and(|info| info.supports_in_app_install());

    v_flex()
        .w_full()
        .p_3()
        .gap_2p5()
        .bg(solid(BG_ELEVATED))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .rounded(px(RADIUS_ROW))
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(solid(TEXT_SECONDARY))
                        .child("Updates"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child(SharedString::from(format!("Current: v{CURRENT_VERSION}"))),
                ),
        )
        .child(match state {
            UpdateState::Idle => div()
                .text_xs()
                .text_color(solid(TEXT_MUTED))
                .child("Check GitHub Releases for new versions of TexTree.")
                .into_any_element(),
            UpdateState::Checking => div()
                .text_xs()
                .text_color(solid(TEXT_MUTED))
                .child("Checking for updates...")
                .into_any_element(),
            UpdateState::UpToDate => div()
                .text_xs()
                .text_color(solid(STATUS_SUCCESS))
                .child("You are on the latest version.")
                .into_any_element(),
            UpdateState::Available(info) => {
                let in_app = info.supports_in_app_install();
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(solid(STATUS_SUCCESS))
                            .child(SharedString::from(if info.published_at.is_empty() {
                                format!("Version {} is available", info.tag_name)
                            } else {
                                format!(
                                    "Version {} available ({})",
                                    info.tag_name, info.published_at
                                )
                            })),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .flex_wrap()
                            .child(
                                Button::new("set-update-download")
                                    .with_size(Size::XSmall)
                                    .primary()
                                    .icon(IconName::ArrowDown)
                                    .label(SharedString::from(if in_app {
                                        format!("Download & Install {}", info.tag_name)
                                    } else {
                                        format!("Download {}", info.tag_name)
                                    }))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if in_app {
                                            this.start_update_install(cx);
                                        } else {
                                            this.open_update_download(cx);
                                        }
                                    })),
                            )
                            .when(in_app, |row| {
                                row.child(
                                    Button::new("set-update-browser")
                                        .with_size(Size::XSmall)
                                        .secondary()
                                        .label("Browser")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.open_update_download(cx);
                                        })),
                                )
                            })
                            .child(
                                Button::new("set-update-notes")
                                    .with_size(Size::XSmall)
                                    .ghost()
                                    .label("Release Notes")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.open_release_notes(cx);
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            UpdateState::Downloading {
                info,
                downloaded_bytes,
                total_bytes,
            } => {
                let (status_text, fraction) = match total_bytes {
                    Some(total) if *total > 0 => {
                        let frac = (*downloaded_bytes as f32 / *total as f32).clamp(0.0, 1.0);
                        let pct = (frac * 100.0).round() as u64;
                        (
                            format!(
                                "Downloading {} — {} / {} ({pct}%)",
                                info.tag_name,
                                format_download_bytes(*downloaded_bytes),
                                format_download_bytes(*total),
                            ),
                            frac,
                        )
                    }
                    _ => (
                        format!(
                            "Downloading {} — {}...",
                            info.tag_name,
                            format_download_bytes(*downloaded_bytes),
                        ),
                        0.25,
                    ),
                };

                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(solid(TEXT_PRIMARY))
                            .child(SharedString::from(status_text)),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(6.))
                            .bg(solid(BG_BASE))
                            .border_1()
                            .border_color(solid(BORDER_SUBTLE))
                            .rounded(px(3.))
                            .overflow_hidden()
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(fraction))
                                    .bg(solid(accent.primary_hex())),
                            ),
                    )
                    .into_any_element()
            }
            UpdateState::Installing(info) => v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(solid(STATUS_SUCCESS))
                        .child(SharedString::from(format!("Installing {}...", info.tag_name))),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Closing TexTree, applying update, and restarting automatically..."),
                )
                .into_any_element(),
            UpdateState::Failed(err) => v_flex()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(STATUS_ERROR))
                        .child(SharedString::from(err.clone())),
                )
                .child(
                    h_flex()
                        .gap_1p5()
                        .when(can_retry, |row| {
                            row.child(
                                Button::new("set-update-retry")
                                    .with_size(Size::XSmall)
                                    .primary()
                                    .icon(IconName::RefreshCw)
                                    .label("Retry")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.start_update_install(cx);
                                    })),
                            )
                        })
                        .child(
                            Button::new("set-update-fallback")
                                .with_size(Size::XSmall)
                                .ghost()
                                .label("Open Releases")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.open_update_download(cx);
                                })),
                        ),
                )
                .into_any_element(),
        })
        .when(!matches!(state, UpdateState::Installing(_)), |card| {
            card.child(
                h_flex().justify_end().child(
                    Button::new("set-check-updates")
                        .with_size(Size::XSmall)
                        .secondary()
                        .disabled(is_busy)
                        .icon(IconName::RefreshCw)
                        .label(if is_checking {
                            "Checking..."
                        } else {
                            "Check for Updates"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.check_for_updates(cx);
                        })),
                ),
            )
        })
}
