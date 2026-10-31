# buntOS TB375FC 포팅 최종 설계안 v1.1

- 작성일: 2026-11-01
- v1.1(2026-11-01): 커널 커스텀과 부트 이미지 도구를 만든 뒤 2.3절(2a 장치 트리를 업스트림 꼴로 시작)과 2.4절(`fastboot boot`와 헤더 v4)을 고쳤다
- 상태: 확정(기기를 받기 전). 기기 실측과 첫 부팅 결과로 고친다
- 대상: Lenovo Xiaoxin Pad Pro 12.7 (2025, TB375FC). 같은 하드웨어인 Lenovo Idea Tab Pro(TB373FU)도 같은 BSP로 지원한다
- 관련 문서:
  - [포팅 재료 조사](porting-materials.md)
  - [Wi-Fi·Bluetooth soc7_0](wifi-soc7_0.md)
  - buntOS의 [하드웨어 실측](../hw/lenovo-tb375fc.md)과 [설계안](../design/ARCHITECTURE.md)(4.2절, 12장)

## 0. 한눈에

**목표.** TB375FC에서 buntOS(mainline 커널, Debian, buntOS 데스크톱)를 태블릿 PC로 쓴다. 터치 10점, 펜, 키보드 커버, Wi-Fi, 배터리·충전, 서스펜드까지 쓸 수 있어야 한다.

**하드웨어.** 펌웨어 두 판(글로벌 ZUI 17, 중국 ZUI 16)에서 직접 읽었다.

| 항목 | 내용 |
|---|---|
| SoC | MediaTek MT6897 (Dimensity 8300, 태블릿판 이름 MT8792). A510 ×4 + A715 ×4, PSCI 1.0, GICv3, Mali-G615 MC6 |
| 화면 | Novatek NT36532 패널 셋(BOE, Tianma, Tianma 무광) 가운데 하나. 2944×1840, **DSI 두 줄(dual-DSI) + DSC**, 144/120/60/30Hz |
| 터치 | 같은 NT36532 칩의 터치(TDDI), SPI, 동시 10손가락, 펜. 플래시가 없어 부팅마다 펌웨어를 내려 준다 |
| 저장장치·USB | UFS(`mediatek,mt8183-ufshci`, 업스트림이 MT6897 IP 판을 안다), mtu3 + xHCI + xsphy, USB-C(MT6375 TCPC), DP Alt |
| 전원 | PMIC MT6363·MT6368·MT6319 ×3·MT6685·MT6375. 차지 펌프 NU2115 ×2·UPM6720 ×2, 게이지 MM8013 |
| 무선 | connsys soc7_0(CONNAC2X, 칩 ID `0x7961` = MT7921 계열) + RF 칩 MT6637. 보정값은 nvdata 파티션 |
| 그 밖 | 앰프 AW883xx ×4, 홀 센서 둘, 센서 허브 SCP(가속도·조도), UART 포고 키보드(Tinno MCU), SAR 둘 |
| 부팅 | LK가 boot v4 + vendor_boot v4(장치 트리는 DT 표) + dtbo를 읽는다. GenieZone이 EL2를 차지할 가능성이 높다 |

**가장 큰 재료 넷**([조사](porting-materials.md)).
1. 이 기기의 벤더 모듈 소스 미러(`mt8792/android_kernel_lenovo_mt8792-modules`, 6.1.138): 패널·터치·앰프·홀·포고 키보드 드라이버가 그대로 있다.
2. 이 기기의 커뮤니티 mainline 포트(`u0n0u/tb375fc-linux`, Linux 7.2): UART·저장장치가 되고 터치·Wi-Fi·GPU·충전은 작업 중이다.
3. 같은 SoC의 완전한 벤더 트리: OPPO Pad 3·OnePlus Pad(같은 MT8792 태블릿 보드), Motorola(가장 새것)
4. 업스트림 견본: MT6895-Mainline(PMIC 조합이 같다), MT6878-mainline(최소 부팅), mainline에 이미 있는 NT36532 패널·panthor G615·pinctrl-mt6878

**가장 큰 일과 위험.**
- 큰 일:
  - SoC 기반(클럭·전원 도메인·핀)
  - 화면 파이프라인(dual-DSI + DSC, 업스트림에 없다)
  - Wi-Fi(soc7_0)
  - 오디오(AFE + MT6368)
