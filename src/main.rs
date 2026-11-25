#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod models;
mod platform;
mod scanner;
mod theme;
mod ui;
mod updater;

use std::borrow::Cow;

use gpui_kit::{
    assets::{Assets, icon_assets},
    *,
};

use crate::models::AppSettings;
use crate::ui::workspace::{
    CancelOrDismiss, FocusSearch, OpenSettings, PickFolder, Quit, Rescan, ToggleInspector,
    ToggleSidebar, Workspace,
};

icon_assets!(
    ExtraIcons,
    [
        CircleStop,
        ExternalLink,
        File,
        Files,
        Folder,
        FolderOpen,
        FolderSearch,
        FolderTree,
        HardDrive,
        Link,
        ListTree,
        Lock,
        LockOpen,
        Monitor,
        ShieldAlert,
    ]
);

struct TexTreeAssets;

impl AssetSource for TexTreeAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if path == "icons/textree.png" {
            return Ok(Some(Cow::Borrowed(include_bytes!("../assets/textree.png"))));
        }
        if path == "icons/textree-64.png" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/textree-64.png"
            ))));
        }
        if let Some(asset) = ExtraIcons.load(path)? {
            return Ok(Some(asset));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        let mut items = Assets.list(path)?;
        items.extend(ExtraIcons.list(path)?);
        Ok(items)
    }
}

fn main() {
    let initial_settings = AppSettings::load();

    gpui_kit::application()
        .with_assets(TexTreeAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::apply_obsidian_theme(initial_settings.accent, cx);

            cx.bind_keys([
                KeyBinding::new("ctrl-k", FocusSearch, Some("TexTreeWorkspace")),
                KeyBinding::new("ctrl-o", PickFolder, Some("TexTreeWorkspace")),
                KeyBinding::new("f5", Rescan, Some("TexTreeWorkspace")),
                KeyBinding::new("escape", CancelOrDismiss, Some("TexTreeWorkspace")),
                KeyBinding::new("ctrl-b", ToggleSidebar, Some("TexTreeWorkspace")),
                KeyBinding::new("ctrl-i", ToggleInspector, Some("TexTreeWorkspace")),
                KeyBinding::new("ctrl-,", OpenSettings, Some("TexTreeWorkspace")),
                KeyBinding::new("ctrl-q", Quit, Some("TexTreeWorkspace")),
            ]);

            let bounds = WindowBounds::centered(size(px(1420.), px(880.)), cx);
            let options = WindowOptions {
                window_bounds: Some(bounds),
                titlebar: Some(TitlebarOptions {
                    title: None,
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(9.), px(9.))),
                }),
                app_owns_titlebar_drag: true,
                ..Default::default()
            };

            let settings_for_window = initial_settings.clone();
            gpui_kit::open_window(options, cx, move |window, cx| {
                window.set_rem_size(px(theme::UI_REM_SIZE));
                cx.new(|cx| Workspace::new(settings_for_window, window, cx))
            })
            .expect("failed to open TexTree window");
        });
}
