# 순정 LK가 커널 장치 트리에 요구하는 것

- 작성일: 2026-11-03
- 대상: 16.0.20.091 순정 LK(`app/mt_boot`), 포크 main의 보드 트리(`mt6897-lenovo-tb375fc.dts`)
- 목적: 시험 커널의 우회(순정 `vendor_boot`, 커널 안 트리)를 그만두고 LK가 우리 `vendor_boot`의 트리를 읽어 넘기게 할 수 있는지, 비용이 얼마인지 정한다.
- **기기에서 돌리지 않았다.** 이 문서의 어떤 판정도 우리 트리로 LK를 끝까지 돌려 본 결과가 아니다. 우리 트리로 본 실측은 `app_load_scp` ASSERT까지다(실측 보고서 7절).
- 표시:
  - **[코드]** 순정 LK를 역어셈블해 읽었다. 이미지는 실행하지 않았다. 역어셈블 결과와 LK 파일은 저장소에 넣지 않는다.
  - **[기록]** 순정 트리로 `_b`를 띄울 때의 LK 기록(`boot-b-7`의 `expdb`)이나 살아 있는 벤더 트리(`root-091/fdt.dtb`)에서 봤다. 우리 트리의 결과가 아니다.
  - **[모델]** 초안을 컴파일한 트리에, LK의 조회 규칙을 옮긴 스크립트를 돌렸다. LK를 실행한 것이 아니다.
  - **[짐작]** 위 셋이 아니다.

## 1. 결론

- **트리를 고치면 LK를 통과할 것으로 본다.** 지금 보드 트리는 12곳에서 막히거나 흐름이 바뀐다 [코드, 모델]. 초안(5절)은 12곳을 모두 지난다 [모델]. 기기 확인은 없다.
- 앞서 본 `app_load_scp`는 첫째일 뿐이다. 나머지는 scp, apusys_rv, 모뎀 끄기, vcp, connsys, `emicen`, `emiisu`, `soc-dbg-error-flag`, `/product_region`, 보드의 `framebuffer_mem`과 `ramoops`, `/chosen`의 `bootargs`다.
- 놓치기 쉬운 셋:
  - `/product_region`이 없으면 ASSERT가 아니라 **지역 불일치 그림을 띄우고 재부팅한다**. 이 기기는 그 검사를 탄다 [기록].
  - **보드의 `framebuffer_mem`과 `ramoops`는 LK의 충돌 검사에 걸린다.** `status`도 보지 않는다. 그래서 패치 0140의 전제(보드 노드로 `no-map`을 준다)가 이 길에서는 성립하지 않는다 [코드].
  - **`gps`와 `connscp`를 트리에서 빼면 LK가 건너뛰는 것이 아니라 멈춘다.** `mt6897.dtsi` 주석과 설계안 v1.20의 "LK를 비켜 가게 한다"는 이 길과 맞지 않는다 [코드]. 10절에 어긋나는 곳을 모았다.
- **D6을 건너뛰는 길은 코드에 없다.** GPUEB는 트리를 보지 않고 늘 올린다. connsys는 노드나 크기가 모자라면 ASSERT다. 그래서 초안은 D6을 그대로 지킨다(6절).
- **권고: 시도한다.** 비용은 벤더 전용 노드 10개, 보드 노드 둘 빼기, 커널 고침과 설정 하나씩이고, 실패해도 `_a`로 돌아오며 `expdb`에 멈춘 곳이 남는다. D5(Wi-Fi), D7(GPU), 온도 보정이 LK가 트리에 적는 값을 쓴다(9절).

## 2. LK가 트리를 보는 차례

`boot_linux_fdt`(0x1078c)에서 차례대로다 [코드].

1. `vendor_boot`의 DT 표를 건너뛰어 첫 트리를 찾고, `dtbo`를 읽는다. `dtbo`가 0이면 `load_dtbo fail`을 찍고 overlay 없이 간다(실측).
2. `fdt_open_into`로 `mb_dtb`(4MiB)에 펼친다.
3. `io_task_callback`(0xf740)을 스레드로 돌린다. 적재 함수 11개를 차례로 부른다(3절). 이 사이 본 스레드는 커널을 풀고 명령줄을 모은다.
4. 스레드를 기다린 뒤 `set_fdt`(0x7a94c)가 링커 표의 훅 41개를 차례로 부른다(4절).
5. `Check region`(0x103c0), `mblock_fdt_append`(0x17858), `kcmdline_finalized`(0x1517c)를 부른다(4절).
6. `/chosen`에 `linux,initrd-start`와 `-end`를 쓰고, `bootconfig`(0xecd8)가 `androidboot.*`를 명령줄에서 떼어 initrd 뒤에 붙인다.
7. 루트에 `bootprof`를 더하고(0x11f0c), `fdt_pack` 뒤 커널로 뛴다.

