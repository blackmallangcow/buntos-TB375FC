# TB375FC 장치 트리

TB375FC(MediaTek MT6897)의 mainline 장치 트리다. 설계안 [2.3절](../../../docs/tb375fc/DESIGN.md)의 방침대로 업스트림 꼴(`mt6897.dtsi` + 보드 dts)로 짰다. 지금 범위는 2a(첫 부팅)와 2b(저장장치·USB)의 자리다.

| 파일 | 내용 |
|---|---|
| `mediatek/mt6897.dtsi` | SoC. CPU·GIC·아키텍처 타이머·PSCI·PMU·워치독·cpufreq-hw는 켜 두고, UART(보드가 켠다)·systimer·UFS·USB·pio·SPMI·클럭 컨트롤러·SPI 여덟은 `disabled`로 둔다 |
| `mediatek/mt6897-lenovo-tb375fc.dts` | 보드. `chosen`, `memory`, UART0을 켠다. framebuffer·ramoops는 기기에서 확인할 때까지 `disabled`. SPMI M 버스에 MT6363(키, 레일 23개), MT6368(레일 6개), MT6319 셋을 두고, UFS 공급(`vcc` MT6363 VEMC, `vccq` MT6319 8/vbuck3)과 USB 공급(`vusb33` MT6368 VUSB)을 잇는다(SPMI는 2b까지 꺼져 있다) |
| `mediatek/mt6897-lenovo-tb373fu.dts` | 글로벌판(Idea Tab Pro). tb375fc.dts를 include하고 `model`·`compatible`만 바꾼다 |

- `build.sh`가 `mediatek/*.dts*`를 Linux 7.2.12의 `arch/arm64/boot/dts/mediatek/`에 복사하고, 패치 0001이 빌드 목록에 dtb 둘을 더한다.
- include는 7.2.12에 있는 `dt-bindings` 헤더(`arm-gic.h`, `gpio.h`, `input.h`, `irq.h`, `phy.h`, `spmi.h`)와 패치 대기열이 더하는 헤더만 쓴다. pio 바인딩과 `mt6897-pinfunc.h`는 패치 0004·0005가(0028이 그 include를 고친다), 클럭·리셋 번호 헤더(`mediatek,mt6897-clock.h`, `mediatek,mt6897-resets.h`)는 0009가 더한다. 보드 dts가 `mt6897-pinfunc.h`를 처음 쓴다.
- 라이선스는 업스트림 MediaTek DTS와 같은 `GPL-2.0-only OR BSD-2-Clause`다. 커뮤니티 포트의 글은 옮기지 않았고, 사실(주소·인터럽트)만 펌웨어 DT와 대조해 썼다.

## 검증

빌드 VM에서 Linux 7.2.12 원본과 그 dtc로 단독 컴파일한다.

```bash
limactl shell --workdir "$PWD" buntos-dev -- bash -lc 'K=~/buntos-build/tb375fc/linux-7.2.12; D=$PWD/kernel/tb375fc/dts; for b in tb375fc tb373fu; do cpp -nostdinc -undef -D__DTS__ -x assembler-with-cpp -I $K/include -I $K/arch/arm64/boot/dts -I $D $D/mediatek/mt6897-lenovo-$b.dts | ~/buntos-build/tb375fc/dtc-out/scripts/dtc/dtc -I dts -O dtb -Wunique_unit_address_if_enabled -Wnode_name_chars_strict -Wproperty_name_chars_strict -o /tmp/$b.dtb -; done'
```

| 검사 | 결과 (2026-11-01) |
|---|---|
| dtc 기본 검사 | 두 dtb 모두 오류·경고 0 |
| 커널 `W=1`·`W=2`와 같은 dtc 플래그 | 오류·경고 0 |
| `dt-validate`(dtschema 2026.9, 7.2.12 바인딩) | dtb마다 12건. 모두 아래 "바인딩에 더할 것"이다. 구조 오류는 없다 |
| 패치를 얹은 트리의 `make W=1 CHECK_DTBS=y`(2026-11-01, pio·SPMI·MT6319를 더한 뒤) | dtc 경고 0. dt-validate는 위 12건 그대로다. pio는 패치 0004의 바인딩을, MT6319는 `mt6315-regulator.yaml`을 통과한다 |
| 같은 검사(2026-11-01, 클럭 컨트롤러와 소비자 클럭을 더한 뒤) | dtc 경고 0. dt-validate는 위 12건 그대로다. 클럭 컨트롤러와 HW voter는 패치 0009·0010의 바인딩을 통과하고, UART·SPMI·USB·UFS의 클럭 속성에서 새 지적이 없다 |
| 같은 검사(2026-11-01, UFS 바인딩·드라이버(패치 0019~0022)와 UFS·USB 노드를 마친 뒤) | dtc 경고 0. dt-validate는 위 12건 그대로다(줄 단위로 같다). UFS 호스트·PHY는 패치 0019·0020의 바인딩을 통과한다. 클럭 이름·리셋 차례·`freq-table-hz`·PHY 클럭을 일부러 틀린 dtb는 지적을 받는다 |
| 같은 검사(2026-11-01, MT6363과 UFS 공급을 더한 뒤, 패치 0014~0022를 함께 얹어) | dtc 경고 0. dt-validate는 위 12건 그대로다. MT6363은 패치 0015·0017의 바인딩과 `mediatek,mt6363-regulator.yaml`을, `&ufshci` 공급은 패치 0020의 바인딩을 통과한다. 일부러 틀린 사본(키 인터럽트 이름, 없는 레일)은 스키마가 잡는다 |
| 같은 검사(2026-11-01, MT6368과 USB 공급을 더한 뒤, 패치 0014~0026을 함께 얹어) | dtc 경고 0. dt-validate는 위 12건 그대로다. MT6368은 패치 0023의 `mediatek,mt6368-regulator.yaml`과 0025의 MFD 바인딩을 통과한다. 일부러 틀린 사본(LDO·벅 모드, 없는 레일, MT6368의 `keys`, 틀린 compatible)은 스키마가 잡는다 |
| 같은 검사(2026-11-01, SPI 여덟과 2c 입력 노드를 더한 뒤, 패치 0027~0031을 함께 얹어) | dtc 경고 0. dt-validate는 위 12건 그대로다(dtb마다 24줄). SPI는 패치 0029의 `mediatek,spi-mt65xx.yaml`, 터치는 0030의 `novatek,nt36532-ts.yaml`, 핀 상태는 0004의 pio 바인딩, 홀 센서는 `gpio-keys.yaml`을 통과한다. 꺼진 노드에는 required와 모르는 속성을 묻지 않으므로, spi2를 켠 사본으로도 확인했다 |
| 같은 검사(2026-11-01, UART 패드 모드와 키보드 노드를 더한 뒤, 패치 0032~0036을 함께 얹어) | dtc 경고 0. dt-validate는 uart 넷의 8줄이 줄어 dtb마다 16줄이다(0035의 MT6897 compatible). 새 지적은 없다. 키보드는 패치 0032의 `oneplus,pogo-keyboard.yaml`로 본다 |
| 같은 검사(2026-11-01, I2C 14개와 USB 호스트 VBUS 노드를 더한 뒤, 패치 0042~0049를 함께 얹어) | dtc 경고 0. dt-validate는 dtb마다 16줄 그대로다. 새 노드를 모두 켜고 `dr_mode = "otg"`로 바꾼 사본에도 새 지적이 없다 |
| 같은 검사(2026-11-01, `spm`·`scpsys`·`ifr_bus`·`vlpcfg_bus`를 더한 뒤, 패치 0050~0054를 함께 얹어) | dtc 경고 0. dt-validate는 줄 단위로 그대로다. 켠 사본, 도메인 32개를 모두 적은 사본도 새 줄 0. 틀린 사본 다섯(voter 없음, access-controllers 없음·하나, 틀린 compatible 둘)은 스키마가 잡는다 |
| 펌웨어 DT와 대조 | 켠 노드와 자리만 둔 노드의 `reg`·`interrupts`가 모두 같다(아래 표의 예외만 일부러 다르다) |

