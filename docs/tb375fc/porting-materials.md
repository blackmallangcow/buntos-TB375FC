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
| **이 기기의 커뮤니티 mainline 포트** `u0n0u/tb375fc-linux`(main) | Linux 7.2 위에 `MT6895-Mainline/linux`의 `7.2-mt6895-xiaomi-xaga`(33f2f15a3)를 거쳐 자기 커밋 13개(머지 하나 포함)를 얹었다(GPL-2.0). 2026-09-23 뒤로 커밋이 없다. 장치 트리 `mt6897.dtsi`·`mt6897-lenovo-tb375fc.dts`(커널에 넣어 LK가 준 트리를 바꿔 끼운다), mfgcfg 클럭, scpsys 전원 도메인 + GPU EB, MT6685·MT6375 충전, nt36xxx 터치(BOE/TM), connectivity soc7_0 플랫폼이 들어 있다. README는 UART·저장장치(eMMC)가 된다고 적었지만, dts는 uart0을 끄고 UFS를 쓴다. 코드로 보면 LK 화면(fbcon), UFS, USB 가젯까지 됐고, 터치·Wi-Fi/BT·GPU·충전은 작업 중이다. Wi-Fi 보정값은 커널이 `mediatek/mt6897/WIFI`로 읽는다. 공개된 initramfs(`tb375fc-linux-utils`)는 nvdata를 마운트하지 않는다. 자세한 것은 [커뮤니티 포트 분석](community-port.md) | ✔ 코드 |
| **같은 SoC의 완전한 벤더 트리** | OPPO Pad 3(`oppo/mt6897_b_16.0_pad_3`, `oppo/mt6897_v_15.0.0_oppopad3`)와 OnePlus Pad(`oneplus/mt6897_b_16.0.0_oneplus_pad`)가 있다. MT8792(MT6897의 태블릿판) 보드, 포고 키보드, `mt8792-clkbuf`가 들어 있다. Motorola는 `MotorolaMobilityLLC/kernel-kernel_device_modules-6.1`, `vendor-mediatek-kernel_modules-connectivity-wlan-core-gen4m` 등으로 나뉘어 있고, 6.1.157로 가장 새것이다. SoC 표(클럭, 전원 도메인, 핀, SPMI)는 OnePlus와 OPPO가 같은 파일이고, Motorola도 전원 도메인 한 줄만 다르다 | ✔ 갈래·저장소, ✔ SoC 표, ○ 나머지 |
| **업스트림 견본** | `MT6895-Mainline/linux`(PMIC 조합 MT6363/MT6368/MT6685/MT6375/MT6319가 같다)에 clk, pinctrl, scpsys, SPMI, PMIC, LVTS, cpufreq-hw, UFS, Panthor가 있다. `MT6878-mainline/linux`는 v7.2 위 커밋 8개(clk-mt6878, mt6878.dtsi, CMF Phone 1)로 simplefb·UFS·USB2·cpufreq-hw까지 부팅하는 최소 견본이다 | ○ |

이 기기의 TB375FC는 사용자 공간에서 `mt8792`라는 이름을 쓴다(커뮤니티 기기 트리의 `fstab.mt8792`, ○). 장치 트리에 `mediatek,mt8792-clkbuf`가 있어 이와 맞는다(✔).

## 2. 블록별 표

크기는 작음(DT나 compatible만), 중간(새 드라이버 하나나 변형 추가), 큼(새 하위 시스템 지원이나 여러 드라이버)이다.

