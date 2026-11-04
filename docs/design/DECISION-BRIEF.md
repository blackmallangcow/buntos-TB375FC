# buntOS 설계 결정 자료 v0.3.12

- 작성일: 2026-10-27
- 변경 이력
  - v0.3.12 (2026-11-04): ADR-011에 EL2 판정과 사람의 결정을 적었다. TB375FC는 EL1로 시작해 KVM을 쓸 수 없지만, 일반 개발 환경에는 충분해 기기를 바꾸지 않고 그대로 진행한다
  - v0.3.11 (2026-11-03): 앱 설치 결정(ADR-014)을 더했다. 설치 파일은 자체 앱 패키지 매니저(bunt-packages)가, 앱 찾기는 GNOME Software와 Flathub가 맡는다
  - v0.3.10 (2026-11-01): ADR-011의 언락 보고를 고쳤다. 16.0.20.091에는 성공과 실패 보고가 함께 있고, ZUXOS(Android 16) 기기는 16.0.20.091로 내린 뒤 언락했다
  - v0.3.9 (2026-11-01): 제품 라인 결정(ADR-013)을 더했다. 이 저장소는 데스크톱 라인이고, 태블릿은 arm64 포크 라인이다. ADR-011은 태블릿 라인의 결정이 됐고, XDA 언락 원문을 브라우저로 확인한 것을 적었다
  - v0.3.8 (2026-11-01): ADR-011에 펌웨어로 확인한 것을 더했다(TB375FC와 TB373FU는 같은 하드웨어, Wi-Fi는 SoC 통합 connsys, GenieZone 흔적)
  - v0.3.7 (2026-10-30): 공개 시점을 실기기(TB375FC) 시험을 마친 뒤로 미뤘다(0.3절, ADR-010)
  - v0.3.6 (2026-10-28): ADR-001 진행에 macOS 26형 셸·시스템 설정 개편, 공통 디자인 토큰과 디자인 스펙, 한글 입력기(fcitx5, M2 측정의 전제)를 더했다
  - v0.3.5 (2026-10-28): 0.3절에 비공개 저장소 반영 상태(main `6eb348d`, CI의 REUSE 검사 통과)를 기록했다
  - v0.3.4 (2026-10-28): ADR-001 진행에 시스템 설정 앱(bunt-settings v0)을 더했다. 잠정안대로 libadwaita 없이 GTK4와 자체 CSS로 만들었다
  - v0.3.3 (2026-10-27): 1차 빌드 성공과 공개 전 그래픽 UI 확인을 0.3절에, bunt-shell v0가 GTK4 시제품을 겸한다는 것을 ADR-001에 기록했다
  - v0.3.2 (2026-10-27): 공개 시점을 1차 빌드 성공 후로 정했다. ADR-010의 로컬 개발 방식을 검증한 구성(Lima VM, 중첩 KVM)으로 구체화했다
  - v0.3.1 (2026-10-27): 공개 준비를 반영했다. 토큰 교체 불필요 확인, 이력 정리, README·CONTRIBUTING·REUSE 추가, 저장소 위치(`blackmallangcow/buntOS`) 지정
  - v0.3 (2026-10-27): 오픈소스 공개를 전제로 ADR-004·006·009·010·011·012를 확정했다. ADR-010은 공개 저장소 보안 권고에 따라 v0.2 권고(공개 저장소에 Mac self-hosted 러너 연결)를 철회하고 호스티드 러너 중심으로 바꿨다. 공개 전 필수 작업(0.3절)을 추가했다
  - v0.2 (2026-10-27): 실기기 전략을 mainline 단일로 확정하고 구 Track A(벤더 커널 하이브리드)를 폐기했다. 그에 따라 ADR-002 결정, ADR-007·008 폐기, ADR-011 개편, ADR-012 신설. 개방성 정의와 EL2/KVM 실측은 [ARCHITECTURE.md](ARCHITECTURE.md) 2.1절, 12.1절에 반영했다
  - v0.1 (2026-10-27): 최초 작성
- 대상: [ARCHITECTURE.md](ARCHITECTURE.md) v0.3.7의 13장
- 표기
  - **[확인]** 공개 자료로 확인함. 괄호 안 번호는 문서 끝 출처(S번호)
  - **[추정]** 근거에서 유도한 판단이며 검증이 필요함
  - **[미확인]** 찾아봤지만 확인하지 못함
  - **[실측]** Phase 0 이후 측정으로만 확정할 수 있음
- 각 항목에는 결정과 함께 그 결정에 이른 선택지와 근거를 남긴다.

---

## 0. 한눈에 보기

### 0.1 결정됨

| ID | 결정 | 버전 |
|---|---|---|
| 포팅 전략 | **mainline 단일**. 구 Track A(벤더 커널 하이브리드) 폐기 | v0.2 |
| D8·D9 | 개방성 정의(펌웨어만 예외), 개발환경 비차단 | v0.2 |
| ADR-002 | GPU 경로: **Panthor + Mesa**. GPU가 동작하기 전에는 pixman | v0.2 |
| ADR-004 | Debian 기준: **forky(testing) 스냅샷 고정**, forky 정식 릴리스 시 stable 전환 | v0.3 |
| ADR-006 | 라이선스: 핵심부 **GPL-3.0-or-later**, SDK·프로토콜 크레이트 **MIT OR Apache-2.0**, 문서 **CC BY-SA 4.0**, 기여는 **DCO**, 표기는 **REUSE** | v0.3 |
| ADR-009 | e2e 드라이버: Phase 0은 **시리얼 콘솔 + QMP**, Phase 1에 **최소 Agent** | v0.3 |
| ADR-010 | CI: **GitHub 공개 저장소 + 호스티드 러너 중심**. 공개 저장소에는 self-hosted 러너를 연결하지 않음 | v0.3 |
| ADR-011 | 실기기: **TB375FC 2대(개발 1, 예비 1)**. EL2 "불가" 판정(2026-11-03)에도 기기 유지(2026-11-04), 글로벌판 동일성 확인 | v0.3 |
| ADR-012 | 재배포 불가 펌웨어: **허용**하되 공개 이미지·저장소에 넣지 않고 사용자 기기에서만 추출 | v0.3 |
| ADR-013 | 제품 라인: **이 저장소는 x86-64·arm64 데스크톱 라인**(GPU 가속), **태블릿은 arm64 포크 라인**(기기마다 최적화, 첫 포크 `buntos-TB375FC`) | v0.3.9 |
| ADR-014 | 앱 설치: **설치 파일은 자체 앱 패키지 매니저**(.deb는 PackageKit, Flatpak은 flatpak, AppImage는 풀어서), **앱 스토어는 GNOME Software + Flathub** | v0.3.11 |
| ADR-007·008 | 폐기 (구 Track A 폐기로 전제 소멸) | v0.2 |

### 0.2 결론은 실측 후에 내되, 평가 기준은 지금 고정할 것

| ID | 질문 | 잠정 가설 | 결정 시점 |
|---|---|---|---|
| ADR-001 | 시스템 앱 툴킷 GTK4 vs Qt6 | GTK4(libadwaita 미사용). 도전자는 Qt6 QML | Phase 1 |
| ADR-003 | FEX-Emu vs box64 | box64(Debian 패키지 존재). FEX와 벤치마크 비교 | Phase 5 |
| ADR-005 | UI 폰트 | Pretendard(UI) + Noto CJK(폴백·고정폭). 둘 다 OFL이라 공개 배포에 문제없음. 지금 정해도 비용이 거의 없음 | Phase 4 (조기 결정 가능) |

### 0.3 공개 전 필수 작업

