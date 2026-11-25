use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AccentPreset {
    #[default]
    Emerald,
    Violet,
    Monochrome,
}

impl AccentPreset {
    pub const ALL: [Self; 3] = [Self::Emerald, Self::Violet, Self::Monochrome];

    pub fn label(self) -> &'static str {
        match self {
            Self::Emerald => "Obsidian Emerald",
            Self::Violet => "Obsidian Violet",
            Self::Monochrome => "Obsidian Monochrome",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Emerald => "Technical emerald highlight (#34D399) for crisp status contrast.",
            Self::Violet => "Modern violet highlight (#A78BFA) over pure carbon surfaces.",
            Self::Monochrome => "Pure neutral luminance (#E4E4E7) with zero chromatic distraction.",
        }
    }

    pub fn primary_hex(self) -> u32 {
        match self {
            Self::Emerald => 0x34d399,
            Self::Violet => 0xa78bfa,
            Self::Monochrome => 0xe4e4e7,
        }
    }

    pub fn hover_hex(self) -> u32 {
        match self {
            Self::Emerald => 0x10b981,
            Self::Violet => 0x8b5cf6,
            Self::Monochrome => 0xd4d4d8,
        }
    }

    pub fn active_hex(self) -> u32 {
        match self {
            Self::Emerald => 0x059669,
            Self::Violet => 0x7c3aed,
            Self::Monochrome => 0xa1a1aa,
        }
    }

    pub fn text_on_accent_hex(self) -> u32 {
        0x09090b
    }

    pub fn hex_tag(self) -> &'static str {
        match self {
            Self::Emerald => "#34D399",
            Self::Violet => "#A78BFA",
            Self::Monochrome => "#E4E4E7",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SizeUnit {
    #[default]
    Binary,
    Decimal,
}

impl SizeUnit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Binary => "Binary (KiB / MiB / GiB — 1024)",
            Self::Decimal => "Decimal (KB / MB / GB — 1000)",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub accent: AccentPreset,
    #[serde(default)]
    pub size_unit: SizeUnit,
    #[serde(default = "default_true")]
    pub show_hidden_files: bool,
    #[serde(default = "default_true")]
    pub confirm_before_trash: bool,
    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
}

fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            accent: AccentPreset::Emerald,
            size_unit: SizeUnit::Binary,
            show_hidden_files: true,
            confirm_before_trash: true,
            auto_check_updates: true,
        }
    }
}

impl AppSettings {
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("TexTree")
            .join("settings.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        let Ok(raw) = fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(&path, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_obsidian_emerald_and_roundtrips_json() {
        let defaults = AppSettings::default();
        assert_eq!(defaults.accent, AccentPreset::Emerald);
        assert_eq!(defaults.accent.primary_hex(), 0x34d399);
        assert!(defaults.show_hidden_files);
        assert!(defaults.confirm_before_trash);
        assert!(defaults.auto_check_updates);

        let custom = AppSettings {
            accent: AccentPreset::Violet,
            size_unit: SizeUnit::Decimal,
            show_hidden_files: false,
            confirm_before_trash: true,
            auto_check_updates: false,
        };
        let json = serde_json::to_string(&custom).unwrap();
        let parsed: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.accent, AccentPreset::Violet);
        assert_eq!(parsed.size_unit, SizeUnit::Decimal);
        assert!(!parsed.show_hidden_files);
        assert!(!parsed.auto_check_updates);
    }
}