| 블록 | 가까운 업스트림 | 벤더·커뮤니티 참고 | 할 일 | 크기 | 단계 | 확인 |
|---|---|---|---|---|---|---|
| CPU·GIC·PSCI·타이머 | `arm,gic-v3`, `arm,psci-1.0`, `arm,armv8-timer` | 벤더 DT(GIC 4셀, PSCI idle 상태) | DT로 옮긴다 | 작음 | 2a | ✔ DT |
| CPU 주파수 | `drivers/cpufreq/mediatek-cpufreq-hw.c`(`mediatek,cpufreq-hw`) | 벤더 `mt6897.dts`의 `performance-controller@11bc00`, 도메인 셋(4+3+1), 전압은 MCUPM | DT와 더미 `cpu-supply` | 작음 | 2b·2e | ✔ 드라이버, ○ 벤더 노드 |
| 클럭(topckgen, apmixedsys, vlp_cksys, pericfg, ufscfg, mfgpll, HWV) | `clk-mt8196-*.c`(HWV, 6.18) | OEM 공식 `clk-mt6897.c`(127KB: apmixedsys PLL 11, topckgen, vlp_cksys 등), `clk-mt6897-peri.c`(pericfg_ao, ufscfg_ao·pdn), dt-bindings `mt6897-clk.h`. HW voter(`0x10320000`)를 PLL 다섯과 topckgen 먹스 열둘에 쓴다. 커뮤니티 포트의 mfgcfg는 GPU 실험 코드다 | `clk-mt6897` 새로 쓰기. 업스트림 HW voter는 FENC형 먹스뿐이라, FENC 없는 먹스(DONE 기준 `0x1C00`)와 PLL HW voter를 더한다. 패치 0009~0013으로 썼다(2026-11-01, 2b 블록 일곱). MFG PLL·GPU 먹스(2e), I2C 게이트(imp_iic_wrap), 화면·카메라 블록은 남았다 | 큼 | 2b | ✔ OEM 소스 |
| 전원 도메인 | `drivers/pmdomain/mediatek/mtk-pm-domains.c`(MT8196 데이터, 6.19), GPU는 `mtk-mfg-pmdomain.c` | OEM 공식 `mtk-scpsys-mt6897.c`: SPM 도메인 33개(`0x1c001000`)와 GPU_EB_RPC 12개(`0x13f91000`), 그 가운데 HW voter 도메인 7개. 버스 보호는 IFR·VLP. MT8196 틀에 그대로 맞는다. **UFS·USB 도메인은 없다.** 커뮤니티 포트는 옛 구조로 conn 도메인 하나만 썼다 | `mt6897-pm-domains.h`와 `mediatek,mt6897-power-controller`. 처음에는 `pd_ignore_unused` | 중간 | 2d 뒤 | ✔ |
| 핀 제어 | `drivers/pinctrl/mediatek/pinctrl-mt6878.c`(6.19), `pinctrl-mt8196.c`(paris) | OEM 공식 `pinctrl-mt6897.c`·`pinctrl-mtk-mt6897.h`: 핀 232개, 레지스터 기준 10개(펌웨어 pio와 같은 순서). EINT는 펌웨어 `mediatek,pins`에 141항목(가상 18개 포함)이 있고, 인스턴스 다섯, 전체 250개, GIC SPI 309다. 커뮤니티의 닫힌 PR #2(AI 생성, 시험 안 함)도 있다 | `pinctrl-mt6897` 새로 쓰기(mt6878을 틀로). 벤더 전용 기능은 업스트림 mt6878처럼 `DRV_ADV`로 바꾸거나 뺀다. GPIO 없는 EINT는 MT8196처럼 가상 핀. 패치 0004~0006으로 썼다(2026-11-01) | 중간 | 2b | ✔ |
| SPMI | `drivers/spmi/spmi-mtk-pmif.c`(MT8196 2-bus, 7.0) | 벤더 `spmi-mtk-pmif-core.c`의 레지스터 표가 업스트림 mt8195 표와 같다. PMIC 여섯은 모두 M 버스에 있다. 커뮤니티는 MT6895 데이터(버스 하나)로 닿았다 | `mediatek,mt8196-spmi` 대체 compatible, P 버스는 끈다, 별칭 `spmi0`. 업스트림은 자식마다 `rcs` 인터럽트와 클럭 셋을 요구한다. 고침 둘을 백포트했다(패치 0002·0003). 코드 대조로 찾은 결함 둘(rcs 디먹스 `bit / 7`, 채널 설정 순서)은 고침을 0007·0008로 얹었고 기기에서 확인한다. M 버스 노드와 MT6319는 장치 트리에 있다 | 작음 | 2b | ✔ |
| PMIC MT6363 | `drivers/regulator/mt6363-regulator.c`(6.19), auxadc(6.17). 둘 다 MFD의 자식이다 | MFD `mtk-spmi-pmic` v12(2026-05-11)는 미병합이고 2026-11-01에도 새 판이 없다(patchwork). BayLibre가 이어받겠다고 했다(2026-10-19). v12에는 리뷰 지적 말고도 인터럽트 마스크가 늘 첫 그룹에 쓰이는 결함이 있어 PWRKEY(48) 같은 인터럽트가 켜지지 않는다. MT6363 키는 업스트림에 없다(벤더는 같은 MediaTek 코드: OnePlus·Motorola). 7.2.12 `mt6363-regulator`는 master·regulator for-next·linux-next(20261030)와 같다. 16비트 값 regmap, VEMC 전압 읽기·쓰기, OC 처리, VSRAM 넷의 레지스터에 결함이 있다 | v12 3/5~5/5를 고쳐 백포트하고 MT6363 키를 새로 썼다(패치 0014~0018, 2026-11-01). regulator 결함은 패치 0037~0041이 고친다(업스트림에 낼 것) | 작음~중간 | 2b | ✔ |
| PMIC MT6319 ×3 | `mt6315-regulator.c`. 바인딩이 `mediatek,mt6319-regulator` + `mediatek,mt6315-regulator` 대체를 이미 받는다 | 벤더 `mediatek,mt6315_6-regulator`. 레지스터, 전압 범위, modeset 마스크가 업스트림과 맞는다 | DT만 쓴다(MFD가 필요 없다) | 작음 | 2b | ✔ |
| PMIC MT6368 | 없음. 패치 0023~0026이 regulator와 MFD 데이터를 더한다(7.2.12 `mt6363-regulator` 틀, 2026-11-01) | OEM 공식 `mt6368-regulator.c`·`mt6368-regulator.h`, `mfd/mt6368/core.h`·`registers.h`, `mt6368.dtsi`. MT6895 포크(커뮤니티)의 `mt6368-regulator.c`는 같은 MediaTek 코드다(헤더가 바이트까지 같다). 업스트림 MT6373 regulator v12와는 레일·레지스터가 달라 백포트하지 않았다 | 코덱·accdet·auxadc는 3(오디오) | 중간 | 2b·오디오 | ✔ regulator, ○ 코덱 |
| PMIC MT6685(RTC, clkbuf) | RTC v1과 클럭 v2(2025-10)는 미병합이다. 의존하던 SPMI 서브디바이스 API는 7.4 병합 창에 들어왔다 | MT6895 포크의 `mt6685-core.c` | MFD에 의존 | 중간 | 2b | ✔ 업스트림, ○ 포크 |
| MT6375(충전기, TCPC, ADC) | MT6370 계열(`mt6370.c`, `tcpci_mt6370.c`) | MT6895 포크의 `mt6375.c`, 충전기, TCPC | 새 드라이버 셋 | 중간~큼 | 2b·2e | ○ |
| 차지 펌프 NU2115 ×2, UPM6720 ×2 | 없음(`bq25980_charger.c`가 견본) | 벤더 | 고속 충전용이라 나중 | 중간 | 2e | ○ |
| 배터리 게이지 MM8013 | `drivers/power/supply/mm8013.c`(`mitsumi,mm8013`) | 커뮤니티도 같다 | compatible만 바꾼다 | 작음 | 2e | ✔ |
| UFS | `drivers/ufs/host/ufs-mediatek.c`(`mediatek,mt8183-ufshci`), `phy-mtk-ufs.c`. `IP_VER_MT6897`(6.17), `ufs-rtff-mtcmos`, `ufs-broken-rtc`, `ufs-disable-mcq`를 안다 | 펌웨어 노드는 `ufs-rtff-mtcmos`·`ufs-broken-rtc`를 쓰고 전원 도메인이 없다. MT6878 포크는 `mediatek,ufs-disable-mcq` | ufscfg 클럭·리셋, VCC. 처음에는 MCQ를 끈다(켤 때는 큐 인터럽트 8개). 바인딩은 클럭 `ufs` 하나만 받아 바인딩 패치가 필요하다. PHY는 `unipro`·`mp` 클럭이 필수인데 펌웨어 PHY 노드에는 클럭이 없다. 패치 0019~0022로 썼다(2026-11-01): 호스트 클럭 여섯·OPP·리셋 셋은 업스트림 MT8196 시리즈 v14(병합 전) 꼴, M-PHY는 벤더처럼 클럭·전원 순서 없는 새 판. 클럭 스케일링과 MCQ는 뒤로 | 작음~중간 | 2b | ✔ |
| USB | `mtu3`, `xhci-mtk`, `phy-mtk-xsphy.c` | 벤더 PHY `mediatek,mt6897-xsphy`@0x11e40000 | DT. 호스트 모드에는 MT6375 OTG의 VBUS가(2026-11-01 순정 트리에서 확인), 역할 전환에는 TCPC가 필요 | 작음 | 2b | ○ |
| 화면 파이프라인 | `drivers/gpu/drm/mediatek`(MT8195/MT8188). MT6897·MT8196 화면은 없다. 진행 중인 시리즈 둘은 최신 판이고 미병합이다(7.4용 pull에 없다): DSC·WDMA·MT8189/96 DSI v9(2026-09), "Huge Restructuring and MT8196" v2(2026-07). v9의 DSI는 패널이 준 RC 값을 DSC 1.2 값으로 덮어쓰므로, DSC 1.1 패널에는 맞춰야 한다. **dual-DSI는 맡은 사람이 없다** | 벤더 `mediatek_v2`: 패널은 **DSI0+DSI1 dual-DSI**, 링크당 1472×1840·4레인·1004Mbps, **DSC 1.1 8bpp**(슬라이스 736×20), 144/120/60/30Hz | SoC 데이터, dual-DSI, 실제 DSC. 첫 데스크톱은 부트로더가 켠 화면을 simple-framebuffer로 받는다 | 큼 | 2c | ✔ 시리즈, ✔ 벤더(미러) |
| 패널 NT36532 | `drivers/gpu/drm/panel/panel-novatek-nt36532.c`(`csot,ppc100hb1-1`, OnePlus Pad 2). 7.4 병합 창에 들어왔다(`47b823940e38`). 두 번째 DSI 링크(of_graph 포트 1)와 DSC PPS를 이미 다룬다. CSOT 설정은 DSC 1.2, 슬라이스 750×20 | 벤더 패널 드라이버 셋(초기화 순서). BOE 패널은 DSC 1.1, 슬라이스 736×20, 패널 고유 RC 표를 쓴다. TB371FC 커뮤니티 패치 | compatible 셋, 초기화 순서, DSC 1.1 설정 | 작음 | 2c | ✔ |
| 터치 NT36532(SPI) + 펜 | `drivers/input/touchscreen/novatek-nvt-ts.c`는 I2C 전용(`novatek,nt11205-ts`, `novatek,nt36672a-ts`). SPI 드라이버 RFC 둘(2024-10 NT36xxx, 2026-09 nt36672a-spi)은 미병합이고 NT36532와 펜을 다루지 않는다. 2024 RFC 바인딩에는 "compatible에서 spi를 빼고 기존 nt36672a에 넣지 못하는 까닭을 적으라"는 리뷰가 있었다 | OEM 공식 트리에 레노버의 NT36532 드라이버가 있다(`kernel_device_modules-6.1/drivers/input/touchscreen/NT36532`, 다른 레노버 기기 판). 이 기기 드라이버(제3자 미러)와 주소 표가 같고, 이 기기에서는 GPIO184로 패널을 가르고 `novatek_ts_fw_boe.bin`/`_tm.bin`을 쓴다. 보고 108바이트(손가락 16비트 좌표, 펜 14바이트), 펜 켜기 `0x7B 0x01`. 커뮤니티 포트의 nt36xxx는 26 MHz 자리 클럭 탓에 SCLK가 약 52 MHz였다 | 새 드라이버 `novatek-nvt-spi`와 바인딩 `novatek,nt36532-ts`(패치 0030·0031), SPI 바인딩의 MT6897(0029). 2026-11-01. 기기 확인 전 | 중간 | 2c | ✔ |
| 외부 화면(DP Alt) | `mtk_dp.c`(MT8195/MT8188), PS5169 v3 패치(PS5170 바인딩 포함) 미병합 | 벤더 `ps5170.c` | 화면 파이프라인, TCPC, 리드라이버가 먼저 | 큼 | 3 | ○ |
| GPU | panthor(G615 = arch 11.8, 6.18). `arm/mali/arch11.8/mali_csffw.bin`이 linux-firmware에 있다(20250917 릴리스부터, WHENCE에 재배포 가능으로 적혀 있다). MT8196은 ASN hash를 켠다 | 벤더 Mali avalon r44p1 | DT. ASN hash가 필요한지 확인한다 | 작음 | 2e | ✔ |
| GPU 전원·주파수(GPUEB) | `mtk-mfg-pmdomain.c`(`mediatek,mt8196-gpufreq`, 6.19), `mtk-gpueb-mailbox.c`(6.18). 다른 SoC를 더한 패치는 아직 없다 | 벤더 `gpufreq_mt6897.c`. IPI 명령 0~18이 같고 공유 메모리는 0xAC부터 다르다. 벤더는 GPU 쪽 전원 도메인 12개(GPU_EB_RPC)를 scpsys로도 다룬다. **gpueb.img는 LK가 올린다** | MT6897 변형(공유 메모리 오프셋, 메일박스 표) | 중간 | 2e | ✔ 드라이버, ○ 비교 |
| Mesa | Panfrost(GL), PanVK | G615 항목은 MR !41803(2026-08-27 병합)으로 26.3에 들어갔다. 26.3은 rc3(2026-10-30)이고, 11-04는 rc4나 정식이다. PanVK는 `PAN_I_WANT_A_BROKEN_VULKAN_DRIVER=1`이 있어야 뜬다 | 이미지에 Mesa 26.3 이상 | 작음 | 2e | ✔ |
| 오디오 AFE + MT6368 코덱 | `sound/soc/mediatek/mt8196`, `mt8189`, 코덱은 `mt6359.c` | 벤더 `mt6897-afe-*`, `mt6897-mt6368.c`, `mt6368.c`, `mt6368-accdet.c` | AFE, 코덱, accdet 새로 | 큼 | 3 | ○ |
| 앰프 AW883xx ×4 | `aw88395`(0x2049), `aw88399`(0x2183), `aw88166`(0x2066) | 벤더 `aw883xx`가 셋 다 받는다 | 칩 ID(레지스터 0x00) 확인, ACF 펌웨어 | 작음 | 3 | ✔ aw88166, ○ 나머지 |
| USB-C 이어폰 스위치 | `fsa4480.c` | — | DT. TCPC가 먼저 | 작음 | 3 | ○ |
| 센서 허브 SCP | `mtk_scp.c`(MT8183~MT8195, ChromeOS IPI) | 벤더 `mtk_nanohub` | 가속도·조도가 SCP 뒤라 사실상 막혀 있다 | 큼 | 3 | ○ |
| 홀 센서 ×2 | `gpio-keys`(`SW_LID`/`SW_TABLET_MODE`) | 펌웨어 `hall_1`(EINT1)·`hall_2`(EINT26). 제3자 미러 `hall.c`는 `hall_1`만 붙잡는다(GPIO1이 0이면 닫힘, 홀 IC AS1912). GPIO26은 포고 키보드 드라이버의 `plug-gpios`와 같은 핀이다 | 보드 gpio-keys 노드(꺼 둠). GPIO26의 뜻은 기기에서 | 작음 | 2c | ✔ 덮개, ○ GPIO26 |
| 진동 | `regulator-haptic` | 펌웨어: `regulator-vibrator`의 `vib-supply` = MT6368 VIBR(2.8~3.5V). 같은 레일이 카메라 `sensor0`의 `afvdd-supply`다 | DT(MT6368 `vibr` 노드와 함께. 실물 모터 확인) | 작음 | 3 | ✔ 레일, ○ 실물 |
| SAR | `drivers/iio/proximity/hx9023s.c`(`tyhx,hx9023s`, 칩 ID 0x1D) | 벤더 `hx9031as.h`가 `HX9023S_ON_BOARD`·0x1D. HX9036은 다른 칩 | hx9023s로 시도. HX9036은 새 드라이버 | 작음 / 중간 | 3 | ✔ 드라이버, ○ 칩 ID |
| 키보드 커버 | "OnePlus/OPPO Pad pogo-pin keyboards" v6(`oneplus-pogo-kbd.c`, Tinno MCU, serdev, 921600 반이중)가 최신 판이고 미병합이다. Qualcomm 태블릿용이고, 바인딩 수정 요구가 있고, 입력 유지자 리뷰는 없다. 프레임은 0x55×8, 0xF1, 주소, 명령, 길이, payload, CRC16-IBM(초기값 0xC596 고정), 0xFE, 0xAA×4다 | 벤더 `pogo_keyboard.dtsi`: `tinno,tn_keyboard`, UART3(GPIO181 TX, 180 RX), plug GPIO26, 전원 GPIO61, 깨우기 GPIO40, tx-en GPIO71, TX·RX 핀을 UART와 GPIO 사이에서 바꾸는 pinctrl 상태 일곱. 드라이버(`tn_keyboard.ko`) 소스는 공개되지 않았다. 벤더 8250 드라이버의 Tinno 훅이 OnePlus 공식 MT6897 트리와 같고, OnePlus MT6897 태블릿은 같은 프레임에 CRC 초기값 0xA5C9를 쓴다. UART3은 probe·startup·resume마다 pericfg_ao `0x11036050`의 비트를 고친다. **USB HID도 I2C도 아니다** | serdev 드라이버(v6 위). 같은 단선 프로토콜로 보고, CRC 초기값과 명령은 기기에서 확인한다. Lenovo PPP(0x7E) 후보는 근거를 찾지 못해 뺐다 | 중간 | 2c | ✔ dtsi·훅, ○ 프로토콜 |
| Wi-Fi·Bluetooth | mt76 `mt7921`(칩 ID 0x7961, PCIe), `mt7915/soc.c`(SoC Wi-Fi + SPI A-die) | soc7_0: OnePlus Pad A16·Motorola gen4m·OPPO Pad 3 갈래, Xiaomi rodin(MT6899). 커뮤니티 포트의 soc7_0 플랫폼. **펌웨어는 LK가 싣고, 보정값은 nvdata에 있다** | [Wi-Fi 설계](wifi-soc7_0.md) | 큼 | 2d | ✔ |
| 열 | `lvts_thermal.c` | 벤더 `mediatek,mt6897-lvts`@0x10315000(센서 16개) | MT6897 데이터 | 중간 | 2e | ○ |
| GenieZone | `drivers/virt/geniezone`은 미병합이다. v13(2024-11)이 아직 최신이고 마지막 활동은 2025-08이다. ACK(android14-6.1~android16-6.12)에는 있다. crosvm에 geniezone 백엔드가 있다 | `MT6895-Mainline/mtk-mod-tee-nogz`(ATF를 고쳐 EL2로 넘기는 실험, 서명 우회 필요, 권하지 않음) | 2a에서 EL 판정 뒤 결정 | 중간~큼 | 2a 뒤 | ○ |
| 비디오 코덱 | `mtk-vcodec`(MT8195까지), MT8196 v8 진행 중(VCP) | 벤더도 VCP | 나중 | 큼 | 범위 밖 | ○ |