- 위험:
  - 언락 중 벽돌
  - EL2 점유(가상화 불가)
  - 센서 허브(자동 회전이 막힐 수 있다)
  - 펌웨어 재배포

## 1. 핵심 결정

| # | 결정 | 까닭 |
|---|---|---|
| D1 | **mainline 커널.** buntOS 이미지와 같은 Debian forky 커널(지금 7.2.12)에 보드 설정과 제출 대기 패치를 더한다. 벤더 GKI는 쓰지 않는다 | buntOS 설계안 4.2.2절. 필요한 업스트림 기능(6.18~7.0)이 7.2에 다 있다. 패널 드라이버만 7.4라 백포트한다 |
| D2 | **업스트림 꼴로 짠다.** `mt6897.dtsi` + `mt6897-lenovo-tb375fc.dts`, clk·pinctrl·pmdomain은 업스트림 틀(mt8196·mt6878 견본)에 MT6897 데이터를 넣는다 | 제출할 수 있어야 유지할 수 있다. 커뮤니티 포트는 클럭·핀 드라이버 없이 fixed-clock과 부트로더 상태에 기대는 구조다. 그래서 사실 확인용과 협력 상대로 쓰고, 기반으로는 쓰지 않는다 |
| D3 | **처음에는 LK가 직접 띄운다.** boot v4 + vendor_boot v4(DT 표) + 빈 dtbo, `fastboot boot`로 RAM에서. 2b부터는 U-Boot 체인로드로 EFI(systemd-boot, UKI) 부팅을 검토한다 | 파티션을 건드리지 않는 첫 부팅. EFI가 되면 buntOS의 QEMU 이미지 꼴과 같아진다(D3 동등성). MT6878-mainline도 U-Boot를 권한다 |
| D4 | **첫 데스크톱은 simple-framebuffer다.** LK가 dual-DSI + DSC로 켜 둔 화면을 그대로 받는다(`clk_ignore_unused`, `pd_ignore_unused`). 화면 파이프라인(mediatek-drm의 MT6897 데이터, dual-DSI, DSC)은 업스트림 시리즈를 따라 뒤에 한다 | 화면이 가장 큰 막힘이다. 그 사이에 나머지(저장장치, USB, 터치, Wi-Fi, GPU)를 진행할 수 있다 |
| D5 | **Wi-Fi는 mt76에 "MT7921의 SoC 변형"을 더한다.** conninfra 전원, nvdata 보정값을 쓴다. 그 전에는 USB 동글로 우회한다 | 칩 ID와 구조가 MT7921과 같고 SoC Wi-Fi 선례(mt7915/soc.c)가 있다([설계](wifi-soc7_0.md)) |
| D6 | **펌웨어는 설치할 때 기기에서 뽑는다.** Mali CSF만 linux-firmware에서 온다. GPUEB는 LK가 올린다 | 재배포할 수 있는지 모른다(buntOS ADR-012). 안드로이드 슬롯 하나를 남긴다 |
| D7 | **GPU는 panthor + MT6897 GPUEB 변형이다.** 이미지에 Mesa 26.3 이상이 필요하다 | panthor·펌웨어·Mesa가 이미 G615를 안다. 전원은 MT8196 드라이버에 공유 메모리 오프셋만 다른 변형을 더한다 |
| D8 | **가상화는 2a에서 EL을 본 뒤 정한다.** GenieZone이 EL2를 차지하면 KVM을 포기하고, gzvm(미병합)을 나중에 검토한다. TEE를 고쳐 EL2를 빼앗는 실험(`mtk-mod-tee-nogz`)은 쓰지 않는다 | 서명 우회가 필요하고 벽돌 위험이 크다 |

## 2. 부팅 길

### 2.1 부트 체인과 언락

