# FreeView

**Official Project Page:** [https://knoblab.xyz/#/project/freeview](https://knoblab.xyz/#/project/freeview)

FreeView는 Windows 및 Android 환경에서 심층 패킷 검사(DPI, Deep Packet Inspection) 및 SNI 기반 네트워크 차단을 우회하여 자유로운 웹 서핑과 DNS 보안을 제공하는 초경량 유저스페이스 터널링 프로그램입니다.

컴팩트 팝업 UI(`350 × 205`, 로컬 `IBM Plex Sans KR` 번들)와 트레이/검색 중심의 On-Demand WebView2 생명주기(포커스 해제 시 WebView2 프로세스 즉시 종료)를 결합하여, 직관적인 사용성과 백그라운드 초경량 상주를 동시에 제공합니다.

---

## 주요 기능

### 프록시 및 터널링 엔진
* **TLS 단편화 및 DPI 우회**: 대용량 및 멀티 세그먼트 TLS ClientHello(ML-KEM768 등) 완전 수신, `TlsHeader5`(`2 ms` 지연) 및 `TlsRecordFrag` 기반의 정밀한 SNI 분할을 지원합니다.
* **DNS 보안 및 Happy Eyeballs**: 내장 DNS 캐시(2,048 엔트리, 1초 타임아웃)와 IPv4/IPv6 듀얼스택 교차 경합 연결(100 ms 간격)을 통해 IPv6 미지원 환경에서도 연결 지연을 방지합니다.
* **고대역폭 릴레이 최적화**: 적응형 릴레이 버퍼(`16 KB` ~ `128 KB`)와 멀티스레드 Tokio 런타임 스케줄링(`opt-level = 3`)을 적용하여 대용량 스트리밍 시 시스템콜 및 타이머 부하를 최소화합니다.
* **연결 안정성 보호**: 루프백 자기 호출 차단, 동시 연결 상한 제어, 유휴 터널 자동 정리 및 비정상 HTTP 헤더 방어 로직을 포함합니다.

### 리소스 및 UI 아키텍처
* **On-Demand WebView2 생명주기**: 시스템 트레이 상주 또는 창 포커스 해제 시 WebView2 프로세스를 완전히 종료하여 백그라운드 메모리 점유율 약 5~8 MB, CPU 점유율 0%를 유지합니다.
* **미니멀 팝업 인터페이스**: 단일 토글 버튼(`켜기`/`끄기`)과 핵심 유틸리티(`로그 보기`, `깃허브`, `업데이트 확인`)로 구성된 `350 × 205` 컴팩트 UI를 제공합니다.
* **오프라인 폰트 번들링**: `@fontsource/ibm-plex-sans-kr`(`.woff2`) 서브셋을 내장하여 외부 CDN 요청 없이 일관된 타이포그래피를 렌더링합니다.
* **경계 안티엘리어싱 및 HiDPI 대응**: 메인 앱 및 설치 마법사 모두 `PerMonitorV2` HiDPI와 ClearType 렌더링을 지원하며, 비정수 배율에서의 모서리 클리핑과 계단 현상을 방지합니다.

---

## 버전별 주요 변경 사항

### v3.3.2
* **Android VpnService TUN 엔진 리소스 보호**: UDP 유한 채널(`512`) 및 동시 소켓 상한(`128`) 적용으로 QUIC/DNS 폭주 시 파일 디스크립터(FD) 고갈 방지, TCP 동시 연결 상한(`256`)·유휴 타임아웃(`180초`)·`32 KB` 핸드셰이크 버퍼 조기 해제(`drop`), 미사용 데스크톱 `Controller` 런타임 제거, `tun_fd` RAII 소유권 확보 및 재시작 경합(`TUN_GEN`) 방지.
* **Windows 프록시 및 UI 태스크 정리 강화**: 서비스 중지 시 대기 중인 연결(`handle_client`) 즉시 취소(`JoinSet` + `shutdown_rx`), 포커스 디바운스 비동기 태스크 전환, 미구독 IPC 로그 브로드캐스트 제거, 설치 마법사 GDI 폰트/DIB 핸들 정리 보완.

### v3.3.0 – v3.3.1
* **창 경계 안티엘리어싱 및 HiDPI 선명도 개선 (3.3.1)**: 설치 마법사에 `WS_EX_LAYERED` + `UpdateLayeredWindow`(`PixelFormat32bppPARGB`) 및 32비트 DIB 알파 복원을 적용하고, 메인 앱 외곽 여백(`354 × 209` 내 `350 × 205` 카드) 및 정수 픽셀 정렬을 통해 경계 클리핑을 해결했습니다.
* **트레이/검색 전용 팝업 생명주기 (3.3.0)**: 작업 표시줄 노출(`skipTaskbar`)과 최소화 버튼을 제거하고, 시스템 트레이 또는 프로그램 재실행 시에만 창을 생성합니다.
* **4단계 보더리스 설치 마법사 (3.3.0)**: 메인 UI와 통일된 `350 × 205` 라이트 톤 GDI+ 4단계 설치 마법사 및 잠금 없는 언인스톨러(`uninstall.exe`)를 도입했습니다.

### v3.2.0 – v3.2.2
* **프록시 릴레이 및 지연시간 최적화 (3.2.2)**: 적응형 버퍼링, TLS 분할 기본 지연 단축(`5 ms` -> `2 ms` 자동 마이그레이션), DNS 폴백 응답성 개선.
* **초저전력 백그라운드 아키텍처 (3.2.0)**: On-Demand WebView2 해제, GPU 합성 애니메이션 적용, 1 MB 단위 로그 로테이션 도입.

---

## 설치 및 실행

### 1. 설치 프로그램 (권장)
릴리스 또는 루트 폴더의 **`FreeView Setup 3.3.2.exe`**를 실행하여 설치합니다.
* **기본 설치 경로**: `%LOCALAPPDATA%\Programs\FreeView` (관리자 권한 불필요, 경로 변경 가능)
* **제공 옵션**: 바탕화면/시작 메뉴 바로가기 생성, Windows 시작 시 자동 실행, 제어판 '앱 및 기능' 등록
* 업그레이드 설치 시 기존 실행 중인 프로세스와 시스템 프록시 설정을 안전하게 복원한 후 설치를 진행합니다.

### 2. 포터블 실행
별도 설치 없이 `freeview.exe`와 `WebView2Loader.dll`을 동일한 디렉터리에 위치시킨 후 `freeview.exe`를 실행합니다.

---

## CLI 옵션 및 유틸리티

```powershell
# 헤드리스 모드 (콘솔 단독 구동, WebView2 미사용)
.\freeview.exe --headless

# 비상 설정 복구 모드 (시스템 프록시 강제 초기화)
.\freeview.exe --restore

# 성능 비교 벤치마크 (cloudflare-speed-cli 10회 교차 측정)
.\benchmark_interleaved.bat
```

---

## 라이선스

* 본 프로젝트는 [MIT License](LICENSE)에 따라 배포됩니다. (`Copyright (c) 2026 Knoblab`)
* 서드파티 오픈소스 및 번들 컴포넌트(폰트, WebView2 로더 등)의 라이선스 고지는 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)에서 확인할 수 있습니다.
