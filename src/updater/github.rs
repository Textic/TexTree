use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPO_URL: &str = "https://github.com/Textic/TexTree";
const LATEST_RELEASE_API_URL: &str = "https://api.github.com/repos/Textic/TexTree/releases/latest";
const SHA256SUMS_ASSET: &str = "SHA256SUMS.txt";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallTargetKind {
    WindowsSetupInstaller,
    WindowsPortableExe,
    MacOsAppBundle,
    MacOsBareBinary,
    LinuxUserBinary,
    LinuxDebPackage,
    ManualBrowserFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub tag_name: String,
    pub release_url: String,
    pub download_url: String,
    pub asset_name: String,
    pub checksum_url: Option<String>,
    pub published_at: String,
    pub install_kind: InstallTargetKind,
}

impl UpdateInfo {
    pub fn supports_in_app_install(&self) -> bool {
        self.install_kind != InstallTargetKind::ManualBrowserFallback
            && self.download_url.starts_with("https://")
            && !self.asset_name.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(UpdateInfo),
    Downloading {
        info: UpdateInfo,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    Installing(UpdateInfo),
    Failed(String),
}

impl UpdateState {
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading { .. } | Self::Installing(_)
        )
    }

    pub fn has_pending_update(&self) -> bool {
        matches!(
            self,
            Self::Available(_) | Self::Downloading { .. } | Self::Installing(_)
        )
    }
}

#[derive(Debug, Clone)]
pub struct StagedUpdate {
    pub kind: InstallTargetKind,
    pub target_exe: PathBuf,
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    pub target_app: Option<PathBuf>,
    pub source_path: PathBuf,
    pub stage_dir: PathBuf,
}

pub fn format_download_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= MB {
        format!("{:.1} MB", bytes as f64 / MB)
    } else if bytes as f64 >= KB {
        format!("{} KB", (bytes as f64 / KB).round() as u64)
    } else {
        format!("{bytes} B")
    }
}

fn hex_encode_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

