# FreeView 3.2.2 (Modern UI & Native Rust Edition)

Windows 환경에서 심층 패킷 검사(DPI, Deep Packet Inspection) 및 SNI 기반 네트워크 차단을 우회하여 자유로운 웹 서핑과 DNS 보안을 제공하는 초경량 유저스페이스 터널링 프로그램입니다.

2.1.1 버전의 모던하고 직관적인 **PDF-DS 디자인 시스템 (다크/라이트 테마, 펄스 애니메이션, 실시간 터미널 로그, 커스텀 타이틀바)**을 유지하면서,
**On-Demand WebView2 아키텍처와 Tokio 비동기 Rust 엔진**으로 백그라운드 대기 시 **메모리 ~5~8 MB / CPU 0%** 수준의 극한 경량화를 달성했습니다.

---

## 🌟 주요 특징 및 3.2.2 개선 사항

* **고대역폭 프록시 릴레이 & 지연시간 최적화 (3.2.2)**:
  * **적응형 릴레이 버퍼 (`16 KB -> 128 KB`) & 타이머 스래싱 제거**: 유휴/소형 연결의 메모리 효율을 유지하면서 대용량 스트리밍 시 시스템콜과 Tokio 타이머 휠 부하를 대폭 절감했습니다.
  * **멀티스레드 Tokio 런타임 스케줄링 & `opt-level = 3`**: 코어 수에 맞춘 워커 스레드 확장(4~8개) 및 속도 최적화 컴파일 프로파일을 적용했습니다.
  * **TLS 분할 지연(`delay_ms`) 최적화 & 안전 마이그레이션**: 실측 검증된 `TlsHeader5` + `2 ms` 기본값 적용(기존 `5 ms` 기본 설정 자동 마이그레이션 포함)으로 초기 TLS 핸드셰이크 지연을 단축하고 `TlsRecordFrag` SNI 중간 분할 정확도를 개선했습니다.
  * **DNS 폴백 및 Happy Eyeballs 응답성 강화**: DNS 캐시 확대(2,048), 타임아웃 단축(1초), 듀얼스택 경합 간격 단축(100 ms)을 적용했습니다.
* **초저전력 · 초경량 리소스 최적화 (3.2.0)**:
  * **On-Demand WebView2 생명주기**: `--startup` 시작 또는 창 닫기(`X`)로 트레이에 상주시킬 때 WebView2 프로세스를 완전히 해제하여 **백그라운드 메모리 ~5~8 MB, CPU 0%**를 유지합니다.
  * **최소화 절전 및 GPU 합성 애니메이션**: 창이 열려 있을 때는 GPU 합성 전용 `opacity`/`transform` 펄스를 사용하여 CPU 리페인트를 제거하고, 최소화(`-`) 시 애니메이션 정지 및 WebView2 `MemoryUsageTargetLevel(LOW)`를 적용합니다.
  * **오프라인 친화적 시스템 폰트 & 로깅 최적화**: 외부 웹폰트 CDN 요청을 제거하고 핵심 상태/오류 로그만 기본 기록(1 MB 로그 로테이션)하며, 필요 시 트레이 메뉴에서 `상세 연결 로그 기록`을 켤 수 있습니다.
* **프록시 엔진 안정성 및 호환성 강화**:
  * 대용량/멀티 세그먼트 TLS ClientHello(ML-KEM768 등) 완전 수신 및 안전한 `TlsRecordFrag` 단편화.
  * Happy Eyeballs(IPv4/IPv6 교차 경합 연결) 적용으로 IPv6 미지원 네트워크에서의 연결 지연 제거.
  * 루프백 자기 호출 차단, 동시 연결 제한 및 유휴 터널 정리, 헤더 없는 HTTP 요청 슬라이스 패닉 수정.
* **PDF-DS 모던 다크 설치 마법사 & 언인스톨러 개선**:
  * 업그레이드 설치 시 기존 프록시 자동 복원 후 안전 종료.
  * 제거(`uninstall.exe`) 시 파일 잠금 없이 설치 폴더 및 데이터 완전 정리.

---

## 🚀 다운로드 및 설치

### 1. 원클릭 설치 프로그램 (권장)
루트 폴더의 **`FreeView Setup 3.2.2.exe`**를 실행하면 1초 만에 설치가 완료됩니다:
* 설치 경로: `%LOCALAPPDATA%\Programs\FreeView` (관리자 권한 UAC 창 없이 설치)
* 바탕화면 및 시작 메뉴 바로가기 자동 생성
* 제어판 "앱 및 기능" 등록
* cmd 창 깜빡임 없이 깔끔한 PDF-DS 다크 GUI 진행

### 2. 무설치(포터블) 실행
`freeview.exe` (동일 폴더의 `WebView2Loader.dll` 포함)를 원하는 폴더로 복사하여 더블클릭하시면 바로 실행됩니다.

---

## 🛠️ CLI 및 복구 명령어

* **헤드리스 모드 (콘솔 구동, WebView2 미사용)**:
  ```powershell
  .\freeview.exe --headless
  ```
* **비상 설정 복구 모드**:
  ```powershell
  .\freeview.exe --restore
  ```
* **성능 비교 벤치마크 (`cloudflare-speed-cli` 10회 교차 측정)**:
  ```powershell
  .\benchmark_interleaved.bat
  ```

---

## 📄 라이선스
* Copyright © 2024 - 2026 QuarterPi & knoblab. All rights reserved.
