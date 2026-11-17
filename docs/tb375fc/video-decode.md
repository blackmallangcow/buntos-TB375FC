# 하드웨어 비디오 디코딩: 가능성 조사와 단계 계획

- 작성일: 2026-11-17
- 대상: TB375FC(MT6897), 포크 main `8db4112`, 커널 7.2.12 + 패치 대기열, 순정 16.0.20.091의 LK와 VCP 펌웨어, Firefox ESR 153.5.1
- 목적: buntOS에서 칩의 비디오 디코더를 쓰려면 무엇이 필요한지 정하고, 끝마다 시험할 수 있는 단계로 나눈다. 계기는 YouTube다. Firefox가 3840x2160@60 VP9를 CPU로 풀어 3672프레임 가운데 3600을 버렸다(2026-11-17 실측).
- **조사(1~6절, 8절)는 읽고 재기만 했다.** 태블릿에서 한 일은 읽기다(커널 설정, 장치 노드, 패키지, 전원 도메인 요약, 그리고 1단계 앞의 레지스터 몇 개). 1단계의 코드는 조사 뒤에 썼고 7.1절에 적는다.
- 표시:
  - **[실측]** 이 조사에서 태블릿이나 원본을 직접 읽었다.
  - **[기록]** 앞서 받아 둔 기기 기록(순정 장치 트리, LK 기록, 순정 `/proc/iomem`)에서 봤다.
  - **[코드]** 소스를 읽었다. 돌려 보지 않았다.
  - **[조사]** 웹의 원본(lore, patchwork, 저장소, 공식 문서)을 열어 봤다. 출처는 10절에 있다.
  - **[짐작]** 위 넷이 아니다.

## 1. 결론

- **할 수 있다고 본다. 다만 큰 일이고, 기기에서 확인된 것은 아직 하나도 없다.** 커널 쪽 셋(IOMMU·SMI, VCP, 디코더)과 사용자 공간 하나(VA-API 드라이버)를 차례로 만든다.
- 순정은 **VCP 펌웨어가 비트스트림을 푸는 stateful 디코더**다. 펌웨어는 서명돼 있어 바꿀 수 없으므로, 우리 커널이 순정 펌웨어의 IPI 규약을 따라야 한다(2절, 3절).
- **벤더 커널 소스는 공개돼 있다**(OnePlus 공식, GPL-2.0). 펌웨어의 소스는 없다(3절).
- **업스트림 MT8196은 틀이 되지 못한다.** 아직 하나도 병합되지 않았고, 디코더는 stateless이며 IOMMU는 다른 하드웨어다. 가장 가까운 틀은 형제 칩 MT6895의 커뮤니티 트리다. mainline 위에서 같은 방식(LK가 올린 펌웨어, 같은 SMC, stateful 디코더)으로 만든 것이 있다(5절).
- **부트 체인은 지금 그대로 된다.** 지금 태블릿은 LK가 순정 트리를 읽으므로, 부팅마다 VCP 펌웨어가 서명 검증을 거쳐 이미 올라가 있다. 장치 트리에 `vcp-support`를 더하는 일은 LK 길로 옮길 때의 일이다(4절).
- **Firefox로 가는 길은 VA-API다.** 형제 칩용으로 stateful 디코더를 VA-API로 내는 드라이버가 있고, VP9(8비트)·H.264·HEVC를 낸다. YouTube는 AV1을 끄고 VP9로 받는다. **AV1을 하드웨어로 푸는 길은 지금 없다**(6절).
- **`comp/`는 고칠 것이 없다.** Firefox가 디코더의 dmabuf를 자기 GL로 가져와 합성한 뒤 지금과 같은 버퍼를 컴포지터에 준다(6.3절).
- 가장 큰 불확실성 넷:
  1. TF-A가 우리 커널의 리셋 풀기를 받아 주는지(2단계).
  2. 펌웨어의 MMDVFS가 우리 커널의 화면 클럭·전압과 다투는지(2~3단계).
  3. 펌웨어의 출력이 블록 형식뿐일 때 4K60에서 푸는 비용(3단계).
  4. Firefox가 그 VA-API 드라이버를 받는지(4단계).
- **첫 단계(IOMMU와 SMI)는 시험 커널과 재부팅이 필요하다.** 조사는 여기서 멈췄고, 사용자가 1단계를 시작하라고 한 뒤 패치 0210~0212와 장치 트리 노드를 만들었다(7.1절). 단계 표는 7절이다.

## 2. 하드웨어와 펌웨어

### 2.1 디코더 블록

순정 장치 트리의 `vdec@16000000`(`mediatek,mt6897-vcodec-dec`)에서 읽었다 [기록: `.handoff/tb375fc/ref/cn-zui16-merged.dts:12609`]. 순정 커널에서 이 노드에 드라이버가 붙어 있었다 [기록: `root-091/iomem.txt:162`].

| 항목 | 값 |
|---|---|
| 레지스터 | 13구역. `VDEC_BASE` 0x16000000, `VDEC_SYS` 0x1602f000, `VDEC_VLD` 0x16020000, `VDEC_MC` 0x16021000, `VDEC_MV` 0x16023000, `VDEC_MISC` 0x16025000, `VDEC_LAT_MISC` 0x16010000, `VDEC_LAT_VLD` 0x16011000, `VDEC_SOC_GCON` 0x1600f000, `VDEC_RACING_CTRL` 0x16004000, `VDEC_LAT_AVC_VLD` 0x16012000, `VDEC_AVC_VLD` 0x16022000, `VDEC_AV1_VLD` 0x16015800 |
| 구조 | 앞단(LAT)과 뒷단(core) 두 단이다. 인터럽트는 SPI 701, 704 |
| 전원 도메인 | VDE0(12), VDE1(13). 포크의 표에 이미 있다(패치 0054) |
| 클럭 | `vdecsys_soc`의 게이트 둘, `vdecsys`의 게이트 하나(패치 0104에 있다). 부모는 topckgen의 `vdec` 먹스(패치 0013의 `CLK_TOP_VDEC`) |
| 클럭과 전압 | 218, 273, 436, 594, 660 MHz에 575, 600, 650, 725, 750 mV(`opp-table-vdec`) |
| 버스 | 뒷단은 SMI larb4(0x1602e000, VDE1)이고 `smi_mdp_common`을 거쳐 `mdp_iommu`(0x1e810000)에 닿는다. 앞단은 larb5(0x1600d000, VDE0)이고 `smi_disp_common`을 거쳐 `disp_iommu`(0x1e802000)에 닿는다. 곧 **디코더는 두 IOMMU를 다 쓴다.** 순정은 두 IOMMU가 페이지 표 하나를 함께 본다 |
| 최대 폭 | `vdec-max-w = <0x1000>`(4096) |

- **코덱:** MediaTek 공식 사양은 재생 코덱을 H.264, HEVC, VP-9, AV1로 적고 "4K AV1 media decoding"을 든다 [조사]. 벤더 드라이버가 아는 fourcc는 H.264, H.265, HEIF, VP8, VP9, AV1, MPEG-1/2/4, H.263, WMV, RV30/40이다 [코드: `vdec/vdec_vcp_if.c:518`].
- **이 기기의 실제 목록과 한계는 재지 않았다.** 지원 형식과 크기 범위는 드라이버에 없고 펌웨어가 실행 중에 답한다(`GET_PARAM_VDEC_CAP_SUPPORTED_FORMATS`, `GET_PARAM_VDEC_CAP_FRAME_SIZES`) [코드: `mtk_vcodec_dec.c:165`, `:207`]. 4K60이 되는지, AV1이 10비트까지인지는 3단계에서 펌웨어에 물어야 안다.
- **출력 형식도 펌웨어가 정한다.** 드라이버가 아는 것은 MediaTek 블록 형식(`MM21`, 10비트 `MT2T`·`MT2R`), AFBC·HyFBC로 압축한 NV12, P010이다 [코드: `mtk-v4l2-vcodec.h:19~93`]. 벤더의 `V4L2_PIX_FMT_MT21`은 fourcc가 `MM21`이라 업스트림의 `V4L2_PIX_FMT_MM21`과 같은 값이다. 압축하지 않은 보통 NV12는 디코더 코드에 나오지 않는다. 그래서 앱에 넘기기 전에 형식을 바꾸는 일이 생길 수 있다(5절).

### 2.2 순정이 디코더를 돌리는 방식

벤더 커널 소스를 읽었다 [코드]. 줄 번호는 OnePlus 공식 갈래(3절)의 `kernel/kernel_device_modules-6.1/` 아래다.

