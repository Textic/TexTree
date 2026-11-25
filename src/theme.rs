use std::rc::Rc;

use gpui_kit::{
    App, Axis, ElementId, Entity, Hsla, MouseButton,
    base::{ResizeHandleContext, ResizeHandleRenderer, ResizeHandleState},
    component::{
        Theme, ThemeMode,
        input::{Input, InputState},
    },
    div, hsla, img,
    prelude::*,
    px, rgb,
};

use crate::models::{AccentPreset, SizeUnit};

// Obsidian Dark (Neutral Carbon Black) flat surface palette
pub const BG_BASE: u32 = 0x09090b;
pub const BG_SURFACE: u32 = 0x111113;
pub const BG_ELEVATED: u32 = 0x18181b;
pub const BG_HOVER: u32 = 0x202024;
pub const BORDER_SUBTLE: u32 = 0x27272a;
pub const BORDER: u32 = 0x323236;

pub const TEXT_PRIMARY: u32 = 0xf4f4f5;
pub const TEXT_SECONDARY: u32 = 0xa1a1aa;
pub const TEXT_MUTED: u32 = 0x71717a;

// Flat semantic status colors
pub const STATUS_SUCCESS: u32 = 0x34d399;
pub const STATUS_WARNING: u32 = 0xfbbf24;
pub const STATUS_ERROR: u32 = 0xf87171;
pub const STATUS_INFO: u32 = 0x60a5fa;

pub const RADIUS_ROW: f32 = 6.;
pub const RADIUS_CONTROL: f32 = 4.;
pub const UI_REM_SIZE: f32 = 17.0;