- **부트 체인:** BROM → preloader → ATF·TEE·GenieZone → LK → 커널. LK는 AVB로 boot, vendor_boot, dtbo를 검증한다.
- **언락:** 공식 길이 없다. CN 펌웨어 ZUI 16.0.20.091에서 언락과 AVB 해제가 됐다는 보고가 있고, 그 펌웨어의 위치를 확인해 두었다(Lolinet 미러, 지금은 지웠다).
  - 진행 조건은 buntOS 설계안 4.2.3절의 go/no-go 넷이다. 하나라도 빠지면 언락하지 않는다.
    1. 전체 파티션 백업과 해시 확인
    2. 독립된 보고 둘 이상으로 교차 확인
    3. MT6897 BROM 복구 길(mtkclient) 확인
    4. OTA 차단
  - nvdata·nvram 계열에는 Wi-Fi MAC과 보정값이 있다. 잃으면 되살릴 수 없다.
- **슬롯:** 한쪽 슬롯과 recovery는 안드로이드로 남긴다. 정보원이자, 펌웨어를 뽑는 곳이자, 복귀 길이다.

### 2.2 이미지 꼴

| 이미지 | 넣는 것 | 메모 |
|---|---|---|
| boot | 헤더 v4, mainline 커널(Image, LZ4 legacy) | 원래 커널도 LZ4 legacy다 |
| vendor_boot | 헤더 v4, DTB 영역에 **DT 표**(magic `0xd7b7ab1e`) 꼴로 우리 장치 트리, 명령줄 | MT6897 LK는 DTB를 DT 표에서 찾는다 |
| init_boot | 헤더 v4, 우리 initramfs(USB 가젯, nvdata 마운트) | LK가 initrd로 넘긴다 |
| dtbo | 빈 오버레이 하나 | LK가 dtbo를 기본 트리에 입힌다. 벤더 dtbo를 남기면 벤더 노드 경로를 기대하는 fixup이 실패할 수 있다 |

- 이미지 만들기는 이 폴더의 `boot/`에 둔다. buntOS `tools/hw/bootimg.py`의 반대 방향으로, `mkbootimg` v4 꼴과 DT 표를 그대로 따른다.
- **LK와 `/chosen`:** "LK가 `/chosen`을 덮어써서 커뮤니티 포트가 장치 트리를 커널에 넣어 우회한다"는 조사 보고가 있다. README에는 없는 내용이라 미확인이다.
  - 커널 명령줄은 boot·vendor_boot 이미지의 명령줄로 준다.
  - 모자라면 커널 `CMDLINE_EXTEND`를 쓴다. DTB를 커널에 넣는 우회는 하지 않는다.
  - 2a에서 LK가 우리 트리의 무엇을 고치는지 살아 있는 트리로 확인한다.

### 2.3 장치 트리 방침

- **2a:** 처음부터 업스트림 꼴로 짠다(v1.0의 "벤더 트리를 줄여 출발"을 바꿨다). 커뮤니티 포트가 이 꼴의 트리로 LK에서 UART 콘솔과 저장장치까지 띄웠다. 주소와 인터럽트는 커뮤니티 포트의 `mt6897.dtsi`, 벤더 `mt6897.dts`, 우리가 뽑은 트리 셋을 대조한다(`kernel/tb375fc/dts/README.md`).
  - LK가 찾는 패널 경로(`/soc/dsi@1400d000/panel1@0` 등)는 화면을 붙이는 2c에서 더한다.
  - 첫 부팅 뒤 LK가 우리 트리의 무엇을 고치는지 살아 있는 트리로 견준다.
- **2b부터:** 꺼 둔 노드(UFS, USB, SPMI 등)를 클럭·핀·전원 도메인 패치와 함께 하나씩 켠다.
- **최종 꼴:** `mt6897.dtsi`(SoC)와 `mt6897-lenovo-tb375fc.dts`(보드)다. TB373FU는 같은 dts에 `model`만 다르다.
- **framebuffer:** 정적 트리에 없다. LK가 부팅 화면을 켠 영역을 기기에서 읽는다(안드로이드의 `/proc/iomem`, 살아 있는 트리). 그 주소로 `simple-framebuffer`를 만든다. 화면은 1472×1840 두 줄을 합친 2944×1840이고, LK가 DSC까지 켜 둔다.
- **예약 메모리:** 정적 트리에는 동적 할당 셋(modem 버퍼, CMA 256MB, 커널 보호)뿐이다. TEE, GenieZone, connsys, framebuffer의 고정 예약은 preloader·LK가 부팅 때 더한다.
  - 그래서 우리 트리에도 `/reserved-memory`(셀 2)를 남긴다.
  - 기기에서 실제 배치를 본 뒤 pstore 칸을 정한다.

