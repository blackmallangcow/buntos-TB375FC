# buntOS 설계안 v0.1

- 상태: 초안 (Draft)
- 작성일: 2026-10-27
- 범위: 전체 아키텍처, 아키텍처별(arm64 / x86-64) 전략, UI, AI 제어 계층, 다국어, 메모리/성능 전략, 테스트 전략, 단계별 로드맵
- 원칙: 이 문서의 수치 중 "실측" 표기가 없는 것은 모두 **가설 또는 초기 예산**이며, Phase 0 측정 결과로 갱신한다 (규칙 4).

---

## 1. 배경과 문제 정의

안드로이드 태블릿은 하드웨어 성능에 비해 활용성이 낮다.

| 제약 | 안드로이드에서의 원인 | buntOS에서의 해결 방향 |
|---|---|---|
| 셸/CMD 제어 불가 | 앱 샌드박스, root 미제공, SELinux 강제 정책 | 표준 Unix 셸과 사용자 소유 root(sudo)를 기본 제공 |
| 개발환경 구성 불가 | bionic libc, 패키지 매니저 부재, Termux는 앱 샌드박스 내부 | glibc 기반 범용 배포판 패키지 생태계 그대로 사용 |
| 테스트 호스팅 불가 | Doze/백그라운드 프로세스 강제 종료 | systemd 서비스 + 명시적 전원 정책(호스팅 모드) |
| 앱 간 제어 불가 | 앱 간 격리, 접근성 API 제한 | OS 레벨 Agent Control Plane이 권한 기반으로 앱 간 제어 제공 |

목표는 "태블릿 폼팩터의 개발용 Unix 워크스테이션"이다.

---

## 2. 핵심 설계 결정 (요약)

| # | 결정 | 이유 |
|---|---|---|
| D1 | 커널은 **Linux**를 사용하고 자체 커널을 작성하지 않는다 | 목표가 "Unix 생태계 흡수"다. 자체 커널은 바이너리/드라이버 호환성을 처음부터 잃는다. buntOS의 차별점은 커널이 아니라 사용자 공간(UI, AI 제어 계층, 통합 경험)이다 |
| D2 | 사용자 공간 베이스는 **Debian (glibc, systemd)** | arm64 / amd64 모두 1급(official) 아키텍처로 패키지 동등성이 가장 높다. musl 기반(Alpine)은 Node/Python 휠 등 사전 빌드 바이너리 호환성 문제가 있어 개발 머신용으로 부적합 |
| D3 | 두 아키텍처는 **동일한 소스, 동일한 패키지 목록, 동일한 이미지 레시피**에서 빌드하고, 차이는 BSP(Board Support Package) 계층에만 허용한다 | 제1 목표(지원 동등성)를 구조적으로 강제 |
| D4 | 디스플레이 서버는 **Wayland**, 컴포지터는 **Rust + Smithay** 기반 자체 구현 (`bunt-comp`) | macOS 수준 UI와 AI 제어 계층(가상 seat, 오프스크린 출력)을 컴포지터 내부에 넣어야 하므로 기존 DE 테마 수정으로는 불가. Rust는 GC 없이 메모리 안전성 확보. Smithay는 GLES/pixman(소프트웨어) 렌더러를 모두 제공하여 GPU 드라이버 불확실성에 대응 가능 |
| D5 | AI 제어는 **비전(스크린샷) 우선이 아니라 의미 구조(semantic) 우선**: CLI/D-Bus 액션 > 접근성 트리 > 스크린샷 순 | Aside의 설계 원칙 채택 (6장) |
| D6 | 파일시스템은 **btrfs + 트랜잭션 단위 스냅샷**, 부트 파티션은 A/B | 개발 머신은 패키지를 자유롭게 설치해야 하므로 불변(immutable) 루트는 장벽이 된다. 대신 롤백을 보장 |
| D7 | 설정값은 코드가 아닌 **선언형 설정(TOML)** 으로 분리 (보드 정의, 해상도/배율, 전원 정책, 권한 정책, 문자열) | 규칙 1 (하드코딩 금지) |

---

## 3. 전체 구조

