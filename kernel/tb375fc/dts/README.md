# TB375FC 장치 트리

TB375FC(MediaTek MT6897)의 mainline 장치 트리다. 설계안 [2.3절](../../../docs/tb375fc/DESIGN.md)의 방침대로 업스트림 꼴(`mt6897.dtsi` + 보드 dts)로 짰다. 지금 범위는 2a(첫 부팅)와 2b(저장장치·USB)의 자리다.

| 파일 | 내용 |
|---|---|
| `mediatek/mt6897.dtsi` | SoC. CPU·GIC·아키텍처 타이머·PSCI·PMU·워치독·cpufreq-hw는 켜 두고, UART(보드가 켠다)·systimer·UFS·USB·SPMI는 `disabled`로 둔다 |
| `mediatek/mt6897-lenovo-tb375fc.dts` | 보드. `chosen`, `memory`, UART0을 켠다. framebuffer·ramoops는 기기에서 확인할 때까지 `disabled` |
| `mediatek/mt6897-lenovo-tb373fu.dts` | 글로벌판(Idea Tab Pro). tb375fc.dts를 include하고 `model`·`compatible`만 바꾼다 |

- `build.sh`가 `mediatek/*.dts*`를 Linux 7.2.12의 `arch/arm64/boot/dts/mediatek/`에 복사하고, 패치 0001이 빌드 목록에 dtb 둘을 더한다.
- include는 7.2.12에 있는 `dt-bindings` 헤더(`arm-gic.h`, `irq.h`, `phy.h`)만 쓴다.
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
| 펌웨어 DT와 대조 | 켠 노드와 자리만 둔 노드의 `reg`·`interrupts`가 모두 같다(아래 표의 예외만 일부러 다르다) |

dtschema는 VM의 venv(`~/buntos-build/tb375fc/venv-dtschema`)에만 설치했다. pylibfdt를 빌드할 Python 헤더는 `apt-get download libpython3.13-dev`로 받아 venv 안에 풀었다(시스템 패키지는 그대로다).

## 출처