저장소 [`blackmallangcow/buntOS`](https://github.com/blackmallangcow/buntOS)를 공개로 전환하기 전에 끝내야 한다. **공개 시점은 실기기(TB375FC) 시험을 마친 뒤로 정했다(2026-10-30. 처음에는 1차 빌드가 성공한 뒤였다). 그때까지 저장소는 비공개로 둔다.** 1차 빌드는 2026-10-27에 성공했다. arm64와 x86-64 이미지가 같은 레시피로 빌드되고 둘 다 e2e 1번을 통과했다([개발 환경](../dev/environment.md) 6장). 이어서 공개할 내용이 보이도록 그래픽 UI까지 만든 뒤 사용자가 직접 확인하기로 했고(2026-10-27), 확인을 마쳤다(6번). 그 뒤 기본 앱과 시스템 설정을 더해 2026-10-28에 비공개 저장소의 main에 올렸고(`6eb348d`), CI의 REUSE 검사를 통과했다. 남은 것은 실기기 시험과 공개 전환이다. 제품 라인을 나눈 뒤(ADR-013, 2026-11-01) 이 저장소의 실기기는 데스크톱 PC가 됐으므로, 공개 시점을 다시 정한다.

1. **토큰 교체** (불필요, 2026-10-27 사용자 확인): `opencode.json`의 `X-Nariya-Token`은 나리야 플랫폼에 접속하는 클라이언트 신원 토큰이다. 플랫폼이 재시작될 때마다 바뀌어 곧바로 폐기되고, 나리야 클라이언트는 자체 클라이언트라 외부에 없다. 따라서 노출돼도 문제가 없다
2. **이력 정리** (2026-10-27 완료): 모든 커밋에서 `opencode.json`과 `attachments/`를 제거했다. 두 경로만 담았던 나리야 자동 커밋 3개는 비어서 빠졌고, 커밋 해시가 바뀌었다. 저장소의 모든 객체에서 토큰 값이 0건임을 확인했다
3. **재유입 방지** (2026-10-27 완료): `.gitignore`에 두 경로를 추가했다. 나리야가 이 파일들을 강제로 커밋하는지는 다음 작업 때 확인한다
4. **ADR-006 이행** (2026-10-27 완료): 현재 파일의 라이선스를 `REUSE.toml`로 표기했다(문서 CC-BY-SA-4.0, 저장소 설정 파일 GPL-3.0-or-later). `LICENSES/`에는 쓰는 라이선스 원문만 둔다. REUSE는 쓰이지 않는 라이선스 원문이 있으면 규격 위반으로 판정하기 때문이다. MIT 원문은 Smithay 예제(smallvil)에서 가져온 코드(`comp/src/grabs.rs`)가 생겨 추가했고, Apache-2.0 원문은 SDK 코드가 생길 때 추가한다. CI에 `reuse lint`를 넣었고 CONTRIBUTING에 DCO 규칙을 적었다
5. **ADR-004 이행** (2026-10-27 완료): README에 forky 정식 릴리스 전 빌드는 보안 지원이 없다고 표시했다
6. **그래픽 UI 확인** (2026-10-27 완료): 이미지를 Mac에서 창으로 띄워 bunt-comp v0와 bunt-shell v0(메뉴바, 시계, Dock, 터미널 실행)를 사용자가 직접 확인했다. 시계(한국 시간), 화면 구성, 키보드·마우스 입력이 정상이었다. 띄우는 방법은 [개발 환경](../dev/environment.md) 4장에 있다

---

## 1. 조사로 바뀐 전제

F2·F3(출하 GKI 설정 공백, android14-6.1 EOL)은 구 Track A의 폐기 근거로 2장에 옮겼다.

| # | 기획서 v0.1 서술 | 조사 결과 | 영향 |
|---|---|---|---|
| F1 | 4.2.1-3: "G615에 대한 공식 지원은 확인되지 않음" | Mesa 공식 Panfrost 문서가 G615/G715를 **Valhall v11**로 등재한다(GLES 3.1, Vulkan 1.4, OpenCL 3.0) [확인 S1]. v11 관련 커밋은 Mesa 25.1.0 릴리스 노트부터 보인다 [확인 S2]. 커널 Panthor는 **Linux 6.18**부터 Gx15 계열을 지원한다 [확인 S3] | mainline 단일 전략의 전제. GPU가 더 이상 mainline의 병목이 아니다 |
| F4 | 4.2.1-1: 언락이 비공식 | TB375FC(CN ZUI 16.0.20.091)에서 언락과 AVB 해제 후 GSI 부팅에 성공한 보고가 있다(2025-08) [확인 S7]. 같은 SoC를 쓰는 TB373FU(Idea Tab Pro)와 Moto Pad 60 Pro가 있다 [확인 S7]. 검색으로 잡히는 "EDL/QFIL" 언락 가이드는 퀄컴 도구 기반이라 이전 세대(스냅드래곤) 모델용으로 보이며, MediaTek 기기인 TB375FC와는 무관하다 [추정] | 언락 자체는 가능성이 높다. XDA 원문은 접근이 차단돼(403) 절차를 직접 확인하지 못했다 → ADR-011 |
| F5 | 7장 폰트 후보 | `fonts-pretendard`가 Debian trixie main에 있다 [확인 S8] | ADR-005의 패키징 장벽이 없다 |
| F6 | 4.4-3 FEX vs box64 | box64는 Debian main에 있고(trixie 0.3.4, forky 0.4.3), FEX-Emu는 Debian에 없다(Ubuntu PPA, Fedora 통합) [확인 S8, S9, S28] | D2/D3 파이프라인 적합성 차이 → ADR-003 |
| F7 | 4.1: "x86-64가 UI/에이전트 계층 개발의 1차 플랫폼" | 현재 개발 머신은 Apple M4(arm64)다. 이 머신에서 aarch64 QEMU는 HVF로 가속되지만 x86-64 QEMU는 TCG(소프트웨어 에뮬레이션)로 돈다 [추정] | ADR-010에서 로컬 1차 개발 플랫폼 규칙으로 반영 |
| F8 | (언급 없음) | MediaTek에는 EL2에서 동작하는 type-1 하이퍼바이저 GenieZone이 있다 [확인 S30]. GenieZone은 `/dev/gzvm`으로 노출된다 [확인 S31]. 리눅스 KVM(pKVM 포함)은 EL2를 필요로 한다 [확인 S32]. 이 기기에서 GenieZone이 EL2를 차지하는지는 [미확인] | 점유 시 어느 트랙이든 KVM 불가(OS로 해결 불가) → ARCHITECTURE 12.1절 실측, ADR-011 입력 |

---

## 2. 구 Track A 폐기 기록 (2026-10-27)

**계기**: 사용자가 목표를 재확인했다. "완전히 열린 스택"과 "OS가 개발환경 세팅을 막아서는 안 된다"이다. v0.1의 4.2.2는 "빨리 실사용 상태에 도달"을 기준으로 Track A를 먼저 택했는데, 이 기준이 1·2장의 목표와 충돌했다. 목표가 문서에 문장으로 적혀 있지 않았던 것이 원인이므로, v0.2에서 D8·D9로 명시했다.

### 2.1 1장에서 지목한 원인별 점검

| 지목한 원인 | 구 Track A | mainline |
|---|---|---|
| 앱 샌드박스, root 미제공, SELinux 강제 | 해소 | 해소 |
| 패키지 매니저 부재 | 해소 | 해소 |
| Doze, 백그라운드 강제 종료 | 해소 | 해소 |
| 앱 격리, 접근성 API 제한 | 해소 (사용자 공간 문제라 트랙과 무관) | 해소 |
| **bionic libc** | **GPU 경로로 다시 들어온다.** libhybris는 bionic으로 빌드된 안드로이드 드라이버를 glibc 프로세스에 불러오는 계층이다 [확인 S25]. GPU를 쓰는 모든 프로그램이 bionic 블롭을 거치고, 블롭이 요구하는 안드로이드 HAL 서비스도 함께 구동해야 한다 | 해소 (Mesa) |

### 2.2 핵심 목표와 설계 결정별 점검

| | 구 Track A의 결과 |
|---|---|
| 완전히 열린 스택 | 디스플레이·터치·Wi-Fi·전원 드라이버(벤더 모듈)와 GPU 드라이버가 폐쇄 바이너리다. 고치지도, 감사하지도, 보안 패치하지도 못한다 |
| 개발환경 비차단 | 커널이 6.1의 KMI에 묶여 버전 업그레이드가 막히고, 구조체를 바꾸는 설정도 켤 수 없고, 2029-07-01 이후엔 보안 패치도 없다 [확인 S6]. systemd는 이미 5.10 미만 커널을 지원하지 않는다 [확인 S5]. 이 하한이 계속 오르면 사용자 공간 업그레이드까지 막힐 수 있다 [추정]. GPU 연산도 표준 Mesa 경로(Vulkan 1.4, OpenCL 3.0 [확인 S1])를 못 쓴다 |
| D2 (glibc, Debian 생태계 그대로) | bionic이 다시 들어오고, 재배포할 수 없는 블롭에 의존한다 |
| D3 (차이는 BSP에만) | BSP 경계 규칙 완화(구 ADR-008)가 필요했고, 커널 버전이 x86-64(6.12 이상)와 기기(6.1)로 갈라진다 |
| D4 (Smithay) | 표준 DRM/GBM/EGL 대신 hwcomposer/gralloc 전용 백엔드가 필요하다 [추정] |
| 작업 재사용성 | 커널 재빌드와 KMI 검증, libhybris 백엔드, HAL 컨테이너는 mainline으로 이어지지 않는다 |

### 2.3 근거: 출하 GKI 커널 설정 (구 ADR-007 조사)

android14-6.1 `gki_defconfig`의 관련 설정 [확인 S4]:

| 옵션 | 출하 GKI | 필요한 곳 |
|---|---|---|
| `CONFIG_FHANDLE` | 꺼짐 | systemd 필수 항목 [확인 S5] |
| `CONFIG_PID_NS` | 꺼짐 | 컨테이너(podman), systemd 샌드박싱 |
| `CONFIG_USER_NS` | 없음(꺼짐) | rootless podman [확인 S16], systemd `PrivateUsers=` [확인 S5], 에이전트 사용자 네임스페이스(ARCHITECTURE 6.5) |
| `CONFIG_BTRFS_FS` | 없음 | D6 (btrfs + 스냅샷) |
| `CONFIG_NF_TABLES` | 없음 | nftables 기반 방화벽과 컨테이너 네트워크 |
| `CONFIG_AUTOFS_FS` | 없음 | systemd 권장 항목 [확인 S5] |

구 Track A를 살리려면 ACK 소스로 커널을 재빌드하면서 KMI를 지켜야 했다. 그래도 벤더 모듈과 GPU 블롭이 폐쇄인 점은 바뀌지 않는다.

**결론**: 구 Track A가 푸는 것은 안드로이드 사용자 공간에서 온 원인뿐이다. 커널·드라이버·GPU 층에서는 목표를 위반하고, 중간 단계로 삼아도 그 작업이 mainline으로 이어지지 않는다.

---

## 3. 확정된 결정 (v0.3 이후)

### ADR-004. Debian 기준 브랜치 → forky 스냅샷 고정

**질문**: trixie(13, stable) + backports로 갈 것인가, forky(14, testing)로 갈 것인가.

**현재 버전 (2026-10-27, qa.debian.org madison 조회)** [확인 S8]

| 패키지 | trixie | trixie-backports | forky (testing) | buntOS에서의 의미 |
|---|---|---|---|---|
| linux | 6.12 LTS | 7.2 | 7.2 | 실기기 GPU(Panthor Gx15)에 6.18 이상 필요 |
| mesa | 25.0.7 | 26.1.6 | 26.1.6 | G615(v11) 지원은 25.1 이후라 실기기 GPU에 필요. trixie 본 저장소로는 어렵다 [추정] |
| rustc | 1.85.1 | 1.96.1 | 1.97.1 | gtk4-rs 0.11의 MSRV는 1.92, accesskit 1.87, iced 1.88 [확인 S10] |
| gtk4 | 4.18.6 | 없음 | 4.24.1 | |
| qt6-base | 6.8.2 | 없음 | 6.11.2 | Qt에서 text-input-v3를 쓰려면 6.8.2 이상 권장 [확인 S11] |
| fcitx5 / fcitx5-hangul | 5.1.12 / 5.1.6 | 없음 | 5.1.23 / 5.1.11 | 한글 IME |
| systemd | 257 | 없음 | 262 | |
| podman | 5.4.2 | 없음 | 5.8.6 | |
| box64 | 0.3.4 | 없음 | 0.4.3 | |
| mkosi | 25.3 | 없음 | 27.1 | 빌드 호스트 도구 |

**일정**

- trixie: 2025-08-09 릴리스. 정규 보안 지원은 2028-08-09까지, LTS는 2030-06-30까지 [확인 S12]
- forky: 2026-10 현재 동결 일정 미발표. 데스크탑 아트워크 공모 마감이 2026-11-26이다. 2년 주기로 보면 2027년 중반 릴리스가 예상된다 [확인 S13, 일정은 추정]

**선택지 비교**

| 기준 | A. trixie + backports (기획서 v0.1 우선안) | B. forky 스냅샷 고정 | C. trixie + 자체 백포트 저장소 |
|---|---|---|---|
| 보안 업데이트 | Debian 보안팀이 지원 | testing은 보안팀 지원 대상이 아니다. 수정이 unstable을 거쳐 늦게 도착한다 | 자체 백포트분은 직접 책임 |
| Mesa / 커널 / rustc | backports에 의존해야 해결 | 기본 저장소에서 해결 | 해결 |
| GTK / Qt / fcitx5 / systemd / podman | trixie 버전에 고정 (backports에 없음) | 최신 | 직접 유지해야 해서 부담이 크다 |
| 실측 결과의 수명 (규칙 4) | forky로 옮기면 ADR-001/005 실측을 다시 해야 한다 | 출시할 플랫폼에서 측정하게 된다 | A와 같음 |
| 재현성 | 높음 | snapshot.debian.org 시점 고정이 필요 | 중간 |
| 동등성 (D3) | amd64·arm64 모두 release arch라 버전이 같다 | testing 이주도 release arch 전체가 함께 움직여서 버전이 같다 | 직접 보장해야 한다 |
| 리베이스 비용 | 2027년 이후 trixie→forky 1회 | 없음 | 큼 |

**결정 (2026-10-27): B. forky(testing) 스냅샷을 고정해 쓰고, forky 정식 릴리스 시점에 stable(forky)로 전환한다.**

이유
1. 외부 사용자에게 실사용 수준으로 내놓는 시점이 forky 릴리스 예상 시점(2027년 중반)보다 늦을 가능성이 높다.
2. A로 시작하면 Phase 1의 ADR-001 실측(GTK 4.18 vs Qt 6.8)을 forky의 GTK 4.24 / Qt 6.11에서 다시 해야 한다.
3. 핵심 패키지(Mesa, rustc, 커널)는 A에서도 어차피 backports를 써야 하므로, A가 주는 "안정성" 이점이 정작 중요한 곳에서 이미 희석된다.
4. mainline 실기기에는 커널 6.18 이상과 Mesa 25.1 이상이 필요하다. forky는 둘 다 기본 저장소에 있고(7.2 / 26.1), trixie는 둘 다 backports에 기대야 한다.

이행 규칙
- mkosi 미러를 snapshot.debian.org의 특정 시점 URL로 고정한다. 스냅샷 갱신은 주 1회 PR로 올리고, e2e를 통과해야 병합한다.
- forky 정식 릴리스 전까지 공개 빌드와 README에 "보안 지원 없음"을 명시한다.
- 자체 컴포넌트의 Rust 툴체인은 `rust-toolchain.toml`로 고정하고 Debian rustc에 의존하지 않는다.
- testing은 커널 버전이 자주 바뀌므로 실기기 대기 패치를 자주 리베이스해야 한다. 업스트림 우선 원칙(ARCHITECTURE 4.2.2)으로 대기 패치를 줄여 이 비용을 낮춘다.

재검토 조건
- 2027년 상반기 이전에 외부 사용자 배포가 필요해지면 → A
- forky 동결이 2027년 하반기 이후로 밀린다고 발표되면 → 재검토

---

### ADR-006. 자체 컴포넌트 라이선스 → 핵심 GPL-3.0-or-later + SDK permissive

**전제 (2026-10-27)**: buntOS는 오픈소스로 공개한다.

**사실관계**

- Smithay는 MIT다 [확인 S10]. 그런데 Smithay로 만든 대표 컴포지터들은 copyleft다: niri는 GPL-3.0-or-later [확인 S14], cosmic-comp는 GPL-3.0, COSMIC의 위젯 라이브러리 libcosmic은 MPL-2.0 [확인 S15, GitHub 라이선스 조회].
- 주요 의존성: gtk4-rs MIT, cxx-qt MIT/Apache-2.0, accesskit MIT/Apache-2.0, fluent MIT/Apache-2.0, iced MIT. Slint만 GPL-3.0-only 또는 자체 라이선스다 [확인 S10].
- GTK(LGPL-2.1+)와 Qt(LGPL-3.0/GPL/상용)는 동적 링크로 쓰면 아래 어느 선택지와도 양립한다.
- 커널 패치와 DTS는 업스트림 규칙(커널 코드는 GPL-2.0, DTS는 관례상 GPL·permissive 이중 라이선스)을 따르므로 선택 대상이 아니다(D8). 펌웨어는 ARCHITECTURE 2.1절과 ADR-012를 따른다.
- GPLv3 6조는 "User Product"(소비자 기기)에 GPLv3 저작물의 목적 코드를 실어 배포할 때, 수정한 버전을 그 기기에 설치·실행하는 데 필요한 "Installation Information"을 함께 제공하도록 요구한다 [확인 S37].
- CC BY-SA 4.0은 2015-10-08부터 GPLv3와 한 방향으로 호환된다. BY-SA 4.0 자료를 각색해 GPLv3로 낼 수 있다 [확인 S38].
- 리눅스 커널은 기여 규칙으로 DCO(`Signed-off-by`)를 쓴다 [확인 S40].

**선택지 비교**

| 기준 | A. 전부 MIT OR Apache-2.0 | B. 핵심부 GPL-3.0+ / SDK는 permissive | C. 전부 MPL-2.0 | D. 전부 GPL-3.0+ |
|---|---|---|---|---|
| niri / cosmic-comp 코드 재사용 | 불가 (아이디어만 참고) | 핵심부에 가능 | 불가 (GPL 코드는 MPL 파일에 넣을 수 없다) | 가능 |
| 제3자 앱의 SDK 사용 | 자유 | 자유 | 자유 (수정한 파일만 공개) | GPL이 전파돼 제약 |
| 폐쇄 포크(OEM 등) 방지 | 못함 | 핵심부는 방지 | 수정 파일만 | 방지 |
| 특허 조항 | Apache-2.0에 있음 | GPL-3.0과 Apache-2.0에 있음 | 있음 | 있음 |
| 나중에 라이선스 변경 | 쉬움 | CLA가 없으면 어려움 | 중간 | 어려움 |

**결정을 가르는 두 질문과 답**

1. 제3자가 buntOS를 가져가 폐쇄 제품으로 만드는 것을 허용하나? → **핵심부는 허용하지 않는다.** 앱과 SDK는 허용한다
2. niri나 cosmic-comp의 코드를 직접 가져다 쓸 수 있게 하나? → **가능하게 둔다**

**결정 (2026-10-27): B**

| 대상 | 라이선스 |
|---|---|
| 컴포지터(bunt-comp), 셸(bunt-shell), 에이전트(buntd-agent, bunt CLI, MCP 어댑터 서버) | GPL-3.0-or-later |
| 외부 앱이 링크하는 SDK·프로토콜 크레이트(앱 매니페스트, 에이전트 클라이언트 라이브러리, MCP 어댑터 클라이언트 등) | MIT OR Apache-2.0 |
| 문서 (`docs/`) | CC BY-SA 4.0 |
| 커널 패치, DTS | 업스트림 규칙 (커널 GPL-2.0, DTS는 관례상 이중 라이선스) |
| 기여 | DCO (커밋에 `Signed-off-by`) |
| 표기 | 파일마다 SPDX 헤더, REUSE 규격 3.3 준수, 라이선스 원문은 `LICENSES/` [확인 S39] |

이유
- **출발점과 일치한다.** 이 프로젝트는 "제조사가 잠근 태블릿"에서 출발했다. GPLv3 6조 덕분에 buntOS 핵심부를 태블릿에 실어 파는 쪽은 사용자가 수정 버전을 설치할 수 있게 해야 한다. D8·D9가 파생 기기까지 이어진다.
- **재사용할 수 있다.** niri(GPL-3.0-or-later)와 cosmic-comp(GPL-3.0) 코드를 쓸 수 있다. 단 cosmic-comp 코드를 들여온 바이너리는 사실상 GPL-3.0-only가 된다.
- **앱 생태계를 막지 않는다.** SDK가 permissive라 폐쇄 앱도 buntOS용으로 만들 수 있다. 사용자가 설치하는 소프트웨어를 막지 않는다는 D9와 같은 선이다.
- **기여 흐름이 하나로 맞는다.** D8에 따른 커널 업스트림 작업도 DCO를 쓴다. CLA를 두지 않으므로 나중에 상용 이중 라이선스로 바꾸기는 사실상 어렵다. 이 점은 감수한다.
- **문서도 가져다 쓸 수 있다.** CC BY-SA 4.0 문서는 GPLv3 코드·문서에 각색해 넣을 수 있다.

대가: GPLv3를 꺼리는 기업은 핵심부를 가져가지 않을 것이다. 그것이 바로 이 프로젝트가 막으려는 시나리오이므로 감수한다.

---

### ADR-009. Phase 0~2의 e2e 드라이버 → 시리얼 + QMP, 이어서 최소 Agent

**문제**: 9.2는 e2e 드라이버로 buntOS 자체의 Agent Control Plane을 쓴다고 정했지만(도그푸딩), Agent Control Plane은 Phase 3에 나온다. 그런데 Phase 0 종료 조건(e2e 1번: 부팅 → 셸)과 Phase 1 종료 조건(터미널 + 한글 입력 e2e)이 그보다 먼저 e2e를 요구한다. 순서가 맞지 않는다.

**선택지**

| | 내용 | 장단점 |
|---|---|---|
| A | 임시 드라이버: QEMU 시리얼 콘솔(로그인, 명령 실행) + QMP(`input-send-event`로 키 입력, `screendump`) | 빠르게 만들 수 있다. 한글 입력 검증(텍스트 필드 값 읽기)에는 테스트 전용 경로가 필요해져 9.1 원칙과 충돌한다 |
| B | Agent Control Plane 최소판을 Phase 1로 앞당긴다: `buntd-agent` JSON-RPC, 실행 경로, 읽기 경로(AT-SPI로 포커스 요소 값 읽기)만 | 제품 경로를 일찍 검증한다. Phase 1 작업량이 늘어난다 |
| C | openQA 같은 외부 화면 매칭 프레임워크 | 9.2 원칙 위반이고 유지비가 크다. 탈락 |

**결정 (2026-10-27): Phase 0은 A, Phase 1은 B.**
- 부팅 → 셸 검증은 시리얼로 충분하다.
- 한글 입력 e2e는 키 입력을 QMP로 넣고(가상 키보드 → 컴포지터 → fcitx5 → 앱으로 이어지는 실제 사용자 입력 경로), 값 확인을 최소 Agent의 AT-SPI 읽기로 한다.
- 시나리오는 처음부터 드라이버에 독립적인 형식(단계 = {동작, 기대값})으로 써서, Phase 3에는 드라이버만 교체한다.
- 공개 프로젝트 관점: 기여자는 QEMU만 있으면 e2e를 로컬에서 재현할 수 있다.

---

### ADR-010. 빌드·CI 인프라 → GitHub 공개 저장소 + 호스티드 러너 중심

**사실관계 (v0.3 갱신)**
- 개발 머신은 Apple M4(arm64)다 [확인: 로컬 조회]. macOS 15 이상, M3 이상 칩에서는 중첩 가상화가 지원되고, UTM 4.6이 Linux 게스트에서 이를 지원한다 [확인 S17].
- mkosi는 Linux에서 실행된다. Mac에서는 Linux VM이 필요하다.
- GitHub 호스티드 Linux x86-64 러너는 2024-04부터 표준(2 vCPU) 러너에서도 KVM 하드웨어 가속을 쓸 수 있다. 러너 사용자를 kvm 그룹에 넣는 udev 규칙이 필요하다 [확인 S34].
- arm64 호스티드 러너(`ubuntu-24.04-arm`)는 2025-08-07 공개 저장소용으로 정식 출시됐고 무료다. 4 vCPU, 16GB RAM, SSD 14GB다 [확인 S35]. `/dev/kvm` 제공 여부는 [미확인 S18].
- GitHub 문서는 self-hosted 러너를 비공개 저장소에서만 쓰라고 권한다. 공개 저장소에서는 포크 PR이 러너 머신에서 위험한 코드를 실행할 수 있기 때문이다 [확인 S36].
- 가속 없이 aarch64 QEMU(TCG)를 돌리면 수 배에서 수십 배 느려 e2e 시간 예산을 넘기 쉽다 [추정].

**선택지 (v0.3)**

| | 내용 | 판단 |
|---|---|---|
| A | 전부 GitHub 호스티드 러너 | arm64 러너에 KVM이 있으면 최선 |
| B | PR용 aarch64 e2e를 공개 저장소에 연결한 M4 Mac self-hosted 러너로 돌린다 (v0.2 권고) | GitHub 권고 위반. 포크 PR 코드가 개인 Mac에서 실행된다 → **철회** |
| C | 호스티드 러너 + 리뷰를 마친 코드에 한해 Mac에서 가속 실행 (비공개 미러 저장소의 self-hosted 러너 또는 메인테이너 로컬 실행) | arm64 러너에 KVM이 없을 때의 보완책 |

**결정 (2026-10-27): 저장소는 GitHub 공개 저장소(`blackmallangcow/buntOS`)로 한다. CI는 A를 기본으로 하고, arm64 러너에 KVM이 없으면 C로 보완한다.**
- x86-64 e2e: 호스티드 Linux 러너에서 KVM 가속으로 전체 시나리오를 돌린다.
- aarch64 e2e: Phase 0 첫 주에 `ubuntu-24.04-arm`에서 `ls -l /dev/kvm`을 실험해 확정한다.
  - KVM 있음 → 호스티드 arm64 러너에서 전체 시나리오 (A)
  - KVM 없음 → PR에서는 TCG로 핵심 시나리오를 돌린다. 가속이 필요한 전체 시나리오는 리뷰를 마친 코드에 한해 Mac에서 실행하고, 그 결과를 병합 전 확인 항목으로 둔다. 이렇게 9.1의 "main 병합 전 e2e 필수"를 지킨다 (C)
- 공개 저장소에는 self-hosted 러너를 연결하지 않는다.
- 공개 시점: 실기기(TB375FC) 시험을 마친 뒤(2026-10-30 결정. 처음에는 1차 빌드 성공 후였다). 비공개 기간에는 arm64 호스티드 러너의 무료 제공(공개 저장소 한정)을 쓸 수 없으므로, aarch64 빌드·e2e는 로컬 개발 VM(`tools/devvm/`)에서 돌린다.
- 호스티드 러너의 디스크 제약(arm64 러너 기준 SSD 14GB)에 맞춰 이미지 빌드 캐시와 산출물 크기를 관리한다.
- 로컬 개발: 별도 x86 Linux 머신이 없으면 로컬 1차 개발은 M4 Mac 위 Linux VM(Lima, `tools/devvm/`)에서 aarch64로 하고, 이미지는 VM 안에서 중첩 KVM으로 띄운다(2026-10-27 검증, [개발 환경](../dev/environment.md)). x86-64는 CI에서 KVM으로 검증한다(F7). x86 Linux 머신 보유 여부는 아직 확인하지 않았다.

---

### ADR-011. 실기기 선택과 언락 진행 기준 → TB375FC + 예비 1대

v0.3.9: ADR-013으로 이 결정은 태블릿 라인(포크 `buntos-TB375FC`)의 결정이 됐다. 이어지는 판단은 포크의 설계안(`docs/tb375fc/DESIGN.md` 2.1절)에 적는다.

**사실관계**
- TB375FC에 대한 공식 언락은 찾지 못했다. CN 펌웨어 ZUI 16.0.20.091에서 언락과 AVB 해제가 됐다는 실사용 보고는 있다 [확인 S7].
- 기획서가 인용한 XDA 비공식 방법(LK 교체, CN ROM ZUI 16 조건)은 원문 접근이 막혀(403) 이번에 재확인하지 못했다.
  - 2026-11-01에 브라우저로 원문과 글타래를 확인했다 [확인 S41, 포럼 보고]. 중국판 ZUI 16.0.20.022에서 순정 LK가 `fastboot flash lk`를 받아, 개발용 펌웨어 덤프의 LK를 구운 뒤 `fastboot flashing unlock_critical`로 언락한다. 다른 사용자 여럿이 .022·.041에서 성공했다.
  - 16.0.20.091에서는 성공 보고(2025-09, 2026-09)와 실패 보고(2025-12)가 함께 있어, 판과 기기에 따라 다르다. ZUXOS(Android 16) 기기는 16.0.20.091로 내린 뒤 언락한 보고가 있다(2026-09). 개발용 LK는 4pda에 올라온 덤프에서 나와 출처를 확인할 수 없다.
  - 백업과 복구는 순정 펌웨어 묶음의 인증 DA·auth와 SP Flash Tool로 한 보고가 여럿이다. mtkclient는 MT6897에 BROM 익스플로잇이 없다.
- 벽돌 위험이 있고, `nvram` 계열 파티션을 잃으면 복구할 수 없다(ARCHITECTURE 4.2.3).
- mainline 단일 전략에서 이 기기의 포팅 부담: MT6897 포팅 선례 없음, UART 없음, Wi-Fi 방식 미확인 (ARCHITECTURE 4.2.1).
- EL2 점유 가능성(F8): 점유가 확인되면 이 기기는 OS를 아무리 열어도 가상화를 제공할 수 없다. D9를 기기 자체가 영구히 제한하게 된다.
- 같은 SoC의 TB373FU와 Moto Pad 60 Pro가 있다 [확인 S7]. Motorola 공식 언락 프로그램이 Moto Pad 60 Pro를 지원하는지, 하드웨어가 TB375FC와 같은지는 [미확인].
- 2026-11-01 펌웨어로 확인 (`docs/hw/lenovo-tb375fc.md`):
  - TB375FC 중국판(ZUI 16.0.20.091)과 TB373FU 글로벌판(ZUI 17.0.04.266)의 장치 트리가 같다. 차이는 지역 표시와 Wi-Fi 앞단 부품 조합 표뿐이다.
  - Wi-Fi는 SoC 통합 connsys다.
  - GenieZone 이미지와 노드가 있어 EL2 점유 가능성이 높다(예비).
- 가격 참고: 2025-12 유럽 할인가 168€ (8/128GB) 사례가 있다 [확인 S19].

**선택지**

| | 내용 | 위험 |
|---|---|---|
| A | TB375FC 1대로 포팅 진행 | 벽돌이 되면 Phase 2가 멈춘다 |
| B | TB375FC + 같은 모델 예비 1대 | 비용 발생 |
| C | mainline 지원이 이미 있는 SoC의 다른 arm64 기기로 변경 | 후보 조사가 필요하다(아직 조사하지 않음). 태블릿 폼팩터·성능 조건을 다시 따져야 한다 |

**결정 (2026-10-27): B. TB375FC 2대(개발용 1, 예비 1)로 진행한다.**
- ARCHITECTURE 12.1절 판정이 "불가(EL2 점유)"로 나오면 C를 본격 검토한다.
  - 2026-11-03 판정: 불가. 2a 첫 부팅에서 커널이 EL1로 시작했다.
  - 2026-11-04 사람의 결정: C를 검토하지 않고 TB375FC로 그대로 간다. 일반 개발 환경에는 EL1로 충분하다. 다른 태블릿을 매입하면 그때 사람이 말해서 KVM을 다시 시도한다.
- 글로벌판 TB373FU와 하드웨어가 같은지 Phase 0에서 확인한다(글로벌 펌웨어의 벤더 DTB 비교). 같으면 BSP 하나로 두 모델을 지원해 기여자와 테스터를 넓힌다. 글로벌판이 중국 내수판보다 구하기 쉬울 것으로 본다 [추정].
- 예산은 별도로 정한다.

언락 진행 조건(go/no-go, ARCHITECTURE 4.2.3에 반영)
1. 전체 파티션 백업을 끝내고, 백업을 다른 기기에 옮겨 해시로 무결성을 확인했다.
2. 언락 절차를 서로 독립된 보고 2개 이상으로 교차 확인했고, 펌웨어 버전이 일치한다.
3. BROM/preloader 모드 복구 경로(mtkclient 등)의 MT6897 지원을 미리 확인했다.
4. 펌웨어 자동 업데이트(OTA)를 껐다. 언락 조건이 특정 ZUI 버전에 묶여 있으므로 **지금 바로** 끈다.

---

### ADR-012. 재배포 불가 펌웨어 → 허용, 공개 이미지에는 미포함

**배경**: ARCHITECTURE 2.1절은 펌웨어를 개방성의 예외로 두되 "오픈소스 > 재배포 가능한 바이너리 > 기기에서 추출하는 재배포 불가 바이너리" 순으로 선호한다. 마지막 단계를 허용할지가 이 질문이다.

**사실관계**
- MT6897의 Wi-Fi/BT, 전원 관리 MCU, GPU 펌웨어 가운데 무엇이 재배포 가능한 형태(linux-firmware 등)로 있는지 [미확인]. 목록은 Phase 0 실측(ARCHITECTURE 12장 펌웨어 목록)으로 만든다.
- Panthor는 GPU 펌웨어 `mali_csffw.bin`을 요구한다. G610용은 linux-firmware로 배포된다 [확인 S33]. G615용(arch 11.8) 배포 여부는 [미확인].
- v0.1도 벤더 블롭을 "재배포 불가, 설치 시 기기에서 추출"로 다루는 방식을 전제했었다.
- buntOS는 오픈소스로 공개하므로 이미지도 공개 배포된다. 재배포 불가 펌웨어를 공개 이미지에 넣는 것 자체가 재배포다.

**선택지**

| | 내용 | 결과 |
|---|---|---|
| A | 허용. 이미지에는 넣지 않고 설치 시 기기에서 추출한다 | 해당 하드웨어가 동작할 수 있다 |
| B | 불허 | 재배포 가능한 펌웨어가 없는 하드웨어는 미지원 |
| C | 기능별로 판단 | 결정이 잦아지고 기준이 흐려진다 |

**결정 (2026-10-27): A.** 펌웨어는 2.1절 정의상 AP 코드가 아니므로 개방성 정의와 충돌하지 않는다. B를 택하면 이 기기에서 Wi-Fi 같은 핵심 기능이 동작하지 않을 가능성이 높다 [추정].

규칙 (ARCHITECTURE 2.1절에 반영)
- 공개 이미지와 저장소에 절대 포함하지 않는다.
- 설치 시 사용자 자신의 기기에서만 추출한다. 제3자 미러에서 받지 않는다.
- 대상 파일과 해시는 `board.toml`에 선언하고, 추출 스크립트는 오픈소스로 둔다.
- 추출에 실패하면 해당 기능만 비활성화되고 OS는 정상 부팅해야 한다.
- 재배포 가능한 판이 나오면 그쪽으로 교체한다.

---

### ADR-013. 제품 라인 → 데스크톱(이 저장소)과 태블릿(arm64 포크)

**사실관계**
- 이 저장소는 x86-64와 arm64 이미지를 같은 레시피로 빌드하고, 양쪽 QEMU에서 같은 e2e를 통과한다(ARCHITECTURE 11장).
- 첫 실기기 TB375FC는 SoC 지원이 업스트림에 없어 기기 쪽 일이 크다: 커널 커스텀, 장치 트리, 부트 이미지, 펌웨어, 언락. 2026-11-01에 이 일을 git 포크 `buntos-TB375FC`로 옮겨 진행했다.
- 기기 쪽 일은 기기마다 다르고, 공용 데스크톱 컴포넌트와 고치는 주기가 다르다.

**결정 (2026-11-01, 사람이 결정)**
- **데스크톱 라인 = 이 저장소(buntOS).** x86-64(Intel·AMD 등 기존 PC 아키텍처)와 arm64의 전통 데스크톱·노트북을 대상으로 하고, GPU 가속을 쓴다.
- **태블릿 라인 = 이 저장소의 arm64를 포크한 저장소.** 기기마다 포크 하나를 두고 그 기기에 맞춰 최적화한다. 첫 포크는 `buntos-TB375FC`(TB375FC·TB373FU)다.

**결과** (ARCHITECTURE 1.1절에 반영)
- 공용 컴포넌트(컴포지터, 셸, 시스템 설정, 로그인 화면, 에이전트, 이미지 레시피의 공통부)는 이 저장소가 기준이고, 포크가 받아 합친다. 공용 고침은 이 저장소에 먼저 낸다.
- 태블릿 포크는 arm64만 빌드한다. 동등성(D3)은 이 저장소의 x86-64와 arm64 사이에서 판정한다.
- 터치 입력과 화면 키보드 같은 공용 기능은 이 저장소에 둔다(터치 화면 노트북과 투인원). 기기마다 맞추는 것은 포크에서 한다.
- ADR-011(TB375FC)은 태블릿 라인의 결정이 된다. 이 저장소의 Phase 2는 데스크톱 실기기로 바꾼다(ARCHITECTURE 11장).
- 공개 시점(0.3절)은 라인에 맞춰 다시 정한다.

### ADR-014. 앱 설치 → 자체 패키지 매니저와 GNOME Software + Flathub

**사실관계**
- 사람의 요청(2026-11-02): "인터넷에서 리눅스 설치파일을 다운받아도 설치매니저가 없으니 커맨드를 써야해서 불편하다. 패키지 매니저라는 명칭으로 사용하여 설치를 쉽게 하고 리눅스 앱스토어 같은것도 있으면 추가해둬라."
- **[확인]** 이미지에는 apt만 있었다. 받은 파일을 두 번 누르면 열 앱이 없었다(mimeapps.list).
- **[실측]** 이미지가 고정한 Debian forky 스냅숏(`20261027T083111Z`)에 PackageKit 1.4.0, flatpak 1.18.4, GNOME Software 51.0(플러그인 `-deb`, `-flatpak`), Bazaar 0.9.4가 arm64로 있다. Debian 꾸러미 색인을 읽어 확인했다.
- **[실측]** 같은 스냅숏에 `libfuse2`가 없다(`libfuse3-4`와 `fuse3`만 있다). 실제 AppImage(Obsidian 1.14.5 arm64)의 런타임에 `libfuse.so.2` 문자열이 있고 이미지에 그 라이브러리가 없다. AppImage 문서는 FUSE가 없으면 풀어서 실행하라고 안내한다(S43).
- **[확인]** GNOME Software는 PackageKit(Debian 패키지), Flatpak, Snap, 펌웨어 플러그인을 갖고 앱과 OS 업데이트까지 맡는다(S44). 한국어 번역이 있고 Debian main에 있다.
- **[확인]** Bazaar는 Flathub와 Flatpak 원격 전용 스토어다. Flathub의 Debian 안내가 그래픽 스토어로 권하지만 Debian 꾸러미는 공식 지원이 아니라고 스스로 적는다(S45, S46). PackageKit을 쓰지 않아 Debian 패키지와 시스템 업데이트를 맡지 못한다. Debian의 판은 0.9.4다.

**선택지**

앱 스토어:

| 선택지 | 좋은 점 | 나쁜 점 | 판단 |
|---|---|---|---|
| GNOME Software + Flathub | Debian과 Flathub를 한곳에서 찾고 업데이트까지 한다. Debian main, 한국어 번역, GTK4 | 한 번 열면 세션 끝까지 약 190MB 상주, 모양이 macOS App Store와 다르다 | **채택** |
| Bazaar | 빠르고 Flathub가 권한다 | Flatpak 전용, Debian 꾸러미 비공식, WebKit 의존 | 나중에 사람이 고를 수 있다 |
| KDE Discover | 기능이 비슷하다 | Qt·KDE 프레임워크를 끌어온다 | 제외 |
| 자체 스토어 | macOS App Store 모양 | Flathub API와 카탈로그, 평점, 업데이트를 새로 만들어야 하고 지금 요청에 비해 크다 | 지금은 하지 않는다 |

설치 파일:

| 선택지 | 좋은 점 | 나쁜 점 | 판단 |
|---|---|---|---|
| 자체 앱 `bunt-packages` | macOS 설치 프로그램의 흐름, "패키지 매니저"라는 이름, AppImage까지, 뜨는 데 1초 안쪽 | 코드를 지켜야 한다 | **채택** |
| GNOME Software의 로컬 파일 열기 | 코드 없음 | 이름과 흐름을 맞출 수 없고 AppImage를 못 다루며, 뜨는 데 카탈로그를 읽는다 | 제외 |
| gdebi | 오래된 .deb 설치기 | GTK3, Python, .deb만 | 제외 |

뒷단:
- **.deb는 PackageKit.** 자체 root 도우미(pkexec + apt-get)는 입력 검증과 잠금을 스스로 맡아야 한다. PackageKit은 polkit 인증, 진행 상황, apt 잠금 줄서기, 미리 해 보기(`SIMULATE`)를 D-Bus로 주고 앱 스토어와 같은 서비스다.
- **Flatpak은 flatpak 명령.** libflatpak 바인딩은 빌드 의존성(`libflatpak-dev`)과 크레이트를 더한다. 명령의 진행 출력으로 백분율을 읽을 수 있음을 시험했다.
- **AppImage는 풀어서 설치.** ~/Applications/<이름>에 unsquashfs로 푼다. 받은 파일을 그대로 실행하면 libfuse2가 없어 옛 런타임이 뜨지 않을 것이고(**[추정]** 문서와 런타임의 문자열로 판단했고 직접 실행해 보지는 않았다), 런타임을 설치 때 실행해야 메타데이터를 읽을 수 있다. 푸는 길은 FUSE가 필요 없고 받은 코드를 실행하지 않는다. 대가는 디스크(Obsidian 140MB → 365MB)와 앱의 자체 업데이트(`$APPIMAGE`)가 꺼지는 것이다.

**결정 (2026-11-03)**
- 이름은 사람이 정한 "패키지 매니저"다. 설치 파일 형식은 .deb, AppImage(2형 squashfs), .flatpak, .flatpakref, .flatpakrepo다. rpm과 snap은 까닭을 알린다.
- 앱 스토어는 GNOME Software와 Flathub이고 모든 앱에는 "앱 스토어"로 보인다. Flathub는 이미지가 넣는다(사용자가 지우면 다시 생기지 않는다). 업데이트 자동 내려받기는 끈다.
- 이미지에 든 패키지는 목록에 없고 지우지 못한다(`/usr/share/buntos/base-packages`).
- 패키지 설치는 매번 관리자 인증을 받는다. PackageKit의 `auth_admin_keep`을 규칙으로 `auth_admin`으로 바꾼다(ARCHITECTURE 6.5절). Flatpak은 샌드박스이고 `--user`로는 어차피 인증 없이 설치되므로 flatpak의 기본 규칙(관리자는 암호 없음)을 둔다.

**결과** (ARCHITECTURE 5.5절, 디자인 스펙 16장)
- 이미지에 PackageKit, flatpak, GNOME Software가 들어간다. 설치 크기는 ADR-014를 넣기 전과 견줘 측정해 ARCHITECTURE 11장에 적는다.
- Flathub에는 자유 소프트웨어가 아닌 앱도 있다. D8의 개방성은 OS 자체의 코드에 대한 것이고 사용자가 고른 앱은 D9(막지 않는다)에 든다.
- **[실측]** 이미지의 apt는 live forky를 본다(mkosi가 스냅숏을 이미지의 sources에 적지 않는다). 받은 .deb의 의존성을 풀 때 이미지보다 새 패키지가 들어올 수 있다. forky 정식 릴리스 전의 보안 지원 없음(ADR-004)과 같은 조건이다.

---

## 4. 실측으로 결정할 항목 (기준과 잠정 가설)

### ADR-001. 시스템 앱 UI 툴킷: GTK4 vs Qt6

**사전 근거**

| 기준 | GTK4 | Qt6 |
|---|---|---|
| Rust 바인딩 | gtk4-rs 0.11.5 (MIT, 2026-09 갱신, MSRV 1.92) [확인 S10]. Rust GUI 중 가장 성숙한 축 [추정] | cxx-qt 0.10 (KDAB, MIT/Apache-2.0). QML 중심이고 QWidgets 바인딩은 없다 [확인 S10, S20] |
| 접근성 구조 | GTK4에서 ATK를 걷어내고 ARIA식 역할·상태를 AT-SPI로 직접 낸다 [확인 S21]. 4.14에 GtkAccessibleText, 4.18에 AccessKit 백엔드가 병합됐다(Linux 기본은 여전히 AT-SPI) [확인 S22]. 커스텀 위젯에서 Selection 인터페이스를 지원할 API가 없다는 이슈가 있었다(과거, 현재 상태는 실측) [확인 S23] | QAccessible에서 AT-SPI 브리지로 낸다. KDE에서 오래 써왔다. 최근 품질 비교 자료는 찾지 못했다 → 실측 |
| Wayland 한글 입력 | text-input-v3를 네이티브로 지원 [확인 S11] | 6.7부터 text-input-v3, 6.8.2 이상 권장 [확인 S11]. trixie는 딱 6.8.2, forky는 6.11 |
| 라이선스 | LGPL-2.1+ | LGPL-3.0 / GPL / 상용 |
| macOS형 외관 | libadwaita를 쓰면 GNOME HIG에 묶인다. 쓰지 않고 자체 CSS 테마를 만들어야 한다 | QML로 자유롭게 디자인하기 쉽다 |
| 메모리 | [실측] | [실측] |

**제3의 선택지(Rust 네이티브: iced/libcosmic, Slint)를 이번 비교에서 빼는 이유**: 접근성이 AccessKit을 거쳐 AT-SPI로 나가는 구조인데, COSMIC은 2024년 알파 시점에 자체 앱의 스크린 리더 지원이 아직 안 됐다 [확인 S24]. D5와 제3 목표에서 접근성 트리가 AI의 입력이 되므로 성숙도 리스크가 크다. Slint는 라이선스 제약도 있다 [확인 S10].

**진행 (2026-10-28)**: bunt-shell v0(메뉴바, Dock)을 GTK4 + gtk4-layer-shell로 만들었다. 시스템 앱으로는 bunt-settings v0(시스템 설정)을 잠정안대로 libadwaita 없이 GTK4와 자체 CSS로 만들었다. 목록, 텍스트 입력, 드롭다운(검색 포함), 토글, 스위치가 들어 있어 측정 프로토콜의 GTK4 쪽 시제품으로 쓸 수 있다. 기본 앱으로 넣은 GNOME 앱들은 libadwaita를 쓰지만, 사용자가 바꿀 수 있는 외부 앱이므로 이 결정의 범위 밖이다. Qt6 시제품과 M1~M4 측정은 아직 하지 않았으므로 결정은 그대로 열려 있다.

**진행 (2026-10-28, macOS 26형 개편)**
- 셸과 시스템 설정을 macOS 26 모양으로 다시 만들었다.
  - 셸: 메뉴 막대 항목, 제어 센터, 모든 앱 패널
  - 시스템 설정: 사이드바 검색, 묶음 상자, 시트, 12개 페이지
- 두 앱은 같은 CSS 변수 파일(`theme/tokens.css`)을 읽고, 규칙은 [디자인 스펙](DESIGN-SPEC.md)에 모았다. libadwaita 없이 GTK4 CSS(4.16 이상의 변수, `color-mix()`)만으로 macOS형 외관을 낼 수 있음을 확인한 셈이다.
- 한글 입력기(fcitx5-hangul)와 bunt-comp의 text-input-v3·input-method-v2가 들어가 M2(한글 IME) 측정의 전제가 갖춰졌다.
- 측정은 아직 하지 않았다.

**범위 추가 제안**: 기획서는 `bunt-shell`(메뉴바, Dock)의 UI 구현 수단을 정하지 않았다. 에이전트는 셸도 조작해야 하므로 ADR-001 범위에 셸을 포함시켜 접근성 노출을 하나로 맞추는 것을 권한다. GTK4 쪽은 gtk4-layer-shell, Qt 쪽은 layer-shell-qt가 있다.

**측정 프로토콜 (지금 고정할 것)**
- 시제품: 같은 기능의 "Files 축소판" 두 벌. 리스트/트리 뷰, 텍스트 필드, 메뉴, 모달 다이얼로그, 토글을 포함한다.
- M1 접근성: 상호작용 요소 중 역할·이름·상태·동작이 올바르게 노출된 비율 / 한↔영 전환 후 accessible id 유지율 / 압축 스냅샷 크기 / 전체 트리 수집 시간
- M2 한글 IME (fcitx5-hangul, bunt-comp 위 text-input-v3): 조합 중 포커스 이동이나 클릭 시 확정 동작 / 자모 단위 백스페이스 / 조합 중 Enter·Tab / 한자 변환 / 후보창 위치 / 화면 키보드 두벌식
- M3 메모리: 기동 직후 PSS·USS, 10분 유휴 후, 창 5개
- M4 개발 생산성: 같은 기능 구현에 든 시간과 코드 양
- 가중치 제안: M1 40 / M2 30 / M3 15 / M4 15. 동점이면 Rust 친화성이 높은 쪽(GTK4)
- 참고: 에이전트가 실제로 다룰 앱의 상당수는 Chromium/Electron 계열이다. 이들은 보조 기술이 활성화된 경우에만 접근성 트리를 내는 것으로 알려져 있다 [추정]. 툴킷 결정과 별개로 Phase 3에서 따로 측정한다.

**잠정 가설: GTK4** (Rust 바인딩 성숙도, 네이티브 text-input-v3, LGPL-2.1). 도전자는 Qt6 QML (디자인 자유도).

> 결정 기록 (Phase 1): ☐ GTK4  ☐ Qt6 / 셸 포함: ☐ 예 ☐ 아니오 / 측정 결과 링크:

---

### ADR-003. x86 바이너리 호환: FEX-Emu vs box64

| 기준 | FEX-Emu | box64 |
|---|---|---|
| Debian 패키지 | 없음. Ubuntu PPA, Fedora 통합 제안 [확인 S9, S28] | main에 있음 (trixie 0.3.4 / forky 0.4.3) [확인 S8] |
| 32비트 x86 | 지원 [확인 S28] | Fedora 문서 기준 미지원 (최신 box32 모드는 확인 필요) [확인 S28] |
| 요구 사항 | ARMv8.0 이상, 4K 페이지 커널, x86-64 RootFS 필요 [확인 S9, S28] | 호스트의 네이티브 라이브러리를 감싸서 쓴다 |
| 성능 | Fedora 평가: 둘이 비슷하고 둘 다 개선 중 [확인 S28] | 같음 |
| 지속성 | Valve가 후원하고 Steam Frame에 쓰인다 [확인 S29, 2차 보도] | 커뮤니티 주도. 2026년에도 릴리스가 이어진다 |
| 운영 부담 | RootFS 이미지 관리(디스크, 업데이트) | 적음 |
| 라이선스 | MIT | MIT |

함께 정해야 할 것: binfmt_misc에서 x86-64 ELF 핸들러는 하나만 등록된다. 컨테이너용 qemu-user-binfmt와 FEX/box64 중 누가 이 자리를 가질지 정책이 필요하다. 또 이 에뮬레이터 패키지는 arm64에만 있으므로 동등성 리포트에 예외로 등록해야 한다.

벤치마크 워크로드 제안: 안드로이드 SDK build-tools(aapt2 등은 Linux arm64 공식 바이너리가 없다고 알려져 있다 [추정]) / x86 전용 Electron 앱 / Node 네이티브 애드온 prebuilt / x86 JDK + Gradle / `podman run --arch amd64` 이미지 빌드

**잠정 가설: box64 기본, FEX와 비교 벤치마크.** FEX가 대표 워크로드에서 확실히 빠르거나(예: 20% 이상) 32비트가 필요한 워크로드가 있으면 FEX로 가고 자체 패키징 비용을 감수한다.

> 결정 기록 (Phase 5): ☐ FEX  ☐ box64 / binfmt 정책:

---

### ADR-005. UI 폰트

| 기준 | Pretendard | Noto Sans CJK KR |
|---|---|---|
| Debian | `fonts-pretendard` 1.3.9 (trixie main). 패키지 6.1MB / 설치 13.8MB [확인 S8] | `fonts-noto-cjk`. 55.3MB / 91.2MB (한중일 전체와 Mono 포함) [확인 S8] |
| 라이선스 | OFL-1.1 | OFL-1.1 |
| 글자 범위 | 한글과 라틴 중심. 한자·일본어·중국어 폴백이 따로 필요하다 [추정] | 한중일 전체 |
| 디자인 | 라틴은 Inter, 한글은 본고딕 계열을 맞춘 시스템 UI 대체용 폰트다. macOS형 UI 지향과 잘 맞는다 | 본고딕 계열 |
| 고정폭 | 없음 | Noto Sans Mono CJK KR (한글 2:1 폭 정합) |

- 개발 머신에서 웹을 보려면 한중일 폴백이 필요하므로 Noto CJK는 어느 경우든 들어간다. 그래서 실제 선택지는 (A) Pretendard + Noto CJK 폴백, (B) Noto CJK 단일이고, A의 추가 비용은 이미지 약 14MB다.
- 고정폭: Noto Sans Mono CJK KR(이미 포함)을 기본으로 한다. D2Coding은 Debian에 없어 자체 패키징이 필요하다 [확인 S8]. JetBrains Mono는 Debian에 있지만 한글이 없어 폭이 어긋난다.

**권고: A.** 렌더링과 메모리 실측은 Phase 4에 하되, 나중에 바꾸기 쉬우니 결정은 지금 해도 비용이 거의 없다.

> 결정 기록: ☐ A  ☐ B / 고정폭:

---

## 5. v0.2에서 결정·폐기된 항목

### ADR-002 (결정됨). arm64 실기기 GPU 경로 → Panthor + Mesa

- **결정 근거**: mainline 단일 전략과 D8(개방성).
- **v0.1 후보의 처리**
  - (1) pixman 소프트웨어 렌더: GPU가 동작하기 전의 대체 렌더러로 유지한다 (bring-up 2a~2d).
  - (2) Mesa Panfrost-on-kbase: 업스트림 Mesa에는 kbase 백엔드가 없다. 커널 인터페이스 백엔드는 `panfrost`, `panthor`, `virtio`뿐이다 [확인: Mesa 소스 트리 조회]. kbase는 업스트림 밖 벤더 커널 드라이버여서 D8에 맞지 않는다. v0.1이 언급한 panfork의 G615 동작 보고는 이번에 재확인하지 못했다. 제외.
  - (3) libhybris + 벤더 Mali 블롭: bionic과 폐쇄 드라이버를 들여오므로 제외 (2장).
- **업스트림 지원**: Mesa가 G615를 v11로 지원한다(GLES 3.1, Vulkan 1.4, OpenCL 3.0) [확인 S1]. v11 커밋은 Mesa 25.1부터 [확인 S2]. Panthor의 Gx15 지원은 Linux 6.18부터 [확인 S3].
- **남은 확인**
  - MT6897 쪽 GPU 전원·클럭 통합 방식: 벤더 DTB로 범위를 파악한다 (ARCHITECTURE 12장)
  - G615용 GPU 펌웨어(arch 11.8)의 배포 형태: ADR-012와 연결
  - bring-up 중 pixman의 네이티브 해상도 프레임 시간

### ADR-007 (폐기). Track A 커널 빌드 방식

구 Track A 폐기로 전제가 사라졌다. 조사 결과(출하 GKI 설정 공백)는 2.3절에 폐기 근거로 남긴다.

### ADR-008 (폐기). BSP 경계 규칙 완화

완화가 필요했던 이유가 libhybris 경로였으므로 함께 폐기한다. BSP 경계 규칙은 v0.1보다 오히려 엄격해졌다: BSP 커널도 mainline 기반이어야 하고 벤더 커널·폐쇄 모듈은 허용하지 않는다 (ARCHITECTURE 3장, D8).

---

## 6. ARCHITECTURE.md 반영 내역

### v0.3
- 머리말: 개정 이력 정리
- 2장: D2에 Debian 기준 브랜치(ADR-004) 추가, D10 라이선스(ADR-006) 신설, 2.1절에 재배포 불가 펌웨어 규칙(ADR-012) 추가
- 4.1: 로컬 1차 개발 플랫폼 규칙(ADR-010)
- 4.2: 실기기 확보(ADR-011), 4.2.2 커널 계열 일치 문구, 4.2.3에 언락 진행 조건(go/no-go)
- 9.2: e2e 드라이버 단계(ADR-009). 9.4 CI 구성(ADR-010) 신설
- 10장: `LICENSES/` 추가, 패키지 미러 고정·Rust 툴체인·라이선스 표기·DCO 규칙
- 11장: Phase 1에 최소 Agent 추가
- 12장: 글로벌판 동일성 확인 항목 추가
- 13장: 확정 항목을 "결정됨" 표로 이동. 남은 미결정은 ADR-001·003·005
- 14장: 참고 자료 추가

### v0.2
- 머리말: v0.2 개정 기록
- 1장: 타협하지 않는 두 목표(완전히 열린 스택, 개발환경 비차단) 명시
- 2장: D8(개방성)·D9(개발환경 비차단) 추가, 2.1절 개방성 정의 신설
- 3장: BSP 경계 규칙에 mainline 커널 요건과 펌웨어 예외 규칙 추가
- 4.1: 1차 개발 플랫폼 문구에 ADR-010 재검토 표시
- 4.2: 같은 SoC 변형 추가. 4.2.1 리스크 갱신(G615 업스트림 지원, Wi-Fi, 디버그 채널, EL2, 커널 소스). 4.2.2를 mainline 단일 전략과 bring-up 단계(2a~2e)로 교체. 4.2.3에 OTA 차단과 단계별 디버그 채널 추가
- 4.4: 가상화(KVM) 항목 추가
- 9.2·9.3: ADR-009 참조, 실기기 스모크 테스트에 가상화 추가
- 10장: `docs/hw/` 추가, BSP·kernel 디렉터리 설명 갱신
- 11장: Phase 2를 mainline bring-up으로 교체, "장기 Track B" 행 삭제, Phase 3~5와의 병행 명시
- 12장: 포팅 정보 수집 중심으로 재구성(벤더 DTB, 펌웨어 목록 등), 12.1절 EL2/KVM 판정 신설
- 13장: ADR-009~012 추가, 포팅 전략과 ADR-002를 결정됨으로 이동
- 14장: 참고 자료 추가

---

## 7. 출처

조회일은 모두 2026-10-27이다.

- S1. Mesa Panfrost 문서 (지원 GPU 표): https://docs.mesa3d.org/drivers/panfrost.html
- S2. Mesa 25.1.0 릴리스 노트: https://docs.mesa3d.org/relnotes/25.1.0.html
- S3. Phoronix, Panthor Linux 6.18 (2025-08-21): https://www.phoronix.com/news/Panthor-More-Arm-Mali-Linux-618 / Gx15 패치: https://lkml.rescloud.iu.edu/2506.0/01349.html
- S4. ACK android14-6.1 `gki_defconfig`: https://android.googlesource.com/kernel/common/+/refs/heads/android14-6.1/arch/arm64/configs/gki_defconfig
- S5. systemd README (커널 요구 사항): https://github.com/systemd/systemd/blob/main/README
- S6. Android 공통 커널 지원 기간: https://source.android.com/docs/core/architecture/kernel/android-common
- S7. TrebleDroid 위키 (TB375FC / TB373FU / Moto Pad 60 Pro): https://github-wiki-see.page/m/TrebleDroid/treble_experimentations/wiki/Lenovo-Idea-Tab-Pro---Lenovo-Xiaoxin-Pad-Pro-2025---Motorola-Moto-Pad-60-Pro
- S8. Debian 패키지 버전 (madison): https://qa.debian.org/madison.php , 패키지 크기: https://packages.debian.org/trixie/fonts-pretendard , https://packages.debian.org/trixie/fonts-noto-cjk
- S9. FEX-Emu: https://github.com/FEX-Emu/FEX
- S10. crates.io API (smithay, gtk4, cxx-qt, iced, slint, accesskit, fluent-bundle): https://crates.io/
- S11. fcitx5 Wayland 가이드: https://fcitx-im.org/wiki/Using_Fcitx_5_on_Wayland
- S12. Debian LTS 일정: https://wiki.debian.org/LTS
- S13. Debian 릴리스 이력: https://en.wikipedia.org/wiki/Debian_release_version_history / forky 아트워크 공모: https://www.linuxcompatible.org/story/debian-14-forky-artwork-call-opens-submit-your-theme-by-nov-26/
- S14. niri 라이선스 (Arch 패키지): https://www.archlinux.org/packages/extra/x86_64/niri/
- S15. COSMIC: https://en.wikipedia.org/wiki/COSMIC_desktop , https://github.com/pop-os/cosmic-comp , https://github.com/pop-os/libcosmic
- S16. rootless podman 요구 사항: https://wiki.gentoo.org/wiki/Libpod
- S17. macOS 15 / M3+ 중첩 가상화: https://forum.parallels.com/threads/macos-15-sequoia-nested-virtualization-for-m3-macs.364397/ , https://news.hada.io/topic?id=15318
- S18. GitHub 러너 중첩 가상화 논의: https://github.com/actions/runner-images/discussions/7191
- S19. 가격 사례: https://www.chollometro.com/ofertas/lenovo-xiaoxin-pad-pro-2025-8128gb-pantalla-127-3k-con-144hz-dimensity-8300-10200mah-8256gb-por-168eur-tablet-1729361
- S20. cxx-qt: https://github.com/KDAB/cxx-qt
- S21. GTK 4 접근성 설계: https://blog.gtk.org/2020/10/21/accessibility-in-gtk-4/
- S22. GTK 접근성 업데이트 (4.18 AccessKit): https://blog.gtk.org/2025/05/12/an-accessibility-update/ , GTK 4.14: https://blog.gtk.org/2024/03/08/accessibility-improvements-in-gtk-4-14/
- S23. GTK 이슈 #6269 (Selection 인터페이스): https://gitlab.gnome.org/GNOME/gtk/-/issues/6269
- S24. COSMIC 알파 3 접근성: https://alternativeto.net/news/2024/11/cosmic-alpha-3-released-with-enhanced-file-management-and-improved-accessibility-features
- S25. libhybris: https://en.wikipedia.org/wiki/Libhybris / UBports 포팅 안내: https://docs.ubports.com/porting/introduction/Intro.html
- S26. UBports 포럼 (GKI, Halium 14): https://forums.ubports.com/post/89132
- S27. postmarketOS MT6878 커널: https://pkgs.postmarketos.org/package/master/postmarketos/aarch64/linux-postmarketos-mediatek-mt6878
- S28. Fedora FEX 통합 제안: https://www.fedoraproject.org/wiki/Changes/FEX
- S29. Steam Frame과 FEX (2차 보도): https://www.pcgameshardware.de/SteamOS-Software-258049/News/ARM-Support-dank-FEX-1487312/
- S30. GenieZone 하이퍼바이저 커널 문서 패치: https://lists.infradead.org/pipermail/linux-arm-kernel/2023-July/853606.html
- S31. `/dev/gzvm` 노출 사례 (DroidVM): https://github.com/HuJK/DroidVM
- S32. Android 가상화 프레임워크(pKVM) 구조: https://source.android.com/docs/core/virtualization/architecture
- S33. Yocto Mesa Panthor 패치 (`mali_csffw.bin`과 linux-firmware): https://patchwork.yoctoproject.org/project/yocto/patch/20250415-mesa-panthor-v2-1-a7850d0091fe@cherry.de
- S34. GitHub 변경 기록, 표준 Linux 러너의 KVM 하드웨어 가속 (2024-04-02): https://github.blog/changelog/2024-04-02-github-actions-hardware-accelerated-android-virtualization-now-available/
- S35. GitHub 변경 기록, 공개 저장소용 arm64 호스티드 러너 정식 출시 (2025-08-07): https://github.blog/changelog/2025-08-07-arm64-hosted-runners-for-public-repositories-are-now-generally-available/
- S36. GitHub 문서, self-hosted 러너 추가 (공개 저장소 관련 경고): https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/add-runners
- S37. GPLv3 원문 (6조 User Product, Installation Information): https://www.gnu.org/licenses/gpl-3.0.html
- S38. CC BY-SA 4.0의 GPLv3 단방향 호환 발표 (2015-10-08): https://creativecommons.org/2015/10/08/cc-by-sa-4-0-now-one-way-compatible-with-gplv3/
- S39. REUSE 규격 3.3: https://reuse.software/spec-3.3/
- S40. 리눅스 커널 패치 제출 문서 (DCO): https://www.kernel.org/doc/html/latest/process/submitting-patches.html
- S41. XDA 비공식 언락 글타래 (2025-02~2026-10, 2026-11-01 브라우저로 확인): https://xdaforums.com/t/unofficial-unlocking-lenovo-xiaoxin-pad-pro-2025-tb375fc.4717695/
- S42. Debian forky 꾸러미 색인 (스냅숏 20261027T083111Z, 2026-11-02 조회): https://snapshot.debian.org/archive/debian/20261027T083111Z/dists/forky/main/binary-arm64/Packages.xz
- S43. AppImage 문서, FUSE 문제 해결 (2026-11-02 조회): https://docs.appimage.org/user-guide/troubleshooting/fuse.html , 형식 규격: https://github.com/AppImage/AppImageSpec/blob/master/draft.md
- S44. GNOME Software README (2026-11-02 조회): https://gitlab.gnome.org/GNOME/gnome-software/-/blob/main/README.md
- S45. Bazaar README (2026-11-02 조회): https://gitlab.gnome.org/World/bazaar/-/blob/main/README.md
- S46. Flathub의 Debian 설치 안내 (2026-11-02 조회): https://flathub.org/setup/Debian
- 미확인: XDA 비공식 언락 스레드(접근 차단): https://xdaforums.com/t/unofficial-unlocking-lenovo-xiaoxin-pad-pro-2025-tb375fc.4717695/