### 2.4 처음 띄우는 법과 디버그 채널

- 첫 부팅은 `fastboot boot`로 RAM에서 한다. 지원하지 않으면 비운 슬롯에 굽는다.
  - `fastboot boot`는 boot.img 하나만 RAM으로 보낸다. 헤더 v4에서 장치 트리는 vendor_boot에, initramfs는 init_boot에 있어서, LK가 이 둘을 굽혀 있는 원래 파티션에서 읽을 가능성이 높다(AOSP 부트로더의 동작이다. MT6897 LK에서는 확인하지 않았다).
  - 그래서 init이 vendor_boot의 표지로 LK가 우리 vendor_boot를 읽었는지 알린다. initramfs를 boot.img에도 넣은 이미지(`mkimages.sh --boot-ramdisk`)로 LK가 그 램디스크를 쓰는지도 본다.
  - 원래 장치 트리로 뜨면 비운 슬롯에 넷을 굽는 길로 간다.
- 디버그 채널:
  1. **UART:** 커뮤니티 포트는 UART 콘솔을 쓴다. 기기 밖 단자는 없으니 위치(시험 패드나 포고 핀의 UART3)를 그 포트와 `tb375fc-linux-utils`에서 확인한다. 포고 핀 UART3(GPIO180·181)은 키보드가 쓰는 줄이다.
  2. **화면 콘솔:** simple-framebuffer + simpledrm
  3. **pstore/ramoops:** 직전 부팅의 커널 기록을 남긴다
  4. **USB 가젯(2b):** configfs ACM + NCM. initramfs 첫 단계에서 띄워 호스트에서 SSH로 들어간다
- 루트 파일 시스템:
  - 2a는 initramfs다.
  - 2b부터는 UFS의 비운 슬롯이나 userdata 일부, 또는 USB-C 저장장치를 쓴다.

## 3. 단계별 bring-up

| 단계 | 할 일 | 종료 조건 |
|---|---|---|
| 2a 첫 부팅 | 줄인 벤더 트리(CPU, GIC, 타이머, PSCI, 메모리, simple-framebuffer, ramoops, UART), 이미지 꼴, Debian 커널 그대로 | initramfs 셸이 화면이나 UART에 뜬다. 커널 기록으로 EL2 판정(12.1절) |
| 2b 저장장치·디버그 | 핀(mt6878 틀), 전원 도메인(mt8196 틀), 클럭(처음에는 fixed + `clk_ignore_unused`), SPMI, PMIC MT6363·MT6319(MFD 패치), UFS, USB(mtu3·xHCI·xsphy), cpufreq-hw | UFS의 Debian 루트, USB 가젯 SSH, USB 호스트(동글·키보드) |
| 2c 화면·입력 (1차) | simple-framebuffer로 buntOS 데스크톱, NT36532 SPI 터치(펌웨어 내려받기), 펜, 홀 센서(gpio-keys), 포고 키보드(serdev) | 2944×1840으로 buntOS 로그인 화면, 터치·펜 조작, 키보드 커버를 붙이면 포인터 모드 |
| 2d 무선 | conninfra, mt76 soc7_0, 펌웨어·nvdata 보정값, FEM, Bluetooth | Wi-Fi 연결, Bluetooth 키보드·이어폰 |
| 2e GPU·전원 | panthor + GPUEB 변형, Mesa 26.3, 클럭 드라이버 완성(`clk_ignore_unused` 빼기), LVTS 열, MT6375 충전·TCPC, MM8013, 서스펜드 | GLES로 컴포지터, 서스펜드·복귀 반복, 배터리·충전 표시 |
| 2c 화면 (2차) | mediatek-drm의 MT6897 데이터, dual-DSI, DSC, NT36532 패널 셋(업스트림 드라이버 + compatible 셋) | 주사율·밝기 조절, 화면 끄고 켜기 |
| 3 최적화 | 8절, 오디오, 외부 화면 | buntOS 설계안 9.3절 스모크 시험 |

화면 2차를 2e 뒤로 둔 것은 업스트림의 DSC·mediatek-drm 재구성 시리즈를 기다리기 위해서다(D4).

