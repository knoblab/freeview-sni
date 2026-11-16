//! 인메모리 링버퍼 및 로테이션 파일 로깅 모듈

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use crate::guard::get_app_dir;

const MAX_LOG_LINES: usize = 200;
const MAX_LOG_FILE_BYTES: u64 = 1_048_576; // 1 MB

static DEBUG_LOGS_ENABLED: AtomicBool = AtomicBool::new(false);

struct LogState {
    lines: VecDeque<String>,
    app_handle: Option<AppHandle>,
    writer: Option<BufWriter<File>>,
    written_bytes: u64,
}

static LOG_STATE: Mutex<Option<LogState>> = Mutex::new(None);

fn open_log_writer() -> (Option<BufWriter<File>>, u64) {
    let app_dir = get_app_dir();
    let log_path = app_dir.join("app.log");

    if let Ok(meta) = fs::metadata(&log_path) {
        if meta.len() >= MAX_LOG_FILE_BYTES {
            let old_path = app_dir.join("app.log.old");
            let _ = fs::remove_file(&old_path);
            let _ = fs::rename(&log_path, &old_path);
        }
    }

    let existing_len = fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
    let writer = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok()
        .map(BufWriter::new);

    (writer, existing_len)
}

pub fn init_logger() {
    let (writer, existing_len) = open_log_writer();
    if let Ok(mut lock) = LOG_STATE.lock() {
        *lock = Some(LogState {
            lines: VecDeque::with_capacity(MAX_LOG_LINES),
            app_handle: None,
            writer,
            written_bytes: existing_len,
        });
    }
}

pub fn set_app_handle(handle: AppHandle) {
    if let Ok(mut lock) = LOG_STATE.lock() {
        if let Some(state) = lock.as_mut() {
            state.app_handle = Some(handle);
        }
    }
}

pub fn set_debug_logs(enabled: bool) {
    DEBUG_LOGS_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn is_debug_logs_enabled() -> bool {
    DEBUG_LOGS_ENABLED.load(Ordering::Relaxed)
}

pub fn add_debug_log(msg: &str) {
    if !is_debug_logs_enabled() {
        return;
    }
    add_log(msg);
}

pub fn add_log(msg: &str) {
    let now = get_local_time_string();
    let formatted = format!("[{}] {}", now, msg);

    let mut handle_to_emit = None;
    if let Ok(mut lock) = LOG_STATE.lock() {
        if let Some(state) = lock.as_mut() {
            // 파일 로테이션 (1 MB 초과 시 app.log.old로 회전)
            if state.written_bytes >= MAX_LOG_FILE_BYTES {
                if let Some(mut w) = state.writer.take() {
                    let _ = w.flush();
                }
                let (new_writer, new_len) = open_log_writer();
                state.writer = new_writer;
                state.written_bytes = new_len;
            }

            if let Some(ref mut w) = state.writer {
                if writeln!(w, "{}", formatted).is_ok() {
                    let _ = w.flush();
                    state.written_bytes += (formatted.len() + 1) as u64;
                }
            }

            if state.lines.len() >= MAX_LOG_LINES {
                state.lines.pop_front();
            }
            state.lines.push_back(formatted.clone());
            handle_to_emit = state.app_handle.clone();
        }
    }

    // 창이 실제로 생성되어 있을 때만 UI로 IPC 이벤트 전송
    if let Some(app) = handle_to_emit {
        if app.get_webview_window("main").is_some() {
            let _ = app.emit("log-update", formatted);
        }
    }
}

pub fn flush_logs() {
    if let Ok(mut lock) = LOG_STATE.lock() {
        if let Some(state) = lock.as_mut() {
            if let Some(ref mut w) = state.writer {
                let _ = w.flush();
            }
        }
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

#[cfg(windows)]
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

#[cfg(not(windows))]
fn get_local_time_string() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let day_secs = secs % 86400;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}