pub fn parse_sha256_for_asset(sums_text: &str, asset_name: &str) -> Option<String> {
    if asset_name.is_empty() {
        return None;
    }
    for line in sums_text.lines() {
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else {
            continue;
        };
        let Some(raw_name) = parts.next() else {
            continue;
        };
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let cleaned = raw_name.trim_start_matches('*').trim_start_matches("./");
        let basename = cleaned.rsplit(['/', '\\']).next().unwrap_or(cleaned);
        if cleaned == asset_name || basename == asset_name {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

pub fn app_bundle_root(exe_path: &Path) -> Option<PathBuf> {
    let macos_dir = exe_path.parent()?;
    if macos_dir.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents_dir = macos_dir.parent()?;
    if contents_dir.file_name()?.to_str()? != "Contents" {
        return None;
    }
    let app_dir = contents_dir.parent()?;
    let app_name = app_dir.file_name()?.to_str()?;
    if app_name.ends_with(".app") {
        Some(app_dir.to_path_buf())
    } else {
        None
    }
}

pub fn detect_install_kind_for(
    os: &str,
    exe_path: &Path,
    has_windows_uninstaller: bool,
    target_dir_writable: bool,
    has_pkexec_dpkg: bool,
) -> InstallTargetKind {
    match os {
        "windows" => {
            if has_windows_uninstaller {
                InstallTargetKind::WindowsSetupInstaller
            } else if target_dir_writable {
                InstallTargetKind::WindowsPortableExe
            } else {
                InstallTargetKind::ManualBrowserFallback
            }
        }
        "macos" => {
            if app_bundle_root(exe_path).is_some() {
                if target_dir_writable {
                    InstallTargetKind::MacOsAppBundle
                } else {
                    InstallTargetKind::ManualBrowserFallback
                }
            } else if target_dir_writable {
                InstallTargetKind::MacOsBareBinary
            } else {
                InstallTargetKind::ManualBrowserFallback
            }
        }
        "linux" => {
            if target_dir_writable {
                InstallTargetKind::LinuxUserBinary
            } else if has_pkexec_dpkg {
                InstallTargetKind::LinuxDebPackage
            } else {
                InstallTargetKind::ManualBrowserFallback
            }
        }
        _ => InstallTargetKind::ManualBrowserFallback,
    }
}

pub fn detect_install_kind() -> InstallTargetKind {
    let Ok(exe_path) = std::env::current_exe() else {
        return InstallTargetKind::ManualBrowserFallback;
    };
    let os = std::env::consts::OS;
    let exe_dir = exe_path.parent();
    let has_win_uninstaller = os == "windows"
        && exe_dir
            .map(|d| d.join("unins000.exe").is_file())
            .unwrap_or(false);
    let writable_dir = match os {
        "macos" => app_bundle_root(&exe_path)
            .and_then(|app| app.parent().map(PathBuf::from))
            .or_else(|| exe_dir.map(PathBuf::from)),
        _ => exe_dir.map(PathBuf::from),
    };
    let target_dir_writable = writable_dir.as_deref().is_some_and(is_dir_writable);
    let has_pkexec_dpkg = os == "linux" && command_in_path("pkexec") && command_in_path("dpkg");

    detect_install_kind_for(
        os,
        &exe_path,
        has_win_uninstaller,
        target_dir_writable,
        has_pkexec_dpkg,
    )
}

fn is_dir_writable(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let probe = dir.join(format!(
        ".textree-write-probe-{}-{stamp}",
        std::process::id()
    ));
    match OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(file) => {
            drop(file);
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn command_in_path(program: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| dir.join(program).is_file())
}

pub fn pick_asset_for_install<'a>(
    assets: &'a [(String, String)],
    preferred_kind: InstallTargetKind,
    os: &str,
    arch: &str,
) -> Option<(&'a str, &'a str, InstallTargetKind)> {
    let macos_arch_zip = if arch == "aarch64" {
        "macos-arm64.zip"
    } else {
        "macos-x86_64.zip"
    };
    let macos_arch_dmg = if arch == "aarch64" {
        "macos-arm64.dmg"
    } else {
        "macos-x86_64.dmg"
    };

    let patterns: &[&str] = match (preferred_kind, os) {
        (InstallTargetKind::WindowsSetupInstaller, "windows") => {
            &["windows-x86_64-setup.exe", "windows-x86_64.exe", ".exe"]
        }
        (InstallTargetKind::WindowsPortableExe, "windows") => {
            &["windows-x86_64.exe", "windows-x86_64-setup.exe"]
        }
        (InstallTargetKind::MacOsAppBundle | InstallTargetKind::MacOsBareBinary, "macos") => {
            &[macos_arch_zip, macos_arch_dmg, ".zip", ".dmg"]
        }
        (InstallTargetKind::LinuxUserBinary, "linux") => {
            &["linux-x86_64.tar.gz", ".tar.gz", "linux-x86_64.deb", ".deb"]
        }
        (InstallTargetKind::LinuxDebPackage, "linux") => {
            &["linux-x86_64.deb", ".deb", "linux-x86_64.tar.gz", ".tar.gz"]
        }
        (_, "windows") => &[
            "windows-x86_64-setup.exe",
            "windows-x86_64.exe",
            "windows-x86_64.zip",
            ".exe",
        ],
        (_, "macos") => &[macos_arch_dmg, macos_arch_zip, ".dmg", ".zip"],
        (_, "linux") => &["linux-x86_64.deb", "linux-x86_64.tar.gz", ".deb", ".tar.gz"],
        _ => &[],
    };

    for pat in patterns {
        if let Some((name, url)) = assets.iter().find(|(n, _)| {
            let lower = n.to_ascii_lowercase();
            lower.ends_with(pat) && (pat.contains(os) || lower.contains(os))
        }) {
            let effective_kind = resolve_install_kind_for_asset(name, preferred_kind, os);
            return Some((name.as_str(), url.as_str(), effective_kind));
        }
    }
    None
}

fn resolve_install_kind_for_asset(
    asset_name: &str,
    preferred: InstallTargetKind,
    os: &str,
) -> InstallTargetKind {
    let lower = asset_name.to_ascii_lowercase();
    match os {
        "windows" => {
            if lower.ends_with("-setup.exe") {
                InstallTargetKind::WindowsSetupInstaller
            } else if lower.ends_with(".exe") {
                if preferred == InstallTargetKind::ManualBrowserFallback {
                    InstallTargetKind::ManualBrowserFallback
                } else {
                    InstallTargetKind::WindowsPortableExe
                }
            } else {
                InstallTargetKind::ManualBrowserFallback
            }
        }
        "macos" => {
            if asset_name.ends_with(".zip") {
                match preferred {
                    InstallTargetKind::MacOsAppBundle | InstallTargetKind::MacOsBareBinary => {
                        preferred
                    }
                    _ => InstallTargetKind::ManualBrowserFallback,
                }
            } else {
                InstallTargetKind::ManualBrowserFallback
            }
        }
        "linux" => {
            if asset_name.ends_with(".tar.gz") {
                if preferred == InstallTargetKind::ManualBrowserFallback {
                    InstallTargetKind::ManualBrowserFallback
                } else {
                    InstallTargetKind::LinuxUserBinary
                }
            } else if asset_name.ends_with(".deb") {
                if preferred == InstallTargetKind::LinuxDebPackage {
                    InstallTargetKind::LinuxDebPackage
                } else {
                    InstallTargetKind::ManualBrowserFallback
                }
            } else {
                InstallTargetKind::ManualBrowserFallback
            }
        }
        _ => InstallTargetKind::ManualBrowserFallback,
    }
}

pub async fn download_and_stage_update<F>(
    info: &UpdateInfo,
    mut on_progress: F,
) -> Result<StagedUpdate, String>
where
    F: FnMut(u64, Option<u64>) + Send,
{
    if info.install_kind == InstallTargetKind::ManualBrowserFallback {
        return Err(
            "Automatic install is unavailable for this installation; please download manually."
                .to_string(),
        );
    }
    if !info.download_url.starts_with("https://") {
        return Err("Refusing to download update over an insecure URL.".to_string());
    }

    let target_exe = std::env::current_exe()
        .map_err(|e| format!("Could not resolve current executable path: {e}"))?;
    let target_app = preflight_target_writable(&target_exe, info.install_kind)?;

    let stage_dir = prepare_staging_dir(&info.tag_name)?;
    let client = reqwest::Client::builder()
        .user_agent(format!("TexTree/{CURRENT_VERSION}"))
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let expected_sha256 = fetch_expected_sha256(&client, info).await?;

    let safe_filename = info
        .asset_name
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("textree-update.bin");
    let download_path = stage_dir.join(safe_filename);

    let actual_sha256 =
        stream_asset_with_hash(&client, &info.download_url, &download_path, &mut on_progress)
            .await
            .inspect_err(|_| {
                let _ = fs::remove_dir_all(&stage_dir);
            })?;

    if let Some(expected) = expected_sha256
        && actual_sha256 != expected
    {
        let _ = fs::remove_dir_all(&stage_dir);
        return Err(format!(
            "SHA-256 verification failed for {} (expected {expected}, got {actual_sha256})",
            info.asset_name
        ));
    }

    let source_path = unpack_and_verify_payload(info.install_kind, &download_path, &stage_dir)
        .await
        .inspect_err(|_| {
            let _ = fs::remove_dir_all(&stage_dir);
        })?;

    Ok(StagedUpdate {
        kind: info.install_kind,
        target_exe,
        target_app,
        source_path,
        stage_dir,
    })
}

fn preflight_target_writable(
    target_exe: &Path,
    kind: InstallTargetKind,
) -> Result<Option<PathBuf>, String> {
    match kind {
        InstallTargetKind::WindowsSetupInstaller => Ok(None),
        InstallTargetKind::WindowsPortableExe | InstallTargetKind::LinuxUserBinary => {
            let dir = target_exe
                .parent()
                .ok_or_else(|| "Current executable has no parent directory.".to_string())?;
            if !is_dir_writable(dir) {
                return Err(format!(
                    "Target folder `{}` is not writable; download manually from Releases.",
                    dir.display()
                ));
            }
            Ok(None)
        }
        InstallTargetKind::MacOsAppBundle => {
            let app_root = app_bundle_root(target_exe)
                .ok_or_else(|| "Current executable is not inside a .app bundle.".to_string())?;
            let parent = app_root
                .parent()
                .ok_or_else(|| "Application bundle has no parent directory.".to_string())?;
            if !is_dir_writable(parent) {
                return Err(format!(
                    "Application folder `{}` is not writable; download the DMG manually.",
                    parent.display()
                ));
            }
            Ok(Some(app_root))
        }
        InstallTargetKind::MacOsBareBinary => {
            let dir = target_exe
                .parent()
                .ok_or_else(|| "Current executable has no parent directory.".to_string())?;
            if !is_dir_writable(dir) {
                return Err(format!(
                    "Target folder `{}` is not writable; download the DMG manually.",
                    dir.display()
                ));
            }
            Ok(None)
        }
        InstallTargetKind::LinuxDebPackage => {
            if !command_in_path("pkexec") || !command_in_path("dpkg") {
                return Err(
                    "System package update requires `pkexec` and `dpkg`; download manually."
                        .to_string(),
                );
            }
            Ok(None)
        }
        InstallTargetKind::ManualBrowserFallback => {
            Err("Automatic installation is unavailable for this installation.".to_string())
        }
    }
}

fn updates_root_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("TexTree")
        .join("updates")
}

fn prepare_staging_dir(tag_name: &str) -> Result<PathBuf, String> {
    let root = updates_root_dir();
    fs::create_dir_all(&root).map_err(|e| format!("Failed to create updates cache dir: {e}"))?;

    if let Ok(entries) = fs::read_dir(&root) {
        let now = SystemTime::now();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let is_stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|mtime| now.duration_since(mtime).ok())
                .is_some_and(|age| age > Duration::from_secs(3600));
            if is_stale {
                let _ = fs::remove_dir_all(path);
            }
        }
    }

    let safe_tag: String = tag_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let stage = root.join(format!("stage-{safe_tag}-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&stage).map_err(|e| format!("Failed to create staging dir: {e}"))?;
    Ok(stage)
}

async fn fetch_expected_sha256(
    client: &reqwest::Client,
    info: &UpdateInfo,
) -> Result<Option<String>, String> {
    let Some(sum_url) = info.checksum_url.as_deref() else {
        return Ok(None);
    };
    if !sum_url.starts_with("https://") {
        return Err("Refusing checksum download over an insecure URL.".to_string());
    }
    let resp = client
        .get(sum_url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch SHA256SUMS.txt: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Failed to fetch SHA256SUMS.txt (HTTP {})",
            resp.status().as_u16()
        ));
    }
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read SHA256SUMS.txt: {e}"))?;
    let expected = parse_sha256_for_asset(&text, &info.asset_name).ok_or_else(|| {
        format!(
            "SHA256SUMS.txt did not contain an entry for {}",
            info.asset_name
        )
    })?;
    Ok(Some(expected))
}

