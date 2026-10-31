# TB375FC 포팅 재료 조사

- 작성일: 2026-11-01. 기준은 mainline master(7.3 이후, 7.4 병합 창), linux-next 20261030, Mesa 26.3 rc다
- 방법:
  - 블록마다 가까운 업스트림 드라이버, 공개 벤더 소스(GPL), 커뮤니티 포트를 찾았다.
  - 가장 중요한 주장은 원본(커널 소스 파일, 저장소 갈래와 파일 목록, 기기 펌웨어)을 직접 열어 다시 확인했다.
- "확인" 열:
  - **✔**: 이 조사에서 원본을 직접 열어 확인했다.
  - **○**: 웹 조사 보고만 있고 다시 확인하지 않았다.
  - **미확인**: 찾지 못했거나 근거가 약하다.

## 1. 가장 중요한 재료

| 재료 | 내용 | 확인 |
|---|---|---|
| **이 기기의 벤더 커널 모듈 소스** `mt8792/android_kernel_lenovo_mt8792-modules`, 갈래 `6.1.138` | 레노버 공식 배포가 아니라 제3자 미러다(출처 미확인). 파일 8,599개이고, 커널 버전이 펌웨어의 GKI(`6.1.138-android14-11`)와 같다. 이 기기 전용 드라이버가 그대로 있다. 패널 `drivers/gpu/drm/panel/panel-nt36532-dsi-vdo-{boe,tianma,tianma-paper}.c`, 터치 `drivers/input/touchscreen/NT36532`, 앰프 `sound/soc/codecs/aw883xx`, `arch/arm64/boot/dts/mediatek/pogo_keyboard.dtsi`, `drivers/input/hall/hall.c`, SAR `drivers/input/sar/hx9031as.c`·`hx9036.c`, `drivers/misc/mediatek/typec/mux/ps5170.c`, `drivers/gpu/mediatek/gpufreq/v2/gpufreq_mt6897.c`, `arch/arm64/boot/dts/mediatek/mt6897.dts`. Wi-Fi(gen4m)·conninfra는 없다(별도 kernel_modules) | ✔ 파일 목록 |
| **이 기기의 커뮤니티 mainline 포트** `u0n0u/tb375fc-linux`(main) | Linux 7.2 위에 `MT6895-Mainline/linux`의 `7.2-mt6895-xiaomi-xaga`(33f2f15a3)를 거쳐 자기 커밋 12개를 얹었다(GPL-2.0). 장치 트리 `mt6897.dtsi`·`mt6897-lenovo-tb375fc.dts`, mfgcfg 클럭, scpsys 전원 도메인 + GPU EB, MT6685·MT6375 충전, nt36xxx 터치(BOE/TM), connectivity soc7_0 플랫폼, Wi-Fi/BT 개발 줄이 들어 있다. README의 상태: **UART 콘솔·저장장치 됨**, 터치·Wi-Fi/BT·GPU·충전 작업 중, 화면·카메라·오디오 시작 전. boot.img에 nvdata를 마운트하는 initramfs를 붙여 **Wi-Fi 보정값**을 얻는다(`tb375fc-linux-utils`) | ✔ README |
| **같은 SoC의 완전한 벤더 트리** | OPPO Pad 3(`oppo/mt6897_b_16.0_pad_3`, `oppo/mt6897_v_15.0.0_oppopad3`)와 OnePlus Pad(`oneplus/mt6897_b_16.0.0_oneplus_pad`)가 있다. MT8792(MT6897의 태블릿판) 보드, 포고 키보드, `mt8792-clkbuf`가 들어 있다. Motorola는 `MotorolaMobilityLLC/kernel-kernel_device_modules-6.1`, `vendor-mediatek-kernel_modules-connectivity-wlan-core-gen4m` 등으로 나뉘어 있고, 6.1.157로 가장 새것이다 | ✔ 갈래·저장소 있음, ○ 내용 |
| **업스트림 견본** | `MT6895-Mainline/linux`(PMIC 조합 MT6363/MT6368/MT6685/MT6375/MT6319가 같다)에 clk, pinctrl, scpsys, SPMI, PMIC, LVTS, cpufreq-hw, UFS, Panthor가 있다. `MT6878-mainline/linux`는 v7.2 위 커밋 8개(clk-mt6878, mt6878.dtsi, CMF Phone 1)로 simplefb·UFS·USB2·cpufreq-hw까지 부팅하는 최소 견본이다 | ○ |