실패는 모두 재부팅으로 끝난다. `_b`가 재시도를 다 쓰면 `_a`로 돌아온다(실측).

## 3. 적재 함수별 요구

`io_task_callback`은 **adsp의 -16만 봐주고 나머지는 0이 아니면 ASSERT로 멈춘다** [코드]. 번호는 `io_task.c`의 줄이다.

| 줄 | 함수(주소) | 트리에서 보는 것 | 노드가 없을 때 | `status`가 okay가 아닐 때 | 초안 |
|---|---|---|---|---|---|
| 189 | scp (0x7c2d4) | compatible `mediatek,scp`, `status`, `scp-sram-size`, `scpctl`, `secure-dump`, `secure-dump-size`, `scp-protect`, `scp-mem-tbl`, `memorydump` | -16, **ASSERT**(실측) | 같다. SCP 쪽 레지스터를 쓴 뒤 -16, **ASSERT** | 순정 값 |
| 190 | ccu (0x1b828) | compatible `mediatek,ccu_rproc`, `secured`, 크기들 | 0, 건너뜀 | 보지 않는다. `secured`가 `yes`가 아니면 0 | 없음 |
| 191 | spm (0x3682c) | compatible `mediatek,sleep`(있으면 `spmfw_version`을 쓴다) | 기록만, 0 | 보지 않는다 | 없음 |
| 192 | apu (0x19a0c) | `/mvpu`, `soc/mvpu` | 기록만, 0 | 보지 않는다 | 없음 |
| 193 | apusys_rv (0x19a50) | `/apusys-rv@190e1000`, `/soc/apusys-rv@190e1000`, `/apusys_rv`, `/soc/apusys_rv`의 `status` | **ASSERT**(`apusys_rv.c:463`). `status` 속성이 없어도 같다 | 기록만, 0, 건너뜀 | `disabled` |
| 194 | dpm (0x1d3e4) | 트리를 받지 않는다 | 해당 없음 | 해당 없음 | 없음 |
| 195 | modem (0x282b4) | `/mddriver`, `/soc/mddriver`의 `ref-args`에 `md_disable` | 언제나 0이지만, `md_disable`이 없으면 모뎀 길로 들어간다(아래) | 보지 않는다 | `md_disable` |
| 196 | adsp (0x774e4) | compatible `mediatek,mt6897-adspsys`, `status`, `core-num`, `mediatek,mt6897-adsp_core_<n>` | -16, **봐준다** | -16, 봐준다 | 없음 |
| 197 | gpueb (0x21b98) | 올릴지 말지에 트리를 보지 않는다. `/soc/gpueb`의 `gpueb-diagnosis-mode`만 읽는다 | 기록만 | 보지 않는다 | dtsi 그대로 |
| 198 | vcp (0x39144) | compatible `mediatek,vcp`, `status`, `vcp-support`, 크기들 | -16, **ASSERT** | -16, **ASSERT** | `okay`, `vcp-support` 없음 |
| 199 | connsys (0x3bb24) | `/soc/consys`→`/consys`, `/soc/gps`→`/gps`, `/soc/wifi`→`/wifi`, `/soc/connscp`→`/connscp` | -2, **ASSERT** | 보지 않는다 | 순정 크기 |

함수마다 더 알아 둘 것 [코드]:

