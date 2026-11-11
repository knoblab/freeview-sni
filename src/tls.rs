//! TLS ClientHello 및 SNI 파싱 모듈

/// 주어진 바이트 버퍼가 TLS ClientHello 레코드인지 확인합니다.
///
/// TLS Record 구조:
/// - byte 0: ContentType (0x16 = Handshake)
/// - byte 1-2: ProtocolVersion (0x03, 0x01 = TLS 1.0 등)
/// - byte 3-4: Length (Handshake Message 길이)
/// - byte 5: HandshakeType (0x01 = ClientHello)
pub fn is_client_hello(buf: &[u8]) -> bool {
    buf.len() >= 6 && buf[0] == 0x16 && buf[1] == 0x03 && buf[5] == 0x01
}

/// ClientHello 내에서 Server Name Indication (SNI) 호스트명 바이트의
/// 시작 오프셋과 길이 `(offset, length)`를 찾습니다.
pub fn find_sni_range(buf: &[u8]) -> Option<(usize, usize)> {
    if !is_client_hello(buf) {
        return None;
    }

    // TLS Record Header: 5 bytes
    // Handshake Header: 4 bytes (Type: 1, Length: 3)
    // ClientVersion: 2 bytes
    // Random: 32 bytes
    let mut cursor = 5 + 4 + 2 + 32;
    if cursor >= buf.len() {
        return None;
    }

    // Session ID length: 1 byte
    let session_id_len = *buf.get(cursor)? as usize;
    cursor += 1 + session_id_len;
    if cursor + 2 > buf.len() {
        return None;
    }

    // Cipher Suites length: 2 bytes
    let cipher_suites_len = u16::from_be_bytes([buf[cursor], buf[cursor + 1]]) as usize;
    cursor += 2 + cipher_suites_len;
    if cursor >= buf.len() {
        return None;
    }

    // Compression Methods length: 1 byte
    let comp_methods_len = *buf.get(cursor)? as usize;
    cursor += 1 + comp_methods_len;
    if cursor + 2 > buf.len() {
        return None;
    }

    // Extensions length: 2 bytes
    let extensions_len = u16::from_be_bytes([buf[cursor], buf[cursor + 1]]) as usize;
    cursor += 2;
    let extensions_end = cursor + extensions_len;
    if extensions_end > buf.len() {
        return None;
    }

    // 각 확장 순회
    while cursor + 4 <= extensions_end {
        let ext_type = u16::from_be_bytes([buf[cursor], buf[cursor + 1]]);
        let ext_len = u16::from_be_bytes([buf[cursor + 2], buf[cursor + 3]]) as usize;
        cursor += 4;

        if cursor + ext_len > extensions_end {
            return None;
        }

        // ext_type 0 = server_name (SNI)
        if ext_type == 0 {
            // ServerNameList length: 2 bytes
            if ext_len < 2 {
                return None;
            }
            let list_len = u16::from_be_bytes([buf[cursor], buf[cursor + 1]]) as usize;
            let mut list_cursor = cursor + 2;
            let list_end = list_cursor + list_len;

            while list_cursor + 3 <= list_end && list_cursor + 3 <= extensions_end {
                let name_type = buf[list_cursor];
                let name_len = u16::from_be_bytes([buf[list_cursor + 1], buf[list_cursor + 2]]) as usize;
                list_cursor += 3;

                // 0 = host_name
                if name_type == 0 {
                    if list_cursor + name_len <= buf.len() {
                        return Some((list_cursor, name_len));
                    }
                    return None;
                }
                list_cursor += name_len;
            }
        }

        cursor += ext_len;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_client_hello_sni() {
        // 규격에 완벽히 부합하는 ClientHello 바이트 패킷 생성
        let mut sample = Vec::new();
        // 1. Record Header
        sample.extend_from_slice(&[0x16, 0x03, 0x01, 0x00, 0x00]); // 길이 임시 0
        // 2. Handshake Header (ClientHello)
        sample.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // 길이 임시 0
        // 3. Client Version (TLS 1.2)
        sample.extend_from_slice(&[0x03, 0x03]);
        // 4. Random 32 bytes
        sample.extend_from_slice(&[0xaa; 32]);
        // 5. Session ID (length 0)
        sample.push(0x00);
        // 6. Cipher Suites (length 2, 0x1301)
        sample.extend_from_slice(&[0x00, 0x02, 0x13, 0x01]);
        // 7. Compression (length 1, 0x00)
        sample.extend_from_slice(&[0x01, 0x00]);

        // 8. Extensions
        let host = b"example.com";
        let mut ext_data = Vec::new();
        // ServerNameList length = 1 (type) + 2 (len) + host.len() = 3 + 11 = 14
        let list_len = (3 + host.len()) as u16;
        ext_data.extend_from_slice(&list_len.to_be_bytes());
        ext_data.push(0x00); // host_name
        ext_data.extend_from_slice(&(host.len() as u16).to_be_bytes());
        ext_data.extend_from_slice(host);

        let mut extensions = Vec::new();
        extensions.extend_from_slice(&[0x00, 0x00]); // ext_type: server_name (0)
        extensions.extend_from_slice(&(ext_data.len() as u16).to_be_bytes());
        extensions.extend_from_slice(&ext_data);

        sample.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
        sample.extend_from_slice(&extensions);

        // 길이 필드 업데이트
        let hs_len = (sample.len() - 5 - 4) as u32;
        sample[6] = (hs_len >> 16) as u8;
        sample[7] = (hs_len >> 8) as u8;
        sample[8] = hs_len as u8;

        let rec_len = (sample.len() - 5) as u16;
        sample[3] = (rec_len >> 8) as u8;
        sample[4] = rec_len as u8;

        assert!(is_client_hello(&sample));
        let (offset, len) = find_sni_range(&sample).expect("SNI should be found");
        assert_eq!(&sample[offset..offset + len], b"example.com");
    }

    #[test]
    fn test_corrupted_packets() {
        assert!(!is_client_hello(&[]));
        assert!(!is_client_hello(b"GET / HTTP/1.1\r\n\r\n"));
        assert_eq!(find_sni_range(&[0x16, 0x03, 0x01, 0x00]), None);
    }
}