이 기기의 TB375FC는 사용자 공간에서 `mt8792`라는 이름을 쓴다(커뮤니티 기기 트리의 `fstab.mt8792`, ○). 장치 트리에 `mediatek,mt8792-clkbuf`가 있어 이와 맞는다(✔).

## 2. 블록별 표

크기는 작음(DT나 compatible만), 중간(새 드라이버 하나나 변형 추가), 큼(새 하위 시스템 지원이나 여러 드라이버)이다.

| 블록 | 가까운 업스트림 | 벤더·커뮤니티 참고 | 할 일 | 크기 | 단계 | 확인 |
|---|---|---|---|---|---|---|
| CPU·GIC·PSCI·타이머 | `arm,gic-v3`, `arm,psci-1.0`, `arm,armv8-timer` | 벤더 DT(GIC 4셀, PSCI idle 상태) | DT로 옮긴다 | 작음 | 2a | ✔ DT |
| CPU 주파수 | `drivers/cpufreq/mediatek-cpufreq-hw.c`(`mediatek,cpufreq-hw`) | 벤더 `mt6897.dts`의 `performance-controller@11bc00`, 도메인 셋(4+3+1), 전압은 MCUPM | DT와 더미 `cpu-supply` | 작음 | 2b·2e | ✔ 드라이버, ○ 벤더 노드 |
| 클럭(topckgen, apmixedsys, vlp_cksys, pericfg, ufscfg, mfgpll, HWV) | `clk-mt8196-*.c`(HWV) | 벤더 `clk-mt6897*.c`(OPPO·Infinix 트리), 커뮤니티 포트는 mfgcfg만 | `clk-mt6897` 새로 쓰기. 처음에는 fixed-clock + `clk_ignore_unused` | 큼 | 2b | ○ |
| 전원 도메인 | `drivers/pmdomain/mediatek/mtk-pm-domains.c`(MT8196 데이터), GPU는 `mtk-mfg-pmdomain.c` | 벤더 `mtk-scpsys-mt6897.c`, 커뮤니티 포트(옛 구조) | `mt6897-pm-domains.h`와 `mediatek,mt6897-power-controller`. 처음에는 `pd_ignore_unused` | 중간 | 2b | ✔ mfg 드라이버, ○ 나머지 |
| 핀 제어 | `drivers/pinctrl/mediatek/pinctrl-mt6878.c`, `pinctrl-mt8196.c`(paris) | 벤더 `pinctrl-mt6897.c`(핀·EINT 표) | `pinctrl-mt6897` 새로 쓰기(mt6878을 틀로) | 중간 | 2b | ✔ mt6878 |
| SPMI | `drivers/spmi/spmi-mtk-pmif.c`(MT8196 2-bus) | 커뮤니티는 mt8195 레지스터 + `spmi_ver=2` | compatible 추가 | 작음 | 2b | ○ |
| PMIC MT6363 | `drivers/regulator/mt6363-regulator.c`, auxadc | MFD `mtk-spmi-pmic`는 미병합(v12, 2026-05) | MFD 패치를 대기열에 같이 둔다 | 작음~중간 | 2b | ✔ regulator, ○ MFD |
| PMIC MT6319 ×3 | `mt6315-regulator.c` | 벤더 `mediatek,mt6315_6-regulator` | compatible만 바꾼다 | 작음 | 2b | ○ |
| PMIC MT6368 | 없음(MT6359가 견본) | MT6895 포크의 `mt6368-regulator.c`, 벤더 오디오 | regulator, 코덱, accdet | 중간~큼 | 2b·오디오 | ○ |
| PMIC MT6685(RTC, clkbuf) | Collabora 패치 미병합 | MT6895 포크의 `mt6685-core.c` | MFD에 의존 | 중간 | 2b | ○ |
| MT6375(충전기, TCPC, ADC) | MT6370 계열(`mt6370.c`, `tcpci_mt6370.c`) | MT6895 포크의 `mt6375.c`, 충전기, TCPC | 새 드라이버 셋 | 중간~큼 | 2b·2e | ○ |
| 차지 펌프 NU2115 ×2, UPM6720 ×2 | 없음(`bq25980_charger.c`가 견본) | 벤더 | 고속 충전용이라 나중 | 중간 | 2e | ○ |
| 배터리 게이지 MM8013 | `drivers/power/supply/mm8013.c`(`mitsumi,mm8013`) | 커뮤니티도 같다 | compatible만 바꾼다 | 작음 | 2e | ✔ |
| UFS | `drivers/ufs/host/ufs-mediatek.c`(`mediatek,mt8183-ufshci`), `phy-mtk-ufs.c`. `ufs-mediatek.h`가 MT6897 IP 판을 안다 | MT6878 포크는 `mediatek,ufs-disable-mcq` | ufscfg 클럭·리셋, VCC, 전원 도메인 | 작음~중간 | 2b | ✔ |
| USB | `mtu3`, `xhci-mtk`, `phy-mtk-xsphy.c` | 벤더 PHY `mediatek,mt6897-xsphy`@0x11e40000 | DT. 역할 전환에는 TCPC가 필요 | 작음 | 2b | ○ |
| 화면 파이프라인 | `drivers/gpu/drm/mediatek`(MT8195/MT8188). MT6897·MT8196 화면은 없다. 진행 중인 시리즈: DSC·WDMA·MT8189/96 DSI v9(2026-09), "Huge Restructuring and MT8196" v2(2026-07). **dual-DSI는 맡은 사람이 없다** | 벤더 `mediatek_v2`: 패널은 **DSI0+DSI1 dual-DSI**, 링크당 1472×1840·4레인·1004Mbps, **DSC 1.1 8bpp**(슬라이스 736×20), 144/120/60/30Hz | SoC 데이터, dual-DSI, 실제 DSC. 첫 데스크톱은 부트로더가 켠 화면을 simple-framebuffer로 받는다 | 큼 | 2c | ○ |
| 패널 NT36532 | `drivers/gpu/drm/panel/panel-novatek-nt36532.c`(`csot,ppc100hb1-1`, OnePlus Pad 2). 이미 mainline master에 있다(7.4) | 벤더 패널 드라이버 셋(초기화 순서), TB371FC 커뮤니티 패치 | compatible 셋과 초기화 순서 추가 | 작음 | 2c | ✔ |
| 터치 NT36532(SPI) + 펜 | `drivers/input/touchscreen/novatek-nvt-ts.c`는 I2C 전용(`novatek,nt11205-ts`, `novatek,nt36672a-ts`) | 벤더 `NT36532`, 커뮤니티 포트의 nt36xxx(BOE/TM), 트리 밖 TB371FC-6.12. **플래시가 없는 칩이라 부팅마다 펌웨어(`novatek_ts_fw_boe.bin`/`_tm.bin`)를 내려 줘야 한다** | SPI 드라이버, 펌웨어 내려받기, 펜은 별도 입력 장치(압력·기울기) | 중간 | 2c | ✔ I2C 드라이버, ○ 나머지 |
| 외부 화면(DP Alt) | `mtk_dp.c`(MT8195/MT8188), PS5169 v3 패치(PS5170 바인딩 포함) 미병합 | 벤더 `ps5170.c` | 화면 파이프라인, TCPC, 리드라이버가 먼저 | 큼 | 3 | ○ |
| GPU | panthor(G615 = arch 11.8). `arm/mali/arch11.8/mali_csffw.bin`이 linux-firmware에 있다 | 벤더 Mali avalon r44p1 | DT | 작음 | 2e | ✔ 펌웨어, ○ 버전 |
| GPU 전원·주파수(GPUEB) | `mtk-mfg-pmdomain.c`(`mediatek,mt8196-gpufreq`), `mtk-gpueb-mailbox.c` | 벤더 `gpufreq_mt6897.c`. IPI 명령 0~18이 같고 공유 메모리는 0xAC부터 다르다. **gpueb.img는 LK가 올린다** | MT6897 변형(공유 메모리 오프셋, 메일박스 표) | 중간 | 2e | ✔ 드라이버, ○ 비교 |
| Mesa | Panfrost(GL), PanVK | G615 항목은 Mesa 26.3부터(26.3.0 2026-11-04 예정). PanVK는 실험 플래그 | 이미지에 Mesa 26.3 이상 | 작음 | 2e | ○ |
| 오디오 AFE + MT6368 코덱 | `sound/soc/mediatek/mt8196`, `mt8189`, 코덱은 `mt6359.c` | 벤더 `mt6897-afe-*`, `mt6897-mt6368.c`, `mt6368.c`, `mt6368-accdet.c` | AFE, 코덱, accdet 새로 | 큼 | 3 | ○ |
| 앰프 AW883xx ×4 | `aw88395`(0x2049), `aw88399`(0x2183), `aw88166`(0x2066) | 벤더 `aw883xx`가 셋 다 받는다 | 칩 ID(레지스터 0x00) 확인, ACF 펌웨어 | 작음 | 3 | ✔ aw88166, ○ 나머지 |
| USB-C 이어폰 스위치 | `fsa4480.c` | — | DT. TCPC가 먼저 | 작음 | 3 | ○ |
| 센서 허브 SCP | `mtk_scp.c`(MT8183~MT8195, ChromeOS IPI) | 벤더 `mtk_nanohub` | 가속도·조도가 SCP 뒤라 사실상 막혀 있다 | 큼 | 3 | ○ |
| 홀 센서 ×2 | `gpio-keys`(`SW_LID`/`SW_TABLET_MODE`) | 벤더 `hall.c`(GPIO 인터럽트, KEY 252/253) | DT | 작음 | 2c | ○ |
| 진동 | `regulator-haptic` | 진동 LDO가 어느 PMIC인지 미확인 | DT(실물부터 확인) | 작음 | 3 | ○ |
| SAR | `drivers/iio/proximity/hx9023s.c`(`tyhx,hx9023s`, 칩 ID 0x1D) | 벤더 `hx9031as.h`가 `HX9023S_ON_BOARD`·0x1D. HX9036은 다른 칩 | hx9023s로 시도. HX9036은 새 드라이버 | 작음 / 중간 | 3 | ✔ 드라이버, ○ 칩 ID |
| 키보드 커버 | "OnePlus/OPPO Pad pogo-pin keyboards" v6(`oneplus-pogo-kbd.c`, Tinno MCU, serdev, 921600 반이중) 리뷰 중 | 벤더 `pogo_keyboard.dtsi`: UART3(GPIO181 TX, 180 RX), plug GPIO26, 전원 GPIO61, 깨우기 GPIO40, tx-en GPIO71. **USB HID도 I2C도 아니다** | serdev 드라이버. 프레이밍(OnePlus형 0x55/0xF1/CRC16, Lenovo PPP 0x7E)부터 확인 | 중간 | 2c | ✔ dtsi 있음, ○ 내용 |
| Wi-Fi·Bluetooth | mt76 `mt7921`(칩 ID 0x7961, PCIe), `mt7915/soc.c`(SoC Wi-Fi + SPI A-die) | soc7_0: OnePlus Pad A16·Motorola gen4m·OPPO Pad 3 갈래, Xiaomi rodin. 커뮤니티 포트의 soc7_0 플랫폼. **보정값은 nvdata** | [Wi-Fi 설계](wifi-soc7_0.md) | 큼 | 2d | ✔ |
| 열 | `lvts_thermal.c` | 벤더 `mediatek,mt6897-lvts`@0x10315000(센서 16개) | MT6897 데이터 | 중간 | 2e | ○ |
| GenieZone | `drivers/virt/geniezone`은 미병합(v13, 2024-11). ACK에는 있다. crosvm에 geniezone 백엔드가 있다 | `MT6895-Mainline/mtk-mod-tee-nogz`(ATF를 고쳐 EL2로 넘기는 실험, 서명 우회 필요, 권하지 않음) | 2a에서 EL 판정 뒤 결정 | 중간~큼 | 2a 뒤 | ○ |
| 비디오 코덱 | `mtk-vcodec`(MT8195까지), MT8196 v8 진행 중(VCP) | 벤더도 VCP | 나중 | 큼 | 범위 밖 | ○ |