- **scp:** `status`가 okay면 `SCP-reserved` 11MiB와 `reserve-memory-scp_share`(0x3d0000 + `secure-dump-size`)를 잡고 `scp` 파티션을 올린 뒤 SMC로 시작시키고 언제나 0을 돌려준다. 파티션 적재 실패의 반환값은 보지 않는다. 다음 조합은 ASSERT다: `secure-dump`를 켜고 크기가 0(`scp.c:138`), `scp-protect`를 켜고 `secure-dump`가 없음(`scp.c:153`).
- **vcp:** `status`가 okay이고 `vcp-support`가 없거나 0이면 아무것도 올리지 않고 0이다. `vcp-support`가 있는데 `vcp-sram-size`나 `vcp-dram-size`가 0이면 panic이다.
- **apusys_rv:** `status`를 앞 4바이트로 견준다. okay면 SMC와 `apusys` 파티션 적재로 간다.
- **modem:** 함수는 언제나 0을 돌려준다. 이 기기는 `Dev en:1`이어서 `md_disable`만이 모뎀 길을 막는다 [기록]. 없으면 `md1img`를 찾으러 간다. 이 기기에는 그 파티션이 없어 실패해 전원을 끌 것으로 본다 [짐작]. 굳이 탈 까닭이 없어 초안은 순정처럼 막는다.
- **gpueb:** `0x1c001f00`으로 MFG0 전원을 보고, 꺼져 있으면 -16으로 ASSERT다. `gpueb` 파티션을 올리고 부팅을 기다린다. 시간을 넘기면 -13으로 ASSERT다. 트리와 상관없다.
- **connsys의 크기 조건:** `/soc/consys`의 `emi-size`, `ro-emi-size`, `rw-emi-size`, `emi-alignment`, `emi-max-addr`, `/soc/gps`의 `emi-size`, `/soc/wifi`의 `emi-size`, `emi-alignment`, `emi-max-addr`가 0이 아니고, `connscp`의 `scp-shm-size`가 1~0x100000이어야 한다. 하나라도 어기면 -2로 ASSERT다.
- **connsys의 멈추는 곳:** `wfdma allocation failed`, `scp-shm allocation failed`, SMC `0xc200041a`의 답이 0x100이 아닐 때 `Fail to set connsys emi addr`와 `Fail to set gps emi addr`는 panic이다.
- **connsys가 트리에 쓰는 것:** `/soc/consys`와 `/gps`에 `emi-addr`, `mblock_fdt_append`에서 `/soc/wifi`에 `memory-region`.
- **`b_mode`:** 2이면 scp, ccu, spm, apu, apusys_rv, dpm, adsp, gpueb, connsys를 부르지 않고, 8이면 connsys만 건너뛴다. modem과 vcp는 언제나 부른다. 2는 복구 부팅, 8은 전원 끈 충전으로 본다 [짐작]. GPUEB를 잃으므로 쓸 길이 아니다.

11개가 모두 `_b`의 파티션과 순정 트리로 도는 것은 기록에서 봤다 [기록]. 파티션 쪽은 이 길에서도 그대로이고, 달라지는 것은 트리다.

## 4. 적재 뒤의 요구

| 곳(주소) | 보는 것 | 없으면 | 초안 |
|---|---|---|---|
| `set_fdt_emi_info`(0x1e7a8) | `/soc/emicen`→`/emicen` | 셋째로 NULL 경로를 찾으며 멈춘다. 넘어가도 ASSERT(`emi_info_v2.c:179`) | `/emicen` |
| 같은 곳 | `/soc/emiisu`→`/emiisu` | ASSERT(`emi_info_v2.c:98`) | `/emiisu` |
| `dbg_err_flag_set_fdt`(0x317b0) | preloader 태그가 준 compatible 문자열 | ASSERT(`dbg_err_flag.c:329`) | 노드 하나 |
| `/chosen`을 쓰는 훅 여럿 | `/chosen` | ASSERT, panic | dts에 있다 |
| `Check region`(0x103c0) | `/product_region`의 `region`, `magic` | **그림을 띄우고 10초 뒤 재부팅** | `PRC` 둘 |
| `mblock_fdt_append`(0x17858) | `/reserved-memory` | panic | dtsi에 있다 |
| 같은 곳 | `/memory` | ASSERT(`mblock.c:1756`) | dts에 있다 |
| 같은 곳 | `/reserved-memory` 자식의 `reg`가 LK의 mblock과 겹치는지 | **겹치면 ASSERT**(`mblock.c:1445`) | 둘을 뺀다 |
| `kcmdline_finalized`(0x1517c) | `/chosen`의 `bootargs` | ASSERT(`mt_boot.c:1072`) | `""` |
| `bootconfig`(0xecd8) | 같은 값 | panic(앞에서 이미 멈춘다) | 같다 |
| `kernel_debug_cmd_init`(0x791f0) | `/chosen`의 `kaslr-seed` | 없으면 건너뜀. 있는데 8바이트가 아니면 ASSERT | 없음 |
| 0x11f0c | 루트의 `bootprof` | 더한다. 이미 있으면 ASSERT | 없음 |