패치를 얹은 트리로 보려면 `build.sh prepare` 뒤 다음을 돌린다(바인딩 스키마는 `out/`에 만든다).

```bash
limactl shell buntos-dev -- bash -lc 'export PATH=~/buntos-build/tb375fc/venv-dtschema/bin:$PATH; make -C ~/buntos-build/tb375fc/linux-7.2.12-tb375fc O=~/buntos-build/tb375fc/out ARCH=arm64 LOCALVERSION= W=1 CHECK_DTBS=y mediatek/mt6897-lenovo-tb375fc.dtb mediatek/mt6897-lenovo-tb373fu.dtb'
```

dtschema는 VM의 venv(`~/buntos-build/tb375fc/venv-dtschema`)에만 설치했다. pylibfdt를 빌드할 Python 헤더는 `apt-get download libpython3.13-dev`로 받아 venv 안에 풀었다(시스템 패키지는 그대로다).

## 출처

| 표시 | 자료 |
|---|---|
| 펌웨어 | 글로벌판 순정 펌웨어 ZUI 17.0.04.266의 SoC DTB(vendor_boot)에 보드 오버레이(dtbo)를 입혀 푼 트리. 푸는 법은 [하드웨어 실측](../../../docs/hw/lenovo-tb375fc.md) 1절. 중국판 ZUI 16.0.20.091도 이 값들이 같다 |
| 벤더 | MediaTek 벤더 `arch/arm64/boot/dts/mediatek/mt6897.dts`와 `mt6897-pinfunc.h`(이 기기 모듈 미러 `mt8792/android_kernel_lenovo_mt8792-modules`, 갈래 6.1.138) |
| 커뮤니티 | [u0n0u/tb375fc-linux](https://github.com/u0n0u/tb375fc-linux)의 `mt6897.dtsi`·`mt6897-lenovo-tb375fc.dts`(GPL-2.0 OR BSD-3-Clause). 부트로더가 넘긴 트리와 LK 기록에서 뽑은 값이 주석에 있다. 그 원본 파일(board.dtb, expdb_full.bin, console-ramoops-0)은 공개되지 않았다([분석](../../../docs/tb375fc/community-port.md)) |
| 7.2.12 | Linux 7.2.12의 드라이버와 바인딩, 업스트림 MediaTek dtsi(mt8195, mt8188)의 관례 |
| OEM 드라이버 | OnePlus 공식 MT6897 커널(`OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6897`, 갈래 `oneplus/mt6897_b_16.0.0_oneplus_pad`)의 UFS 호스트·M-PHY·xsphy·mtu3 드라이버. 펌웨어 속성을 드라이버가 어떻게 쓰는지 대조했다 |
| 업스트림 진행 | lore의 "MediaTek UFS Cleanup and MT8196 Enablement" v14(2026-10-16, 병합 전)와 그 리뷰. UFS 바인딩의 클럭·리셋 이름과 OPP를 따랐다 |

- 부트로더가 넘긴 살아 있는 트리(board.dtb)는 공개 저장소(`u0n0u/tb375fc-linux`, `Villode/tb375fc-linux-utils`)에서 찾지 못했다(2026-11-01). 그래서 부트로더가 더하는 값(framebuffer, 예약 메모리, 메모리 크기)은 커뮤니티 dts의 주석을 후보로만 쓰고 기기에서 확인한다.
- 펌웨어에서 푼 파일은 로컬 `.handoff/tb375fc/ref/`에 있고 저장소에는 넣지 않는다.

### SoC (`mt6897.dtsi`)

| 노드 | 값 | 출처 | 메모 |
|---|---|---|---|
| `/`, `/soc` | `#address-cells`·`#size-cells` 2/2, `soc`의 `dma-ranges` 64GiB | 펌웨어 | 벤더·커뮤니티도 같다. 부트로더가 `/memory`·`/reserved-memory`를 이 셀 수로 고친다 |
| `cpus` | A510 ×4(MPIDR 0x000~0x300), A715 ×4(0x400~0x700), `enable-method = "psci"` | 펌웨어 | 노드 이름은 업스트림 규칙대로 `reg`에 맞췄다. 펌웨어는 `cpu@0~3`, `cpu@100~102`, `cpu@200`이라 `reg`와 다르다 |
| `capacity-dmips-mhz` | A510 338, A715 1080, 마지막 A715 1024 | 펌웨어 | |
| `performance-domains` | 0(A510 넷), 1(A715 셋), 2(A715 하나) | 펌웨어 | |
| `cpu-map` | 클러스터 하나(core0~7) | 7.2.12 | DynamIQ 한 덩어리라 mt8195·mt8188처럼 쓴다. 펌웨어는 DVFS 도메인대로 클러스터 셋이다 |
| `idle-states` | cpu-off `0x00010000`, cluster-off `0x01010001`, 코어 종류별(l/m/b) 지연값 | 펌웨어 | mcusys-off(`0x02010007`)와 system 상태(`0x0201000f`~`0x020180ff`)는 SPM이 조율하므로 뺐다 |
| `psci` | `arm,psci-1.0`, `smc` | 펌웨어 | |
| `pmu-a510`, `pmu-a715` | PPI 7, `ppi_cluster0`(cpu0~3), `ppi_cluster1`(cpu4~7) | 펌웨어 | 펌웨어는 `arm,armv8-pmuv3`. 7.2.12가 아는 코어별 compatible로 바꿨다 |
| `gic` | GICD `0x0c400000`(0x40000), GICR `0x0c440000`(0x200000), 4셀, PPI 9, ppi-partitions | 펌웨어 | 아래 "GIC 기준 주소" |
| `timer` | PPI 13·14·11·10 `LEVEL_HIGH`, 13 MHz | 펌웨어 | 커뮤니티는 `LEVEL_LOW`로 적었으나 펌웨어 값(4)은 HIGH다. `clock-frequency`는 mt8188처럼 둔다 |
| `clk26m`, `clk13m` | 26 MHz, 26 MHz ÷ 2 | 펌웨어, 7.2.12 | 펌웨어의 `clk13m`은 13 MHz fixed-clock이다. 업스트림처럼 fixed-factor로 썼다 |
| `systimer` | `0x1cc10000`, SPI 330, `clk13m` | 펌웨어 | 깊은 idle에서 쓸 broadcast 타이머. 펌웨어의 인터럽트 셋 가운데 첫째만 쓴다(바인딩이 하나). 출하 GKI는 업스트림 SYST 드라이버를 쓰지 않으므로(`# CONFIG_MTK_TIMER is not set`) 2a에서는 `disabled`다. 그동안은 hrtimer broadcast가 대신한다 |
| `uart0`~`uart3` | `0x11001000`~`0x11004000`(0x1000), SPI 250~253 | 펌웨어 | `baud`는 clk26m. 7.2.12 `8250_mtk`는 `baud`가 있으면 `bus`도 요구해 하나만 주면 probe가 실패한다. `bus`는 펌웨어처럼 pericfg_ao의 UART 게이트다. 다만 uart0은 2a 콘솔이라 클럭 컨트롤러를 기다리지 않게 2b까지 clk26m을 둔다. `uart3`에는 `mediatek,syscon-pad-mode = <&pericfg_ao 0x50>`을 둔다(패치 0035·0036). 이 UART는 패드나 UART 허브로 나가고, OEM `uarthub` 코드로 bit 4가 1이면 패드다. MT6897은 허브를 쓰지 않는다(펌웨어 `uarthub-disable`) |
| `i2c0`~`i2c13` | 펌웨어와 같은 주소·AP-DMA·SPI 186~199, `clock-div = 1` | 펌웨어, OEM 드라이버 | `disabled`. compatible은 `mediatek,mt6897-i2c` + 업스트림 `mediatek,mt8188-i2c`(패치 0042). 벤더 `mt6897_compat`이 mt8188 데이터와 같다. `main` = imp_iic_wrap 게이트, `dma` = pericfg_ao `DMA_BCLK`. 펌웨어처럼 i2c6 말고는 핀을 설정하지 않는다(LK가 먹스한다). 벤더의 `mediatek,fifo-use-polling`·`scl-gpio-id`는 뺐다 |
| `imp_iic_wrap_{c,en,es,n,s}` | `0x11281000`, `0x11b74000`, `0x11cc4000`, `0x11f02000`, `0x11d03000`(각 0x1000) | 펌웨어 | `disabled`. 게이트 번호는 펌웨어의 소비자와 같다. 부모는 topckgen `i2c`(HW voter) |
| `spm` | `0x1c001000`(0x1000), `mediatek,mt6897-scpsys` + syscon + simple-mfd | 펌웨어(`scpsys`) | 업스트림 꼴로 나눴다: 부모는 레지스터(syscon), 자식 `scpsys`가 제어기다. 펌웨어는 한 노드다. 업스트림 dtsi는 부모를 `scpsys`, 자식을 `spm`이라 부르는데, 소비자가 펌웨어처럼 `<&scpsys …>`로 쓰게 라벨을 바꿔 달았다 |
| `scpsys` | `mediatek,mt6897-power-controller`, `access-controllers` = IFR·VLP, `mediatek,hardware-voter` = `hwv` | 펌웨어, 패치 0050 | 도메인은 첫 소비자가 생길 때 적는다(지금 conn). 벤더 번호 그대로라 부모가 자식보다 작다. 자식 노드의 `status`는 드라이버가 보지 않으니(`scpsys_add_subdomain`) 켜지 않으려면 노드를 적지 않는다 |
| `ifr_bus`, `vlpcfg_bus` | `0x1002c000`, `0x1c00c000`(0x1000) | 펌웨어 | 버스 보호. 늘 꺼 둔다: 드라이버가 regmap으로 쓸 때는 `status`를 보지 않고(`device_node_to_regmap`), 켜면 드라이버 없는 장치가 생겨 fw_devlink가 `access-controllers`를 보고 `scpsys`를 deferred probe 시간 한도(10초)까지 붙든다 |
| `dsi0`, `dsi1` | `0x1400d000` SPI 368, `0x1420d000` SPI 412 | 펌웨어 | `disabled`. compatible·클럭·PHY는 화면 파이프라인과 함께 더한다 |
| `wifi`, `consys` | `wifi@18000000`(SPI 889, 둘째 `reg`는 PCCIF가 있는 `0x10001000`), `consys@18001000`(`cfg-on`, `iocfg-rt`, conn 도메인), LK가 읽는 크기 | 펌웨어, 벤더 | `disabled`(2d). LK는 경로 `/soc/consys`, `/soc/wifi`로 찾고 유닛 주소와 status를 보지 않는다. 유닛 주소가 겹치지 않게 consys를 `18001000`으로 뒀다 |
| `bluetooth` | `bluetooth@18800000`(`bgf`, `conn`, SPI 614·891) | 펌웨어, 벤더 | `disabled`(2d). LK는 이 노드를 찾지 않는다. 트리의 892는 BEIF 빌드에서 쓰지 않는다 |
| `spi0`~`spi7` | `0x11010000`~`0x11017000`(0x100), SPI 242~249, `mediatek,pad-select` 0(spi0~5)·1(spi6·7) | 펌웨어, OEM 드라이버 | `disabled`. compatible은 `mediatek,mt6897-spi` + 업스트림 `mediatek,mt6991-spi`(패치 0029). OEM `spi-mt65xx`의 `mt6985` 데이터와 업스트림 `mt6991` 데이터가 같은 기능이다. 클럭은 펌웨어처럼 `parent-clk` = topckgen `mainpll_d6_d2`(spi0~2)·`univpll_d6_d2`(spi3~7), `sel-clk` = spi 먹스, `spi-clk` = pericfg_ao 게이트다. 드라이버가 실제 클럭 값으로 분주를 계산한다(커뮤니티 포트는 26 MHz 자리값 때문에 SCLK가 약 52 MHz였다). OEM의 소프트웨어 CS는 업스트림이 쓰지 않는다 |
| `watchdog` | `0x1c00a000`(0x100) | 펌웨어 | 7.2.12 `mtk_wdt`는 대체 compatible `mediatek,mt6589-wdt`로 붙고, 켜져 있으면 `WDOG_HW_RUNNING`으로 받아 커널이 쓰다듬는다. 펌웨어의 `syscon`·`simple-mfd`·reboot-mode는 뺐다 |
| `performance` | `0x11bc10`·`0x11bd30`·`0x11be50`(각 0x120) | 펌웨어 | 노드 이름을 첫 `reg`에 맞췄다(펌웨어 `@11bc00`). mt8195와 같은 CSRAM 배치에 도메인이 하나 더 있다 |
| `reserved-memory` | 틀(셀 2, `ranges`) | 설계안 2.3 | 고정 예약(ATF, TEE, GenieZone, connsys 등)은 부트로더가 `mblock-*` 노드로 더한다 |
| `ufshci`, `ufsphy` | `0x112b0000`(0x3000) SPI 168, `0x112a0000`(0x10000) | 펌웨어, OEM 드라이버, 업스트림 진행 | `disabled`. compatible은 `mediatek,mt6897-ufshci`·`mediatek,mt6897-ufsphy`(패치 0019~0022, 대체 없음). 호스트 클럭 여섯: `main`·`aes` = ufscfg_pdn 게이트, `unipro_sysclk`·`tx_symbol`·`rx_symbol0`·`rx_symbol1` = ufscfg_ao 게이트. `freq-table-hz` 대신 OPP 둘(벤더의 `ufs` 먹스 273·458 MHz, `aes_ufsfde` 416·546 MHz). 리셋 셋 `unipro`·`crypto`·`hci`(ufscfg_pdn). 이름과 OPP는 업스트림 v14 꼴이다. 펌웨어의 먹스와 그 최소·최대 부모 여섯은 업스트림 리뷰대로 넣지 않아 클럭 스케일링은 없다. rtff-mtcmos·broken-rtc·pmc-via-fastauto·tx-skew-fix는 compatible이 뜻한다. `mediatek,ufs-disable-mcq`로 시작한다. MCQ 큐 인터럽트 SPI 177~184는 공통 바인딩(`ufs-common.yaml`)이 인터럽트를 하나만 받아 아직 넣지 못한다. PHY는 벤더가 `mphy-ver = <1>`로 다루는 새 판이라 클럭과 전원 순서가 없다. M-PHY 리셋(ufscfg_ao bit 8)은 바인딩만 받고, 벤더도 쓰지 않아 넣지 않았다 |
| `ssusb`, `xhci0` | mac `0x11201000`(0x2e00) + ippc `0x11203e00`(0x100) SPI 160, xHCI `0x11200000` SPI 161, xHCI `sys_ck` = clk26m | 펌웨어 | `disabled`. mtu3 클럭: `sys_ck` = topckgen `usb_top`, `xhci_ck` = `ssusb_xhci`(펌웨어의 `host_ck` 자리), `frmcnt_ck` = pericfg_ao 게이트. MT6897에 없는 `ref_ck`·`mcu_ck`·`dma_ck`는 바인딩의 이름 차례 때문에 clk26m으로 자리를 채운다. `maximum-speed = "super-speed"`(펌웨어). 펌웨어의 깨우기 접점(`mediatek,syscon-wakeup = <pericfg_ao 0x200 103>`)은 넣지 않았다. 벤더의 103(mt6983 계열: CTRL0 bit 0·1, CTRL2 0x699)과 업스트림의 103(MT8195 IP0: CTRL1 `0x20`)이 다른 꼴이다 |
| `xsphy` | 공유 `0x11e43000`(0x200), U2 `0x11e40000`(0x400), U3 `0x11e43400`(0x500), `ref` = clk26m | 펌웨어, OEM 드라이버 | `disabled`. 업스트림 이름 `xs-phy@`/`usb-phy@`로 바꿨다. U2에 펌웨어의 `mediatek,eye-vrt = <7>`·`mediatek,eye-term = <3>`(업스트림과 같은 칸). 내부 저항 보정(`mediatek,efuse-intr = <0x25>`, efuse 값이 있으면 그것)은 넣지 않았다. MT6897 U2의 INTR_CAL은 `U2PHY_COM+0x30`의 [29:24]인데, 업스트림 드라이버는 이 값을 `+0x04`의 [23:19]에 쓴다. efuse는 nvmem이 아니라 LK가 넘기는 devinfo(`mediatek,devinfo`, `reg` 없음)라 efuse 노드도 두지 않았다 |
| `pio` | `0x10005000` 외 핀 설정 영역 9개, EINT 영역 다섯(`0x11ce0000`·`0x11de0000`·`0x11e90000`·`0x11fe0000`·`0x1c01e000`), GIC SPI 309 | 펌웨어(pio와 EINT 노드) | `disabled`(2b에서 SPMI와 함께 켠다). 펌웨어는 핀과 EINT가 다른 노드(`mediatek,mt6983-eint`)다. 업스트림 MT6878처럼 한 노드로 합쳤다. `gpio-ranges`는 실제 핀 232개이고, GPIO가 없는 EINT 232~249는 드라이버가 가상 핀으로 둔다 |
| `spmi` | M 버스: pmif `0x1cc04000` + spmimst `0x1cc01000`, P 버스: pmif `0x1cc06000` + spmimst `0x1cc00000` | 펌웨어 | `disabled`. 펌웨어는 한 노드에 영역 넷이다. 7.2.12의 MT8196(SPMI 2.0, 버스 둘) 꼴로 나눴다. pmif 크기는 펌웨어 `0x8ff`를 `0x900`으로 썼다(`spmi-pmif-mpu`가 `0x1cc04900`에서 시작). PMIC 여섯은 모두 M 버스에 있다(펌웨어에 `spmi-dev-mask`가 없어 벤더 드라이버가 모두 M 버스로 보낸다). 그래서 P 버스는 끈다. M 버스에는 `rcs` 인터럽트(pio EINT 248, 펌웨어 `rcs_irq`와 같다)와 클럭 셋을 준다. 클럭은 vlpckgen의 `pwrap_ulposc`(PMIF 둘)와 `spmi_m_mst`(SPMI 마스터)다. MT6897에는 MT8195의 infracfg_ao PMIF 게이트가 없어(벤더 SPMI 드라이버도 클럭을 받지 않는다) 그 부모 먹스 자체를 준 추론이다. 두 먹스 모두 게이트가 없어 켜기는 아무 일도 하지 않는다 |
| 클럭 컨트롤러 | topckgen `0x10000000`, apmixedsys `0x1000c000`, pericfg_ao `0x11036000`, ufscfg_ao `0x112b8000`, ufscfg_pdn `0x112bb000`, vlpckgen `0x1c013000`(각 0x1000), HW voter syscon `0x10320000`(0x2000) | 펌웨어 | `disabled`(2b에서 pio·SPMI와 함께 켠다). 펌웨어처럼 topckgen·apmixedsys·pericfg_ao만 `mediatek,hardware-voter`를 가리킨다. ufscfg 둘은 UFS 리셋도 낸다(`#reset-cells`) |
| `clk32k`, `ulposc` | 32000Hz, 588MHz | 펌웨어 | 고정 클럭. `clk32k`는 펌웨어 값이다(32768이 아니다). `ulposc`는 펌웨어 트리에 바이트 `23 0c 2b 00`(=588000000)으로 있다 |

**GIC 기준 주소.** 커뮤니티 dtsi는 펌웨어 값(`0x0c400000`)을 쓰면서, 업스트림 MTK 계열(MT8183~MT8195)처럼 `0x0c000000`/`0x0c040000`을 대안으로 적어 두었다. 이 트리는 펌웨어 값을 쓴다.
- 펌웨어 SoC 트리와, 커뮤니티가 읽은 부트로더가 넘긴 트리가 모두 `0x0c400000`이다.
- MT6897에서 `0x0c000000`부터는 MCUSYS다. 펌웨어 트리에 `mcusys-ao-cfg`(`0x0c000000`), `mediatek,gic500`(`0x0c000000`, 4MiB 디버그 창), CPU PLL(`0x0c030000`~), MCUPM(`0x0c070000`)이 있다. `mediatek,gic_cpu`는 `0x0c400000`을 가리킨다.
- 커뮤니티 포트는 이 값으로 화면 콘솔과 저장장치까지 돌았다. 인터럽트가 안 들어오면 둘 다 안 된다.
- 커뮤니티 보드 dts 머리 주석의 "GIC 0xc000000 + 재분배 0xc400000"은 위 사실과 맞지 않는다.

### 보드 (`mt6897-lenovo-tb375fc.dts`)

| 노드 | 값 | 출처 | 메모 |
|---|---|---|---|
| `model`, `compatible` | `lenovo,tb375fc`, `mediatek,mt6897` | 설계안 | TB373FU는 `lenovo,tb373fu` |
| `chosen` | `stdout-path = "serial0:921600n8"` | MTK 관례 | 부트로더가 `bootargs`, initrd, `kaslr-seed`, `atag,*`를 더한다 |
| `uart0` | `okay`. TX GPIO207, RX GPIO208(mode 1) | 벤더 pinfunc | 디버그 UART다 |
| `memory@40000000` | 1GiB 자리 | 펌웨어 | 부트로더가 실제 크기(8·12GiB)로 고친다. 펌웨어 정적 트리도 1GiB 자리다 |
| `framebuffer0` | `0xfc16f000`, 2944×1840, stride 11776, `a8r8g8b8` | 커뮤니티 | `disabled`. 바인딩대로 `/chosen` 아래에 둔다. 커뮤니티는 LK 기록(mblock 할당, `atag,videolfb`)에서 읽었고, 이 주소로 fbcon을 띄웠다(12GiB 기기) |
| `framebuffer_mem` | `0xfc16f000`, 0x3e90000(LK가 할당한 크기, 세 화면보다 조금 크다), `no-map` | 커뮤니티 | `disabled`. 부트로더가 이미 예약하면 지운다 |
| MT6363 `pmic@4` | `interrupts = <4 64 IRQ_TYPE_LEVEL_HIGH>`(SPMI rcs, SID 4. 가운데 칸은 컨트롤러가 버린다), `interrupt-controller`, 셀 셋. `keys`: 전원 PWRKEY 48/뗌 51(`KEY_POWER`, `wakeup-source`), HOMEKEY 49/뗌 52(`KEY_VOLUMEUP`), 길게 누름 리셋 모드 2(전원+볼륨 올림), 시간 0. `regulators@30`: 펌웨어가 늘 켜 두는 22개(`regulator-always-on`)와 VEMC(`vufs_vcc`) | 펌웨어, OnePlus 공식 MT6897 커널(인터럽트 번호, 레지스터) | `mediatek,mt6363`(패치 0015·0016). 레지스터 기준 0x30은 벤더 주소 − 업스트림 주소로 확인했다(TOP_TRAP 0x36/0x6 등). 펌웨어가 필요할 때만 켜는 11개(vbuck1·3·7, vcn13·15, vrf*, vsram-digrf)는 노드에 넣지 않아 리눅스가 건드리지 않는다. 전압 한계도 주지 않는다(코어가 부트로더 전압을 바꾸지 않게. VEMC 전압 읽기는 7.2.12에서 틀린다). 이름은 펌웨어 소비자에서 땄다: `vcore`(dvfsrc), `vsram_gpu`(gpufreq의 vsram), `vmodem`·`vsram_modem`(모뎀), `vufs_vcc`(UFS), `vio18` |
| `ufshci` 공급 | `vcc-supply = <&mt6363_vemc>`, `vccq-supply = <&mt6319_8_vbuck3>` | 펌웨어 | 펌웨어 `ufshci`와 같다 |
| MT6368 `pmic@5` | `interrupts = <5 48 IRQ_TYPE_LEVEL_HIGH>`(SPMI rcs, SID 5. 가운데 칸은 컨트롤러가 버린다. 48은 MT6368 RCS0), `interrupt-controller`, 셀 셋. `regulators@30`: 펌웨어가 늘 켜 두는 6개 vbuck2(`vgpu`), vbuck5, vbuck6, vio28, vtp, vusb(`vusb33`). 모두 `regulator-always-on` | 펌웨어, OEM(레지스터, 전압 표, 인터럽트 번호) | `mediatek,mt6368`(패치 0025·0026)과 `mediatek,mt6368-regulator`(0023·0024). 레지스터 기준은 MT6363과 같은 0x30이다(드라이버의 주소 = 벤더 주소 − 0x30). 펌웨어가 늘 켜 두지 않는 레일(vbuck1·3·4, vaux18, vrf13·18-aif, vant18, vibr, vfp, vmc, vaud18, vcn33-1·2, vefuse, vmch-eint-*, vsim2)과 펌웨어에 없는 vpa·vmch는 노드에 넣지 않아 리눅스가 건드리지 않는다. 전압 한계도 주지 않는다(MT6363과 같은 방침). 이름은 펌웨어 소비자에서 땄다: `vgpu`(gpufreq의 `vgpu-supply`), `vusb33`(ssusb의 `vusb33-supply`) |
| `ssusb` 공급 | `vusb33-supply = <&mt6368_vusb>` | 펌웨어 | 펌웨어 `ssusb`와 같다. 펌웨어처럼 늘 켜 둔다 |
| `gpio-keys` | `switch-lid`: GPIO1, active low, `SW_LID`, 100 ms, `wakeup-source` | 펌웨어(`hall_1` EINT1), 제3자 미러 `hall.c` | `disabled`(2c). GPIO1이 0이면 덮개 닫힘이다. 펌웨어의 `hall_2`(GPIO26)는 키보드 줄을 본다 |
| `uart3` 키보드 `keyboard` | `lenovo,tb375fc-pogo-keyboard`. `power-gpios` GPIO61, `tx-enable-gpios` GPIO71, `wake-gpios` GPIO40(상승 에지), `plug-gpios` GPIO26(active low), `wakeup-source`, 핀 상태 `default`·`detached` | 펌웨어 `tinno,tn_keyboard`, 스톡 모듈 | `disabled`(uart3, 2c). 스톡 모듈이 쓰는 핀 상태만 옮겼다: RX는 붙으면 URXD3, 떼면 GPIO180 입력(바이어스 없음), TX는 늘 UTXD3. `uart_wake_*` 상태와 `uart_rx_gpio`(EINT 없음)는 돌 때 쓰지 않는다 |
| `&i2c5` MT6375 `pmic@34` | 400 kHz(펌웨어 1 MHz), `charger`의 `usb-otg-vbus-regulator`(5 V, `vbus-switch-supply`), 핀 상태 GPIO179 output-low | 펌웨어, OEM, 스톡 모듈 | `disabled`. 인터럽트(GPIO7)는 넣지 않았다 |
| `&i2c10` `charger@65`·`charger@66`·`charger@6d`·`charger@6e` | `usb-otg-switch-regulator` | 펌웨어, 스톡 모듈 | `disabled`. `vbus-switch-supply`는 사용자 기기의 칩(`@65`, adb로 확인)을 가리킨다. 다른 기기는 부품이 다를 수 있어 기기에서 본다. compatible은 스톡 드라이버의 표대로 `nuvolta,nu2115`·`unisemi,upm6720`이다(펌웨어의 `@65`는 `southchip,upm6720`이고 I2C 이름으로 붙는다) |
| `usb-vbus-output` | `regulator-output`, `vout-supply` = 부스트 | 7.2.12 | `disabled`. TCPC 전까지 사용자 공간이 VBUS를 켠다 |
| `vddio_1v8` | regulator-fixed 1.8 V, GPIO63, `regulator-boot-on`, `regulator-always-on` | 펌웨어(패널의 `vddio_1v8-gpios`, 터치의 1.8V 핀 상태) | `disabled`(2c). 패널과 터치가 같이 쓰는 I/O 전원이라 simple-framebuffer 동안 꺼지면 안 된다. 패널 드라이버의 `vddio-supply`이지만 늘 켜 둔다: 벤더는 MIPI 앞에 켜고 뒤에 끄는데 패널 prepare로는 그 차례를 못 지키고, 터치도 쓴다 |
| `&dsi0` `panel@0` | `lenovo,tb375fc-nt36532`, 리셋 GPIO41, ID GPIO184, `vddio_1v8`, `lcd_avdd`, `lcd_avee`, 273×171mm | 펌웨어, 미러, 스톡 모듈, LK | `disabled`. 패널 셋을 스트랩으로 가른다. 순정의 `panel1@0`~`panel3@2`는 따르지 않았다. 형제 노드로 두면 Tianma에서 dsi1이 영원히 기다리고, LK가 고친 트리를 받지 않는 길에서 BOE로 잘못 고른다 |
| `&dsi0`·`&dsi1` `ports/port@1` | 패널의 `port@0`·`port@1`로 | 업스트림 `mediatek,dsi.yaml`(번호) | `disabled`. 입력 포트는 파이프라인과 함께 더한다 |
| `lcd_avdd`, `lcd_avee` | regulator-fixed, GPIO187·186, `regulator-boot-on`, `off-on-delay-us = <80000>` | 펌웨어(`bias_p`·`bias_n`), 미러 | `disabled`. SM5109의 켜기 핀이다. 전압 5.8V는 TPS65132 식으로 읽은 값이고 소프트웨어는 쓰지 않는다. 80ms는 벤더가 끈 뒤 기다리는 시간이다 |
| `framebuffer0`의 `panel` | `&panel0` | 미러(셋 다 273615×171009µm) | simpledrm이 `width-mm`·`height-mm`를 읽는다. 파이프라인을 켤 때 뺀다 |
| `conn_pins_off`, `conn_pins_on` | GPIO192~202 | 펌웨어, 벤더 | consys가 쓴다(2d) |
| 레일 `mt6363_vcn13`, `mt6363_vrfio18`, `mt6368_vant18`, `mt6368_vcn33_1`, `mt6368_vcn33_2` | 1.35 V, 1.7 V(벤더가 쓰는 값), 나머지는 범위 없음 | 펌웨어, 벤더 | `disabled`(2d). of_regulator가 사용 가능한 자식만 보므로 리눅스가 건드리지 않는다 |
| `pmic@9` MT6685 | `reg = <0x9 SPMI_USID>` | 펌웨어 | `disabled`(2d). 드라이버가 없고 connsys가 DCXO 레지스터 하나를 읽는다 |
| `spi2` 터치 `touchscreen@0` | `novatek,nt36532-ts`, 9.6 MHz, IRQ = pio EINT9, 리셋 GPIO60, `iovcc-supply = <&vddio_1v8>`, `firmware-name` 둘(BOE, Tianma), `novatek,panel-id-gpios` = GPIO184, 크기 18401×29441, 171×273 mm, `touchscreen-swapped-x-y` | 펌웨어, OEM(프로토콜), 제3자 미러(패널 판별, 펌웨어 이름), 기기 실측(좌표 범위) | spi2와 함께 꺼져 있다. GPIO184가 0이면 BOE, 1이면 Tianma다. 컨트롤러 좌표는 픽셀의 10배이고 X가 짧은 쪽이다. 축 뒤집기는 기기에서 정한다 |
| `pio` 핀 상태 | `spi2-pins`(GPIO11~14, 6 mA), `touch-pins`(GPIO9 IRQ, GPIO184 패널 ID, GPIO60 리셋), `hall-pins`(GPIO1), `pogo-keyboard-pins`(GPIO181 UTXD3, GPIO26·40 입력, GPIO61·71), `pogo-keyboard-rx-pins`(GPIO180 URXD3), `pogo-keyboard-rx-detached-pins`(GPIO180 입력) | 펌웨어 | drive-strength는 0027이 핀 드라이버에 더한 연산으로 적용된다 |
| `aliases` | `spmi0 = &spmi_m` | 7.2.12 드라이버 | 버스가 둘인 MT8196 꼴은 버스마다 `spmiN` 별칭이 있어야 probe한다 |
| MT6319 `pmic@6`~`pmic@8` | 6: vbuck1(`vmm`)·vbuck3(`vproc1`)·vbuck4(`vproc3`), 7: vbuck1(`vproc2`)·vbuck3, 8: vbuck3(`vufs_vccq`). 모두 0.3~1.19375V | 펌웨어 | `mediatek,mt6319-regulator` + `mt6315` 대체. 이름은 펌웨어의 소비자에서 땄다(ISP·카메라 전원 도메인, MCUPM CPU DVFS의 proc1~4, UFS VCCQ). 소비자가 아직 없어 모두 `regulator-always-on`이다. 펌웨어는 셋 모두 buck1+buck2 2상인데, 7.2.12 드라이버는 SID로 짝을 정해(6이면 1+2+4) 맞지 않으므로 동작 모드(`regulator-allowed-modes`)는 열지 않는다 |
| `ramoops` | `0x48090000`, 0xe0000(record 0x20000, console 0x40000, pmsg 0x80000) | 커뮤니티 | `disabled`. `aee_debug_kinfo`(`0x48080000`+0x10000)와 `minirdump`(`0x48170000`) 사이 틈이다. LK가 벤더 커널 명령줄에 넣는 `ramoops.*` 값과 같다(벤더 자신의 ramoops 자리) |

## 바인딩에 더할 것

`dt-validate`가 짚은 것이다. 모두 바인딩 한두 줄짜리 패치이고, 업스트림에 낼 때 dts와 같이 낸다.

| 바인딩 | 더할 것 |
|---|---|
| `arm/mediatek.yaml` | `lenovo,tb375fc`·`lenovo,tb373fu` + `mediatek,mt6897` |
| `serial/mediatek,uart.yaml` | `mediatek,mt6897-uart` |
| `watchdog/mediatek,mtk-wdt.yaml` | `mediatek,mt6897-wdt`(`mediatek,mt6589-wdt` 대체 목록) |
| `timer/mediatek,timer.yaml` | `mediatek,mt6897-timer`(`mediatek,mt6765-timer` 대체 목록) |
| `usb/mediatek,mtu3.yaml`, `usb/mediatek,mtk-xhci.yaml` | `mediatek,mt6897-mtu3`, `mediatek,mt6897-xhci` |
| `phy/mediatek,xsphy.yaml` | `mediatek,mt6897-xsphy` |
| `spmi/mediatek,mt8196-spmi.yaml` | `mediatek,mt6897-spmi`(MT8196 대체). 레지스터는 맞는다(2026-11-01 대조) |
| `cpufreq/cpufreq-mediatek-hw.yaml` | `reg` 최대 2개 → 3개 |

UFS 호스트와 M-PHY의 MT6897 compatible은 패치 0019·0020이 더한다(업스트림에서는 MT8196 시리즈 v14 위에 다시 낸다).

## 할 일

### 2a: 기기에서 확인할 것

1. **부트로더가 고치는 것.** 첫 부팅 뒤 `/sys/firmware/fdt`를 떠서 우리 dtb와 견준다. 특히 다음을 본다.
   - `/chosen`의 자식 노드가 남는지. 사라지면(MT6895 xaga 포트에서 보고됨) framebuffer를 루트로 옮긴다
   - `/memory`가 실제 크기로 바뀌는지
   - `/reserved-memory`에 `mblock-*` 노드가 붙는지. BL31(`0x48800000`, 2MB)이 들어오지 않으면 직접 예약한다(커뮤니티: 없으면 보조 CPU와 서스펜드가 멈춘다)
   - `/cpus` 노드를 이름으로 건드리는지. 우리 이름(`cpu@100`)이 펌웨어 이름(`cpu@100` = 첫 A715)과 가리키는 코어가 다르다
2. **framebuffer.** 안드로이드에서 `/proc/iomem`과 살아 있는 트리(`/chosen`의 `atag,videolfb`, `/reserved-memory`)로 주소·크기·형식을 확인한다. 둘 다 root가 필요하다. 실기기(ZUXOS 1.5.10.044, 2026-11-01)에서는 `/proc/iomem`이 막혀 있었고, `/proc/device-tree`는 이름만 보이고 값은 막혀 있었다. 이름으로는 LK가 화면 영역을 `mblock-14-framebuffer`로 예약한다. 패널이 그리는 버퍼는 가로 2944×1840이다(SurfaceFlinger의 `framebufferSpace`. 화면 회전 설정과 상관없다). 커뮤니티 포트는 이 주소로 fbcon을 띄웠다(12GiB 기기). 우리 기기는 8GB다. 맞으면 `framebuffer0`을 켜고, 부트로더가 예약하지 않으면 `framebuffer_mem`도 켠다. 안드로이드에서 실제 크기 약 273×171mm도 확인해 둔다(2c에서 `panel` 노드의 `width-mm`·`height-mm`).
3. **ramoops.** 살아 있는 트리의 예약 영역으로 `0x48090000`~`0x4816ffff`가 비었는지, 따뜻한 재부팅에 지워지지 않는지 보고 켠다. 실기기에서 LK는 pstore 자리를 `mblock-2-pstore`로 예약한다(값은 root 필요). LK가 명령줄에 `ramoops.*`를 넣으면 노드와 겹쳐 장치가 둘 생기므로 하나만 둔다. 크기가 `0xe0000`을 넘으면 minirdump를 덮는다.
4. **UART0.** 시험 패드(GPIO207 TX, GPIO208 RX)의 위치와 921600bps를 확인한다. 부트로더가 UART0 클럭을 끄고 넘기면 레지스터 접근이 멈출 수 있다. 그러면 `uart0`을 끄고 화면과 pstore로 본다. 커뮤니티 포트도 UART를 쓰지 않았다(uarthub 경로에 mainline 드라이버가 없다는 주석). "UART0이 키보드 줄"이라는 그쪽 주석은 펌웨어와 맞지 않는다(키보드는 UART3).
5. **워치독.** 커널이 넘겨받은 뒤 다시 켜지지 않는지 본다(`CONFIG_MEDIATEK_WATCHDOG=y`).
6. **idle·cpufreq.** 멈추면 `cpuidle.off=1`, `initcall_blacklist=mtk_cpufreq_hw_driver_init`로 하나씩 뺀다.
7. **systimer.** 첫 부팅이 된 뒤에 켜 보고, 펌웨어가 같은 채널(`+0x40`)을 쓰지 않는지(멈춤, 인터럽트 폭주) 본다.
8. **EL2 판정**(설계안 D8)은 커널 기록으로 한다.

### 2b: 저장장치·USB

- **클럭.** 드라이버(패치 0009~0013)와 노드가 있다. 2b에서 클럭 컨트롤러 여섯을 켜고 uart0의 `bus`를 pericfg_ao 게이트로 바꾼다. `clk_ignore_unused`는 그대로 둔다(없으면 쓰지 않는 화면·UFS 게이트가 늦은 초기화에서 닫힌다). PLL 다섯, topckgen 먹스 여덟, pericfg_ao 게이트 셋이 HW voter(`0x10320000`)를 쓴다. 처음 볼 것은 apmixedsys가 뜨는지다(mainpll이 critical이라 등록할 때 바로 투표한다).
  - 레지스터 블록(펌웨어): topckgen `0x10000000`, infracfg_ao `0x10001000`, apmixedsys `0x1000c000`, pericfg_ao `0x11036000`, ufscfg_ao `0x112b8000`, ufscfg_pdn `0x112bb000`, vlp_cksys `0x1c013000`, HW voter `0x10320000`
  - UFS: 호스트 `main`·`aes`(ufscfg_pdn), UniPro 넷(ufscfg_ao). PHY는 클럭을 받지 않는다. USB: mtu3 `sys_ck`·`host_ck`(topckgen), `frmcnt_ck`(pericfg_ao). UART `bus`(pericfg_ao)
- **UFS.**
  - 바인딩과 드라이버: 패치 0019~0022. 노드는 다 있고 2b에서 클럭 컨트롤러와 함께 켠다
  - 리셋: 클럭 드라이버가 MT8196처럼 낸다(ufscfg_pdn: UniPro·crypto·HCI = set `0x48`·clr `0x4c`의 bit 0·1·2, ufscfg_ao: M-PHY bit 8). 펌웨어의 `ti,syscon-reset`은 쓰지 않는다. 호스트에 셋을 준다. 드라이버는 업스트림 v14처럼 bulk API로 받는다. M-PHY 리셋은 벤더가 쓰지 않아(`mphy_rst`를 주석으로 막았다) 넣지 않는다
  - 공급: `vcc` = MT6363 VEMC, `vccq` = MT6319(SPMI 8) vbuck3. 둘 다 보드 dts에 이었다(패치 0014~0018). VEMC는 소비자가 UFS뿐이라 always-on이 아니고, 부트 명령줄의 `regulator_ignore_unused`가 UFS가 붙기 전에 꺼지는 것을 막는다
  - MCQ: dtsi가 `mediatek,ufs-disable-mcq`로 시작한다. MCQ를 켤 때는 공통 바인딩을 고쳐 큐 인터럽트 SPI 177~184를 넣고 이 속성을 뺀다
  - 전원 도메인은 필요 없다. 펌웨어의 `mediatek,ufs-rtff-mtcmos`(SMC)·`-broken-rtc`·`-pmc-via-fastauto`·`-tx-skew-fix`는 compatible이 뜻한다(match data)
  - 클럭 스케일링은 없다. 7.2.12 드라이버는 `ufs_sel`과 그 최소·최대 부모가 클럭 목록에 있어야 스케일링한다. 업스트림 리뷰는 그 부모를 `clocks`에서 빼라고 했다. 먹스는 LK가 정한 부모 그대로 돈다
  - PHY(`phy-mtk-ufs`): MT6897 M-PHY는 벤더가 `mphy-ver = <1>`로 다루는 새 판이다. 벤더는 클럭을 받지 않고 power_on/off에서 아무것도 하지 않는다. 0021이 MT6897에 전원 연산 없는 ops를 준다(7.2.12의 MT8183 순서는 PLL·CDR을 강제로 끄고 켠다)
- **USB.** 가젯용 `dr_mode = "peripheral"`(보드)로 시작한다. `vusb33` 공급은 펌웨어처럼 MT6368 VUSB(`mt6368_vusb`, always-on)다. 보드가 이어 두어 **ssusb를 켤 때 SPMI M 버스와 MT6368(MFD·regulator, `CONFIG_REGULATOR_MT6368`)도 켜져 있어야 한다.** 그렇지 않으면 mtu3가 공급을 기다리며 뜨지 않는다. xsphy에는 펌웨어의 eye 값만 넣었다. 업스트림 드라이버에 MT6897 꼴이 없어 셋은 뒤로 미룬다: U2 내부 저항 보정(INTR_CAL 칸과 efuse), 벤더가 MT6897에만 넣는 U3 RX CDR 값(U3 포트 `+0x408`의 [23:16] `RG_XTP_LN0_RX_CDR_RESERVE` = 0x8), 깨우기 접점(벤더 판 103). 호스트 모드는 MT6375 OTG의 VBUS 뒤, 역할 전환은 TCPC 뒤다.
- **SPMI.**
  - 벤더 레지스터 표는 업스트림 mt8195 표와 같다(2026-11-01 대조). 그래서 MT8196 데이터를 그대로 쓴다. 커뮤니티는 MT6895 데이터(같은 레지스터, 버스 하나)로 닿았다
  - PMIC 여섯(MT6363 @4, MT6368 @5, MT6319 @6~8, MT6685 @9)은 모두 M 버스에 있다. M 자식만 켜고 P 자식은 끈다. 별칭은 `spmi0` 하나다
  - 업스트림은 자식마다 `rcs` 인터럽트(pio EINT 248)와 클럭 셋(`pmif_sys_ck`, `pmif_tmr_ck`, `spmimst_clk_mux`)을 요구한다. 없으면 probe가 실패한다. rcs 때문에 pinctrl-mt6897 뒤다
  - 노드는 다 있다(M 버스, 별칭, MT6363, MT6368, MT6319 셋). 2b에서 pio와 함께 `okay`로 바꾸고, 커널 기록으로 MT6363 MFD·regulator·키, MT6368 MFD·regulator, MT6319 셋의 probe, `/sys/kernel/debug/regulator/regulator_summary`의 레일 이름, 전원 키 이벤트(`evtest`)를 본다. 드라이버 고침 둘(rcs 디먹스, 채널 설정 순서, 패치 0007·0008)과 MFD 인터럽트 고침(0016)도 이때 확인한다. 자세한 확인 목록은 [체크리스트](../../../docs/tb375fc/bringup-checklist.md) 6.3~6.5절에 있다
- **핀.** pio 노드와 드라이버(패치 0004~0006)가 있다. 켜면 probe가 EINT 마스크를 모두 세운다(업스트림 동작). 핀 상태(`pinctrl-0`)는 쓰는 노드가 생길 때 더한다
- **cpufreq-hw.** 바인딩의 `reg` 개수를 고친다.

### 그 뒤

- 2c:
  - 포고 키보드: 패치 0032~0036과 노드가 있다(꺼 둠). 2c에서 pio·클럭 컨트롤러와 함께 uart3을 켜고 아래를 본다.
    1. `mediatek,syscon-pad-mode`: 켠 뒤 `0x11036050`의 값(bit 4 = 1, bit 0 = 0). 스톡 로그의 "first get uart3_def_value"처럼 바뀌기 전 값도 본다
    2. GPIO26이 키보드를 붙이면 0, 떼면 1인지. GPIO61이 따라가는지
    3. 처음 프레임(`dyndbg`로 `oneplus_pogo_kbd`): 0x7B가 오는지, "bad sum"이 없는지. 전 판과 다르면 여기서 보인다
    4. `evtest`: 키, 미디어 키, 터치패드(손가락 넷, 2944×1840. 해상도는 실측으로 정한다), Caps Lock LED(0x6A/0x27)
    5. 키보드를 뒤로 접을 때 0x93/0x03 프레임과 `SW_TABLET_MODE`
    6. 921600에서 PIO로 넘침이 있는지(`/proc/tty/driver/serial`의 oe). 스톡은 APDMA를 쓴다
    7. 서스펜드: 0x92/0x02 응답, 키로 깨우기(GPIO40), 붙이고 떼어 깨우기(GPIO26)
    8. TX 게이트와 응답 지연: `dyndbg`의 `gate shut after N us, reply after M us`. N에서 프레임 시간(25바이트면 271 µs), M에서 응답 프레임 시간(31바이트면 340 µs)을 뺀다. 로직 분석기가 있으면 GPIO71과 선을 함께 본다
    9. 잠든 사이 펼치기와 접기: 깬 뒤 SLEEP 0 다음에 MCU가 0x93/0x03을 다시 보내는지. 보내지 않으면 접은 것을 깬 뒤 알 수 없다
  - 터치·펜·홀 센서: 패치 0027~0031과 노드가 있다(꺼 둠). 2c에서 pio·클럭 컨트롤러와 함께 spi2·gpio-keys·`vddio_1v8`을 켜고, spi2의 SCLK(9.6 MHz 아래, 깨지면 `cs-gpios`), 터치의 trim ID·패널 ID·펌웨어 내려받기, `evtest`의 좌표와 축 방향, 펜(압력·기울기·단추·호버, 펜 켜기 명령 `0x7B 0x01`), 서스펜드 뒤 터치, 덮개와 키보드에 따른 GPIO1·26 값을 본다([체크리스트](../../../docs/tb375fc/bringup-checklist.md) 7절)
- 2c 2차:
  - 패널: `dsi0`·`dsi1`에 compatible과 파이프라인을 붙이고 `panel0`·`lcd_avdd`·`lcd_avee`를 켜고 `framebuffer0`의 `panel`을 뺀다(패치 0083·0084)
- 2d: `consys@18001000`(`mediatek,mt6897-consys`), `wifi@18000000`(`mediatek,mt6897-wifi`), `bluetooth@18800000`, `pmic@9`, 레일 다섯을 켠다. 루트의 `gps`·`connscp`는 그때까지 트리에 없다. LK는 status를 보지 않아, 넷이 다 있으면 메모리를 나누고 펌웨어를 싣기 때문이다. LK가 `emi-addr`와 `memory-region`을 채우는지 본다
  - `&spm`을 켠다(`status = "okay"`). `ifr_bus`·`vlpcfg_bus`는 켜지 않는다. 소비자는 `power-domains = <&scpsys MT6897_POWER_DOMAIN_CONN>`
- 2e: mcusys-off·system idle 상태와 s2idle(SPM), DSU PMU(SPI 35), `#cooling-cells`
