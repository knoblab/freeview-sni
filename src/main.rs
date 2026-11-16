#![windows_subsystem = "windows"]

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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use serde::Serialize;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Listener, Manager, State, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use windows_sys::Win32::System::Console::{
    AttachConsole, SetConsoleCtrlHandler, ATTACH_PARENT_PROCESS,
};

use config::AppConfig;
use controller::{Controller, StatusPayload};
use guard::{register_panic_hook, startup_self_heal};
use logging::{add_log, flush_logs, get_recent_logs, init_logger, set_app_handle};

static IS_CREATING_WINDOW: AtomicBool = AtomicBool::new(false);
static LAST_LOW_MEMORY: AtomicBool = AtomicBool::new(false);
static HEADLESS_STOP: AtomicBool = AtomicBool::new(false);

#[derive(Serialize)]
pub struct InitialState {
    #[serde(rename = "isServiceRunning")]
    pub is_service_running: bool,
    pub port: u16,
    pub logs: Vec<String>,
    pub version: String,
}

pub struct AppState {
    pub controller: Arc<Mutex<Controller>>,
}

pub struct TrayMenuState {
    pub status_item: MenuItem<tauri::Wry>,
    pub toggle_item: MenuItem<tauri::Wry>,
}

struct CreatingWindowGuard;
impl Drop for CreatingWindowGuard {
    fn drop(&mut self) {
        IS_CREATING_WINDOW.store(false, Ordering::SeqCst);
    }
}

#[cfg(windows)]
fn set_webview_low_memory(win: &WebviewWindow, low: bool) {
    if LAST_LOW_MEMORY.swap(low, Ordering::SeqCst) == low {
        return;
    }

    let _ = win.with_webview(move |webview| {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL,
        };
        use windows_core::Interface;

        unsafe {
            if let Ok(core) = webview.controller().CoreWebView2() {
                if let Ok(wv19) = core.cast::<ICoreWebView2_19>() {
                    let level = COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL(if low { 1 } else { 0 });
                    let _ = wv19.SetMemoryUsageTargetLevel(level);
                }
            }
        }
    });
}

#[cfg(not(windows))]
fn set_webview_low_memory(_win: &WebviewWindow, _low: bool) {}

pub fn open_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        set_webview_low_memory(&win, false);
        let _ = win.show();
        let _ = win.unminimize();
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
            set_webview_low_memory(&win, false);
            let _ = win.show();
            let _ = win.unminimize();
            let _ = win.set_focus();
            return;
        }

        let Some(win_cfg) = handle.config().app.windows.first().cloned() else {
            return;
        };

        LAST_LOW_MEMORY.store(false, Ordering::SeqCst);
        if let Ok(builder) = WebviewWindowBuilder::from_config(&handle, &win_cfg) {
            if let Ok(win) = builder.build() {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }
    });
}

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

#[tauri::command]
async fn toggle_service(state: State<'_, AppState>) -> Result<StatusPayload, String> {
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

#[tauri::command]
fn get_initial_state(state: State<'_, AppState>) -> InitialState {
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
    }
}

unsafe extern "system" fn headless_ctrl_handler(_ctrl_type: u32) -> i32 {
    HEADLESS_STOP.store(true, Ordering::SeqCst);
    1
}

