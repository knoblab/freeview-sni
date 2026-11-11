//! 사용자 환경설정(Config) 모듈

use std::fs;
use serde::{Deserialize, Serialize};
use crate::desync::DesyncMode;
use crate::guard::get_app_dir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub port: u16,
    pub delay_ms: u64,
    pub mode_str: String,
    pub autostart: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            delay_ms: 5,
            mode_str: "TlsHeader5".to_string(),
            autostart: true,
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

    pub fn load() -> Self {
        let path = get_app_dir().join("config.json");
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<AppConfig>(&content) {
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