## 4. 커널 구성

- **기반:** Debian forky 커널(buntOS 이미지와 같은 판, 지금 7.2.12)을 쓴다. 포팅이 진행되면 그때의 Debian 판을 따라간다.
- **설정과 패치:** 보드 설정 조각(`kernel/config/tb375fc.config`)과 업스트림 제출 대기 패치 대기열(`kernel/patches/`)을 둔다. 대기열에 처음 들어갈 것:
  - NT36532 패널 드라이버 백포트(7.4)와 compatible 셋
  - MFD `mtk-spmi-pmic`(v12, MT6363 regulator·키, MT6685가 의존)
  - `pinctrl-mt6897`, `mt6897` 전원 도메인 데이터, `clk-mt6897`
  - NT36532 SPI 터치, 포고 키보드(v6 시리즈 위)
  - mt76 soc7_0과 conninfra
  - GPUEB MT6897 변형
- **2a에 필요한 설정**(Debian 기본에 대부분 있다):
  - `ARCH_MEDIATEK`, `SERIAL_8250_MT6577`
  - `DRM_SIMPLEDRM`
  - `PSTORE_RAM`
  - `USB_CONFIGFS_ACM`·`NCM`
- **buntOS 빌드:** `bsp/lenovo-tb375fc/`를 만든다. mkosi 설정, 커널 조각, 부트 이미지 만들기를 둔다. 이 폴더에서 다듬은 것을 buntOS 저장소로 옮긴다.

## 5. 블록별 포팅 계획

크기는 작음(DT나 compatible만), 중간(새 드라이버나 변형 하나), 큼(여러 드라이버나 새 SoC 지원)이다. 근거와 확인 여부는 [조사](porting-materials.md) 2절에 있다.

| 블록 | 업스트림에 있는 것 | 할 일 | 크기 | 단계 |
|---|---|---|---|---|
| CPU·GIC·PSCI·타이머 | 전부 | DT | 작음 | 2a |
| CPU 주파수 | `mediatek-cpufreq-hw` | DT(도메인 셋), 더미 `cpu-supply` | 작음 | 2b |
| 핀 제어 | `pinctrl-mt6878`(틀) | `pinctrl-mt6897`(벤더 표) | 중간 | 2b |
| 전원 도메인 | `mtk-pm-domains`(MT8196) | MT6897 데이터 | 중간 | 2b |
| 클럭 | `clk-mt8196-*`(틀) | `clk-mt6897`(벤더 표) | 큼 | 2b~2e |
| SPMI·MT6363·MT6319 | `spmi-mtk-pmif`, `mt6363-regulator`, `mt6315-regulator` | compatible, MFD 패치 | 작음~중간 | 2b |
| MT6368·MT6685·MT6375 | 없음 | MT6895 포크 견본으로 새로 | 중간~큼 | 2b·2e·3 |
| UFS | `ufs-mediatek`, `phy-mtk-ufs` | ufscfg 클럭·리셋, VCC, 전원 도메인 | 작음~중간 | 2b |
| USB | `mtu3`, `xhci-mtk`, `phy-mtk-xsphy` | DT, 역할 전환은 TCPC 뒤 | 작음 | 2b |
| 화면 파이프라인 | MT8195·MT8188까지. DSC·dual-DSI 없음 | MT6897 데이터, dual-DSI, DSC | 큼 | 2c(2차) |
| 패널 NT36532 | `panel-novatek-nt36532`(7.4) | compatible 셋과 초기화 순서 | 작음 | 2c(2차) |
| 터치 NT36532 SPI + 펜 | I2C 드라이버뿐 | SPI 드라이버, 펌웨어 내려받기, 펜 장치 | 중간 | 2c |
| 홀 센서 | `gpio-keys` | DT(`SW_TABLET_MODE`, `SW_LID`) | 작음 | 2c |
| 포고 키보드 | v6 시리즈(리뷰 중) | serdev 드라이버, 프레이밍 확인 | 중간 | 2c |
| Wi-Fi·BT | `mt7921`, `mt7915/soc.c`(선례) | soc7_0 버스, conninfra, 펌웨어, FEM, BT 전송 | 큼 | 2d |
| GPU | panthor(G615), linux-firmware CSF | DT | 작음 | 2e |
| GPU 전원 | `mtk-mfg-pmdomain`, `mtk-gpueb-mailbox`(MT8196) | MT6897 변형(공유 메모리 오프셋, 메일박스 표) | 중간 | 2e |
| 열 | `lvts_thermal` | MT6897 데이터(센서 16개) | 중간 | 2e |
| 게이지 | `mm8013` | DT | 작음 | 2e |
| 차지 펌프 | 없음 | 새로(고속 충전) | 중간 | 3 |
| 오디오 | MT8196·MT8189 AFE, MT6359 코덱, AW88395·AW88399·AW88166, FSA4480 | MT6897 AFE, MT6368 코덱·accdet, 앰프 칩 확인, ACF | 큼 | 3 |
| SAR | `hx9023s` | HX9031AS를 hx9023s로 시도, HX9036 새로 | 작음/중간 | 3 |
| 센서 허브 | MT8183~MT8195 SCP만 | MT6897 SCP(nanohub) 길 조사 | 큼 | 3 |
| 외부 화면 | `mtk_dp`(MT8195), PS5169/70 패치 | 화면·TCPC 뒤 | 큼 | 3 |
| 비디오 코덱·카메라 | — | 범위 밖 | — | — |

