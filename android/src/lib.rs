mod autostart;
mod config;
mod controller;
mod desync;
mod guard;
mod logging;
mod proxy;
mod resolver;
mod sysproxy;
mod tls;
#[cfg(target_os = "android")]
mod tun_vpn;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::sync::{Arc, Mutex};
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindowBuilder};

use config::AppConfig;
use controller::StatusPayload;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use controller::Controller;
use guard::{register_panic_hook, startup_self_heal};
use logging::{add_log, flush_logs, get_recent_logs, init_logger, set_app_handle};
#[cfg(any(target_os = "android", target_os = "ios"))]
use logging::set_debug_logs;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::sync::OnceLock;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::time::Instant;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Listener, WindowEvent,
};

const GITHUB_REPO_URL: &str = "https://github.com/knoblab/freeview-sni";
#[cfg(windows)]
const GITHUB_RELEASES_URL: &str = "https://github.com/knoblab/freeview-sni/releases";
#[cfg(windows)]
const GITHUB_LATEST_API_URL: &str =
    "https://api.github.com/repos/knoblab/freeview-sni/releases/latest";

#[cfg(not(any(target_os = "android", target_os = "ios")))]
static IS_CREATING_WINDOW: AtomicBool = AtomicBool::new(false);
#[cfg(not(any(target_os = "android", target_os = "ios")))]
static WINDOW_WAS_FOCUSED: AtomicBool = AtomicBool::new(false);
#[cfg(not(any(target_os = "android", target_os = "ios")))]
static LAST_WINDOW_OPEN_MS: AtomicU64 = AtomicU64::new(0);
#[cfg(not(any(target_os = "android", target_os = "ios")))]
static BLUR_SEQ: AtomicU64 = AtomicU64::new(0);
#[cfg(not(any(target_os = "android", target_os = "ios")))]
static LAST_BLUR_CLOSE_MS: AtomicU64 = AtomicU64::new(0);
#[cfg(windows)]
static HEADLESS_STOP: AtomicBool = AtomicBool::new(false);
#[cfg(not(any(target_os = "android", target_os = "ios")))]
static START_INSTANT: OnceLock<Instant> = OnceLock::new();

#[derive(Serialize)]
pub struct InitialState {
    #[serde(rename = "isServiceRunning")]
    pub is_service_running: bool,
    pub port: u16,
    pub logs: Vec<String>,
    pub version: String,
    #[serde(rename = "desyncMode")]
    pub desync_mode: String,
    #[serde(rename = "delayMs")]
    pub delay_ms: u64,
}

#[derive(Serialize)]
pub struct UpdateCheckResult {
    pub status: String,
    #[serde(rename = "latestVersion")]
    pub latest_version: Option<String>,
}

