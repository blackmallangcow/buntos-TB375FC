# TB375FC 장치 트리

TB375FC(MediaTek MT6897)의 mainline 장치 트리다. 설계안 [2.3절](../../../docs/tb375fc/DESIGN.md)의 방침대로 업스트림 꼴(`mt6897.dtsi` + 보드 dts)로 짰다. 지금 범위는 2a(첫 부팅)와 2b(저장장치·USB)의 자리다.

| 파일 | 내용 |
|---|---|
| `mediatek/mt6897.dtsi` | SoC. CPU·GIC·아키텍처 타이머·PSCI·PMU·워치독·cpufreq-hw는 켜 두고, UART(보드가 켠다)·systimer·UFS·USB·pio·SPMI는 `disabled`로 둔다 |
| `mediatek/mt6897-lenovo-tb375fc.dts` | 보드. `chosen`, `memory`, UART0을 켠다. framebuffer·ramoops는 기기에서 확인할 때까지 `disabled`. SPMI M 버스에 MT6319 셋을 둔다(SPMI는 2b까지 꺼져 있다) |
| `mediatek/mt6897-lenovo-tb373fu.dts` | 글로벌판(Idea Tab Pro). tb375fc.dts를 include하고 `model`·`compatible`만 바꾼다 |

- `build.sh`가 `mediatek/*.dts*`를 Linux 7.2.12의 `arch/arm64/boot/dts/mediatek/`에 복사하고, 패치 0001이 빌드 목록에 dtb 둘을 더한다.
- include는 7.2.12에 있는 `dt-bindings` 헤더(`arm-gic.h`, `irq.h`, `phy.h`, `spmi.h`)만 쓴다. pio 바인딩과 `mt6897-pinfunc.h`는 패치 0004·0005가 더한다.
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
| `ufshci`, `ufsphy` | `0x112b0000`(0x3000) SPI 168, `0x112a0000`(0x10000) | 펌웨어 | `disabled`. compatible은 펌웨어 그대로 `mt8183`(7.2.12 바인딩이 받는다). MCQ 큐 인터럽트 SPI 177~184는 아직 넣지 않았다. 전원 도메인 없이 `mediatek,ufs-rtff-mtcmos`(SMC)로 켜고, `mediatek,ufs-broken-rtc`도 쓴다 |
| `ssusb`, `xhci0` | mac `0x11201000`(0x2e00) + ippc `0x11203e00`(0x100) SPI 160, xHCI `0x11200000` SPI 161, xHCI `sys_ck` = clk26m | 펌웨어 | `disabled` |
| `xsphy` | 공유 `0x11e43000`(0x200), U2 `0x11e40000`(0x400), U3 `0x11e43400`(0x500), `ref` = clk26m | 펌웨어 | `disabled`. 업스트림 이름 `xs-phy@`/`usb-phy@`로 바꿨다 |
| `pio` | `0x10005000` 외 핀 설정 영역 9개, EINT 영역 다섯(`0x11ce0000`·`0x11de0000`·`0x11e90000`·`0x11fe0000`·`0x1c01e000`), GIC SPI 309 | 펌웨어(pio와 EINT 노드) | `disabled`(2b에서 SPMI와 함께 켠다). 펌웨어는 핀과 EINT가 다른 노드(`mediatek,mt6983-eint`)다. 업스트림 MT6878처럼 한 노드로 합쳤다. `gpio-ranges`는 실제 핀 232개이고, GPIO가 없는 EINT 232~249는 드라이버가 가상 핀으로 둔다 |
| `spmi` | M 버스: pmif `0x1cc04000` + spmimst `0x1cc01000`, P 버스: pmif `0x1cc06000` + spmimst `0x1cc00000` | 펌웨어 | `disabled`. 펌웨어는 한 노드에 영역 넷이다. 7.2.12의 MT8196(SPMI 2.0, 버스 둘) 꼴로 나눴다. pmif 크기는 펌웨어 `0x8ff`를 `0x900`으로 썼다(`spmi-pmif-mpu`가 `0x1cc04900`에서 시작). PMIC 여섯은 모두 M 버스에 있다(펌웨어에 `spmi-dev-mask`가 없어 벤더 드라이버가 모두 M 버스로 보낸다). 그래서 P 버스는 끈다. M 버스에는 `rcs` 인터럽트(pio EINT 248, 펌웨어 `rcs_irq`와 같다)와 클럭 셋을 준다. 클럭은 clk-mt6897 전까지 clk26m이 자리만 채운다(드라이버는 켜기만 하고 주파수를 쓰지 않는다) |

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
| `uart0` | `okay`. TX GPIO207, RX GPIO208(mode 1) | 벤더 pinfunc | 디버그 UART다. UART3은 포고 키보드 줄(GPIO180 URXD3, GPIO181 UTXD3, mode 2)이라 꺼 둔다 |
| `memory@40000000` | 1GiB 자리 | 펌웨어 | 부트로더가 실제 크기(8·12GiB)로 고친다. 펌웨어 정적 트리도 1GiB 자리다 |
| `framebuffer0` | `0xfc16f000`, 2944×1840, stride 11776, `a8r8g8b8` | 커뮤니티 | `disabled`. 바인딩대로 `/chosen` 아래에 둔다. 커뮤니티는 LK 기록(mblock 할당, `atag,videolfb`)에서 읽었고, 이 주소로 fbcon을 띄웠다(12GiB 기기) |
| `framebuffer_mem` | `0xfc16f000`, 0x3e90000(LK가 할당한 크기, 세 화면보다 조금 크다), `no-map` | 커뮤니티 | `disabled`. 부트로더가 이미 예약하면 지운다 |
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

