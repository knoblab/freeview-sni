# FreeView 3.1 (Modern UI & Native Rust Edition)

Windows 환경에서 심층 패킷 검사(DPI, Deep Packet Inspection) 및 SNI 기반 네트워크 차단을 우회하여 자유로운 웹 서핑과 DNS 보안을 제공하는 초경량 유저스페이스 터널링 프로그램입니다.

2.1.1 버전의 모던하고 직관적인 **PDF-DS 디자인 시스템 (다크/라이트 테마, 펄스 애니메이션, 실시간 터미널 로그, 커스텀 타이틀바)**을 100% 동일하게 복원하였으며,
무거운 Electron 프레임워크(150MB+) 대신 **Windows 내장 WebView2 + Tokio 비동기 Rust 엔진 (단일 실행 파일 ~4.3MB, 메모리 30~50MB)**으로 완전히 재구축되었습니다.

---

## 🌟 주요 특징 및 개선 사항

* **2.1.1 UI 100% 복원**:
  * PDF-DS 디자인: 대형 원형 전원 버튼, 가동 시 레드 펄스/스핀 애니메이션.
  * 커스텀 타이틀바: 원클릭 라이트/다크 테마 토글, 최소화, 닫기(트레이 숨김), 윈도우 드래그 이동.
  * 하단 `SYSTEM STATUS` 실시간 로그 터미널 모니터링 및 실시간 HTTPS 우회 트래픽 추적.
* **초경량 고성능 (Electron 완전 탈피)**:
  * Electron(150MB+) 대신 Windows 10/11 기본 내장 WebView2 런타임 활용.
  * 본체 실행 파일 크기: 단 **4.34 MB** (설치 파일: **5.19 MB**).
  * 메모리 점유율을 150~200MB에서 **30~50MB** 수준으로 획기적 절감.
* **PDF-DS 모던 다크 설치 마법사**:
  * DWM Immersive Dark Mode 지원으로 윈도우 타이틀바까지 다크 모드 일체화.
  * 딥 다크 매트 블랙(`#09090b`), 차콜 카드(`#18181b`), 펑셔널 레드(`#ef4444`) 라운드 버튼 및 커스텀 프로그레스 바 적용.
  * `CREATE_NO_WINDOW` 적용으로 cmd/PowerShell 창 깜빡임 100% 차단.
  * `WebView2Loader.dll` 누락 방지 자동 배포.
* **2.1.1의 차단 불가 버그(file:// PAC) 근본 해결**:
  * 2.1.1에서 Chromium/Edge 및 Windows가 지원하지 않는 `file://` PAC 경로를 사용하여 차단이 풀리지 않던 회귀 문제를 해결.
  * 2.0.0의 검증된 시스템 수동 루프백 프록시 연동 및 TLS Record Header 분할(5바이트 분할 + 5ms 지연) 기법 복원.
  * 추가 고급 우회 모드 내장: `SniSplit` (SNI 호스트명 중간 분할), `TlsRecordFrag` (RFC 표준 준수 레코드 단편화).
* **코드 감사 기반 세부 결함 수정**:
  * HTTP 프록싱 지원: 일반 `http://` 접속 시 소켓을 강제 종료하던 문제 수정.
  * 포트 자동 감지: 8080 포트 점유 시 타 프로세스를 강제 종료하지 않고 8081..8099 포트 자동 탐색 바인딩.
  * 네이티브 WinINet 직접 동기화: Rust 내부에서 `InternetSetOptionW`를 직접 호출하여 0.1ms 이내에 즉각 시스템 프록시 반영.
  * 트레이 아이콘 단일화 및 실시간 상태 동기화: ID `main-tray`로 1개만 노출, 툴팁 및 메뉴 실시간 갱신.
  * 언인스톨러 등록 버그 수정: Windows 제어판(설정 - 앱 및 기능)에서 정상적으로 프로그램 제거 가능.
  * 비정상 종료 방어 및 자가 복구: 시스템 셧다운, 프로세스 강제 종료 시에도 네트워크 프록시 자동 복구.

---

## 🚀 다운로드 및 설치

### 1. 원클릭 설치 프로그램 (권장)
루트 폴더의 **`FreeView Setup 3.1.0.exe`** (5.19 MB)를 실행하면 1초 만에 설치가 완료됩니다:
* 설치 경로: `%LOCALAPPDATA%\Programs\FreeView` (관리자 권한 UAC 창 없이 설치)
* 바탕화면 및 시작 메뉴 바로가기 자동 생성
* 제어판 "앱 및 기능" 등록
* cmd 창 깜빡임 없이 깔끔한 PDF-DS 다크 GUI 진행

### 2. 무설치(포터블) 실행
`freeview.exe` (4.34 MB, 동일 폴더의 `WebView2Loader.dll` 포함)를 원하는 폴더로 복사하여 더블클릭하시면 바로 실행됩니다.

---

## 🛠️ CLI 및 복구 명령어

* **헤드리스 모드 (콘솔 구동)**:
  ```powershell
  .\freeview.exe --headless
  ```
* **비상 설정 복구 모드**:
  ```powershell
  .\freeview.exe --restore
  ```

---

## 📄 라이선스
* Copyright © 2024 - 2026 QuarterPi & knoblab. All rights reserved.