```
+--------------------------------------------------------------------------+
|  Apps: 터미널, 파일, 설정, 브라우저(Chromium), VS Code, 컨테이너 앱 ...       |
+------------------------------+-------------------------------------------+
|  bunt-shell (Dock, 메뉴바,    |  Agent Control Plane                      |
|  런처, 알림센터, 제어센터,     |   - buntd-agent (세션/권한/감사 로그)       |
|  미션컨트롤, OSK)             |   - a11y 집계기 (AT-SPI -> 압축 스냅샷)     |
|                              |   - bunt CLI / JSON-RPC 소켓 / MCP 어댑터  |
+------------------------------+-------------------------------------------+
|  bunt-comp (Wayland 컴포지터, Smithay)                                     |
|   - 사용자 seat / 에이전트 seat 분리, 오프스크린 출력, 신뢰 레이어(승인 UI)   |
|   - 렌더러: GLES(EGL) | pixman(소프트웨어 폴백)                              |
+--------------------------------------------------------------------------+
|  시스템 서비스: systemd, PipeWire, NetworkManager, fcitx5, Secret Service,  |
|  podman, systemd-oomd, zram, snapper(btrfs)                                |
+--------------------------------------------------------------------------+
|  Debian 사용자 공간 (glibc) -- 양 아키텍처 동일                              |
+--------------------------------------+-----------------------------------+
|  BSP: x86-64 generic                 |  BSP: arm64                       |
|  mainline LTS 커널, UEFI,            |  - qemu-aarch64 (CI 기준 타깃)     |
|  systemd-boot                        |  - lenovo-tb375fc (실기기)         |
+--------------------------------------+-----------------------------------+
```

**BSP 경계 규칙**: BSP가 제공할 수 있는 것은 커널, 커널 모듈, 펌웨어, 부트 이미지 형식, 보드 정의 TOML(패널 크기/배율/입력장치 매핑/전원 프로파일)뿐이다. 그 위의 모든 계층은 아키텍처와 무관하게 동일 바이너리 패키지 집합(동일 버전)이어야 한다.

---

## 4. 아키텍처별 전략

### 4.1 x86-64 (데스크탑)

- 커널: mainline LTS, Debian 커널 설정 기반 + buntOS 조각(config fragment)
- 부트: UEFI + systemd-boot, UKI(Unified Kernel Image), Secure Boot 서명은 후속 단계
- GPU: Mesa (Intel/AMD), NVIDIA는 후속 과제
- 리스크 낮음. UI/에이전트 계층 개발의 1차 플랫폼

### 4.2 arm64 실기기: Lenovo Xiaoxin Pad Pro 12.7 (2025, TB375FC)

공개 자료 기준 하드웨어 (Phase 0에서 실측 확인 필요):

| 항목 | 공개 사양 | 출처 |
|---|---|---|
| SoC | MediaTek Dimensity 8300-Ultra (MT6897 계열로 추정) | devicespecifications.com, alibaba 가이드 |
| CPU | 1x A715 3.35GHz + 3x A715 3.2GHz + 4x A510 2.2GHz | devicespecifications.com |
| GPU | Arm Mali-G615 MC6 (Valhall, CSF 계열) | 동일 |
| RAM | 8 / 12 GB LPDDR5X | 동일 |
| 디스플레이 | 12.7" 2944x1840 IPS | 동일 |
| 무선 | Wi-Fi 6, BT 5.3 (Wi-Fi 전용 모델) | geekwills |
| 기본 OS | Android 14, ZUI 16 | 동일 |

#### 4.2.1 핵심 리스크 (조사 결과)

1. **부트로더 언락이 비공식**: 레노버 공식 언락 채널이 TB375FC를 지원하지 않는다. XDA의 비공식 방법(LK 파티션 교체)이 존재하며 CN ROM ZUI 16 조건이 붙는다. 벽돌(brick) 위험 존재.
2. **MT6897 mainline 커널 지원이 확인되지 않음**: postmarketOS / mainline 커뮤니티에서 해당 SoC 포팅 사례를 찾지 못했다.
3. **GPU**: 오픈소스 Panthor(커널) + Panfrost(Mesa)는 Mali-G610(Valhall v10, CSF)에서 GLES 3.1 적합성을 통과했다. G615는 같은 세대이므로 이론적 가능성은 있으나 **G615에 대한 공식 지원은 확인되지 않음**. 커뮤니티 포크(panfork, 벤더 kbase 위에서 동작)에서 G615 렌더링 동작 보고가 있다.