## 3. 벤더 소스 목록

| 순위 | 출처 | 갈래 | 커널 | 쓰임 | 확인 |
|---|---|---|---|---|---|
| 1 | 이 기기 모듈 미러 `mt8792/android_kernel_lenovo_mt8792-modules` | `6.1.138` (그리고 `lineage-22.2`) | 6.1.138 | 이 기기 전용: 패널 셋, 터치, 앰프, 홀, SAR, 포고 키보드, GPU 주파수, 기본 DTS | ✔ |
| 2 | OPPO Pad 3, OnePlus Pad | `oppo/mt6897_b_16.0_pad_3`, `oneplus/mt6897_b_16.0.0_oneplus_pad` | 6.1.134 | MT8792 태블릿 보드, SoC 전부, gen4m soc7_0, conninfra, BT, Mali | ✔ 갈래, gen4m `chips/soc7_0`, conninfra·BT, SoC 표 |
| 3 | Motorola | `MotorolaMobilityLLC/kernel-kernel_device_modules-6.1`, `kernel-mtk`, `vendor-mediatek-kernel_modules-connectivity-wlan-core-gen4m`, `…-conninfra`, `…-bt-mt66xx`, `…-gpu` | 6.1.157 | 가장 새 MediaTek 수정 대조. Moto Pad 60 Pro 보드는 없다 | ✔ gen4m `chips/soc7_0`, SoC 표(OnePlus와 같은 파일), ○ 나머지 |
| 4 | OnePlus Nord CE5, OPPO Reno13/14, realme P3 Ultra | 같은 저장소의 다른 갈래 | 6.1.75~6.1.157 | 폰 보드. OPPO Reno13 A15 두 갈래에는 gen4m이 없다 | ○ |
| 5 | Transsion(Infinix GT 30 Pro 등) | 공식 포털, 미러 `hoshiyomiX/android_kernel_infinix_gt30pro` | 6.1.157 | SoC 전부와 soc7_0 | ○ |
| 6 | ASUS Pad T3201(12.2″ 태블릿) | `Edward-Projects/android_kernel_device_modules_asus_mt6897` | 6.1.145 | 태블릿 보드 `tb8792p1_64_wifi.dts`. Wi-Fi·Mali 모듈은 없다 | ○ |
| 7 | Xiaomi duchamp(Redmi K70E/POCO X6 Pro) | `MiCode/MTK_kernel_device_modules` 등 `bsp-duchamp-u-oss` | 6.1.25 | SoC 일부. gen4m soc7_0이 없다(✔) | ✔ soc7_0 없음 |
| — | Xiaomi rodin | `MiCode/MTK_kernel_modules` `bsp-rodin-v-oss` | — | gen4m `chips/soc7_0`이 있다. MT6899 기기다 | ✔ |
| — | Lenovo 공식 | 찾지 못함 | — | — | 미확인 |

