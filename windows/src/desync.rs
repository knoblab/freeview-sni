//! SNI 검열 우회를 위한 패킷 탈동기화(Desync) 모듈

use std::io;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use crate::tls;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesyncMode {
    /// 2.0.0 검증 기본 모드: TLS 레코드 헤더(5바이트)를 먼저 전송하고 지연(기본 5ms) 후 나머지 전송
    TlsHeader5,
    /// SNI 호스트명 문자열의 중간 지점에서 TCP 세그먼트를 분할 전송
    SniSplit,
    /// TLS ClientHello를 2개의 합법적인 TLS 레코드로 분할 재포장(RFC 호환 레코드 단편화)
    TlsRecordFrag,
}

impl Default for DesyncMode {
    fn default() -> Self {
        DesyncMode::TlsHeader5
    }
}

#[derive(Debug, Clone)]
pub struct DesyncConfig {
    pub mode: DesyncMode,
    pub delay_ms: u64,
}

impl Default for DesyncConfig {
    fn default() -> Self {
        Self {
            mode: DesyncMode::TlsHeader5,
            delay_ms: 2,
        }
    }
}

/// TLS ClientHello 레코드의 페이로드를 SNI 호스트명 중간 지점에서
/// 2개의 유효한 TLS Handshake 레코드 `(rec1, rec2)`로 분할합니다.
fn fragment_tls_record(data: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if data.len() < 5 {
        return None;
    }
    let rec_len = u16::from_be_bytes([data[3], data[4]]) as usize;
    if rec_len <= 4 || data.len() < 5 + rec_len || rec_len > 16384 {
        return None;
    }

    let record_type = data[0]; // 0x16 (Handshake)
    let proto_ver = [data[1], data[2]];
    let payload = &data[5..5 + rec_len];
    let trailing = &data[5 + rec_len..];

    let cut = match tls::find_sni_range(&data[..5 + rec_len]) {
        Some((offset, len)) if offset > 5 => (offset - 5) + (len / 2),
        _ => payload.len() / 2,
    };
    let cut = std::cmp::max(1, std::cmp::min(cut, payload.len() - 1));

    let part1 = &payload[..cut];
    let part2 = &payload[cut..];

    let mut rec1 = Vec::with_capacity(5 + part1.len());
    rec1.push(record_type);
    rec1.extend_from_slice(&proto_ver);
    rec1.extend_from_slice(&(part1.len() as u16).to_be_bytes());
    rec1.extend_from_slice(part1);

    let mut rec2 = Vec::with_capacity(5 + part2.len() + trailing.len());
    rec2.push(record_type);
    rec2.extend_from_slice(&proto_ver);
    rec2.extend_from_slice(&(part2.len() as u16).to_be_bytes());
    rec2.extend_from_slice(part2);
    rec2.extend_from_slice(trailing);

    Some((rec1, rec2))
}

async fn write_split(upstream: &mut TcpStream, first: &[u8], second: &[u8], delay_ms: u64) -> io::Result<()> {
    upstream.write_all(first).await?;
    if !second.is_empty() {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        upstream.write_all(second).await?;
    }
    Ok(())
}