#### 4.2.2 이중 트랙 전략

| | Track A: 벤더 커널 하이브리드 (단기) | Track B: Mainline (장기) |
|---|---|---|
| 커널 | 기기 출하 GKI 커널 + 벤더 모듈(vendor_dlkm) 재사용 | mainline + MT6897 DTS/드라이버 패치 |
| 디스플레이 | 벤더 커널의 mtk DRM/KMS 노드를 직접 사용 (KMS 노출 여부 실측) | 업스트림 mediatek-drm 확장 |
| GPU | 후보 3개를 실측 비교: (1) pixman 소프트웨어 렌더, (2) Mesa Panfrost-on-kbase 계열, (3) libhybris + 벤더 Mali 블롭 | Panthor + Panfrost (G615 compatible 추가) |
| Wi-Fi/BT | 벤더 모듈 + 펌웨어. 안드로이드 측 로더 데몬 의존성 실측 필요 | 업스트림 mt76 계열 지원 여부 조사 |
| 장점 | 빠른 부팅/사용 가능, 하드웨어 대부분 동작 가능성 | 장기 유지보수, 보안 업데이트, 드라이버 자유 |
| 단점 | 커널이 Android GKI에 고정, 블롭은 재배포 불가(설치 시 기기에서 추출) | 작업량 매우 큼, 성공 보장 없음 |

결정: **Track A로 먼저 실사용 가능 상태에 도달**하고, Track B는 병행 연구로 진행한다. 컴포지터의 렌더러 추상화(D4)가 GPU 경로 교체를 흡수하므로 상위 계층은 트랙 전환 영향을 받지 않는다.

#### 4.2.3 실기기 안전 수칙

- 어떤 플래시 작업 전에도 **전체 파티션 백업** (특히 `nvram`, `nvdata`, `nvcfg`, `protect1/2`, `persist` 계열: Wi-Fi MAC/캘리브레이션 데이터. 소실 시 복구 불가)
- 안드로이드 `recovery` 와 한쪽 슬롯은 보존하여 언제든 ZUI로 복귀 가능하게 유지
- 기기에는 UART가 노출되어 있지 않으므로 디버그 채널은 **USB 가젯(configfs: ACM 시리얼 + NCM 이더넷)** 을 initramfs 최초 단계에서 띄우고, pstore/ramoops로 커널 패닉 로그를 확보한다

### 4.3 arm64 CI 기준 타깃: QEMU `virt` (aarch64, UEFI)

- 실기기 없이 모든 커밋에서 arm64를 검증하기 위한 기준 플랫폼
- x86-64와 동일한 e2e 시나리오를 그대로 실행하여 **동등성을 기계적으로 검증**
- 실기기 회귀는 별도 on-device 스모크 테스트로 보완 (9장)

### 4.4 동등성(제1 목표) 보장 장치

1. **패키지 동등성 리포트**: 빌드마다 아키텍처별 설치 패키지 목록과 버전을 비교. arm64에만 누락된 패키지가 있으면 릴리스 차단
2. **동일 기능 매트릭스**: 셸, sudo, systemd 서비스, podman/docker 호환, 툴체인(gcc/clang/rustup/node/python/go/jdk), SSH 서버, 로컬 호스팅(포트 바인딩, 리버스 프록시)을 e2e로 양쪽에서 실행
3. **x86-64 전용 바이너리 대응** (arm64에서): binfmt_misc + qemu-user-static(컨테이너 멀티아키 이미지), 사용자 바이너리용 FEX-Emu 또는 box64 중 성능 실측 후 택1
4. **호스팅 모드**: 전원 연결 시 `systemd-inhibit` 기반 서스펜드 억제, 화면 꺼짐 상태에서도 네트워크 서비스 유지. 정책은 TOML로 설정

---

## 5. UI/UX (제2 목표)

### 5.1 원칙