- **비트스트림을 푸는 것은 커널이 아니라 VCP 펌웨어다.** 커널의 V4L2 드라이버는 앱이 준 비트스트림 버퍼와 빈 프레임 버퍼의 주소를 공유 구조체(VSI)에 적고 IPI로 "풀어라"를 보낸다(`vdec_vcp_decode`, `vdec/vdec_vcp_if.c:1339`). 헤더 해석, 참조 프레임 관리, 디코더 레지스터 쓰기는 펌웨어가 한다. 펌웨어 안에 디코더 인터럽트를 기다리는 코드의 문자열이 있다 [기록: `boot-b-7/expdb.str:29584`].
- 그래서 **순정 드라이버는 stateful V4L2 디코더다.** 업스트림 `mtk-vcodec`의 MT8183~MT8195처럼 앱이 프레임마다 해석한 헤더를 넘기는 stateless(request API)가 아니다.
- IPI는 작다. 커널이 보내는 명령이 아홉(INIT, START, DEINIT, RESET, SET_PARAM, QUERY_CAP, FRAME_BUFFER, BACKUP, RESUME), 펌웨어가 커널에 시키는 일이 열하나다(프레임 돌려주기, LAT·core 잠그고 풀기, 메모리 할당과 해제, 인터럽트 기다리기 등) [코드: `vdec_ipi_msg.h:52~92`].
- 펌웨어가 시키는 일에서 커널이 맡는 것:
  - 전원과 클럭: `LOCK_LAT`, `LOCK_CORE`가 오면 전원 도메인과 게이트를 켠다(`vdec/vdec_vcp_if.c:774~800`).
  - 메모리: 펌웨어의 작업 버퍼를 dma-buf 힙에서 잡아 IOMMU 주소로 돌려준다(`handle_vdec_mem_alloc`, `:402`).
  - 인터럽트: 디코더 인터럽트는 커널이 받고(`mtk_vcodec_intr.c:431`), 펌웨어의 `WAITISR`에 답한다.
- **`vdec` 클럭 먹스와 전압은 펌웨어 쪽 MMDVFS가 다룬다.** 순정 트리는 `vdec-mmdvfs-in-vcp = <1>`이고, 디코더를 열 때 커널이 `mtk_mmdvfs_enable_vcp()`를 부른다(`mtk_vcodec_dec_drv.c:111`). 커널의 MMDVFS 드라이버는 VCP가 준비되면 초기화 IPI 둘(`FUNC_MMDVFS_INIT`, `FUNC_MMDVFSRC_INIT`)을 보낸다(`drivers/soc/mediatek/mtk-mmdvfs-v3.c:1602`). 순정 트리의 MMDVFS는 `vdec`만이 아니라 `disp`, `mdp`, `smi_common`, `vcore`, `vmm`도 든다 [기록: `cn-zui16-merged.dts:5722`]. 우리 커널은 화면 먹스를 스스로 쥐고 있어, 이 초기화를 그대로 보내면 펌웨어와 다툴 수 있다(8절 위험 2).

### 2.3 VCP

VCP(벤더 코드의 다른 이름 MMuP)는 멀티미디어용 RISC-V 코어다(이미지 이름 `tinysys-vcp-RV55_A`). 순정 트리의 `vcp@1ec00000`에서 읽었다 [기록: `cn-zui16-merged.dts:14360`].

| 항목 | 값 |
|---|---|
| 레지스터 | SRAM 0x1ea00000(256 KiB), 설정 0x1ec24000 등, 메일박스 다섯(0x1ecfb000~0x1ecff000) |
| 인터럽트 | 워치독 SPI 857, 메일박스 SPI 859~863 |
| 전원 도메인 | MM_PROC(8). 포크의 표에 있다(패치 0054). 지금 태블릿에서는 꺼져 있다: 전원 도메인 요약에 `mm-infra`만 보인다 [실측] |
| 클럭 | topckgen의 `mmup` 먹스(패치 0013의 `CLK_TOP_MMUP`)와 둘 |
| IOMMU | `mdp_iommu`의 larb43 포트 0(`VIDEO_UP`). 보통 영역은 도메인 5(IOVA 0x1_4000_0000부터 1.5 GiB), 보안 재생은 도메인 4 [코드: `include/dt-bindings/memory/mt6897-larb-port.h:630`, `drivers/iommu/mtk_iommu.c:550`] |
| 메모리 | SRAM 256 KiB, DRAM 8 MiB(`vcp-dram-size`), LK가 잡는 예약 영역 `me_vcp_reserved` 8 MiB |

커널이 VCP를 켜는 차례 [코드: `drivers/misc/mediatek/vcp/rv/vcp_helper.c`]:

1. MM_PROC 전원 도메인과 클럭 셋을 켠다(`vcp_enable_pm_clk`, `:1090`).
2. SMC `MTK_SIP_KERNEL_DAPC_MMUP_CONTROL`로 접근 보호를 연다(`:302`).
3. 펌웨어의 작업 메모리를 dma-buf 힙에서 잡아 IOMMU로 매핑하고(`vcp_alloc_iova`, `:2141`), 그 IOVA와 크기를 VCP 레지스터 둘에 쓴다(`reset_vcp`, `:1468`).
4. SMC `MTK_SIP_TINYSYS_VCP_CONTROL`(`RESET_RELEASE`)로 리셋을 푼다. **리셋을 푸는 것은 커널이 아니라 TF-A다.**
5. 0.8초 안에 메일박스로 준비 IPI(`IPI_IN_VCP_READY`)가 온다(`:796`, `:3650`).

- **펌웨어 이미지는 커널이 올리지 않는다.** LK가 `vcp` 파티션에서 읽어 서명을 검증하고 올린다(4절). 커널 드라이버에는 `request_firmware`가 없다.
- **VCP의 메모리 접근은 IOMMU를 지난다.** 커널이 VCP에 주는 주소가 IOVA다. 그래서 IOMMU와 SMI가 VCP보다 먼저다.
- 같은 펌웨어 안에 디코더, 인코더, MMDVFS, MMQOS가 함께 있다. IPI 번호 표는 `drivers/misc/mediatek/vcp/include/vcp.h:84~125`에 있다. 디코더는 메일박스 0의 `IPI_OUT_VDEC_1`·`IPI_IN_VDEC_1`만 쓴다.

### 2.4 태블릿의 지금 상태 [실측, 2026-11-17]

| 항목 | 값 |
|---|---|
| 커널 | 7.2.12-tb375fc. `/dev/video*`, `/dev/media*`, `/dev/dma_heap/`이 없다 |
| 커널 설정 | `CONFIG_MTK_IOMMU=y`, `CONFIG_MTK_SMI=y`, `CONFIG_VIDEO_DEV=m`, `CONFIG_V4L2_MEM2MEM_DEV=m`, `CONFIG_VIDEO_MEDIATEK_VCODEC=m`, `CONFIG_DMABUF_HEAPS=y`, `CONFIG_DMABUF_HEAPS_SYSTEM=m`, `CONFIG_MAILBOX=y`, `CONFIG_REMOTEPROC=y`. 틀은 이미 빌드되고, MT6897 데이터와 노드가 없어 아무것도 붙지 않는다 |
| 장치 트리 | `vcp`, `vdec`, IOMMU, SMI 노드가 없다. 예약 영역 `mblock-28-me-vcp-reserved@beb00000`은 있다 |
| 전원 도메인 | `mm-infra`만 켜져 있다. MM_PROC, VDE0, VDE1은 트리에 적지 않아 요약에 없다 |
| 패키지 | `firefox-esr` 153.5.1esr-1, `libva2` 2.24.1, `libdav1d7`, `libvpx12`, `gstreamer1.0-plugins-good` 1.28.9. `ffmpeg`·`libavcodec`, `v4l-utils`, `vainfo`는 없다 |
| 컴포지터 | linux-dmabuf 형식·수정자 608쌍을 알린다. NV12가 그 안에 있는지는 재지 않았다(`wayland-info`가 없다) |

## 3. 벤더 소스의 공개 상태

