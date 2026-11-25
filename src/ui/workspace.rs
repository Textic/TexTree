use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures_channel::mpsc::unbounded;
use futures_util::StreamExt as _;
use gpui_kit::{
    ClipboardItem, Context, Entity, FocusHandle, Focusable, KeyDownEvent, PathPromptOptions,
    Pixels, Point, ScrollStrategy, SharedString, Subscription, UniformListScrollHandle, Window,
    WindowControlArea,
    assets::IconName as LucideIcon,
    component::{
        Icon, IconName, Sizable as _, Size, StyledExt as _,
        button::{Button, ButtonVariants as _},
        h_flex,
        input::{Input, InputEvent, InputState},
        resizable::{h_resizable, resizable_panel, v_resizable},
        v_flex,
    },
    div, prelude::*, px,
};

use crate::models::{
    AccentPreset, AppSettings, DiskTree, ExtensionStat, NodeId, SizeUnit, SortColumn, SortSpec,
    VisibleRow, VolumeInfo,
};
use crate::platform::{self, FileLockStatus};
use crate::scanner::{self, ScanEvent, ScanProgress, ScanTarget};
use crate::theme::{
    BG_BASE, BG_ELEVATED, BG_HOVER, BG_SURFACE, BORDER_SUBTLE, RADIUS_CONTROL, RADIUS_ROW,
    STATUS_ERROR, STATUS_SUCCESS, STATUS_WARNING, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
    apply_obsidian_theme, crisp_resize_handle_appearance, format_count, format_duration_ms,
    format_size, full_hit_input, render_app_badge, soft_tint, solid,
};
use crate::ui::extensions_view::render_extensions_view;
use crate::ui::files_view::render_files_view;
use crate::ui::inspector::render_inspector;
use crate::ui::settings_modal::{
    PendingConfirm, SettingsCategory, render_confirm_modal, render_settings_modal,
};
use crate::ui::sidebar::render_sidebar;
use crate::ui::tree_table::render_tree_table;
use crate::ui::treemap_view::{TreemapCanvasCache, render_treemap_pane};
use crate::updater::{self, REPO_URL, UpdateInfo, UpdateState, spawn_tokio};