- 기준: macOS의 공간 구조(상단 메뉴바, 하단 Dock, 미션 컨트롤, Spotlight형 런처, 제어 센터, 알림 센터)와 상호작용 품질(관성 스크롤, 제스처 연속성, 120Hz급 애니메이션)
- **아키텍처와 무관하게 동일한 UI**. 폼팩터 차이는 "입력 모드"로만 다룬다
  - 포인터 모드(마우스/트랙패드): 데스크탑과 동일
  - 터치 모드: 히트 영역 확대, 화면 키보드 자동 표시, 가장자리 스와이프 제스처
  - 모드 전환은 하드웨어 이벤트(키보드 연결/분리, 포인터 장치 유무)로 자동. UI 레이아웃 자체는 바뀌지 않음
- 배율(scale)은 보드 정의 TOML의 패널 물리 크기와 해상도로 계산하며 분수 배율(wp-fractional-scale-v1)을 지원

### 5.2 구성 요소

| 컴포넌트 | 구현 | 비고 |
|---|---|---|
| `bunt-comp` | Rust, Smithay | 창 관리, 애니메이션, 제스처, 다중 seat, 신뢰 레이어 |
| `bunt-shell` | Rust, 레이어 셸 클라이언트 | 메뉴바, Dock, 런처, 알림, 제어센터 |
| 시스템 앱 UI 툴킷 | GTK4 또는 Qt6 중 Phase 1에서 시제품 비교 후 결정 | 선택 기준: (1) AT-SPI 접근성 노출 품질 (AI 계층의 입력이 됨), (2) 한국어 IME 품질, (3) 메모리 사용량 실측 |
| 화면 키보드 | input-method-v2 클라이언트 | 두벌식/QWERTY, fcitx5와 연동 |

UI 툴킷을 성급히 확정하지 않는 이유: 접근성 트리 품질이 제3 목표의 성패를 좌우하므로, 실측 비교 후 결정한다.

---

## 6. AI 친화 계층: Agent Control Plane (제3 목표)

### 6.1 Aside 설계에서 채택하는 원칙

Aside 공개 자료(aside.com 블로그 "How we built a browser agent...")에서 확인한 설계와 buntOS 대응:

| Aside 원칙/기법 | buntOS 대응 |
|---|---|
| 모델의 학습 데이터 분포를 따른다: 전용 툴셋 대신 Playwright 문법의 JS REPL + bash 두 개만 제공 | 에이전트 API를 **Playwright 로케이터 문법과 동형**으로 설계 (`getByRole`, `getByText`, `click`, `fill`). 별도 학습 없이 모델이 사용 가능. 그리고 OS이므로 bash는 원래 1급 도구 |
| CDP 같은 저수준 프로토콜을 모델에 노출하지 않는다 | Wayland 프로토콜/AT-SPI D-Bus 원시 데이터를 노출하지 않고 고수준 API만 노출 |
| 컨텍스트 윈도 보호: raw DOM 대신 가공된 a11y 트리, ref ID, 중간 노드 제거, 포커스/클릭 가능 여부 추가 | **압축 a11y 스냅샷**: 의미 없는 컨테이너 제거, 상호작용 가능 요소에 ref 부여, 핵심 정보 선두 배치, 크기 상한 |
| 비동기 이벤트를 에이전트에게 신호로 전달 (팝업 열림, 다운로드 시작, 탭 닫힘) | 컴포지터/시스템 이벤트 스트림: 창 생성/종료, 모달 다이얼로그, 알림, 다운로드/프로세스 종료, 권한 요청 |
| 에이전트 탭은 백그라운드에서 열리고 사용자 포커스를 빼앗지 않음 | **에이전트 전용 seat + 오프스크린 출력**: 에이전트가 띄운 창은 사용자 화면을 방해하지 않음. 사용자가 원하면 화면으로 끌어올 수 있음 |
| 뷰포트 1440x900 고정 (CUA 학습 데이터 해상도와 일치) | 오프스크린 출력 해상도는 설정값(기본 1440x900)으로 고정, 사용자 화면 크기와 분리 |
| 비전(CUA)은 DOM 불가 시의 폴백 | 스크린샷+좌표는 최후 폴백. 대상 창 단위 캡처, 좌표는 창 기준으로 매핑 |
| 자격증명은 AI에 노출하지 않고 자동 입력 | Secret Service 연동 **핸들 기반 자동 입력**: 에이전트는 비밀값이 아닌 참조만 받는다 |
| 민감 동작은 사람 승인 | 결제/전송/삭제/권한 상승 등은 **신뢰 레이어 승인 UI** 필요 |
| 모든 접근 기록 | 추가 전용(append-only) 감사 로그 |

