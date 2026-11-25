pub mod github;
pub mod runtime;

pub use github::{
    CURRENT_VERSION, REPO_URL, UpdateInfo, UpdateState, check_for_updates,
    download_and_stage_update, format_download_bytes, open_url_in_browser, spawn_detached_updater,
};
pub use runtime::spawn_tokio;