- **클럭.** 처음에는 fixed-clock과 `clk_ignore_unused`로 시작하고, clk-mt6897이 오면 바꾼다. 벤더 clk-mt6897은 HW voter(`0x10320000`)를 PLL 다섯과 topckgen 먹스 열둘에 쓴다.
  - 레지스터 블록(펌웨어): topckgen `0x10000000`, infracfg_ao `0x10001000`, apmixedsys `0x1000c000`, pericfg_ao `0x11036000`, ufscfg_ao `0x112b8000`, ufscfg_pdn `0x112bb000`, vlp_cksys `0x1c013000`, HW voter `0x10320000`
  - UFS: 호스트 `ufs`, PHY `unipro`·`mp`. USB: mtu3 `sys_ck`·`host_ck`(topckgen), `frmcnt_ck`(pericfg_ao). UART `bus`(pericfg_ao)
- **UFS.**
  - 리셋: 펌웨어처럼 ufscfg_pdn의 `ti,syscon-reset`(업스트림 `reset-ti-syscon`)으로 `unipro_rst`·`crypto_rst`·`hci_rst`를 준다. 비트는 set `0x48`, clr `0x4c`의 0·1·2다
  - 공급: `vcc` = MT6363 VEMC, `vccq` = MT6319(SPMI 8) vbuck3(`mt6319_8_vbuck3`, 보드 dts에 있다). MT6363 전에는 `vcc`를 `regulator-fixed`로 둔다
  - 처음에는 `mediatek,ufs-disable-mcq`로 시작한다. MCQ를 켤 때는 큐 인터럽트 SPI 177~184를 넣는다
  - 전원 도메인은 필요 없다. 펌웨어처럼 `mediatek,ufs-rtff-mtcmos`(SMC)와 `mediatek,ufs-broken-rtc`를 쓴다
  - 바인딩은 클럭 `ufs` 하나만 받으므로 바인딩 패치가 필요하다. PHY(`phy-mtk-ufs`)는 `unipro`·`mp` 클럭이 필수인데, 펌웨어 PHY 노드에는 클럭이 없다
- **USB.** 가젯용 `dr_mode = "peripheral"`로 시작한다. `vusb33` 공급, xsphy efuse 보정(펌웨어 `mediatek,efuse-intr = <0x25>`)을 넣는다. 역할 전환은 MT6375 TCPC 뒤다.
- **SPMI.**
  - 벤더 레지스터 표는 업스트림 mt8195 표와 같다(2026-11-01 대조). 그래서 MT8196 데이터를 그대로 쓴다. 커뮤니티는 MT6895 데이터(같은 레지스터, 버스 하나)로 닿았다
  - PMIC 여섯(MT6363 @4, MT6368 @5, MT6319 @6~8, MT6685 @9)은 모두 M 버스에 있다. M 자식만 켜고 P 자식은 끈다. 별칭은 `spmi0` 하나다
  - 업스트림은 자식마다 `rcs` 인터럽트(pio EINT 248)와 클럭 셋(`pmif_sys_ck`, `pmif_tmr_ck`, `spmimst_clk_mux`)을 요구한다. 없으면 probe가 실패한다. rcs 때문에 pinctrl-mt6897 뒤다
  - 노드는 다 있다(M 버스, 별칭, MT6319 셋). 2b에서 pio와 함께 `okay`로 바꾸고, 커널 기록으로 MT6319 셋의 probe와 전압 읽기를 본다. 드라이버 고침 둘(rcs 디먹스, 채널 설정 순서, 패치 0007·0008)도 이때 확인한다
- **핀.** pio 노드와 드라이버(패치 0004~0006)가 있다. 켜면 probe가 EINT 마스크를 모두 세운다(업스트림 동작). 핀 상태(`pinctrl-0`)는 쓰는 노드가 생길 때 더한다
- **cpufreq-hw.** 바인딩의 `reg` 개수를 고친다.

### 그 뒤

- 2c:
  - 패널 노드(부트로더가 찾는 `/soc/dsi@1400d000/panel1@0`~`panel3@2`)와 `framebuffer0`의 `panel`(273×171mm)
  - 포고 키보드: UART3 serdev, 키보드 전원 GPIO61, 연결 감지 GPIO26, 깨우기 GPIO40, tx-en GPIO71, TX·RX 핀의 pinctrl 전환. 벤더 드라이버는 probe·startup·resume마다 pericfg_ao `0x11036050`의 bit4를 켜고 bit0을 끈다
  - 터치: SPI2 `0x11012000`, 리셋 GPIO60, IRQ GPIO9, 최대 9.6 MHz. SPI 클럭은 실제 값으로 줘야 한다(커뮤니티 포트는 26 MHz 자리값 때문에 SCLK가 약 52 MHz였다)
- 2d: `consys@18000000`(`mediatek,mt6897-consys-atf`)와 `wifi@18000000`. LK가 `emi-addr`와 `memory-region`을 채우는지 본다
- 2e: mcusys-off·system idle 상태와 s2idle(SPM), DSU PMU(SPI 35), `#cooling-cells`