#[inline]
pub fn solid(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// Returns a subtle translucent tint derived from a hex color (matching Obsidian Dark soft badges).
pub fn soft_tint(hex: u32, alpha: f32) -> Hsla {
    let base: Hsla = rgb(hex).into();
    hsla(base.h, base.s, base.l, alpha)
}

/// Crisp flat 1px panel cut divider at rest; highlights cleanly when hovered or dragged.
pub fn crisp_resize_handle_appearance(accent: AccentPreset) -> ResizeHandleRenderer {
    let accent_hex = accent.primary_hex();
    Rc::new(move |handle: &ResizeHandleContext, _, _| {
        let axis = handle.axis();
        let (color, thick) = match handle.state() {
            ResizeHandleState::Idle => (solid(BORDER_SUBTLE), false),
            ResizeHandleState::Hovered => (solid(0x3f3f46), true),
            ResizeHandleState::Pressed | ResizeHandleState::Dragging => (solid(accent_hex), true),
        };

        Some(
            div()
                .flex_none()
                .bg(color)
                .map(|d| match (axis, thick) {
                    (Axis::Horizontal, false) => d.w(px(1.)).h_full(),
                    (Axis::Horizontal, true) => d.w(px(2.)).h_full(),
                    (Axis::Vertical, false) => d.h(px(1.)).w_full(),
                    (Axis::Vertical, true) => d.h(px(2.)).w_full(),
                })
                .into_any_element(),
        )
    })
}

/// Applies the Obsidian Dark flat theme with the chosen accent preset (Emerald by default).
pub fn apply_obsidian_theme(accent: AccentPreset, cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |theme| {
        theme.shadow = false;
        theme.focus_ring = false;
        theme.radius = px(RADIUS_CONTROL);
        theme.radius_lg = px(RADIUS_ROW);
        theme.font_size = px(14.);
        theme.mono_font_size = px(12.);

        let bg_base = solid(BG_BASE);
        let bg_surface = solid(BG_SURFACE);
        let bg_elevated = solid(BG_ELEVATED);
        let bg_hover = solid(BG_HOVER);
        let border = solid(BORDER_SUBTLE);
        let fg = solid(TEXT_PRIMARY);
        let fg_muted = solid(TEXT_SECONDARY);

        let primary = solid(accent.primary_hex());
        let primary_hover = solid(accent.hover_hex());
        let primary_active = solid(accent.active_hex());
        let primary_fg = solid(accent.text_on_accent_hex());

        let danger = solid(STATUS_ERROR);
        let danger_hover = solid(0xef4444);
        let danger_active = solid(0xdc2626);

        let success = solid(STATUS_SUCCESS);
        let warning = solid(STATUS_WARNING);
        let info = solid(STATUS_INFO);

        let c = &mut theme.colors;
        c.background = bg_base;
        c.foreground = fg;
        c.border = border;
        c.input = border;
        c.caret = primary;
        c.ring = primary;
        c.selection = soft_tint(accent.primary_hex(), 0.22);
        c.drag_border = primary;
        c.drop_target = bg_hover;

        c.muted = bg_elevated;
        c.muted_foreground = fg_muted;
        c.accent = bg_hover;
        c.accent_foreground = fg;

        c.primary = primary;
        c.primary_hover = primary_hover;
        c.primary_active = primary_active;
        c.primary_foreground = primary_fg;

        c.secondary = bg_elevated;
        c.secondary_hover = bg_hover;
        c.secondary_active = bg_hover;
        c.secondary_foreground = fg;

        c.button = bg_elevated;
        c.button_hover = bg_hover;
        c.button_active = bg_hover;
        c.button_foreground = fg;

        c.button_primary = primary;
        c.button_primary_hover = primary_hover;
        c.button_primary_active = primary_active;
        c.button_primary_foreground = primary_fg;

        c.button_secondary = bg_elevated;
        c.button_secondary_hover = bg_hover;
        c.button_secondary_active = bg_hover;
        c.button_secondary_foreground = fg;

        c.danger = danger;
        c.danger_hover = danger_hover;
        c.danger_active = danger_active;
        c.danger_foreground = solid(BG_BASE);
        c.button_danger = danger;
        c.button_danger_hover = danger_hover;
        c.button_danger_active = danger_active;
        c.button_danger_foreground = solid(BG_BASE);

        c.success = success;
        c.success_hover = solid(0x10b981);
        c.success_active = solid(0x059669);
        c.success_foreground = solid(BG_BASE);
        c.button_success = success;
        c.button_success_hover = solid(0x10b981);
        c.button_success_active = solid(0x059669);
        c.button_success_foreground = solid(BG_BASE);

        c.warning = warning;
        c.warning_hover = solid(0xf59e0b);
        c.warning_active = solid(0xd97706);
        c.warning_foreground = solid(BG_BASE);
        c.button_warning = warning;
        c.button_warning_hover = solid(0xf59e0b);
        c.button_warning_active = solid(0xd97706);
        c.button_warning_foreground = solid(BG_BASE);

        c.info = info;
        c.info_hover = solid(0x3b82f6);
        c.info_active = solid(0x2563eb);
        c.info_foreground = solid(BG_BASE);
        c.button_info = info;
        c.button_info_hover = solid(0x3b82f6);
        c.button_info_active = solid(0x2563eb);
        c.button_info_foreground = solid(BG_BASE);

        c.popover = bg_elevated;
        c.popover_foreground = fg;

        c.sidebar = bg_surface;
        c.sidebar_foreground = fg;
        c.sidebar_border = border;
        c.sidebar_accent = bg_elevated;
        c.sidebar_accent_foreground = fg;
        c.sidebar_primary = primary;
        c.sidebar_primary_foreground = primary_fg;

        c.list = bg_surface;
        c.list_even = bg_surface;
        c.list_head = bg_elevated;
        c.list_hover = bg_elevated;
        c.list_active = bg_elevated;
        c.list_active_border = primary;
        c.list_highlight_foreground = primary;

        c.tab_bar = bg_surface;
        c.tab_bar_segmented = bg_surface;
        c.tab = bg_surface;
        c.tab_foreground = fg_muted;
        c.tab_active = bg_base;
        c.tab_active_foreground = fg;

        c.table = bg_base;
        c.table_even = bg_surface;
        c.table_head = bg_elevated;
        c.table_head_foreground = fg_muted;
        c.table_foot = bg_surface;
        c.table_foot_foreground = fg_muted;
        c.table_hover = bg_elevated;
        c.table_active = bg_elevated;
        c.table_active_border = primary;
        c.table_row_border = border;

        c.title_bar = bg_elevated;
        c.title_bar_border = border;
        c.status_bar = bg_surface;
        c.status_bar_border = border;

        c.scrollbar = bg_base;
        c.scrollbar_thumb = solid(BORDER);
        c.scrollbar_thumb_hover = solid(0x3f3f46);

        c.group_box = bg_surface;
        c.group_box_foreground = fg;
        c.switch = bg_hover;
        c.switch_thumb = fg;
        c.link = primary;
        c.link_hover = primary_hover;
        c.link_active = primary_active;
    });
}