- **`Check region`** [코드, 기록]: 퓨즈가 끊긴 기기에서, RPMB 0x3f80의 값이 `PRC`나 `ROW`로 초기화돼 있으면 트리의 `region`을 RPMB 값과, `magic`을 LK 안의 상수와 앞 3바이트로 견준다. 이 LK의 상수는 `PRC`다. 이 기기의 기록은 `The device is fused`, `region has been init`, `buf = PRC`, `region match, normal boot`다. 언락과 상관없이 탄다.
- **`dbg_err_flag_set_fdt`** [코드, 기록]: 찾는 문자열은 LK 파일에 없고 preloader 태그에서 온다. 순정 트리에서 LK가 값을 써 넣은 노드의 compatible이 `mediatek, soc-dbg-error-flag`이니(쉼표 뒤 공백까지) 같은 문자열이다. 태그의 항목 수가 0이면 이 검사는 돌지 않지만, 값이 써 있으므로 이 기기에서는 1이다.
- **충돌 검사** [코드]: `status`를 보지 않고 `reg`만 본다. `status = "disabled"`로는 비켜 갈 수 없다. `alloc-ranges`가 있는 자식은 따로 검사하는데 우리 트리에는 없다.
- **겹치는 곳** [기록]: `framebuffer@fc16f000`은 `mblock-14-framebuffer`와, `ramoops@48090000`은 `mblock-2-pstore`와 같은 자리다. preloader에서 온 칸이라 우리 트리가 바뀌어도 그대로일 것으로 본다 [짐작].

**노드가 없어도 기록만 남기는 훅** [코드]: `met_set_fdt`, `platform_fdt_adsp_params`, `set_fdt_aod_scp`, `set_bat_data`, `set_charger_atm`, `csffw_fwlog_reserve_mem`, `set_fdt_disp_plat_dbg`, `set_fdt_dramc`, `leds_fdt_set_led_param`, `ccci_update_*` 둘, `mfgsys_reserve_init`, `set_fdt_imix_r`, `tracker_dconfig_update_to_fdt`, `slbc_set_fdt`, `memory_ssheap_fixup`, `memory_ssmr_fixup`, `sspm_set_fdt`, `mt6375_set_fdt_typec_info`, `mtk_wdt_doe_setup`, `disp_lcm_fill_lcm_dts_phandle`, `mmc_remove_prop`, `mrdump_key_fdt_init`. 이들은 `fdt_setprop` 실패에는 멈추는 것이 있으나 노드가 없어서 멈추지는 않는다. 이 가운데 `mfgsys_reserve_init`이 `/soc/gpueb`에 `gpueb_mem_addr`, `gpueb_mem_size`를 쓴다.

**LK가 우리 트리를 바꾸는 것** [코드]: `/memory`의 `reg`, `mblock_info`, `/reserved-memory`의 `mblock-<n>-<이름>` 노드, `/firmware/android`, `/debug-kinfo`, `/md_attr_node`, `/chosen`의 `atag,*`, `aee,*`, `bootargs`. 루트에 `model`이 있으면 `MT8792`로 덮어쓴다.

## 5. 최소 노드 초안

보드 dts 끝에 더하는 꼴이다. 값은 살아 있는 벤더 트리에서 읽었다(일련번호가 있는 `bootargs`는 옮기지 않았다). `/soc/consys`와 `/soc/wifi`는 `mt6897.dtsi`에 이미 있고 값이 순정과 같아 더하지 않았다. `/soc/gpueb`도 있다.

