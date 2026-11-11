//! 인메모리 링버퍼 및 파일 로깅 모듈

use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

use crate::guard::get_app_dir;

const MAX_LOG_LINES: usize = 500;

struct LogState {
    lines: VecDeque<String>,
    app_handle: Option<AppHandle>,
}

static LOG_STATE: Mutex<Option<LogState>> = Mutex::new(None);

pub fn init_logger() {
    let mut lock = LOG_STATE.lock().unwrap();
    *lock = Some(LogState {
        lines: VecDeque::with_capacity(MAX_LOG_LINES),
        app_handle: None,
    });
}

pub fn set_app_handle(handle: AppHandle) {
    if let Ok(mut lock) = LOG_STATE.lock() {
        if let Some(state) = lock.as_mut() {
            state.app_handle = Some(handle);
        }
    }
}

pub fn add_log(msg: &str) {
    let now = get_local_time_string();
    let formatted = format!("[{}] {}", now, msg);

    // 파일 로깅
    let log_path = get_app_dir().join("app.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = writeln!(file, "{}", formatted);
    }

    // 인메모리 링버퍼 및 Tauri UI 실시간 이벤트 발송
    let mut handle_to_emit = None;
    if let Ok(mut lock) = LOG_STATE.lock() {
        if let Some(state) = lock.as_mut() {
            if state.lines.len() >= MAX_LOG_LINES {
                state.lines.pop_front();
            }
            state.lines.push_back(formatted.clone());
            handle_to_emit = state.app_handle.clone();
        }
    }

    if let Some(app) = handle_to_emit {
        let _ = app.emit("log-update", formatted);
    }
}

pub fn get_recent_logs() -> Vec<String> {
    if let Ok(lock) = LOG_STATE.lock() {
        if let Some(state) = lock.as_ref() {
            return state.lines.iter().cloned().collect();
        }
    }
    Vec::new()
}

fn get_local_time_string() -> String {
    #[repr(C)]
    struct SystemTime {
        w_year: u16,
        w_month: u16,
        w_day_of_week: u16,
        w_day: u16,
        w_hour: u16,
        w_minute: u16,
        w_second: u16,
        w_milliseconds: u16,
    }

    unsafe extern "system" {
        fn GetLocalTime(lpSystemTime: *mut SystemTime);
    }

    let mut st = SystemTime {
        w_year: 0,
        w_month: 0,
        w_day_of_week: 0,
        w_day: 0,
        w_hour: 0,
        w_minute: 0,
        w_second: 0,
        w_milliseconds: 0,
    };

    unsafe {
        GetLocalTime(&mut st);
    }

    format!("{:02}:{:02}:{:02}", st.w_hour, st.w_minute, st.w_second)
}