라이선스:
- OEM 저장소에는 LICENSE 파일이 없고, 파일마다 SPDX가 GPL-2.0이다.
- SoC 파일과 dt-bindings 헤더도 `GPL-2.0` 단독이다. 업스트림 바인딩 헤더(`GPL-2.0-only OR BSD-2-Clause`)는 다시 쓴다.
- gen4m은 BSD-2-Clause(GPL 호환)이고, conninfra·Mali는 GPL-2.0이다(○).
- 옮겨 쓰는 코드는 출처와 원래 저작권 줄을 남기고 업스트림 꼴로 다시 쓴다.

## 4. 커널 버전

- mainline은 7.3(master는 7.4 병합 창)이다.
- buntOS 이미지의 Debian forky 커널은 **7.2.12**다(✔ 빌드 기록).
- 필요한 업스트림 기능과 들어온 판(2026-11-01 확인):

| 기능 | 판 |
|---|---|
| MT8196 pinctrl | 6.16 |
| MT6363·MT6373 auxadc, UFS `IP_VER_MT6897`·`ufs-broken-rtc` | 6.17 |
| panthor G615, MT8196 clk, GPUEB 메일박스 | 6.18 |
| MT6363 regulator, 전원 도메인 HWV·MT8196, `mtk-mfg-pmdomain`, pinctrl-mt6878 | 6.19 |
| SPMI v2 2-bus | 7.0 |
| NT36532 패널, SPMI 서브디바이스 API, pmif FSM 멈춤 우회 | 7.4 병합 창 |

