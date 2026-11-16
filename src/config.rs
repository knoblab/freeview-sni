//! 사용자 환경설정(Config) 모듈

use std::fs;
use serde::{Deserialize, Serialize};
use crate::desync::DesyncMode;
use crate::guard::get_app_dir;

const CURRENT_CONFIG_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub port: u16,
    pub delay_ms: u64,
    pub mode_str: String,
    pub autostart: bool,
    #[serde(default)]
    pub debug_logs: bool,
    #[serde(default)]
    pub config_version: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            delay_ms: 2,
            mode_str: "TlsHeader5".to_string(),
            autostart: true,
            debug_logs: false,
            config_version: CURRENT_CONFIG_VERSION,
        }
    }
}

impl AppConfig {
    pub fn get_desync_mode(&self) -> DesyncMode {
        match self.mode_str.as_str() {
            "SniSplit" => DesyncMode::SniSplit,
            "TlsRecordFrag" => DesyncMode::TlsRecordFrag,
            _ => DesyncMode::TlsHeader5,
        }
    }

    /// 3.2.2 이전 기본값(`TlsHeader5`, `delay_ms: 5`)을 사용하던 설정만 1회 안전하게 `2 ms`로 마이그레이션합니다.
    fn migrate_legacy_defaults(&mut self) -> bool {
        if self.config_version >= CURRENT_CONFIG_VERSION {
            return false;
        }
        if self.mode_str == "TlsHeader5" && self.delay_ms == 5 {
            self.delay_ms = 2;
        }
        self.config_version = CURRENT_CONFIG_VERSION;
        true
    }

    pub fn load() -> Self {
        let path = get_app_dir().join("config.json");
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(mut cfg) = serde_json::from_str::<AppConfig>(&content) {
                if cfg.migrate_legacy_defaults() {
                    cfg.save();
                }
                return cfg;
            }
        }
        let default = Self::default();
        default.save();
        default
    }

    pub fn save(&self) {
        let path = get_app_dir().join("config.json");
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, content);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_migration_from_legacy_default() {
        let legacy_json = r#"{
            "port": 8080,
            "delay_ms": 5,
            "mode_str": "TlsHeader5",
            "autostart": true
        }"#;
        let mut cfg: AppConfig = serde_json::from_str(legacy_json).unwrap();
        assert!(cfg.migrate_legacy_defaults());
        assert_eq!(cfg.delay_ms, 2);
        assert_eq!(cfg.config_version, CURRENT_CONFIG_VERSION);
        assert!(!cfg.migrate_legacy_defaults());
    }

    #[test]
    fn test_config_preserves_custom_settings_during_migration() {
        let custom_json = r#"{
            "port": 8080,
            "delay_ms": 8,
            "mode_str": "SniSplit",
            "autostart": true
        }"#;
        let mut cfg: AppConfig = serde_json::from_str(custom_json).unwrap();
        assert!(cfg.migrate_legacy_defaults());
        assert_eq!(cfg.delay_ms, 8);
        assert_eq!(cfg.mode_str, "SniSplit");
        assert_eq!(cfg.config_version, CURRENT_CONFIG_VERSION);
    }
}