| 출처 | 내용 | 확인 |
|---|---|---|
| OnePlus 공식 `OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6897`, 갈래 `oneplus/mt6897_b_16.0.0_oneplus_pad`(커밋 `76792af`) | `drivers/media/platform/mtk-vcodec/`(디코더·인코더, 약 2만 3천 줄), `drivers/misc/mediatek/vcp/`(약 9천 5백 줄), `drivers/soc/mediatek/mtk-mbox.c`·`mtk_tinysys_ipi.c`(1,737줄), `mtk-mmdvfs-v3.c`, MT6897 데이터가 든 `drivers/iommu/mtk_iommu.c`(`:3691`)와 `drivers/memory/mtk-smi.c`(`:2517`, `:4111`), `include/dt-bindings/memory/mt6897-larb-port.h`. 파일마다 SPDX가 GPL-2.0이다 | [코드] 받아서 읽었다 |
| 이 기기의 모듈 미러 `mt8792/android_kernel_lenovo_mt8792-modules`, 갈래 `6.1.138`(제3자 미러) | 같은 두 폴더가 있다. `vdec_ipi_msg.h`, `vcodec_ipi_msg.h`, `vcp.h`, `vdec/vdec_vcp_if.c`가 OnePlus 갈래와 한 줄도 다르지 않다. 곧 IPI 규약은 안드로이드 판과 상관없이 같다 | [조사] 다섯 파일을 받아 견줬다 |
| Motorola `MotorolaMobilityLLC/kernel-kernel_device_modules-6.1` | `drivers/media/platform/mtk-vcodec`가 있다 | [조사] 폴더가 있는 것만 봤다 |
| Lenovo 공식 | 찾지 못했다([포팅 재료 조사](porting-materials.md) 3절과 같다) | 미확인 |
| VCP 펌웨어의 소스 | 공개된 것이 없다. 기기의 `vcp_a`·`vcp_b` 파티션에 서명된 이미지로만 있다 | [기록] |
| 커뮤니티 포트 `u0n0u/tb375fc-linux` | 비디오, VCP, IOMMU를 다루지 않는다([커뮤니티 포트 분석](community-port.md)에 해당 항목이 없다) | [기록] |
| 형제 칩의 커뮤니티 트리 `MT6895-Mainline/linux`, `MT6895-Mainline/vaapi-mtk-vcp` | 벤더 소스가 아니라 mainline 위에 새로 쓴 VCP, stateful 디코더, VA-API 드라이버다. 5.3절 | [조사] 받아서 읽었다 |

- 벤더 커널 코드는 GPL이라 참고해 다시 쓸 수 있다(문서 규칙: 출처와 저작권 줄을 남기고 업스트림 꼴로 다시 쓴다).
- 펌웨어는 바꿀 수 없다. LK가 서명을 검증하고(`img_auth_required = 1`, `sbc_en = 1`) [기록], 리셋은 TF-A가 푼다. 그래서 **커널 쪽이 순정 펌웨어의 IPI 규약에 맞춰야 한다.** 다른 칩의 펌웨어(ChromeOS용 MT8196 펌웨어 등)를 올리는 길은 없다.

## 4. 부트 체인

### 4.1 지금은 LK가 이미 VCP 펌웨어를 올린다

- 지금의 부팅은 시험 커널 길이다. LK는 순정 `vendor_boot`의 트리를 읽고, 커널은 안에 넣은 자기 트리를 쓴다(`kernel/tb375fc/test-kernel/README.md`).
- 순정 트리의 `vcp` 노드에는 `vcp-support = <1>`과 크기들이 있다. 그래서 **LK는 부팅마다 VCP 펌웨어를 올린다** [기록: `boot-b-7/expdb.str:53302~53369`]:
  - `me_vcp_reserved`를 0xbeb00000에 8 MiB 잡는다.
  - `vcp_b`에서 `tinysys-vcp-RV55_A`(199,168바이트)와 `tinysys-vcp-RV55_A_dram`(1,709,392바이트)을 읽고, 둘 다 인증서 사슬과 서명 검증을 지난다.
  - `[PROFILE] ::: load vcp takes 68 ms`.
- 커널 쪽 시험 트리는 그 예약을 `no-map`으로 옮겨 두었고(`mt6897-lenovo-tb375fc-test.dts:131`), 지금 태블릿의 트리에도 보인다 [실측].
- 곧 **지금 길에서는 장치 트리에 `vcp-support`를 더할 일이 없다.** 1~3단계는 부트 이미지의 `vendor_boot`·`dtbo`를 건드리지 않고 커널(`boot`)만 바꿔 시험할 수 있다.
- 모르는 것: LK가 올린 이미지를 TF-A가 어디서 찾는지, SRAM에 이미 들어 있는지. 벤더 커널은 이미지 위치를 다루지 않고 SMC만 부르므로 TF-A가 안다고 본다 [짐작]. 2단계의 첫 시험이 이것을 가린다.

### 4.2 LK가 우리 트리를 읽는 길로 옮길 때

[순정 LK가 커널 장치 트리에 요구하는 것](lk-kernel-tree.md)의 초안은 `vcp-support`를 빼서 VCP를 건너뛴다(3절, 6절). 디코더를 쓰려면 그 초안에 순정 값을 되돌려야 한다.

- 더할 것: `vcp-support = <1>`, `vcp-sram-size = <0x40000>`, `vcp-dram-size = <0x800000>`, `vcp-mem-tbl`, `memorydump`, `vcp-secure-dump*`, `vcp-sec-dump-key`.
- 잘못될 수 있는 것:
  1. `vcp-support`가 있는데 `vcp-sram-size`나 `vcp-dram-size`가 0이면 LK가 panic이다 [코드: 그 문서 3절].
  2. `me_vcp_reserved` 8 MiB가 다시 생겨 그 아래 칸들의 자리가 순정과 같아진다. 초안은 VCP를 뺀 배치로 계산했으므로(connsys의 `overall_pa`), 되돌리면 그 계산을 다시 한다 [짐작].
  3. 시험 트리의 고정 예약(`mblock-28`)과 LK가 새로 잡는 자리가 어긋나면 커널이 펌웨어 메모리를 덮는다. LK 길에서는 LK가 적는 `mblock-*`를 그대로 받으므로 이 문제는 없다.
  4. LK에는 VCP 덤프 길(`save_vcp_coredump`, `vcp_do_dramdump`)이 있다 [기록: LK 문자열]. 커널이 VCP를 켠 채 패닉하면 LK가 다음 부팅에 VCP 전원과 영역을 읽는다. 지금까지는 VCP가 꺼져 있어 타지 않은 길이다 [짐작].
- 이 일은 LK 길 자체가 기기에서 확인된 뒤의 일이고, 디코더 단계들을 막지 않는다.

## 5. 업스트림의 지금 상태

기준은 mainline master(7.4-rc3)와 linux-next 20261116이다. lore는 자동 접근을 막아(403) 시리즈의 판과 날짜는 patchwork에서 읽었다 [조사]. "직접"과 "보고"의 뜻은 6절과 같다.

### 5.1 MT8196의 VCP와 디코더

| 시리즈 | 최신 판 | 날짜 | 상태 |
|---|---|---|---|
| VCP 메일박스(`mtk-vcp-mailbox`) | v9 | 2025-10-13 | **병합, 7.0부터**(보고: 커밋 `b562abd95672`) |
| VCP remoteproc 드라이버(Xiangzhi Tang) | v4, 패치 7 | 2026-04-27 | 미병합. 바인딩 지적 둘 뒤로 새 판이 없다(보고) |
| MT8196 디코더(Yunfei Dong, v5부터 Kyrie Wu) | **v9**, 패치 14 | 2026-11-17 | 미병합. VCP v4에 기댄다(보고) |
| MT8196 인코더(Irui Wang) | v8 | 2026-09-14 | 미병합(보고) |
| MT8189 디코더·인코더 | v12 | 2026-11-17 | 미병합(보고) |
| MT8196 SMMU | RFC v1 | 2025-06-16 | 미병합(보고) |
| MT8196 SMI | v1 | 2025-03-20 | 미병합(보고) |

- **MT8196의 비디오는 하나도 들어오지 않았다.** master에 VCP remoteproc 드라이버가 없고, 디코더 표는 mt8173, mt8183, mt8192, mt8186, mt8195, mt8188뿐이며, 펌웨어 종류는 VPU와 SCP 둘이다(직접: master의 `drivers/remoteproc/`, `decoder/mtk_vcodec_dec_drv.c:540~560`, `common/mtk_vcodec_fw.c`). [포팅 재료 조사](porting-materials.md)에 적은 "v8"은 이제 v9다.
- **MT8196 디코더는 stateless다.** 앱이 헤더를 해석해 넘기는 request API이고, LAT와 core 하나의 구조다(보고: v9 패치). 업스트림에서 stateful인 것은 MT8173 하나다.
- MT8196의 VCP 드라이버는 **펌웨어를 커널이 올린다**(`mediatek/mt8196/vcp.img`, linux-firmware에 있다, 보고). 리셋은 같은 SMC(`MTK_SIP_SMC_CMD(0x52C)`)로 TF-A가 푼다. MT6897 벤더 코드의 번호와 같다 [코드: 벤더 `include/linux/soc/mediatek/mtk_sip_svc.h:155`].
- linux-firmware의 `mediatek/`에 mt68xx, mt69xx는 없다(보고). MT6897용 VCP 펌웨어는 기기 안에만 있다.

### 5.2 MT6897은 MT8196에서 얼마나 먼가

