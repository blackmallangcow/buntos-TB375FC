# TB375FC Wi-Fi·Bluetooth: MediaTek connsys soc7_0

- 작성일: 2026-11-01
- 목적: "MT6897의 내장 Wi-Fi는 칩 ID `0x7961`인 soc7_0 코어에 RF 칩 MT6637을 붙인 구성"이라는 단서(GitHub 이슈)를 기기 펌웨어와 공개 소스로 다시 확인하고, 포팅 길을 정한다

## 1. 결론

- 이 기기의 Wi-Fi와 Bluetooth는 SoC 안의 MediaTek connsys **soc7_0**이다.
  - CONNAC2X 계열이고, 칩 ID는 `0x7961`이다.
  - 여기에 RF 칩(A-die) **MT6637**이 붙는다.
  - 기기 펌웨어, 공개 벤더 소스, GitHub 이슈 셋이 서로 맞는다.
- 칩 ID `0x7961`은 업스트림 mt76의 **MT7921**(PCIe)과 같은 번호다. 벤더 드라이버도 MT7921과 같은 CONNAC2X WFDMA 구조를 쓴다.
  - 다만 이 기기에서는 PCIe가 아니라 SoC 버스(AXI)에 붙는다.
  - 전원, 클럭, A-die는 connsys 기반부(conninfra)가 다룬다.
- 업스트림에는 "SoC 안의 Wi-Fi + SPI로 붙은 A-die" 구조의 선례가 있다. `mt76/mt7915/soc.c`(MT7986·MT7981 라우터 SoC)다.
- 그래서 포팅 길은 새 드라이버를 처음부터 쓰는 것이 아니다. mt76의 MT7921 코드에 **SoC(AXI) 버스와 conninfra 전원 순서를 더하는** 것이다(4절).
- 위험은 "방식 미확인"에서 "구조를 아는 큰 일"로 줄었다. 다만 펌웨어 명령이 업스트림 MT7921과 같은지는 아직 모른다.

## 2. 근거

### 2.1 기기 펌웨어 (TB373FU 글로벌판 ZUI 17.0.04.266)

같은 롬 ZIP에서 부분 받기로 두 이미지를 더 받아 읽었다(buntOS `tools/hw/zipremote.py`).

| 이미지 | 크기 | 읽은 것 |
|---|---|---|
| `connsys_wifi.img` | 1,627,168바이트 | MTK 파티션 머리(`connsys_wifi`)와 서명 둘(`cert1`, `cert2`). 문자열 `soc7_0`, 빌드 `t-neptune-main-soc7_0-2104-PONSOT_SOC7_0_E1_ASIC-20250320113502`, 소스 경로 `wifi/base/soc7_0/hal/phy/6637/hal_phy_rom.c`, `wifi/base/soc7_0/hal/cal/6637/hal_cal_flow_rom.c` → A-die는 MT6637이다 |
| `connsys_bt.img` | 768,240바이트 | 같은 꼴. `soc7_0`, 같은 빌드 문자열 |

장치 트리에서 읽은 것:
- 노드: `wifi@18000000`(`mediatek,wifi`), `bt@18000000`(`mediatek,bt`), `gps@18c00000`, `connfem`
- PMIC가 주는 connsys 전원: `mt6363-consys`, `mt6368-consys`, `mt6685-consys`
- Wi-Fi 앞단(FEM) 조합 표: `wlan7207h/c`, Qorvo `qm42195`·`qm45197`, Skyworks `sky85732` 등

### 2.2 공개 벤더 소스 (GPL)

Xiaomi `MiCode/MTK_kernel_modules`의 `bsp-rodin-v-oss` 갈래(Android 15):
- `connectivity/wlan/core/gen4m/chips/soc7_0/`에 세 파일이 있다.
  - `soc7_0.c`(2,845줄, 91,724바이트)
  - `dbg_soc7_0.c`
  - `hal_dmashdl_soc7_0.c`
- `connectivity/wlan/core/gen4m/include/chips/soc7_0.h`에 칩 정보가 있다.
  - `#define SOC7_0_CHIP_ID 0x7961`
  - `SOC7_0_PATCH_START_ADDR 0x00900000`
  - `CONNAC2X_TOP_HCR 0x88000000`
  - `SOC7_0_REMAP_BASE_ADDR 0x7c500000`