```dts
/ {
	chosen {
		/* mt_boot.c:1072, bootconfig.c: 없으면 멈춘다. 내용은 boot와 vendor_boot에서 온다 */
		bootargs = "";
	};

	/* 지역 검사: RPMB의 값, LK의 상수와 같아야 한다. 다르면 재부팅한다 */
	product_region {
		region = "PRC";
		magic = "PRC";
	};

	/* io_task.c:189. status가 okay여야 한다. 값은 순정 트리와 같다 */
	scp {
		compatible = "mediatek,scp";
		status = "okay";
		scp-sram-size = <0x200000>;
		secure-dump = "enable";
		secure-dump-size = <0x4c8000>;
		scp-protect = "enable";
		scp-mem-tbl = <0x00 0x00 0x00>, <0x01 0xca700 0x00>,
			      <0x02 0x100000 0x00>, <0x03 0x180000 0x00>,
			      <0x04 0x19000 0x00>, <0x05 0xa000 0x00>,
			      <0x07 0x1a000 0x00>, <0x08 0x10000 0x00>,
			      <0x09 0x1000 0x00>, <0x0a 0x2000 0x00>,
			      <0x0b 0x100 0x00>, <0x0c 0x100 0x00>,
			      <0x0f 0x10000 0x00>, <0x11 0x8000 0x00>,
			      <0x12 0x8000 0x00>;
		memorydump = <0x200000 0xc4000 0x3c00 0x400 0x200000>;
	};

	/* io_task.c:198. status가 okay여야 한다. vcp-support가 없으면 아무것도 올리지 않는다 */
	vcp {
		compatible = "mediatek,vcp";
		status = "okay";
	};

	/* apusys_rv.c:463. 노드와 status가 있어야 한다. okay가 아니면 올리지 않는다 */
	apusys-rv@190e1000 {
		reg = <0 0x190e1000 0 0x1000>;
		status = "disabled";
	};

	/* io_task.c:199. /soc/consys, /soc/wifi와 함께 있어야 한다. LK는 status를 보지 않는다 */
	gps {
		emi-size = <0x100000>;
	};

	connscp {
		scp-shm-size = <0x20000>;
		scp-remap-offset = <0x2100000>;
		scp-remap-size = <0x20000>;
	};

	/* 없으면 LK가 모뎀 길로 들어간다(Dev en:1) */
	mddriver {
		ref-args = "md_disable";
	};

	/* emi_info_v2.c:179, :98 */
	emicen {
	};

	emiisu {
	};

	/* dbg_err_flag.c:329. compatible의 공백까지 순정과 같아야 한다 */
	soc-dbg-error-flag {
		compatible = "mediatek, soc-dbg-error-flag";
		status = "disabled";
	};
};

/* mblock.c:1445: LK의 mblock과 겹치는 reg가 있으면 멈춘다. status는 보지 않는다 */
&reserved_memory {
	/delete-node/ framebuffer@fc16f000;
	/delete-node/ ramoops@48090000;
};
```

- **노드를 루트에 둔 까닭:** LK는 `/soc/…` 다음에 `/…`를 찾는다. `/soc`(`simple-bus`) 아래에 `reg` 없이 두면 dtc의 `simple_bus_reg` 경고가 나는데 루트에는 없다.
- **커널 드라이버가 붙지 않는다** [코드]:
  - `compatible`이 없는 노드는 `of_platform_populate`가 장치를 만들지 않는다(`drivers/of/platform.c`의 strict 검사).
  - `mediatek,scp`와 `mediatek,vcp`는 `status`가 okay여야 해서 장치는 생기지만, 7.2.12와 패치 대기열에 이 compatible을 찾는 드라이버가 없다. `mediatek,scp`는 메인라인에서 phandle 속성 이름으로만 쓴다.
  - `soc-dbg-error-flag`는 `disabled`로 두어 장치도 생기지 않는다. LK는 이 노드의 `status`를 보지 않는다.
- **검사** [모델]: 위 초안을 지금 보드 트리에 얹어 컴파일하면 dtc 경고가 없다. 조회 모델은 지금 트리에서 12건, 초안에서 0건이다. `fdtget`(libfdt)으로 `/soc/consys`, `/soc/wifi`처럼 유닛 주소 없이 찾는 경로도 확인했다. `dt-validate`는 벤더 속성과 공백 있는 compatible을 지적한다. 그래서 이 노드는 업스트림에 낼 수 없고, 보드 dts에서 떼어 따로 두는 편이 낫다.

## 6. D6을 지키는 쪽과 건너뛰는 쪽

**D6에 걸린 둘은 건너뛸 수 없다** [코드]. 초안은 둘 다 LK가 올리게 둔다.

| 블록 | 트리로 끌 수 있나 | 초안의 결과 |
|---|---|---|
| GPUEB | 없다. 트리를 보지 않고 올린다. 파티션을 망치면 ASSERT다 | LK가 올리고, `/soc/gpueb`에 공유 메모리를 적는다 |
| connsys Wi-Fi, BT, GNSS | 없다. 노드와 크기가 모자라면 ASSERT다 | LK가 칸을 나누고 `connsys_bt`, `connsys_gnss`, `connsys_wifi`를 올리고 `emi-addr`, `memory-region`을 적는다 |

크기를 작게 적어 칸을 줄이는 우회는 권하지 않는다. 순정에서 돈 적이 없고, SMC가 거부하면 panic이며, 이미지 크기가 칸을 넘을 때의 동작은 다 읽지 않았다.

