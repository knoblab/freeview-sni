//! 비동기 로컬 프록시 서버 모듈 (HTTP CONNECT 및 일반 HTTP 지원)

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Semaphore};
use tokio::task::JoinSet;

use crate::desync::{self, DesyncConfig};
use crate::resolver::SharedResolver;

const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
const CLIENT_HELLO_TIMEOUT: Duration = Duration::from_secs(3);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const TUNNEL_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_HEADER_BYTES: usize = 8192;
const MAX_CLIENT_HELLO_BYTES: usize = 32768;
const MAX_CONCURRENT_CONNECTIONS: usize = 512;

struct ConnectionGuard(Arc<AtomicUsize>);

impl ConnectionGuard {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

pub struct ProxyServer {
    pub port: u16,
    active_connections: Arc<AtomicUsize>,
    semaphore: Arc<Semaphore>,
    shutdown_rx: watch::Receiver<bool>,
}

impl ProxyServer {
    pub async fn bind(
        start_port: u16,
        shutdown_rx: watch::Receiver<bool>,
    ) -> io::Result<(Self, TcpListener)> {
        let max_port = start_port.saturating_add(20);
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
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS)),
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
                            let Ok(permit) = self.semaphore.clone().try_acquire_owned() else {
                                drop(client_stream);
                                continue;
                            };

                            let resolver = resolver.clone();
                            let desync_config = desync_config.clone();
                            let active_connections = self.active_connections.clone();
                            let port = self.port;
                            let task_shutdown = self.shutdown_rx.clone();

