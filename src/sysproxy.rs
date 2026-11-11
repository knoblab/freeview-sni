//! Windows 시스템 프록시 설정 및 WinINet API 동기화 모듈

use std::io;
use serde::{Deserialize, Serialize};
use windows_sys::Win32::Networking::WinInet::{
    InternetSetOptionW, INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED,
};
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

const INET_SETTINGS_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProxySnapshot {
    pub proxy_enable: Option<u32>,
    pub proxy_server: Option<String>,
    pub proxy_override: Option<String>,
    pub auto_config_url: Option<String>,
}

impl ProxySnapshot {
    /// 현재 Windows 레지스트리에 저장된 프록시 설정 스냅샷을 캡처합니다.
    /// (FreeView 이전 세션의 잔여값은 스냅샷에서 안전하게 필터링)
    pub fn capture() -> io::Result<Self> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu.open_subkey_with_flags(INET_SETTINGS_PATH, KEY_READ)?;

        let proxy_enable = key.get_value("ProxyEnable").ok();
        let mut proxy_server: Option<String> = key.get_value("ProxyServer").ok();
        let proxy_override: Option<String> = key.get_value("ProxyOverride").ok();
        let mut auto_config_url: Option<String> = key.get_value("AutoConfigURL").ok();

        // 1. FreeView 이전 세션 잔류 ProxyServer 필터링 (127.0.0.1:808X)
        if let Some(ref s) = proxy_server {
            if s.starts_with("127.0.0.1:808") {
                proxy_server = None;
            }
        }

        // 2. 2.1.1 버전의 잘못된 file:// PAC 잔여물 필터링
        if let Some(ref url) = auto_config_url {
            let lower = url.to_lowercase();
            if lower.contains("freeview_proxy.pac") || lower.starts_with("file://") {
                auto_config_url = None;
            }
        }

        Ok(Self {
            proxy_enable,
            proxy_server,
            proxy_override,
            auto_config_url,
        })
    }
}

/// WinINet API를 호출하여 변경된 프록시 설정을 OS 전역(브라우저, 서비스)에 즉각 반영합니다.
pub fn refresh_wininet() {
    unsafe {
        InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_SETTINGS_CHANGED,
            std::ptr::null_mut(),
            0,
        );
        InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_REFRESH,
            std::ptr::null_mut(),
            0,
        );
    }
}

/// FreeView 프록시 설정을 Windows에 적용합니다.
pub fn apply_proxy(port: u16) -> io::Result<ProxySnapshot> {
    let snapshot = ProxySnapshot::capture().unwrap_or_default();

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(INET_SETTINGS_PATH)?;

    // 1. 이전 버전(2.1.1)의 AutoConfigURL 키 완전 삭제
    let _ = key.delete_value("AutoConfigURL");

    // 2. 수동 루프백 프록시 설정
    key.set_value("ProxyServer", &format!("127.0.0.1:{}", port))?;
    key.set_value(
        "ProxyOverride",
        &"<local>;localhost;127.*;10.*;172.16.*;192.168.*",
    )?;
    key.set_value("ProxyEnable", &1u32)?;

    // 3. WinINet 즉시 동기화
    refresh_wininet();

    Ok(snapshot)
}

/// 이전 스냅샷 상태로 Windows 프록시 설정을 완전히 복원합니다.
pub fn restore_proxy(snapshot: Option<&ProxySnapshot>) -> io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(INET_SETTINGS_PATH)?;

    if let Some(snap) = snapshot {
        if let Some(enable) = snap.proxy_enable {
            let _ = key.set_value("ProxyEnable", &enable);
        } else {
            let _ = key.set_value("ProxyEnable", &0u32);
        }

        if let Some(ref server) = snap.proxy_server {
            let _ = key.set_value("ProxyServer", server);
        } else {
            let _ = key.delete_value("ProxyServer");
        }

        if let Some(ref overrides) = snap.proxy_override {
            let _ = key.set_value("ProxyOverride", overrides);
        } else {
            let _ = key.delete_value("ProxyOverride");
        }

        if let Some(ref auto_url) = snap.auto_config_url {
            let _ = key.set_value("AutoConfigURL", auto_url);
        } else {
            let _ = key.delete_value("AutoConfigURL");
        }
    } else {
        let _ = key.set_value("ProxyEnable", &0u32);
        let _ = key.delete_value("AutoConfigURL");
    }

    refresh_wininet();
    Ok(())
}
