# buntOS 설계안 v0.3.9

- 상태: 초안 (Draft)
- 작성일: 2026-10-27
- 개정 이력
  - v0.3.9 (2026-10-28): 11장에 메뉴 막대·Dock 자동 가리기(최대화한 창이 막대 자리까지 채움), Dock 크기·투명도, 시스템 설정의 데스크탑 및 Dock·기본 앱 페이지를 반영했다
  - v0.3.8 (2026-10-28): 11장에 화면 배율(소수 배율, viewporter, 화면이 바뀔 때 창 맞추기, macOS형 크기 고르기)을 반영하고 다음 작업 3번에서 분수 배율을 뺐다
  - v0.3.7 (2026-10-28): 5.2절과 11장에 메뉴 막대 항목(제어 센터, 배터리·전원 모드, Wi-Fi, 사운드, 입력 소스, 앱 항목), 가운데 뜨는 모든 앱 패널, 배경화면, 한글 입력기(fcitx5, input-method-v2), 컴포지터 제어 API, 시스템 설정 재디자인, 필수 드라이버·펌웨어를 반영하고 다음 작업을 고쳤다. UI 규칙은 [디자인 스펙](DESIGN-SPEC.md)으로 옮겼다
  - v0.3.6 (2026-10-28): 5.2절과 11장에 macOS형 메뉴바(앞에 있는 앱의 메뉴, 네트워크 아이콘, 알림 센터), 모든 앱 목록, 창 목록 프로토콜의 셸 전용 공개, NetworkManager를 반영하고 다음 작업을 고쳤다
  - v0.3.5 (2026-10-28): 11장 진행 상황에 저장소 반영 상태와 지금 CI가 하는 일(REUSE 검사)을 적었다
  - v0.3.4 (2026-10-28): 11장에 기본 앱과 시스템 설정 v0, 다음 작업 순서(x86-64 확인은 필수 드라이버 뒤)를 반영했다. 5.2절에 bunt-settings, 10장에 `settings/`와 공용 번역 크레이트를 추가했다
  - v0.3.3 (2026-10-27): 11장에 진행 상황(1차 빌드 성공, 그래픽 세션 v0)과 다음 작업을 기록했다. 5.2절과 13장에 bunt-shell v0가 ADR-001의 GTK4 시제품을 겸한다고 적었다
  - v0.3.2 (2026-10-27): 공개 시점(1차 빌드 성공 후)과 검증된 개발 환경(Lima VM, 중첩 KVM)을 4.1, 9.4, 10장에 반영했다
  - v0.3.1 (2026-10-27): 9.4에 저장소 위치(`blackmallangcow/buntOS`)를 명시했다
  - v0.3 (2026-10-27): 오픈소스 공개를 전제로 ADR-004·006·009·010·011·012를 확정해 반영했다(D2, D10, 2.1절, 4.1, 4.2, 9.2, 9.4, 10장, 11장, 12장, 13장)
  - v0.2 (2026-10-27): 실기기 전략을 mainline 단일로 바꾸고 벤더 커널 하이브리드(구 Track A)를 폐기했다. 개방성 정의(D8, 2.1절), 개발환경 비차단 원칙(D9), EL2/KVM 실측(12.1절)을 추가했다
- 결정 근거: [DECISION-BRIEF.md](DECISION-BRIEF.md)
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

목표는 "태블릿 폼팩터의 개발용 Unix 워크스테이션"이다. 이를 위해 다음 두 가지는 타협하지 않는다.

1. **완전히 열린 스택** (D8, 정의는 2.1절): 위 제약은 모두 사용자가 통제할 수 없는 코드(벤더 프레임워크, 벤더 커널, 폐쇄 드라이버)에서 나온다. 사용자 공간만 바꾸고 그 아래를 벤더 코드로 남기면 같은 문제가 다른 층에서 다시 생긴다
2. **OS는 개발환경 구성을 막지 않는다** (D9): root, 패키지 설치, 컨테이너, 가상화, 커널 모듈 빌드, 커널 업그레이드를 OS 구성이 막아서는 안 된다

---

## 2. 핵심 설계 결정 (요약)

| # | 결정 | 이유 |
|---|---|---|
| D1 | 커널은 **Linux**를 사용하고 자체 커널을 작성하지 않는다 | 목표가 "Unix 생태계 흡수"다. 자체 커널은 바이너리/드라이버 호환성을 처음부터 잃는다. buntOS의 차별점은 커널이 아니라 사용자 공간(UI, AI 제어 계층, 통합 경험)이다 |
| D2 | 사용자 공간 베이스는 **Debian (glibc, systemd)**. 기준은 **forky(testing) 스냅샷 고정**이며 forky 정식 릴리스 시 stable로 전환한다(ADR-004) | arm64 / amd64 모두 1급(official) 아키텍처로 패키지 동등성이 가장 높다. musl 기반(Alpine)은 Node/Python 휠 등 사전 빌드 바이너리 호환성 문제가 있어 개발 머신용으로 부적합. forky는 실기기 GPU에 필요한 커널 6.18 이상과 Mesa 25.1 이상을 기본 저장소에 갖고 있고, 실측을 출시할 플랫폼에서 한 번만 하면 된다 |
| D3 | 두 아키텍처는 **동일한 소스, 동일한 패키지 목록, 동일한 이미지 레시피**에서 빌드하고, 차이는 BSP(Board Support Package) 계층에만 허용한다 | 제1 목표(지원 동등성)를 구조적으로 강제 |
| D4 | 디스플레이 서버는 **Wayland**, 컴포지터는 **Rust + Smithay** 기반 자체 구현 (`bunt-comp`) | macOS 수준 UI와 AI 제어 계층(가상 seat, 오프스크린 출력)을 컴포지터 내부에 넣어야 하므로 기존 DE 테마 수정으로는 불가. Rust는 GC 없이 메모리 안전성 확보. Smithay는 GLES/pixman(소프트웨어) 렌더러를 모두 제공하여 GPU 드라이버 불확실성에 대응 가능 |
| D5 | AI 제어는 **비전(스크린샷) 우선이 아니라 의미 구조(semantic) 우선**: CLI/D-Bus 액션 > 접근성 트리 > 스크린샷 순 | Aside의 설계 원칙 채택 (6장) |
| D6 | 파일시스템은 **btrfs + 트랜잭션 단위 스냅샷**, 부트 파티션은 A/B | 개발 머신은 패키지를 자유롭게 설치해야 하므로 불변(immutable) 루트는 장벽이 된다. 대신 롤백을 보장 |
| D7 | 설정값은 코드가 아닌 **선언형 설정(TOML)** 으로 분리 (보드 정의, 해상도/배율, 전원 정책, 권한 정책, 문자열) | 규칙 1 (하드코딩 금지) |
| D8 | **개방성**: 애플리케이션 프로세서(AP, 메인 CPU)에서 실행되는 커널·드라이버·사용자 공간 코드는 전부 오픈소스이며 업스트림을 목표로 한다. 벤더 커널, 폐쇄 커널 모듈, 폐쇄 사용자 공간 드라이버, bionic 호환 계층(libhybris)은 쓰지 않는다. 펌웨어만 예외다(2.1절) | 1장의 원인(bionic, 벤더 통제)을 구조적으로 제거한다. 커널을 업스트림과 함께 올릴 수 있어야 개발환경이 시간이 지나도 막히지 않는다(D9) |
| D9 | **개발환경 비차단**: root, 패키지 설치, 컨테이너(rootful/rootless), 가상화(KVM), 커널 모듈 빌드, 커널 업그레이드를 OS 구성이 막지 않는다. 하드웨어나 펌웨어가 막는 경우는 실측으로 확인해 원인과 함께 기록한다 | 1장의 목표 그 자체. 검증은 동등성 기능 매트릭스(4.4)와 실측(12장)으로 한다 |
| D10 | **라이선스**: 컴포지터(bunt-comp), 셸(bunt-shell), 에이전트(buntd-agent, bunt CLI, MCP 어댑터 서버)는 GPL-3.0-or-later. 외부 앱이 링크하는 SDK·프로토콜 크레이트는 MIT OR Apache-2.0. 문서는 CC BY-SA 4.0. 기여는 DCO, 표기는 파일 단위 SPDX 헤더(REUSE 규격)로 한다(ADR-006) | buntOS는 오픈소스로 공개한다. GPLv3는 소비자 기기에 실어 배포하는 쪽에 수정 버전 설치 정보(Installation Information)를 요구하므로, 파생 기기가 다시 잠기는 것을 막아 D8·D9를 파생물까지 잇는다. SDK는 permissive로 두어 앱 생태계를 막지 않는다(D9) |

