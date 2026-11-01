# TB375FC 커뮤니티 mainline 포트 분석

- 작성일: 2026-11-01
- 대상: [u0n0u/tb375fc-linux](https://github.com/u0n0u/tb375fc-linux)(main, `a887fcc`), [Villode/tb375fc-linux-utils](https://github.com/Villode/tb375fc-linux-utils)(`30dc377`)
- 목적: [설계안](DESIGN.md) 11절의 "커뮤니티 포트의 커밋을 읽고 겹침을 정리"를 끝낸다. 우리 계획(설계안 5절, `kernel/tb375fc/patches/series`)과 겹치는 곳, 가져다 쓸 사실, 협력할 일을 정한다.
- 표시:
  - ✔: 코드를 직접 열어 확인했다(펌웨어 트리와 대조한 것 포함).
  - ○: 커뮤니티의 주석이나 측정만 있다.
  - ✗: 틀렸다.
- 출처의 성격:
  - 둘 다 개인 저장소다.
  - 레지스터 값의 상당수는 벤더 모듈을 역어셈블해 얻었다고 적혀 있다. 그래서 OEM GPL 소스와 대조한 뒤에 쓴다.
  - 코드는 옮기지 않고 사실만 쓴다(설계안 D2).

## 1. 한눈에

- **바탕:** Linux 7.2 위의 MT6895-Mainline `7.2-mt6895-xiaomi-xaga`(`33f2f15a3`)에 자체 커밋 13개(머지 하나 포함)를 얹었다. 2026-09-23 뒤로 커밋이 없다. 뿌리 갈래는 그 뒤로도 나아갔지만 따라가지 않는다.
- **된 것**(코드와 주석): LK가 켠 화면 위의 fbcon, UFS, USB 가젯.
- **작업 중:**
  - 터치: IRQ 없이 폴링하고, 펌웨어 내려받기가 실패한다.
  - Wi-Fi·BT: WF ROM이 아직 뜨지 않는다.
  - GPU: gpusys를 켜면 SoC가 멈춘다.
  - 충전: 폴링으로 돈다.
- **README와 코드가 다른 곳**(✗):
  - README는 UART와 eMMC가 된다고 적었다. 그러나 dts는 uart0을 끄고 UFS를 쓴다.
  - utils README는 initramfs가 nvdata에서 보정값을 복사한다고 적었다. 그러나 코드에는 nvdata가 없다.
- **코드의 성격:** 하류식이다. 레지스터를 직접 쓰고, 벤더 스택을 옮겨 왔고, AI의 도움을 받았다. 그래서 사실 확인과 실기 시험 상대로 쓴다.

## 2. 커밋

`gh api repos/u0n0u/tb375fc-linux/compare/33f2f15a3...main`으로 확인했다(✔). 설계안 v1.1의 "12개"는 README를 쓴 커밋 `a887fcc`를 뺀 수다.

| 해시 | 날짜 | 하는 일 |
|---|---|---|
| `aef99312cbe5` | 09-14 | 첫 스냅샷. 첫 dtsi·dts, 커널에 넣은 장치 트리로 바꿔 끼우기(`setup.c`), 화면 카나리, RGU 워치독 끄기, MT6897 compatible(i2c·spmi·disp-pwm·spi), UFS PHY·xsphy·mtu3·xHCI를 클럭·레귤레이터 없이 뜨게 고침, SPMI·PMIC 폴링, mm8013·nt36xxx 고침 |
| `375cfe902b9d` | 09-14 | 위와 같은 내용(다른 작성자 표기). Wi-Fi·BT 곁가지의 바탕 |
| `0ab6b6cc3876` | 09-14 | MT6895 conninfra를 복제해 이름을 바꾸고 connfem·BT를 더한다. consys·wifi·bt 노드. 빌드만 확인 |
| `4de7a0a8762d` | 09-23 | dts: connsys EMI 예약, SPMI·PMIC 레일, scpsys·gpusys, GPU, 충전 노드 |
| `a85293bd33af` | 09-23 | `clk-mt6897-mfgcfg.c`. 실제로는 BG3D 게이트 하나와 GPUEB·MFG 레지스터 실험 코드 |
| `c5c79f641fc0` | 09-23 | `mtk-scpsys-mt6897{,-gpusys}.c`(conn 도메인 하나와 gpusys), panthor의 MT6897 예외 처리 |
| `cbae621ce0c9` | 09-23 | mt6685-consys 셀, MT6375 충전기를 인터럽트 없이 폴링 |
| `0f7d7c5709b7` | 09-23 | nt36xxx: 벤더 순서로 probe, 칩 상태를 건드리지 않는 보존 모드(`tddi_preserve`), 디버그 매개변수 |
| `15a563c84010` | 09-23 | conninfra·BTIF·NVRAM 경로, soc7_0 MCU 초기화 수정(벤더 모듈 역어셈블에서 얻음) |
| `4592bf5f174c`, `89ceed2c9744`, `a887fcc34190` | 09-23 | README(갈래 표, 상태 표, 계보) |
| `df71e0fdac8a` | 09-23 | `merge -s ours`. `0ab6b6c`를 조상으로만 남기고 내용은 바꾸지 않는다 |

그 밖(✔):
- 닫힌 PR 셋이 있다. 2026-09-24에 AI 에이전트가 열었고, 이튿날 병합하지 않고 닫았다. 가지는 지웠지만 PR ref로 남아 있다.
  - #1: 리뷰 지적 고침
  - #2: `pinctrl-mt6897` GPIO·EINT 드라이버(`96a1733`). 빌드도 실기 시험도 하지 않았다
  - #3: CI
- 포크 넷에는 새 커밋이 없다.
- 주석이 가리키는 `reports/`, `configs/tb375fc-s1.config` 등은 공개되지 않았다.

## 3. 우리 계획과 겹침

| 커밋 | 블록 | 가져다 쓸 사실 | 우리 방식과 다른 점 | 협력 |
|---|---|---|---|---|
| `aef9931` | 2a | `0xfc16f000`·stride 11776으로 fbcon이 보이고, 펭귄 여덟(CPU 8개)이 뜬다. LK가 RGU(`0x1c00a000`)를 켠 채 넘긴다. BL31(`0x48800000`, 2MB)을 예약하지 않으면 보조 CPU와 서스펜드가 멈춘다(○) | 그쪽은 커널에 넣은 장치 트리로 바꿔 끼우고, 12GiB 기기의 예약 메모리를 고정값으로 적는다. 우리는 LK가 vendor_boot 트리에 메모리와 예약을 채우게 한다 | 사실만 |
| `aef9931` | 2b UFS·USB·SPMI | LK가 켜 둔 클럭으로 UFS가 돈다. USB 주변 장치 모드는 vusb33·vbus 없이 돈다. SPMI `chan` 레지스터는 버스 probe 전에 정해야 한다. rcs 인터럽트 없이 폴링한다(○) | 드라이버 쪽에서 클럭·레귤레이터를 없어도 되게 고친다 | 시험 상대 |
| `aef9931`, `0f7d7c5` | 2c 터치 | SPI2 `0x11012000`, 리셋 GPIO60, IRQ GPIO9, 9.6 MHz(✔ 펌웨어). CS GPIO12, 1.8V GPIO63, 전원 GPIO186·187. probe 순서는 eng_reset, 리셋 해제, chip trim이다. SPI 클럭 자리값을 26 MHz로 둬서 실제 SCLK가 약 52 MHz였고, `spi-max-frequency`를 낮췄다(○) | 하류 nt36xxx 드라이버에서 레지스터를 직접 쓴다. EINT가 없어 폴링한다 | 높음. 양쪽 다 필요하고 그쪽에는 실기가 있다 |
| `aef9931` | 2e 게이지 | mm8013 @0x55(i2c7 `0x11d02000`). SMBus word 읽기가 불안정해 바이트 단위로 읽는다(○) | 드라이버를 고친다 | 업스트림 수정 후보 |
| `4de7a0a`, `cbae621` | PMIC·충전 | MT6363 @4, MT6368 @5, MT6685 @9. connsys 레일은 VCN13, VRFIO18, VCN33_1·2, VANT18. MT6375 @0x34(i2c5 `0x11f01000`), INT GPIO7(○) | 하류 드라이버를 인터럽트 없이 쓴다 | 낮음 |
| `c5c79f6` | 2b 전원 도메인 | SPM `0x1c001000`, 상태 레지스터 `0xFB0`·`0xFB4`. conn 도메인은 ctl `0xE04`, ifr `0x1002c000`에 버스 보호 항목 넷이 있다(○, 벤더 모듈에서 뽑음) | 그쪽은 옛 `soc/mediatek/mtk-scpsys`, 우리는 `mtk-pm-domains` | 사실만. OEM 소스와 대조 |
| `a85293b`, `c5c79f6` | 2e GPU 전원 | mfgcfg `0x13fbf000`, GPUEB `0x13c00000`·`0x13c60000`·`0x13c62000`. GPUEB가 살아 있으면 `0x13c60618`이 `0x800f0000`이다. MFGPLL 약 265 MHz. gpusys를 켜면 SoC가 멈춘다(○) | 그쪽은 initcall에서 레지스터를 직접 쓴다. 우리는 `mtk-mfg-pmdomain` 변형 | 사실만 |
| `0ab6b6c`, `15a563c` | 2d Wi-Fi·BT | EMI: consys `0x8cc00000`(0xd00000), wfdma `0x8d900000`(0x1400000). SPM+`0xE04`가 `0xc200000d`이면 conn이 켜진 것이다. ROM 완료는 `0x18060A10` bit30. WF ROM은 아직 뜨지 않는다(○) | 그쪽은 벤더 gen4m과 conninfra(약 4만 줄), 우리는 mt76 | 사실만. OEM `soc7_0.c`와 대조 |
| PR #2 | 2b 핀 | EINT `0x11ce0000` 외 블록 넷, SPI 309, 250핀(✔ 펌웨어) | pinconf가 없다 | 높음. 우리 `pinctrl-mt6897`이 그쪽 터치 IRQ를 풀어 준다 |

겹치지 않는 블록: clk-mt6897(그쪽은 fixed-clock뿐), MFD `mtk-spmi-pmic`, 포고 키보드, 패널·화면 파이프라인, 열, 오디오.

## 4. 부팅과 기록

- **장치 트리를 커널 안에 넣는다**(✔ `arch/arm64/kernel/Makefile`, `setup.c`).
  - LK가 준 트리에서 `/memory`와 initrd만 읽는다.
  - 그 뒤 커널에 넣은 트리로 바꿔 끼우고, 그 트리의 `/chosen`에서 명령줄을 다시 읽는다.
  - 그래서 LK가 더한 bootargs와 mblock 예약은 버려진다(코드를 읽고 판단했다).
  - 이 구조는 MT6895 xaga 포트에서 물려받았다. 주석의 까닭은 이렇다: LK가 boot·vendor_boot 명령줄을 무시하고 `/chosen`을 자기 명령줄로 덮어쓴다(○). MT6897 LK에 우리처럼 vendor_boot DT 표로 트리를 넣어 시험한 기록은 없다.
- **initramfs:** boot.img에 붙인다고만 적었다. init_boot가 있을 때 LK가 boot.img의 램디스크를 쓰는지에 대한 근거는 없다.
- **`fastboot boot`:** 적힌 곳이 없다. 주석에 "멈추면 키를 눌러 fastboot로 되살린다"고 있어, boot 파티션에 굽는 것으로 보인다(추측).
- **UART:** 쓰지 않는다(✔ dts에서 uart0을 꺼 둠).
  - 주석에 적힌 까닭: 쓸 디버그 단자가 없고, uarthub 경로에는 mainline 드라이버가 없다.
  - 시험 패드 위치, 전압, 속도는 어디에도 없다.
  - "UART0이 키보드 배선"이라는 주석은 펌웨어와 맞지 않는다(✗). 키보드는 UART3(GPIO180·181)을 쓴다.
- **UART 대신 쓴 기록 방법**(✔ 코드와 주석으로 확인. 효과는 ○):
  1. LK 화면 위의 fbcon. `panic=0`으로 패닉 화면을 그대로 남긴다
  2. setup_arch 단계마다 화면에 블록을 그리는 카나리
  3. ramoops를 벤더와 같은 자리에 두고, 순정 recovery로 재부팅해 `/sys/fs/pstore`를 읽는다
  4. expdb 파티션에 남은 LK 기록을 recovery에서 dd로 뜬다
  5. USB 가젯(RNDIS, ACM)
- **recovery의 자리:** 이 기기에는 recovery 파티션이 없다(✔ 중국판 펌웨어 묶음에 recovery.img가 없다). recovery 램디스크는 vendor_boot 안에 있다. 그래서 우리 vendor_boot를 구운 슬롯에서는 순정 recovery를 쓸 수 없다.

## 5. 부트로더 값

- **살아 있는 트리, LK 기록, `/proc/iomem`:** 공개되지 않았다(✔ 두 저장소, PR, 포크, Villode의 다른 저장소를 모두 봤다). 원본 파일 이름(`board.dtb`, `expdb_full.bin`, `console-ramoops-0`)만 주석에 남아 있다.
- **framebuffer `0xfc16f000`:**
  - LK의 mblock 할당 기록과 `atag,videolfb` 값이 같다고 적었다(○). 이 주소로 fbcon이 보였다(○).
  - 크기 `0x3e90000`은 화면 세 장(`0x3dfe000`)보다 조금 크다(✔ 계산).
- **ramoops `0x48090000`:**
  - LK가 벤더 커널 명령줄에 넣는 값이다(○ 주석). 순정 이미지의 명령줄과 GKI 설정에는 없다(✔).
  - 명령줄의 `ramoops.*`와 DT 노드를 함께 두면 장치가 둘 생기고 하나만 돈다(○).
- **메모리 배치:** 살아 있는 트리의 mblock 표에서 뽑았다(○). 12GiB 기기의 값이라 8GB 판에는 그대로 쓸 수 없다(✔ 주소 계산).

## 6. tb375fc-linux-utils

- **진단 모듈:** insmod한 뒤 dmesg로 결과를 본다.
  - `diag_spi`, `spi_probe`, `peri_gate_poke`, `regdump`, `test_gpio`, `test_pm`: SPI2·IOCFG·pericfg_ao 레지스터를 보고 터치 SPI를 시험한다. `regdump`는 벤더 recovery 커널에 올려 LK가 남긴 상태를 읽는다.
  - `conn_watch`, `emi_peek`: connsys EMI와 conn 전원 레지스터를 읽기만 한다.
  - `conn_probe`: MPU 표를 읽는다. 특정 SMC를 부르면 AP가 멈춘다고 적었다(○).
- **initramfs:** tty0에 busybox 셸만 띄운다. nvdata를 마운트하지 않는다(✔ `initramfs/init.c`). busybox가 들어갈 `root/` 폴더도 저장소에 없다.
- **Wi-Fi 보정값:**
  - 커뮤니티 커널은 `mediatek/mt6897/WIFI`와 `BT_Addr`를 request_firmware로 읽는다(✔).
  - 원조 [MT6895 initramfs](https://github.com/MT6895-Mainline/initramfs)는 nvdata를 ext4 읽기 전용으로 붙이고, `APCFG/APRDEB/WIFI`와 `BT_Addr`를 펌웨어 폴더로 복사한다(✔).
  - Wi-Fi 펌웨어 파일은 두 저장소 어디에도 없다.

## 7. 라이선스와 연락

- tb375fc-linux:
  - README는 GPL-2.0이라고 적었다.
  - 새 dts는 `GPL-2.0 OR BSD-3-Clause`, 새 C 파일은 `GPL-2.0`이다. 저작권 줄은 없다(✔).
- utils: LICENSE가 GPL-2.0 본문이다. 일부 파일에는 SPDX가 없다(✔).
- 작성자와 연락 길:
  - 작성자는 Villode다. 저장소는 Villode 계정에서 u0n0u 조직으로 옮겨졌다.
  - 두 저장소 모두 이슈 트래커가 열려 있다(지금 0건). Discussions와 Wiki는 꺼져 있고, 공개 이메일은 없다.
  - 글은 중국어로 쓴다. 마지막 활동은 2026-09-25다.
- 나눌 만한 일:
  - `pinctrl-mt6897`: 그쪽 터치 IRQ를 풀어 준다
  - 터치 드라이버
  - 살아 있는 트리와 LK 기록
- 연락은 아직 하지 않았다.