gpui_kit::actions!(
    textree,
    [
        FocusSearch,
        PickFolder,
        Rescan,
        CancelOrDismiss,
        ToggleSidebar,
        ToggleInspector,
        ToggleTreemap,
        OpenSettings,
        Quit
    ]
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MainTab {
    #[default]
    Tree,
    TopFiles,
    Extensions,
}

#[derive(Debug, Clone, Default)]
pub enum ScanState {
    #[default]
    Idle,
    Scanning(ScanProgress),
    Ready,
    Error(String),
}

pub struct Workspace {
    focus_handle: FocusHandle,
    pub settings: AppSettings,
    pub is_elevated: bool,
    pub volumes: Vec<VolumeInfo>,
    pub selected_target: ScanTarget,
    pub scan_state: ScanState,
    cancel_token: Option<Arc<AtomicBool>>,
    scan_seq: u64,
    pub tree: Option<Arc<DiskTree>>,
    pub tree_version: u64,
    pub active_tab: MainTab,
    pub sort: SortSpec,
    pub expanded: HashSet<NodeId>,
    pub selected_node: Option<NodeId>,
    pub selected_top_children: Vec<NodeId>,
    pub selected_full_path: Option<SharedString>,
    pub selected_has_fs_path: bool,
    pub treemap_root: Option<NodeId>,
    pub treemap_hovered: Option<NodeId>,
    pub treemap_cache: Rc<RefCell<TreemapCanvasCache>>,
    pub visible_rows: Vec<VisibleRow>,
    pub top_file_rows: Vec<NodeId>,
    pub extension_rows: Vec<ExtensionStat>,
    pub ext_filter: Option<String>,
    pub denied_only_filter: bool,
    pub search_input: Entity<InputState>,
    pub search_query: String,
    pub tree_scroll: UniformListScrollHandle,
    pub files_scroll: UniformListScrollHandle,
    pub ext_scroll: UniformListScrollHandle,
    lock_check_seq: u64,
    pub lock_status: FileLockStatus,
    pub show_sidebar: bool,
    pub show_inspector: bool,
    pub show_settings: bool,
    pub settings_category: SettingsCategory,
    pub pending_confirm: Option<PendingConfirm>,
    pub toast_message: Option<(String, bool)>,
    pub update_state: UpdateState,
    pub last_update_info: Option<UpdateInfo>,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(settings: AppSettings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let volumes = platform::list_volumes();
        let selected_target = ScanTarget::AllFixedDrives;

        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Filter files, folders, or extensions... (Ctrl+K)")
        });
        let search_sub = cx.subscribe_in(&search_input, window, |this, state, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.search_query = state.read(cx).value().to_string();
                this.rebuild_cached_views();
                cx.notify();
            }
        });

        let auto_scan = settings.auto_scan_on_launch;
        let auto_check = settings.auto_check_updates;
        let mut ws = Self {
            focus_handle,
            settings,
            is_elevated: platform::is_elevated(),
            volumes,
            selected_target,
            scan_state: ScanState::Idle,
            cancel_token: None,
            scan_seq: 0,
            tree: None,
            tree_version: 0,
            active_tab: MainTab::Tree,
            sort: SortSpec::default(),
            expanded: HashSet::new(),
            selected_node: None,
            selected_top_children: Vec::new(),
            selected_full_path: None,
            selected_has_fs_path: false,
            treemap_root: None,
            treemap_hovered: None,
            treemap_cache: Rc::new(RefCell::new(TreemapCanvasCache::default())),
            visible_rows: Vec::new(),
            top_file_rows: Vec::new(),
            extension_rows: Vec::new(),
            ext_filter: None,
            denied_only_filter: false,
            search_input,
            search_query: String::new(),
            tree_scroll: UniformListScrollHandle::new(),
            files_scroll: UniformListScrollHandle::new(),
            ext_scroll: UniformListScrollHandle::new(),
            lock_check_seq: 0,
            lock_status: FileLockStatus::NotChecked,
            show_sidebar: true,
            show_inspector: true,
            show_settings: false,
            settings_category: SettingsCategory::Appearance,
            pending_confirm: None,
            toast_message: None,
            update_state: UpdateState::Idle,
            last_update_info: None,
            _subscriptions: vec![search_sub],
        };

        if auto_scan {
            ws.start_scan(ScanTarget::AllFixedDrives, cx);
        }
        if auto_check {
            ws.run_update_check(false, cx);
        }
        ws
    }

    pub fn refresh_volumes(&mut self, cx: &mut Context<Self>) {
        self.volumes = platform::list_volumes();
        cx.notify();
    }

    pub fn start_scan(&mut self, target: ScanTarget, cx: &mut Context<Self>) {
        if let Some(prev) = self.cancel_token.take() {
            prev.store(true, Ordering::Relaxed);
        }

        self.scan_seq = self.scan_seq.wrapping_add(1);
        let seq = self.scan_seq;
        self.selected_target = target.clone();
        self.scan_state = ScanState::Scanning(ScanProgress::default());
        self.toast_message = None;

        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel_token = Some(cancel.clone());
        let (tx, mut rx) = unbounded::<ScanEvent>();
        let volumes = self.volumes.clone();
        let sort = self.sort;
        let prefer_mft = self.settings.prefer_ntfs_mft;

        scanner::spawn_scan(target, volumes, sort, prefer_mft, cancel, tx);

        cx.spawn(async move |this, cx| {
            while let Some(evt) = rx.next().await {
                let mut should_stop = false;
                let _ = this.update(cx, |this, cx| {
                    if this.scan_seq != seq {
                        should_stop = true;
                        return;
                    }
                    match evt {
                        ScanEvent::Progress(progress) => {
                            this.scan_state = ScanState::Scanning(progress);
                            cx.notify();
                        }
                        ScanEvent::Completed(tree) => {
                            this.cancel_token = None;
                            this.scan_state = ScanState::Ready;
                            let root_id = tree.root;
                            let mut expanded = HashSet::new();
                            expanded.insert(root_id);
                            this.tree = Some(Arc::new(tree));
                            this.tree_version = this.tree_version.wrapping_add(1);
                            this.expanded = expanded;
                            this.selected_node = Some(root_id);
                            this.treemap_root = Some(root_id);
                            this.treemap_hovered = None;
                            this.lock_status = FileLockStatus::NotApplicable;
                            this.ext_filter = None;
                            this.denied_only_filter = false;
                            this.refresh_selection_cache();
                            this.rebuild_cached_views();
                            this.volumes = platform::list_volumes();
                            cx.notify();
                            should_stop = true;
                        }
                        ScanEvent::Cancelled => {
                            this.cancel_token = None;
                            this.scan_state = if this.tree.is_some() {
                                ScanState::Ready
                            } else {
                                ScanState::Idle
                            };
                            this.toast_message = Some(("Scan cancelled.".to_string(), false));
                            cx.notify();
                            should_stop = true;
                        }
                        ScanEvent::Error(err) => {
                            this.cancel_token = None;
                            this.scan_state = ScanState::Error(err);
                            cx.notify();
                            should_stop = true;
                        }
                    }
                });
                if should_stop {
                    break;
                }
            }
        })
        .detach();

        cx.notify();
    }

    pub fn stop_scan(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = self.cancel_token.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    pub fn rescan_current(&mut self, cx: &mut Context<Self>) {
        let target = self.selected_target.clone();
        self.start_scan(target, cx);
    }

    pub fn scan_selected_folder(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .selected_node
            .and_then(|id| self.tree.as_ref()?.full_path(id))
        else {
            return;
        };
        if path.is_dir() {
            self.start_scan(ScanTarget::Folder(path), cx);
        }
    }

    pub fn pick_folder(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select folder to analyze".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(folder) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    this.start_scan(ScanTarget::Folder(folder), cx);
                });
            }
        })
        .detach();
    }

    pub fn set_active_tab(&mut self, tab: MainTab, cx: &mut Context<Self>) {
        if self.active_tab != tab {
            self.active_tab = tab;
            cx.notify();
        }
    }

    pub fn set_sort_column(&mut self, col: SortColumn, cx: &mut Context<Self>) {
        if self.sort.column == col {
            self.sort.descending = !self.sort.descending;
        } else {
            self.sort.column = col;
            self.sort.descending = !matches!(col, SortColumn::Name);
        }
        if let Some(tree) = &mut self.tree {
            Arc::make_mut(tree).sort_children(self.sort);
        }
        self.rebuild_cached_views();
        cx.notify();
    }

    pub fn toggle_expand(&mut self, node_id: NodeId, cx: &mut Context<Self>) {
        if !self.expanded.remove(&node_id) {
            self.expanded.insert(node_id);
        }
        self.rebuild_visible_rows();
        cx.notify();
    }

    pub fn select_node(&mut self, node_id: NodeId, cx: &mut Context<Self>) {
        let changed = self.selected_node != Some(node_id);
        self.selected_node = Some(node_id);
        if changed {
            self.refresh_selection_cache();
            self.inspect_lock_for_selected(cx);
        }
        cx.notify();
    }

    fn refresh_selection_cache(&mut self) {
        self.selected_top_children.clear();
        self.selected_full_path = None;
        self.selected_has_fs_path = false;

        let (Some(tree), Some(node_id)) = (&self.tree, self.selected_node) else {
            return;
        };
        let Some(node) = tree.get(node_id) else {
            return;
        };
        if let Some(path) = tree.full_path(node_id) {
            self.selected_has_fs_path = true;
            self.selected_full_path = Some(SharedString::from(path.to_string_lossy().into_owned()));
        } else {
            self.selected_full_path = Some(node.name.clone());
        }
        if node.is_container() {
            self.selected_top_children = tree.largest_children(node_id, 5);
        }
    }

    pub fn recheck_selected_lock(&mut self, cx: &mut Context<Self>) {
        self.inspect_lock_for_selected(cx);
        cx.notify();
    }

    fn inspect_lock_for_selected(&mut self, cx: &mut Context<Self>) {
        self.lock_check_seq = self.lock_check_seq.wrapping_add(1);
        let seq = self.lock_check_seq;

        let Some((is_file, path)) = self.selected_node.and_then(|id| {
            let tree = self.tree.as_ref()?;
            let node = tree.get(id)?;
            Some((node.is_file(), tree.full_path(id)?))
        }) else {
            self.lock_status = FileLockStatus::NotApplicable;
            return;
        };

        if !is_file {
            self.lock_status = FileLockStatus::NotApplicable;
            return;
        }

        self.lock_status = FileLockStatus::Checking;
        let (tx, mut rx) = unbounded::<FileLockStatus>();
        std::thread::spawn(move || {
            let status = platform::inspect_file_lock(&path);
            let _ = tx.unbounded_send(status);
        });

        cx.spawn(async move |this, cx| {
            if let Some(status) = rx.next().await {
                let _ = this.update(cx, |this, cx| {
                    if this.lock_check_seq == seq {
                        this.lock_status = status;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    pub fn reveal_node_in_tree(&mut self, node_id: NodeId, cx: &mut Context<Self>) {
        let Some(tree) = &self.tree else {
            return;
        };
        for anc in tree.ancestors_of(node_id) {
            self.expanded.insert(anc);
        }
        self.active_tab = MainTab::Tree;
        self.rebuild_visible_rows();
        self.select_node(node_id, cx);
        if let Some(row_idx) = self.visible_rows.iter().position(|r| r.node_id == node_id) {
            self.tree_scroll
                .scroll_to_item(row_idx, ScrollStrategy::Center);
        }
        cx.notify();
    }

    pub fn filter_by_extension(&mut self, ext: String, cx: &mut Context<Self>) {
        self.ext_filter = Some(ext);
        self.denied_only_filter = false;
        self.active_tab = MainTab::TopFiles;
        self.rebuild_top_files();
        cx.notify();
    }

    pub fn clear_extension_filter(&mut self, cx: &mut Context<Self>) {
        self.ext_filter = None;
        self.rebuild_top_files();
        cx.notify();
    }

    pub fn toggle_denied_only_filter(&mut self, cx: &mut Context<Self>) {
        self.denied_only_filter = !self.denied_only_filter;
        self.rebuild_top_files();
        cx.notify();
    }

    pub fn clear_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_query.clear();
        self.search_input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.rebuild_cached_views();
        cx.notify();
    }

    fn rebuild_cached_views(&mut self) {
        self.rebuild_visible_rows();
        self.rebuild_top_files();
        self.rebuild_extensions();
    }

    fn rebuild_visible_rows(&mut self) {
        self.visible_rows = match &self.tree {
            Some(tree) => tree.flatten_visible(
                &self.expanded,
                &self.search_query,
                self.settings.show_hidden_files,
            ),
            None => Vec::new(),
        };
    }

    fn rebuild_top_files(&mut self) {
        self.top_file_rows = match &self.tree {
            Some(tree) => tree.top_files(
                &self.search_query,
                self.ext_filter.as_deref(),
                self.denied_only_filter,
                self.settings.show_hidden_files,
                2_000,
            ),
            None => Vec::new(),
        };
    }

    fn rebuild_extensions(&mut self) {
        self.extension_rows = match &self.tree {
            Some(tree) => tree.extension_stats(self.settings.show_hidden_files),
            None => Vec::new(),
        };
    }

    pub fn reveal_selected_in_explorer(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .selected_node
            .and_then(|id| self.tree.as_ref()?.full_path(id))
        else {
            return;
        };
        match platform::reveal_in_file_manager(&path) {
            Ok(()) => self.toast_message = None,
            Err(err) => self.toast_message = Some((err, true)),
        }
        cx.notify();
    }

    pub fn copy_selected_path(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self
            .selected_node
            .and_then(|id| self.tree.as_ref()?.full_path(id))
        else {
            return;
        };
        let path_str = path.to_string_lossy().into_owned();
        cx.write_to_clipboard(ClipboardItem::new_string(path_str));
        self.toast_message = Some(("Path copied to clipboard.".to_string(), false));
        cx.notify();
    }

    pub fn request_trash_selected(&mut self, cx: &mut Context<Self>) {
        let Some(node_id) = self.selected_node else {
            return;
        };
        let Some(tree) = &self.tree else {
            return;
        };
        let Some(node) = tree.get(node_id) else {
            return;
        };
        if node_id == tree.root
            || node
                .flags
                .intersects(crate::models::NodeFlags::IS_VOLUME | crate::models::NodeFlags::IS_VIRTUAL)
        {
            return;
        }
        let Some(path) = tree.full_path(node_id) else {
            return;
        };

        let pending = PendingConfirm::TrashItem {
            node_id,
            name: node.name.to_string(),
            path,
            size: node.logical_size,
            is_dir: node.is_container(),
            item_count: node.total_items(),
        };

        if self.settings.confirm_before_trash {
            self.pending_confirm = Some(pending);
            cx.notify();
        } else {
            self.pending_confirm = Some(pending);
            self.accept_confirm(cx);
        }
    }

    pub fn dismiss_confirm(&mut self, cx: &mut Context<Self>) {
        self.pending_confirm = None;
        cx.notify();
    }

    pub fn accept_confirm(&mut self, cx: &mut Context<Self>) {
        let Some(action) = self.pending_confirm.take() else {
            return;
        };
        let PendingConfirm::TrashItem {
            node_id,
            name,
            path,
            ..
        } = action;

        let (tx, mut rx) = unbounded::<Result<(), String>>();
        std::thread::spawn(move || {
            let res = platform::move_to_trash(&path);
            let _ = tx.unbounded_send(res);
        });

        cx.spawn(async move |this, cx| {
            if let Some(res) = rx.next().await {
                let _ = this.update(cx, |this, cx| match res {
                    Ok(()) => {
                        let parent_id = this
                            .tree
                            .as_ref()
                            .and_then(|t| t.get(node_id))
                            .and_then(|n| n.parent);
                        if let Some(tree) = &mut this.tree {
                            Arc::make_mut(tree).remove_subtree_soft(node_id);
                            this.tree_version = this.tree_version.wrapping_add(1);
                        }
                        this.selected_node = parent_id;
                        this.treemap_hovered = None;
                        this.lock_status = FileLockStatus::NotApplicable;
                        this.refresh_selection_cache();
                        this.rebuild_cached_views();
                        this.volumes = platform::list_volumes();
                        this.toast_message =
                            Some((format!("Moved \"{name}\" to Recycle Bin."), false));
                        cx.notify();
                    }
                    Err(err) => {
                        this.toast_message = Some((err, true));
                        cx.notify();
                    }
                });
            }
        })
        .detach();
        cx.notify();
    }

    // --- Settings mutators ---

    pub fn set_accent_preset(&mut self, accent: AccentPreset, cx: &mut Context<Self>) {
        self.settings.accent = accent;
        self.settings.save();
        apply_obsidian_theme(accent, cx);
        cx.notify();
    }

    pub fn set_size_unit(&mut self, unit: SizeUnit, cx: &mut Context<Self>) {
        self.settings.size_unit = unit;
        self.settings.save();
        cx.notify();
    }

    pub fn toggle_show_hidden(&mut self, cx: &mut Context<Self>) {
        self.settings.show_hidden_files = !self.settings.show_hidden_files;
        self.settings.save();
        self.rebuild_cached_views();
        cx.notify();
    }

    pub fn toggle_confirm_before_trash(&mut self, cx: &mut Context<Self>) {
        self.settings.confirm_before_trash = !self.settings.confirm_before_trash;
        self.settings.save();
        cx.notify();
    }

    pub fn toggle_auto_scan_on_launch(&mut self, cx: &mut Context<Self>) {
        self.settings.auto_scan_on_launch = !self.settings.auto_scan_on_launch;
        self.settings.save();
        cx.notify();
    }

    pub fn toggle_treemap(&mut self, cx: &mut Context<Self>) {
        self.settings.show_treemap = !self.settings.show_treemap;
        self.settings.save();
        cx.notify();
    }

    pub fn toggle_prefer_ntfs_mft(&mut self, cx: &mut Context<Self>) {
        self.settings.prefer_ntfs_mft = !self.settings.prefer_ntfs_mft;
        self.settings.save();
        cx.notify();
    }

    pub fn relaunch_as_admin(&mut self, cx: &mut Context<Self>) {
        match platform::relaunch_as_admin() {
            Ok(()) => {
                self.settings.save();
                cx.quit();
            }
            Err(err) => {
                self.toast_message = Some((err, true));
                cx.notify();
            }
        }
    }

    pub fn set_treemap_root(&mut self, node_id: NodeId, cx: &mut Context<Self>) {
        self.treemap_root = Some(node_id);
        self.treemap_hovered = None;
        if !self.settings.show_treemap {
            self.settings.show_treemap = true;
            self.settings.save();
        }
        cx.notify();
    }

    pub fn treemap_zoom_up(&mut self, cx: &mut Context<Self>) {
        let Some(tree) = &self.tree else {
            return;
        };
        let current = self.treemap_root.unwrap_or(tree.root);
        if current == tree.root {
            return;
        }
        if let Some(pid) = tree.get(current).and_then(|n| n.parent) {
            self.treemap_root = Some(pid);
            self.treemap_hovered = None;
            cx.notify();
        }
    }

    pub fn handle_treemap_hover(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let hit = {
            let cache = self.treemap_cache.borrow();
            let lx = f32::from(position.x) - f32::from(cache.origin.x);
            let ly = f32::from(position.y) - f32::from(cache.origin.y);
            cache.layout.hit_test(lx, ly).map(|r| r.node_id)
        };
        if self.treemap_hovered != hit {
            self.treemap_hovered = hit;
            cx.notify();
        }
    }

    pub fn handle_treemap_click(
        &mut self,
        position: Point<Pixels>,
        click_count: usize,
        cx: &mut Context<Self>,
    ) {
        let hit = {
            let cache = self.treemap_cache.borrow();
            let lx = f32::from(position.x) - f32::from(cache.origin.x);
            let ly = f32::from(position.y) - f32::from(cache.origin.y);
            cache.layout.hit_test(lx, ly)
        };
        let Some(rect) = hit else {
            return;
        };
        self.reveal_node_in_tree(rect.node_id, cx);
        if click_count >= 2
            && let Some(tree) = &self.tree
            && let Some(node) = tree.get(rect.node_id)
        {
            if node.is_container() {
                self.set_treemap_root(rect.node_id, cx);
            } else if let Some(pid) = node.parent {
                self.set_treemap_root(pid, cx);
            }
        }
    }

    pub fn toggle_auto_check_updates(&mut self, cx: &mut Context<Self>) {
        self.settings.auto_check_updates = !self.settings.auto_check_updates;
        self.settings.save();
        cx.notify();
    }

    // --- Auto-updater methods ---

    pub fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if self.update_state.is_busy() {
            return;
        }
        self.run_update_check(true, cx);
    }

    fn run_update_check(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.update_state.is_busy() {
            return;
        }
        if manual {
            self.update_state = UpdateState::Checking;
            cx.notify();
        }
        let (tx, mut rx) = unbounded::<Result<Option<UpdateInfo>, String>>();
        spawn_tokio(async move {
            let outcome = updater::check_for_updates().await;
            let _ = tx.unbounded_send(outcome);
        });
        cx.spawn(async move |this, cx| {
            if let Some(res) = rx.next().await {
                let _ = this.update(cx, |this, cx| {
                    if matches!(
                        this.update_state,
                        UpdateState::Downloading { .. } | UpdateState::Installing(_)
                    ) {
                        return;
                    }
                    this.update_state = match res {
                        Ok(Some(info)) => {
                            this.last_update_info = Some(info.clone());
                            UpdateState::Available(info)
                        }
                        Ok(None) => {
                            if manual {
                                UpdateState::UpToDate
                            } else {
                                UpdateState::Idle
                            }
                        }
                        Err(err) => {
                            if manual {
                                UpdateState::Failed(err)
                            } else {
                                UpdateState::Idle
                            }
                        }
                    };
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn current_or_last_update_info(&self) -> Option<&UpdateInfo> {
        match &self.update_state {
            UpdateState::Available(info)
            | UpdateState::Downloading { info, .. }
            | UpdateState::Installing(info) => Some(info),
            _ => self.last_update_info.as_ref(),
        }
    }

    pub fn start_update_install(&mut self, cx: &mut Context<Self>) {
        if self.update_state.is_busy() {
            return;
        }
        let Some(info) = self.current_or_last_update_info().cloned() else {
            return;
        };
        if !info.supports_in_app_install() {
            self.open_update_download(cx);
            return;
        }

        enum InstallMsg {
            Progress {
                downloaded: u64,
                total: Option<u64>,
            },
            Staged(updater::github::StagedUpdate),
            Failed(String),
        }

        self.last_update_info = Some(info.clone());
        self.update_state = UpdateState::Downloading {
            info: info.clone(),
            downloaded_bytes: 0,
            total_bytes: None,
        };
        cx.notify();

        let (tx, mut rx) = unbounded::<InstallMsg>();
        let info_for_task = info.clone();
        spawn_tokio(async move {
            let outcome = updater::download_and_stage_update(&info_for_task, |downloaded, total| {
                let _ = tx.unbounded_send(InstallMsg::Progress { downloaded, total });
            })
            .await;
            match outcome {
                Ok(staged) => {
                    let _ = tx.unbounded_send(InstallMsg::Staged(staged));
                }
                Err(err) => {
                    let _ = tx.unbounded_send(InstallMsg::Failed(err));
                }
            }
        });

        let install_info = info;
        cx.spawn(async move |this, cx| {
            while let Some(msg) = rx.next().await {
                match msg {
                    InstallMsg::Progress { downloaded, total } => {
                        let _ = this.update(cx, |this, cx| {
                            if let UpdateState::Downloading {
                                downloaded_bytes,
                                total_bytes,
                                ..
                            } = &mut this.update_state
                            {
                                *downloaded_bytes = downloaded;
                                *total_bytes = total;
                                cx.notify();
                            }
                        });
                    }
                    InstallMsg::Failed(err) => {
                        let _ = this.update(cx, |this, cx| {
                            this.update_state = UpdateState::Failed(err);
                            cx.notify();
                        });
                        break;
                    }
                    InstallMsg::Staged(staged) => {
                        let _ = this.update(cx, |this, cx| {
                            this.update_state = UpdateState::Installing(install_info.clone());
                            this.settings.save();
                            cx.notify();
                        });

                        match updater::spawn_detached_updater(&staged) {
                            Ok(()) => {
                                let _ = this.update(cx, |_, cx| {
                                    cx.quit();
                                });
                            }
                            Err(err) => {
                                let _ = this.update(cx, |this, cx| {
                                    this.update_state = UpdateState::Failed(err);
                                    cx.notify();
                                });
                            }
                        }
                        break;
                    }
                }
            }
        })
        .detach();
    }

    pub fn open_update_download(&mut self, cx: &mut Context<Self>) {
        let url = self
            .current_or_last_update_info()
            .map(|i| i.download_url.clone())
            .unwrap_or_else(|| format!("{REPO_URL}/releases"));
        if let Err(err) = updater::open_url_in_browser(&url) {
            self.update_state = UpdateState::Failed(err);
            cx.notify();
        }
    }

    pub fn open_release_notes(&mut self, cx: &mut Context<Self>) {
        let url = self
            .current_or_last_update_info()
            .map(|i| i.release_url.clone())
            .unwrap_or_else(|| format!("{REPO_URL}/releases"));
        if let Err(err) = updater::open_url_in_browser(&url) {
            self.update_state = UpdateState::Failed(err);
            cx.notify();
        }
    }

    pub fn open_github_repo(&mut self, cx: &mut Context<Self>) {
        if let Err(err) = updater::open_url_in_browser(REPO_URL) {
            self.toast_message = Some((err, true));
            cx.notify();
        }
    }

    fn handle_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.show_settings || self.pending_confirm.is_some() {
            return;
        }
        let key = event.keystroke.key.as_str();
        match (self.active_tab, key) {
            (MainTab::Tree, "down") if !self.visible_rows.is_empty() => {
                let next_idx = self
                    .selected_node
                    .and_then(|sel| self.visible_rows.iter().position(|r| r.node_id == sel))
                    .map(|i| (i + 1).min(self.visible_rows.len() - 1))
                    .unwrap_or(0);
                let id = self.visible_rows[next_idx].node_id;
                self.select_node(id, cx);
                self.tree_scroll
                    .scroll_to_item(next_idx, ScrollStrategy::Nearest);
            }
            (MainTab::Tree, "up") if !self.visible_rows.is_empty() => {
                let prev_idx = self
                    .selected_node
                    .and_then(|sel| self.visible_rows.iter().position(|r| r.node_id == sel))
                    .map(|i| i.saturating_sub(1))
                    .unwrap_or(0);
                let id = self.visible_rows[prev_idx].node_id;
                self.select_node(id, cx);
                self.tree_scroll
                    .scroll_to_item(prev_idx, ScrollStrategy::Nearest);
            }
            (MainTab::Tree, "right") => {
                if let Some(sel) = self.selected_node
                    && let Some(row) = self.visible_rows.iter().find(|r| r.node_id == sel).copied()
                    && row.has_children
                    && !row.is_expanded
                {
                    self.toggle_expand(sel, cx);
                }
            }
            (MainTab::Tree, "left") => {
                if let Some(sel) = self.selected_node
                    && let Some(row) = self.visible_rows.iter().find(|r| r.node_id == sel).copied()
                {
                    if row.is_expanded {
                        self.toggle_expand(sel, cx);
                    } else if let Some(pid) = self.tree.as_ref().and_then(|t| t.get(sel)?.parent) {
                        self.select_node(pid, cx);
                        if let Some(idx) = self.visible_rows.iter().position(|r| r.node_id == pid) {
                            self.tree_scroll
                                .scroll_to_item(idx, ScrollStrategy::Nearest);
                        }
                    }
                }
            }
            (MainTab::TopFiles, "down") if !self.top_file_rows.is_empty() => {
                let next_idx = self
                    .selected_node
                    .and_then(|sel| self.top_file_rows.iter().position(|&id| id == sel))
                    .map(|i| (i + 1).min(self.top_file_rows.len() - 1))
                    .unwrap_or(0);
                let id = self.top_file_rows[next_idx];
                self.select_node(id, cx);
                self.files_scroll
                    .scroll_to_item(next_idx, ScrollStrategy::Nearest);
            }
            (MainTab::TopFiles, "up") if !self.top_file_rows.is_empty() => {
                let prev_idx = self
                    .selected_node
                    .and_then(|sel| self.top_file_rows.iter().position(|&id| id == sel))
                    .map(|i| i.saturating_sub(1))
                    .unwrap_or(0);
                let id = self.top_file_rows[prev_idx];
                self.select_node(id, cx);
                self.files_scroll
                    .scroll_to_item(prev_idx, ScrollStrategy::Nearest);
            }
            (_, "delete") => {
                self.request_trash_selected(cx);
            }
            _ => {}
        }
    }

    fn render_header(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = self.settings.accent;
        let is_scanning = matches!(self.scan_state, ScanState::Scanning(_));
        let has_search = !self.search_query.is_empty();
        let update_available = self.update_state.has_pending_update();
        let top_update_pill: Option<SharedString> = match &self.update_state {
            UpdateState::Available(info) => Some(SharedString::from(format!("Update {}", info.tag_name))),
            UpdateState::Downloading { info, .. } => {
                Some(SharedString::from(format!("Downloading {}...", info.tag_name)))
            }
            UpdateState::Installing(info) => {
                Some(SharedString::from(format!("Installing {}...", info.tag_name)))
            }
            _ => None,
        };

        h_flex()
            .w_full()
            .h(px(38.))
            .flex_shrink_0()
            .items_center()
            .bg(solid(BG_ELEVATED))
            .border_b_1()
            .border_color(solid(BORDER_SUBTLE))
            // Brand area (draggable)
            .child(
                h_flex()
                    .h_full()
                    .px_3()
                    .gap_2()
                    .items_center()
                    .flex_shrink_0()
                    .window_control_area(WindowControlArea::Drag)
                    .child(render_app_badge(18.))
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(solid(TEXT_PRIMARY))
                            .child("TexTree"),
                    ),
            )
            // Interactive controls (kept OUTSIDE drag containers)
            .child(
                h_flex()
                    .h_full()
                    .items_center()
                    .gap_1p5()
                    .flex_shrink_0()
                    .child(
                        Button::new("hdr-toggle-sidebar")
                            .with_size(Size::XSmall)
                            .ghost()
                            .icon(IconName::PanelLeft)
                            .tooltip("Toggle Sidebar (Ctrl+B)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_sidebar = !this.show_sidebar;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .max_w(px(200.))
                            .h(px(26.))
                            .px_2()
                            .rounded(px(RADIUS_CONTROL))
                            .bg(solid(BG_BASE))
                            .border_1()
                            .border_color(solid(BORDER_SUBTLE))
                            .flex()
                            .items_center()
                            .text_xs()
                            .text_color(solid(TEXT_SECONDARY))
                            .truncate()
                            .child(SharedString::from(self.selected_target.display_label())),
                    )
                    .child(if is_scanning {
                        Button::new("hdr-stop-scan")
                            .with_size(Size::XSmall)
                            .danger()
                            .icon(LucideIcon::CircleStop)
                            .label("Stop")
                            .tooltip("Stop active scan (Esc)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.stop_scan(cx);
                            }))
                    } else {
                        Button::new("hdr-start-scan")
                            .with_size(Size::XSmall)
                            .primary()
                            .icon(IconName::Play)
                            .label("Scan")
                            .tooltip("Scan selected target (F5)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.rescan_current(cx);
                            }))
                    })
                    // Single-border unified search bar (borderless child input)
                    .child(
                        h_flex()
                            .w(px(280.))
                            .h(px(26.))
                            .px_2()
                            .gap_1p5()
                            .items_center()
                            .rounded(px(RADIUS_ROW))
                            .bg(solid(BG_BASE))
                            .border_1()
                            .border_color(solid(BORDER_SUBTLE))
                            .child(
                                Icon::new(IconName::Search)
                                    .xsmall()
                                    .text_color(solid(TEXT_MUTED)),
                            )
                            .child(
                                div().flex_1().min_w_0().h_full().child(full_hit_input(
                                    "hdr-search-hit",
                                    &self.search_input,
                                    Input::new(&self.search_input)
                                        .with_size(Size::XSmall)
                                        .appearance(false),
                                )),
                            )
                            .when(has_search, |bar| {
                                bar.child(
                                    div()
                                        .id("hdr-search-clear")
                                        .cursor_pointer()
                                        .text_color(solid(TEXT_MUTED))
                                        .hover(|s| s.text_color(solid(TEXT_PRIMARY)))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.clear_search(window, cx);
                                        }))
                                        .child(Icon::new(IconName::Close).xsmall()),
                                )
                            }),
                    ),
            )
            // Draggable spacer occupying all open middle space
            .child(
                div()
                    .flex_1()
                    .min_w(px(36.))
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            // Right toolbar controls + integrated window controls
            .child(
                h_flex()
                    .h_full()
                    .items_center()
                    .gap_1p5()
                    .pr_1p5()
                    .flex_shrink_0()
                    .when_some(self.toast_message.clone(), |row, (msg, is_err)| {
                        let color = if is_err {
                            STATUS_ERROR
                        } else {
                            accent.primary_hex()
                        };
                        row.child(
                            div()
                                .id("hdr-toast")
                                .max_w(px(240.))
                                .px_2()
                                .py_0p5()
                                .rounded(px(RADIUS_CONTROL))
                                .bg(soft_tint(color, 0.14))
                                .border_1()
                                .border_color(soft_tint(color, 0.30))
                                .text_xs()
                                .text_color(solid(color))
                                .truncate()
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.toast_message = None;
                                    cx.notify();
                                }))
                                .child(SharedString::from(msg)),
                        )
                    })
                    .when_some(top_update_pill, |row, label| {
                        row.child(
                            div()
                                .id("hdr-update-pill")
                                .px_2()
                                .py_0p5()
                                .rounded(px(RADIUS_CONTROL))
                                .bg(soft_tint(accent.primary_hex(), 0.16))
                                .border_1()
                                .border_color(solid(accent.primary_hex()))
                                .text_xs()
                                .font_medium()
                                .text_color(solid(accent.primary_hex()))
                                .cursor_pointer()
                                .hover(|s| s.bg(soft_tint(accent.primary_hex(), 0.24)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.settings_category = SettingsCategory::UpdatesAbout;
                                    this.show_settings = true;
                                    cx.notify();
                                }))
                                .child(label),
                        )
                    })
                    .when(cfg!(target_os = "windows"), |row| {
                        if self.is_elevated {
                            row.child(
                                h_flex()
                                    .px_1p5()
                                    .py_0p5()
                                    .gap_1()
                                    .items_center()
                                    .rounded(px(RADIUS_CONTROL))
                                    .bg(soft_tint(STATUS_SUCCESS, 0.14))
                                    .border_1()
                                    .border_color(soft_tint(STATUS_SUCCESS, 0.30))
                                    .text_xs()
                                    .font_medium()
                                    .text_color(solid(STATUS_SUCCESS))
                                    .child(Icon::new(LucideIcon::ShieldCheck).xsmall())
                                    .child("MFT"),
                            )
                        } else {
                            row.child(
                                h_flex()
                                    .id("hdr-relaunch-admin")
                                    .px_2()
                                    .py_0p5()
                                    .gap_1()
                                    .items_center()
                                    .rounded(px(RADIUS_CONTROL))
                                    .bg(soft_tint(accent.primary_hex(), 0.12))
                                    .border_1()
                                    .border_color(soft_tint(accent.primary_hex(), 0.35))
                                    .text_xs()
                                    .font_medium()
                                    .text_color(solid(accent.primary_hex()))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(soft_tint(accent.primary_hex(), 0.22)))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.relaunch_as_admin(cx);
                                    }))
                                    .child(Icon::new(LucideIcon::Zap).xsmall())
                                    .child("Admin Mode (MFT)"),
                            )
                        }
                    })
                    .child(
                        Button::new("hdr-toggle-treemap")
                            .with_size(Size::XSmall)
                            .ghost()
                            .icon(IconName::PanelBottom)
                            .tooltip("Toggle Treemap (Ctrl+T)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_treemap(cx);
                            })),
                    )
                    .child(
                        Button::new("hdr-toggle-inspector")
                            .with_size(Size::XSmall)
                            .ghost()
                            .icon(IconName::PanelRight)
                            .tooltip("Toggle Inspector (Ctrl+I)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_inspector = !this.show_inspector;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .relative()
                            .child(
                                Button::new("hdr-open-settings")
                                    .with_size(Size::XSmall)
                                    .ghost()
                                    .icon(IconName::Settings)
                                    .tooltip("Settings (Ctrl+,)")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_settings = true;
                                        cx.notify();
                                    })),
                            )
                            .when(update_available, |wrap| {
                                wrap.child(
                                    div()
                                        .absolute()
                                        .top(px(2.))
                                        .right(px(2.))
                                        .w(px(6.))
                                        .h(px(6.))
                                        .rounded(px(999.))
                                        .bg(solid(accent.primary_hex())),
                                )
                            }),
                    ),
            )
            .child(render_window_controls(window))
    }

    fn render_center_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = self.settings.accent;
        let unit = self.settings.size_unit;

        v_flex()
            .size_full()
            .bg(solid(BG_BASE))
            .overflow_hidden()
            // Flat subtab strip
            .child(
                h_flex()
                    .w_full()
                    .h(px(34.))
                    .px_3()
                    .items_center()
                    .justify_between()
                    .bg(solid(BG_SURFACE))
                    .border_b_1()
                    .border_color(solid(BORDER_SUBTLE))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(subtab_btn(
                                "tab-tree",
                                "Tree View",
                                LucideIcon::ListTree,
                                self.active_tab == MainTab::Tree,
                                MainTab::Tree,
                                cx,
                            ))
                            .child(subtab_btn(
                                "tab-files",
                                "Top Files",
                                LucideIcon::Files,
                                self.active_tab == MainTab::TopFiles,
                                MainTab::TopFiles,
                                cx,
                            ))
                            .child(subtab_btn(
                                "tab-ext",
                                "Extensions",
                                LucideIcon::FolderTree,
                                self.active_tab == MainTab::Extensions,
                                MainTab::Extensions,
                                cx,
                            )),
                    )
                    .when_some(self.tree.as_ref(), |bar, tree| {
                        let root = tree.root_node();
                        bar.child(
                            h_flex()
                                .items_center()
                                .gap_3()
                                .text_xs()
                                .text_color(solid(TEXT_MUTED))
                                .child(SharedString::from(format!(
                                    "Total: {}",
                                    format_size(root.logical_size, unit)
                                )))
                                .child(SharedString::from(format!(
                                    "Allocated: {}",
                                    format_size(root.allocated_size, unit)
                                )))
                                .child(SharedString::from(format!(
                                    "{} files",
                                    format_count(root.file_count)
                                ))),
                        )
                    }),
            )
            // Inline live scan banner or scan error banner
            .when_some(
                match &self.scan_state {
                    ScanState::Scanning(p) => Some(p.clone()),
                    _ => None,
                },
                |pane, p| {
                    pane.child(
                        h_flex()
                            .w_full()
                            .h(px(30.))
                            .px_3()
                            .gap_3()
                            .items_center()
                            .justify_between()
                            .bg(soft_tint(accent.primary_hex(), 0.10))
                            .border_b_1()
                            .border_color(soft_tint(accent.primary_hex(), 0.30))
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .flex_1()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(solid(accent.primary_hex()))
                                            .child("Scanning"),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .truncate()
                                            .text_xs()
                                            .text_color(solid(TEXT_SECONDARY))
                                            .child(SharedString::from(if p.current_path.is_empty() {
                                                "Enumerating files...".to_string()
                                            } else {
                                                p.current_path
                                            })),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .flex_shrink_0()
                                    .gap_3()
                                    .text_xs()
                                    .text_color(solid(TEXT_PRIMARY))
                                    .child(SharedString::from(format!(
                                        "{} files",
                                        format_count(p.scanned_files)
                                    )))
                                    .child(SharedString::from(format_size(p.scanned_bytes, unit)))
                                    .when(p.denied_count > 0, |row| {
                                        row.child(
                                            div()
                                                .text_color(solid(STATUS_WARNING))
                                                .child(SharedString::from(format!(
                                                    "{} denied",
                                                    p.denied_count
                                                ))),
                                        )
                                    })
                                    .child(
                                        div()
                                            .text_color(solid(TEXT_MUTED))
                                            .child(SharedString::from(format_duration_ms(
                                                p.elapsed_ms,
                                            ))),
                                    ),
                            ),
                    )
                },
            )
            .when_some(
                match &self.scan_state {
                    ScanState::Error(err) => Some(err.clone()),
                    _ => None,
                },
                |pane, err| {
                    pane.child(
                        h_flex()
                            .w_full()
                            .px_3()
                            .py_1p5()
                            .items_center()
                            .gap_2()
                            .bg(soft_tint(STATUS_ERROR, 0.14))
                            .border_b_1()
                            .border_color(soft_tint(STATUS_ERROR, 0.30))
                            .text_xs()
                            .text_color(solid(STATUS_ERROR))
                            .child(SharedString::from(err)),
                    )
                },
            )
            // Active view (with optional split Treemap below) or calm empty state
            .child(div().flex_1().min_h_0().w_full().child(if self.tree.is_none() {
                self.render_empty_state(cx).into_any_element()
            } else {
                let primary_view = match self.active_tab {
                    MainTab::Tree => render_tree_table(self, cx).into_any_element(),
                    MainTab::TopFiles => render_files_view(self, cx).into_any_element(),
                    MainTab::Extensions => render_extensions_view(self, cx).into_any_element(),
                };
                if self.settings.show_treemap {
                    v_resizable("center-treemap-split")
                        .with_handle_appearance(crisp_resize_handle_appearance(accent))
                        .child(resizable_panel().child(primary_view))
                        .child(
                            resizable_panel()
                                .size(px(260.))
                                .size_range(px(140.)..px(560.))
                                .fixed()
                                .child(render_treemap_pane(self, cx)),
                        )
                        .into_any_element()
                } else {
                    primary_view
                }
            }))
    }

    fn render_empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let is_scanning = matches!(self.scan_state, ScanState::Scanning(_));

        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child(render_app_badge(44.))
            .child(
                v_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(solid(TEXT_PRIMARY))
                            .child(if is_scanning {
                                "Scanning storage..."
                            } else {
                                "Analyze Disk Space"
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(solid(TEXT_MUTED))
                            .child(if is_scanning {
                                "Building hierarchical size tree in parallel."
                            } else {
                                "Choose a drive from the sidebar, scan your entire PC, or pick a folder."
                            }),
                    ),
            )
            .when(!is_scanning, |col| {
                col.child(
                    h_flex()
                        .pt_1()
                        .gap_2()
                        .child(
                            Button::new("empty-scan-all")
                                .with_size(Size::Small)
                                .primary()
                                .icon(LucideIcon::Monitor)
                                .label("Scan Entire PC")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.start_scan(ScanTarget::AllFixedDrives, cx);
                                })),
                        )
                        .child(
                            Button::new("empty-pick-folder")
                                .with_size(Size::Small)
                                .secondary()
                                .icon(LucideIcon::FolderSearch)
                                .label("Scan Folder...")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.pick_folder(window, cx);
                                })),
                        ),
                )
            })
    }
}