### 2.1 개방성 정의 (D8 상세)

적용 범위는 **OS가 기본 제공하거나 OS 동작이 의존하는 코드**다. 사용자가 직접 설치하는 소프트웨어는 폐쇄 앱이라도 사용자의 선택이며, D9에 따라 막지 않는다.

| 계층 | 예 | 요구 |
|---|---|---|
| AP 사용자 공간 | 셸, 시스템 앱, Mesa, PipeWire, 네트워크 서비스 | 오픈소스 필수. 폐쇄 바이너리와 bionic 호환 계층 금지 |
| AP 커널 | 커널, 드라이버, DTS | 오픈소스 필수. BSP 패치는 업스트림 제출이 기본이고, 다운스트림 패치는 제출 대기열로만 유지한다. 벤더 커널과 폐쇄 모듈 금지 |
| 펌웨어 | GPU, Wi-Fi/BT, 전원 관리 MCU 등 AP가 아닌 프로세서에서 실행되는 코드 | 예외로 허용. 선호 순서: 오픈소스 > 재배포 가능한 바이너리(linux-firmware 등) > 기기에서 추출하는 재배포 불가 바이너리(아래 규칙 조건으로 허용, ADR-012) |
| 부트 체인과 런타임 펌웨어 | preloader, LK, TF-A, TEE | 지금은 교체할 수 없으므로 범위 밖으로 두고 요구 사항으로 관리한다: (1) 사용자가 자기 커널을 부팅할 수 있어야 한다(언락), (2) EL2를 OS에 넘기는지 실측한다(12.1절), (3) 오픈 부트 체인으로의 교체는 장기 과제 |

이 선은 Debian의 non-free-firmware 구분과 같다. 펌웨어 목록은 BSP의 `board.toml`에 선언하고, 이미지에 포함하는 것(재배포 가능)과 설치 시 기기에서 추출하는 것을 구분한다.

재배포 불가 펌웨어 규칙 (ADR-012)

- 공개 이미지와 저장소에 절대 포함하지 않는다. 공개 이미지에 넣는 것 자체가 재배포다
- 설치 시 사용자 자신의 기기에서만 추출한다. 제3자 미러에서 받지 않는다
- 대상 파일과 해시는 `board.toml`에 선언하고, 추출 스크립트는 오픈소스로 둔다
- 추출에 실패하면 해당 기능만 비활성화되고 OS는 정상 부팅해야 한다
- 재배포 가능한 판이 나오면 그쪽으로 교체한다

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

**BSP 경계 규칙**: BSP가 제공할 수 있는 것은 커널, 커널 모듈, 펌웨어, 부트 이미지 형식, 보드 정의 TOML(패널 크기/배율/입력장치 매핑/전원 프로파일)뿐이다. 그 위의 모든 계층은 아키텍처와 무관하게 동일 바이너리 패키지 집합(동일 버전)이어야 한다. BSP 커널은 mainline 커널에 BSP 설정 조각과 업스트림 제출 대기 패치만 더한 것이어야 하며, 벤더 커널과 폐쇄 모듈은 BSP에도 허용하지 않는다(D8). 펌웨어는 2.1절의 예외 규칙을 따른다.

---

## 4. 아키텍처별 전략

### 4.1 x86-64 (데스크탑)

- 커널: mainline LTS, Debian 커널 설정 기반 + buntOS 조각(config fragment)
- 부트: UEFI + systemd-boot, UKI(Unified Kernel Image), Secure Boot 서명은 후속 단계
- GPU: Mesa (Intel/AMD), NVIDIA는 후속 과제
- 리스크 낮음. UI/에이전트 계층의 1차 검증 플랫폼. 개발 머신이 Apple Silicon이고 별도 x86 Linux 머신이 없으면, 로컬 1차 개발은 Mac 위 Linux VM(Lima)에서 aarch64로 하고(이미지는 VM 안에서 중첩 KVM으로 부팅) x86-64는 CI에서 KVM으로 검증한다(ADR-010, 9.4, [개발 환경](../dev/environment.md))

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
| 같은 SoC 변형 | TB373FU (Lenovo Idea Tab Pro), Moto Pad 60 Pro (하드웨어 동일 여부는 미확인) | TrebleDroid 위키 |

실기기 확보: TB375FC 2대(개발용 1, 예비 1)로 진행한다. 글로벌판 TB373FU와 하드웨어가 같은지 Phase 0에서 확인해(12장) 같으면 BSP 하나로 두 모델을 지원한다. 12.1절 판정이 "불가(EL2 점유)"면 기기 선택을 다시 검토한다(ADR-011).

#### 4.2.1 핵심 리스크 (v0.2 갱신)