| 항목 | MT8196(업스트림 시리즈) | MT6897(순정) | 거리 |
|---|---|---|---|
| 디코더 드라이버의 꼴 | stateless | stateful(펌웨어가 해석) | **멀다.** 펌웨어가 달라 규약이 다르다. MT8196 디코더 코드는 가져다 쓸 수 없다 |
| VCP 펌웨어 | 커널이 올린다 | LK가 올린다. 서명 검증 | 다르다. 적재 부분은 못 쓴다 |
| VCP와 말하는 길 | 메일박스 다섯 + IPC 층 | 메일박스 다섯 + 벤더 IPI | 가깝다. 병합된 `mtk-vcp-mailbox`가 쓸 만한지는 레지스터를 견줘야 안다 |
| VCP 전원 | MM_PROC 도메인 하나, 클럭 없음 | MM_PROC + 클럭 셋 | 가깝다 |
| IOMMU | arm-smmu-v3(MediaTek 구현) | MediaTek m4u(`mtk_iommu`) | **다른 하드웨어다** |
| SMI | MT8196 데이터(미병합) | MT6897 데이터(없음) | 틀은 같고 데이터를 새로 넣는다 |
| 디코더 레지스터 | 공개된 노드가 없다 | 13구역(2.1절) | 견줄 자료가 없다 |

- **MT8196은 이 일의 틀이 되지 못한다.** 디코더는 꼴이 다르고 IOMMU는 하드웨어가 다르다. 쓸 수 있는 것은 VCP 메일박스와, VCP 드라이버의 뼈대 정도다.
- **IOMMU와 SMI의 틀은 이미 있다.** 7.2의 `mtk_iommu.c`는 MT6893과 MT8188·MT8195(vdo·vpp 두 개), MT8189까지 받는다(직접: v7.2 `mtk_iommu.c:1903~1921`). MT8189의 MM IOMMU 데이터가 34비트 IOVA, 35비트 PA, 뱅크 다섯, 여러 도메인으로 MT6897과 가장 가깝다(보고). `mtk-smi.c`는 MT8195의 sub-common까지 받는다(직접: v7.2 `mtk-smi.c:549~859`). MT6897은 데이터 표와 compatible을 더하는 일이다.
- 포크의 화면 경로는 SMI와 IOMMU를 전혀 설정하지 않는다([DESIGN.md](DESIGN.md) 10절의 위험 표). 디코더는 그럴 수 없다: VCP에 주는 주소가 IOVA이고(2.3절), 디코더의 작업 버퍼도 IOMMU 주소로 넘긴다(2.2절).

### 5.3 같은 계열 칩의 커뮤니티 작업 (가장 가까운 틀)

`MT6895-Mainline/linux`(GitHub, 커뮤니티, 리뷰 없음)의 갈래 `7.2-mt6895-xiaomi-xaga`(머리 `25fd36e67958`, 2026-11)에 Dimensity 8100용 VCP와 디코더가 있다 [조사, 직접: 파일을 받아 읽었다]. 포크가 PMIC와 SPMI에서 이미 견본으로 삼은 저장소다.

| 파일 | 내용 |
|---|---|
| `drivers/remoteproc/mtk_vcp.c`(995줄) | 머리말이 "bootloader-owned firmware and vendor codec mailboxes"다. 커널은 펌웨어를 올리지 않고, SMC 0x52c(리셋)와 0x52e(접근 보호)를 부른다. 우리 2.3절의 차례와 같다 |
| `drivers/media/platform/mediatek/vcodec/vcp/mtk_vcp_vdec*.c`(디코더 약 4,400줄) | 벤더 IPI 규약을 말하는 **stateful** V4L2 디코더. 규약 머리말은 "verified against xaga vendor headers"다 |
| `drivers/iommu/mtk_iommu_mt6895.c`(3,744줄) | `disp-iommu`, `mdp-iommu`의 다운스트림 포트 |
| `MT6895-Mainline/vaapi-mtk-vcp`(MIT, 약 1만 줄, 2026-10-11) | 그 stateful 디코더를 **VA-API**로 내는 libva 드라이버. 디코드는 H.264, HEVC Main·Main 10, VP9 Profile 0, MPEG-2다 |

- 뜻하는 바:
  - **mainline 커널에서 이 SMC들로 VCP를 켜는 길이 형제 칩에서 이미 걸어졌다.** 8절 위험 1이 작아진다. 다만 README는 xaga에서 개발하고 시험했다고만 적고, 우리가 돌려 본 것이 아니다.
  - **MT6895의 펌웨어는 블록 형식(MM21, MT2T)만 낸다.** 드라이버 주석이 "there is no linear form"이라 적고, 커널 안에서 CPU로 풀어 NV12·P010으로 낸다(`mtk_vcp_vdec_drv.c:805~937`). MT6897도 같을 가능성이 높다 [짐작].
  - stateful 디코더를 VA-API로 잇는 드라이버가 있다. 6절의 길이 하나 늘어난다.
- 규약은 형제이지 같지 않다. MT6895의 명령 표에는 `END`가 있고 MT6897에는 없으며, MT6897에는 `RESUME`과 `SLICE_DONE_ISR`이 더 있다(MT6895 `mtk_vcp_vdec_abi.h`, MT6897 `vdec_ipi_msg.h:52~92`). IOMMU 포트도 다르다(MT6895는 larb33 포트 2 도메인 7, MT6897은 larb43 포트 0 도메인 5). **그대로 옮길 수는 없고, 구조를 본으로 삼아 MT6897 벤더 헤더에 맞춰 다시 쓴다.**
- 그 밖의 MT68xx·MT69xx 칩의 비디오 작업은 메일링 리스트에 없다(보고). `MT6878-mainline/linux`에는 VCP도 디코더도 없다(보고).

## 6. 사용자 공간: Firefox ESR가 디코더를 쓰는 길

Firefox ESR 153의 소스(`esr153` 갈래, `config/milestone.txt`는 153.8.0)와 FFmpeg, Debian을 봤다 [조사]. "직접"은 이 조사에서 그 파일을 다시 열어 확인한 것이고, "보고"는 하위 조사가 열어 본 것을 다시 열지 않은 것이다.

### 6.1 길 넷

| 길 | 코덱 | 설치할 것 | 우리 드라이버(stateful)와 맞나 | ESR 153 상태 |
|---|---|---|---|---|
| **V4L2-M2M**: 시스템 libavcodec의 `*_v4l2m2m` 디코더 | Firefox가 이름으로 찾는 것은 `h264_`, `hevc_`, `vp8_`, `vp9_`, `av1_v4l2m2m`(직접: `FFmpegVideoDecoder.cpp`의 `FindVideoHardwareAVCodec`) | `AV_PIX_FMT_DRM_PRIME`으로 내는 libavcodec. FFmpeg 원본과 Debian의 것은 그렇게 내지 않는다(보고: `libavcodec/v4l2_m2m_dec.c`). Raspberry Pi의 FFmpeg 포크가 내고, 원본에는 PR 23471이 열려 있다 | **맞는다** | H.264는 116부터 들어 있다. VP9·AV1은 아래 걸림돌 셋이 있다 |
| **VA-API, stateful용 드라이버**: libva와 `vaapi-mtk-vcp`(5.3절, 커뮤니티) | 드라이버가 내는 것은 H.264, HEVC Main·Main 10, VP9 Profile 0, MPEG-2다. Firefox에서 VP9는 안의 ffvpx가, H.264·HEVC는 시스템 libavcodec이 VA-API로 푼다(보고). **AV1은 없다** | 그 드라이버를 MT6897에 맞춰 옮긴 것(Debian에 없다), H.264에는 `libavcodec63`(Debian 것 그대로) | **맞는다.** stateful 디코더를 VA-API로 내는 드라이버다. H.264·HEVC는 파라미터 세트를 다시 만들어 넣는다(직접: `va_mtkvcp_ps.c`) | Firefox의 기본 하드웨어 길이다. 이 드라이버와 Firefox를 함께 돌린 기록은 찾지 못했다. RDD 샌드박스가 막는지도 모른다 |
| **VA-API, stateless용 드라이버**: `libva-v4l2-request`와 그 포크, `asahi-va-driver` | H.264, HEVC, VP8, VP9, AV1 등(보고) | Debian에 없다(보고) | 안 맞는다 | RDD 샌드박스를 꺼야 돈다(버그 2080696, 열려 있음) |
| **FFmpeg `v4l2request` hwaccel** | MPEG-2, H.264, HEVC, VP8, VP9, AV1 | FFmpeg 원본에 없다. PR 20847이 열려 있다(보고) | 안 맞는다(stateless용) | Firefox 지원이 없다(버그 1969297) |