### 6.2 제어 경로 우선순위

1. **액션 경로**: CLI, D-Bus 인터페이스, 앱이 선언한 인텐트(앱 매니페스트의 `actions`). 가장 빠르고 결정적
2. **의미 경로**: 압축 a11y 스냅샷 + 로케이터 기반 조작
3. **시각 경로**: 창 단위 스크린샷 + 좌표 입력 (폴백)

에이전트는 상위 경로가 가능할 때 하위 경로를 쓰지 않도록 API가 유도한다 (스냅샷에 해당 요소의 액션 경로 존재 여부를 표기).

### 6.3 구성 요소

- `buntd-agent` (시스템 사용자 서비스, Rust)
  - 에이전트 세션 관리, 권한 평가, 감사 로그, 이벤트 스트림
  - 인터페이스: 유닉스 소켓 JSON-RPC (1차), 이를 감싸는 `bunt agent` CLI, MCP 서버 어댑터 (외부 AI 클라이언트 연결용)
- a11y 집계기: AT-SPI 버스 구독, 앱별 트리 캐시, **변경분(diff) 기반 증분 갱신**
- 컴포지터 확장 (`bunt-comp` 내부, 외부 클라이언트에 비공개)
  - 에이전트 seat: 별도 포커스/포인터/키보드. 사용자 입력과 충돌하지 않음
  - 오프스크린 출력: 헤드리스 출력에 에이전트 창 배치
  - 창 단위 캡처: dmabuf 기반, 요청 시에만 인코딩
  - 신뢰 레이어: 승인 다이얼로그는 컴포지터가 직접 그리며, **에이전트 seat의 입력은 신뢰 레이어에 전달되지 않음** (에이전트가 스스로 승인 버튼을 누르는 것을 구조적으로 차단)

### 6.4 API 형태 (초안)

```js
// Playwright 문법과 동형. 대상이 브라우저 페이지가 아니라 데스크탑 앱.
const app = await desktop.launch('org.buntos.Files');
await app.getByRole('button', { name: '새 폴더' }).click();
await app.getByRole('textbox', { name: '이름' }).fill('project');
await app.keyboard.press('Enter');

const snap = await app.snapshot();          // 압축 a11y 스냅샷 (ref 포함)
await app.ref('e12').click();               // 스냅샷 ref로 조작
const shot = await app.screenshot();        // 폴백용 창 단위 캡처
```

스냅샷 노드는 **지역화된 이름과 안정적 식별자(accessible id)를 함께** 제공한다. 언어 설정(한/영)이 바뀌어도 자동화 스크립트가 깨지지 않게 하기 위함이다.

### 6.5 권한 모델

- 권한 단위: `(에이전트, 대상 앱, 동작 등급)`
- 동작 등급: `read`(스냅샷/캡처) / `interact`(입력) / `sensitive`(전송, 삭제, 결제, 비밀값 사용) / `system`(sudo, 패키지 설치, 서비스 제어)
- `sensitive`, `system`은 기본적으로 매번 신뢰 레이어 승인. 정책은 TOML로 사용자가 조정
- 에이전트 프로세스는 별도 cgroup slice와 사용자 네임스페이스에서 실행

---

## 7. 다국어 (제4 목표)