pub struct AppState {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub controller: Arc<Mutex<Controller>>,
    pub desync_mode: String,
    pub delay_ms: u64,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub struct TrayMenuState {
    pub status_item: MenuItem<tauri::Wry>,
    pub toggle_item: MenuItem<tauri::Wry>,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
struct CreatingWindowGuard;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl Drop for CreatingWindowGuard {
    fn drop(&mut self) {
        IS_CREATING_WINDOW.store(false, Ordering::SeqCst);
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn monotonic_ms() -> u64 {
    START_INSTANT
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis() as u64
}

#[cfg(windows)]
fn shell_open(target: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let op: Vec<u16> = "open\0".encode_utf16().collect();
    let file: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

#[cfg(not(windows))]
fn shell_open(_target: &str) {}

#[cfg(windows)]
fn parse_semver_tuple(ver: &str) -> Option<(u32, u32, u32)> {
    let clean = ver.trim().trim_start_matches(['v', 'V']);
    let core = clean.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u32>().ok()?;
    Some((major, minor, patch))
}

#[cfg(windows)]
fn is_newer_version(latest: &str, current: &str) -> bool {
    match (parse_semver_tuple(latest), parse_semver_tuple(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[cfg(windows)]
fn fetch_https_body(url: &str) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Networking::WinInet::{
        InternetCloseHandle, InternetOpenUrlW, InternetOpenW, InternetReadFile, InternetSetOptionW,
        INTERNET_FLAG_NO_CACHE_WRITE, INTERNET_FLAG_RELOAD, INTERNET_FLAG_SECURE,
        INTERNET_OPEN_TYPE_DIRECT, INTERNET_OPTION_CONNECT_TIMEOUT,
        INTERNET_OPTION_RECEIVE_TIMEOUT,
    };

    let ua: Vec<u16> = format!("FreeView/{}\0", env!("CARGO_PKG_VERSION"))
        .encode_utf16()
        .collect();
    let url_w: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    let headers: Vec<u16> = "Accept: application/vnd.github+json\r\n\0"
        .encode_utf16()
        .collect();

    unsafe {
        let h_internet = InternetOpenW(
            ua.as_ptr(),
            INTERNET_OPEN_TYPE_DIRECT,
            std::ptr::null(),
            std::ptr::null(),
            0,
        );
        if h_internet.is_null() {
            return Err("InternetOpenW failed".to_string());
        }

        let timeout_ms: u32 = 5000;
        let _ = InternetSetOptionW(
            h_internet,
            INTERNET_OPTION_CONNECT_TIMEOUT,
            &timeout_ms as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );
        let _ = InternetSetOptionW(
            h_internet,
            INTERNET_OPTION_RECEIVE_TIMEOUT,
            &timeout_ms as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
        );

        let flags = INTERNET_FLAG_SECURE | INTERNET_FLAG_RELOAD | INTERNET_FLAG_NO_CACHE_WRITE;
        let h_url = InternetOpenUrlW(
            h_internet,
            url_w.as_ptr(),
            headers.as_ptr(),
            (headers.len() - 1) as u32,
            flags,
            0,
        );
        if h_url.is_null() {
            InternetCloseHandle(h_internet);
            return Err("InternetOpenUrlW failed".to_string());
        }

        let mut body = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let mut bytes_read: u32 = 0;
            let ok = InternetReadFile(
                h_url,
                chunk.as_mut_ptr() as *mut _,
                chunk.len() as u32,
                &mut bytes_read,
            );
            if ok == 0 || bytes_read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..bytes_read as usize]);
            if body.len() >= 128 * 1024 {
                break;
            }
        }

        InternetCloseHandle(h_url);
        InternetCloseHandle(h_internet);

        if body.is_empty() {
            return Err("Empty response".to_string());
        }
        Ok(body)
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn open_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        LAST_WINDOW_OPEN_MS.store(monotonic_ms(), Ordering::SeqCst);
        BLUR_SEQ.fetch_add(1, Ordering::SeqCst);
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }

    if IS_CREATING_WINDOW
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let handle = app.clone();
    std::thread::spawn(move || {
        let _guard = CreatingWindowGuard;

        if let Some(win) = handle.get_webview_window("main") {
            LAST_WINDOW_OPEN_MS.store(monotonic_ms(), Ordering::SeqCst);
            BLUR_SEQ.fetch_add(1, Ordering::SeqCst);
            let _ = win.show();
            let _ = win.set_focus();
            return;
        }

        let Some(win_cfg) = handle.config().app.windows.first().cloned() else {
            return;
        };

        WINDOW_WAS_FOCUSED.store(false, Ordering::SeqCst);
        LAST_WINDOW_OPEN_MS.store(monotonic_ms(), Ordering::SeqCst);
        BLUR_SEQ.fetch_add(1, Ordering::SeqCst);
        match WebviewWindowBuilder::from_config(&handle, &win_cfg) {
            Ok(builder) => match builder.build() {
                Ok(win) => {
                    LAST_WINDOW_OPEN_MS.store(monotonic_ms(), Ordering::SeqCst);
                    let _ = win.show();
                    let _ = win.set_focus();
                }
                Err(e) => add_log(&format!("[UI 오류] 창 생성 실패: {}", e)),
            },
            Err(e) => add_log(&format!("[UI 오류] 창 설정 로드 실패: {}", e)),
        }
    });
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn update_tray_state(app: &AppHandle, is_running: bool) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        let tooltip = if is_running {
            "FreeView — 보호 활성화"
        } else {
            "FreeView — 대기 중"
        };
        let _ = tray.set_tooltip(Some(tooltip));
    }
    if let Some(tray_state) = app.try_state::<TrayMenuState>() {
        let (status_text, toggle_text) = if is_running {
            ("● 보호 활성화", "▣ 서비스 중지")
        } else {
            ("○ 대기 중 (보호 꺼짐)", "▶ 서비스 시작")
        };
        let _ = tray_state.status_item.set_text(status_text);
        let _ = tray_state.toggle_item.set_text(toggle_text);
    }
}

#[cfg(target_os = "android")]
pub struct VpnPluginHandle(pub tauri::plugin::PluginHandle<tauri::Wry>);

#[cfg(target_os = "android")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToggleVpnArgs {
    mode: String,
    delay_ms: u64,
}

#[tauri::command]
async fn toggle_service(
    #[allow(unused_variables)] app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StatusPayload, String> {
    #[cfg(target_os = "android")]
    {
        let handle = app
            .try_state::<VpnPluginHandle>()
            .ok_or_else(|| "VPN 플러그인이 초기화되지 않았습니다.".to_string())?;
        let args = ToggleVpnArgs {
            mode: state.desync_mode.clone(),
            delay_ms: state.delay_ms,
        };
        return handle
            .0
            .run_mobile_plugin::<StatusPayload>("toggleVpn", args)
            .map_err(|e| format!("VPN 호출 실패: {}", e));
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let ctrl_arc = state.controller.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if let Ok(mut ctrl) = ctrl_arc.lock() {
                ctrl.toggle_service()
            } else {
                StatusPayload {
                    is_running: false,
                    port: 8080,
                    error: Some("컨트롤러 잠금 실패".to_string()),
                }
            }
        })
        .await
        .map_err(|e| e.to_string())
    }