**D6 밖의 블록**은 끌 수 있는 것을 끈다. 커널에 소비자가 아직 없고, LK가 잡는 메모리가 약 55MiB 줄어든다(순정 트리의 칸으로 계산) [기록, 계산].

| 블록 | 초안 | 나중에 쓰려면 |
|---|---|---|
| SCP | **못 끈다.** 순정 값으로 올린다 | 파티션 `scp_b`를 비우면 적재만 실패하고 지나간다 [코드]. 시험은 없다 |
| SPM, DPM | 트리와 상관없이 올라간다 | 해당 없음 |
| VCP | `vcp-support`를 빼 건너뛴다 | 순정의 `vcp-support`, 크기들, `vcp-mem-tbl`, `memorydump`, `vcp-secure-dump-*`를 더한다 |
| APUSYS | `status = "disabled"` | 순정 노드를 옮긴다 |
| ADSP | 노드 없음 | `mediatek,mt6897-adspsys`와 `adsp_core_<n>`을 옮긴다. 오디오 일을 할 때 다시 본다 |
| CCU | 노드 없음 | `mediatek,ccu_rproc`, `secured = "yes"` |
| 모뎀 | `md_disable` | 해당 없음 |

SCP를 `status`만 두는 더 작은 꼴도 ASSERT는 지나간다. 그러면 `scp-sram-size`가 0이어서 LK가 SCP SRAM 머리에 적는 크기가 0이 되고, `secure-dump`와 메모리 표도 빠진 채 시작 SMC까지 간다 [코드]. SCP가 그 뒤 어떻게 되는지는 모른다 [짐작]. 순정에서 돈 적이 없는 길이고 얻는 것은 메모리 약 5MiB라 초안에 넣지 않았다.

순정 값으로 올린 SCP는 커널에 드라이버가 없어도 문제가 없을 것이다. 지금의 시험 커널이 이미 그 상태로 도는 까닭이다 [기록].

## 7. 트리 밖에서 함께 바꿀 것

1. **framebuffer의 `no-map`.** 초안은 `framebuffer_mem`을 뺀다. LK의 `mblock-14-framebuffer`에는 `no-map`이 없어, 이대로면 simpledrm이 버퍼를 매핑하지 못해 화면이 나오지 않는다(패치 0140의 커밋 메시지).
   - 0140은 보드 노드가 있어야 뜻이 있으므로 이 길에서는 효과가 없다.
   - 후보: `__reserved_mem_reserve_reg()`에서 compatible `mediatek,framebuffer`를 `no-map`으로 다루는 작은 패치 [짐작]. 시험하지 않았다.
   - 없어도 LK와 커널 부팅은 된다. pstore로 기록을 읽을 수 있다.
2. **`ramoops`.** 초안은 노드를 뺀다. LK가 `mblock-2-pstore`를 잡고 명령줄에 `ramoops.*`를 넣으므로 그대로 동작할 것으로 본다 [기록, 짐작].
3. **`CONFIG_BOOT_CONFIG=y`.** LK는 명령줄의 `androidboot.*`를 떼어 initrd 뒤에 붙이고 `bootconfig`를 명령줄에 더한다. 지금 `out/.config`에는 꺼져 있다.
   - 꺼진 채로는 initramfs를 푼 뒤 그 꼬리에서 `Initramfs unpacking failed: invalid magic`을 찍는다. 이미 푼 내용은 남고 부팅은 이어진다(`init/initramfs.c`) [코드].
4. **UART.** 시험 dts처럼 `&uart0`을 끄고 `stdout-path`를 뺀다. `mkimages.sh`의 기본 명령줄에는 `console=ttyS0,921600n8`과 `earlycon`이 있으니, 첫 시도의 `boot` 명령줄은 시험 dts의 것을 그대로 쓴다(`buntos.autoreboot`로 기록을 읽는다).
5. **명령줄.** `boot`와 `vendor_boot`의 명령줄은 LK가 덧붙인다(0x12178, 0x1218c) [코드]. 그래서 `bootargs`는 비워도 된다. LK가 덧붙이는 `root=/dev/ram`은 `initramfs/init`이 무시한다. 합친 길이가 2047바이트를 넘으면 panic이다.
6. **`model`.** LK가 `MT8792`로 덮어쓴다 [기록]. 시스템 설정(`settings/src/system.rs:113`)의 기기 이름이 그렇게 보인다. `model`을 트리에서 빼면 LK가 쓰지 않지만 설정은 이름을 잃는다. 어느 쪽이든 설정 쪽을 함께 고쳐야 한다.
7. **TB373FU.** `mt6897-lenovo-tb373fu.dts`가 이 보드를 포함한다. 글로벌판의 RPMB 값과 LK 상수는 `ROW`일 것으로 보지만 확인하지 않았다 [짐작]. `/product_region`은 판마다 따로 둔다.
8. **KASLR.** `/chosen`에 8바이트 `kaslr-seed`를 두면 LK가 값을 채운다 [코드]. 선택이다.