async fn stream_asset_with_hash<F>(
    client: &reqwest::Client,
    url: &str,
    dest_path: &Path,
    on_progress: &mut F,
) -> Result<String, String>
where
    F: FnMut(u64, Option<u64>),
{
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Failed to download update: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Update download failed with HTTP {}",
            resp.status().as_u16()
        ));
    }

    let total_bytes = resp.content_length();
    on_progress(0, total_bytes);

    let mut file = tokio::fs::File::create(dest_path)
        .await
        .map_err(|e| format!("Failed to create temporary installer file: {e}"))?;
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;
    let mut last_emit = Instant::now();
    let mut stream = resp.bytes_stream();

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| format!("Download interrupted: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Failed writing update file: {e}"))?;
        hasher.update(&chunk);
        downloaded = downloaded.saturating_add(chunk.len() as u64);

        if last_emit.elapsed() >= Duration::from_millis(80) {
            on_progress(downloaded, total_bytes);
            last_emit = Instant::now();
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("Failed flushing update file: {e}"))?;
    file.sync_all()
        .await
        .map_err(|e| format!("Failed syncing update file: {e}"))?;
    on_progress(downloaded, total_bytes.or(Some(downloaded)));

    Ok(hex_encode_lower(&hasher.finalize()))
}

async fn unpack_and_verify_payload(
    kind: InstallTargetKind,
    download_path: &Path,
    stage_dir: &Path,
) -> Result<PathBuf, String> {
    match kind {
        InstallTargetKind::WindowsSetupInstaller | InstallTargetKind::WindowsPortableExe => {
            verify_windows_pe_header(download_path)?;
            Ok(download_path.to_path_buf())
        }
        InstallTargetKind::LinuxDebPackage => {
            let size = fs::metadata(download_path).map(|m| m.len()).unwrap_or(0);
            if size == 0 {
                return Err("Downloaded .deb package is empty.".to_string());
            }
            Ok(download_path.to_path_buf())
        }
        InstallTargetKind::MacOsAppBundle | InstallTargetKind::MacOsBareBinary => {
            let zip = download_path.to_path_buf();
            let extract_dir = stage_dir.join("extracted");
            let extract_clone = extract_dir.clone();
            tokio::task::spawn_blocking(move || extract_zip_ditto(&zip, &extract_clone))
                .await
                .map_err(|e| format!("Archive extraction task failed: {e}"))??;

            let app_path = extract_dir.join("TexTree.app");
            let bin_path = app_path.join("Contents").join("MacOS").join("textree");
            if !bin_path.is_file() {
                return Err("Update archive did not contain TexTree.app binary.".to_string());
            }
            if kind == InstallTargetKind::MacOsAppBundle {
                Ok(app_path)
            } else {
                Ok(bin_path)
            }
        }
        InstallTargetKind::LinuxUserBinary => {
            let tar_path = download_path.to_path_buf();
            let extract_dir = stage_dir.join("extracted");
            let extract_clone = extract_dir.clone();
            tokio::task::spawn_blocking(move || extract_tar_gz(&tar_path, &extract_clone))
                .await
                .map_err(|e| format!("Archive extraction task failed: {e}"))??;

            let bin_path = extract_dir.join("textree");
            if !bin_path.is_file() {
                return Err("Update archive did not contain `textree` binary.".to_string());
            }
            Ok(bin_path)
        }
        InstallTargetKind::ManualBrowserFallback => {
            Err("Automatic installation is unavailable for this installation.".to_string())
        }
    }
}

