# FreeView 3.3.1 (Minimal Concept UI & Native Rust Edition)

Windows 환경에서 심층 패킷 검사(DPI, Deep Packet Inspection) 및 SNI 기반 네트워크 차단을 우회하여 자유로운 웹 서핑과 DNS 보안을 제공하는 초경량 유저스페이스 터널링 프로그램입니다.

**Figma 컨셉 미니멀 UI (`350 × 205`, 로컬 `IBM Plex Sans KR` 번들)**와 **트레이/검색 중심 On-Demand WebView2 생명주기(창 외부 클릭 시 WebView2 즉시 종료)**를 결합해, 사용성 높은 팝업 경험과 백그라운드 초경량 상주를 동시에 제공합니다.

---

## 🌟 주요 특징 및 3.3.1 개선 사항

* **창 경계 안티엘리어싱 및 ClearType / HiDPI 선명도 개선 (3.3.1)**:
  * **설치 마법사 Per-Pixel Alpha 레이어드 렌더링 & HiDPI 지원**: `WS_EX_LAYERED` + `UpdateLayeredWindow`(`PixelFormat32bppPARGB` GDI+ 안티엘리어싱)와 `PerMonitorV2` HiDPI 인식, 네이티브 Windows ClearType(`CLEARTYPE_QUALITY`) + 32비트 DIB 알파 복원을 적용해 모서리 계단 현상과 글씨 흐림을 제거했습니다.
  * **메인 앱 모서리 클리핑 방지 & 폰트 렌더링 개선**: WebView2 뷰포트 외곽 여백(`354 × 209` 내 `350 × 205` 카드)으로 비정수 DPI에서의 경계 클리핑을 방지하고, 정수 픽셀 정렬과 `subpixel-antialiased` 렌더링으로 가독성을 개선했습니다.
* **미니멀 Figma 컨셉 UI & 로컬 폰트 번들링 (3.3.0)**:
  * **컴팩트 팝업 UI (`350 × 205`, `1.2x` 스케일)**: `"켜기"`/`"끄기"` 단일 원버튼 토글과 `"로그 보기"`, `"깃허브"`, `"업데이트 확인"` 액션으로 화면을 단순화했습니다.
  * **오프라인 `IBM Plex Sans KR` 폰트 번들링**: `@fontsource/ibm-plex-sans-kr` `.woff2` 서브셋을 UI 내부에 번들링하여 외부 CDN 의존성 없이 동일한 타이포그래피를 제공합니다.
  * **실시간 GitHub 릴리스 업데이트 확인 & 로그 바로 열기**: `업데이트 확인` 클릭 시 GitHub Releases API(`releases/latest`)를 조회해 최신 버전 여부를 표시하거나 새 릴리스 페이지를 바로 엽니다. `로그 보기`는 `%LOCALAPPDATA%\FreeView\app.log`를 즉시 열어줍니다.
* **트레이/검색 전용 팝업 생명주기 & 최소화 제거 (3.3.0)**:
  * 작업 표시줄 노출(`skipTaskbar`)과 최소화 버튼을 제거하고, 시스템 트레이 클릭 또는 프로그램 재실행(Windows 검색/바로가기)으로만 창을 엽니다.
  * 창 바깥을 클릭해 포커스를 잃으면 창을 숨기는 대신 WebView2 프로세스를 완전히 종료하여 백그라운드 메모리 점유를 즉시 해제합니다.
* **컨셉 일치형 4단계 설치 마법사 (3.3.0 / 3.3.1)**:
  * `FreeView Setup 3.3.1.exe`를 메인 UI와 동일한 `350 × 205` 라이트 톤 보더리스 GDI+ 4단계 마법사(경로 선택 → 자동 실행/바로가기 옵션 → 설치 진행률 바 → 완료/실행)로 전면 재구현했습니다.
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
루트 폴더의 **`FreeView Setup 3.3.1.exe`**를 실행하면 간편하게 설치가 완료됩니다:
* 설치 경로: `%LOCALAPPDATA%\Programs\FreeView` (관리자 권한 UAC 창 없이 설치, 폴더 변경 가능)
* 바탕화면 및 시작 메뉴 바로가기 생성 옵션 제공
* 제어판 "앱 및 기능" 등록
* cmd 창 깜빡임 없이 깔끔한 4단계 보더리스 GUI 진행

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
* 본 프로젝트는 [MIT License](LICENSE)에 따라 배포됩니다. (`Copyright (c) 2026 Knoblab`)
* 서드파티 오픈소스 및 번들 컴포넌트(폰트, WebView2 로더 등)의 라이선스 고지는 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)를 참고해 주세요.
