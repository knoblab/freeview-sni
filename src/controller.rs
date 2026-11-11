//! 서비스 생명주기 및 런타임 제어기(Controller)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::runtime::Runtime;
use tokio::sync::watch;

use crate::config::AppConfig;
use crate::desync::DesyncConfig;
use crate::guard::{self, save_state};
use crate::logging::add_log;
use crate::proxy::ProxyServer;
use crate::resolver::DnsResolver;
use crate::sysproxy::{apply_proxy, restore_proxy, ProxySnapshot};

pub struct Controller {
    config: AppConfig,
    is_running: Arc<AtomicBool>,
    runtime: Runtime,
    shutdown_tx: Option<watch::Sender<bool>>,
    proxy_server: Option<Arc<ProxyServer>>,
    last_snapshot: Option<ProxySnapshot>,
    app_handle: Option<AppHandle>,
}

impl Controller {
    pub fn new(config: AppConfig) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build()
            .expect("Tokio runtime creation failed");

        Self {
            config,
            is_running: Arc::new(AtomicBool::new(false)),
            runtime: rt,
            shutdown_tx: None,
            proxy_server: None,
            last_snapshot: None,
            app_handle: None,
        }
    }

    pub fn set_app_handle(&mut self, app_handle: AppHandle) {
        self.app_handle = Some(app_handle);
    }

    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    pub fn get_port(&self) -> u16 {
        self.proxy_server.as_ref().map(|s| s.port).unwrap_or(self.config.port)
    }

    pub fn start_service(&mut self) -> Result<u16, String> {
        if self.is_running() {
            return Ok(self.get_port());
        }

        add_log("네트워크 보호 활성화 시작...");

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let start_port = self.config.port;

        let (server, listener) = match self.runtime.block_on(ProxyServer::bind(start_port, shutdown_rx)) {
            Ok(res) => res,
            Err(e) => {
                let err_msg = format!("프록시 바인드 실패: {}", e);
                add_log(&err_msg);
                return Err(err_msg);
            }
        };

        let bound_port = server.port;
        add_log(&format!("로컬 프록시 엔진 바인드 완료 (127.0.0.1:{})", bound_port));
        let server_arc = Arc::new(server);
        self.proxy_server = Some(server_arc.clone());
        self.shutdown_tx = Some(shutdown_tx);

        let resolver = Arc::new(DnsResolver::new());
        let desync_cfg = DesyncConfig {
            mode: self.config.get_desync_mode(),
            delay_ms: self.config.delay_ms,
        };

        self.runtime.spawn(async move {
            server_arc.run(listener, resolver, desync_cfg).await;
        });

        // Windows 시스템 프록시 적용
        match apply_proxy(bound_port) {
            Ok(snap) => {
                self.last_snapshot = Some(snap.clone());
                save_state(true, Some(snap));
                add_log("Windows 시스템 프록시 등록 완료 (ProxyEnable=1, 127.0.0.1:8080)");
            }
            Err(e) => {
                let err_msg = format!("시스템 프록시 설정 실패: {}", e);
                add_log(&err_msg);
                // 고아 리스너 방지: shutdown_tx로 리스너 명시적 정리
                if let Some(tx) = self.shutdown_tx.take() {
                    let _ = tx.send(true);
                }
                self.proxy_server = None;
                return Err(err_msg);
            }
        }

        self.is_running.store(true, Ordering::SeqCst);
        add_log("✓ 보호 활성화 완료 - 모든 웹 트래픽 암호화 및 차단 우회 가동 중");

        if let Some(ref app) = self.app_handle {
            let _ = app.emit("status-update", true);
        }

        Ok(bound_port)
    }

    pub fn stop_service(&mut self) {
        if !self.is_running() {
            return;
        }

        add_log("네트워크 보호 중지 중...");

        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        self.proxy_server = None;

        let _ = restore_proxy(self.last_snapshot.as_ref());
        save_state(false, None);
        add_log("Windows 시스템 프록시 해제 (ProxyEnable=0, 원래 설정 복원)");

        self.is_running.store(false, Ordering::SeqCst);
        add_log("로컬 프록시 엔진 안전 종료 완료");
        add_log("○ 대기 상태 전환 - 보호 기능이 완전히 꺼졌습니다.");

        if let Some(ref app) = self.app_handle {
            let _ = app.emit("status-update", false);
        }
    }

    pub fn toggle_service(&mut self) -> bool {
        if self.is_running() {
            self.stop_service();
            false
        } else {
            let _ = self.start_service();
            self.is_running()
        }
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        if self.is_running() {
            guard::emergency_restore();
        }
    }
}