fn verify_windows_pe_header(path: &Path) -> Result<(), String> {
    use std::io::Read;
    let mut f = fs::File::open(path).map_err(|e| format!("Cannot read downloaded file: {e}"))?;
    let mut magic = [0u8; 2];
    f.read_exact(&mut magic)
        .map_err(|_| "Downloaded Windows executable is truncated or invalid.".to_string())?;
    if &magic != b"MZ" {
        return Err("Downloaded file is not a valid Windows executable.".to_string());
    }
    Ok(())
}

fn extract_zip_ditto(zip_path: &Path, extract_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(extract_dir)
        .map_err(|e| format!("Failed to create extraction directory: {e}"))?;
    let status = Command::new("ditto")
        .arg("-x")
        .arg("-k")
        .arg(zip_path)
        .arg(extract_dir)
        .status()
        .map_err(|e| format!("Failed to run `ditto` to extract update archive: {e}"))?;
    if !status.success() {
        return Err(format!("`ditto` exited with status {status}"));
    }
    Ok(())
}

fn extract_tar_gz(tar_path: &Path, extract_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(extract_dir)
        .map_err(|e| format!("Failed to create extraction directory: {e}"))?;
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(tar_path)
        .arg("-C")
        .arg(extract_dir)
        .status()
        .map_err(|e| format!("Failed to run `tar` to extract update archive: {e}"))?;
    if !status.success() {
        return Err(format!("`tar` exited with status {status}"));
    }
    Ok(())
}

