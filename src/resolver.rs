//! DNS 조회 및 사내망/로컬 도메인 분기 모듈

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use hickory_resolver::config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts};
use hickory_resolver::TokioAsyncResolver;

const DOH_LOCAL_SUFFIXES: &[&str] = &[
    ".local", ".localhost", ".internal", ".intranet",
    ".lan", ".home", ".corp", ".private", ".test",
    ".example", ".invalid",
];

pub struct DnsResolver {
    resolver: TokioAsyncResolver,
}

impl DnsResolver {
    pub fn new() -> Self {
        let mut config = ResolverConfig::new();

        // Cloudflare DNS (IPv4 & IPv6)
        config.add_name_server(NameServerConfig::new(
            SocketAddr::new(IpAddr::from([1, 1, 1, 1]), 53),
            Protocol::Udp,
        ));
        config.add_name_server(NameServerConfig::new(
            SocketAddr::new(IpAddr::from([1, 0, 0, 1]), 53),
            Protocol::Udp,
        ));
        config.add_name_server(NameServerConfig::new(
            SocketAddr::new(
                "2606:4700:4700::1111".parse().unwrap(),
                53,
            ),
            Protocol::Udp,
        ));

        // Google DNS 보조 (8.8.8.8)
        config.add_name_server(NameServerConfig::new(
            SocketAddr::new(IpAddr::from([8, 8, 8, 8]), 53),
            Protocol::Udp,
        ));

        let mut opts = ResolverOpts::default();
        opts.timeout = std::time::Duration::from_secs(3);
        opts.attempts = 2;
        opts.cache_size = 1024;

        let resolver = TokioAsyncResolver::tokio(config, opts);
        Self { resolver }
    }

    /// 주어진 호스트명이 사내망/로컬 전용 도메인인지 검사합니다.
    pub fn is_local_domain(&self, hostname: &str) -> bool {
        // 1. IP 주소 리터럴인지 검사
        if hostname.parse::<IpAddr>().is_ok() {
            return true;
        }

        // 2. 단일 라벨 호스트명 (점이 없는 내부 서버명, 예: 'intranet', 'nas')
        if !hostname.contains('.') {
            return true;
        }

        let lower = hostname.to_lowercase();
        for suffix in DOH_LOCAL_SUFFIXES {
            if lower == &suffix[1..] || lower.ends_with(suffix) {
                return true;
            }
        }

        false
    }

    /// 대상 호스트와 포트를 `SocketAddr` 목록으로 해석합니다.
    /// 로컬 도메인은 OS 기본 DNS로, 일반 인터넷 도메인은 외부 안전 DNS(1.1.1.1 등)로 해석합니다.
    pub async fn resolve(&self, host: &str, port: u16) -> Vec<SocketAddr> {
        // 1. 이미 IP 리터럴인 경우
        if let Ok(ip) = host.parse::<IpAddr>() {
            return vec![SocketAddr::new(ip, port)];
        }

        // 2. 사내망/로컬 도메인은 OS DNS 우선 사용
        if self.is_local_domain(host) {
            if let Ok(iter) = tokio::net::lookup_host(format!("{}:{}", host, port)).await {
                return iter.collect();
            }
        }

        // 3. 외부 보안 DNS(1.1.1.1) 질의
        let mut results = Vec::new();
        if let Ok(lookup) = self.resolver.lookup_ip(host).await {
            for ip in lookup.iter() {
                results.push(SocketAddr::new(ip, port));
            }
        }

        // 4. 외부 DNS 실패 시 OS DNS 자동 폴백
        if results.is_empty() {
            if let Ok(iter) = tokio::net::lookup_host(format!("{}:{}", host, port)).await {
                results.extend(iter);
            }
        }

        results
    }
}

pub type SharedResolver = Arc<DnsResolver>;