                            tokio::spawn(async move {
                                let _permit = permit;
                                let _guard = ConnectionGuard::new(active_connections);
                                let _ = handle_client(
                                    client_stream,
                                    port,
                                    resolver,
                                    desync_config,
                                    task_shutdown,
                                )
                                .await;
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
    mut shutdown_rx: watch::Receiver<bool>,
) -> io::Result<()> {
    let _ = client.set_nodelay(true);

    // 단일 버퍼(16KB)를 헤더 수신 및 첫 TLS 패킷 수신에 재사용
    let mut buf = vec![0u8; 16384];
    let mut total_read = 0usize;

    let header_end_pos = loop {
        let read_limit = MAX_HEADER_BYTES.min(buf.len());
        if total_read >= read_limit {
            return Ok(());
        }

        let read_fut = client.read(&mut buf[total_read..read_limit]);
        let n = match tokio::time::timeout(HEADER_TIMEOUT, read_fut).await {
            Ok(Ok(n)) => n,
            _ => return Ok(()),
        };

        if n == 0 {
            return Ok(());
        }

        let search_start = total_read.saturating_sub(3);
        total_read += n;

        if let Some(rel_pos) = find_subsequence(&buf[search_start..total_read], b"\r\n\r\n") {
            break search_start + rel_pos + 4;
        }
    };

    let header_str = match std::str::from_utf8(&buf[..header_end_pos]) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    let mut lines = header_str.split("\r\n");
    let req_line = match lines.next() {
        Some(l) if !l.is_empty() => l,
        _ => return Ok(()),
    };

    let mut parts = req_line.split_whitespace();
    let method = match parts.next() {
        Some(m) => m,
        None => return Ok(()),
    };
    let target = match parts.next() {
        Some(t) => t,
        None => return Ok(()),
    };
    let proto = parts.next().unwrap_or("HTTP/1.1");

    // 1. PAC 파일 요청 처리
    if method.eq_ignore_ascii_case("GET") && (target == "/proxy.pac" || target.ends_with("/proxy.pac")) {
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

        if is_self_hostname(&host, port, listen_port) {
            let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n").await;
            return Ok(());
        }

        let addrs = resolver.resolve(&host, port).await;
        if addrs.is_empty() {
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await;
            return Ok(());
        }

        if is_self_target(&addrs, listen_port) {
            let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n").await;
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

        // 잔여 데이터를 버퍼 앞으로 이동하고, 첫 패킷(또는 완전한 TLS ClientHello) 수신
        let leftover_len = total_read - header_end_pos;
        if leftover_len > 0 {
            buf.copy_within(header_end_pos..total_read, 0);
        }
        let mut hello_len = leftover_len;

        if hello_len == 0 {
            match tokio::time::timeout(CLIENT_HELLO_TIMEOUT, client.read(&mut buf)).await {
                Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return Ok(()),
                Ok(Ok(n)) => hello_len = n,
            }
        }

        // TLS Handshake(0x16)인 경우 다중 세그먼트 ClientHello(예: ML-KEM768 > 1460B)까지 완전히 수집
        if hello_len > 0 && buf[0] == 0x16 {
            while hello_len < 5 {
                match tokio::time::timeout(Duration::from_millis(1500), client.read(&mut buf[hello_len..])).await {
                    Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
                    Ok(Ok(n)) => hello_len += n,
                }
            }

            if let Some(expected_total) = crate::tls::tls_record_total_len(&buf[..hello_len]) {
                let target_len = expected_total.min(MAX_CLIENT_HELLO_BYTES);
                if target_len > buf.len() {
                    buf.resize(target_len, 0);
                }
                while hello_len < target_len {
                    match tokio::time::timeout(Duration::from_millis(1500), client.read(&mut buf[hello_len..target_len])).await {
                        Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
                        Ok(Ok(n)) => hello_len += n,
                    }
                }
            }
        }

        // SNI 탈동기화(Desync) 적용 전송
        desync::send_first(&mut upstream, &buf[..hello_len], &desync_cfg).await?;
        crate::logging::add_debug_log(&format!("[우회] HTTPS {}:{} (SNI 분할 우회 중계)", host, port));

        drop(buf);
        relay_bidirectional(client, upstream, &mut shutdown_rx).await;
        return Ok(());
    }

    // 3. 일반 HTTP 프록시 요청 (절대 URI: GET http://example.com/foo HTTP/1.1)
    if target.len() > 7 && target[..7].eq_ignore_ascii_case("http://") {
        let (host_part, path_part) = split_http_target(&target[7..]);

        let (host, port) = match parse_host_port(host_part, 80) {
            Some(hp) => hp,
            None => {
                let _ = client.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
                return Ok(());
            }
        };

        if is_self_hostname(&host, port, listen_port) {
            let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n").await;
            return Ok(());
        }

        let addrs = resolver.resolve(&host, port).await;
        if addrs.is_empty() {
            let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\n\r\n").await;
            return Ok(());
        }

        if is_self_target(&addrs, listen_port) {
            let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n").await;
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

        let sanitized_headers = build_sanitized_http_headers(method, &path_part, proto, lines);
        upstream.write_all(sanitized_headers.as_bytes()).await?;
        crate::logging::add_debug_log(&format!("[중계] HTTP {}:{} ({})", host, port, path_part));

        // 바디 데이터(POST 등)가 함께 읽혀 있었다면 전달
        if total_read > header_end_pos {
            upstream.write_all(&buf[header_end_pos..total_read]).await?;
        }

        drop(buf);
        relay_bidirectional(client, upstream, &mut shutdown_rx).await;
        return Ok(());
    }

    Ok(())
}

fn split_http_target(trimmed: &str) -> (&str, String) {
    match trimmed.find(['/', '?']) {
        Some(idx) => {
            if trimmed.as_bytes()[idx] == b'?' {
                (&trimmed[..idx], format!("/{}", &trimmed[idx..]))
            } else {
                (&trimmed[..idx], trimmed[idx..].to_string())
            }
        }
        None => (trimmed, "/".to_string()),
    }
}

fn build_sanitized_http_headers<'a, I>(method: &str, path_part: &str, proto: &str, lines: I) -> String
where
    I: Iterator<Item = &'a str>,
{
    let mut sanitized_headers = format!("{} {} {}\r\n", method, path_part, proto);

    for line in lines {
        if line.is_empty() {
            break;
        }
        let is_proxy_hdr = line.len() >= 6 && line[..6].eq_ignore_ascii_case("proxy-");
        let is_conn_hdr = line.len() >= 11 && line[..11].eq_ignore_ascii_case("connection:");
        if is_proxy_hdr || is_conn_hdr {
            continue;
        }
        sanitized_headers.push_str(line);
        sanitized_headers.push_str("\r\n");
    }

    sanitized_headers.push_str("Connection: close\r\n\r\n");
    sanitized_headers
}

async fn relay_half<R, W>(mut reader: R, mut writer: W)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buf = [0u8; 8192];
    loop {
        match tokio::time::timeout(TUNNEL_IDLE_TIMEOUT, reader.read(&mut buf)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
            Ok(Ok(n)) => {
                if writer.write_all(&buf[..n]).await.is_err() {
                    break;
                }
            }
        }
    }
    let _ = writer.shutdown().await;
}

async fn relay_bidirectional(
    client: TcpStream,
    upstream: TcpStream,
    shutdown_rx: &mut watch::Receiver<bool>,
) {
    let (cr, cw) = client.into_split();
    let (ur, uw) = upstream.into_split();

    tokio::select! {
        _ = async {
            tokio::join!(relay_half(cr, uw), relay_half(ur, cw));
        } => {}
        _ = shutdown_rx.changed() => {}
    }
}

fn is_self_hostname(host: &str, port: u16, listen_port: u16) -> bool {
    if port != listen_port {
        return false;
    }
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return ip.is_loopback();
    }
    false
}

fn is_self_target(addrs: &[SocketAddr], listen_port: u16) -> bool {
    addrs.iter().any(|a| a.port() == listen_port && a.ip().is_loopback())
}

fn parse_host_port(s: &str, default_port: u16) -> Option<(String, u16)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    if s.starts_with('[') {
        let close_bracket = s.find(']')?;
        let host = s[1..close_bracket].to_string();
        if host.is_empty() {
            return None;
        }
        let rest = &s[close_bracket + 1..];
        let port = if let Some(colon) = rest.find(':') {
            rest[colon + 1..].parse().ok()?
        } else {
            default_port
        };
        Some((host, port))
    } else if s.matches(':').count() > 1 {
        // 브래킷 없는 IPv6 리터럴
        s.parse::<IpAddr>().ok()?;
        Some((s.to_string(), default_port))
    } else if let Some(colon) = s.rfind(':') {
        let host = &s[..colon];
        if host.is_empty() {
            return None;
        }
        let port = s[colon + 1..].parse().ok()?;
        Some((host.to_string(), port))
    } else {
        Some((s.to_string(), default_port))
    }
}

fn interleave_addrs(addrs: &[SocketAddr]) -> Vec<SocketAddr> {
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for &addr in addrs {
        if addr.is_ipv4() {
            v4.push(addr);
        } else {
            v6.push(addr);
        }
    }

    let mut ordered = Vec::with_capacity(addrs.len());
    let max_len = v4.len().max(v6.len());
    for i in 0..max_len {
        if let Some(&a) = v4.get(i) {
            ordered.push(a);
        }
        if let Some(&a) = v6.get(i) {
            ordered.push(a);
        }
    }
    ordered
}

async fn connect_single(addr: SocketAddr) -> io::Result<TcpStream> {
    match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
        Ok(res) => res,
        Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "연결 타임아웃")),
    }
}