pub fn spawn_detached_updater(staged: &StagedUpdate) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        spawn_windows_updater(staged)
    }
    #[cfg(not(target_os = "windows"))]
    {
        spawn_unix_updater(staged)
    }
}

#[cfg(target_os = "windows")]
fn spawn_windows_updater(staged: &StagedUpdate) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;

    let script_path = staged.stage_dir.join("apply-update.ps1");
    let log_path = updates_root_dir().join("last-update.log");
    let install_dir = staged
        .target_exe
        .parent()
        .ok_or_else(|| "Current executable has no parent directory.".to_string())?
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_string();

    let mode = match staged.kind {
        InstallTargetKind::WindowsSetupInstaller => "setup",
        InstallTargetKind::WindowsPortableExe => "portable",
        _ => return Err("Unsupported Windows self-update mode.".to_string()),
    };

    let ps1 = r#"$ErrorActionPreference = 'Stop'
$pidToWait = [int]$env:TEXTREE_PID
$targetExe = $env:TEXTREE_TARGET_EXE
$sourceFile = $env:TEXTREE_SOURCE_FILE
$installDir = $env:TEXTREE_INSTALL_DIR
$stageDir = $env:TEXTREE_STAGE_DIR
$mode = $env:TEXTREE_MODE
$logFile = $env:TEXTREE_LOG_FILE