/// 클라이언트가 보낸 첫 번째 업스트림 데이터(주로 TLS ClientHello)를
/// 설정된 전략에 따라 분할/지연하여 업스트림 소켓으로 전송합니다.
pub async fn send_first(
    upstream: &mut TcpStream,
    data: &[u8],
    config: &DesyncConfig,
) -> io::Result<()> {
    if !tls::is_client_hello(data) {
        // TLS 핸드셰이크가 아닌 경우 일반 전송
        return upstream.write_all(data).await;
    }

    match config.mode {
        DesyncMode::TlsHeader5 => {
            let split_index = std::cmp::min(data.len(), 5);
            write_split(upstream, &data[..split_index], &data[split_index..], config.delay_ms).await
        }

        DesyncMode::SniSplit => {
            let cut = match tls::find_sni_range(data) {
                Some((offset, len)) if len > 1 => offset + (len / 2),
                _ => std::cmp::min(data.len(), 5), // SNI 파싱 실패 시 5바이트로 폴백
            };
            write_split(upstream, &data[..cut], &data[cut..], config.delay_ms).await
        }

        DesyncMode::TlsRecordFrag => {
            if let Some((rec1, rec2)) = fragment_tls_record(data) {
                write_split(upstream, &rec1, &rec2, config.delay_ms).await
            } else {
                // 부분 레코드이거나 비표준 길이인 경우 안전한 5바이트 분할로 폴백
                let split_index = std::cmp::min(data.len(), 5);
                write_split(upstream, &data[..split_index], &data[split_index..], config.delay_ms).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_frag_logic() {
        let sample: Vec<u8> = vec![
            0x16, 0x03, 0x01, 0x00, 0x0a,
            0x01, 0x00, 0x00, 0x06,
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        ];

        let rec_len = u16::from_be_bytes([sample[3], sample[4]]) as usize;
        assert_eq!(rec_len, 10);
        let payload = &sample[5..5 + rec_len];
        let cut = payload.len() / 2;
        let part1 = &payload[..cut];
        let part2 = &payload[cut..];

        let mut rec1 = Vec::new();
        rec1.push(sample[0]);
        rec1.extend_from_slice(&sample[1..3]);
        rec1.extend_from_slice(&(part1.len() as u16).to_be_bytes());
        rec1.extend_from_slice(part1);

        let mut rec2 = Vec::new();
        rec2.push(sample[0]);
        rec2.extend_from_slice(&sample[1..3]);
        rec2.extend_from_slice(&(part2.len() as u16).to_be_bytes());
        rec2.extend_from_slice(part2);

        assert_eq!(rec1.len() + rec2.len(), sample.len() + 5);
    }

    #[test]
    fn test_partial_record_falls_back_safely() {
        let partial: Vec<u8> = vec![0x16, 0x03, 0x01, 0x02, 0x00, 0x01, 0x00, 0x01];
        assert!(tls::is_client_hello(&partial));
        let rec_len = u16::from_be_bytes([partial[3], partial[4]]) as usize;
        assert!(partial.len() < 5 + rec_len);
        assert!(fragment_tls_record(&partial).is_none());
    }

    #[test]
    fn test_tls_record_frag_splits_sni_midpoint() {
        let mut sample = Vec::new();
        sample.extend_from_slice(&[0x16, 0x03, 0x01, 0x00, 0x00]);
        sample.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        sample.extend_from_slice(&[0x03, 0x03]);
        sample.extend_from_slice(&[0xaa; 32]);
        sample.push(0x00);
        sample.extend_from_slice(&[0x00, 0x02, 0x13, 0x01]);
        sample.extend_from_slice(&[0x01, 0x00]);

        let host = b"example.com";
        let mut ext_data = Vec::new();
        let list_len = (3 + host.len()) as u16;
        ext_data.extend_from_slice(&list_len.to_be_bytes());
        ext_data.push(0x00);
        ext_data.extend_from_slice(&(host.len() as u16).to_be_bytes());
        ext_data.extend_from_slice(host);

        let mut extensions = Vec::new();
        extensions.extend_from_slice(&[0x00, 0x00]);
        extensions.extend_from_slice(&(ext_data.len() as u16).to_be_bytes());
        extensions.extend_from_slice(&ext_data);

        sample.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
        sample.extend_from_slice(&extensions);

        let hs_len = (sample.len() - 5 - 4) as u32;
        sample[6] = (hs_len >> 16) as u8;
        sample[7] = (hs_len >> 8) as u8;
        sample[8] = hs_len as u8;

        let rec_len = (sample.len() - 5) as u16;
        sample[3] = (rec_len >> 8) as u8;
        sample[4] = rec_len as u8;

        let (rec1, rec2) = fragment_tls_record(&sample).expect("fragmentation should succeed");
        assert_eq!(rec1.len() + rec2.len(), sample.len() + 5);
        assert!(!rec1.windows(host.len()).any(|w| w == host));
        assert!(!rec2.windows(host.len()).any(|w| w == host));
        assert!(rec1.ends_with(b"examp"));
        assert!(rec2[5..].starts_with(b"le.com"));
    }
}