- 7.2에는 패널 드라이버와 SPMI 고침 둘을 백포트한다.
- 병합되지 않은 것(MFD `mtk-spmi-pmic`, DSC·재구성 시리즈, 포고 키보드)은 패치 대기열에 둔다.
- MT6897·MT8792를 업스트림에 낸 사람은 아직 없다(linux-mediatek 보관소와 patchew 제목 검색). clk-mt6878과 mt6878.dtsi도 리스트에 나온 적이 없다.
- 커뮤니티 포트도 7.2라서 대조하기 쉽다.

## 5. 부트 체인과 복구

언락 go/no-go(설계안 2.1절)를 판단하려고 2026-11-01에 찾았다. LK와 펌웨어 묶음은 앞서 받아 둔 글로벌판 ZUI 17.0.04.266과 중국판 ZUI 16.0.20.091이다. XDA 글타래는 사이트가 자동 접근을 막아(403), 내장 브라우저로 원글과 1쪽·10쪽을 읽었다.

| 항목 | 내용 | 확인 |
|---|---|---|
| 언락 방식 | 두 판의 LK 문자열이 같다. 개발자 옵션의 OEM 토글, 일련번호에 묶인 레노버 서명 언락 이미지(머리·서명·일련번호 검사), `fastboot oem unlock`·`oem unlock-flash`·`flashing unlock`이 있다 | ✔ LK 문자열 |
| 언락 보고 | XDA 원글(2025-02-09, 방법을 만든 사람): 중국판 ZUI 16.0.20.022에서 순정 LK가 `fastboot flash lk`를 받는다. 개발용 펌웨어 덤프의 `lk_a`(MD5 `2c01c2ec…`)를 구운 뒤 `fastboot flashing unlock_critical`로 언락한다. 다른 사용자 여럿이 .022·.041에서 성공했다(2025-02). TrebleDroid 위키는 .091에서 언락한 뒤 GSI를 띄웠다(2025-08). 16.0.20.091에서는 성공 보고(2025-09, 2026-09)와 실패 보고(2025-12)가 함께 있다. 2026-09 보고는 구운 뒤 시스템으로 한 번 부팅하고 fastboot로 다시 들어가야 언락됐다. 글로벌 17.0.04.184에서 내리지 않고 된 보고도 있다(2025-10). 2026-06에 LineageOS 23.2가 나왔다. 개발용 LK의 출처(4pda 덤프)는 확인할 수 없다. 커뮤니티 포트 둘에는 언락 절차가 없다 | ✔ 원문(브라우저), 포럼 보고 |
| 펌웨어 묶음의 인증 | 중국판 묶음에 `da.auth`, 인증 DA `DA_BR.bin`, SP Flash Tool용 `SLA_Challenge`가 있다. DA 인증(DAA)과 SLA가 켜져 있다는 뜻이다 | ✔ 묶음 목록 |
| mtkclient | MT6897은 hwcode `0x1203`, DA 방식 XML(V6)이다. BROM 익스플로잇 값(blacklist, send_ptr, cmd_handler 등)과 payload가 모두 주석이다. 토론 #113의 채택된 답(2026-03-14): V6라 서명된 DA가 있어야 하고, 그래도 Carbonara나 Heapbait에 취약해야 한다 | ✔ 소스, ✔ 토론 |
| SP Flash Tool | 원글 작성자가 TB373FU 펌웨어의 DA·preloader·auth로 이 기기용 scatter와 readback 설정을 만들었다. SP Flash Tool로 개발용 ROM을 굽거나 백업을 되돌린 보고가 여럿이다. 중국판 백업을 되돌린 뒤 OTA가 되고 다시 잠글 수 있었다는 보고도 있다. 128GB 기기에 256GB 기기의 덤프를 구우면 실패한다 | ✔ 원문(브라우저), 포럼 보고 |
| ZUXOS(Android 16) | ZUXOS 1.5.10.x에서 바로 언락한 보고는 없다. 1.5.10.103 기기를 SP Flash Tool로 16.0.20.091까지 내린 뒤 언락한 보고가 있다(2026-09). Android 16 판에서 다른 판의 이미지를 구우면 "Red State"로 부팅하지 못한다는 보고가 여럿이다(롬 바꾸기 글타래 27~29쪽, 일부는 AI로 쓴 글). 1.5.10.044를 말한 글은 찾지 못했다 | ✔ 원문(브라우저), 포럼 보고 |
| 레노버 언락 파일 | 레노버가 발급한 sn.img와 개인이 만든 sn.img 웹 도구가 이 기기에서 인증에 실패했다는 보고가 둘 있다(2025-09, 2025-12) | ✔ 원문(브라우저), 포럼 보고 |
| 레노버 공식 복구 | Rescue and Smart Assistant(Lenovo Software Fix)가 있다. 이 기기의 ROM을 그 앱으로 받았다는 보고가 있다. 일련번호·레노버 서버에 묶이는지는 확인하지 못했다 | ○ 포럼 보고 |
| `fastboot boot` | LK fastboot 명령 표에 `download:`, `boot`, `flash:`, `erase:`, `getvar:`, `continue`, `set_active:`가 있다. LK는 boot·init_boot·vendor_boot 머리를 읽고 장치 트리를 `mb_dtb`로 넘긴다 | ✔ LK 문자열 |
| 헤더 v4의 장치 트리 | 장치 트리는 vendor_boot에 있고, 부트로더가 boot와 vendor_boot를 함께 읽어 램디스크를 잇는다. `fastboot boot`일 때 어떻게 잇는지는 문서에 정해져 있지 않다 | ✔ AOSP 문서 |

