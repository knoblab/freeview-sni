//! Android VpnService TUN 패킷 처리 및 SNI Desync 엔진
#![cfg(target_os = "android")]

use std::fs::File;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::os::unix::io::FromRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jlong, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use netstack_smoltcp::{StackBuilder, TcpListener, UdpSocket as StackUdpSocket};
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::runtime::Runtime;
use tokio::sync::watch;

use crate::desync::{self, DesyncConfig, DesyncMode};
use crate::logging::add_log;
use crate::tls;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const CLIENT_HELLO_TIMEOUT: Duration = Duration::from_secs(3);
const UDP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_FIRST_CHUNK: usize = 32768;

struct TunEngineState {
    runtime: Runtime,
    shutdown_tx: Option<watch::Sender<bool>>,
}

static TUN_STATE: OnceLock<Mutex<TunEngineState>> = OnceLock::new();
static TUN_RUNNING: AtomicBool = AtomicBool::new(false);

fn get_tun_state() -> &'static Mutex<TunEngineState> {
    TUN_STATE.get_or_init(|| {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(4)
            .build()
            .expect("Android TUN Tokio runtime creation failed");
        Mutex::new(TunEngineState {
            runtime: rt,
            shutdown_tx: None,
        })
    })
}

fn set_nonblocking(fd: i32) -> io::Result<()> {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn start_tun_engine(tun_fd: i32, mode_str: &str, delay_ms: u64) -> Result<(), String> {
    stop_tun_engine();

    if let Err(e) = set_nonblocking(tun_fd) {
        return Err(format!("TUN O_NONBLOCK 설정 실패: {}", e));
    }

    let file = unsafe { File::from_raw_fd(tun_fd) };
    let mode = match mode_str {
        "SniSplit" => DesyncMode::SniSplit,
        "TlsRecordFrag" => DesyncMode::TlsRecordFrag,
        _ => DesyncMode::TlsHeader5,
    };
    let desync_cfg = DesyncConfig { mode, delay_ms };

    let mut state = get_tun_state()
        .lock()
        .map_err(|_| "TUN 상태 잠금 실패".to_string())?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    state.shutdown_tx = Some(shutdown_tx);
    TUN_RUNNING.store(true, Ordering::SeqCst);

    state.runtime.spawn(async move {
        if let Err(e) = run_tun_stack(file, desync_cfg, shutdown_rx).await {
            add_log(&format!("[TUN 오류] {}", e));
        }
        TUN_RUNNING.store(false, Ordering::SeqCst);
    });

    add_log(&format!(
        "Android VPN TUN 엔진 시작 (fd={}, mode={}, delay={}ms)",
        tun_fd, mode_str, delay_ms
    ));
    Ok(())
}

pub fn stop_tun_engine() {
    if let Ok(mut state) = get_tun_state().lock() {
        if let Some(tx) = state.shutdown_tx.take() {
            let _ = tx.send(true);
        }
    }
    TUN_RUNNING.store(false, Ordering::SeqCst);
}

pub fn is_tun_running() -> bool {
    TUN_RUNNING.load(Ordering::SeqCst)
}

async fn run_tun_stack(
    file: File,
    desync_cfg: DesyncConfig,
    mut shutdown_rx: watch::Receiver<bool>,
) -> io::Result<()> {
    let async_tun = Arc::new(AsyncFd::new(file)?);

    let (stack, runner, udp_socket, tcp_listener) = StackBuilder::default()
        .enable_tcp(true)
        .enable_udp(true)
        .mtu(1500)
        .build()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("{:?}", e)))?;

    let udp_socket = udp_socket.ok_or_else(|| io::Error::new(io::ErrorKind::Other, "No UDP"))?;
    let tcp_listener =
        tcp_listener.ok_or_else(|| io::Error::new(io::ErrorKind::Other, "No TCP"))?;

    let runner_handle = runner.map(tokio::spawn);

    let (mut stack_sink, mut stack_stream) = stack.split();

    // 1. TUN -> Netstack
    let tun_reader = async_tun.clone();
    let mut rx_shut = shutdown_rx.clone();
    let read_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];
        loop {
            tokio::select! {
                _ = rx_shut.changed() => {
                    if *rx_shut.borrow() { break; }
                }
                guard_res = tun_reader.readable() => {
                    let mut guard = match guard_res {
                        Ok(g) => g,
                        Err(_) => break,
                    };
                    match guard.try_io(|inner| (&*inner.get_ref()).read(&mut buf)) {
                        Ok(Ok(0)) => break,
                        Ok(Ok(n)) => {
                            if stack_sink.send(buf[..n].to_vec()).await.is_err() {
                                break;
                            }
                        }
                        Ok(Err(_)) => break,
                        Err(_would_block) => continue,
                    }
                }
            }
        }
    });

    // 2. Netstack -> TUN
    let tun_writer = async_tun.clone();
    let mut tx_shut = shutdown_rx.clone();
    let write_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tx_shut.changed() => {
                    if *tx_shut.borrow() { break; }
                }
                pkt_opt = stack_stream.next() => {
                    let Some(Ok(pkt)) = pkt_opt else { break; };
                    loop {
                        let mut guard = match tun_writer.writable().await {
                            Ok(g) => g,
                            Err(_) => return,
                        };
                        match guard.try_io(|inner| (&*inner.get_ref()).write(pkt.as_ref())) {
                            Ok(Ok(_)) => break,
                            Ok(Err(_)) => return,
                            Err(_would_block) => continue,
                        }
                    }
                }
            }
        }
    });

    // 3. TCP 연결 처리 (SNI Desync 적용)
    let tcp_shut = shutdown_rx.clone();
    let tcp_task = tokio::spawn(handle_tcp_connections(tcp_listener, desync_cfg, tcp_shut));

    // 4. UDP 패킷 처리 (DNS 및 일반 UDP 포워딩)
    let udp_shut = shutdown_rx.clone();
    let udp_task = tokio::spawn(handle_udp_packets(udp_socket, udp_shut));

    // 종료 대기
    let _ = shutdown_rx.changed().await;

    read_task.abort();
    write_task.abort();
    tcp_task.abort();
    udp_task.abort();
    if let Some(h) = runner_handle {
        h.abort();
    }

    Ok(())
}

