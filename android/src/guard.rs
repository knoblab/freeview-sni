//! 비정상 종료 방어(Crash Guard) 및 자가 복구(Self-Healing) 모듈

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use serde::{Deserialize, Serialize};

use crate::sysproxy::{self, ProxySnapshot};

static APP_DIR: OnceLock<PathBuf> = OnceLock::new();
static ACTIVE_SNAPSHOT: Mutex<Option<ProxySnapshot>> = Mutex::new(None);
static NEEDS_RESTORE: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ServiceState {
    pub running: bool,
    pub pid: u32,
    pub snapshot: Option<ProxySnapshot>,
    pub timestamp: u64,
}

pub fn set_app_dir(path: PathBuf) {
    let _ = fs::create_dir_all(&path);
    let _ = APP_DIR.set(path);
}

pub fn get_app_dir() -> PathBuf {
    APP_DIR
        .get_or_init(|| {
            let base = match std::env::var_os("LOCALAPPDATA") {
                Some(val) => PathBuf::from(val),
                None => std::env::temp_dir(),
            };
            let dir = base.join("FreeView");
            let _ = fs::create_dir_all(&dir);
            dir
        })
        .clone()
}

pub fn get_state_file_path() -> PathBuf {
    get_app_dir().join("state.json")
}

pub fn save_state(running: bool, snapshot: Option<ProxySnapshot>) {
    NEEDS_RESTORE.store(running, Ordering::SeqCst);

    if let Ok(mut lock) = ACTIVE_SNAPSHOT.lock() {
        *lock = snapshot.clone();
    }

    let state = ServiceState {
        running,
        pid: std::process::id(),
        snapshot,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };

    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let _ = fs::write(get_state_file_path(), json);
    }
}

pub fn load_state() -> ServiceState {
    if let Ok(content) = fs::read_to_string(get_state_file_path()) {
        if let Ok(state) = serde_json::from_str::<ServiceState>(&content) {
            return state;
        }
    }
    ServiceState::default()
}

/// 앱 시작 시 이전 비정상 종료 여부를 검사하고 자동 복구합니다.
pub fn startup_self_heal() -> bool {
    let state = load_state();
    if state.running {
        let _ = sysproxy::restore_proxy(state.snapshot.as_ref());
        save_state(false, None);
        true
    } else {
        save_state(false, None);
        false
    }
}

/// 정상 종료 또는 비상 종료 시 미복원 상태인 경우에만 프록시 설정을 동기식으로 원복합니다.
pub fn emergency_restore() {
    if !NEEDS_RESTORE.swap(false, Ordering::SeqCst) {
        return;
    }
    let snapshot = ACTIVE_SNAPSHOT.lock().ok().and_then(|g| g.clone());
    let _ = sysproxy::restore_proxy(snapshot.as_ref());
    save_state(false, None);
}

/// 패닉 훅 등록: 메인 스레드 치명적 패닉 시에만 프록시 설정을 원복하도록 방어
pub fn register_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("unnamed");
        if thread_name == "main" {
            emergency_restore();
        }
        default_hook(info);
    }));
}