- **쓸 수 있는 길은 둘이다.** 2절대로 펌웨어가 비트스트림을 통째로 받으므로 커널 드라이버는 stateful이 되고, stateful을 받는 길은 V4L2-M2M과 stateful용 VA-API 드라이버다.
- **YouTube에는 VA-API 길이 맞다.** VP9가 Firefox의 기본 하드웨어 길(ffvpx의 VA-API)을 타고, FFmpeg를 고칠 일이 없다. V4L2-M2M 길은 H.264만 확실하고 고친 libavcodec이 든다.
- VA-API 길에서 Firefox가 드라이버에 바라는 것: `vaExportSurfaceHandle`의 `DRM_PRIME_2`와 NV12·YV12·P010·P016 가운데 하나(보고: `FFmpegVideoFramePool.cpp`). 커뮤니티 드라이버에 그 내보내기가 있다(직접: `va_mtkvcp_img.c:907`). libva가 이 드라이버를 스스로 고르지 못하면 `LIBVA_DRIVER_NAME`을 준다(README).
- V4L2-M2M 길에서 Firefox의 조건(직접: `widget/gtk/GfxInfo.cpp:965~988`, `widget/gtk/v4l2test/v4l2test.cpp`):
  - `/dev/video*`가 `V4L2_CAP_STREAMING`과 M2M(단일 평면이나 다중 평면)을 알린다.
  - **CAPTURE 형식에 `NV12`나 `YV12`가 있어야 한다.** 없으면 V4L2 전체를 끈다.
  - OUTPUT 형식의 fourcc `H264`, `VP80`, `VP90`, `HEVC`, `AV01`이 코덱마다의 하드웨어 지원이 된다.
  - 디코더가 `AV_PIX_FMT_DRM_PRIME`을 내야 한다(직접: `FFmpegVideoDecoder.cpp`의 `ChooseV4L2PixelFormat`).

V4L2-M2M 길의 걸림돌 셋(VA-API 길에는 없다):

- **걸림돌 1: 디코더 모듈의 차례.** RDD 프로세스는 ffvpx를 시스템 FFmpeg보다 먼저 올린다(직접: `dom/media/platforms/PDMFactory.cpp`의 `CreateRddPDMs`). ffvpx는 V4L2 없이 빌드돼 있어(보고: `CONFIG_V4L2_M2M 0`), VP8·VP9·AV1이 ffvpx의 소프트웨어 디코더에 먼저 걸릴 수 있다. 코드에 `vp9_v4l2m2m`, `av1_v4l2m2m`을 찾는 길이 있고(버그 1965646은 141, 2008353은 151에 들어갔다) 다른 기기에서 됐다는 보고가 버그에 있으나, ESR 153에서 실제로 그 길을 타는지는 **확인하지 못했다.** H.264는 ffvpx에 디코더가 없어 이 문제가 없다.
- **걸림돌 2: 한 세션에 코덱 하나.** V4L2가 처음 성공한 코덱만 기억하고 나머지를 거절하는 결함(버그 2071471)은 159에서 고쳐졌고 ESR 153에는 고침이 없다(보고).
- **걸림돌 3: FFmpeg 원본에 `av1_v4l2m2m`이 없다**(보고: `libavcodec/allcodecs.c`). AV1을 이 길로 풀려면 libavcodec 패치가 더 든다.

### 6.2 YouTube에서 얻는 것

- **목표는 VP9다.** 잰 영상이 3840x2160@60 VP9였고, YouTube의 1440p 이상은 VP9나 AV1로만 온다. Google의 공식 도움말은 4K를 보려면 VP9를 지원해야 한다고 적는다 [조사].
- AV1은 이 길에 없으니 Firefox에서 끈다(`media.av1.enabled`, 기본값은 켜짐, 보고). 켜 둔 채 YouTube가 AV1을 주면 지금처럼 CPU로 푼다.
- VP9 Profile 0은 8비트다. HDR 영상(Profile 2, 10비트)은 커뮤니티 드라이버의 목록에 없다.
- 커뮤니티 드라이버는 **한 번에 하드웨어 세션 하나**만 받는다(README). 탭 둘에서 동시에 재생하면 둘째는 소프트웨어로 간다고 본다 [짐작].
- H.264만 되는 경우(V4L2-M2M 길)에는 `media.mediasource.vp9.enabled`도 꺼야 하고, YouTube의 H.264는 1080p까지라는 것이 널리 알려져 있다(공식 문서에서 확인하지 못했다). 화면이 2944x1840이라 1080p는 원본보다 낮다.
- Firefox는 `MediaCapabilities`에서 하드웨어 지원이 있거나 해상도가 낮으면 `powerEfficient`를 참으로 답한다(보고). YouTube가 이 값을 보고 코덱을 고르는지는 확인하지 못했다.

### 6.3 프레임이 화면에 닿는 길

- Firefox는 디코더가 준 dmabuf의 평면마다 EGLImage를 만든다(Y는 `R8`, UV는 `GR88`, 10비트는 `R16`·`GR1616`). WebRender가 그것을 자기 버퍼에 합성하고, 그 버퍼가 컴포지터로 간다(보고: `widget/gtk/DMABufSurface.cpp`). 컴포지터가 받는 것은 지금과 같은 RGB 버퍼다.
- 그래서 **`comp/`는 고칠 것이 없다.** 컴포지터가 앱의 dmabuf를 텍스처로만 가져오는 것(`comp/src/dmabuf.rs`의 머리말)은 이 길을 막지 않는다. 필요한 것은 GPU 쪽이다: Mesa(panfrost)가 디코더가 만든 dmabuf를 `R8`·`GR88`로 가져올 수 있어야 한다. 디코더 버퍼가 선형 NV12면 될 것으로 보지만 재지 않았다 [짐작].
- 비디오 dmabuf를 서브서피스로 컴포지터에 바로 넘기는 길도 Firefox에 있으나(`gfx/layers/NativeLayerWayland.cpp`), 리눅스에서는 기본으로 꺼져 있고 차단 목록에 있다(보고). 이 길을 켜면 컴포지터가 NV12 dmabuf를 받아야 한다. `comp/`의 시험에 NV12를 외부 텍스처로 가져오는 경우가 있다(`comp/src/blur.rs:1642`). 평면에 바로 올리는 직접 스캔아웃은 아직 없다. 뒤의 최적화이고 첫 목표에는 필요 없다.
- **MM21은 Firefox에 닿지 못한다.** Firefox는 어느 길에서나 YUV420·NV12·P010 계열만 받고(보고), FFmpeg의 V4L2 형식 표에도 MM21이 없다(보고: `libavcodec/v4l2_fmt.c`). 펌웨어가 블록 형식만 내면(5.3절) 누군가 풀어야 한다.
  - MT6895 커뮤니티 드라이버는 커널 안에서 CPU로 푼다. 4K NV12 한 장이 약 12 MiB라 60 Hz면 초당 약 750 MiB를 옮긴다(계산). 1080p는 그 4분의 1이다. 4K60에서 이 비용이 얼마인지는 재야 안다.
  - Mesa 25.0의 panfrost에는 MediaTek 16L32S 풀기가 들어 있다(보고: 릴리스 노트). Firefox의 평면별 가져오기로 그 길을 탈 수 있는지는 모른다 [짐작].

### 6.4 다른 기기의 사정 [조사, 보고]

| 기기 | 디코더 | Firefox |
|---|---|---|
| Raspberry Pi 4 | stateful | Raspberry Pi의 FFmpeg 포크로 된다(버그 1833354, 1852765) |
| Qualcomm iris·venus | stateful | 같은 포크와 고치지 않은 Firefox로 된다(버그 1852765, 2071471) |
| Raspberry Pi 5 | stateless HEVC | 안 된다(버그 1969297) |
| Apple(Asahi) | stateless | `asahi-va-driver`로 VA-API, 샌드박스를 꺼야 한다(버그 2080696) |

우리 드라이버는 위 둘째 줄까지의 모양(stateful)이라, 이미 길이 난 쪽에 든다.

## 7. 단계 계획

단계마다 끝에 시험이 있고, 앞 단계가 지나야 다음이 뜻이 있다. 크기는 [포팅 재료 조사](porting-materials.md)와 같은 눈금이다(작음, 중간, 큼).

