# 앞 카메라: 조사와 단계 계획

- 작성일: 2026-11-27
- 대상: TB375FC(MT6897), 포크 main `49972d6`, 커널 7.2.12 + 패치 대기열(시험 커널 #344), 순정 16.0.20.091의 장치 트리
- 목적: buntOS의 얼굴 인식 잠금 해제 시제품(본체 저장소 ADR-016)을 이 기기의 앞 카메라로 한다. 2026-11-26에 사용자가 정했다: 적외선 카메라는 권장이고 필수가 아니다. 이 문서는 앞 카메라를 libcamera로 쓰기까지 필요한 것을 정하고, 끝마다 시험할 수 있는 단계로 나눈다. 뒤 카메라는 이 계획 밖이다.
- **기기에서 한 일은 0단계의 시험 하나다.** 시험 모듈로 앞 센서에 약 0.45초 전원을 넣고 칩 ID를 읽었다(2.1절). 나머지는 읽고 조사만 했다.
- 표시:
  - **[실측]** 이 조사에서 태블릿을 직접 읽거나 시험했다.
  - **[기록]** 앞서 받아 둔 기기 기록(순정 장치 트리 `root-091/fdt.dtb`)에서 봤다.
  - **[코드]** 소스를 읽었다. 돌려 보지 않았다.
  - **[조사]** 웹의 원본(저장소, 패키지 목록, 메일링 리스트)을 열어 봤다. 출처는 9절에 있다.
  - **[짐작]** 위 넷이 아니다.

## 1. 결론

- **할 수 있다고 본다. 큰 일이다.** 센서와 전원 길은 기기에서 확인했다. 큰 일은 SoC 쪽 드라이버, 곧 CSI 수신부(seninf)와 원시 프레임을 메모리에 쓰는 블록(camsv)을 새로 쓰는 것이다. 본가에는 MediaTek의 seninf·camsv 드라이버가 없다 [조사].
- **앞 센서는 SmartSens SC820CS다** [실측]. I2C 9번 버스의 0x36에서 칩 ID 0xD154가 읽혔다. Bayer RGB 센서라 적외선 카메라가 아니다 [코드].
- **벤더 소스가 다 공개돼 있다** [조사]. 센서 드라이버와 설정표는 레노버 소스의 미러에 있고, seninf·camsv·CSI PHY는 OnePlus 공식 MT6897 소스에 있다. 모두 GPL-2.0이다.
- **가는 길은 센서 → seninf → camsv → 메모리 → libcamera 소프트 ISP다.** 벤더의 ISP 길(cam-raw)은 CCU 펌웨어와 함께 돌아 크다. camsv는 ISP를 거치지 않고 원시 프레임을 메모리에 쓰는 블록이다 [코드]. 얼굴 인식에는 1632×1224 30 fps로 충분하다고 본다 [짐작].
- **포크에 이미 있는 것**: 전원 도메인 CAM_VCORE·CAM_MAIN·CSI_RX의 드라이버 표(패치 0054, 장치 트리 노드는 없다), MCLK용 camtg 먹스(0013), I2C 9번 노드와 게이트(0044, 꺼 둠), disp·mdp IOMMU와 SMI(0210~0216), DVFSRC.
- **없는 것**: MT6368의 VSIM2 레일(0024에 VSIM이 없다), 카메라 클럭 블록(`camsys_main` 등), SMI larb14 노드, seninf·camsv 드라이버, SC820CS 드라이버, libcamera의 항목.
- 가장 큰 불확실성 셋:
  1. camsv의 DMA가 IOMMU를 거쳐 메모리에 쓰는지. larb14는 순정에서 disp·mdp IOMMU 두 곳의 목록에 다 들어 있고(2.4절), disp IOMMU는 화면이 쓰고 있다.
  2. CSI PHY의 보정과 타이밍. 순정은 efuse 셀 `rg_csi`와 settle 값을 쓴다.
  3. 카메라 전원 도메인과 VCORE. seninf는 데이터 속도에 따라 DVFSRC에 VCORE를 요청한다(`cdphy-dvfs-step`) [기록].
- **1단계부터 시험 커널과 재부팅이 필요하다.** MT6368 regulator 드라이버가 커널에 내장돼 있어, VSIM2를 더하려면 새 커널이 필요하다 [코드: 시험 커널 #344의 빌드 설정]. 장치 트리도 시험 커널에 내장돼 있다.

## 2. 하드웨어

### 2.1 앞 센서 [실측, 2026-11-26]

- 순정 트리는 앞 센서 자리(`sensor1@10`, I2C 9번)에 후보 둘 `s5k4h7_mipi_raw`, `sc820cs_mipi_raw`를 적는다 [기록].
- 시험 모듈이 순정과 같은 줄로 전원을 넣고, GPIO155·156에 소프트웨어 I2C 버스를 붙였다. 장치 트리 오버레이 없이 GPIO 조회표, 핀 매핑, SPMI 직접 읽기·쓰기로 했다.
- 결과:
  - 0x36: 레지스터 0x3107·0x3108 = `0xd1 0x54`(SC820CS의 칩 ID 0xD154).
  - 0x50: 모듈 EEPROM이 대답했다(2.3절). 내용은 읽지 않았다.
  - S5K4H7(칩 ID 0x487B)의 주소 0x10·0x2D는 대답하지 않았다.
- 모듈을 내린 뒤 VSIM2 레지스터와 GPIO 방향은 처음 값으로 돌아갔다. 커널 경고는 없었다.

### 2.2 전원과 제어 줄 [기록, 실측]

| 줄 | 앞(SC820CS, I2C 9번) | 뒤(뒤 카메라) |
|---|---|---|
| DOVDD(입출력) | MT6368 VSIM2 | MT6368 VSIM2 (같은 줄) |
| AVDD(아날로그) | GPIO36 high | GPIO36 high (같은 줄) |
| DVDD(디지털) | GPIO37 high | GPIO38 high |
| 리셋 | GPIO118 (high = 동작) | GPIO116 |
| MCLK | topckgen `camtg4` → GPIO126 기능 1 `CMMCLK3` | `camtg2` → GPIO124 기능 1 `CMMCLK1` |
| AF 전원 | 없음 | MT6368 VIBR |
| I2C | GPIO155 `SCL9`, GPIO156 `SDA9` | 다른 버스 |

- **AVDD와 DOVDD를 앞뒤가 같이 쓴다.** 앞을 켤 때는 뒤 센서의 리셋(GPIO116)을 low로 잡는다. 순정도 그렇게 한다 [기록]. 부팅 뒤 처음 상태: GPIO116·118·36은 출력 low, GPIO37은 입력이었다 [실측].
- **VSIM2 레지스터** [코드, 실측]: SPMI 절대 주소(SID 5)로 EN 0x1b96(비트 0 켜기, 비트 1 저전력), VOCAL 0x1d90, VOSEL 0x1d91이다. VOSEL은 비트 3:0으로 벤더의 `ldo_volt_table1`을 고르며, 4가 1.8 V다.
  - 레노버 미러와 OnePlus 공식 소스의 `mt6368-regulator.h`가 같다. 우리 드라이버(0024)는 같은 레지스터를 노드 기준 0x30을 뺀 주소로 쓴다. VIBR로 이 대응을 기기에서 확인했다.
  - 기기에서 읽은 VSIM2: 꺼짐, VOSEL 4, VOCAL 6. VOCAL을 10 mV 단계로 읽으면 1.86 V다.
  - 벤더 드라이버는 VOCAL을 쓰지 않고 VOSEL만 쓰므로, 순정에서도 1.86 V였을 것이다 [짐작].
- **벤더의 켜는 순서** [코드: 레노버 SC820CS 드라이버의 `pw_seq`]: 리셋 low → MCLK 24 MHz(구동 단계 2) → DOVDD 1.8 V, 5 ms → DVDD, 9 ms → AVDD, 5 ms → 리셋 high, 5 ms. 0단계 시험은 이와 다른 순서(DOVDD → AVDD → DVDD → MCLK → 리셋)로 칩 ID만 읽었다. 드라이버는 벤더 순서를 따른다.

### 2.3 센서의 설정 [코드]

레노버 미러의 `sc820cs_mipi_raw`(src-v4l2 틀)에서 읽었다.

- I2C 주소 0x6c(7비트 0x36), 칩 ID 레지스터 0x3107·0x3108, MCLK 24 MHz, MIPI D-PHY 4레인, 데이터 형식 0x2b(RAW10), `RAW_B`, 검은 수준(OB) 0x40.
- 모드 셋, 모두 30 fps:
  - preview 1632×1224(3264×2448을 2×2로 묶음)
  - capture 3264×2448
  - video 3264×1836
  - 셋 다 pclk 132 MHz, 줄 길이 1760, 프레임 길이 2500, MIPI 픽셀 속도 264 M/s다.
- 링크 주파수는 레인당 660 Mbps, 곧 330 MHz로 셈한다 [짐작: 264 M × 10비트 ÷ 4레인 ÷ 2. 벤더의 "MIPI 픽셀 속도"가 이 뜻인지는 센서의 PLL 설정으로 확인한다].
- 아날로그 이득은 1~16배이고, 표(`sc820cs_ana_gain_table.h`)로 코드를 고른다.
- 레지스터: 스트림 0x0100, 노출 0x3e00~0x3e02, 이득 0x3e08·0x3e09, 프레임 길이 0x320e·0x320f. OnePlus 공식 SC820CS 드라이버도 같은 레지스터를 쓴다.
- EEPROM(`sc820cs_mtk_eeprom.c`): 주소 0xA0(7비트 0x50), 크기 0x4000. 모듈 정보는 0x01부터 0x1D바이트, 렌즈 그림자 보정(LSC)은 0x48부터 0x74C바이트다.

### 2.4 SoC 쪽 [기록, 코드]

- **seninf** (`seninf-top@1a00e000`, `mediatek,seninf-core`):
  - 레지스터 `base` 0x1a00e000(0x18000), `ana-rx` 0x11c80000(0x30000). seninf 12, mux 13, cam-mux 43.
  - 전원 도메인 CAM_MAIN(벤더 번호 17)과 CSI_RX(30).
  - 클럭: `camsys_main`의 seninf·cam·camtg 게이트, topckgen의 `seninf`·`seninf1`~`seninf5`·`camtm` 먹스.
  - DVFSRC VCORE 공급과 `cdphy-dvfs-step0`~`3`.
  - CSI 포트: 앞 센서는 `seninf-csi-port-0`(`dphy-settle-delay-dt = <0x11>`, efuse 셀 `rg_csi`), 뒤 센서는 포트 1(`hs-trail-parameter = <0x20>`).
- **CSI PHY는 3.0이다.** 벤더 seninf 드라이버는 `mtk-csi-phy-ver` 속성이 `3.1`일 때만 3.1(MT6878용)을 고른다. 순정 트리에는 이 속성이 없다 [코드, 기록].
- **camsv** 여섯(`camsv1@1a100000`부터 0x1000 간격, `mediatek,camsv`):
  - 레지스터는 블록마다 `base`, `base_DMA`, `base_SCQ`와 각각의 `inner_` 판이다. camsv1은 cam-mux 0번에 붙는다.
  - 전원 도메인 CAM_MAIN.
  - 클럭: `camsys_main`의 `cam_cgpdn`·`larb14_con`·`camsv_top_con`·`cam2mm0/1/2_gals`·`cam2sys_gals`·`camsv_a_con`, topckgen `cam`·`camtm`, vlp `camtg`.
  - EMI 대역폭 요청 `interconnects`.
- **SMI larb14** (`0x1a002000`, larb ID 14): CAM_MAIN 도메인, `camsys_main`의 larb14 게이트. 순정에서는 `iommu@1e802000`과 `iommu@1e810000`의 larb 목록에 모두 들어 있다. 두 IOMMU는 우리 트리의 `disp_iommu`·`mdp_iommu`다. camsv의 대역폭 이름도 `imgo0_disp_a`, `imgo0_mdp_a`처럼 두 길을 적는다 [기록]. 포트를 어느 IOMMU에 두는지는 벤더 코드로 확인한다.
- **카메라 클럭 블록**: 순정 트리에 `camsys_main`, `camsys_rawa/b/c`, `camsys_yuva/b/c`, `camsys_mraw`, `camsys_ipe`, `cam_vcore`가 있다 [기록]. seninf와 camsv에는 `camsys_main`이면 된다고 본다 [짐작].

### 2.5 포크의 지금 상태 [코드]

| 것 | 상태 |
|---|---|
| 전원 도메인 CAM_VCORE, CAM_MAIN, CSI_RX | 드라이버 표만 있다(패치 0054, CSI_RX는 기본 꺼짐). 장치 트리의 전원 제어기 노드에는 AUDIO, CONN, 화면, MM_INFRA, VDE0·VDE1뿐이다 |
| topckgen `camtg2`~`camtg8`, `seninf*` 먹스 | 있다(0013). `camtg4`는 지금 24 MHz(`univpll_192m_d8`), 꺼짐 [실측] |
| `camsys_main` 등 카메라 클럭 블록 | 없다(DESIGN 5절의 "카메라·ISP 블록 20개, 게이트 136"에 든다) |
| MT6368 VSIM2 | 없다. 0024에 VSIM1·VSIM2가 없고, 보드 노드도 없다 |
| I2C 9번 | 컨트롤러 노드와 게이트 `CLK_IMPES_I2C9`가 있고 꺼 뒀다. 핀 설정(`SCL9`·`SDA9`)은 없다 |
| disp·mdp IOMMU, SMI | 있다(0210~0216, 비디오 디코딩 작업). larb14 노드는 없다 |
| DVFSRC | 모듈이 있다(비디오 디코딩 작업) |
| V4L2 | `VIDEO_DEV`, `V4L2_FWNODE`, `V4L2_CCI_I2C`, `VIDEOBUF2_DMA_CONTIG`가 모듈, `MEDIA_CONTROLLER`와 `VIDEO_V4L2_SUBDEV_API`가 내장 [코드: 시험 커널 #344의 빌드 설정] |

## 3. 소스와 업스트림

### 3.1 벤더 소스 [조사]

- **SC820CS, 이 기기 것**: `lenovo-mt8792-devs/android_kernel_lenovo_mt8792-modules`, 갈래 `lineage-24.0`의 `drivers/media/platform/mtkcam/imgsensor/src-v4l2/common/sc820cs_mipi_raw/`. 커뮤니티 미러이고, 커밋 기록에 레노버의 `Kernel-6.1.138.tar.gz`를 들여왔다고 적혀 있다. 머리글은 SPDX GPL-2.0, Copyright MediaTek이다. 같은 미러에 EEPROM 배치(`cam_cal/src_v4l2/custom/layout/sc820cs_mtk_eeprom.c`)와 MT6897 camsys 전체가 있다.
- **SC820CS, 제조사 공식**: OnePlus(MT6789, OnePlus Pad Go, `sc820cs_mipi_raw_bluey`), OPPO(MT6835, `_barley`), realme(C75)가 모두 칩 ID를 0xD154로 적는다. OnePlus 드라이버는 주소 0x6c, 4레인, MCLK 24 MHz, 같은 타이밍이다. 설정표의 PLL 값은 레노버와 다르다(모듈이 다르다).
- **seninf·camsv·CSI PHY, 제조사 공식**: `OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6897`, 갈래 `oneplus/mt6897_b_16.0.0_oneplus_pad`의 `vendor/mediatek/kernel_modules/mtkcam/camsys/isp7sp/cam/`. 여기에 `mtk_cam-plat-mt6897.c`, `mtk_cam-seninf-drv.c`(약 127 KB), `mtk_cam-sv.c`(약 79 KB), `mtk_csi_phy_3_0/`, `mtk_csi_phy_3_1/`이 있다. 비디오 디코딩 작업에서 쓴 것과 같은 저장소다.
- 레노버 공식 소스 묶음(레노버의 오픈 소스 내려받기)과 미러가 같은지는 아직 보지 않았다. 설정표를 패치에 넣기 전에 맞댄다.

### 3.2 업스트림 [조사]

- **본가에 없다.** master(`25faec9ce61c`, 2026-11-26)의 `drivers/media/platform/mediatek/`에는 `jpeg`, `mdp`, `mdp3`, `vcodec`, `vpu`뿐이다.
- 병합되지 않은 시리즈 둘:
  - **MT8365 ISP3.0(BayLibre)**: seninf와 camsv 두 드라이버다. camsv는 SoC의 ISP를 거치지 않고 seninf의 데이터를 메모리에 쓰는 길로, 우리 길과 같은 모양이다. 확인한 것은 v6(2024-07)까지다.
  - **MT8188 ISP7.x camsys(MediaTek)**: v1(2024-10)과 v2(2025-07)가 있다. seninf 쪽은 `mediatek,mt8188-seninf-core`로, 순정의 `seninf-core`와 이름 틀이 같다. 가장 가까운 칩 세대다.
- 센서 드라이버(SC820CS)는 본가에 없다. `drivers/media/i2c/`의 다른 센서 드라이버 틀(CCI 레지스터 도우미)로 새로 쓴다.

## 4. 사용자 공간

- **libcamera simple 파이프라인과 소프트 ISP.** ISP 하드웨어 없이 원시 프레임을 소프트웨어로 디베이어하고 자동 노출·화이트 밸런스를 맞춘다 [조사]. simple 파이프라인은 소프트 ISP를 드라이버마다 켜고 끈다 [코드].
  - Debian testing의 libcamera는 0.7.2, trixie는 0.4.0이다 [조사].
  - simple 파이프라인은 드라이버 이름 목록으로 장치를 받는다. 지금 목록의 MediaTek 항목은 `mtk-seninf` 하나이고, 변환기로 `mtk-mdp`를 쓰며 소프트 ISP는 꺼져 있다 [코드]. 우리 드라이버에는 소프트 ISP를 켠 항목이 필요하므로 libcamera에 한 줄짜리 고침이 필요하다.
  - 노출과 이득을 제대로 맞추려면 libcamera에 이 센서의 도우미(이득 코드 ↔ 배율)와 센서 속성이 있어야 한다고 본다 [짐작]. 화소 크기는 아직 모른다.
- **얼굴 엔진까지.** Howdy는 V4L2 장치를 OpenCV나 ffmpeg로 연다. camsv의 원시 노드는 바로 쓸 수 없으니, libcamera의 V4L2 호환 층이나 PipeWire를 거친다 [짐작]. 5단계에서 확인한다.

## 5. 단계 계획

단계마다 끝에 시험이 있고, 앞 단계가 지나야 다음 단계에 뜻이 있다. 크기는 [포팅 재료 조사](porting-materials.md)와 같은 눈금이다(작음, 중간, 큼).

| 단계 | 만드는 것 | 끝의 시험 | 사용자에게 필요한 것 | 크기 | 판정 |
|---|---|---|---|---|---|
| 0 | 센서 확인(시험 모듈)과 이 계획 | 칩 ID 0xD154 | 없음 | — | 끝 |
| 1 | 전원과 센서: MT6368 VSIM2 레일, I2C 9번과 핀, 보드 노드, SC820CS 센서 드라이버(모듈) | 드라이버가 켜는 순서대로 전원을 넣고 칩 ID를 읽는다. 끄면 레일과 핀이 돌아간다. 화면, 잠자기, 뒤 카메라 줄이 그대로다 | 시험 커널 굽기와 재부팅 | 중간 | **지났다(2026-11-27, 시험 커널 #346, 5.1절)** |
| 2 | 카메라 전원과 클럭, SMI: CAM_VCORE·CAM_MAIN·CSI_RX 노드, `camsys_main` 게이트, larb14 노드 | 켜고 끄기를 되풀이해도 화면과 잠자기가 그대로다. 켜 둔 동안에만 seninf·camsv의 레지스터를 읽는다 | 재부팅 | 중간 | **전원과 클럭이 됐다(2026-11-27, 시험 커널 #352, 5.2절). larb14 노드는 아직이다.** #347로 부팅하면 화면이 깨졌는데, 전원 제어기의 첫 probe가 중간에 미뤄진 탓이었다(패치 0247로 고침) |
| 3 | seninf: CSI PHY 3.0, efuse 보정, 포트 0 → mux → cam-mux 길 | 센서가 내보내는 동안 seninf의 상태 칸에 패킷과 프레임이 잡힌다 | 재부팅 여러 번 | 큼 | **됐다(2026-11-27, 시험 커널 #353, 5.3절). seninf 드라이버(0248~0249)로 센서 프레임이 cam-mux까지 들어온다** |
| 4 | camsv와 V4L2: 원시 프레임 DMA, larb14 포트와 IOMMU, media 그래프(센서 → seninf → camsv → 비디오 노드) | `v4l2-ctl`로 RAW10 프레임을 받는다. 크기가 맞고, 렌즈를 가렸을 때와 비출 때 값이 다르다 | 시험 캡처를 어디로 비출지 정하기(7절) | 큼 | — |
| 5 | libcamera: simple 파이프라인 항목, 센서 도우미, 소프트 ISP, 꾸러미 | `cam`으로 30 fps를 받고 qcam에 그림이 나온다. CPU 사용률을 잰다 | 그림 확인 | 중간 | — |
| 6 | 얼굴 엔진 시험(ADR-016의 1단계로 넘긴다): Howdy의 `add`·`test`를 libcamera 길로 돌린다 | 본인 인식률, 시간, 사진이나 화면 속 영상에 속는지 | 얼굴 등록 | 중간 | 본체 저장소 |
| 7 | 다듬기: 잠자기, 전력, VCORE와 클럭, 이미지에 넣기, 업스트림 | 잠자기 뒤 캡처, 캡처 중 전력과 온도 | 긴 시험 | 중간 | 뒤에 정한다 |

### 5.1 1단계: 전원과 센서

- **VSIM2**: 0024에 VSIM1·VSIM2 설명을 더한다(`ldo_volt_table1`, VOCAL은 VIBR처럼 10 mV 단계). 보드의 MT6368 노드에 `vsim2`를 1.8 V로 넣는다.
  - 순정의 1.86 V(VOCAL 6)를 그대로 둘지, 1.80 V로 맞출지 정한다. 둘 다 센서의 DOVDD 범위 안이라고 본다 [짐작: 데이터시트를 보지 못했다].
- **AVDD·DVDD**: GPIO36·37을 각각 `regulator-fixed`로 둔다. GPIO36은 뒤 센서와 함께 쓰므로 나중에 뒤 센서 노드도 같은 레일을 가리킨다.
- **뒤 센서 리셋**: GPIO116을 출력 low로 묶어 둔다(`gpio-hog`). 뒤 카메라를 맡는 드라이버가 생기면 푼다.
- **I2C 9번**: `status = "okay"`와 핀 `SCL9`·`SDA9`(GPIO155·156 기능 1). 400 kHz는 순정 값이다 [기록].
- **센서 노드**: `smartsens,sc820cs`(새 바인딩), 주소 0x36. 공급 셋, `reset-gpios`(GPIO118), `clocks = <&topckgen CLK_TOP_CAMTG4>`와 24 MHz, MCLK 핀(GPIO126 기능 1), 엔드포인트(레인 1~4, 링크 주파수)를 둔다. seninf 쪽 엔드포인트는 3단계에서 잇는다.
- **드라이버** `sc820cs.c`: `v4l2_subdev`와 CCI 도우미를 쓴다.
  - 컨트롤: 노출, 아날로그 이득, vblank, hblank, 픽셀 속도, 링크 주파수.
  - 모드는 셋 모두 옮기되 먼저 1632×1224를 쓴다.
  - 설정표는 레노버 소스에서 가져오고(GPL-2.0, MediaTek 저작권 표시를 따른다), 런타임 PM으로 켜고 끈다.
- **시험**:
  - 부팅 때 드라이버가 켜는 순서대로 전원을 넣고 칩 ID를 읽은 뒤 끈다.
  - 런타임 PM으로 켜고 끈다(`power/control`을 `on`으로 두었다가 `auto`로). 켠 동안 사용자 공간에서 칩 ID를 다시 읽는다.
  - 수신부가 없으면 센서의 서브디바이스 노드가 생기지 않아, 컨트롤 쓰기와 스트림 켜기는 3단계에서 본다.
  - 잠자기와 깨기 뒤에도 같다.
- **결과** [실측, 2026-11-27, 시험 커널 #346]:
  - 부팅 기록에 `sc820cs 9-0036: SC820CS, chip ID 0xd154`. I2C 9번은 `i2c-mt65xx` 컨트롤러로 떴다.
  - 런타임 PM으로 켜면 `mt6368_vsim2`(1.86 V), `cam_avdd`, `cam1_dvdd`가 켜지고 `camtg4`가 24 MHz로 켜진다. 그때 `i2ctransfer`로 읽은 칩 ID가 `0xd1 0x54`다. 1초 뒤 자동으로 꺼져 레일 셋과 클럭이 모두 꺼진다.
  - 핀: GPIO126은 센서가 `CMMCLK3`로, GPIO155·156은 I2C 컨트롤러가 `SCL9`·`SDA9`로 잡는다. GPIO36·37은 고정 레귤레이터가, GPIO118은 리셋으로, GPIO116은 `rear-camera-reset` 고정으로 출력 low다.
  - 재부팅 전후 비교에서 바뀐 것은 카메라 쪽뿐이다(새 모듈 넷, 새 레일 셋, I2C 9번과 장치, 핀). 화면, Wi-Fi, 블루투스, 소리, GPU, 스왑, 서비스는 그대로이고 커널 경고는 0줄이다.
  - 보지 않은 것: 잠자기와 깨기 뒤(전원 키를 눌러야 한다), 설정표와 컨트롤 쓰기(3단계).
- **만들며 고친 것**:
  - I2C 9번에 별칭 `i2c9`를 두었다. 별칭 없는 버스는 14번부터 번호를 받는데(지금 앰프 14번, 연료계 15번), 별칭 없이 켜면 그 번호들이 밀릴 수 있다.
  - I2C 9번의 클럭 블록 `imp_iic_wrap_es`는 시험 커널의 덧씌움(`test-stage.dtsi`)이 이미 켜 두었다(I2C 3·13번이 쓴다).

### 5.2 2단계: 카메라 클럭과 SMI

- 전원 제어기 노드에 CAM_VCORE, CAM_MAIN, CSI_RX를 더한다. 부모 관계는 벤더 트리를 따른다.
- `camsys_main` 게이트를 클럭 드라이버에 더한다. 게이트 목록과 비트는 OnePlus 공식 소스의 `clk-mt6897-cam.c`와 순정 트리의 `clock-names`로 맞춘다. 필요하면 `cam_vcore`도 더한다.
- larb14 노드를 넣고, IOMMU 목록에 larb14 포트를 더한다. 처음에는 MMU 비트를 쓰지 않는다. 비디오 디코딩 1단계처럼 부팅 때는 아무 레지스터도 쓰지 않고, 시험 때만 켠다.
- **차례가 중요하다.** CAM_VCORE → CAM_MAIN → 클럭을 켠 뒤에만 그 블록의 레지스터를 읽는다. 꺼진 도메인을 읽으면 기기가 재부팅된다. 이 기기에서 여러 번 겪었다.
- 시험: 켜고 끄기 스무 번, 그 사이 화면 프레임과 잠자기. 켠 동안 seninf와 camsv의 판(버전) 칸을 읽는다.
- **만든 것** (패치 0244~0246, 시험 커널 #347):
  - `camsys_main`(게이트 30)과 `cam_vcore`(1)의 클럭 블록. 도메인이 이 게이트를 켜야 올라오므로 상태를 읽지 않는 연산을 쓰고 커널에 넣는다.
  - 시험 덧씌움에 MM_INFRA 아래 CAM_VCORE, 그 아래 CAM_MAIN, 최상위 CSI_RX를 순정의 클럭 목록대로 둔다. CAM_MAIN의 공급은 늘 켜 두는 `vmm`(MT6319 6/vbuck1)이다.
  - 순정의 "ccu"는 topckgen 19번(camtg7)을 가리킨다. 여기서는 CCU의 실제 클럭인 VLP 19번(ccusys)을 쓴다.
  - 시험 노드는 꺼 둔 채 두고, 시험 모듈 `camtest`가 그 노드로 장치를 만든다.
- **결과** [실측, 2026-11-27]:
  - 전원 제어기는 새 도메인 셋을 꺼진 채로 등록했다.
  - 시험 모듈의 단계 다섯이 모두 오류 없이 지났고, 커널 경고는 없었다.
    - CSI_RX 켜고 끄기.
    - CAM_MAIN 켜고 끄기. genpd가 CAM_VCORE를 먼저 켰다.
    - CAM_MAIN을 켠 채 게이트 다섯을 열고 읽기. `camsys_main`의 게이트 레지스터가 연 게이트만 0으로 읽혔다(CG0 0x0ff0f1e0, CG1 0x3e). `SENINF_TOP_CTRL`과 camsv1의 예비 레지스터는 0이었다.
    - CSI_RX도 켜고 `CDPHY_RX_ANA_0`을 읽기. 0x00670010이었다.
  - **그러나 #347로 부팅하면 화면이 깨졌다**(왼쪽 반 검정, 오른쪽 반 세로줄). Wi-Fi도 공유기 인증에서 계속 시간 초과가 났다.
    - 비디오 서비스가 디코더 쪽 IOMMU를 붙인 직후(6.2초)부터 디스플레이 IOMMU(`1e802000`)에서 번역 오류가 쏟아졌다. 그 부팅에서만 1,290개였다.
    - 기록에 남은 부팅 16번 가운데 IOMMU 오류가 난 것은 #347 부팅 하나뿐이다. 그래서 #347의 변경이 원인일 가능성이 컸다. 경로는 아래 "좁히기"에서 찾았다.
    - IOMMU를 붙이는 차례는 #346과 같았다. 오류 기록의 `larb=0`은 MT6897 데이터에 larb 번호표가 없어 늘 0으로 찍히는 값이라, 어느 장치가 읽었는지 알 수 없다.
    - Wi-Fi 인증 시간 초과는 예전 #343·#344 부팅에도 있었다.
  - USB로 접속해 기록을 받고 #346으로 되돌렸다(모듈 폴더 되돌리기, boot_b에 #346). 되돌린 뒤 화면, Wi-Fi, IOMMU 오류(0)가 모두 정상이다.
- **좁히기** [실측, 2026-11-27, 시험 커널 #348~#351]: 2단계의 변경을 나눠 넣은 커널로, 비디오 서비스를 끈 채 부팅했다. 그다음 비디오 모듈을 손으로 올려 IOMMU 오류를 셌다. USB를 꽂아 두었다.

| 커널 | 넣은 것 | 전원 제어기 등록 | IOMMU 오류 |
|---|---|---|---|
| #348 | 클럭 블록만 | 한 번 | 없음 |
| #349 | CAM_VCORE와 CAM_MAIN | 두 번 | 남 |
| #350 | CAM_VCORE만 | 한 번 | 없음 |
| #351 | CAM_VCORE와, 공급만 달고 게이트는 뺀 CAM_MAIN | 두 번 | 남 |

  - 아홉 번의 부팅에서 오류는 전원 제어기가 두 번 등록된 부팅에서만, 그리고 그때마다 났다.
  - 경로: CAM_MAIN의 공급(`mt6319_6_vbuck1`)은 전원 제어기가 처음 probe할 때 아직 등록되지 않았다. 드라이버는 도메인을 하나씩 더하면서 기본으로 꺼 두지 않는 도메인을 바로 켠다. 그래서 CAM_MAIN에서 미뤄질 때는 이미 MM_INFRA, DIS0, OVL0을 켠 뒤였고, 정리하며 이 셋을 껐다가 다음 probe에서 다시 켰다. 부트로더가 켜 둔 화면 길의 일부를 껐다 켠 것이다.
  - 그 뒤 비디오 서비스가 IOMMU를 붙이자, 디스플레이 IOMMU가 CMA 영역의 주소(화면 버퍼로 짐작)에서 번역 오류를 냈고 화면 반쪽이 검게 됐다.
  - Wi-Fi(CONN)와 오디오 도메인은 기본으로 꺼 두는 도메인이라 정리 때 끄지 않는다(패치 0052). #347의 Wi-Fi 인증 시간 초과는 이 일과 따로다.
- **고침** (패치 0247, 시험 커널 #352 = #347 + 0247): 전원 제어기가 도메인을 하나라도 더하기 전에 모든 도메인과 하위 도메인의 공급과 클럭을 먼저 찾는다. 하나라도 없으면 아무 도메인도 건드리지 않고 미룬다. 본가 드라이버도 같은 차례로 도메인을 켜므로 같은 결함이 있다.
- **고친 결과** [실측, 2026-11-27 02:02, #352]:
  - 전원 제어기가 한 번만 등록됐고, 미뤄진 장치가 없다.
  - 비디오 서비스가 디코더까지 올린 뒤에도 IOMMU 오류가 0이다. DSI 켜짐, Wi-Fi 붙음, 커널 경고 0, 실패한 유닛 0.
  - camtest 단계 4가 #347과 같은 값으로 지났다. 1단계 센서 드라이버도 칩 ID를 읽었다.
  - 사용자가 화면이 반쪽 없이 다 나오는 것을 눈으로 확인했다(02:07).

### 5.3 3단계: seninf

- 작은 드라이버로 시작한다: 포트 0, D-PHY 4레인, seninf 하나, mux 하나, cam-mux 0번(camsv1).
- 틀은 벤더 isp7sp의 seninf(PHY 3.0)와 MT8188 시리즈의 구조에서 가져온다.
- CSI 보정은 efuse 셀 `rg_csi`를 nvmem으로 읽는다. 그 값은 기록에 남기지 않는다.
- VCORE: 처음에는 시험 동안 DVFSRC에 높은 단계를 걸어 둔다. 데이터 속도별 단계(`cdphy-dvfs-step`)는 7단계에서 맞춘다.
- 시험: 1단계의 센서를 스트림으로 켜고, seninf의 패킷·프레임 상태 칸이 움직이는지 본다. 벤더 드라이버의 상태 출력(`_show_status`)이 읽는 칸을 따른다.
- **레지스터 차례를 기기에서 확인했다** [실측, 2026-11-27 02:34~02:52, 시험 커널 #352, 커널 고침 없음]:
  - 차례는 OnePlus 공식 MT6897 소스의 isp7sp seninf(CSI PHY 3.0, `mtk_cam-seninf-hw_phy_3_0.c`)에서 뽑았다. 손으로 레지스터를 쓰는 시험 모듈(저장소 밖)로 돌렸다.
  - 주소: `base`(0x1a00e000) 안에 seninf 위(+0), seninf1 제어(+0x200), seninf1 CSI-2(+0xa00), mux1(+0xd00), 시험 무늬 생성기 TG1(+0xf00), cam-mux k(+0x17000 + 0x40·k), cam-mux 전역(+0x17f00)이 있다. `ana-rx`(0x11c80000) 안에 CSI0A 아날로그(+0), CSI0B(+0x1000), D-PHY(+0x2000), C-PHY(+0x3000), MAC 위(+0x4000), 포트 0의 CSI-2 MAC(+0x5000)이 있다.
  - 포트 0은 4D1C(데이터 4, 클럭 1)로 A·B 반쪽을 함께 쓰고 seninf1을 쓴다. 클럭은 CSI0A의 L2 쌍이고, 레인 선택(LC0=2, LD0=1, LD1=3, LD2=0, LD3=4)은 벤더 코드에 고정돼 있다. ECC가 맞으니 이 보드와 맞는다.
  - 차례: seninf1과 MAC 위 리셋 → VC/DT 거르개 → 아날로그 초기값(A, B) → D-PHY 타이밍 → MAC 위와 CSI-2 → seninf1 켜기 → 아날로그 D-PHY 모드와 전원(밴드갭, 200 µs, 저역 필터, 오프셋 보정) → 레인 켜기와 선택 → mux1(소스 8, 화소 모드 3) → cam-mux 0(바깥쪽 페이지, 안쪽 페이지) → 센서 스트림. 센서는 먼저 켜 두어 레인이 LP-11에 있게 한다.

| 단계 | 한 일 | 결과 |
|---|---|---|
| 0 | CSI_RX, CAM_MAIN, camsys 게이트, topckgen seninf 먹스(312 MHz)와 camtm(208 MHz)을 켜고 읽기 | 됐다 |
| 1 | seninf 안의 시험 무늬 생성기 → seninf1 → mux1 → cam-mux 0, PHY와 센서 없음 | mux1과 cam-mux 0이 5632×4224 프레임을 받았다 |
| 2 | 센서 없이 포트 0 수신부 설정과 되읽기 | 아날로그, D-PHY, MAC, cam-mux의 값이 벤더 코드로 계산한 값과 모두 같다. 쓰는 레인의 오프셋 보정이 끝났다 |
| 3 | 센서 스트림 1초 | 1632×1224 RAW10(DT 0x2b, 패킷 2040바이트)이 초당 30장, ECC와 CRC 정상. 프레임마다 패킷 1225개 |
| 4 | 3에 센서 시험 무늬 | 3과 같다 |

  - **D-PHY trail이 갈랐다.** 센서 쪽 값 없이 벤더 기본 경로(data settle 26, clock settle 0, trail 꺼짐)로 받으면, 패킷 머리(ECC)는 맞는데 모든 패킷의 CRC가 틀렸다. 레노버의 SC820CS 드라이버는 `get_csi_param`으로 settle 고정을 풀고 data·clock settle 0x28, trail 0x40을 준다. 벤더 seninf 코드는 이 값을 seninf 클럭(312 MHz) 주기로 그대로 쓴다. trail 0x40을 켜면 settle이 26이든 0x28이든 ECC와 CRC가 모두 정상이었다. clock settle 0x28에 trail을 끄면 네 레인 모두 SoT 동기 오류가 나고 FS·FE를 잃었다.
  - 그 밖에 본 것: cam-mux 0~31은 전원을 켜면 모두 켜진 상태다. 벤더는 처음 켤 때 mux와 cam-mux를 모두 끈다. MAC의 패킷 수는 프레임마다 다시 센다.
  - efuse 보정값은 읽지 않았다. 종단 저항 코드는 기본값 0x10이다.
  - 이 시험 동안 커널 경고와 IOMMU 오류는 없었고, 화면과 Wi-Fi는 그대로였다.
- **드라이버** (패치 0248~0249, 시험 커널 #353) [실측, 2026-11-27 03:22~03:24]:
  - `drivers/media/platform/mediatek/mt6897-seninf/`에 있다. V4L2 서브디바이스(입력 패드, 출력 패드)와 미디어 장치를 만들고, 센서를 fwnode 그래프로 묶는다. 하드웨어와 전원 도메인 둘은 스트림 동안만 켠다(런타임 PM 장치 링크).
  - 위의 차례를 그대로 옮겼다. 처음 켤 때 mux와 cam-mux를 모두 끈다. D-PHY 타이밍은 원래 센서 드라이버의 값을 ns로 두고(data settle 128, clock settle 128, trail 205) seninf 클럭 주기로 바꾼다.
  - 종단 저항 보정은 퓨즈 셀(devinfo 사본 0x1d4, `csi-trim@1d4`)을 읽어 벤더와 같은 검사합이 맞을 때만 쓴다. 값은 출력하지 않는다. 이 기기에서 검사합이 맞아 보정값을 썼다.
  - 받는 쪽(camsv)이 아직 없어 debugfs의 `stream`으로 켜고 끄고, `status`로 상태를 본다.
  - 결과: 부팅 때 `/dev/media0`와 seninf·센서의 서브디바이스 노드가 생겼다. 스트림을 켜면 MAC 상태 0x324(ECC·CRC 정상), 초당 30장이고 mux1과 cam-mux 0이 1632×1224를 받았다. 켜고 끄기 다섯 번이 모두 같았고, 끄면 도메인까지 꺼졌다. 수신부를 먼저 켜고 센서가 그 뒤에 켜지는 순서로도 된다. 커널 경고와 IOMMU 오류는 없었고, 화면과 Wi-Fi는 그대로였다.
- 다음: 4단계(camsv의 DMA, larb14, IOMMU).

### 5.4 4단계: camsv와 V4L2

- camsv1이 seninf의 cam-mux 0번 출력을 받아 `imgo` DMA로 메모리에 쓴다. RAW10을 MIPI 묶음 그대로 쓸지 16비트로 풀어 쓸지는 camsv가 내는 형식과 libcamera 소프트 ISP가 받는 형식으로 정한다 [짐작].
- 벤더는 `SCQ`와 `inner` 레지스터(그림자 레지스터)로 프레임마다 설정을 넘긴다. 처음에는 프레임 시작 인터럽트에서 다음 버퍼 주소를 직접 쓰는 길로 해 본다 [짐작].
- larb14 포트의 MMU를 켜고 IOMMU 도메인에 버퍼를 매핑한다. disp IOMMU는 화면이 쓰고 있으므로, 포트를 바꿀 때 화면이 깨지는지 본다.
- V4L2: `vb2-dma-contig`, media 그래프, 비디오 노드 하나.
- 시험: `v4l2-ctl --stream-mmap`로 프레임을 받는다. 크기와 프레임 간격을 확인하고, 렌즈를 가렸을 때와 비출 때를 견준다. 그림으로 바꿔 보는 것은 사람이 보고 싶다고 할 때만 한다(7절).

### 5.5 5단계: libcamera

- simple 파이프라인 목록에 우리 media 장치의 드라이버 이름을 소프트 ISP를 켠 채로 더하는 한 줄짜리 고침을 넣는다. 고침은 BSP 꾸러미로 내고, libcamera에도 낸다.
- 센서 도우미: 이득 표(`sc820cs_ana_gain_table.h`)로 코드와 배율을 바꾼다. 센서 속성에는 화소 크기를 넣어야 하는데, 값을 공개 자료에서 찾는다.
- 시험: `cam -l`에 카메라가 나오고, `cam --capture`로 30 fps를 받는다. qcam이나 GStreamer(`libcamerasrc`)로 화면에 보인다. 이때의 CPU 사용률을 잰다.

### 5.6 6단계 이후

- 6단계는 본체 저장소 ADR-016의 1단계다. 이 문서에서는 카메라를 넘겨주는 데까지다.
- 7단계: 잠자기 때 센서와 도메인을 끄는 것, 캡처 중 전력, VCORE 단계, 이미지와 BSP 꾸러미에 넣기, 센서 드라이버의 업스트림.

## 6. 위험

| # | 위험 | 드러나는 곳 | 줄이는 길 |
|---|---|---|---|
| 1 | **꺼진 블록을 읽어 재부팅한다.** 카메라 블록은 CAM_MAIN·CSI_RX 뒤에 있다 | 2~4단계 | 도메인과 클럭을 켠 것을 확인한 뒤에만 읽는다. 시험 모듈 하나에 커널 고침 하나, 첫 커널 경고에서 멈춘다 |
| 2 | **IOMMU를 바꾸다 화면이 깨진다.** larb14가 disp IOMMU 목록에 있다 | 4단계 | larb14 포트의 비트만 바꾼다. 부팅 때는 쓰지 않고 손으로 올리는 시험 모듈에서만 켠다 |
| 3 | **CSI 보정과 타이밍이 틀려 패킷이 안 온다** | 3단계 | 벤더 PHY 3.0의 차례를 그대로 따른다. settle 값은 순정 트리 값을 쓴다 |
| 4 | **VCORE를 두고 다툰다.** seninf, 화면, 비디오 디코더가 모두 DVFSRC를 쓴다 | 3단계, 7단계 | 처음에는 시험 동안 높은 단계로 고정한다 |
| 5 | **앞뒤가 함께 쓰는 줄.** 뒤 센서가 반쯤 켜진다 | 1단계 | 뒤 리셋을 low로 묶는다. AVDD는 레일 하나를 두 노드가 함께 가리킨다 |
| 6 | **설정표가 커뮤니티 미러에서 왔다** | 1단계 | 레노버 공식 소스 묶음과 맞댄다. 같은 센서의 제조사 공식 드라이버(OnePlus)와 레지스터 뜻을 견준다 |
| 7 | **유지 비용.** seninf·camsv는 본가에 없어 대기열에 오래 남는다 | 계속 | 작게 쓴다. MT8188 시리즈가 병합되면 그쪽으로 옮긴다 |
| 8 | **잠자기.** 카메라가 켜진 채 s2idle에 들어가면 깨어날 때 문제가 생길 수 있다 | 7단계 | 잠들기 전에 스트림, 센서, 도메인을 끈다 |
| 9 | **사생활.** 시험 프레임에 사람과 방이 찍힌다 | 4단계부터 | 7절의 규칙 |

## 7. 사생활과 기록 규칙

- 시험 프레임은 기기와 개발 맥 안에만 둔다. 저장소, 이슈, 외부 서비스에 올리지 않는다.
- 사람이 찍힐 수 있는 캡처는 사용자에게 묻고 한다. 4단계의 첫 캡처는 렌즈를 가리거나 사용자가 고른 곳을 비춘다.
- efuse 값(CSI 보정)과 EEPROM의 모듈 일련 정보는 출력하지 않는다.
- 다 쓴 시험 프레임은 지운다.
- buntOS 쪽에서는 카메라가 켜진 동안 메뉴 막대에 표시를 둔다(macOS의 초록 점). 본체 저장소의 일이다. 본체 설계에는 아직 카메라 사용 중 표시가 없고, AI 제어 표시만 그 모양을 본떴다.

## 8. 다음에 할 일과 물을 것

- 0단계와 1단계는 끝났다(1단계는 시험 커널 #346, 5.1절). 2단계도 전원과 클럭이 됐다(시험 커널 #352, 5.2절). #347에서 화면이 깨진 것은 전원 제어기의 첫 probe가 중간에 미뤄져 화면 쪽 도메인을 껐다 켠 탓이었고, 패치 0247로 고쳤다. 3단계도 됐다(시험 커널 #353, 5.3절): seninf 드라이버로 센서 프레임이 cam-mux까지 들어온다. 다음은 4단계(camsv, larb14, IOMMU)다.
- 1단계 전에 할 것: 레노버 공식 소스 묶음을 받아 SC820CS 설정표가 미러와 같은지 본다.
- 사용자가 정할 것:
  1. 패치 0247을 본가에 낼지. 공급이 늦게 등록되는 보드라면 본가 드라이버도 이미 켠 도메인을 껐다 켠다.
  2. DOVDD를 1.80 V로 맞출지. 1단계는 순정처럼 PMIC 값 1.86 V(VOCAL 6) 그대로 둔다(전압 제한을 주지 않았다).
  3. 레노버 공식 소스 묶음을 사용자가 내려받을지. 레노버 사이트에서 받는다.

## 9. 출처

- 레노버 미러(SC820CS, EEPROM 배치, camsys): https://github.com/lenovo-mt8792-devs/android_kernel_lenovo_mt8792-modules/tree/lineage-24.0/drivers/media/platform/mtkcam
- 레노버 미러(`kd_imgsensor.h`, MT6368 regulator 머리 파일): https://github.com/mt8792/android_kernel_lenovo_mt8792-modules/tree/6.1.138
- OnePlus 공식 MT6897(isp7sp camsys, MT6368 regulator 머리 파일, 전원 도메인 번호): https://github.com/OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6897/tree/oneplus/mt6897_b_16.0.0_oneplus_pad
- OnePlus 공식 MT6789(SC820CS): https://github.com/OnePlusOSS/android_kernel_oneplus_mt6789/tree/oneplus/mt6789_t_13.2.0_onepluspad_go/drivers/misc/mediatek/imgsensor/src/common/v1_1/sc820cs_mipi_raw_bluey
- OPPO 공식 MT6835(SC820CS 칩 ID): https://github.com/oppo-source/android_kernel_oppo_mt6835
- realme 공식 C75(SC820CS 칩 ID): https://github.com/realme-kernel-opensource/realme_C75-AndroidV-kernel-source
- 본가 master의 MediaTek 미디어 드라이버: https://github.com/torvalds/linux/tree/master/drivers/media/platform/mediatek
- MT8365 ISP3.0 시리즈 v6: https://lore.kernel.org/lkml/20240729-add-mtk-isp-3-0-support-v6-0-c374c9e0c672@baylibre.com/T/
- MT8188 ISP7 시리즈 v1: https://lore.kernel.org/linux-arm-kernel/20241009111551.27052-1-Shu-hsiang.Yang@mediatek.com/T/ , v2의 seninf 패치: https://lkml.org/lkml/2025/7/7/40
- libcamera simple 파이프라인: https://gitlab.freedesktop.org/camera/libcamera/-/blob/master/src/libcamera/pipeline/simple/simple.cpp
- Debian의 libcamera 판: https://api.ftp-master.debian.org/madison?package=libcamera
- 관련 문서: [하드웨어 비디오 디코딩](video-decode.md)(IOMMU, SMI, DVFSRC), [포팅 재료 조사](porting-materials.md)
