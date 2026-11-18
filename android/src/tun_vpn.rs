//! Android VpnService TUN 패킷 처리 및 SNI Desync 엔진
#![cfg(target_os = "android")]

use std::fs::File;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::os::unix::io::FromRawFd;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
use tokio::sync::{mpsc, watch, Semaphore};
use tokio::task::{JoinHandle, JoinSet};

use crate::desync::{self, DesyncConfig, DesyncMode};
use crate::logging::add_log;
use crate::tls;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const CLIENT_HELLO_TIMEOUT: Duration = Duration::from_secs(3);
const UDP_TIMEOUT: Duration = Duration::from_secs(5);
const TCP_IDLE_TIMEOUT: Duration = Duration::from_secs(180);
const IDLE_TIMER_RESET_INTERVAL: Duration = Duration::from_secs(5);
const TCP_RELAY_BUF_BYTES: usize = 16 * 1024;
const MAX_CONCURRENT_TCP: usize = 256;
const MAX_CONCURRENT_UDP: usize = 128;
const MAX_FIRST_CHUNK: usize = 32768;

struct TunEngineState {
    runtime: Runtime,
    shutdown_tx: Option<watch::Sender<bool>>,
    task_handle: Option<JoinHandle<()>>,
}

struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

static TUN_STATE: OnceLock<Mutex<TunEngineState>> = OnceLock::new();
static TUN_RUNNING: AtomicBool = AtomicBool::new(false);
static TUN_GEN: AtomicU64 = AtomicU64::new(0);

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
            task_handle: None,
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
    if tun_fd < 0 {
        return Err(format!("유효하지 않은 TUN FD: {}", tun_fd));
    }

    // 소유권을 즉시 확보해 이후 초기화 실패 시에도 커널 FD가 누수되지 않도록 보장
    let file = unsafe { File::from_raw_fd(tun_fd) };

    stop_tun_engine();

    if let Err(e) = set_nonblocking(tun_fd) {
        return Err(format!("TUN O_NONBLOCK 설정 실패: {}", e));
    }

    let mode = match mode_str {
        "SniSplit" => DesyncMode::SniSplit,
        "TlsRecordFrag" => DesyncMode::TlsRecordFrag,
        _ => DesyncMode::TlsHeader5,
    };
    let desync_cfg = DesyncConfig { mode, delay_ms };

    let mut state = get_tun_state()
        .lock()
        .map_err(|_| "TUN 상태 잠금 실패".to_string())?;

    let my_gen = TUN_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    state.shutdown_tx = Some(shutdown_tx);
    TUN_RUNNING.store(true, Ordering::SeqCst);

    let handle = state.runtime.spawn(async move {
        if let Err(e) = run_tun_stack(file, desync_cfg, shutdown_rx).await {
            add_log(&format!("[TUN 오류] {}", e));
        }
        if TUN_GEN.load(Ordering::SeqCst) == my_gen {
            TUN_RUNNING.store(false, Ordering::SeqCst);
        }
    });
    state.task_handle = Some(handle);

    add_log(&format!(
        "Android VPN TUN 엔진 시작 (fd={}, mode={}, delay={}ms)",
        tun_fd, mode_str, delay_ms
    ));
    Ok(())
}

pub fn stop_tun_engine() {
    TUN_GEN.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut state) = get_tun_state().lock() {
        if let Some(tx) = state.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        if let Some(handle) = state.task_handle.take() {
            handle.abort();
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

    let _runner_guard = runner.map(|r| AbortOnDrop(tokio::spawn(r)));

    let (mut stack_sink, mut stack_stream) = stack.split();

    // 1. TUN -> Netstack
    let tun_reader = async_tun.clone();
    let mut rx_shut = shutdown_rx.clone();
    let mut read_guard = AbortOnDrop(tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];
        loop {
            if *rx_shut.borrow() {
                break;
            }
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
    }));

    // 2. Netstack -> TUN
    let tun_writer = async_tun.clone();
    let mut tx_shut = shutdown_rx.clone();
    let mut write_guard = AbortOnDrop(tokio::spawn(async move {
        loop {
            if *tx_shut.borrow() {
                break;
            }
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
    }));

    // 3. TCP 연결 처리 (SNI Desync 적용)
    let tcp_shut = shutdown_rx.clone();
    let _tcp_guard = AbortOnDrop(tokio::spawn(handle_tcp_connections(
        tcp_listener,
        desync_cfg,
        tcp_shut,
    )));

    // 4. UDP 패킷 처리 (DNS 및 일반 UDP 포워딩)
    let udp_shut = shutdown_rx.clone();
    let _udp_guard = AbortOnDrop(tokio::spawn(handle_udp_packets(udp_socket, udp_shut)));

    // 종료 신호 또는 TUN I/O 종료 대기 (AbortOnDrop 가드가 모든 하위 태스크 자동 정리)
    if !*shutdown_rx.borrow() {
        tokio::select! {
            _ = shutdown_rx.changed() => {}
            _ = &mut read_guard.0 => {}
            _ = &mut write_guard.0 => {}
        }
    }

    Ok(())
}