try {
    for ($i = 0; $i -lt 150; $i++) {
        if (-not (Get-Process -Id $pidToWait -ErrorAction SilentlyContinue)) {
            break
        }
        Start-Sleep -Milliseconds 200
    }
    Start-Sleep -Milliseconds 300

    if ($mode -eq 'setup') {
        $dirArg = '/DIR="' + $installDir + '"'
        $proc = Start-Process -FilePath $sourceFile -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', $dirArg) -Wait -PassThru
        if ($proc.ExitCode -ne 0) {
            throw "Installer exited with code $($proc.ExitCode)"
        }
    } else {
        $copied = $false
        for ($attempt = 0; $attempt -lt 25; $attempt++) {
            try {
                Copy-Item -LiteralPath $sourceFile -Destination $targetExe -Force
                $copied = $true
                break
            } catch {
                Start-Sleep -Milliseconds 200
            }
        }
        if (-not $copied) {
            throw "Failed to replace $targetExe"
        }
    }

    if (Test-Path -LiteralPath $targetExe) {
        Start-Process -FilePath $targetExe -WorkingDirectory $installDir
    }

    Start-Sleep -Milliseconds 500
    if ($stageDir -and (Test-Path -LiteralPath $stageDir)) {
        Remove-Item -LiteralPath $stageDir -Recurse -Force -ErrorAction SilentlyContinue
    }
} catch {
    try {
        "$(Get-Date -Format o) UPDATE FAILED: $_" | Out-File -LiteralPath $logFile -Encoding utf8
    } catch {}
    exit 1
}
"#;
    fs::write(&script_path, ps1)
        .map_err(|e| format!("Failed to write update helper script: {e}"))?;

    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-File",
        ])
        .arg(&script_path)
        .env("TEXTREE_PID", std::process::id().to_string())
        .env("TEXTREE_TARGET_EXE", &staged.target_exe)
        .env("TEXTREE_SOURCE_FILE", &staged.source_path)
        .env("TEXTREE_INSTALL_DIR", install_dir)
        .env("TEXTREE_STAGE_DIR", &staged.stage_dir)
        .env("TEXTREE_MODE", mode)
        .env("TEXTREE_LOG_FILE", log_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
        .map_err(|e| format!("Failed to launch background updater: {e}"))?;

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn spawn_unix_updater(staged: &StagedUpdate) -> Result<(), String> {
    let script_path = staged.stage_dir.join("apply-update.sh");
    let log_path = updates_root_dir().join("last-update.log");
    let mode = match staged.kind {
        InstallTargetKind::MacOsAppBundle => "macos_app",
        InstallTargetKind::MacOsBareBinary => "macos_bin",
        InstallTargetKind::LinuxUserBinary => "linux_bin",
        InstallTargetKind::LinuxDebPackage => "linux_deb",
        _ => return Err("Unsupported Unix self-update mode.".to_string()),
    };

    let sh_script = r#"#!/bin/sh
set -eu
trap '' HUP

i=0
while [ "$i" -lt 150 ]; do
  if ! kill -0 "$TEXTREE_PID" 2>/dev/null; then
    break
  fi
  sleep 0.2 2>/dev/null || sleep 1
  i=$((i + 1))
done
sleep 0.3 2>/dev/null || sleep 1

case "$TEXTREE_MODE" in
  macos_app)
    BACKUP_APP="${TEXTREE_TARGET_APP}.old.$$"
    if [ -d "$TEXTREE_TARGET_APP" ]; then
      mv "$TEXTREE_TARGET_APP" "$BACKUP_APP"
    fi
    if ditto "$TEXTREE_SOURCE_PATH" "$TEXTREE_TARGET_APP"; then
      rm -rf "$BACKUP_APP"
    else
      if [ -d "$BACKUP_APP" ] && [ ! -e "$TEXTREE_TARGET_APP" ]; then
        mv "$BACKUP_APP" "$TEXTREE_TARGET_APP"
      fi
      echo "UPDATE FAILED: ditto copy failed" >"$TEXTREE_LOG_FILE" 2>/dev/null || true
      exit 1
    fi
    xattr -dr com.apple.quarantine "$TEXTREE_TARGET_APP" 2>/dev/null || true
    open -n "$TEXTREE_TARGET_APP"
    ;;
  macos_bin|linux_bin)
    TMP_TARGET="${TEXTREE_TARGET_EXE}.tmp.$$"
    cp -f "$TEXTREE_SOURCE_PATH" "$TMP_TARGET"
    chmod 755 "$TMP_TARGET"
    mv -f "$TMP_TARGET" "$TEXTREE_TARGET_EXE"
    "$TEXTREE_TARGET_EXE" >/dev/null 2>&1 &
    ;;
  linux_deb)
    if pkexec dpkg -i "$TEXTREE_SOURCE_PATH" >"$TEXTREE_LOG_FILE" 2>&1; then
      "$TEXTREE_TARGET_EXE" >/dev/null 2>&1 &
    else
      exit 1
    fi
    ;;
  *)
    exit 1
    ;;