| 단계 | 만드는 것 | 끝의 시험 | 사용자에게 필요한 것 | 크기 | 판정 |
|---|---|---|---|---|---|
| 0 | 이 조사 | 이 문서 | 없음 | — | 끝 |
| 1 | IOMMU와 SMI의 MT6897 데이터, MDP 쪽의 `mdp_iommu`·`smi_mdp_common`·larb4와 VDE0·VDE1 도메인 노드, 켜 달라고 할 때만 도는 시험 모듈 | 부팅, 화면과 잠자기가 그대로다. 시험 모듈을 올리면 IOMMU가 초기화되고 VDE0·VDE1이 켜지며, 매핑과 TLB 무효화에 하드웨어가 답한다. 내리면 전원이 꺼진다 | 시험 커널 굽기와 재부팅 | 중간 | **지났다(2026-11-17, 7.1절)** |
| 2 | VCP 드라이버(전원, SMC, 메모리, 메일박스, IPI) | 리셋을 푼 뒤 0.8초 안에 준비 IPI가 온다. 펌웨어 기록이 읽힌다. 켜고 끄기를 되풀이해도 같다 | 시험 커널 굽기와 재부팅 여러 번 | 큼 | **간다. 다만 TF-A가 받아 주는지는 해 봐야 안다** |
| 3 | stateful V4L2 디코더(순정 펌웨어의 IPI 규약) | 펌웨어가 답한 코덱·크기·출력 형식 목록. H.264 1080p 파일을 풀어 소프트웨어 디코더의 결과와 프레임마다 견준다. 이어서 HEVC, VP9, AV1, 4K60 | 재부팅, 시험 도구 설치(`v4l-utils`, GStreamer의 V4L2 디코더) | 큼 | **2단계가 지나면 간다** |
| 4 | Firefox로 잇기: stateful용 VA-API 드라이버를 MT6897에 맞춰 옮기고 BSP 패키지로 넣는다. AV1을 끄는 설정 | `vainfo`의 목록. 그다음 YouTube 1440p·4K VP9에서 Stats for nerds의 버린 프레임과 전력을 지금과 견준다 | 패키지 설치, 사용자의 눈 확인 | 중간~큼 | **간다. 다만 이 드라이버와 Firefox의 조합은 기록이 없어 해 봐야 안다** |
| 5 | AV1 | YouTube AV1에서 버린 프레임 | Firefox 판 결정 | 모름 | **지금은 길이 없다.** VA-API 드라이버에 AV1이 없고, V4L2-M2M 길은 6.1절 걸림돌 셋에 걸린다 |
| 6 | 다듬기: 클럭과 전압 정책, 잠자기, 발열, LK 길의 트리, 이미지에 넣기 | 재생 한 시간의 전력과 온도, 잠자기 뒤 재생 | 긴 시험 | 중간 | 뒤에 정한다 |

### 7.1 1단계: IOMMU와 SMI

- 업스트림 `mtk_iommu.c`와 `mtk-smi.c`에 MT6897 데이터를 더한다(패치 0210~0212). 벤더와 다른 것은 TLB 무효화의 동기 비트 하나다(`mtk_iommu.c:1051`, `:2089`). SMI의 대역폭 표와 잠자기 제어는 옮기지 않았다.
- **화면은 이미 두 IOMMU를 지나고 있다.** 화면의 larb 여섯 가운데 larb0·21·33은 `disp_iommu`에, larb1·20·32는 `mdp_iommu`에 달려 있다 [기록: 순정 트리의 `mediatek,larbs`]. 지금은 larb 포트의 MMU 비트가 꺼져 있어 번역 없이 지난다. 그래서 "화면과 상관없는 IOMMU"는 없다. 앞서 이 문서에 "`mdp_iommu`만 켜면 화면과 떨어진다"고 적은 것은 틀렸다.
- 그래서 **부팅 때는 아무 레지스터도 쓰지 않게** 했다. 업스트림 드라이버는 마스터가 처음 붙을 때 IOMMU를 초기화한다. 마스터 노드를 꺼 두고, 시험 모듈을 손으로 올릴 때만 그 노드로 장치를 만든다. 문제가 나도 재부팅하면 처음 상태다.
- 1단계는 MDP 쪽만 한다: `mdp_iommu`, `smi_mdp_common`, larb4, VDE0·VDE1. 2단계의 VCP도 `mdp_iommu` 뒤다. `disp_iommu`와 larb5는 디코더 앞단을 쓰는 3단계에 더한다.
- 태블릿에서 읽은 지금 값 [실측, 2026-11-17, 블록의 전원과 클럭이 켜져 있는 것을 먼저 확인했다]:

| 곳 | 값 | 뜻 |
|---|---|---|
| topckgen `CLK_CFG_13`(0x100000e0) | 0x01800004 | `vdec` 먹스는 `mainpll_d5_d2`(218.4 MHz)이고 꺼짐 비트(bit 7)가 0이다. LK가 켜 둔 채다 |
| 하드웨어 보터 EN(0x10321410) | 0x80000079 | MM_INFRA와 화면 넷이 켜져 있고 VDE0(bit 1)·VDE1(bit 2)은 꺼져 있다 |
| 두 IOMMU의 `MISC_CTRL`(+0x48) | 0x000A000A | 표준 AXI와 순서 쓰기가 켜져 있다. 초기화가 둘 다 끈다(벤더도 같다) |
| 두 IOMMU의 `DCM_DIS`(+0x50) | 0x007FFFFF | 클럭 게이팅이 꺼져 있다. 초기화가 0으로 쓴다(벤더도 같다) |
| 두 IOMMU의 인터럽트 제어(+0x120, +0x124) | 0 | 아무도 초기화하지 않았다 |
| 두 IOMMU의 표 주소(+0x0)와 보호 주소(+0x114) | 0xC0000007 | 램 밖을 가리키는 값이다. 커널이 붙은 적이 없다 |
| `smi_mdp_common`의 버스 선택(+0x220) | 0 | 그대로 둔다(패치 0212) |

- **남는 위험:** 시험 모듈을 올리는 순간 `mdp_iommu`의 `MISC_CTRL`과 `DCM_DIS`가 바뀌고, 그 IOMMU를 화면 트래픽이 지나고 있다. 순정 커널도 LK가 화면을 켜 둔 채 같은 값을 쓰므로 괜찮을 것으로 보지만 [짐작], 화면을 보면서 한다.
- 이 단계에는 버스에 실제로 접근하는 마스터가 없다. **번역이 맞는지는 2단계에서 처음 드러난다.** 여기서 보는 것은 붙는 것, 전원이 오르내리는 것, TLB 무효화에 하드웨어가 답하는 것, 화면과 잠자기에 영향이 없는 것이다.
- **기기에 올리기 전 확인한 것(2026-11-17):**
  - 시험 커널 #328을 빌드했다. 장치 트리의 차이는 꺼 둔 노드 넷(`smi@1e80f000`, `larb@1602e000`, `iommu@1e810000`, `iommu-selftest`), 켠 `vdecsys`, VDE0·VDE1 도메인 노드뿐이다(#326의 트리와 견줬다).
  - 부팅 때 달라지는 것은 `vdecsys`의 게이트 여섯이 등록되는 것과 VDE0·VDE1이 꺼진 도메인으로 등록되는 것이다. 둘 다 블록의 레지스터를 쓰지 않는다 [코드: 패치 0104의 `no_status` 게이트, `mtk-pm-domains.c`의 `KEEP_DEFAULT_OFF`].
  - QEMU에서 가짜 레지스터(RAM) 위로 흐름을 돌렸다: 시험 모듈이 노드 셋을 켜고, SMI common·larb·IOMMU가 붙고, 장치가 IOMMU 그룹에 들어가고, 초기화가 `CTRL`에 동기 비트(0x8)를, larb4의 포트 0·2에 MMU 비트를 쓰고, 매핑 네 크기가 통과하고, 전원을 세 번 껐다 켰다. 하드웨어의 답(TLB 무효화 완료, 전원 도메인)은 QEMU로 볼 수 없다.
  - **찾은 결함:** 업스트림 `mtk_iommu`는 장치가 사라져도 그 IOMMU 그룹의 포인터를 쥐고 있다가 다음 장치에 다시 쓴다(`mtk_iommu_device_group`). 시험 모듈을 내렸다 올리자 QEMU에서 드러났다. 장치가 사라질 일이 없는 보통 쓰임에서는 타지 않는다. 시험 모듈은 내릴 수 없게 만들고 sysfs로 전원을 켜고 끈다. 고침은 따로 다룬다.