- `soc7_0.c`의 구조:
  - CONNAC2X WFDMA 함수(`soc7_0asicConnac2x…`)를 쓴다.
  - `_HIF_PCIE`와 `_HIF_AXI` 빌드가 둘 다 있다. AXI 쪽 CR 매핑은 `WF_MCU_BUS_CR_REMAP 0x830c0000 → +0x400000` 등이고, 주석에 "HIF only ioremap 700k range (refer to dts)"라고 적혀 있다.
  - conninfra와 이어진다(`CFG_SUPPORT_CONNINFRA`, `wake_up_conninfra_off`, sleep protect 폴링).
  - MCU ROM 이미지를 예약 메모리(EMI)에 내려받는다(`soc7_0_SetupRomEmi`, `request_firmware` 길).
  - 펌웨어 파일 이름은 `WIFI_RAM_CODE_soc7_0_<버전>_1.bin`, `soc7_0_ram_wmmcu_<버전>_<x>_hdr.bin`이다.
- 같은 갈래의 `connectivity/` 아래에 `conninfra`, `connfem`, `bt`, `gps`, `fmradio`가 있다.

같은 SoC 태블릿의 OEM 공식 저장소에도 있다. 이쪽이 더 좋은 출처다.
- OnePlus Pad(`OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6897`, 갈래 `oneplus/mt6897_b_16.0.0_oneplus_pad`)
- OPPO Pad 3(`oppo-source/android_kernel_modules_and_devicetree_oppo_mt6897`, 갈래 `oppo/mt6897_b_16.0_pad_3`)
- 둘 다 경로는 `vendor/mediatek/kernel_modules/connectivity/wlan/core/gen4m/chips/soc7_0`이다(✔ 갈래의 칩 목록을 직접 확인).
- Motorola의 `vendor-mediatek-kernel_modules-connectivity-wlan-core-gen4m` 저장소에도 있다고 보고되었다(○).

빠진 곳:
- Xiaomi `bsp-duchamp-u-oss`(Dimensity 8300 기기)의 `gen4-mt79xx/chips`는 `soc5_0`까지다. `soc7_0`이 없다.
- OPPO Reno13의 Android 15 갈래(`oppo/mt6897_v_15.0.0_reno13`, `…_reno13_pro`)에는 gen4m 자체가 없다. 이슈 #42가 말한 OPPO 공개분은 이 갈래다.

### 2.3 GitHub 이슈 MiCode/MTK_kernel_modules#42

Dimensity 8300 기기의 속성으로 적힌 값이다. 2.1의 기기 펌웨어와 맞는다.
- `ro.hardware.wlan.chip: mt6637`
- `vendor.connsys.wifi.adie.chipid: 0x6637`
- `ro.vendor.wlan.gen: gen4m_6897`
- 펌웨어 `WIFI_RAM_CODE_soc7_0-…`
- 레지스터 기준 `0x830c0000`

### 2.4 업스트림 Linux

- `drivers/net/wireless/mediatek/mt76/mt7921/pci.c`: `PCI_DEVICE(PCI_VENDOR_ID_MEDIATEK, 0x7961)`이다. 칩 ID 0x7961을 MT7921로 받고, MT7920 구분(`MT_HW_BOUND`)도 같은 칩 ID에서 한다.
- `drivers/net/wireless/mediatek/mt76/mt7915/soc.c`: MT7986·MT7981의 SoC Wi-Fi(WMAC)다. A-die 레지스터를 SPI로 읽고 쓰고(`mt76_wmac_spi_read/write`, `mt7986_wmac_adie_efuse_read`), SoC 전원 순서를 드라이버 안에 둔다. 이 기기 구조의 선례다.

## 3. 펌웨어

리눅스 드라이버는 펌웨어를 파일로 읽는다(`request_firmware`). 필요한 것은 세 가지다.
- MCU ROM EMI 이미지(`soc7_0_ram_wmmcu_…_hdr.bin`)
- 패치(시작 주소 `0x00900000`)
- RAM 코드(`WIFI_RAM_CODE_soc7_0_…_1.bin`)

받을 곳과 할 일:
- 펌웨어 파일은 기기의 vendor 파티션(`/vendor/firmware`, super.img 안)에 있을 것이다.
- `connsys_wifi` 파티션과의 관계(같은 이미지인지, 부트로더가 따로 올리는지)는 기기에서 확인한다.
- 재배포할 수 있는지 모른다. 그래서 linux-firmware처럼 이미지에 넣지 않고, 설치할 때 기기에서 뽑는 길을 먼저 둔다(buntOS ADR-012).

