//! 비동기 로컬 프록시 서버 모듈 (HTTP CONNECT 및 일반 HTTP 지원)

use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use crate::desync::{self, DesyncConfig};
use crate::resolver::SharedResolver;

const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);

pub struct ProxyServer {
    pub port: u16,
    active_connections: Arc<AtomicUsize>,
    shutdown_rx: watch::Receiver<bool>,
}

impl ProxyServer {
    pub async fn bind(
        start_port: u16,
        shutdown_rx: watch::Receiver<bool>,
    ) -> io::Result<(Self, TcpListener)> {
        let max_port = start_port + 20;
        let mut chosen_listener = None;
        let mut chosen_port = start_port;

        for port in start_port..=max_port {
            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            match TcpListener::bind(addr).await {
                Ok(l) => {
                    chosen_listener = Some(l);
                    chosen_port = port;
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        let listener = chosen_listener.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("포트 {}..{} 가 모두 점유 중입니다.", start_port, max_port),
            )
        })?;

        let server = Self {
            port: chosen_port,
            active_connections: Arc::new(AtomicUsize::new(0)),
            shutdown_rx,
        };

        Ok((server, listener))
    }

    pub async fn run(
        self: Arc<Self>,
        listener: TcpListener,
        resolver: SharedResolver,
        desync_config: DesyncConfig,
    ) {
        let mut shutdown_rx = self.shutdown_rx.clone();

        loop {
            tokio::select! {
                res = listener.accept() => {
                    match res {
                        Ok((client_stream, _client_addr)) => {
                            let resolver = resolver.clone();
                            let desync_config = desync_config.clone();
                            let active_connections = self.active_connections.clone();
                            let port = self.port;

                            tokio::spawn(async move {
                                active_connections.fetch_add(1, Ordering::SeqCst);
                                let _ = handle_client(client_stream, port, resolver, desync_config).await;
                                active_connections.fetch_sub(1, Ordering::SeqCst);
                            });
                        }
                        Err(_) => {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        break;
                    }
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn get_active_connections(&self) -> usize {
        self.active_connections.load(Ordering::Relaxed)
    }
}

async fn handle_client(
    mut client: TcpStream,
    listen_port: u16,
    resolver: SharedResolver,
    desync_cfg: DesyncConfig,
) -> io::Result<()> {
    let _ = client.set_nodelay(true);

    let mut header_buf = vec![0u8; 8192];
    let mut total_read = 0;

    let header_end_pos = loop {
        let read_fut = client.read(&mut header_buf[total_read..]);
        let n = match tokio::time::timeout(HEADER_TIMEOUT, read_fut).await {
            Ok(Ok(n)) => n,
            _ => return Ok(()),
        };

        if n == 0 {
            return Ok(());
        }
        total_read += n;

        if let Some(pos) = find_subsequence(&header_buf[..total_read], b"\r\n\r\n") {
            break pos + 4;
        }
        if total_read >= header_buf.len() {
            return Ok(());
        }
    };

    let header_str = match std::str::from_utf8(&header_buf[..header_end_pos]) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    let mut lines = header_str.split("\r\n");
    let req_line = match lines.next() {
        Some(l) if !l.is_empty() => l,
        _ => return Ok(()),
    };

    let parts: Vec<&str> = req_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }

    let method = parts[0];
    let target = parts[1];

    // 1. PAC 파일 요청 처리
    if method == "GET" && target == "/proxy.pac" {
        let pac_body = format!(
            "function FindProxyForURL(url, host) {{\n  return \"PROXY 127.0.0.1:{}; DIRECT\";\n}}\n",
            listen_port
        );
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/x-ns-proxy-autoconfig\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            pac_body.len(),
            pac_body
        );
        client.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    // 2. HTTPS CONNECT 터널링
    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = match parse_host_port(target, 443) {
            Some(hp) => hp,
            None => {
                let _ = client.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
                return Ok(());
            }
        };

        let addrs = resolver.resolve(&host, port).await;
        if addrs.is_empty() {
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await;
            return Ok(());
        }

        let mut upstream = match connect_any(&addrs).await {
            Ok(s) => s,
            Err(_) => {
                let _ = client.write_all(b"HTTP/1.1 504 Gateway Timeout\r\n\r\n").await;
                return Ok(());
            }
        };
        let _ = upstream.set_nodelay(true);

        // 클라이언트에 터널 수립 성공 회신
        client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;

        // 버그 11 수정: CONNECT 헤더 뒤에 이미 읽힌 잔여 데이터가 있는지 확인
        let leftover = &header_buf[header_end_pos..total_read];
        let mut first_packet = Vec::with_capacity(16384);
        if !leftover.is_empty() {
            first_packet.extend_from_slice(leftover);
        } else {
            let mut buf = vec![0u8; 16384];
            let n = client.read(&mut buf).await?;
            if n == 0 {
                return Ok(());
            }
            first_packet.extend_from_slice(&buf[..n]);
        }

        // SNI 탈동기화(Desync) 적용 전송
        desync::send_first(&mut upstream, &first_packet, &desync_cfg).await?;
        crate::logging::add_log(&format!("[우회] HTTPS {}:{} (SNI 분할 우회 중계)", host, port));

        // 이후 패킷은 양방향 고속 중계
        let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
        return Ok(());
    }

    // 3. 일반 HTTP 프록시 요청 (절대 URI: GET http://example.com/foo HTTP/1.1)
    if target.starts_with("http://") {
        let trimmed = &target[7..];
        let (host_part, path_part) = match trimmed.find('/') {
            Some(idx) => (&trimmed[..idx], &trimmed[idx..]),
            None => (trimmed, "/"),
        };

        let (host, port) = match parse_host_port(host_part, 80) {
            Some(hp) => hp,
            None => return Ok(()),
        };

        let addrs = resolver.resolve(&host, port).await;
        if addrs.is_empty() {
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await;
            return Ok(());
        }

        let mut upstream = match connect_any(&addrs).await {
            Ok(s) => s,
            Err(_) => {
                let _ = client.write_all(b"HTTP/1.1 504 Gateway Timeout\r\n\r\n").await;
                return Ok(());
            }
        };
        let _ = upstream.set_nodelay(true);

        // 버그 12 수정: Proxy-* 홉별 헤더 필터링 및 Connection: close 강제
        let proto = parts.get(2).copied().unwrap_or("HTTP/1.1");
        let mut sanitized_headers = format!("{} {} {}\r\n", method, path_part, proto);

        for line in header_str[req_line.len() + 2..header_end_pos - 4].split("\r\n") {
            let lower = line.to_lowercase();
            if lower.starts_with("proxy-") || lower.starts_with("connection:") {
                continue;
            }
            sanitized_headers.push_str(line);
            sanitized_headers.push_str("\r\n");
        }
        sanitized_headers.push_str("Connection: close\r\n\r\n");

        upstream.write_all(sanitized_headers.as_bytes()).await?;
        crate::logging::add_log(&format!("[중계] HTTP {}:{} ({})", host, port, path_part));

        // 바디 데이터(POST 등)가 함께 읽혀 있었다면 전달
        let leftover = &header_buf[header_end_pos..total_read];
        if !leftover.is_empty() {
            upstream.write_all(leftover).await?;
        }
        upstream.flush().await?;

        let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
        return Ok(());
    }

    Ok(())
}

fn parse_host_port(s: &str, default_port: u16) -> Option<(String, u16)> {
    if s.starts_with('[') {
        let close_bracket = s.find(']')?;
        let host = s[1..close_bracket].to_string();
        let rest = &s[close_bracket + 1..];
        let port = if let Some(colon) = rest.find(':') {
            rest[colon + 1..].parse().ok()?
        } else {
            default_port
        };
        Some((host, port))
    } else if let Some(colon) = s.rfind(':') {
        let host = s[..colon].to_string();
        let port = s[colon + 1..].parse().ok()?;
        Some((host, port))
    } else {
        Some((s.to_string(), default_port))
    }
}

async fn connect_any(addrs: &[SocketAddr]) -> io::Result<TcpStream> {
    let mut last_err = None;
    for addr in addrs {
        match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(e)) => last_err = Some(e),
            Err(_) => {
                last_err = Some(io::Error::new(io::ErrorKind::TimedOut, "연결 타임아웃"))
            }
        }
    }
    Err(last_err.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "연결 가능한 주소가 없습니다.")
    }))
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}