fn run_headless(config: AppConfig) {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        let _ = SetConsoleCtrlHandler(Some(headless_ctrl_handler), 1);
    }

    println!("FreeView {} 헤드리스 모드 시작 (종료: Ctrl+C)", env!("CARGO_PKG_VERSION"));
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

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // 1. 비상 복구 CLI 모드: --restore
    if args.iter().any(|arg| arg == "--restore") {
        let _ = sysproxy::restore_proxy(None);
        guard::save_state(false, None);
        return;
    }

    // 2. 패닉 훅 등록
    register_panic_hook();

    // 3. 로거 초기화
    init_logger();

    // 4. 자가 복구
    if startup_self_heal() {
        add_log("⚠ 비정상 종료 감지 → 네트워크 설정 복구 완료.");
    } else {
        add_log(&format!("FreeView {} 네이티브 엔진 초기화.", env!("CARGO_PKG_VERSION")));
    }

    // 5. 환경설정 로드
    let config = AppConfig::load();
    let is_startup = args.iter().any(|arg| arg == "--startup");
    let is_headless = args.iter().any(|arg| arg == "--headless");

    // 6. 헤드리스 콘솔 모드 분기 (WebView2 및 트레이 없이 최소 메모리로 구동)
    if is_headless {
        run_headless(config);
        return;
    }

    let initial_debug_logs = config.debug_logs;
    let controller = Controller::new(config);
    let app_state = AppState {
        controller: Arc::new(Mutex::new(controller)),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            open_main_window(app);
        }))
        .on_window_event(|window, event| {
            match event {
                WindowEvent::Resized(_) | WindowEvent::Focused(_) => {
                    if let Some(ww) = window.app_handle().get_webview_window(window.label()) {
                        let minimized = ww.is_minimized().unwrap_or(false);
                        set_webview_low_memory(&ww, minimized);
                    }
                }
                WindowEvent::Destroyed => {
                    LAST_LOW_MEMORY.store(false, Ordering::SeqCst);
                }
                _ => {}
            }
        })
        .manage(app_state)
        .setup(move |app| {
            let handle = app.handle().clone();
            set_app_handle(handle.clone());

            // 컨트롤러에 AppHandle 전달 및 시작 시 서비스 자동 실행
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(mut ctrl) = state.controller.lock() {
                    ctrl.set_app_handle(handle.clone());
                    let _ = ctrl.start_service();
                }
            }

            // 시스템 트레이 구성
            setup_tray(&handle, initial_debug_logs)?;

            let handle_for_listener = handle.clone();
            handle.listen("status-update", move |event| {
                if let Ok(payload) = serde_json::from_str::<StatusPayload>(event.payload()) {
                    update_tray_state(&handle_for_listener, payload.is_running);
                }
            });

            // 초기 트레이 상태 동기화
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

            // --startup이 아니면 온디맨드 창 표시 (--startup 시 WebView2 미생성으로 ~5MB 유지)
            if !is_startup {
                open_main_window(&handle);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            toggle_service,
            get_initial_state
        ])
        .build(tauri::generate_context!())
        .expect("Tauri application build failed")
        .run(|app_handle, event| {
            match event {
                tauri::RunEvent::ExitRequested { code, api, .. } => {
                    // X 버튼으로 마지막 창이 파괴되어도 트레이/프록시 상주를 위해 앱 종료 방지
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
            }
        });
}

fn setup_tray(app: &AppHandle, initial_debug_logs: bool) -> Result<(), Box<dyn std::error::Error>> {
    let title_item = MenuItem::with_id(
        app,
        "title",
        format!("FreeView {}", env!("CARGO_PKG_VERSION")),
        false,
        None::<&str>,
    )?;
    let status_item = MenuItem::with_id(app, "status", "● 보호 활성화", false, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let open_item = MenuItem::with_id(app, "open", "앱 열기", true, None::<&str>)?;
    let toggle_item = MenuItem::with_id(app, "toggle", "▣ 서비스 중지", true, None::<&str>)?;
    let autostart_enabled = autostart::is_autostart_enabled();
    let autostart_item = CheckMenuItem::with_id(
        app,
        "autostart",
        "Windows 시작 시 자동 실행",
        true,
        autostart_enabled,
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
    let logs_item = MenuItem::with_id(app, "logs", "로그 폴더 열기", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let exit_item = MenuItem::with_id(app, "exit", "종료", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &title_item,
            &status_item,
            &sep1,
            &open_item,
            &toggle_item,
            &autostart_item,
            &debug_logs_item,
            &logs_item,
            &sep2,
            &exit_item,
        ],
    )?;

    let icon = app.default_window_icon().cloned().ok_or("No default window icon")?;
    let autostart_item_ref = autostart_item.clone();
    let debug_logs_item_ref = debug_logs_item.clone();

    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .menu(&menu)
        .tooltip("FreeView — 보호 활성화")
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            match id {
                "open" => {
                    open_main_window(app);
                }
                "toggle" => {
                    if let Some(state) = app.try_state::<AppState>() {
                        let ctrl_arc = state.controller.clone();
                        tauri::async_runtime::spawn_blocking(move || {
                            if let Ok(mut ctrl) = ctrl_arc.lock() {
                                ctrl.toggle_service();
                            }
                        });
                    }
                }
                "autostart" => {
                    let target = !autostart::is_autostart_enabled();
                    let _ = autostart::set_autostart(target);
                    let _ = autostart_item_ref.set_checked(autostart::is_autostart_enabled());
                }
                "debug_logs" => {
                    if let Some(state) = app.try_state::<AppState>() {
                        if let Ok(mut ctrl) = state.controller.lock() {
                            let next = !ctrl.is_debug_logs();
                            ctrl.set_debug_logs(next);
                            let _ = debug_logs_item_ref.set_checked(next);
                        }
                    }
                }
                "logs" => {
                    let dir = guard::get_app_dir();
                    let _ = std::process::Command::new("explorer").arg(dir).spawn();
                }
                "exit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(win) = app.get_webview_window("main") {
                    let is_visible = win.is_visible().unwrap_or(false);
                    let is_minimized = win.is_minimized().unwrap_or(false);
                    if is_visible && !is_minimized {
                        // 창 닫기(파기)로 WebView2 메모리 즉시 반환
                        let _ = win.close();
                    } else {
                        set_webview_low_memory(&win, false);
                        let _ = win.show();
                        let _ = win.unminimize();
                        let _ = win.set_focus();
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