## 3. 벤더 소스 목록

| 순위 | 출처 | 갈래 | 커널 | 쓰임 | 확인 |
|---|---|---|---|---|---|
| 1 | 이 기기 모듈 미러 `mt8792/android_kernel_lenovo_mt8792-modules` | `6.1.138` (그리고 `lineage-22.2`) | 6.1.138 | 이 기기 전용: 패널 셋, 터치, 앰프, 홀, SAR, 포고 키보드, GPU 주파수, 기본 DTS | ✔ |
| 2 | OPPO Pad 3, OnePlus Pad | `oppo/mt6897_b_16.0_pad_3`, `oneplus/mt6897_b_16.0.0_oneplus_pad` | 6.1.134 | MT8792 태블릿 보드, SoC 전부, gen4m soc7_0, conninfra, BT, Mali | ✔ 갈래와 gen4m `chips/soc7_0`, ○ 나머지 |
| 3 | Motorola | `MotorolaMobilityLLC/kernel-kernel_device_modules-6.1`, `kernel-mtk`, `vendor-mediatek-kernel_modules-connectivity-wlan-core-gen4m`, `…-conninfra`, `…-bt-mt66xx`, `…-gpu` | 6.1.157 | 가장 새 MediaTek 수정 대조. Moto Pad 60 Pro 보드는 없다 | ✔ gen4m 저장소, ○ 나머지 |
| 4 | OnePlus Nord CE5, OPPO Reno13/14, realme P3 Ultra | 같은 저장소의 다른 갈래 | 6.1.75~6.1.157 | 폰 보드. OPPO Reno13 A15 두 갈래에는 gen4m이 없다 | ○ |
| 5 | Transsion(Infinix GT 30 Pro 등) | 공식 포털, 미러 `hoshiyomiX/android_kernel_infinix_gt30pro` | 6.1.157 | SoC 전부와 soc7_0 | ○ |
| 6 | ASUS Pad T3201(12.2″ 태블릿) | `Edward-Projects/android_kernel_device_modules_asus_mt6897` | 6.1.145 | 태블릿 보드 `tb8792p1_64_wifi.dts`. Wi-Fi·Mali 모듈은 없다 | ○ |
| 7 | Xiaomi duchamp(Redmi K70E/POCO X6 Pro) | `MiCode/MTK_kernel_device_modules` 등 `bsp-duchamp-u-oss` | 6.1.25 | SoC 일부. gen4m soc7_0이 없다(✔) | ✔ soc7_0 없음 |
| — | Xiaomi rodin | `MiCode/MTK_kernel_modules` `bsp-rodin-v-oss` | — | gen4m `chips/soc7_0`이 있다(✔). 기기 SoC는 미확인 | ✔ |
| — | Lenovo 공식 | 찾지 못함 | — | — | 미확인 |

