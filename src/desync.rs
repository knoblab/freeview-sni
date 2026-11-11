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
            delay_ms: 5,
        }
    }
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
            let first = &data[..split_index];
            let second = &data[split_index..];

            upstream.write_all(first).await?;
            upstream.flush().await?;

            if !second.is_empty() {
                tokio::time::sleep(Duration::from_millis(config.delay_ms)).await;
                upstream.write_all(second).await?;
                upstream.flush().await?;
            }
            Ok(())
        }

        DesyncMode::SniSplit => {
            let cut = match tls::find_sni_range(data) {
                Some((offset, len)) if len > 1 => offset + (len / 2),
                _ => std::cmp::min(data.len(), 5), // SNI 파싱 실패 시 5바이트로 폴백
            };

            let first = &data[..cut];
            let second = &data[cut..];

            upstream.write_all(first).await?;
            upstream.flush().await?;

            if !second.is_empty() {
                tokio::time::sleep(Duration::from_millis(config.delay_ms)).await;
                upstream.write_all(second).await?;
                upstream.flush().await?;
            }
            Ok(())
        }

        DesyncMode::TlsRecordFrag => {
            // RFC 8446/5246: Handshake 메시지는 여러 TLS 레코드로 분할될 수 있음
            // data 구조: RecordHeader(5바이트) + Handshake Payload(N바이트)
            if data.len() <= 5 + 4 {
                return upstream.write_all(data).await;
            }

            let record_type = data[0]; // 0x16 (Handshake)
            let proto_ver = [data[1], data[2]]; // 0x03, 0x01 등
            let payload = &data[5..];

            // 분할 지점: SNI 시작 지점 직전 또는 페이로드의 절반
            let cut = match tls::find_sni_range(data) {
                Some((offset, _)) if offset > 5 => offset - 5,
                _ => payload.len() / 2,
            };

            let cut = std::cmp::max(1, std::cmp::min(cut, payload.len() - 1));

            let part1 = &payload[..cut];
            let part2 = &payload[cut..];

            // 1번 레코드
            let mut rec1 = Vec::with_capacity(5 + part1.len());
            rec1.push(record_type);
            rec1.extend_from_slice(&proto_ver);
            rec1.extend_from_slice(&(part1.len() as u16).to_be_bytes());
            rec1.extend_from_slice(part1);

            // 2번 레코드
            let mut rec2 = Vec::with_capacity(5 + part2.len());
            rec2.push(record_type);
            rec2.extend_from_slice(&proto_ver);
            rec2.extend_from_slice(&(part2.len() as u16).to_be_bytes());
            rec2.extend_from_slice(part2);

            upstream.write_all(&rec1).await?;
            upstream.flush().await?;

            tokio::time::sleep(Duration::from_millis(config.delay_ms)).await;

            upstream.write_all(&rec2).await?;
            upstream.flush().await?;

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_record_frag_logic() {
        let sample: Vec<u8> = vec![
            0x16, 0x03, 0x01, 0x00, 0x0a, // Record Header (len: 10)
            0x01, 0x00, 0x00, 0x06,       // Handshake Header
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        ];

        let payload = &sample[5..];
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
}