## 6. 출처

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
- 커뮤니티 포트: [커뮤니티 포트 분석](community-port.md)
- OEM SoC 표(2절): OnePlus 갈래 `oneplus/mt6897_b_16.0.0_oneplus_pad`의 `kernel/kernel_device_modules-6.1/` 아래 `drivers/clk/mediatek/clk-mt6897.c`, `drivers/soc/mediatek/mtk-scpsys-mt6897.c`, `drivers/pinctrl/mediatek/pinctrl-mt6897.c`, `drivers/spmi/spmi-mtk-pmif-core.c`
- NT36532 터치(OEM): 같은 OnePlus 갈래의 `kernel/kernel_device_modules-6.1/drivers/input/touchscreen/NT36532`, SPI 컨트롤러는 같은 트리의 `drivers/spi/spi-mt65xx.c`. 업스트림 리뷰(2024 NT36xxx RFC 바인딩, novatek-nvt-ts로 합칠지, regmap 페이지 접근, NT36672A e7t)의 링크는 터치 작업 보고 10절
- 업스트림 진행(2026-11-01 확인):
  - NT36532 패널 병합: https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/commit/?id=47b823940e38607dad180eb2f82094ae65565a4b
  - Mesa G615: https://gitlab.freedesktop.org/mesa/mesa/-/merge_requests/41803
  - 포고 키보드 v6: https://patchwork.kernel.org/project/linux-input/patch/20261016163859.1070138-3-wyibo6109@gmail.com/
  - Novatek SPI 터치 RFC: https://lore.kernel.org/all/20241015-nt36xxx-v1-0-3919d0bffee6@gmail.com/ , https://lore.kernel.org/all/20260911-ginkgo-submission-final-v1-6-0e68e63a18d4@gmail.com/
  - pinctrl-mt6878 v3: https://patchew.org/linux/20250915-mt6878-pinctrl-support-v3-0-593cc007d4cf@mentallysanemainliners.org/
  - MFD v12와 리뷰, 이어받기 제안: linux-mediatek 보관소 http://lists.infradead.org/pipermail/linux-mediatek/ (2026-05, 2026-10)
- 부트 체인과 복구(5절):
  - TrebleDroid 위키: https://github.com/TrebleDroid/treble_experimentations/wiki/Lenovo-Idea-Tab-Pro---Lenovo-Xiaoxin-Pad-Pro-2025---Motorola-Moto-Pad-60-Pro
  - XDA 비공식 언락 글타래(포럼, 2025-02~2026-10): https://xdaforums.com/t/unofficial-unlocking-lenovo-xiaoxin-pad-pro-2025-tb375fc.4717695/
  - XDA 롬 바꾸기 글타래(포럼, 2025-02~2026-10, SP Flash Tool 사용법과 Android 16 판의 보고): https://xdaforums.com/t/flashing-cn-global-rom-for-tb375fc-tb373fu-moto-pad-60-pro.4720245/
  - mtkclient: https://github.com/bkerler/mtkclient/blob/main/mtkclient/config/brom_config.py , https://github.com/bkerler/mtkclient/discussions/113
  - AOSP: https://source.android.com/docs/core/architecture/partitions/vendor-boot-partitions , https://android.googlesource.com/platform/system/core/+/refs/heads/main/fastboot/README.md