- **기기 결과(2026-11-17, 시험 커널 #328, 사용자 승인 뒤 `boot_b`에 굽고 재부팅):**

| 본 것 | 결과 |
|---|---|
| 부팅 | 30초 만에 SSH가 돌아왔다. 커널 경고의 종류가 앞 부팅(#326)과 같고 새것이 없다. 실패한 서비스 없음. 하드웨어 보터 EN은 0x80000079 그대로이고 VDE0·VDE1은 꺼진 도메인으로 보인다 |
| 시험 모듈 올리기(전원은 쥐지 않음) | 노드 셋이 켜지고 `mtk-smi-common`, `mtk-smi-larb`, `mtk-iommu`가 붙었다. 시험 장치가 IOMMU 그룹 0에 들어갔다. IOMMU 레지스터: 표 주소 0x7BF00000, `MISC_CTRL` 0, `CTRL` 0x1008(동기 비트), 인터럽트 제어 0x6F·0x3FFF, 인터럽트(SPI 447) 등록 |
| 화면 | 초기화 뒤에도 정상이다(사용자가 눈으로 확인) |
| VDE 전원 켜기 | 보터 EN이 0x8000007F가 되고 vde0·vde1이 on이다. larb4의 레지스터가 읽힌다 |
| 매핑 | 4 KiB, 64 KiB, 1 MiB, 8 MiB를 8번씩: 통과. **TLB 무효화 시간 초과 0건**(하드웨어가 답한다). 번역 오류 인터럽트 0건 |
| 전원 켜고 끄기 10번 | 매번 EN이 0x79로 갔다가 0x7F로 돌아왔고 매핑이 통과했다. **화면 도메인과 달리 VDE 도메인은 보터로 끈 뒤 다시 켜진다** |
| 잠자기(전원을 놓은 채) | 충전기가 꽂혀 있어 감시 잠자기로 들어가 s2idle을 24번 드나들었고, 전원 키로 깨웠다. 깬 뒤 IOMMU 레지스터가 그대로이고, 전원 켜기와 매핑이 다시 통과했다. 이 동안 IOMMU·SMI 쪽 커널 오류 없음 |

- 재면서 알게 된 것 [실측]:
  - larb4의 포트 0~3은 MMU 비트가 처음부터 1이다. 우리가 붙인 것은 포트 0과 2뿐인데 1과 3도 1로 읽혔다. 벤더가 포트 설정을 건너뛰는 것(`SKIP_CFG_PORT`)과 맞는다. 곧 **디코더의 larb는 전원이 켜지면 번역이 기본이다.**
  - `DCM_DIS`에 0을 쓰면 0x000431D8로 읽힌다. 쓸 수 없는 비트가 있다.
  - 두 IOMMU의 처음 값은 위 표와 같고, 초기화 뒤에도 화면이 정상이므로 `MISC_CTRL`을 0으로 바꾸는 것은 우회 중인 화면 트래픽에 영향이 없다(이 한 번의 관찰에서).
- 아직 보지 않은 것: 디코더 전원을 쥔 채로 드는 잠자기, `disp_iommu`와 larb5, 실제 마스터가 번역을 거쳐 메모리에 닿는 것(2단계).
- 잠자기 시험에서 놓친 것: RTC 알람 45초로 깨우려 했으나, 충전기가 꽂혀 있으면 충전 감시가 알람을 10초 간격으로 바꿔 쓰고 전원 키로만 끝난다. 사용자가 전원 키로 깨웠다.
- 시험 모듈과 QEMU 틀은 저장소 밖이다(`.handoff/vdec-stage1/`의 `selftest/`, `qemu/`).
- VDE0·VDE1은 하드웨어 보터로 켜고 끈다. 화면 도메인은 보터로 끈 뒤 다시 켜지지 않아 늘 켜 두게 했다(패치 0188). 디코더 도메인도 같은지 이 단계에서 켜고 끄기를 되풀이해 본다.

### 7.2 2단계: VCP

- 벤더 `vcp_helper.c`에서 켜는 차례(2.3절의 다섯)와 메일박스·IPI만 옮겨 작은 드라이버로 다시 쓴다. 덤프, 로거의 대부분, 디버그 sysfs, 복구 작업은 뺀다.
- 본은 MT6895 커뮤니티의 `drivers/remoteproc/mtk_vcp.c`(995줄, GPL-2.0-only)다. 펌웨어를 올리지 않는 꼴이 우리와 같다. 레지스터 오프셋, IOMMU 포트와 IOVA 범위, IPI 표는 MT6897 벤더 값으로 바꾼다. 메일박스는 병합된 `mtk-vcp-mailbox`(7.0부터)를 쓸 수 있는지 먼저 본다.
- 첫 시험은 MMDVFS 초기화 IPI를 보내지 않고 한다. 펌웨어가 우리 화면 클럭과 전압을 건드리지 않게 하려는 것이다(8절 위험 2).
- 실패해도 태블릿이 망가지지 않게: VCP는 켜 달라고 할 때만 켜고(모듈, 자동 적재 안 함), 준비 IPI가 안 오면 리셋을 걸고 전원을 내린다.

### 7.3 3단계: 디코더

- 벤더의 `vdec_vcp_if.c`(2,082줄), `vdec_ipi_msg.h`, VSI 구조체를 규약의 근거로 삼아, V4L2 stateful 디코더 규격(커널 문서 `dev-decoder.rst`)에 맞는 드라이버를 쓴다. 구조의 본은 MT6895 커뮤니티의 `mtk_vcp_vdec*.c`(약 4,400줄)다. 벤더의 전용 컨트롤 51개와 DVFS·QoS·SLC·보안 재생은 옮기지 않는다.
- 출력이 블록 형식뿐이면 MT6895 드라이버처럼 커널에서 풀어 NV12로 낸다(8절 위험 6).
- **첫 결과물은 프레임이 아니라 목록이다.** `QUERY_CAP`의 답(코덱, 크기 범위, 출력 형식)이 2.1절의 "재지 않았다"를 채우고 4·5단계의 모양을 정한다.
- 시험 도구는 Firefox가 아니다. `v4l2-ctl`과 GStreamer의 stateful V4L2 디코더로 파일을 풀어 견준다. 태블릿에 `gstreamer1.0-plugins-good`이 이미 있다 [실측].
- 클럭은 가장 낮은 OPP(218 MHz, 575 mV)에 고정해 시작한다. 전압 레일이 무엇인지와 올리는 길은 6단계다.

### 7.4 4단계: Firefox

- `vaapi-mtk-vcp`(MIT)를 본으로 삼아 MT6897용 VA-API 드라이버를 BSP 패키지로 만든다. 장치 이름, 형식, 우리 디코더의 동작에 맞춰 옮긴다. 인코더와 VideoProc은 뺀다.
- 차례: `vainfo`로 프로파일 목록, 작은 VA 시험 프로그램으로 VP9 파일 풀기, 그다음 Firefox다. Firefox에서 막히면 RDD 샌드박스(`MOZ_DISABLE_RDD_SANDBOX=1`)를 시험으로만 꺼서 원인을 가린다.
- Firefox 설정: `media.av1.enabled`를 끈다. H.264를 하드웨어로 풀려면 `libavcodec63`을 깐다(Debian 것 그대로, 사용자가 정한 "ffmpeg는 디코더 뒤"가 여기다). VP9만이면 libavcodec 없이 된다.
- 예비 길은 V4L2-M2M이다. `DRM_PRIME`을 내는 libavcodec(원본 FFmpeg의 PR 23471이나 Raspberry Pi 포크의 패치)을 따로 만들어야 하고 H.264만 확실하다.

### 7.5 5단계(AV1)의 길

1. 그대로 둔다. YouTube는 AV1을 끄면 VP9를 준다. 이것으로 처음의 문제는 풀린다.
2. VA-API 드라이버에 AV1을 더한다. VA-API의 AV1은 타일 데이터만 넘기므로 프레임 헤더를 다시 만들어야 한다. H.264·HEVC의 파라미터 세트보다 훨씬 어렵다고 본다 [짐작].
3. V4L2-M2M 길로 AV1을 푼다. libavcodec에 `av1_v4l2m2m`을 더하고, ESR 153의 걸림돌 1·2가 풀려야 한다. 버그 2071471의 고침은 159에 있다.

## 8. 위험

| # | 위험 | 드러나는 곳 | 줄이는 길 |
|---|---|---|---|
| 1 | **TF-A가 리셋 풀기 SMC를 받아 주지 않는다.** 형제 칩 MT6895에서는 mainline 커널이 같은 번호의 SMC로 VCP를 켠다(5.3절). MT6897에서 해 본 기록은 없고, LK가 올린 이미지를 TF-A가 어떻게 찾는지도 코드로 보지 못했다 | 2단계 첫 시험 | 준비 IPI가 없으면 SMC 반환값과 VCP 레지스터를 읽어 가린다. 여기서 막히면 디코더 전체가 막힌다 |
| 2 | **펌웨어의 MMDVFS가 우리 커널과 다툰다.** 펌웨어는 `disp`·`mdp`·`smi_common` 먹스와 `vcore`·`vmm` 전압도 다루게 돼 있다. 우리 커널은 화면 먹스를 스스로 쥔다 | 2~3단계 | 초기화 IPI를 보내지 않고 시작한다. MT6895 커뮤니티 드라이버에는 MMDVFS를 다루는 코드가 없다(직접: `mtk_vcp.c`, `mtk_vcp_vdec*.c`). 그쪽은 그 없이 도는 것으로 보이나, MT6897에서 디코더가 그 없이 도는지와 `vdec` 먹스를 커널이 직접 열어도 되는지는 해 봐야 안다 |
| 3 | **IOMMU를 초기화하며 화면이 깨진다.** 화면 트래픽이 두 IOMMU를 다 지난다(7.1절). 초기화는 AXI 방식과 클럭 게이팅 설정을 바꾼다 | 1단계에서 시험 모듈을 올릴 때 | 부팅 때는 쓰지 않고 손으로 올릴 때만 초기화한다. 화면 larb의 MMU 비트는 건드리지 않는다. 깨지면 재부팅한다 |
| 4 | **하이퍼바이저.** 벤더 IOMMU 드라이버는 보호 영역을 매핑할 때 GenieZone을 부른다(`HYP_PMM_*`, `mtk_iommu.c:698`). 우리가 쓰는 VCP 도메인 5와 디코더 포트는 보통 영역이라 부르지 않을 것으로 읽었으나 [코드], 하이퍼바이저가 IOMMU 표를 따로 검사하는 기기라면 막힌다 | 2단계 | 번역 오류 인터럽트를 본다 |
| 5 | **펌웨어가 디코더 IPI 말고 다른 것도 기다린다.** 로거, 속성 메모리, MMQOS, 기능 등록 | 2~3단계 | 벤더 코드의 초기화 차례를 그대로 따른 뒤 하나씩 뺀다 |
| 6 | **출력이 블록 형식(MM21)뿐이다.** MT6895가 그렇다. Firefox는 선형 NV12 계열만 받는다 | 3단계의 목록 | 커널에서 CPU로 풀어 낸다(MT6895 드라이버의 방식). 4K60에서는 초당 약 750 MiB를 옮기는 비용이 든다. GPU에서 푸는 길은 그 뒤에 본다 |
| 7 | **잠자기.** 벤더는 잠들기 전에 리셋을 걸고 깨면 다시 푼다. 이 기기의 s2idle은 여러 패치로 겨우 맞춰 둔 상태다 | 2단계, 6단계 | 재생 중이 아닐 때는 VCP를 꺼 둔다 |
| 8 | **Firefox가 VA-API 드라이버를 받지 않는다.** RDD 샌드박스가 `/dev/video*`의 ioctl을 막거나, 드라이버가 Firefox가 쓰는 호출을 다 갖추지 못했을 수 있다. 이 조합의 기록이 없다 | 4단계 | 샌드박스를 꺼서 가린 뒤, 드라이버를 고치거나 V4L2-M2M 길로 간다 |
| 11 | **커뮤니티 코드에 기댄다.** MT6895의 커널 드라이버와 VA-API 드라이버는 리뷰를 받지 않았고 한 저장소뿐이다. 제대로 도는지 우리가 본 적이 없다 | 2~4단계 | 본으로만 쓰고, 규약은 MT6897 벤더 헤더(OEM 공식)에 맞춘다 |
| 9 | **유지 비용.** 디코더와 VCP 드라이버는 업스트림과 규약이 달라(5절) 대기열에 오래 남는다. 커널을 올릴 때마다 따라 옮긴다 | 계속 | 드라이버를 작게 쓴다 |
| 10 | **LK의 VCP 덤프 길.** VCP를 켠 채 커널이 죽으면 다음 부팅에 LK가 처음 타는 길을 탄다(4.2절) | 2단계 뒤의 패닉 | `expdb`와 `kedump-clear` 기록을 본다 |

## 9. 다음에 할 일과 물을 것

- **1단계가 첫 단계이고, 재부팅이 필요하다.** 그래서 이 조사는 코드를 쓰지 않고 여기서 멈춘다.
- 사용자가 정할 것:
  1. 1단계를 시작할지. 시험 커널을 `boot_b`에 쓰고 재부팅한다. 화면이 안 나오면 USB를 꽂아 fastboot로 `_a`에 돌아가야 한다(지금은 Wi-Fi SSH만 쓴다).
  2. 1단계 앞의 레지스터 읽기(7.1절)를 해도 되는지.
  3. 4단계의 목표를 "YouTube VP9(4K까지), AV1은 끔"으로 두어도 되는지.

## 10. 출처

- 벤더 소스(OEM 공식): https://github.com/OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6897 갈래 `oneplus/mt6897_b_16.0.0_oneplus_pad`, 커밋 `76792af`. 읽은 곳은 `kernel/kernel_device_modules-6.1/` 아래 `drivers/media/platform/mtk-vcodec/`, `drivers/misc/mediatek/vcp/`, `drivers/iommu/mtk_iommu.c`, `drivers/memory/mtk-smi.c`, `drivers/soc/mediatek/mtk-mmdvfs-v3.c`, `include/dt-bindings/memory/mt6897-larb-port.h`, `include/linux/soc/mediatek/mtk_sip_svc.h`
- 이 기기의 모듈 미러(제3자): https://github.com/mt8792/android_kernel_lenovo_mt8792-modules/tree/6.1.138
- MediaTek 공식 사양: https://www.mediatek.com/products/smartphones/mediatek-dimensity-8300
- 기기 기록(git 밖): `.handoff/tb375fc/ref/cn-zui16-merged.dts`(순정 트리), `.handoff/tb375fc-device-readings/boot-b-7/expdb.str`(LK 기록), `.handoff/tb375fc-device-readings/root-091/iomem.txt`(순정 커널), `.handoff/tb375fc-dev-lk/sections/stock-lk.bin`의 문자열
- 업스트림 소스(직접): https://github.com/torvalds/linux 의 `v7.2`와 master(7.4-rc3)에서 `drivers/iommu/mtk_iommu.c`, `drivers/memory/mtk-smi.c`, `drivers/media/platform/mediatek/vcodec/`, `drivers/remoteproc/`
- 업스트림 시리즈(보고, patchwork):
  - VCP remoteproc v4: https://patchwork.kernel.org/project/linux-mediatek/cover/20260427111446.22955-1-xiangzhi.tang@mediatek.com/
  - VCP 메일박스 병합: https://git.kernel.org/torvalds/c/b562abd956726f57bb78813b4c77db51d28933a9
  - MT8196 디코더 v9: https://patchwork.kernel.org/project/linux-mediatek/cover/20261117032950.1425556-1-kyrie.wu@mediatek.com/
  - MT8196 인코더 v8: https://patchwork.kernel.org/project/linux-mediatek/cover/20260914073423.16131-1-irui.wang@mediatek.com/
  - MT8189 디코더·인코더 v12: https://patchwork.kernel.org/project/linux-mediatek/cover/20261117085834.1464723-1-kyrie.wu@mediatek.com/
  - MT8196 SMMU RFC: https://patchwork.kernel.org/project/linux-mediatek/cover/20250616025628.25454-1-xueqi.zhang@mediatek.com/
  - MT8196 SMI v1: https://patchwork.kernel.org/project/linux-mediatek/cover/20250320073625.25225-1-xueqi.zhang@mediatek.com/
  - linux-firmware의 MT8196 VCP: https://git.kernel.org/pub/scm/linux/kernel/git/firmware/linux-firmware.git/commit/?id=3d6067ec19759fe0fa6286605d646d465ef0bc4d
- 형제 칩의 커뮤니티 트리(GitHub, 리뷰 없음): https://github.com/MT6895-Mainline/linux 갈래 `7.2-mt6895-xiaomi-xaga`, https://github.com/MT6895-Mainline/vaapi-mtk-vcp
- Firefox ESR 153 소스: https://github.com/mozilla-firefox/firefox/tree/esr153 의 `dom/media/platforms/PDMFactory.cpp`, `dom/media/platforms/ffmpeg/FFmpegVideoDecoder.cpp`, `widget/gtk/GfxInfo.cpp`, `widget/gtk/v4l2test/v4l2test.cpp`(직접), `widget/gtk/DMABufSurface.cpp`, `gfx/layers/NativeLayerWayland.cpp`, `security/sandbox/linux/broker/SandboxBrokerPolicyFactory.cpp`(보고)
- Mozilla 버그(보고): https://bugzilla.mozilla.org/show_bug.cgi?id=1833354 (V4L2 H.264), `1965646`(VP8·VP9·HEVC), `2008353`(AV1), `2071471`(코덱 하나만), `1969297`(stateless), `2080696`(샌드박스와 V4L2 VA 드라이버), `1852765`
- FFmpeg(보고): https://code.ffmpeg.org/FFmpeg/FFmpeg/pulls/20847 (`v4l2request`), `pulls/23471`(V4L2-M2M의 `DRM_PRIME`)
- YouTube 도움말: https://support.google.com/youtube/answer/1722171
- 확인하지 못한 것:
  - lore의 원문(403). patchwork에 없는 답장이나 다른 제목의 새 판은 놓쳤을 수 있다.
  - MT6895 커뮤니티 드라이버가 실제로 도는지. 소스와 README만 읽었다.
  - MT6897 펌웨어의 코덱·크기·출력 형식 목록, 4K60 처리량.
  - YouTube의 코덱별 해상도 상한.
  - panfrost가 디코더의 dmabuf를 가져오는지, 컴포지터가 알리는 형식에 NV12가 있는지.