fn subtab_btn(
    id: &'static str,
    label: &'static str,
    icon: LucideIcon,
    active: bool,
    tab: MainTab,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    h_flex()
        .id(id)
        .px_2p5()
        .py_1()
        .gap_1p5()
        .items_center()
        .rounded(px(RADIUS_CONTROL))
        .cursor_pointer()
        .bg(if active {
            solid(BG_HOVER)
        } else {
            gpui_kit::transparent_black()
        })
        .text_xs()
        .font_medium()
        .text_color(if active {
            solid(TEXT_PRIMARY)
        } else {
            solid(TEXT_SECONDARY)
        })
        .hover(|s| s.bg(solid(BG_ELEVATED)).text_color(solid(TEXT_PRIMARY)))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_active_tab(tab, cx);
        }))
        .child(Icon::new(icon).xsmall())
        .child(label)
}

fn render_window_controls(window: &Window) -> impl IntoElement {
    let max_icon = if window.is_maximized() {
        IconName::WindowRestore
    } else {
        IconName::WindowMaximize
    };

    h_flex()
        .h_full()
        .flex_shrink_0()
        .items_center()
        .child(window_control_btn(
            "win-min",
            IconName::WindowMinimize,
            WindowControlArea::Min,
            false,
        ))
        .child(window_control_btn(
            "win-max",
            max_icon,
            WindowControlArea::Max,
            false,
        ))
        .child(window_control_btn(
            "win-close",
            IconName::WindowClose,
            WindowControlArea::Close,
            true,
        ))
}