- 기본 로케일: `ko_KR.UTF-8`, `en_US.UTF-8` 를 기본 이미지에 포함. 최초 설정에서 선택
- 문자열: 소스에 사용자 노출 문자열 하드코딩 금지. 자체 컴포넌트(Rust)는 **Fluent**(복수형/조사 처리에 유리), 기존 생태계 패키지는 gettext 그대로
- 한국어 조사 처리(을/를, 이/가)는 Fluent 선택자로 처리
- 입력기: fcitx5 + fcitx5-hangul 기본. 컴포지터는 text-input-v3 / input-method-v2 구현 필수 (Wayland에서 한글 입력 품질의 핵심)
- 폰트: 한글 UI 폰트(Pretendard 또는 Noto Sans KR 중 렌더링/메모리 실측 후 결정), 고정폭은 한글 폭 정합이 맞는 폰트 선택
- CI에서 미번역 키 검출 (빌드 경고 → 릴리스 시 오류)

---

## 8. 성능과 메모리 관리 (규칙 5)

### 8.1 시스템 수준

| 영역 | 전략 |
|---|---|
| 메모리 압축 | zram(zstd) 스왑. 크기는 RAM 대비 비율로 설정(TOML) |
| OOM | systemd-oomd + PSI(메모리 압력) 기반. 커널 OOM killer 이전에 우선순위 낮은 slice부터 정리 |
| cgroup v2 slice | `interactive.slice`(포그라운드 앱, 최우선) / `agent.slice`(MemoryHigh 상한) / `hosting.slice`(사용자 서버) / `background.slice` |
| CPU (big.LITTLE) | EAS 스케줄러 + uclamp: 포그라운드 앱과 컴포지터 렌더 스레드는 `cpu.uclamp.min` 상향, 백그라운드는 하향 |
| 저장장치 | btrfs 압축(zstd), noatime, 스냅샷 보존 개수 상한 |
| 부팅 | systemd-analyze로 critical chain 측정, 지연 시작 가능한 서비스는 socket/path 활성화 |

### 8.2 컴포지터/셸

- 데미지 트래킹: 변경 영역만 재합성
- 다이렉트 스캔아웃: 전체화면/단일 창은 합성 생략
- dmabuf 제로카피 경로 우선
- 프레임 루프에서 힙 할당 금지: 버퍼/정점/명령 리스트는 풀(pool)과 프레임 아레나로 재사용
- 오프스크린 출력은 에이전트 세션이 있을 때만 생성하고, 관찰자(스냅샷/캡처 요청)가 없으면 렌더링 빈도를 낮춤
- 애니메이션은 디스플레이 vblank 동기화, 주사율은 KMS 모드 목록에서 읽음(하드코딩 금지)

### 8.3 Agent Control Plane

- a11y 트리: 앱별 캐시 + 변경 이벤트 기반 증분 갱신. 전체 재수집은 캐시 무효화 시에만
- 스냅샷 직렬화: 스트리밍 직렬화, 크기 상한 초과 시 하위 트리를 요약 노드로 대체
- 캡처: 요청 시에만 GPU에서 다운스케일 후 인코딩. 상시 캡처 금지
- 감사 로그: 메모리 버퍼 + 주기적 flush, 크기 기반 로테이션

### 8.4 초기 예산 (가설, Phase 1에서 실측 후 확정)

| 항목 | 초기 예산 |
|---|---|
| 로그인 직후 유휴 시스템 메모리 (셸+컴포지터+서비스) | 측정 기준선 확보 후 결정. 비교 대상: 동일 기기 ZUI 16 유휴 사용량, Debian+GNOME 유휴 사용량 |
| 컴포지터 프레임 시간 | 디스플레이 주기 이내 (p99) |
| 에이전트 스냅샷 1회 지연 | 캐시 적중 시 수십 ms 단위 목표, 실측 후 확정 |
| 콜드 부팅 → 로그인 화면 | 기준선 측정 후 회귀 감시 |

모든 예산은 CI 벤치마크로 회귀를 감시한다 (값 자체를 코드에 박지 않고 기준선 파일로 관리).

---

## 9. 테스트 전략 (규칙 2, 3)

### 9.1 원칙

- 테스트를 위한 테스트 금지: 커버리지 수치를 목표로 삼지 않는다. 각 테스트는 "깨지면 사용자 기능이 깨지는" 동작을 검증해야 한다
- 함수/아키텍처 변경: 해당 단위 테스트 필수 (Rust `cargo test`, 셸 스크립트는 bats)
- main 병합 전: e2e 필수

### 9.2 e2e 구성