async fn handle_tcp_connections(
    mut tcp_listener: TcpListener,
    desync_cfg: DesyncConfig,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() { break; }
            }
            conn = tcp_listener.next() => {
                let Some((stream, _local, remote)) = conn else { break; };
                let cfg = desync_cfg.clone();
                tokio::spawn(async move {
                    let _ = relay_tcp_with_desync(stream, remote, cfg).await;
                });
            }
        }
    }
}

async fn relay_tcp_with_desync(
    mut client: netstack_smoltcp::TcpStream,
    remote: SocketAddr,
    desync_cfg: DesyncConfig,
) -> io::Result<()> {
    let mut upstream =
        tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(remote)).await??;
    let _ = upstream.set_nodelay(true);

    let mut first_buf = vec![0u8; MAX_FIRST_CHUNK];
    match tokio::time::timeout(CLIENT_HELLO_TIMEOUT, client.read(&mut first_buf)).await {
        Ok(Ok(0)) => return Ok(()),
        Ok(Ok(n)) => {
            let payload = &first_buf[..n];
            if tls::is_client_hello(payload) {
                desync::send_first(&mut upstream, payload, &desync_cfg).await?;
            } else {
                upstream.write_all(payload).await?;
            }
        }
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            // 클라이언트가 먼저 보내지 않는 프로토콜이면 그대로 양방향 릴레이 진행
        }
    }

    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
    Ok(())
}

async fn handle_udp_packets(
    udp_socket: StackUdpSocket,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(Vec<u8>, SocketAddr, SocketAddr)>();
    let (mut read_half, mut write_half) = udp_socket.split();

    let writer_task = tokio::spawn(async move {
        while let Some((data, local, remote)) = rx.recv().await {
            if write_half.send((data, remote, local)).await.is_err() {
                break;
            }
        }
    });

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() { break; }
            }
            pkt = read_half.next() => {
                let Some((data, local, remote)) = pkt else { break; };
                let tx_reply = tx.clone();
                tokio::spawn(async move {
                    let bind_addr = if remote.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
                    if let Ok(sock) = UdpSocket::bind(bind_addr).await {
                        if sock.send_to(&data, remote).await.is_ok() {
                            let mut resp = vec![0u8; 4096];
                            if let Ok(Ok((n, _))) =
                                tokio::time::timeout(UDP_TIMEOUT, sock.recv_from(&mut resp)).await
                            {
                                resp.truncate(n);
                                let _ = tx_reply.send((resp, local, remote));
                            }
                        }
                    }
                });
            }
        }
    }

    writer_task.abort();
}

#[no_mangle]
pub extern "system" fn Java_com_qpi_freeview_FreeViewVpnService_nativeStartTun(
    mut env: JNIEnv,
    _class: JClass,
    tun_fd: jint,
    mode: JString,
    delay_ms: jlong,
) -> jboolean {
    let mode_str: String = env
        .get_string(&mode)
        .map(|s| s.into())
        .unwrap_or_else(|_| "TlsHeader5".to_string());
    let delay = delay_ms.max(0) as u64;

    match start_tun_engine(tun_fd as i32, &mode_str, delay) {
        Ok(()) => JNI_TRUE,
        Err(e) => {
            add_log(&format!("[JNI] TUN 시작 실패: {}", e));
            JNI_FALSE
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_qpi_freeview_FreeViewVpnService_nativeStopTun(
    _env: JNIEnv,
    _class: JClass,
) {
    stop_tun_engine();
}

#[no_mangle]
pub extern "system" fn Java_com_qpi_freeview_FreeViewVpnService_nativeIsRunning(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    if is_tun_running() {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}