## 4. 포팅 설계

| 단계 | 할 일 | 참고 |
|---|---|---|
| W1 conninfra 최소 | connsys 전원 켜기: PMIC의 consys 전원(MT6363·MT6368·MT6685), conn_infra 클럭과 sleep protect, 버스 깨우기, A-die MT6637 전원과 SPI | 벤더 `connectivity/conninfra`(rodin), 업스트림 `mt7915/soc.c`의 SoC 전원 순서 |
| W2 mt76 SoC 버스 | DT의 `wifi@18000000` 영역 MMIO 매핑(AXI CR 표), 인터럽트, WFDMA 링 | 업스트림 `mt7921`/`mt792x` 공통 코드, 벤더 `soc7_0.c`의 `_HIF_AXI` 표 |
| W3 펌웨어 내려받기 | ROM EMI 이미지를 예약 메모리에, 패치와 RAM 코드를 MCU에 | 벤더 `soc7_0_SetupRomEmi`, mt7921 MCU 내려받기 |
| W4 MCU 명령 | 업스트림 mt7921의 CONNAC2 MCU 명령으로 동작하는지 확인. 다르면 soc7_0 전용 경로를 둔다 | 벤더 gen4m의 명령 정의 |
| W5 FEM | `connfem` 표에서 이 기기의 조합을 골라 RF 앞단 설정 | 벤더 `connectivity/connfem` |
| B1 Bluetooth | connsys BT 전송(conninfra 위)과 펌웨어 내려받기. HCI 위는 업스트림 btmtk를 다시 쓸 수 있는지 본다 | 벤더 `connectivity/bt` |

- **순서:** W1은 2b의 플랫폼 기반(클럭, 전원 도메인, PMIC)이 먼저 있어야 한다. 그래서 Wi-Fi는 설계안의 2d 단계 그대로 둔다.
- **우회:** 그 전에는 USB-C 쪽 업스트림 장치(USB Wi-Fi 동글 mt7921u, 유선 이더넷)로 네트워크를 쓴다. 2b에서 USB 호스트(mtu3·xHCI)가 되면 가능하다.
- **업스트림 제출:** mt76 유지자에게 "MT7921의 SoC 변형"으로 제출하는 것을 목표로 한다. conninfra는 SoC 전원 드라이버로 따로 나눈다.

## 5. 남은 확인

- soc7_0 펌웨어가 업스트림 MT7921의 MCU 명령과 같은지(W4)
- vendor 파티션의 펌웨어 파일 이름과 버전, `connsys_wifi` 파티션의 쓰임(super.img 또는 기기에서)
- rodin 갈래 기기의 SoC. soc7_0이 같으면 드라이버는 같다
- Bluetooth 전송 방식(BTIF 또는 공유 메모리)과 업스트림 btmtk를 다시 쓸 수 있는지

## 출처

- [OnePlus Pad 갈래의 gen4m/chips/soc7_0](https://github.com/OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6897/tree/oneplus/mt6897_b_16.0.0_oneplus_pad/vendor/mediatek/kernel_modules/connectivity/wlan/core/gen4m/chips/soc7_0)
- [OPPO Pad 3 갈래](https://github.com/oppo-source/android_kernel_modules_and_devicetree_oppo_mt6897/tree/oppo/mt6897_b_16.0_pad_3)
- [MiCode/MTK_kernel_modules `bsp-rodin-v-oss` 갈래: gen4m/chips/soc7_0](https://github.com/MiCode/MTK_kernel_modules/tree/bsp-rodin-v-oss/connectivity/wlan/core/gen4m/chips/soc7_0)
- [MiCode/MTK_kernel_modules 이슈 #42](https://github.com/MiCode/MTK_kernel_modules/issues/42)
- [Linux mt76 mt7921/pci.c](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/mediatek/mt76/mt7921/pci.c)
- [Linux mt76 mt7915/soc.c](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/mediatek/mt76/mt7915/soc.c)
- 기기 펌웨어: Lolinet 미러 `TB373FU_ROW_OPEN_USER_M21.814_V_ZUI_17.0.04.266_ST_251120.zip`의 `connsys_wifi.img`, `connsys_bt.img`
