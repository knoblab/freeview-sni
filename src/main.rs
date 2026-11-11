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

use std::sync::Mutex;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Listener, Manager, State,
};
use serde::Serialize;

use config::AppConfig;
use controller::Controller;
use guard::{startup_self_heal, register_panic_hook};
use logging::{add_log, init_logger, set_app_handle, get_recent_logs};

#[derive(Serialize)]
pub struct InitialState {
    #[serde(rename = "isServiceRunning")]
    pub is_service_running: bool,
    pub logs: Vec<String>,
    pub version: String,
}

pub struct AppState {
    pub controller: Mutex<Controller>,
}

pub struct TrayMenuState {
    pub status_item: MenuItem<tauri::Wry>,
    pub toggle_item: MenuItem<tauri::Wry>,
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
fn toggle_service(app: AppHandle, state: State<'_, AppState>) -> bool {
    if let Ok(mut ctrl) = state.controller.lock() {
        let next = ctrl.toggle_service();
        update_tray_state(&app, next);
        next
    } else {
        false
    }
}

#[tauri::command]
fn get_initial_state(state: State<'_, AppState>) -> InitialState {
    let is_running = if let Ok(ctrl) = state.controller.lock() {
        ctrl.is_running()
    } else {
        false
    };

    InitialState {
        is_service_running: is_running,
        logs: get_recent_logs(),
        version: "3.1.0".to_string(),
    }
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
        add_log("FreeView 3.1 네이티브 엔진 초기화.");
    }

    // 5. 환경설정 로드
    let config = AppConfig::load();
    let is_startup = args.iter().any(|arg| arg == "--startup");

    let controller = Controller::new(config);
    let app_state = AppState {
        controller: Mutex::new(controller),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
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
            setup_tray(&handle)?;

            let handle_for_listener = handle.clone();
            handle.listen("status-update", move |event| {
                if let Ok(is_running) = serde_json::from_str::<bool>(event.payload()) {
                    update_tray_state(&handle_for_listener, is_running);
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

            // 윈도우 창 표시 제어 (--startup이 아니면 바로 표시)
            if let Some(window) = app.get_webview_window("main") {
                if !is_startup {
                    let _ = window.show();
                }
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
                tauri::RunEvent::Exit => {
                    if let Some(state) = app_handle.try_state::<AppState>() {
                        if let Ok(mut ctrl) = state.controller.lock() {
                            ctrl.stop_service();
                        }
                    }
                    guard::emergency_restore();
                }
                _ => {}
            }
        });
}

fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let title_item = MenuItem::with_id(app, "title", "FreeView 3.1", false, None::<&str>)?;
    let status_item = MenuItem::with_id(app, "status", "● 보호 활성화", false, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let open_item = MenuItem::with_id(app, "open", "앱 열기", true, None::<&str>)?;
    let toggle_item = MenuItem::with_id(app, "toggle", "▣ 서비스 중지", true, None::<&str>)?;
    let autostart_enabled = autostart::is_autostart_enabled();
    let autostart_item = CheckMenuItem::with_id(app, "autostart", "Windows 시작 시 자동 실행", true, autostart_enabled, None::<&str>)?;
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
            &logs_item,
            &sep2,
            &exit_item,
        ],
    )?;

    let icon = app.default_window_icon().cloned().ok_or("No default window icon")?;

    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .menu(&menu)
        .tooltip("FreeView — 보호 활성화")
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            match id {
                "open" => {
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.show();
                        let _ = win.unminimize();
                        let _ = win.set_focus();
                    }
                }
                "toggle" => {
                    if let Some(state) = app.try_state::<AppState>() {
                        if let Ok(mut ctrl) = state.controller.lock() {
                            let next = ctrl.toggle_service();
                            update_tray_state(app, next);
                        }
                    }
                }
                "autostart" => {
                    let current = autostart::is_autostart_enabled();
                    let _ = autostart::set_autostart(!current);
                }
                "logs" => {
                    let dir = guard::get_app_dir();
                    let _ = std::process::Command::new("explorer").arg(dir).spawn();
                }
                "exit" => {
                    if let Some(state) = app.try_state::<AppState>() {
                        if let Ok(mut ctrl) = state.controller.lock() {
                            ctrl.stop_service();
                        }
                    }
                    guard::emergency_restore();
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                let app = tray.app_handle();
                if let Some(win) = app.get_webview_window("main") {
                    let is_visible = win.is_visible().unwrap_or(false);
                    if is_visible {
                        let _ = win.hide();
                    } else {
                        let _ = win.show();
                        let _ = win.unminimize();
                        let _ = win.set_focus();
                    }
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