- 대상: x86-64 QEMU, aarch64 QEMU(`virt`, UEFI) **두 아키텍처 모두 동일 시나리오**
- 구동 방식: 빌드된 이미지를 QEMU로 부팅 → 시나리오 실행 → 결과 수집
- **e2e 드라이버로 buntOS 자체의 Agent Control Plane을 사용** (도그푸딩). 별도의 테스트 전용 자동화 경로를 만들지 않으므로 테스트 코드가 실제 제품 경로를 검증한다
- 시나리오 예 (워크플로우 단위):
  1. 부팅 → 로그인 → 셸 표시
  2. 터미널 실행 → `apt install` → 스냅샷 생성 확인 → 롤백
  3. 컨테이너로 웹 서버 실행 → 로컬 포트 응답 확인 (호스팅 시나리오)
  4. 한국어 입력: 텍스트 필드에 한글 조합 입력 후 값 검증
  5. 에이전트가 앱 A의 값을 읽어 앱 B에 입력 (앱 간 제어)
  6. 에이전트가 `sensitive` 동작 시도 → 승인 UI 표시 → 에이전트 seat로 승인 불가 확인

### 9.3 실기기 (TB375FC)

- 릴리스 후보마다 수동 트리거 스모크 테스트: 부팅, 디스플레이, 터치, Wi-Fi, 배터리 잔량, 서스펜드/복귀
- USB 가젯 NCM으로 호스트에서 SSH 접속하여 동일 e2e 시나리오 일부 실행

---

## 10. 저장소 구조 (제안)

```
buntOS/
  docs/design/            설계 문서, ADR
  image/                  mkosi 이미지 레시피 (공통 + 아키텍처별 오버레이)
  bsp/
    x86_64-generic/       커널 설정 조각, 부트 설정, board.toml
    qemu-aarch64/
    lenovo-tb375fc/       부트 이미지 패키징, 모듈/펌웨어 추출 스크립트, board.toml
  kernel/                 공통 설정 조각, 패치
  comp/                   bunt-comp (Rust)
  shell/                  bunt-shell (Rust)
  agent/                  buntd-agent, bunt CLI, MCP 어댑터 (Rust)
  i18n/                   Fluent 리소스 (ko, en)
  policy/                 기본 권한/전원/메모리 정책 TOML
  tests/e2e/              QEMU e2e 하네스와 시나리오
  ci/                     CI 파이프라인 정의
```

- 이미지 빌드 도구: mkosi (Debian 지원, 아키텍처별 빌드, UKI 생성)
- 패키지 형식: 자체 컴포넌트도 .deb로 패키징하여 일반 Debian 패키지와 같은 경로로 배포/업데이트

---

## 11. 로드맵과 종료 조건

| 단계 | 내용 | 종료 조건 (측정 가능해야 함) |
|---|---|---|
| **Phase 0** 실측/기반 | 실기기 정보 수집 및 전체 백업, 리포 골격, mkosi로 x86-64/aarch64 QEMU 부팅 이미지, CI | 실측 보고서 작성 완료, 양 아키텍처 QEMU 이미지가 같은 레시피로 부팅하여 셸 도달 (e2e 1번) |
| **Phase 1** 컴포지터 최소 기능 | bunt-comp: 창 표시, 입력, 데미지 트래킹, pixman/GLES 렌더러, text-input-v3 | 양 아키텍처 QEMU에서 터미널 창 실행 및 한글 입력 e2e 통과, 메모리/프레임 기준선 기록 |
| **Phase 2** 실기기 부팅 (Track A) | TB375FC 언락, 벤더 커널 + Debian 루트, USB 가젯 디버그 | 기기에서 로그인 셸 도달, 디스플레이 출력, 터치 입력, Wi-Fi 연결 |
| **Phase 3** Agent Control Plane v0 | buntd-agent, a11y 집계, 에이전트 seat, CLI/MCP | e2e 5, 6번 통과 |
| **Phase 4** UI 셸 | 메뉴바/Dock/런처/제어센터/OSK, 입력 모드 전환 | 양 아키텍처 동일 UI, 터치/포인터 모드 자동 전환 확인 |
| **Phase 5** 개발/호스팅 완성도 | 툴체인 매트릭스, 컨테이너, 호스팅 모드, x86 바이너리 호환 | 동등성 매트릭스 전 항목 양쪽 통과 |
| **장기** Track B | MT6897 mainline 포팅 연구 | 별도 계획 |