## 8. 남은 불확실성

1. **ATF의 connsys SMC.** `0xc200041a`는 답이 0x100이 아니면 panic이다. 순정은 통과한다 [기록].
   - APUSYS를 건너뛰면 `overall_pa`가 `0x8cc00000`에서 `0x8ce00000`으로 옮겨 갈 것으로 계산했다 [짐작].
   - ATF가 주소를 어디까지 보는지 모른다. 걸리면 LK 기록에 `Fail to set connsys emi addr`가 남는다. APUSYS를 순정 값으로 되돌리면 같은 자리로 온다 [짐작].
2. **VCP, APUSYS, ADSP, CCU를 올리지 않은 상태.** LK 안의 길은 코드로만 봤다. 커널에 소비자가 없어 문제없을 것으로 본다 [짐작]. TF-A, SSPM, SPM이 이들의 상태를 보는지는 모른다. 서스펜드에서 처음 드러날 수 있다.
3. **훅이 우리 트리에서 처음 돈다.** 41개를 읽었으나 `fdt_setprop` 실패 경로와 레지스터 접근까지 다 따라가지는 않았다. 간접 호출(`blr`) 안도 일부만 봤다. `Bad DTB header`, 메모리 부족 같은 트리 밖의 실패는 표에 넣지 않았다. 훅과 적재 함수 밖의 `fdt_*` 호출도 훑었는데, 플랫폼 초기화(`lastbus_init(fdt)` 등)는 LK 자기 트리를 본다고 읽었다.
4. **`mblock` 할당이 달라진다.** 빠지는 칸이 있어 위쪽 할당의 자리가 바뀔 수 있다. 할당 실패는 panic인데, 빠지기만 하므로 실패하지 않을 것으로 본다 [짐작].
5. **지역 불일치의 재시도.** ASSERT와 다른 재부팅 이유(`0x1cfb0(2, 7)`)를 쓴다. 재시도 횟수를 똑같이 쓰는지는 보지 않았다 [짐작]. `_a`로 못 돌아오면 사용자가 `fastboot --set-active=a`를 한다.
6. **커널 쪽.** LK가 더하는 `mblock-*`, `/debug-kinfo`, `/md_attr_node`, `/soc/wifi`의 `memory-region`이 7.2.12에서 문제를 내는지. 드라이버가 없거나 꺼져 있어 괜찮을 것으로 본다 [짐작]. `no-map`이 아닌 칸(`framebuffer`, `aee_lk`, `gz-log`, `log_store`)도 그대로 받는다.
7. **다른 판.** 16.0.20.091만 읽었다. 12GiB 기기와 다른 LK 판은 칸과 검사가 다를 수 있다.

## 9. 이 길을 택할지

| | 지금(시험 커널) | LK 길 |
|---|---|---|
| LK가 읽는 트리 | 순정 `vendor_boot` | 우리 `vendor_boot` |
| 메모리와 예약 | 8GiB, 16.0.20.091에서 옮긴 고정값 43개 | LK가 채운다. 12GiB와 다른 칸에도 맞는다 |
| GPU 공유 메모리(D7) | LK가 순정 트리에 적고 버려진다 | `/soc/gpueb`에 적힌다 |
| Wi-Fi `emi-addr`, `memory-region`(D5) | 같이 버려진다 | 적힌다 |
| `atag,*`(`devinfo`의 온도 보정, 칩 ID, 화면) | 없다 | 있다 |
| 커널 | `setup_machine_fdt`를 고친다. 올릴 수 없는 꼴이다 | 보통 커널. `no-map` 고침 하나와 설정 하나 |
| 트리 | 보드 트리에 시험 층 하나 | 벤더 전용 노드 10개를 따로 둔다. 업스트림에 내지 않는다 |
| LK가 올리는 코프로세서 | 순정 전부 | SCP, GPUEB, connsys, SPM, DPM |
| 부팅 | **실측으로 된다** | **실측 없다.** 실패해도 `_a`로 돌아온다 |