async fn handle_tcp_connections(
    mut tcp_listener: TcpListener,
    desync_cfg: DesyncConfig,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_TCP));
    let mut join_set = JoinSet::new();

    loop {
        if *shutdown_rx.borrow() {
            break;
        }
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() { break; }
            }
            Some(_) = join_set.join_next(), if !join_set.is_empty() => {}
            conn = tcp_listener.next() => {
                let Some((stream, _local, remote)) = conn else { break; };
                let Ok(permit) = semaphore.clone().try_acquire_owned() else {
                    drop(stream);
                    continue;
                };
                let cfg = desync_cfg.clone();
                let task_shut = shutdown_rx.clone();
                join_set.spawn(async move {
                    let _permit = permit;
                    let _ = relay_tcp_with_desync(stream, remote, cfg, task_shut).await;
                });
            }
        }
    }
}

async fn relay_half_with_timeout<R, W>(mut reader: R, mut writer: W, idle_timeout: Duration)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; TCP_RELAY_BUF_BYTES];
    let idle_sleep = tokio::time::sleep(idle_timeout);
    tokio::pin!(idle_sleep);
    let mut last_timer_reset = tokio::time::Instant::now();
    let reset_interval = IDLE_TIMER_RESET_INTERVAL.min(idle_timeout / 4);

    loop {
        let n = tokio::select! {
            biased;
            res = reader.read(&mut buf) => match res {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            },
            _ = &mut idle_sleep => break,
        };

        if writer.write_all(&buf[..n]).await.is_err() {
            break;
        }

        if last_timer_reset.elapsed() >= reset_interval {
            let now = tokio::time::Instant::now();
            idle_sleep.as_mut().reset(now + idle_timeout);
            last_timer_reset = now;
        }
    }
    let _ = writer.shutdown().await;
}

async fn relay_bidirectional_tun(
    client: netstack_smoltcp::TcpStream,
    upstream: TcpStream,
    shutdown_rx: &mut watch::Receiver<bool>,
) {
    let (cr, cw) = tokio::io::split(client);
    let (ur, uw) = upstream.into_split();

    tokio::select! {
        _ = async {
            tokio::join!(
                relay_half_with_timeout(cr, uw, TCP_IDLE_TIMEOUT),
                relay_half_with_timeout(ur, cw, TCP_IDLE_TIMEOUT)
            );
        } => {}
        _ = shutdown_rx.changed() => {}
    }
}

async fn relay_tcp_with_desync(
    mut client: netstack_smoltcp::TcpStream,
    remote: SocketAddr,
    desync_cfg: DesyncConfig,
    mut shutdown_rx: watch::Receiver<bool>,
) -> io::Result<()> {
    let mut upstream =
        tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(remote)).await??;
    let _ = upstream.set_nodelay(true);

    {
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
    }

    relay_bidirectional_tun(client, upstream, &mut shutdown_rx).await;
    Ok(())
}

async fn handle_udp_packets(
    udp_socket: StackUdpSocket,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let (tx, mut rx) = mpsc::channel::<(Vec<u8>, SocketAddr, SocketAddr)>(512);
    let (mut read_half, mut write_half) = udp_socket.split();
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_UDP));
    let mut join_set = JoinSet::new();

    let _writer_guard = AbortOnDrop(tokio::spawn(async move {
        while let Some((data, local, remote)) = rx.recv().await {
            if write_half.send((data, remote, local)).await.is_err() {
                break;
            }
        }
    }));

    loop {
        if *shutdown_rx.borrow() {
            break;
        }
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() { break; }
            }
            Some(_) = join_set.join_next(), if !join_set.is_empty() => {}
            pkt = read_half.next() => {
                let Some((data, local, remote)) = pkt else { break; };
                let Ok(permit) = semaphore.clone().try_acquire_owned() else {
                    continue;
                };
                let tx_reply = tx.clone();
                join_set.spawn(async move {
                    let _permit = permit;
                    let bind_addr = if remote.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
                    if let Ok(sock) = UdpSocket::bind(bind_addr).await {
                        if sock.send_to(&data, remote).await.is_ok() {
                            let mut resp = vec![0u8; 4096];
                            if let Ok(Ok((n, _))) =
                                tokio::time::timeout(UDP_TIMEOUT, sock.recv_from(&mut resp)).await
                            {
                                resp.truncate(n);
                                let _ = tx_reply.try_send((resp, local, remote));
                            }
                        }
                    }
                });
            }
        }
    }
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