---

## 12. Phase 0 실측 항목 (TB375FC)

언락 전 ADB(개발자 옵션)만으로 수집 가능한 항목부터 진행한다. 결과는 `docs/hw/lenovo-tb375fc.md`에 기록한다.

| 항목 | 명령 (adb shell) | 판단 대상 |
|---|---|---|
| 커널 버전/GKI 브랜치 | `uname -a`, `cat /proc/version` | Track A 커널 기준 |
| SoC 식별 | `getprop ro.soc.model`, `getprop ro.board.platform`, `cat /proc/cpuinfo` | MT6897 여부 확정 |
| 부트 구성 | `getprop ro.boot.slot_suffix`, `getprop ro.boot.dynamic_partitions`, `ls -l /dev/block/by-name/` (권한 허용 범위) | A/B, super 파티션, vendor_boot 유무 |
| 부트 이미지 헤더 | 펌웨어 boot.img 추출 후 `unpack_bootimg` (호스트) | header 버전, initramfs 구조 |
| 디스플레이 | `dumpsys display`, `dumpsys SurfaceFlinger` | 해상도, 주사율 목록, 패널 정보 |
| DRM/KMS 노출 | `ls /dev/dri` (root 필요 시 언락 후) | Track A 컴포지터 KMS 직접 사용 가능성 |
| GPU | `dumpsys SurfaceFlinger | grep -i GLES`, `ls /dev/mali*` | 드라이버 경로 (kbase) |
| 입력 장치 | `getevent -lp` | 터치/펜/키보드 커버 컨트롤러 |
| Wi-Fi/BT | `lsmod` 또는 `cat /proc/modules`, `getprop | grep -i wifi` | 칩/모듈/펌웨어 로더 의존성 |
| 메모리 기준선 | `cat /proc/meminfo`, `dumpsys meminfo` | 8.4 예산 비교 기준 |
| 전원 | `dumpsys battery`, `/sys/class/power_supply/*` | 충전/배터리 드라이버 |

---

## 13. 미결정 사항 (ADR로 결정 예정)

| ID | 질문 | 결정 시점 | 결정 방법 |
|---|---|---|---|
| ADR-001 | 시스템 앱 UI 툴킷 GTK4 vs Qt6 | Phase 1 | 동일 시제품 앱으로 a11y 트리 품질, 한글 IME, 메모리 실측 비교 |
| ADR-002 | arm64 실기기 GPU 경로 | Phase 2 | 4.2.2의 3개 후보 실측 (프레임 시간, 전력, 안정성) |
| ADR-003 | x86 바이너리 호환 FEX-Emu vs box64 | Phase 5 | 대표 워크로드 벤치마크 |
| ADR-004 | Debian stable vs testing 기준 | Phase 0 | 툴체인 최신성 요구 vs 안정성. stable + backports 우선 검토 |
| ADR-005 | UI 폰트 | Phase 4 | 렌더링 품질/메모리 실측 |
| ADR-006 | 라이선스 (자체 컴포넌트) | Phase 0 | Smithay(MIT), Debian 패키지 라이선스와의 정합성 검토 |

---

## 14. 참고 자료

- Aside 설계: https://aside.com/blog/how-we-built-the-sota-browser-agent-that-outperforms-fable
- TB375FC 사양: https://www.devicespecifications.com/en/model/53556179
- TB375FC 비공식 언락: https://xdaforums.com/t/unofficial-unlocking-lenovo-xiaoxin-pad-pro-2025-tb375fc.4717695/
- Panthor G610 적합성: https://www.collabora.com/news-and-blog/news-and-events/taming-the-panthor-opengl-es-31-conformance-achived-mali-g610.html
- Mesa Panfrost: https://docs.mesa3d.org/drivers/panfrost.html
- Android GKI android14-6.1: https://source.android.com/docs/core/architecture/kernel/android-common
- Dimensity 8300: https://www.mediatek.com/products/smartphones/mediatek-dimensity-8300