## 6. Wi-Fi·Bluetooth

자세한 것은 [docs/wifi-soc7_0.md](wifi-soc7_0.md)다.

| 확인한 것 | 근거 |
|---|---|
| 내장 connsys soc7_0, 칩 ID `0x7961`(CONNAC2X), AXI·PCIe 두 빌드 | 벤더 소스 `soc7_0.h`의 `SOC7_0_CHIP_ID 0x7961`, `soc7_0.c` |
| RF 칩(A-die) MT6637 | 기기 Wi-Fi 펌웨어의 소스 경로 `wifi/base/soc7_0/hal/phy/6637/…` |
| 기기 펌웨어가 soc7_0용 | `connsys_wifi.img`·`connsys_bt.img`의 빌드 `t-neptune-main-soc7_0-2104-…-20250320113502` |
| 벤더 드라이버 소스 | gen4m `chips/soc7_0`: Xiaomi rodin, OnePlus Pad A16, OPPO Pad 3, Motorola gen4m 저장소. Dimensity 8300 Xiaomi 갈래와 OPPO Reno13 A15에는 없다 |
| 업스트림 길 | mt76 `mt7921`이 0x7961을 PCIe로 지원하고, `mt7915/soc.c`가 SoC Wi-Fi + SPI A-die 선례다 |
| 보정값 | nvdata 파티션(커뮤니티 포트는 initramfs에서 nvdata를 마운트한다) |

**설계.**
- 방식: mt76에 "MT7921의 SoC 변형"을 더한다. 할 일은 다음과 같다.
  - AXI MMIO 버스
  - conninfra 전원 순서
  - 예약 메모리에 ROM 이미지 올리기
  - nvdata 보정값 넘기기
  - FEM 고르기
- 순서: 2b의 클럭·PMIC가 먼저다.
- 우회: 그 전에는 USB 동글(mt7921u)이나 USB 이더넷을 쓴다.
- 함께 일하기: 커뮤니티 포트의 soc7_0 플랫폼 작업과 겹치므로 같이 할 수 있는지 먼저 본다.

## 7. 펌웨어 방침

| 펌웨어 | 어디서 | 누가 올리나 | 재배포 |
|---|---|---|---|
| Mali CSF(`arm/mali/arch11.8/mali_csffw.bin`) | linux-firmware | 커널(panthor) | 가능 |
| GPUEB(`gpueb.img`) | 기기 파티션 | **LK**(벤더 커널에 request_firmware가 없다) | 해당 없음 |
| 터치 NT36532(`novatek_ts_fw_boe.bin`, `novatek_ts_fw_tm.bin`) | 기기 vendor | 커널(부팅마다) | 미확인. 기기에서 뽑는다 |
| Wi-Fi·BT soc7_0(ROM EMI, 패치, RAM 코드) | 기기 vendor | 커널 | 미확인. 기기에서 뽑는다 |
| Wi-Fi 보정값·MAC | 기기 nvdata | 커널(initramfs가 넘긴다) | 기기마다 다르다 |
| 앰프 AW883xx ACF, SCP·오디오 DSP | 기기 vendor·파티션 | 커널 | 미확인 |