1. **부트로더 언락이 비공식**: 레노버 공식 언락 채널이 TB375FC를 지원하지 않는다. XDA의 비공식 방법(LK 파티션 교체)이 존재하며 CN ROM ZUI 16 조건이 붙는다. 벽돌(brick) 위험 존재. CN 펌웨어 ZUI 16.0.20.091에서 언락과 AVB 해제 후 GSI 부팅에 성공한 보고가 있다(TrebleDroid 위키, 2025-08)
2. **MT6897 mainline 지원이 없다**: postmarketOS / mainline 커뮤니티에서 해당 SoC 포팅 사례를 찾지 못했다. 인접 SoC인 MT6878은 postmarketOS에 mainline 기반 커뮤니티 커널이 있다. 클럭, 전원 도메인, pinctrl, UFS, USB, PMIC, 디스플레이 파이프라인, 오디오를 직접 포팅해야 한다
3. **GPU는 업스트림이 지원한다**: Mesa Panfrost가 Mali-G615를 Valhall v11로 지원하고(GLES 3.1, Vulkan 1.4, OpenCL 3.0), 커널 Panthor는 Linux 6.18부터 Gx15 계열을 지원한다. 남은 일은 MT6897 쪽 GPU 전원·클럭 통합이다(추정)
4. **Wi-Fi/BT**: SoC 통합형인지, mainline 드라이버가 있는지 확인하지 못했다. 가장 큰 리스크 후보다
5. **디버그 채널**: UART가 노출되어 있지 않고, bring-up 초기에는 USB 드라이버도 없다(4.2.3)
6. **EL2 점유 가능성**: MediaTek에는 EL2에서 동작하는 자체 하이퍼바이저 GenieZone이 있다. 이 기기에서 GenieZone이 EL2를 차지하면 리눅스는 KVM을 쓸 수 없고, 이는 OS로 해결할 수 없다(D9). 12.1절에서 판정한다
7. **벤더 커널 소스 공개 미확인**: 공개되지 않으면 기기에서 추출한 벤더 DTB와 안드로이드에서의 동작 관찰이 주된 정보원이 된다. GPL 소스 공개 요청은 병행한다

#### 4.2.2 포팅 전략: mainline 단일 (v0.2 결정)

v0.1의 이중 트랙 중 **벤더 커널 하이브리드(구 Track A)는 폐기**하고 mainline(구 Track B) 하나로 간다. 구 Track A는 1장의 원인과 핵심 결정을 다음과 같이 충족하지 못한다.

| 기준 | 구 Track A의 결과 |
|---|---|
| 1장 원인 "bionic libc" | GPU 경로(libhybris + 벤더 블롭)로 bionic이 다시 들어온다. 블롭이 요구하는 안드로이드 HAL 서비스도 함께 구동해야 한다 |
| D8 개방성 | 디스플레이·터치·Wi-Fi·전원 드라이버(벤더 모듈)와 GPU 드라이버가 폐쇄 바이너리다 |
| D9 개발환경 비차단 | 커널이 벤더 모듈의 KMI(android14-6.1)에 묶여 버전 업그레이드와 구조체를 바꾸는 설정 변경이 막힌다. 출하 GKI에는 systemd 필수 옵션(`CONFIG_FHANDLE`), 컨테이너용 네임스페이스, btrfs가 빠져 있다. 브랜치 EOL은 2029-07-01 |
| D3 동등성 | BSP 경계 규칙을 완화해야만 성립하고, 커널 버전이 x86-64와 갈라진다 |
| 작업 재사용성 | 커널 재빌드와 KMI 검증, libhybris 백엔드, HAL 컨테이너는 mainline으로 이어지지 않는다 |

전략

- **업스트림 우선**: DTS와 드라이버는 업스트림(linux-mediatek 등) 제출을 기본으로 한다. `kernel/`의 다운스트림 패치는 제출 대기열로만 둔다
- **커널 계열 일치**: 실기기 커널은 x86-64와 같은 Debian 커널 계열에 BSP 설정 조각과 제출 대기 패치를 더해 빌드한다. 실기기 GPU에 필요한 커널 6.18 이상과 Mesa 25.1 이상은 forky 기본 저장소로 충족한다(ADR-004)
- **안드로이드의 역할**: OS 경로가 아니라 포팅 정보원이다. 벤더 DTB와 펌웨어 추출, 하드웨어 동작 관찰에 쓰며, 이를 위해 안드로이드 슬롯 하나를 보존한다(4.2.3)
- **렌더링**: GPU가 동작하기 전에는 pixman 소프트웨어 렌더러로 동작한다. 렌더러 추상화(D4)가 전환을 흡수한다
- **제품 개발과 병행**: 실기기 포팅은 Phase 3~5와 병행한다. 제품 기능의 종료 조건은 QEMU(양 아키텍처)에서 판정하므로 포팅 진척이 제품 개발을 막지 않는다

bring-up 단계 (각 단계는 측정 가능한 종료 조건을 가진다)

| 단계 | 내용 | 종료 조건 |
|---|---|---|
| 2a 최초 부팅 | 기기 부트로더(LK)에서 mainline 커널 + 최소 DTS(CPU, 메모리, 인터럽트, 타이머, 부트로더가 켜둔 화면의 simple-framebuffer. 가능 여부는 실측) | initramfs 셸 도달을 화면 콘솔 또는 재부팅 후 pstore 로그로 확인. 12.1절 EL2 최종 판정 |
| 2b 저장장치·디버그 | 클럭, 전원 도메인, pinctrl, UFS, USB(가젯 모드) | UFS의 Debian 루트로 부팅, USB 가젯(ACM/NCM)으로 호스트에서 SSH 접속 |
| 2c 화면·입력 | 디스플레이 파이프라인(mediatek-drm)과 DSI 패널, 터치 컨트롤러 | KMS로 네이티브 해상도 출력, 터치로 컴포지터(pixman) 조작, 로그인 셸 도달 |
| 2d 무선 | Wi-Fi/BT 드라이버와 펌웨어(ADR-012) | Wi-Fi 연결 |
| 2e GPU·전원 | Panthor + Mesa, cpufreq/EAS, 서스펜드·복귀, 배터리·충전 | GLES 렌더러로 컴포지터 동작, 서스펜드·복귀 반복 성공, 배터리 잔량 보고 |

Phase 2의 종료 조건은 2a~2d다(11장). 2e는 Phase 4·5의 실기기 판정 전에 끝낸다.

#### 4.2.3 실기기 안전 수칙

- 어떤 플래시 작업 전에도 **전체 파티션 백업** (특히 `nvram`, `nvdata`, `nvcfg`, `protect1/2`, `persist` 계열: Wi-Fi MAC/캘리브레이션 데이터. 소실 시 복구 불가)
- 안드로이드 `recovery` 와 한쪽 슬롯은 보존하여 언제든 ZUI로 복귀 가능하게 유지 (포팅 정보원으로도 쓴다)
- 언락 조건이 특정 ZUI 버전에 묶여 있으므로 펌웨어 자동 업데이트(OTA)를 끈다
- 언락 진행 조건(go/no-go, ADR-011): (1) 전체 파티션 백업을 다른 기기로 옮겨 해시로 무결성 확인, (2) 언락 절차를 서로 독립된 보고 2개 이상으로 교차 확인하고 펌웨어 버전 일치 확인, (3) BROM/preloader 모드 복구 경로(mtkclient 등)의 MT6897 지원 사전 확인, (4) OTA 차단. 하나라도 빠지면 언락하지 않는다
- 기기에는 UART가 노출되어 있지 않다. 디버그 채널은 bring-up 단계에 맞춰 확보한다: (1) 부트로더가 켜둔 화면을 simple-framebuffer 콘솔로 쓴다, (2) pstore/ramoops로 직전 부팅의 커널 로그를 확보한다, (3) USB 포팅(2b) 후에는 **USB 가젯(configfs: ACM 시리얼 + NCM 이더넷)** 을 initramfs 최초 단계에서 띄운다