esac

rm -rf "$TEXTREE_STAGE_DIR" 2>/dev/null || true
"#;
    fs::write(&script_path, sh_script)
        .map_err(|e| format!("Failed to write update helper script: {e}"))?;

    Command::new("sh")
        .arg(&script_path)
        .env("TEXTREE_PID", std::process::id().to_string())
        .env("TEXTREE_TARGET_EXE", &staged.target_exe)
        .env(
            "TEXTREE_TARGET_APP",
            staged.target_app.as_deref().unwrap_or(Path::new("")),
        )
        .env("TEXTREE_SOURCE_PATH", &staged.source_path)
        .env("TEXTREE_STAGE_DIR", &staged.stage_dir)
        .env("TEXTREE_MODE", mode)
        .env("TEXTREE_LOG_FILE", log_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to launch background updater: {e}"))?;

    Ok(())
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

pub fn parse_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    let core = trimmed.split(['-', '+']).next()?.trim();
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    Some((major, minor, patch))
}

pub fn is_newer_version(current: &str, latest: &str) -> bool {
    match (parse_semver(current), parse_semver(latest)) {
        (Some(cur), Some(lat)) => lat > cur,
        _ => false,
    }
}

pub async fn check_for_updates() -> Result<Option<UpdateInfo>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(format!("TexTree/{CURRENT_VERSION}"))
        .build()
        .map_err(|e| format!("Failed to initialize HTTP client: {e}"))?;

    let resp = client
        .get(LATEST_RELEASE_API_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    if resp.status().as_u16() == 404 {
        return Ok(None);
    }

    if !resp.status().is_success() {
        return Err(format!(
            "GitHub API returned HTTP {}",
            resp.status().as_u16()
        ));
    }

    let rel: GithubRelease = resp
        .json()
        .await
        .map_err(|e| format!("Invalid GitHub release payload: {e}"))?;

    if rel.draft || rel.prerelease || !is_newer_version(CURRENT_VERSION, &rel.tag_name) {
        return Ok(None);
    }

    let version = rel
        .tag_name
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();
    let pairs: Vec<(String, String)> = rel
        .assets
        .iter()
        .map(|a| (a.name.clone(), a.browser_download_url.clone()))
        .collect();
    let preferred_kind = detect_install_kind();
    let (asset_name, download_url, install_kind) = match pick_asset_for_install(
        &pairs,
        preferred_kind,
        std::env::consts::OS,
        std::env::consts::ARCH,
    ) {
        Some((name, url, kind)) => (name.to_string(), url.to_string(), kind),
        None => (
            String::new(),
            rel.html_url.clone(),
            InstallTargetKind::ManualBrowserFallback,
        ),
    };
    let checksum_url = pairs
        .iter()
        .find(|(name, _)| name == SHA256SUMS_ASSET)
        .map(|(_, url)| url.clone());

    let published_at = rel
        .published_at
        .as_deref()
        .and_then(|s| s.split('T').next())
        .unwrap_or("")
        .to_string();

    Ok(Some(UpdateInfo {
        version,
        tag_name: rel.tag_name,
        release_url: rel.html_url,
        download_url,
        asset_name,
        checksum_url,
        published_at,
        install_kind,
    }))
}

