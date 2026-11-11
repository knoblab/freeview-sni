//! Windows 시작 프로그램(HKCU Run) 등록 및 해제 모듈

use std::io;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APP_NAME: &str = "FreeView";

/// 현재 자동 시작이 등록되어 있는지 확인합니다.
pub fn is_autostart_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags(RUN_KEY_PATH, KEY_READ) {
        let val: Result<String, _> = key.get_value(APP_NAME);
        return val.is_ok();
    }
    false
}

/// 시작 프로그램 자동 실행을 설정하거나 해제합니다.
pub fn set_autostart(enable: bool) -> io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(RUN_KEY_PATH)?;

    if enable {
        if let Ok(exe_path) = std::env::current_exe() {
            let cmd = format!("\"{}\" --startup", exe_path.to_string_lossy());
            key.set_value(APP_NAME, &cmd)?;
        }
    } else {
        let _ = key.delete_value(APP_NAME);
    }

    Ok(())
}