- 펌웨어 추출 스크립트는 `firmware/`에 두고, 펌웨어 파일 자체는 저장소에 넣지 않는다(buntOS ADR-012).
- 설치기는 남겨 둔 안드로이드 슬롯이나 처음 백업에서 펌웨어를 가져온다.

## 8. 기기 맞춤 최적화 (buntOS 쪽)

| 항목 | 할 일 | buntOS에서 바꿀 곳 |
|---|---|---|
| 화면 배율 | 12.7″ 2944×1840은 약 273ppi다. buntOS 규칙(내장 화면은 논리 125ppi에 가깝게)이면 기본 배율은 **225%**(논리 1308×818)이고, 고를 수 있는 것은 100~275%다. DSI에는 EDID가 없으므로 패널(또는 simple-framebuffer 노드)이 실제 크기 약 273×171mm를 알려야 이 규칙이 돈다 | 패널 드라이버의 `width_mm`·`height_mm`, simplefb DT의 물리 크기 |
| 주사율 | simple-framebuffer 단계는 LK가 정한 모드 하나다. mediatek-drm 단계에서 144/120/60/30Hz를 열고, 움직임이 없으면 낮추는 적응형 주사율(macOS ProMotion처럼)을 둔다 | `comp`의 출력 모드와 프레임 일정 |
| 터치 | 10점을 그대로 받는다(컴포지터에 손가락 수 제한이 없다). 손바닥 거부와 가장자리 띠(위 24px, 아래 20px)를 실기기에서 맞춘다 | `comp/src/touch.rs` |
| 펜 | NT36532 펜을 tablet-v2(압력·기울기·단추)로 앱에 보낸다. 컴포지터에 smithay tablet 틀이 있고, 펜 디지타이저는 입력 모드 계산에서 이미 뺀다 | `comp` tablet-v2, 시스템 설정 펜 페이지(나중) |
| 키보드 커버 | 포고 키보드가 붙으면 포인터 모드, 떼면 터치 모드다(지금 규칙 그대로). plug GPIO26으로 붙음을 안다. 커버를 뒤로 접으면 홀 센서가 `SW_TABLET_MODE`를 보내므로 그때는 키보드·터치패드를 세지 않는 규칙을 더한다(iPad Magic Keyboard처럼) | `comp/src/input_mode.rs`, gpio-keys DT |
| 회전 | 가속도 센서가 SCP(nanohub) 뒤에 있어 자동 회전이 막힐 수 있다. 처음에는 시스템 설정의 수동 회전이고, SCP 길이 열리면 iio-sensor-proxy로 자동 회전한다 | 시스템 설정 디스플레이 |
| 전원 | A715 ×4 + A510 ×4의 EAS, cpufreq-hw, GPU 주파수, s2idle, 덮개(홀)로 잠자기, 45W PD 충전, LVTS 열 관리 | 커널, buntOS 전원 설정 |
| 오디오 | 오디오가 되기 전에는 USB·Bluetooth 오디오다. 그 뒤 스피커 넷(AW883xx)의 보호 설정과 ALSA UCM을 둔다. 볼륨 키는 PMIC 키다 | UCM, WirePlumber |
| 메모리 | 8·12GB. zram과 GPU 공유 메모리를 맞춘다 | 이미지 설정 |
| 그래픽 | Mesa 26.3 이상(G615). PanVK는 실험 플래그가 필요해 GL(Panfrost)로 시작한다 | 이미지의 Mesa 판, `GSK_RENDERER` 기본값 |

## 9. 저장소 구조

이 저장소는 buntOS의 포크(`buntos-TB375FC`)다. buntOS 구조에 기기 쪽을 더한다.

