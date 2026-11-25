use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Optical,
    RamDisk,
    Unknown,
}

impl DriveKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fixed => "Local Disk",
            Self::Removable => "Removable",
            Self::Network => "Network",
            Self::Optical => "Optical",
            Self::RamDisk => "RAM Disk",
            Self::Unknown => "Drive",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeInfo {
    /// Root path, e.g. `C:\` on Windows or `/` on Unix.
    pub root_path: PathBuf,
    /// Short drive label shown in badges, e.g. `C:` or `/`.
    pub short_name: String,
    /// OS volume label, e.g. `DEV`, `Kingston 500 SSD`.
    pub label: String,
    /// File system name, e.g. `NTFS`, `exFAT`, `APFS`, `ext4`.
    pub file_system: String,
    pub kind: DriveKind,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// Allocation cluster size in bytes (used to estimate size on disk accurately).
    pub cluster_size: u64,
}

impl VolumeInfo {
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }

    pub fn used_fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.used_bytes() as f64 / self.total_bytes as f64).clamp(0.0, 1.0) as f32
        }
    }

    pub fn display_title(&self) -> String {
        if self.label.is_empty() {
            format!("{} ({})", self.short_name, self.kind.label())
        } else {
            format!("{} {}", self.short_name, self.label)
        }
    }
}