async fn connect_any(addrs: &[SocketAddr]) -> io::Result<TcpStream> {
    let ordered = interleave_addrs(addrs);
    if ordered.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "연결 가능한 주소가 없습니다.",
        ));
    }

    let mut join_set = JoinSet::new();
    let mut idx = 0usize;
    let mut last_err = None;

    join_set.spawn(connect_single(ordered[idx]));
    idx += 1;

    let stagger = tokio::time::sleep(Duration::from_millis(250));
    tokio::pin!(stagger);

    loop {
        if join_set.is_empty() && idx >= ordered.len() {
            break;
        }

        tokio::select! {
            Some(res) = join_set.join_next() => {
                match res {
                    Ok(Ok(stream)) => return Ok(stream),
                    Ok(Err(e)) => {
                        last_err = Some(e);
                        if idx < ordered.len() {
                            join_set.spawn(connect_single(ordered[idx]));
                            idx += 1;
                            stagger.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(250));
                        }
                    }
                    Err(_) => {}
                }
            }
            _ = &mut stagger, if idx < ordered.len() => {
                join_set.spawn(connect_single(ordered[idx]));
                idx += 1;
                stagger.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(250));
            }
        }
    }

    Err(last_err.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "연결 가능한 주소가 없습니다.")
    }))
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn test_parse_host_port_edge_cases() {
        assert_eq!(parse_host_port("example.com:443", 443), Some(("example.com".to_string(), 443)));
        assert_eq!(parse_host_port("example.com", 80), Some(("example.com".to_string(), 80)));
        assert_eq!(parse_host_port("[::1]:8443", 443), Some(("::1".to_string(), 8443)));
        assert_eq!(parse_host_port("2001:db8::1", 443), Some(("2001:db8::1".to_string(), 443)));
        assert_eq!(parse_host_port(":443", 443), None);
        assert_eq!(parse_host_port("", 443), None);
    }

    #[test]
    fn test_headerless_http_request_no_panic() {
        let req = "GET http://example.com?foo=bar HTTP/1.1\r\n\r\n";
        let mut lines = req.split("\r\n");
        let req_line = lines.next().unwrap();
        let mut parts = req_line.split_whitespace();
        let method = parts.next().unwrap();
        let target = parts.next().unwrap();
        let proto = parts.next().unwrap();

        let (host_part, path_part) = split_http_target(&target[7..]);
        assert_eq!(host_part, "example.com");
        assert_eq!(path_part, "/?foo=bar");

        let sanitized = build_sanitized_http_headers(method, &path_part, proto, lines);
        assert_eq!(sanitized, "GET /?foo=bar HTTP/1.1\r\nConnection: close\r\n\r\n");
    }

    #[test]
    fn test_self_target_loop_prevention() {
        assert!(is_self_hostname("localhost", 8080, 8080));
        assert!(is_self_hostname("127.0.0.1", 8080, 8080));
        assert!(!is_self_hostname("127.0.0.1", 8081, 8080));

        let loop_addrs = vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080)];
        let v6_loop = vec![SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 8080)];
        let external = vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 8080)];
        assert!(is_self_target(&loop_addrs, 8080));
        assert!(is_self_target(&v6_loop, 8080));
        assert!(!is_self_target(&external, 8080));
    }
}