### 4.3 arm64 CI 기준 타깃: QEMU `virt` (aarch64, UEFI)

- 실기기 없이 모든 커밋에서 arm64를 검증하기 위한 기준 플랫폼
- x86-64와 동일한 e2e 시나리오를 그대로 실행하여 **동등성을 기계적으로 검증**
- 실기기 회귀는 별도 on-device 스모크 테스트로 보완 (9장)

### 4.4 동등성(제1 목표) 보장 장치

1. **패키지 동등성 리포트**: 빌드마다 아키텍처별 설치 패키지 목록과 버전을 비교. arm64에만 누락된 패키지가 있으면 릴리스 차단
2. **동일 기능 매트릭스**: 셸, sudo, systemd 서비스, podman/docker 호환, 툴체인(gcc/clang/rustup/node/python/go/jdk), SSH 서버, 로컬 호스팅(포트 바인딩, 리버스 프록시)을 e2e로 양쪽에서 실행
3. **x86-64 전용 바이너리 대응** (arm64에서): binfmt_misc + qemu-user-static(컨테이너 멀티아키 이미지), 사용자 바이너리용 FEX-Emu 또는 box64 중 성능 실측 후 택1
4. **호스팅 모드**: 전원 연결 시 `systemd-inhibit` 기반 서스펜드 억제, 화면 꺼짐 상태에서도 네트워크 서비스 유지. 정책은 TOML로 설정
5. **가상화(KVM)**: OS 구성은 KVM을 막지 않는다(커널 설정에 KVM 포함, `/dev/kvm` 접근 정책 제공). 실기기에서 실제로 쓸 수 있는지는 펌웨어가 EL2를 OS에 넘기는지에 달려 있으므로 12.1절에서 판정하고, 불가하면 원인과 함께 동등성 예외로 기록한다

---

## 5. UI/UX (제2 목표)

### 5.1 원칙

- 기준: macOS의 공간 구조(상단 메뉴바, 하단 Dock, 미션 컨트롤, Spotlight형 런처, 제어 센터, 알림 센터)와 상호작용 품질(관성 스크롤, 제스처 연속성, 120Hz급 애니메이션)
- **아키텍처와 무관하게 동일한 UI**. 폼팩터 차이는 "입력 모드"로만 다룬다
  - 포인터 모드(마우스/트랙패드): 데스크탑과 동일
  - 터치 모드: 히트 영역 확대, 화면 키보드 자동 표시, 가장자리 스와이프 제스처
  - 모드 전환은 하드웨어 이벤트(키보드 연결/분리, 포인터 장치 유무)로 자동. UI 레이아웃 자체는 바뀌지 않음
- 배율(scale)은 보드 정의 TOML의 패널 물리 크기와 해상도로 계산하며 분수 배율(wp-fractional-scale-v1)을 지원
- 화면별 모양, 토큰, 컴포넌트 규칙은 [디자인 스펙](DESIGN-SPEC.md)에 둔다. 기준은 macOS 26/27과 Apple HIG다

### 5.2 구성 요소

| 컴포넌트 | 구현 | 비고 |
|---|---|---|
| `bunt-comp` | Rust, Smithay | 창 관리, 애니메이션, 제스처, 다중 seat, 신뢰 레이어. 창 목록·창 조작 프로토콜(wlr-foreign-toplevel)은 컴포지터가 직접 띄운 셸에게만, 입력기 프로토콜(input-method-v2, virtual-keyboard)은 직접 띄운 입력기(fcitx5)에게만 연다. 앱에는 text-input-v3를 준다. 시스템 설정과 셸이 쓰는 제어 API(D-Bus `org.buntos.Compositor1`: 화면 모드·배율, 키 반복, 막대 자동 가리기)를 낸다. 셸의 메뉴 막대와 Dock을 macOS의 WindowServer처럼 숨기고 꺼낸다 |
| `bunt-shell` | Rust, 레이어 셸 클라이언트. v0은 GTK4 + gtk4-layer-shell로 만들었고 ADR-001의 GTK4 시제품을 겸한다 | 메뉴바, 메뉴 막대 항목(입력 소스, 사운드, 배터리·전원 모드, Wi-Fi, Bluetooth, 앱의 StatusNotifierItem), 제어 센터, Dock, 모든 앱 패널, 알림, 배경화면. 메뉴바의 앱 메뉴는 앱이 D-Bus로 내보내는 동작(GAction)과 창 목록 프로토콜로 만든다. GNOME 앱처럼 창 안의 메뉴를 밖으로 내보내지 않는 앱은 그 메뉴를 옮길 수 없다 |
| `bunt-settings` | Rust, 시스템 앱. v0은 ADR-001 잠정안대로 libadwaita 없이 GTK4와 자체 CSS로 만들었고, GTK4 시제품을 겸한다 | 시스템 설정. macOS 26 시스템 설정의 구성(검색 사이드바, 묶음 상자)을 따른다. Wi-Fi, Bluetooth, 네트워크, 배터리, 화면 모드, 메뉴 막대, 데스크탑 및 Dock, 배경화면, 디스플레이, 키보드, 언어, 날짜와 시간, 기본 앱, 정보 |
| 공통 디자인 토큰 | CSS 변수 (`theme/tokens.css`) | 셸과 시스템 설정이 같은 색·모서리·글자 크기·유리 재질을 쓴다. 규칙은 [디자인 스펙](DESIGN-SPEC.md) |
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
- Agent Control Plane이 준비되기 전의 e2e 드라이버 (ADR-009)
  - Phase 0: QEMU 시리얼 콘솔(로그인, 명령 실행) + QMP(`input-send-event`로 키 입력, `screendump`) 임시 드라이버로 부팅 → 셸을 검증한다
  - Phase 1: Agent Control Plane 최소판(`buntd-agent` JSON-RPC, 실행 경로, AT-SPI 읽기 경로)을 앞당겨 구현한다. 키 입력은 QMP로 넣어 가상 키보드 → 컴포지터 → fcitx5 → 앱으로 이어지는 실제 입력 경로를 거치게 하고, 값 확인은 AT-SPI 읽기로 한다
  - 시나리오는 드라이버에 독립적인 형식(단계 = {동작, 기대값})으로 작성하고, Phase 3에는 드라이버만 교체한다