```
buntos-TB375FC/                 buntOS 포크 (원격 buntos = 원본)
  docs/tb375fc/                 이 문서, 조사, Wi-Fi 설계
  docs/hw/lenovo-tb375fc.md     펌웨어 실측
  kernel/tb375fc/
    build.sh                    Linux 7.2.x 받기·패치·설정·빌드
    config/tb375fc.config       보드 설정 조각
    patches/                    업스트림 제출 대기 패치 (series)
    dts/mediatek/               mt6897.dtsi, mt6897-lenovo-tb375fc.dts, -tb373fu.dts
  bsp/lenovo-tb375fc/
    boot/                       boot·vendor_boot·init_boot·dtbo 만들기 (헤더 v4, DT 표)
    initramfs/                  첫 부팅용 initramfs (busybox, USB 가젯, nvdata)
    firmware/                   기기에서 펌웨어와 보정값을 뽑는 스크립트 (파일은 두지 않는다)
  tools/hw/                     펌웨어 받기·풀기 도구
```

- 빌드 산출물은 저장소 밖(빌드 VM의 `~/buntos-build/tb375fc/`)에 둔다.
- 원본 buntOS로 돌려줄 만한 것(펌웨어 도구, 데스크톱 쪽 고침)은 원본에 따로 낸다.

## 10. 위험과 대응

| 위험 | 영향 | 대응 |
|---|---|---|
| 언락 중 벽돌, nvdata 손실 | 개발 중단, Wi-Fi 보정값 잃음 | 예비 1대, go/no-go 넷, 전체 백업, BROM 복구 길 사전 확인 |
| GenieZone의 EL2 점유 | KVM 불가 | 2a에서 판정(D8). "불가"면 buntOS ADR-011 재검토, gzvm은 미병합이라 나중 |
| 화면: dual-DSI + DSC가 업스트림에 없음 | 주사율·밝기·화면 끄기가 늦어진다 | simple-framebuffer로 데스크톱(D4), 업스트림 DSC·재구성 시리즈를 따라가고 dual-DSI는 우리가 맡을 수도 있다 |
| 센서 허브(SCP nanohub) | 자동 회전·조도 없음 | 수동 회전, SCP 길은 3단계에서 조사 |
| MFD `mtk-spmi-pmic` 미병합 | PMIC가 늦어진다 | 패치 대기열에 넣고 업스트림 판을 따라간다 |
| Mesa 26.3 의존 | GPU 가속이 늦어진다 | 이미지의 Mesa 판 확인, 그 전에는 CPU 렌더링(buntOS는 이미 llvmpipe에서 돈다) |
| Wi-Fi 펌웨어 명령이 MT7921과 다름 | 2d 지연 | USB 동글 우회, soc7_0 전용 경로 |
| 펌웨어 재배포 불가 | 배포 꼴 | 설치할 때 기기에서 뽑기(D6) |
| 소스 출처 | 법적 불확실 | 이 기기 모듈 미러는 제3자다. 참고만 하고 코드는 업스트림 꼴로 다시 쓰며, GPL 출처(OEM 공식 저장소)를 우선한다 |
| 커뮤니티 포트와 겹침 | 중복 작업 | 연락해 나눌 수 있는 일(soc7_0, 터치, 충전)을 맞춘다 |
| 생산 시기별 부품 차이 | 일부 기기만 다름 | Wi-Fi FEM은 가장 새 펌웨어의 표, 패널은 LK가 고른 노드를 읽는다 |

## 11. 일정과 다음 작업

1. **지금(기기 없이):**
   - 2a 장치 트리 초안(벤더 트리 줄이기, 커뮤니티 dtsi 대조)
   - `boot/` 이미지 만들기 스크립트
   - Debian 7.2 커널 설정 확인
   - 커뮤니티 포트의 커밋 12개를 읽고 겹침을 정리
2. **기기 도착 직후(언락 전, ADB):** buntOS 설계안 12장 항목을 잰다.
   - `getevent -lp`
   - framebuffer 주소, 살아 있는 트리
   - 파티션
   - vendor 펌웨어 목록
   - UART 위치
3. **언락:** go/no-go를 통과한 뒤 2a를 한다(`fastboot boot`).
4. **2b → 2c(1차) → 2d → 2e → 2c(2차) → 3:** 각 단계의 종료 조건을 buntOS 저장소의 개발 환경 검증 기록에 남긴다.

## 12. 출처

[포팅 재료 조사](porting-materials.md) 5절과 [Wi-Fi 문서](wifi-soc7_0.md)의 출처 목록을 따른다.