/// Renders the TexTree circular application emblem from embedded PNG assets,
/// selecting the pre-filtered 64px asset at small UI sizes to prevent bilinear aliasing.
pub fn render_app_badge(size_px: f32) -> impl IntoElement {
    let asset_path = if size_px <= 48.0 {
        "icons/textree-64.png"
    } else {
        "icons/textree.png"
    };
    img(asset_path)
        .w(px(size_px))
        .h(px(size_px))
        .flex_shrink_0()
}

/// Wraps a borderless single-line `Input` so clicks anywhere in its vertical height focus the field.
pub fn full_hit_input(
    id: impl Into<ElementId>,
    state: &Entity<InputState>,
    input: Input,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let target = state.clone();
    div()
        .id(id)
        .h_full()
        .flex()
        .items_center()
        .cursor_text()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            if !window.default_prevented() {
                target.update(cx, |s, cx| s.focus(window, cx));
                window.prevent_default();
            }
        })
        .child(input.h_full().py_0())
}

pub fn format_size(bytes: u64, unit: SizeUnit) -> String {
    let (base, suffixes): (f64, [&str; 6]) = match unit {
        SizeUnit::Binary => (1024.0, ["B", "KiB", "MiB", "GiB", "TiB", "PiB"]),
        SizeUnit::Decimal => (1000.0, ["B", "KB", "MB", "GB", "TB", "PB"]),
    };
    if bytes < base as u64 {
        return format!("{bytes} B");
    }
    let mut val = bytes as f64;
    let mut idx = 0usize;
    while val >= base && idx + 1 < suffixes.len() {
        val /= base;
        idx += 1;
    }
    if val >= 100.0 {
        format!("{val:.0} {}", suffixes[idx])
    } else if val >= 10.0 {
        format!("{val:.1} {}", suffixes[idx])
    } else {
        format!("{val:.2} {}", suffixes[idx])
    }
}

pub fn format_count(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, &b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(b as char);
    }
    out
}

pub fn format_timestamp(unix_secs: i64) -> String {
    if unix_secs <= 0 {
        return "—".to_string();
    }
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(unix_secs, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "—".to_string())
}

pub fn format_duration_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        let secs = ms / 1000;
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

pub fn usage_bar_color(fraction: f32, accent: AccentPreset) -> u32 {
    if fraction >= 0.92 {
        STATUS_ERROR
    } else if fraction >= 0.80 {
        STATUS_WARNING
    } else {
        accent.primary_hex()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes_counts_and_durations() {
        assert_eq!(format_size(0, SizeUnit::Binary), "0 B");
        assert_eq!(format_size(1536, SizeUnit::Binary), "1.50 KiB");
        assert_eq!(format_size(1500, SizeUnit::Decimal), "1.50 KB");
        assert_eq!(format_size(1024 * 1024 * 256, SizeUnit::Binary), "256 MiB");
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(1234567), "1,234,567");
        assert_eq!(format_duration_ms(420), "420 ms");
        assert_eq!(format_duration_ms(3400), "3.4 s");
        assert_eq!(format_duration_ms(125_000), "2m 5s");
    }
}