- 시나리오 예 (워크플로우 단위):
  1. 부팅 → 로그인 → 셸 표시
  2. 터미널 실행 → `apt install` → 스냅샷 생성 확인 → 롤백
  3. 컨테이너로 웹 서버 실행 → 로컬 포트 응답 확인 (호스팅 시나리오)
  4. 한국어 입력: 텍스트 필드에 한글 조합 입력 후 값 검증
  5. 에이전트가 앱 A의 값을 읽어 앱 B에 입력 (앱 간 제어)
  6. 에이전트가 `sensitive` 동작 시도 → 승인 UI 표시 → 에이전트 seat로 승인 불가 확인

### 9.3 실기기 (TB375FC)

- 릴리스 후보마다 수동 트리거 스모크 테스트: 부팅, 디스플레이, 터치, Wi-Fi, 배터리 잔량, 서스펜드/복귀, 가상화(12.1절 판정이 "가능"일 때)
- USB 가젯 NCM으로 호스트에서 SSH 접속하여 동일 e2e 시나리오 일부 실행 (bring-up 2b 이후)

### 9.4 CI 구성 (ADR-010)

- 저장소: GitHub 공개 저장소 [`blackmallangcow/buntOS`](https://github.com/blackmallangcow/buntOS). 1차 빌드 성공 후 공개하며, 그 전까지는 비공개로 두고 aarch64 빌드·e2e는 로컬 개발 VM(`tools/devvm/`)에서 돌린다(arm64 호스티드 러너 무료 제공은 공개 저장소 한정)
- x86-64 e2e: GitHub 호스티드 Linux 러너에서 KVM 가속으로 전체 시나리오를 돌린다
- aarch64 e2e: Phase 0 첫 주에 호스티드 arm64 러너(`ubuntu-24.04-arm`)의 `/dev/kvm` 유무를 확인해 둘 중 하나로 확정한다
  - KVM 있음: 호스티드 arm64 러너에서 전체 시나리오
  - KVM 없음: PR에서는 TCG로 핵심 시나리오를 돌린다. 가속이 필요한 전체 시나리오는 리뷰를 마친 코드에 한해 메인테이너의 Apple Silicon Mac에서 실행하고(비공개 미러 저장소의 self-hosted 러너 또는 로컬 실행), 그 결과를 병합 전 확인 항목으로 둔다(9.1)
- 공개 저장소에는 self-hosted 러너를 연결하지 않는다. 포크 PR 코드가 러너 머신에서 실행될 수 있기 때문이다
- 호스티드 러너의 디스크 제약(arm64 러너 기준 SSD 14GB)에 맞춰 이미지 빌드 캐시와 산출물 크기를 관리한다
- 라이선스 표기 검사(`reuse lint`)를 CI에 포함한다(D10)

---

## 10. 저장소 구조 (제안)

```
buntOS/
  LICENSES/               라이선스 원문 (REUSE 규격, D10)
  docs/design/            설계 문서, ADR
  docs/hw/                실기기 실측 보고서
  docs/dev/               개발 환경 안내
  image/                  mkosi 이미지 레시피 (공통 + 아키텍처별 오버레이)
  bsp/
    x86_64-generic/       커널 설정 조각, 부트 설정, board.toml
    qemu-aarch64/
    lenovo-tb375fc/       DTS, 커널 설정 조각, 부트 이미지 패키징(LK 형식), 펌웨어 추출 스크립트, board.toml(펌웨어 목록 포함)
  kernel/                 공통 설정 조각, 업스트림 제출 대기 패치
  comp/                   bunt-comp (Rust)
  shell/                  bunt-shell (Rust)
  settings/               bunt-settings, 시스템 설정 앱 (Rust)
  agent/                  buntd-agent, bunt CLI, MCP 어댑터 (Rust)
  i18n/                   Fluent 리소스 (ko, en)와 이를 읽는 공용 크레이트 bunt-i18n
  policy/                 기본 권한/전원/메모리 정책 TOML
  tests/e2e/              QEMU e2e 하네스와 시나리오
  ci/                     CI 파이프라인 정의
  tools/devvm/            개발용 빌드 VM 설정(Lima)과 설치 스크립트
```

- 이미지 빌드 도구: mkosi (Debian 지원, 아키텍처별 빌드, UKI 생성)
- 패키지 형식: 자체 컴포넌트도 .deb로 패키징하여 일반 Debian 패키지와 같은 경로로 배포/업데이트
- 패키지 미러: snapshot.debian.org의 특정 시점으로 고정하고, 주 1회 갱신 PR(e2e 통과 시 병합)로 올린다. forky 정식 릴리스 전의 공개 빌드에는 "보안 지원 없음"을 표시한다(ADR-004)
- Rust 툴체인: `rust-toolchain.toml`로 고정하고 Debian rustc에 의존하지 않는다(ADR-004)
- 라이선스 표기: 파일마다 SPDX 헤더를 달고, 헤더를 달 수 없는 파일은 `REUSE.toml`로 표기한다(D10)
- 기여: 커밋에 DCO `Signed-off-by`를 단다(D10)

---

## 11. 로드맵과 종료 조건

| 단계 | 내용 | 종료 조건 (측정 가능해야 함) |
|---|---|---|
| **Phase 0** 실측/기반 | 실기기 정보 수집 및 전체 백업, 리포 골격, mkosi로 x86-64/aarch64 QEMU 부팅 이미지, CI | 실측 보고서 작성 완료(12.1절 EL2/KVM 예비 판정 포함), 양 아키텍처 QEMU 이미지가 같은 레시피로 부팅하여 셸 도달 (e2e 1번) |
| **Phase 1** 컴포지터 최소 기능 | bunt-comp: 창 표시, 입력, 데미지 트래킹, pixman/GLES 렌더러, text-input-v3, 최소 Agent(9.2, ADR-009) | 양 아키텍처 QEMU에서 터미널 창 실행 및 한글 입력 e2e 통과, 메모리/프레임 기준선 기록 |
| **Phase 2** 실기기 bring-up (mainline) | TB375FC 언락, 4.2.2의 2a~2d 단계 (이어서 2e) | 기기에서 로그인 셸 도달, KMS 디스플레이 출력, 터치 입력, Wi-Fi 연결 (2a~2d 종료 조건) |
| **Phase 3** Agent Control Plane v0 | buntd-agent, a11y 집계, 에이전트 seat, CLI/MCP | e2e 5, 6번 통과 |
| **Phase 4** UI 셸 | 메뉴바/Dock/런처/제어센터/OSK, 입력 모드 전환 | 양 아키텍처 동일 UI, 터치/포인터 모드 자동 전환 확인 |
| **Phase 5** 개발/호스팅 완성도 | 툴체인 매트릭스, 컨테이너, 호스팅 모드, x86 바이너리 호환 | 동등성 매트릭스 전 항목 양쪽 통과 |

Phase 2는 Phase 3~5와 병행한다. Phase 3~5의 종료 조건은 QEMU(양 아키텍처)에서 판정하고, 실기기 판정은 해당 bring-up 단계(특히 2e)가 끝난 뒤 추가한다.

진행 상황 (2026-10-28)

- Phase 0: 이미지 레시피(`image/`, `bsp/`)와 e2e 1번(`tests/e2e/boot-to-shell.sh`)을 만들었다. arm64(`qemu-aarch64`)와 x86-64(`x86_64-generic`) 이미지가 같은 레시피로 빌드되고 둘 다 e2e 1번을 통과했다(1차 빌드 성공, 로컬 개발 VM 기준, [개발 환경](../dev/environment.md) 6장). 남은 종료 조건은 실기기 실측 보고서(12장)와 CI에서의 빌드·e2e다
- Phase 1·4 일부: bunt-comp v0(창 표시·이동, 레이어 셸, 커서, 배경, 단축키)과 bunt-shell v0(메뉴바, 시계, Dock)이 arm64 QEMU에서 뜬다. 빌드 VM 안(KVM)과 Mac(HVF, QEMU 창) 양쪽에서 확인했고, Mac 창에서는 사용자가 키보드·마우스로 직접 조작해 확인했다. x86-64 이미지에는 아직 넣지 않았다. 빌드 VM과 아키텍처가 달라 CI에서 빌드하며, 필수 드라이버를 넣은 뒤에 확인하기로 했다(2026-10-28)
- Phase 4 일부: 기본 앱과 시스템 설정 v0를 넣었다. 기본 앱은 리눅스 데스크톱에서 널리 쓰는 GNOME 앱(파일, 텍스트 편집기, 계산기, 이미지 보기, 문서 보기, 시스템 모니터)과 터미널용 nano, vim, htop이다. bunt-settings v0는 정보(기기 이름 포함), 모양(밝게·어둡게), 날짜와 시간(시간대, 24시간제), 언어를 다룬다. 메뉴바에 로고 메뉴(이 시스템에 관하여, 시스템 설정, 다시 시작, 시스템 종료)를, bunt-comp에 팝업 그랩·창 최대화·크기 조절을 더했다. arm64 QEMU에서 확인했다
- Phase 4 일부 (macOS형 메뉴바): 메뉴바를 macOS처럼 바꿨다. 배경은 투명하고 글씨는 흰색이며, 앞에 있는 창이 화면을 채우면(최대화, 전체 화면) 검은색이 된다. 왼쪽에는 앞에 있는 앱의 이름과 메뉴(정보, 설정, 종료, 새 창, 닫기, 확대/축소, 전체 화면, 도움말)를, 오른쪽에는 네트워크 아이콘(유선·Wi-Fi·연결 없음)과 알림 센터를 여는 날짜·시간을 둔다. bunt-shell에 알림 서버(org.freedesktop.Notifications)와 배너, Dock에 모든 앱 목록과 실행 중 표시를 넣었다. bunt-comp에 창 목록 프로토콜과 전체 화면을 더했고, 이미지에는 NetworkManager를 넣었다(QEMU에서 유선 연결). arm64 QEMU에서 확인했다
- 저장소: 2026-10-28에 위 작업을 비공개 저장소의 main에 올렸다(`6eb348d`). 지금 CI는 REUSE 검사만 하며 통과했다. 이미지 빌드와 e2e는 아직 CI에 없다(다음 작업 2번)
- Phase 4 일부 (macOS 26형 데스크톱, 2026-10-28): 기준을 macOS 26/27과 Apple HIG로 정하고 [디자인 스펙](DESIGN-SPEC.md) v0.1과 공통 디자인 토큰(`theme/tokens.css`)을 만들었다.
  - 메뉴 막대 항목: 입력 소스, 사운드, 배터리·전원 모드, Wi-Fi, Bluetooth, 앱의 StatusNotifierItem
  - 제어 센터: Wi-Fi·Bluetooth, 방해 금지, 다크 모드, 저전력 모드, 밝기, 음량
  - 화면 가운데 뜨는 모든 앱 패널, 바꿀 수 있는 배경화면
  - 시스템 설정: 사이드바 검색과 묶음 상자의 새 틀, 새 페이지 Wi-Fi, Bluetooth, 네트워크, 배터리, 메뉴 막대, 배경화면, 디스플레이, 키보드
  - arm64 QEMU에서 확인했다. Wi-Fi는 가짜 무선 장치(mac80211_hwsim)로 확인했다
- Phase 1 일부 (한글 입력, 2026-10-28):
  - bunt-comp에 text-input-v3, input-method-v2, virtual-keyboard-v1을 넣고 fcitx5와 fcitx5-hangul을 붙였다
  - 입력 소스 전환은 Ctrl+Space와 한/영 키다
  - 시스템 설정이 쓰는 컴포지터 제어 API(화면 모드·배율, 키 반복)와 해상도 변경 따라가기를 더했다
  - 셸 표면(메뉴, 제어 센터, 모든 앱, Dock) 뒤를 흐리게 그린다(유리 재질)
  - 한글 입력은 arm64 QEMU에서 사람이 조작해 확인했다. 한글 입력 e2e(자동 시험)는 아직 없다
- 화면 배율 (2026-10-28):
  - bunt-comp에 wp_viewporter, wp_fractional_scale_v1, wl_compositor v6(표면별 버퍼 배율)을 넣었다. 125%, 150% 같은 0.25 단위 배율을 쓴다
  - 배율로 나눈 논리 화면이 1024×640보다 작아지지 않는 배율만 둔다. 기본 배율은 EDID의 화면 크기로 정한다
  - 배율이나 해상도가 바뀌면 창을 화면 안으로 옮기고, 넘치면 줄인다. Dock은 넘치면 아이콘을 줄인다
  - 시스템 설정 디스플레이 페이지를 macOS처럼 "더 큰 텍스트 ↔ 더 많은 공간"과 "~처럼 보임"으로 바꿨다. 규칙은 [디자인 스펙](DESIGN-SPEC.md) 3.1절
- 메뉴 막대·Dock 설정과 기본 앱 (2026-10-28):
  - 메뉴 막대와 Dock을 자동으로 가린다. 메뉴 막대는 항상, 최대화하거나 전체 화면일 때, 전체 화면일 때, 안 함이고, Dock은 항상, 전체 화면일 때(최대화한 창도 포함), 안 함("자동으로 Dock 가리기")이다. 숨은 막대는 화면 가장자리를 가리키면 미끄러져 나온다. 셸이 설정을 컴포지터 API(`SetPanels`)로 알리고, 컴포지터가 막대를 숨기고 꺼내며 최대화·전체 화면 창이 막대 자리까지 채울지 정한다. 셸에서 막대 표면을 숨기면 GTK가 그 표면을 다시 그리지 않는 일이 있어 컴포지터로 옮겼다
  - Dock 크기(32~96px), 투명도, 실행 중 표시, 메뉴 막대 배경 보기(흐린 유리)
  - 시스템 설정에 데스크탑 및 Dock 페이지(기본 웹 브라우저 포함)와 기본 앱 페이지를 더했다. 기본 앱은 종류별(웹 브라우저, 메일, 텍스트, 이미지, 음악, 동영상, PDF, 폴더)과 확장자별로 고르고 `mimeapps.list`에 적는다. 이미지에 배포판 기본값(`/usr/share/applications/mimeapps.list`)을 넣었다
  - 규칙은 [디자인 스펙](DESIGN-SPEC.md) 3.2절, 8장, 9장
- 입력과 성능 손질 (2026-10-28):
  - 대화 상자(부모가 있는 창)는 부모 위에 남고, 모달 대화 상자(xdg-dialog-v1)가 키보드를 받는다. 시트를 띄운 채 부모 창을 누르면 시트가 뒤로 숨어 누르기가 먹지 않던 문제를 고쳤다
  - 화면에 보인 표면에만 프레임마다 프레임 알림을 보내고, 가려진 표면은 1초에 한 번 보낸다. 흐림은 아래의 손상이 흐릴 범위에 닿을 때만 다시 한다. 소프트웨어 렌더러(llvmpipe)에서는 60Hz로 그린다
  - Dock이 아이콘 크기를 줄여도 판이 줄지 않던 문제, 시스템 설정 슬라이더 길이가 행마다 다르던 문제를 고쳤다
- 필수 드라이버 (2026-10-28):
  - Debian non-free-firmware의 Wi-Fi·Bluetooth·GPU·오디오 펌웨어
  - wpasupplicant, bluez, PipeWire, UPower, power-profiles-daemon, iio-sensor-proxy
  - x86-64의 CPU 마이크로코드
  - 웹 브라우저 Firefox ESR
  - x86-64 이미지도 이 레시피로 빌드해 e2e 1번을 통과했다(TCG). x86-64 이미지의 자체 컴포넌트(그래픽 세션)는 아직 없다

다음 작업 (제안 순서)

1. 저장소 공개 전환: 사용자가 결정한다([결정 자료](DECISION-BRIEF.md) 0.3절)
2. x86-64 그래픽 세션: x86-64 이미지에도 자체 컴포넌트를 넣고, CI 호스티드 러너에서 빌드와 e2e 1번을 돌린다(Phase 0 종료 조건). 필수 드라이버를 넣은 x86-64 이미지의 빌드와 e2e 1번은 로컬에서 통과했다(2026-10-28)
3. bunt-comp 기본기: dmabuf(linux-dmabuf)와 GPU 렌더링, 최소화, 앱 창의 흐림 요청(ext-background-effect). 지금은 GTK 앱이 CPU(cairo)로 그린다. 셸 표면 뒤 흐림과 분수 배율은 넣었다(2026-10-28)
4. 한글 입력 e2e: 입력 소스 전환과 한글 조합을 자동으로 확인한다(Phase 1 종료 조건)
5. 시스템 서비스 에이전트: NetworkManager 비밀 에이전트(저장된 Wi-Fi 암호가 바뀌었을 때), BlueZ 페어링 에이전트(PIN이 필요한 기기). 시스템 설정의 사운드·알림 페이지
6. 최소 Agent: Phase 1의 e2e 드라이버 (ADR-009)
7. 메모리·프레임 기준선 기록 (Phase 1 종료 조건). ADR-001의 Qt6 시제품을 만들어 GTK4 시제품(bunt-shell v0, bunt-settings v0)과 비교 측정한다
8. 실기기 실측 보고서: TB375FC를 받은 뒤 12장 항목을 잰다 (Phase 0 종료 조건)

---

## 12. Phase 0 실측 항목 (TB375FC)

언락 전 ADB(개발자 옵션)만으로 수집 가능한 항목부터 진행한다. 목적은 두 가지다: (1) mainline 포팅 범위를 정할 하드웨어 정보 수집, (2) OS로는 해결할 수 없는 펌웨어 제약(EL2 등) 확인. 결과는 `docs/hw/lenovo-tb375fc.md`에 기록한다.

| 항목 | 명령 (별도 표기가 없으면 adb shell) | 판단 대상 |
|---|---|---|
| 커널 버전/설정 | `uname -a`, `cat /proc/version`, `zcat /proc/config.gz` (읽기 권한은 기기마다 다름) | 벤더 드라이버 목록(포팅할 하드웨어 블록 식별), GenieZone/KVM 관련 옵션 |
| SoC 식별 | `getprop ro.soc.model`, `getprop ro.board.platform`, `cat /proc/cpuinfo` | MT6897 여부 확정 |
| 글로벌판 동일성 | TB373FU 글로벌 펌웨어에서 추출한 벤더 DTB를 TB375FC 것과 비교 (호스트) | BSP 하나로 두 모델을 지원할 수 있는지 (ADR-011) |
| 부트 구성 | `getprop ro.boot.slot_suffix`, `getprop ro.boot.dynamic_partitions`, `ls -l /dev/block/by-name/` (권한 허용 범위) | A/B, super 파티션, vendor_boot 유무 |
| 부트 이미지 헤더 | 펌웨어 boot.img 추출 후 `unpack_bootimg` (호스트) | header 버전, DTB 위치. mainline 커널을 LK에 넘길 이미지 형식 결정 |
| 벤더 DTB | 펌웨어의 vendor_boot/dtbo 이미지에서 추출 후 `dtc -I dtb -O dts` (호스트) | 레지스터 주소, 클럭·전원 도메인·인터럽트 연결. mainline DTS 작성의 근거 |
| EL2/KVM | 12.1절 | 가상화 가능 여부(D9), ADR-011의 입력 |
| 디스플레이 | `dumpsys display`, `dumpsys SurfaceFlinger` | 해상도, 주사율 목록, 패널 정보(패널 드라이버 식별) |
| 디스플레이 파이프라인 | 벤더 DTB의 디스플레이 계열 노드(mmsys, ovl, rdma, dsi 등), 언락 후 `ls /dev/dri` | mediatek-drm 포팅 범위 |
| GPU | `dumpsys SurfaceFlinger \| grep -i GLES`, `ls /dev/mali*`, 벤더 DTB의 GPU 관련 노드 | MT6897 쪽 GPU 전원·클럭 통합 방식(Panthor 포팅 범위) |
| 입력 장치 | `getevent -lp` | 터치/펜/키보드 커버 컨트롤러 식별, mainline 드라이버 유무 |
| Wi-Fi/BT | `cat /proc/modules`, `getprop \| grep -i wifi` | SoC 통합형인지 외장 칩인지, mainline 드라이버 유무 |
| 펌웨어 목록 | `ls -R /vendor/firmware /vendor/etc/firmware` (권한 허용 범위) | 2.1절 펌웨어 예외 대상 목록과 재배포 가능성 분류(ADR-012) |
| 저장장치 | `cat /proc/partitions`, 벤더 DTB의 UFS 노드 | UFS 호스트·PHY 포팅 범위 |
| 메모리 기준선 | `cat /proc/meminfo`, `dumpsys meminfo` | 8.4 예산 비교 기준 |
| 전원 | `dumpsys battery`, `/sys/class/power_supply/*` | PMIC, 충전기, 배터리 게이지 식별 |

### 12.1 EL2/KVM 판정

리눅스가 KVM을 쓰려면 커널이 EL2에서 시작해야 한다. MediaTek에는 EL2에서 동작하는 자체 하이퍼바이저 GenieZone이 있고, 이것이 EL2를 차지하면 OS 구성과 무관하게 KVM을 쓸 수 없다(4.2.1-6).

| 시점 | 확인 | 해석 |
|---|---|---|
| 언락 전 (adb) | `ls -l /dev/kvm /dev/gzvm` | "No such file"이면 노드가 없다. "Permission denied"면 노드는 있다 |
| 언락 전 (adb) | `getprop \| grep -i hypervisor` | 안드로이드 가상화 프레임워크(AVF)용 하이퍼바이저 지원 표시 |
| 언락 전 (adb/호스트) | `zcat /proc/config.gz \| grep -i -E "KVM\|GZ"`, 벤더 DTB에서 `grep -i geniezone` | 벤더 커널의 KVM·GenieZone 사용 흔적 |
| 언락 후 (2a 단계) | mainline 커널 부팅 로그의 `CPU: All CPU(s) started at EL2`(또는 `EL1`)와 kvm 초기화 메시지 | 최종 판정 근거 |

판정

- **가능**: 커널이 EL2에서 시작하고 mainline 커널에서 `/dev/kvm`이 생성된다 → 동등성 매트릭스의 가상화 항목(4.4-5)을 실기기에도 적용한다
- **불가 (EL2 점유)**: 커널이 EL1에서 시작하고 GenieZone 흔적이 있다 → OS로는 해결할 수 없다. 원인과 함께 D9 예외로 기록하고 ADR-011(기기 선택)의 입력으로 올린다
- **미정**: 안드로이드 측 신호는 예비 판정일 뿐이다. 신호가 엇갈리거나 부족하면 2a 단계의 부팅 로그로 판정한다

---

## 13. 미결정 사항 (ADR로 결정 예정)

| ID | 질문 | 결정 시점 | 결정 방법 |
|---|---|---|---|
| ADR-001 | 시스템 앱 UI 툴킷 GTK4 vs Qt6 | Phase 1 | 동일 시제품 앱으로 a11y 트리 품질, 한글 IME, 메모리 실측 비교. GTK4 쪽 시제품은 bunt-shell v0와 bunt-settings v0로 있고, Qt6 시제품과 측정은 아직이다 |
| ADR-003 | x86 바이너리 호환 FEX-Emu vs box64 | Phase 5 | 대표 워크로드 벤치마크 |
| ADR-005 | UI 폰트 | Phase 4 | 렌더링 품질/메모리 실측 |

결정됨

| ID | 결정 | 반영 위치 | 버전 |
|---|---|---|---|
| 포팅 전략 | mainline 단일. 벤더 커널 하이브리드(구 Track A) 폐기 | 4.2.2 | v0.2 |
| ADR-002 | arm64 실기기 GPU 경로: Panthor + Mesa. GPU가 동작하기 전에는 pixman | D8, 4.2.2 | v0.2 |
| ADR-004 | Debian 기준: forky(testing) 스냅샷 고정, forky 정식 릴리스 시 stable 전환 | D2, 10장 | v0.3 |
| ADR-006 | 라이선스: 핵심부 GPL-3.0-or-later, SDK MIT OR Apache-2.0, 문서 CC BY-SA 4.0, DCO, REUSE | D10, 10장 | v0.3 |
| ADR-009 | e2e 드라이버: Phase 0 시리얼 콘솔 + QMP, Phase 1 최소 Agent | 9.2, 11장 | v0.3 |
| ADR-010 | CI: GitHub 공개 저장소, 호스티드 러너 중심, 공개 저장소에 self-hosted 러너 미연결 | 4.1, 9.4 | v0.3 |
| ADR-011 | 실기기: TB375FC 2대(개발 1, 예비 1). EL2 "불가" 판정 시 기기 재검토, 글로벌판 동일성 확인 | 4.2, 4.2.3, 12장 | v0.3 |
| ADR-012 | 재배포 불가 펌웨어: 허용하되 공개 이미지·저장소에 넣지 않고 사용자 기기에서만 추출 | 2.1절 | v0.3 |

ADR-007·008은 구 Track A 폐기로 무효가 됐다. 각 항목의 선택지와 근거는 [DECISION-BRIEF.md](DECISION-BRIEF.md)에 있다.

---

## 14. 참고 자료

- Aside 설계: https://aside.com/blog/how-we-built-the-sota-browser-agent-that-outperforms-fable
- TB375FC 사양: https://www.devicespecifications.com/en/model/53556179
- TB375FC 비공식 언락: https://xdaforums.com/t/unofficial-unlocking-lenovo-xiaoxin-pad-pro-2025-tb375fc.4717695/
- Panthor G610 적합성: https://www.collabora.com/news-and-blog/news-and-events/taming-the-panthor-opengl-es-31-conformance-achived-mali-g610.html
- Mesa Panfrost (지원 GPU 표, G615 = Valhall v11): https://docs.mesa3d.org/drivers/panfrost.html
- Panthor의 Gx15 계열 지원 (Linux 6.18): https://www.phoronix.com/news/Panthor-More-Arm-Mali-Linux-618
- Dimensity 8300: https://www.mediatek.com/products/smartphones/mediatek-dimensity-8300
- TB375FC 언락·GSI 부팅 보고 (TrebleDroid): https://github-wiki-see.page/m/TrebleDroid/treble_experimentations/wiki/Lenovo-Idea-Tab-Pro---Lenovo-Xiaoxin-Pad-Pro-2025---Motorola-Moto-Pad-60-Pro
- MT6878 mainline 기반 커뮤니티 커널 (postmarketOS): https://pkgs.postmarketos.org/package/master/postmarketos/aarch64/linux-postmarketos-mediatek-mt6878
- GenieZone 하이퍼바이저 (커널 문서 패치): https://lists.infradead.org/pipermail/linux-arm-kernel/2023-July/853606.html
- systemd 커널 요구 사항: https://github.com/systemd/systemd/blob/main/README
- GPLv3 원문 (6조 User Product, Installation Information): https://www.gnu.org/licenses/gpl-3.0.html
- REUSE 규격 3.3: https://reuse.software/spec-3.3/
- GitHub self-hosted 러너 (공개 저장소 관련 경고): https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/add-runners
- GitHub 공개 저장소용 arm64 호스티드 러너 정식 출시: https://github.blog/changelog/2025-08-07-arm64-hosted-runners-for-public-repositories-are-now-generally-available/
- 구 Track A 검토 자료: Android 공통 커널 지원 기간 https://source.android.com/docs/core/architecture/kernel/android-common , android14-6.1 `gki_defconfig` https://android.googlesource.com/kernel/common/+/refs/heads/android14-6.1/arch/arm64/configs/gki_defconfig