    #[cfg(target_os = "ios")]
    {
        let _ = (app, state);
        Err("iOS는 지원되지 않습니다.".to_string())
    }
}

#[tauri::command]
fn get_initial_state(state: State<'_, AppState>) -> InitialState {
    #[cfg(target_os = "android")]
    let (is_running, port) = (tun_vpn::is_tun_running(), 0);

    #[cfg(target_os = "ios")]
    let (is_running, port) = (false, 0);

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let (is_running, port) = if let Ok(ctrl) = state.controller.lock() {
        (ctrl.is_running(), ctrl.get_port())
    } else {
        (false, 8080)
    };

    InitialState {
        is_service_running: is_running,
        port,
        logs: get_recent_logs(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        desync_mode: state.desync_mode.clone(),
        delay_ms: state.delay_ms,
    }
}

#[tauri::command]
fn get_logs() -> Vec<String> {
    get_recent_logs()
}

#[tauri::command]
fn open_log_file() {
    flush_logs();
    let log_path = guard::get_app_dir().join("app.log");
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path);
    shell_open(&log_path.to_string_lossy());
}

#[tauri::command]
fn open_github() {
    shell_open(GITHUB_REPO_URL);
}

#[tauri::command]
async fn check_update() -> UpdateCheckResult {
    #[cfg(windows)]
    {
        let current = env!("CARGO_PKG_VERSION");
        match tauri::async_runtime::spawn_blocking(|| fetch_https_body(GITHUB_LATEST_API_URL)).await
        {
            Ok(Ok(bytes)) => {
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    if let Some(tag) = json.get("tag_name").and_then(|v| v.as_str()) {
                        if is_newer_version(tag, current) {
                            let url = json
                                .get("html_url")
                                .and_then(|v| v.as_str())
                                .unwrap_or(GITHUB_RELEASES_URL);
                            shell_open(url);
                            return UpdateCheckResult {
                                status: "update_available".to_string(),
                                latest_version: Some(tag.to_string()),
                            };
                        }
                        return UpdateCheckResult {
                            status: "up_to_date".to_string(),
                            latest_version: Some(tag.to_string()),
                        };
                    }
                    if json.get("message").and_then(|v| v.as_str()) == Some("Not Found") {
                        return UpdateCheckResult {
                            status: "up_to_date".to_string(),
                            latest_version: None,
                        };
                    }
                }
                UpdateCheckResult {
                    status: "error".to_string(),
                    latest_version: None,
                }
            }
            _ => UpdateCheckResult {
                status: "error".to_string(),
                latest_version: None,
            },
        }
    }

    #[cfg(not(windows))]
    {
        UpdateCheckResult {
            status: "up_to_date".to_string(),
            latest_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn headless_ctrl_handler(_ctrl_type: u32) -> i32 {
    HEADLESS_STOP.store(true, Ordering::SeqCst);
    1
}

#[cfg(windows)]
fn run_headless(config: AppConfig) {
    use windows_sys::Win32::System::Console::{
        AttachConsole, SetConsoleCtrlHandler, ATTACH_PARENT_PROCESS,
    };

    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        let _ = SetConsoleCtrlHandler(Some(headless_ctrl_handler), 1);
    }

    println!(
        "FreeView {} 헤드리스 모드 시작 (종료: Ctrl+C)",
        env!("CARGO_PKG_VERSION")
    );
    let mut ctrl = Controller::new(config);
    match ctrl.start_service() {
        Ok(port) => {
            println!("로컬 프록시 가동 중: 127.0.0.1:{}", port);
            while !HEADLESS_STOP.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            ctrl.stop_service();
        }
        Err(e) => {
            eprintln!("서비스 시작 실패: {}", e);
        }
    }
    guard::emergency_restore();
    flush_logs();
    println!("FreeView 헤드리스 모드 안전 종료 완료.");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let args: Vec<String> = std::env::args().collect();

        if args.iter().any(|arg| arg == "--restore") {
            let _ = sysproxy::restore_proxy(None);
            guard::save_state(false, None);
            return;
        }

        register_panic_hook();
        init_logger();

        if startup_self_heal() {
            add_log("⚠ 비정상 종료 감지 → 네트워크 설정 복구 완료.");
        } else {
            add_log(&format!(
                "FreeView {} 네이티브 엔진 초기화.",
                env!("CARGO_PKG_VERSION")
            ));
        }

        let config = AppConfig::load();
        let is_startup = args.iter().any(|arg| arg == "--startup");
        #[cfg(windows)]
        if args.iter().any(|arg| arg == "--headless") {
            run_headless(config);
            return;
        }

        let initial_debug_logs = config.debug_logs;
        let desync_mode = config.mode_str.clone();
        let delay_ms = config.delay_ms;
        let controller = Controller::new(config);
        let app_state = AppState {
            controller: Arc::new(Mutex::new(controller)),
            desync_mode,
            delay_ms,
        };

        tauri::Builder::default()
            .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                open_main_window(app);
            }))
            .on_window_event(|window, event| match event {
                WindowEvent::Focused(true) => {
                    WINDOW_WAS_FOCUSED.store(true, Ordering::SeqCst);
                    BLUR_SEQ.fetch_add(1, Ordering::SeqCst);
                }
                WindowEvent::Focused(false) => {
                    if !WINDOW_WAS_FOCUSED.load(Ordering::SeqCst) {
                        return;
                    }
                    if monotonic_ms().saturating_sub(LAST_WINDOW_OPEN_MS.load(Ordering::SeqCst))
                        < 450
                    {
                        return;
                    }
                    let seq = BLUR_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
                    let win_clone = window.clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                        if BLUR_SEQ.load(Ordering::SeqCst) == seq
                            && !win_clone.is_focused().unwrap_or(false)
                        {
                            WINDOW_WAS_FOCUSED.store(false, Ordering::SeqCst);
                            LAST_BLUR_CLOSE_MS.store(monotonic_ms(), Ordering::SeqCst);
                            let _ = win_clone.close();
                        }
                    });
                }
                WindowEvent::Destroyed => {
                    WINDOW_WAS_FOCUSED.store(false, Ordering::SeqCst);
                    BLUR_SEQ.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            })
            .manage(app_state)
            .setup(move |app| {
                let handle = app.handle().clone();
                set_app_handle(handle.clone());

                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut ctrl) = state.controller.lock() {
                        ctrl.set_app_handle(handle.clone());
                        let _ = ctrl.start_service();
                    }
                }

                setup_tray(&handle, initial_debug_logs)?;

                let handle_for_listener = handle.clone();
                handle.listen("status-update", move |event| {
                    if let Ok(payload) = serde_json::from_str::<StatusPayload>(event.payload()) {
                        update_tray_state(&handle_for_listener, payload.is_running);
                    }
                });

                let initial_running = if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(ctrl) = state.controller.lock() {
                        ctrl.is_running()
                    } else {
                        false
                    }
                } else {
                    false
                };
                update_tray_state(&handle, initial_running);

                if !is_startup {
                    open_main_window(&handle);
                }

                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                toggle_service,
                get_initial_state,
                get_logs,
                open_log_file,
                open_github,
                check_update
            ])
            .build(tauri::generate_context!())
            .expect("Tauri application build failed")
            .run(|app_handle, event| match event {
                tauri::RunEvent::ExitRequested { code, api, .. } => {
                    if code.is_none() {
                        api.prevent_exit();
                    }
                }
                tauri::RunEvent::Exit => {
                    if let Some(state) = app_handle.try_state::<AppState>() {
                        if let Ok(mut ctrl) = state.controller.lock() {
                            ctrl.stop_service();
                        }
                    }
                    guard::emergency_restore();
                    flush_logs();
                }
                _ => {}
            });
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        register_panic_hook();

        let config = AppConfig::default();
        set_debug_logs(config.debug_logs);
        let desync_mode = config.mode_str.clone();
        let delay_ms = config.delay_ms;
        let app_state = AppState {
            desync_mode,
            delay_ms,
        };

        let mut builder = tauri::Builder::default().manage(app_state);
        #[cfg(target_os = "android")]
        {
            builder = builder.plugin(
                tauri::plugin::Builder::<tauri::Wry, ()>::new("vpn")
                    .setup(|app, api| {
                        let handle = api.register_android_plugin("com.qpi.freeview", "VpnPlugin")?;
                        app.manage(VpnPluginHandle(handle));
                        Ok(())
                    })
                    .build(),
            );
        }

        builder
            .setup(|app| {
                if let Ok(dir) = app.path().app_data_dir() {
                    guard::set_app_dir(dir);
                }
                init_logger();
                let _ = startup_self_heal();
                add_log(&format!(
                    "FreeView {} Android 엔진 초기화.",
                    env!("CARGO_PKG_VERSION")
                ));

                let handle = app.handle().clone();
                set_app_handle(handle.clone());

                if let Some(win_cfg) = handle.config().app.windows.first().cloned() {
                    if let Ok(builder) = WebviewWindowBuilder::from_config(&handle, &win_cfg) {
                        let _ = builder.build();
                    }
                }
                Ok(())
            })
            .invoke_handler(tauri::generate_handler![
                toggle_service,
                get_initial_state,
                get_logs,
                open_log_file,
                open_github,
                check_update
            ])
            .run(tauri::generate_context!())
            .expect("Tauri mobile application run failed");
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn setup_tray(app: &AppHandle, initial_debug_logs: bool) -> Result<(), Box<dyn std::error::Error>> {
    let title_item = MenuItem::with_id(
        app,
        "title",
        format!("FreeView {}", env!("CARGO_PKG_VERSION")),
        false,
        None::<&str>,
    )?;
    let status_item = MenuItem::with_id(app, "status", "● 보호 활성화", false, None::<&str>)?;
    let toggle_item = MenuItem::with_id(app, "toggle", "▣ 서비스 중지", true, None::<&str>)?;
    let autostart_item = CheckMenuItem::with_id(
        app,
        "autostart",
        "Windows 시작 시 자동 실행",
        true,
        autostart::is_autostart_enabled(),
        None::<&str>,
    )?;
    let debug_logs_item = CheckMenuItem::with_id(
        app,
        "debug_logs",
        "상세 연결 로그 기록",
        true,
        initial_debug_logs,
        None::<&str>,
    )?;
    let log_item = MenuItem::with_id(app, "open_log", "로그 파일 열기", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;

    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[
            &title_item,
            &status_item,
            &sep1,
            &toggle_item,
            &autostart_item,
            &debug_logs_item,
            &sep2,
            &log_item,
            &sep3,
            &quit_item,
        ],
    )?;

    let mut tray_builder = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("FreeView");

    if let Some(icon) = app.default_window_icon() {
        tray_builder = tray_builder.icon(icon.clone());
    }

    let autostart_item_clone = autostart_item.clone();
    let debug_logs_item_clone = debug_logs_item.clone();

    let _tray = tray_builder
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "toggle" => {
                let app_clone = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Some(state) = app_clone.try_state::<AppState>() {
                        if let Ok(mut ctrl) = state.controller.lock() {
                            ctrl.toggle_service();
                        }
                    }
                });
            }
            "autostart" => {
                let new_state = !autostart::is_autostart_enabled();
                if autostart::set_autostart(new_state).is_ok() {
                    let _ = autostart_item_clone.set_checked(new_state);
                    let mut cfg = AppConfig::load();
                    cfg.autostart = new_state;
                    cfg.save();
                }
            }
            "debug_logs" => {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut ctrl) = state.controller.lock() {
                        let next_state = !ctrl.is_debug_logs();
                        ctrl.set_debug_logs(next_state);
                        let _ = debug_logs_item_clone.set_checked(next_state);
                    }
                }
            }
            "open_log" => {
                open_log_file();
            }
            "quit" => {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut ctrl) = state.controller.lock() {
                        ctrl.stop_service();
                    }
                }
                guard::emergency_restore();
                flush_logs();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if monotonic_ms().saturating_sub(LAST_BLUR_CLOSE_MS.load(Ordering::SeqCst)) < 250 {
                    return;
                }
                let app = tray.app_handle();
                if let Some(win) = app.get_webview_window("main") {
                    if win.is_visible().unwrap_or(false) {
                        let _ = win.close();
                    } else {
                        open_main_window(app);
                    }
                } else {
                    open_main_window(app);
                }
            }
        })
        .build(app)?;

    app.manage(TrayMenuState {
        status_item,
        toggle_item,
    });

    Ok(())
}