라이선스:
- OEM 저장소에는 LICENSE 파일이 없고, 파일마다 SPDX가 GPL-2.0이다.
- gen4m은 BSD-2-Clause(GPL 호환)이고, conninfra·Mali는 GPL-2.0이다(○).
- 옮겨 쓰는 코드는 출처와 원래 저작권 줄을 남기고 업스트림 꼴로 다시 쓴다.

## 4. 커널 버전

- mainline은 7.3(master는 7.4 병합 창)이다.
- buntOS 이미지의 Debian forky 커널은 **7.2.12**다(✔ 빌드 기록).
- 필요한 업스트림 기능과 들어온 판(○):

| 기능 | 판 |
|---|---|
| panthor G615 | 6.18 |
| MT6363 regulator | 6.19 |
| 전원 도메인 HWV·MT8196·GPUEB | 6.19 |
| pinctrl-mt6878 | 6.19 |
| SPMI v2 2-bus | 7.0 |
| NT36532 패널 | 7.4 |

- 7.2는 패널 드라이버만 백포트하면 된다.
- 병합되지 않은 것(MFD `mtk-spmi-pmic`, DSC 시리즈)은 패치 대기열에 둔다.
- 커뮤니티 포트도 7.2라서 대조하기 쉽다.

## 5. 출처