pub fn open_url_in_browser(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("Refusing to open non-HTTP URL.".to_string());
    }

    #[cfg(target_os = "windows")]
    let status = Command::new("explorer.exe").arg(trimmed).spawn();
    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(trimmed).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let status = Command::new("xdg-open").arg(trimmed).spawn();

    status
        .map(|_| ())
        .map_err(|e| format!("Failed to open browser: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semver_and_picks_assets() {
        assert_eq!(parse_semver("v0.1.0"), Some((0, 1, 0)));
        assert!(is_newer_version("0.1.0", "v0.1.1"));
        assert!(!is_newer_version("0.1.0", "v0.1.0"));

        let assets = vec![
            (
                "textree-v0.1.1-windows-x86_64.exe".to_string(),
                "https://example.com/portable.exe".to_string(),
            ),
            (
                "textree-v0.1.1-windows-x86_64-setup.exe".to_string(),
                "https://example.com/setup.exe".to_string(),
            ),
            (
                "textree-v0.1.1-linux-x86_64.tar.gz".to_string(),
                "https://example.com/linux.tar.gz".to_string(),
            ),
            (
                "textree-v0.1.1-macos-arm64.zip".to_string(),
                "https://example.com/mac.zip".to_string(),
            ),
        ];

        assert_eq!(
            pick_asset_for_install(&assets, InstallTargetKind::WindowsSetupInstaller, "windows", "x86_64"),
            Some((
                "textree-v0.1.1-windows-x86_64-setup.exe",
                "https://example.com/setup.exe",
                InstallTargetKind::WindowsSetupInstaller,
            ))
        );
        assert_eq!(
            pick_asset_for_install(&assets, InstallTargetKind::WindowsPortableExe, "windows", "x86_64"),
            Some((
                "textree-v0.1.1-windows-x86_64.exe",
                "https://example.com/portable.exe",
                InstallTargetKind::WindowsPortableExe,
            ))
        );
    }

    #[test]
    fn parses_sha256_entries() {
        let sums = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  textree-v0.1.1-windows-x86_64.exe\n";
        assert_eq!(
            parse_sha256_for_asset(sums, "textree-v0.1.1-windows-x86_64.exe"),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_string())
        );
        assert_eq!(
            parse_sha256_for_asset(sums, "textree-v0.1.1-linux-x86_64.deb"),
            None
        );
    }
}