fn window_control_btn(
    id: &'static str,
    icon: IconName,
    area: WindowControlArea,
    is_close: bool,
) -> impl IntoElement {
    div()
        .id(id)
        .w(px(36.))
        .h_full()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(solid(TEXT_MUTED))
        .hover(move |s| {
            if is_close {
                s.bg(solid(STATUS_ERROR)).text_color(solid(BG_BASE))
            } else {
                s.bg(solid(BG_HOVER)).text_color(solid(TEXT_PRIMARY))
            }
        })
        .window_control_area(area)
        .child(Icon::new(icon).small())
}

impl Focusable for Workspace {
    fn focus_handle(&self, _cx: &gpui_kit::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = self.settings.accent;

        let center_and_inspector = if self.show_inspector {
            h_resizable("center-inspector-split")
                .with_handle_appearance(crisp_resize_handle_appearance(accent))
                .child(resizable_panel().child(self.render_center_pane(cx)))
                .child(
                    resizable_panel()
                        .size(px(310.))
                        .size_range(px(240.)..px(480.))
                        .fixed()
                        .child(render_inspector(self, cx)),
                )
                .into_any_element()
        } else {
            self.render_center_pane(cx).into_any_element()
        };

        let body = if self.show_sidebar {
            h_resizable("main-sidebar-split")
                .with_handle_appearance(crisp_resize_handle_appearance(accent))
                .child(
                    resizable_panel()
                        .size(px(248.))
                        .size_range(px(195.)..px(380.))
                        .fixed()
                        .child(render_sidebar(self, cx)),
                )
                .child(resizable_panel().child(center_and_inspector))
                .into_any_element()
        } else {
            center_and_inspector
        };

        div()
            .id("textree-workspace-root")
            .key_context("TexTreeWorkspace")
            .track_focus(&self.focus_handle)
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(solid(BG_BASE))
            .text_color(solid(TEXT_PRIMARY))
            .on_key_down(cx.listener(Self::handle_key_down))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.search_input.update(cx, |s, cx| s.focus(window, cx));
            }))
            .on_action(cx.listener(|this, _: &PickFolder, window, cx| {
                this.pick_folder(window, cx);
            }))
            .on_action(cx.listener(|this, _: &Rescan, _, cx| {
                if !matches!(this.scan_state, ScanState::Scanning(_)) {
                    this.rescan_current(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CancelOrDismiss, window, cx| {
                if this.pending_confirm.is_some() {
                    this.dismiss_confirm(cx);
                } else if this.show_settings {
                    this.show_settings = false;
                    cx.notify();
                } else if matches!(this.scan_state, ScanState::Scanning(_)) {
                    this.stop_scan(cx);
                } else if !this.search_query.is_empty() {
                    this.clear_search(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.show_sidebar = !this.show_sidebar;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleInspector, _, cx| {
                this.show_inspector = !this.show_inspector;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleTreemap, _, cx| {
                this.toggle_treemap(cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| {
                this.show_settings = !this.show_settings;
                cx.notify();
            }))
            .on_action(cx.listener(|_, _: &Quit, _, cx| {
                cx.quit();
            }))
            .child(self.render_header(window, cx))
            .child(div().flex_1().min_h_0().w_full().child(body))
            .when(self.show_settings, |root| {
                root.child(render_settings_modal(self, cx))
            })
            .when_some(self.pending_confirm.as_ref(), |root, action| {
                root.child(render_confirm_modal(action, self.settings.size_unit, cx))
            })
    }
}