- **권고: 시도한다.** 위 표의 값들은 Wi-Fi, GPU, 온도를 켤 때 어차피 필요하다. 지금 길로 가려면 기기와 판마다 손으로 옮기거나, 커널 진입에서 LK 트리의 값을 합치는 코드를 더 짜야 한다. 그쪽이 LK 위험은 없지만 더 무겁고 올릴 수 없다.
- **언제.** 시험 커널은 이 길이 통과할 때까지 둔다. 2b(USB, UFS)를 막지 않는다. 다만 Wi-Fi(2d)나 GPU(2e)를 켜기 전에는 정해야 한다.
- **시도 차례:**
  1. 초안을 `mt6897-lenovo-tb375fc-lk.dtsi` 같은 파일로 떼어 포함하고, 시험 층의 UART 설정을 옮긴다.
  2. `CONFIG_BOOT_CONFIG=y`를 켠다. `no-map` 고침은 있으면 넣고, 없으면 pstore로 본다.
  3. `mkimages.sh`로 `boot`, `init_boot`, `vendor_boot`를 만든다. `dtbo_b`는 0인 채로 둔다.
  4. `_b`에 굽고 `--set-active=b`다. 굽기는 사용자 결정이다.
  5. `_a`로 돌아온 뒤 `expdb`와 pstore를 읽는다.
- **기록에서 볼 줄** [기록의 꼴]:
  - `[PROFILE] ::: load connsys takes`, `overall_pa=`
  - `set_fdt:34: runs platform_fdt_set_ufs hook function`(훅의 끝)
  - `[Check region] region match, normal boot`
  - `setup_mem_property_use_mblock_info`
  - `Set device: /soc/wifi, memory-region=`
  - 멈추면 `ASSERT FAILED at (…)`이 어디인지가 표의 줄 번호와 맞는다.

## 10. 기존 기록과 어긋나는 곳

이 작업에서 고치지 않았다. 이 길을 택하면 함께 고친다.

| 곳 | 적힌 내용 | 코드로 본 것 |
|---|---|---|
| `mt6897.dtsi` connsys 주석, `kernel/tb375fc/dts/README.md` 2d 줄 | `gps`, `connscp`를 빼 두면 LK가 비켜 간다 | 메모리는 나누지 않지만 -2를 돌려주고 호출자가 ASSERT한다 |
| 설계안 v1.20, 위험 표(`LK가 mainline 트리의…`) | -2로 돌아오고 아무것도 싣지 않는다 | -2 뒤에 ASSERT가 있다. 지금은 앞의 `app_load_scp`에서 멈춰 드러나지 않았다 |
| 보드 dts `framebuffer_mem`, `ramoops` 주석, 패치 0140 메시지 | LK 노드와 보드 노드가 함께 있다고 본다 | 함께 있으면 LK가 `mblock.c:1445`에서 멈춘다 |
| 실측 보고서 7절 "우리 트리로 LK 통과" 줄 | 찾는 벤더 노드에 `mediatek,sleep`, `/soc/gpueb`를 든다 | 이 둘은 없어도 기록만 남긴다. 표에 없는 `apusys_rv`, `emicen`, `emiisu`, `soc-dbg-error-flag`, `/product_region`이 필요하다 |
| 체크리스트 5.3절 "그 노드를 더해야 이 길이 열린다" | 적재 노드만 말한다 | 4절의 요구도 있다 |

`docs/tb375fc/README.md`의 문서 표에 이 문서를 더하는 일도 남았다.

## 부록: 다시 보는 법

- LK와 도구: `.handoff/tb375fc-dev-lk/sections/`의 `stock-lk.bin`, `dis.py`(git 밖). 빌드 VM에서 돌린다.
- 이 조사에서 만든 것: `.handoff/tb375fc-lk-tree/`(git 밖).
  - `hooks.py`: 훅 표(파일 오프셋 0x101490, 16바이트씩 41개, 기준 주소 `0xffff000050700000`)를 푼다.
  - `cfg.py`: 함수에서 닿는 ASSERT와 `fdt_*` 호출을 모은다.
  - `lk_model.py <dtb> <벤더 dts>`: 3절과 4절의 조회를 옮긴 모델이다. 지금 트리에서 12건, 초안에서 0건이 나온다. LK를 대신하지 않는다.
- 기기 기록: `.handoff/tb375fc-device-readings/boot-b-7/expdb.str`의 순정 트리 부팅. 줄에 기기 값이 섞여 있으니 그대로 옮기지 않는다.