- 이 기기 모듈 미러: https://github.com/mt8792/android_kernel_lenovo_mt8792-modules/tree/6.1.138
- 커뮤니티 포트: https://github.com/u0n0u/tb375fc-linux (README-tb375fc.md), https://github.com/Villode/tb375fc-linux-utils
- OPPO: https://github.com/oppo-source/android_kernel_modules_and_devicetree_oppo_mt6897 , OnePlus: https://github.com/OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6897
- Motorola: https://github.com/MotorolaMobilityLLC/kernel-kernel_device_modules-6.1 , https://github.com/MotorolaMobilityLLC/vendor-mediatek-kernel_modules-connectivity-wlan-core-gen4m
- Transsion: https://opensource.dev.infinixmobility.com , https://github.com/hoshiyomiX/android_kernel_infinix_gt30pro
- ASUS: https://github.com/Edward-Projects/android_kernel_device_modules_asus_mt6897
- Xiaomi: https://github.com/MiCode/MTK_kernel_modules , https://github.com/MiCode/MTK_kernel_device_modules
- 업스트림 견본: https://github.com/MT6895-Mainline/linux , https://github.com/MT6878-mainline/linux , https://wiki.postmarketos.org/wiki/MediaTek_MT6878
- 업스트림 소스: https://kernel.googlesource.com/pub/scm/linux/kernel/git/torvalds/linux/ (`drivers/gpu/drm/panel/panel-novatek-nt36532.c`, `drivers/ufs/host/ufs-mediatek.h`, `drivers/pmdomain/mediatek/mtk-mfg-pmdomain.c`, `drivers/iio/proximity/hx9023s.c`, `drivers/input/touchscreen/novatek-nvt-ts.c` 등)
- linux-firmware Mali: https://gitlab.com/kernel-firmware/linux-firmware/-/tree/main/arm/mali
- 진행 중 패치(○):
  - NT36532 v3: https://patchew.org/linux/20260804073419.84727-1-junjie.cao@linux.dev/
  - DSC/WDMA v9: https://lore.kernel.org/all/20260916110825.102631-1-angelogioacchino.delregno@collabora.com/
  - Huge Restructuring v2: https://lore.kernel.org/all/20260714114414.184512-1-angelogioacchino.delregno@collabora.com/
  - 포고 키보드 v6: https://lore.kernel.org/all/20261016163859.1070138-1-wyibo6109@gmail.com/
  - MT6363 MFD v12: https://patchwork.kernel.org/project/linux-mediatek/cover/20260511101355.122478-1-angelogioacchino.delregno@collabora.com/
  - GenieZone v13: https://patchwork.kernel.org/project/linux-mediatek/patch/20241114100802.4116-26-liju-clr.chen@mediatek.com/
- 트리 밖 태블릿 포트: https://github.com/oproskyrin/TB371FC-6.12 , https://github.com/MeteoricmeteorLL/tb710fu-linux , https://github.com/joonhoekim/tb323fu-linux