| 표시 | 자료 |
|---|---|
| 펌웨어 | 글로벌판 순정 펌웨어 ZUI 17.0.04.266의 SoC DTB(vendor_boot)에 보드 오버레이(dtbo)를 입혀 푼 트리. 푸는 법은 [하드웨어 실측](../../../docs/hw/lenovo-tb375fc.md) 1절. 중국판 ZUI 16.0.20.091도 이 값들이 같다 |
| 벤더 | MediaTek 벤더 `arch/arm64/boot/dts/mediatek/mt6897.dts`와 `mt6897-pinfunc.h`(이 기기 모듈 미러 `mt8792/android_kernel_lenovo_mt8792-modules`, 갈래 6.1.138) |
| 커뮤니티 | [u0n0u/tb375fc-linux](https://github.com/u0n0u/tb375fc-linux)의 `mt6897.dtsi`·`mt6897-lenovo-tb375fc.dts`(GPL-2.0 OR BSD-3-Clause). 부트로더가 넘긴 트리와 LK 기록에서 뽑은 값이 주석에 있다 |
| 7.2.12 | Linux 7.2.12의 드라이버와 바인딩, 업스트림 MediaTek dtsi(mt8195, mt8188)의 관례 |

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
| `uart0`~`uart3` | `0x11001000`~`0x11004000`(0x1000), SPI 250~253 | 펌웨어 | `baud`·`bus` 모두 clk26m. 7.2.12 `8250_mtk`는 `baud`가 있으면 `bus`도 요구해 하나만 주면 probe가 실패한다. 진짜 `bus`(pericfg_ao 게이트)는 clk-mt6897 뒤 |
| `watchdog` | `0x1c00a000`(0x100) | 펌웨어 | 7.2.12 `mtk_wdt`는 대체 compatible `mediatek,mt6589-wdt`로 붙고, 켜져 있으면 `WDOG_HW_RUNNING`으로 받아 커널이 쓰다듬는다. 펌웨어의 `syscon`·`simple-mfd`·reboot-mode는 뺐다 |
| `performance` | `0x11bc10`·`0x11bd30`·`0x11be50`(각 0x120) | 펌웨어 | 노드 이름을 첫 `reg`에 맞췄다(펌웨어 `@11bc00`). mt8195와 같은 CSRAM 배치에 도메인이 하나 더 있다 |
| `reserved-memory` | 틀(셀 2, `ranges`) | 설계안 2.3 | 고정 예약(ATF, TEE, GenieZone, connsys 등)은 부트로더가 `mblock-*` 노드로 더한다 |
| `ufshci`, `ufsphy` | `0x112b0000`(0x3000) SPI 168, `0x112a0000`(0x10000) | 펌웨어 | `disabled`. compatible은 펌웨어 그대로 `mt8183`(7.2.12 바인딩이 받는다). MCQ 큐 인터럽트 SPI 177~184는 아직 넣지 않았다 |
| `ssusb`, `xhci0` | mac `0x11201000`(0x2e00) + ippc `0x11203e00`(0x100) SPI 160, xHCI `0x11200000` SPI 161, xHCI `sys_ck` = clk26m | 펌웨어 | `disabled` |
| `xsphy` | 공유 `0x11e43000`(0x200), U2 `0x11e40000`(0x400), U3 `0x11e43400`(0x500), `ref` = clk26m | 펌웨어 | `disabled`. 업스트림 이름 `xs-phy@`/`usb-phy@`로 바꿨다 |
| `spmi` | M 버스: pmif `0x1cc04000` + spmimst `0x1cc01000`, P 버스: pmif `0x1cc06000` + spmimst `0x1cc00000` | 펌웨어 | `disabled`. 펌웨어는 한 노드에 영역 넷이다. 7.2.12의 MT8196(SPMI 2.0, 버스 둘) 꼴로 나눴다. pmif 크기는 펌웨어 `0x8ff`를 `0x900`으로 썼다(`spmi-pmif-mpu`가 `0x1cc04900`에서 시작) |

**GIC 기준 주소.** 커뮤니티 dtsi는 펌웨어 값(`0x0c400000`)을 쓰면서, 업스트림 MTK 계열(MT8183~MT8195)처럼 `0x0c000000`/`0x0c040000`을 대안으로 적어 두었다. 이 트리는 펌웨어 값을 쓴다.
- 펌웨어 SoC 트리와, 커뮤니티가 읽은 부트로더가 넘긴 트리가 모두 `0x0c400000`이다.
- MT6897에서 `0x0c000000`부터는 MCUSYS다. 펌웨어 트리에 `mcusys-ao-cfg`(`0x0c000000`), `mediatek,gic500`(`0x0c000000`, 4MiB 디버그 창), CPU PLL(`0x0c030000`~), MCUPM(`0x0c070000`)이 있다. `mediatek,gic_cpu`는 `0x0c400000`을 가리킨다.
- 커뮤니티 포트는 이 값으로 UART 콘솔과 저장장치까지 돌았다. 인터럽트가 안 들어오면 둘 다 안 된다.
- 커뮤니티 보드 dts 머리 주석의 "GIC 0xc000000 + 재분배 0xc400000"은 위 사실과 맞지 않는다.

### 보드 (`mt6897-lenovo-tb375fc.dts`)

| 노드 | 값 | 출처 | 메모 |
|---|---|---|---|
| `model`, `compatible` | `lenovo,tb375fc`, `mediatek,mt6897` | 설계안 | TB373FU는 `lenovo,tb373fu` |
| `chosen` | `stdout-path = "serial0:921600n8"` | MTK 관례 | 부트로더가 `bootargs`, initrd, `kaslr-seed`, `atag,*`를 더한다 |
| `uart0` | `okay`. TX GPIO207, RX GPIO208(mode 1) | 벤더 pinfunc | 디버그 UART다. UART3은 포고 키보드 줄(GPIO180 URXD3, GPIO181 UTXD3, mode 2)이라 꺼 둔다 |
| `memory@40000000` | 1GiB 자리 | 펌웨어 | 부트로더가 실제 크기(8·12GiB)로 고친다. 펌웨어 정적 트리도 1GiB 자리다 |
| `framebuffer0` | `0xfc16f000`, 2944×1840, stride 11776, `a8r8g8b8` | 커뮤니티 | `disabled`. 바인딩대로 `/chosen` 아래에 둔다. 커뮤니티는 LK 기록(mblock 할당, `atag,videolfb`)에서 읽었다 |
| `framebuffer_mem` | `0xfc16f000`, 0x3e90000(세 화면), `no-map` | 커뮤니티 | `disabled`. 부트로더가 이미 예약하면 지운다 |
| `ramoops` | `0x48090000`, 0xe0000(record 0x20000, console 0x40000, pmsg 0x80000) | 커뮤니티 | `disabled`. `aee_debug_kinfo`(`0x48080000`+0x10000)와 `minirdump`(`0x48170000`) 사이 틈이다 |

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
| `spmi/mediatek,mt8196-spmi.yaml` | `mediatek,mt6897-spmi`(MT8196 대체). 드라이버 데이터가 맞는지 먼저 확인한다(2b) |
| `cpufreq/cpufreq-mediatek-hw.yaml` | `reg` 최대 2개 → 3개 |

## 할 일

### 2a: 기기에서 확인할 것

1. **부트로더가 고치는 것.** 첫 부팅 뒤 `/sys/firmware/fdt`를 떠서 우리 dtb와 견준다. 특히 다음을 본다.
   - `/chosen`의 자식 노드가 남는지. 사라지면(MT6895 xaga 포트에서 보고됨) framebuffer를 루트로 옮긴다
   - `/memory`가 실제 크기로 바뀌는지
   - `/reserved-memory`에 `mblock-*` 노드가 붙는지
   - `/cpus` 노드를 이름으로 건드리는지. 우리 이름(`cpu@100`)이 펌웨어 이름(`cpu@100` = 첫 A715)과 가리키는 코어가 다르다
2. **framebuffer.** 언락 전 안드로이드에서 `/proc/iomem`과 살아 있는 트리(`/chosen`의 `atag,videolfb`, `/reserved-memory`)로 주소·크기·형식을 확인한다. 맞으면 `framebuffer0`을 켜고, 부트로더가 예약하지 않으면 `framebuffer_mem`도 켠다. 안드로이드에서 실제 크기 약 273×171mm도 확인해 둔다(2c에서 `panel` 노드의 `width-mm`·`height-mm`).
3. **ramoops.** 살아 있는 트리의 예약 영역으로 `0x48090000`~`0x4816ffff`가 비었는지, 따뜻한 재부팅에 지워지지 않는지 보고 켠다.
4. **UART0.** 시험 패드(GPIO207 TX, GPIO208 RX)의 위치와 921600bps를 확인한다. 부트로더가 UART0 클럭을 끄고 넘기면 레지스터 접근이 멈출 수 있다. 그러면 `uart0`을 끄고 화면과 pstore로 본다.
5. **워치독.** 커널이 넘겨받은 뒤 다시 켜지지 않는지 본다(`CONFIG_MEDIATEK_WATCHDOG=y`).
6. **idle·cpufreq.** 멈추면 `cpuidle.off=1`, `initcall_blacklist=mtk_cpufreq_hw_driver_init`로 하나씩 뺀다.
7. **systimer.** 첫 부팅이 된 뒤에 켜 보고, 펌웨어가 같은 채널(`+0x40`)을 쓰지 않는지(멈춤, 인터럽트 폭주) 본다.
8. **EL2 판정**(설계안 D8)은 커널 기록으로 한다.

### 2b: 저장장치·USB

- **클럭.** 처음에는 fixed-clock과 `clk_ignore_unused`로 시작하고, clk-mt6897이 오면 바꾼다.
  - 레지스터 블록(펌웨어): topckgen `0x10000000`, infracfg_ao `0x10001000`, apmixedsys `0x1000c000`, pericfg_ao `0x11036000`, ufscfg_ao `0x112b8000`, ufscfg_pdn `0x112bb000`, vlp_cksys `0x1c013000`, HW voter `0x10320000`
  - UFS: 호스트 `ufs`, PHY `unipro`·`mp`. USB: mtu3 `sys_ck`·`host_ck`(topckgen), `frmcnt_ck`(pericfg_ao). UART `bus`(pericfg_ao)
- **UFS.**
  - 리셋: 펌웨어처럼 ufscfg_pdn의 `ti,syscon-reset`(업스트림 `reset-ti-syscon`)으로 `unipro_rst`·`crypto_rst`·`hci_rst`를 준다. 비트는 set `0x48`, clr `0x4c`의 0·1·2다
  - 공급: `vcc` = MT6363 VEMC, `vccq` = MT6319(SPMI 8) vbuck3. PMIC 전에는 `regulator-fixed`로 둔다
  - 처음에는 `mediatek,ufs-disable-mcq`로 시작한다. 전원 도메인도 필요하다(pmdomain 패치)
- **USB.** 가젯용 `dr_mode = "peripheral"`로 시작한다. `vusb33` 공급, xsphy efuse 보정(펌웨어 `mediatek,efuse-intr = <0x25>`)을 넣는다. 역할 전환은 MT6375 TCPC 뒤다.
- **SPMI.**
  - MT8196 드라이버 데이터(mt8195 레지스터 + SPMI 2.0)가 MT6897에 맞는지 확인한다. 커뮤니티는 MT6895 데이터(같은 레지스터, 버스 하나)로 M 버스의 MT6363·MT6368에 닿았다
  - PMIC를 버스에 나눈다(MT6363 @4, MT6368 @5, MT6319 @6~8, MT6685 @9)
  - `rcs` 인터럽트(pio EINT 248)를 넣는다. pinctrl-mt6897 뒤다
  - MT8196 데이터는 `spmi0`·`spmi1` 별칭이 필요하다
- **핀.** pio `0x10005000` 외 9개 영역(펌웨어)
- **cpufreq-hw.** 바인딩의 `reg` 개수를 고친다.

### 그 뒤

- 2c: 패널 노드(부트로더가 찾는 `/soc/dsi@1400d000/panel1@0`~`panel3@2`)와 `framebuffer0`의 `panel`(273×171mm), 포고 키보드(UART3 serdev, 키보드 전원 GPIO61, 연결 감지 GPIO26)
- 2e: mcusys-off·system idle 상태와 s2idle(SPM), DSU PMU(SPI 35), `#cooling-cells`
