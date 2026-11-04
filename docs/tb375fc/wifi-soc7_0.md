# TB375FC Wi-Fi·Bluetooth: MediaTek connsys soc7_0

- 작성일: 2026-11-01
- 고친 날: 2026-11-01. 오픈소스 쪽 2차 조사(MCU 명령, LK 적재, Bluetooth, conninfra)를 반영했다
- 고친 날: 2026-11-04. 기기 결과(레거시 명령 길, 2.4 GHz 연결)를 반영했다
- 목적: "MT6897의 내장 Wi-Fi는 칩 ID `0x7961`인 soc7_0 코어에 RF 칩 MT6637을 붙인 구성"이라는 단서(GitHub 이슈)를 기기 펌웨어와 공개 소스로 다시 확인하고, 포팅 길을 정한다

## 1. 결론

- 이 기기의 Wi-Fi와 Bluetooth는 SoC 안의 MediaTek connsys **soc7_0**이다.
  - CONNAC2X 계열이고, 칩 ID는 `0x7961`이다.
  - 여기에 RF 칩(A-die) **MT6637**이 붙는다.
  - 기기 펌웨어, 공개 벤더 소스, GitHub 이슈 셋이 서로 맞는다.
  - soc7_0은 MT6878과 MT6899도 쓴다(5절).
- 칩 ID `0x7961`은 업스트림 mt76의 **MT7921**(PCIe)과 같은 번호다. 벤더 드라이버도 MT7921과 같은 CONNAC2X WFDMA 구조를 쓴다.
  - 다만 이 기기에서는 PCIe가 아니라 SoC 버스(AXI)에 붙는다.
  - 전원, 클럭, A-die는 connsys 기반부(conninfra)가 다룬다. 이 기기에서는 그 대부분을 TF-A SIP 호출로 한다.
- **펌웨어는 LK가 싣는다.** LK가 부팅 때 Wi-Fi·BT 펌웨어를 conninfra 예약 메모리(EMI)에 올리고, 커널 드라이버는 시작 명령만 보낸다(3절). 그래서 리눅스 쪽에는 펌웨어 파일이 필요 없다.
- 업스트림에는 "SoC 안의 Wi-Fi + SPI로 붙은 A-die" 구조의 선례가 있다. `mt76/mt7915/soc.c`(MT7986·MT7981 라우터 SoC)다.
- 그래서 포팅 길은 새 드라이버를 처음부터 쓰는 것이 아니다. mt76의 MT7921 코드에 **SoC(AXI) 버스, conninfra 전원 순서, LK 적재본 기동을 더하는** 것이다(4절).
- MCU 명령의 머리 꼴과 레거시 명령 번호는 MT7921과 같다(6절).
  - 다른 것은 펌웨어 기동, 보정값 전달, DMA 링 배치다.
  - 펌웨어는 uni cmd를 받지 않는다(2026-11-04 기기). 인터페이스, BSS, 스테이션, 키, 채널 시간을 레거시 명령으로 보낸다.
- **2.4 GHz와 5 GHz의 WPA2-PSK 연결이 인터넷까지 된다(2026-11-04 기기).** 2.4 GHz는 내려받기 최고 138 Mbit/s, 올리기 87 Mbit/s다. 5 GHz는 짧은 시험이라 속도를 재지 않았다. 칩 리셋도 첫 시도에 돌아오고, 연결 중이었으면 약 9초에 다시 붙는다(패치 0066·0067, 7절).
- Bluetooth 전송은 UART(BTIF)가 아니라 EMI 안의 공유 메모리 링(BEIF)이다. HCI 위의 WMT 명령은 업스트림 btmtk와 같은 꼴이다.

## 2. 근거

### 2.1 기기 펌웨어 (TB373FU 글로벌판 ZUI 17.0.04.266)

같은 롬 ZIP에서 부분 받기로 두 이미지를 더 받아 읽었다(buntOS `tools/hw/zipremote.py`).

| 이미지 | 크기 | 읽은 것 |
|---|---|---|
| `connsys_wifi.img` | 1,627,168바이트 | MTK 파티션 머리(`connsys_wifi`)와 서명 둘(`cert1`, `cert2`). 문자열 `soc7_0`, 빌드 `t-neptune-main-soc7_0-2104-PONSOT_SOC7_0_E1_ASIC-20250320113502`, 소스 경로 `wifi/base/soc7_0/hal/phy/6637/hal_phy_rom.c`, `wifi/base/soc7_0/hal/cal/6637/hal_cal_flow_rom.c` → A-die는 MT6637이다 |
| `connsys_bt.img` | 768,240바이트 | 같은 꼴. `soc7_0`, 같은 빌드 문자열 |

장치 트리에서 읽은 것:
- 노드: `wifi@18000000`(`mediatek,wifi`), `bt@18000000`(`mediatek,bt`), `gps@18c00000`, `connfem`
- connsys 노드의 compatible은 `mediatek,mt6897-consys-atf`다(전원 순서를 TF-A에 맡긴다)
- PMIC가 주는 connsys 전원: `mt6363-consys`, `mt6368-consys`, `mt6685-consys`
- Wi-Fi 앞단(FEM) 조합 표: `wlan7207h/c`, Qorvo `qm42195`·`qm45197`, Skyworks `sky85732` 등

### 2.2 공개 벤더 소스 (GPL)

Xiaomi `MiCode/MTK_kernel_modules`의 `bsp-rodin-v-oss` 갈래(Android 15). rodin은 MT6899 기기다(`MiCode/MTK_kernel_device_modules`의 `rodin.dts`가 `rodin-mt6899-common.dtsi`를 include한다).
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
  - `_HIF_PCIE`와 `_HIF_AXI` 빌드가 둘 다 있다. AXI 쪽 CR 매핑은 `WF_MCU_BUS_CR_REMAP 0x830c0000 → +0x400000` 등이다. 주석에 "HIF only ioremap 700k range (refer to dts)"라고 적혀 있는데, DT의 reg는 `0x700000`(7 MiB)이다.
  - conninfra와 이어진다(`CFG_SUPPORT_CONNINFRA`, `wake_up_conninfra_off`, sleep protect 폴링).
  - MCU ROM 이미지를 예약 메모리(EMI)에 내려받는 길(`soc7_0_SetupRomEmi`, `request_firmware`)이 있다. 다만 MT6897·MT6878 빌드는 `CFG_WLAN_LK_FWDL_SUPPORT=1`이라 이 길이 꺼진다(3절).
  - 펌웨어 파일 이름은 LK를 쓰지 않는 빌드에서만 쓰인다: `WIFI_RAM_CODE_soc7_0_1_1.bin`, `soc7_0_ram_wmmcu_1_1_hdr.bin`. 따로 된 패치 파일은 없다.
- 같은 갈래의 `connectivity/` 아래에 `conninfra`, `connfem`, `bt`, `gps`, `fmradio`가 있다.

같은 SoC 태블릿의 OEM 공식 저장소에도 있다. 이쪽이 더 좋은 출처다.
- OnePlus Pad(`OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6897`, 갈래 `oneplus/mt6897_b_16.0.0_oneplus_pad`)
- OPPO Pad 3(`oppo-source/android_kernel_modules_and_devicetree_oppo_mt6897`, 갈래 `oppo/mt6897_b_16.0_pad_3`)
- 둘 다 경로는 `vendor/mediatek/kernel_modules/connectivity/wlan/core/gen4m/chips/soc7_0`이다(✔ 갈래의 칩 목록을 직접 확인).
- OPPO Pad 3의 soc7_0 파일, conninfra의 MT6897 파일, BT 플랫폼 헤더는 OnePlus와 같은 파일이다(✔).
- Motorola의 `vendor-mediatek-kernel_modules-connectivity-wlan-core-gen4m` 저장소에도 있다(✔, 파일은 OnePlus와 다르다).

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
- 움직임: mt76의 SoC 변형은 MT7986·MT7981뿐이다. soc7_0, MT6897, conninfra, BEIF를 다룬 패치는 없다(patchwork 제목 검색, `openwrt/mt76` 2026-10-24 기준).

## 3. 펌웨어

- **LK가 싣는다.** MT6897의 gen4m 빌드는 `CFG_WLAN_LK_FWDL_SUPPORT=1`이다. 코드 주석에도 드라이버가 펌웨어를 내려받지 않는다고 적혀 있다.
  - 커널 드라이버는 EMI의 manifest(+0xB00024)만 읽고 시작 명령(INIT 0x42)을 보낸다.
  - 커널에는 파티션을 읽는 코드가 없다.
  - Bluetooth도 같다(`SUPPORT_BIN2IMG=1`). EMI에서 버전만 읽는다. `0xB48024`의 오프셋은 펌웨어가 돌기 전에는 0일 수 있다. LK는 `connsys_bt`를 EMI+0에 싣는다(`0x3c4dc`, 역어셈블).
- **무엇이 실리나:** `connsys_wifi`·`connsys_bt` 파티션의 서명된 이미지일 것이다. LK 소스가 없어 추정이지만 근거는 강하다. 기기에서 EMI의 manifest로 확인한다.
- **예약 메모리**(펌웨어 트리):
  - `consys@18000000`: 12 MiB. RO 6.5 MiB(Wi-Fi 3 MiB, BT 2.875 MiB, GNSS 640 KiB)와 RW 4.5 MiB다. `emi-addr`는 LK가 채운다. LK는 유닛 주소를 보지 않는다. wifi와 겹치지 않게 mainline 트리에서는 `consys@18001000`으로 둔다. `gps`(1 MiB)와 `connscp`(128 KiB)도 LK가 요구한다.
  - `wifi@18000000`: 20 MiB. 호스트 DMA 링과 버퍼를 두는 칸이고, MPU는 LK가 연다. LK의 `mblock_fdt_reserved_append`가 `/reserved-memory/mblock-<n>-<name>`을 `shared-dma-pool`, `no-map`으로 만들고 wifi 경로에 `memory-region`을 쓴다(LK 역어셈블, 기기 확인 전).
- **linux-firmware에는 soc7_0 파일이 없다.** 실기기(ZUXOS 1.5.10.044)의 `/vendor/firmware`에는 BT·MCU RAM 코드(`soc7_0_ram_bt_1_1_hdr.bin`, `soc7_0_ram_mcu_1_1_hdr.bin`과 각 1a·1b판)가 있고, Wi-Fi RAM 코드는 없다(2026-11-01). BT 쪽 이름은 LK를 쓰지 않는 빌드가 찾는 대체 이름과 같다.
- **방침:** Wi-Fi·BT 펌웨어는 뽑아 둘 파일이 아니라 지켜 둘 파티션이다. `connsys_wifi`·`connsys_bt`를 건드리지 않고 LK 적재본을 쓴다. 재배포 문제도 피한다(buntOS ADR-012).

## 4. 포팅 설계

코드는 패치 0055~0069(Wi-Fi)와 0085·0086(Bluetooth)이다. 기기 결과는 7절이다.

| 단계 | 할 일 | 참고 |
|---|---|---|
| W1 conninfra 최소 | connsys 전원 켜기. 기기 트리가 `mediatek,mt6897-consys-atf`라 init, A-die, AFE, PLL, 저전력 단계를 TF-A SIP 호출(`0xC2000534`)로 한다. 커널 몫은 셋이다: PMIC 전원(MT6363 VCN13·VRFIO18, MT6368 VCN33_1·VCN33_2·VANT18), 클럭 방식(MT6685 DCXO), CONN 전원 도메인. AP2CONN·CONN2AP sleep protect는 전원 도메인의 버스 보호가 맡는다(패치 0054). A-die MT6637은 conn_infra RF SPI 마스터와 하드웨어 세마포어로 닿는다 | 벤더 `conninfra/conn_drv/connv2/platform/mt6897`(OnePlus 공식), 업스트림 `mt7915/soc.c`(같은 RF SPI·세마포어 절차) |
| W2 mt76 SoC 버스 | `wifi@18000000` 영역의 MMIO 매핑. AXI 표는 0x7c000000→+0, 0x830c0000→+0x400000이고 창은 7 MiB다. 인터럽트는 889(WFDMA)다. 링 배치는 명령 TX 15, RX 이벤트 2 | 업스트림 `mt7921`/`mt792x` 공통 코드, MT7902의 TXQ 15 분기(DMA 배치 선례), 벤더 `soc7_0.c`의 `_HIF_AXI` 표 |
| W3 펌웨어 기동 | LK가 실은 것을 쓴다. EMI 주소(DT `emi-addr`)를 받아 remap하고, MCU를 기동한 뒤 시작 명령 INIT 0x42를 보낸다. 그 앞에 시간 동기(INIT 0x09)와 PHY action(INIT 0x08: FEM, NVRAM, 보정)이 있다. DMA는 LK가 연 20 MiB 칸 안에서만 한다 | 벤더 `chips/common/fw_dl.c`, `pre_cal.c` |
| W4 MCU 명령 | 머리 꼴과 레거시 명령 번호는 mt7921과 같다(6절). 펌웨어가 uni cmd를 받지 않아(2026-11-04 기기) 인터페이스는 CE 0x11, BSS는 0x12, 스테이션은 0x13·0x14, 키는 0x07, 채널 시간은 0x1c로 보낸다(패치 0064·0065). 보정값은 NVRAM 태그를 CMD 0x48 조각으로 보낸다 | 6절 표, 벤더 `nic_cmd_event.h` |
| W5 FEM | `connfem` 표에서 이 기기의 조합을 골라 RF 앞단 설정 | 벤더 `connectivity/connfem` |
| B1 Bluetooth | 전송은 공유 메모리 링 BEIF(EMI+0xBA0000, 32 KiB)와 IRQ 둘(SW 614, 버스 멈춤 891)이다. 펌웨어는 LK가 싣는다. hci_dev와 WMT-over-HCI(0xFC6F) 머리는 업스트림 btmtk와 같으므로 다시 쓴다. 전송 드라이버는 새로 쓴다. 업스트림의 SoC 내장 BT 선례는 btmtkuart의 MT7622(BTIF 위 serdev)뿐이다 | 벤더 `bt/mt66xx/btif`(`SUPPORT_BEIF=1`), 업스트림 `drivers/bluetooth/btmtk.h` |

- **순서:** W1은 2b의 플랫폼 기반(클럭, 핀, PMIC)이 먼저 있어야 한다. 그래서 Wi-Fi는 설계안의 2d 단계 그대로 둔다.
- **우회:** 그 전에는 USB-C 쪽 업스트림 장치(USB Wi-Fi 동글 mt7921u, 유선 이더넷)로 네트워크를 쓴다. 2b에서 USB 호스트(mtu3·xHCI)가 되면 가능하다.
- **업스트림 제출:**
  - mt76 유지자에게 "MT7921의 SoC 변형"으로 제출하는 것을 목표로 한다.
  - conninfra는 SoC 전원 드라이버로 따로 나눈다. TF-A SIP에 기대는 부분은 업스트림 UFS의 SIP 호출(`mediatek,ufs-rtff-mtcmos`)이 선례다.
- **함께 일하기:** MT6878도 soc7_0이라 그 mainline 포트와 나눌 수 있다.

## 5. soc7_0을 쓰는 SoC

- MT6878, MT6897, MT6899가 gen4m에서 같은 칩 설정(`CONNAC2X2_SOC7_0`)을 쓰고, 셋 다 LK가 펌웨어를 싣는다(✔ MiCode `Kbuild.6878`·`Kbuild.6897`·`Kbuild.6899`, OnePlus `Kbuild.6878`).
- Xiaomi rodin은 MT6899다(✔). 상품명은 확인하지 못했다.
- gen4m의 soc7_0 블록에는 MT6983·MT6895·MT6879·MT6886 분기도 있다. 이 SoC들도 soc7_0을 쓰는지는 분기를 보고 미루어 본 것이다(○).

## 6. MCU 명령 비교 (W4)

OnePlus 공식 gen4m(MT6897 빌드)과 업스트림 mt76(7.3)을 대조했다(✔ 2026-11-01).

| 항목 | soc7_0 | 업스트림 mt7921 | 판단 |
|---|---|---|---|
| 펌웨어 내려받기 | 없다. LK 적재본의 manifest를 읽고 시작 명령 INIT 0x42 하나를 보낸다. 패치 세마포어, `PATCH_START_REQ`, `FW_SCATTER`, `START_REQ`를 쓰지 않는다 | 패치와 RAM 코드를 내려받는다 | 다르다. 0x42는 업스트림에 없다 |
| 그 앞 단계 | FWDL 모드, 시간 동기(INIT 0x09), PHY action(INIT 0x08) | 없다 | 새로 쓴다 |
| 명령 머리 | `struct WIFI_CMD`(HW TXD 32바이트와 머리, 합계 64바이트) | `mt76_connac2_mcu_txd` | 같다(CONNAC2 레거시) |
| uni cmd | soc7_0 빌드에 uni 지원 설정이 없다. BSS·STA·키·ROC·suspend를 레거시 명령으로 보낸다 | 여러 명령을 uni로 보낸다 | 다르다. 기기에서 uni `DEV_INFO`는 멀쩡한 펌웨어에서도 시간 초과이고 바로 뒤의 CE 0x8a에는 답이 온다(2026-11-04). 레거시로 보낸다 |
| 레거시 명령 번호 | CE 0x01·03·05·0a·0f·16·17·1b·1c·1d·33·5d·61·62·8a·a1·b0·c0·c5·ca·cd·d0과 EXT 0x05·08·21이 같은 뜻이다 | 같다 | 같다. CE 0x5c(CLC)와 EXT 0xbd(RF pin)는 없다. EXT 0x46(`MAC_INIT_CTRL`), 0x3e(`PROTECT_CTRL`)도 벤더 표에 없고, 0x4e(`SET_RX_PATH`)와 0x2c는 벤더 헤더에 다른 이름으로 있다(`EFUSE_BUFFER_RD`, `GET_SENSOR_RESULT`). 기기에서 본다. 답을 기다리는 것 가운데 벤더 표에 없거나 이름이 다른 것: `PROTECT_CTRL`은 wlan0이 생기기 전(`mt7921_mac_init`), `MAC_INIT_CTRL`과 `SET_RX_PATH`는 `ip link up`(`__mt7921_start`)에서 간다. 체크리스트 5·7단계에 답이 없을 때의 줄을 적었다. |
| 이벤트 | `WIFI_EVENT`(RXD 24바이트와 머리). 주요 EID가 같다 | `mt76_connac2_mcu_rxd` | 같다. RSSI 알림 번호만 다르다(0xa1, 업스트림 0x96) |
| NIC 능력(0x8a) | 0x17은 P2P, 0x20은 MLR 능력 | 0x17은 WFDMA 재할당, 0x20은 칩 능력 | 뜻이 달라 따로 읽는다 |
| 기능 플래그 | 펌웨어 파일이 없어 trailer의 feature_set을 읽을 수 없다 | `mt792x_get_offload_capability()`가 파일에서 읽는다 | CNM 여부를 다른 길로 정한다 |
| 보정값 | NVRAM 태그 목록을 CMD 0x48로 보낸다. 목록의 끝은 첫 태그(CTRL)의 길이로 정한다(+256). 조각은 태그마다 1,500바이트 하나다(`wlanLoadManufactureData`). PHY action에는 태그 7·8·21·9와 비트 7이 선 태그만 넣는다(`wlanGetEpaElnaFromNvram`, 최대 1 KiB) | efuse 모드(EXT 0x21)뿐 | 새로 썼다 |
| INIT 이벤트 | 페이로드는 RXD 24바이트 + 8바이트 뒤다. 일반 이벤트는 + 12바이트 뒤다(`struct mt76_connac2_mcu_rxd`) | `mt7921_mcu_parse_response`는 PATCH_SEM_CONTROL에서만 이 차이를 다룬다 | 자기 함수를 얹었다 |
| DMA | 명령 TX 링 15. RX는 0·1 데이터, 2 이벤트, 3 tx-free | 명령 링 17, 이벤트 RX 0 | 다르다. MT7902의 TXQ 15 분기가 선례다 |
| 레지스터 | MT7921과 같은 칩 주소 공간이다 | `fixed_map` | AXI 표만 새로 쓴다 |
| 채널 목록(CE 0x0f) | 채널 묶음을 보내는 옛 꼴. 벤더는 `TAG_CAP_SINGLE_SKU`를 보고하지 않는 펌웨어에 이 꼴을 쓴다 | `mt76_connac_mcu_set_channel_domain()`의 새 꼴 | 다르다. 새 꼴은 8채널(76바이트)부터 펌웨어가 주기 이벤트까지 멈추고 답하지 않는다(기기). 나가는 자리에서 옛 꼴로 바꾼다(패치 0062) |
| 스캔 요청(CE 0x03) | 벤더 구조체가 packed가 아니어서 여분 SSID 앞에 2바이트가 있고 주소가 2바이트 뒤다 | `struct mt76_connac_hw_scan_req` 1186바이트 | 다르다. 1186바이트는 끝 이벤트도 없이 무시하고 1192바이트부터 스캔한다(기기). 나가는 자리에서 맞춘다(패치 0063) |
| 연결 명령의 길이 | CE 0x12 116, 0x13 200, 0x07 64, 0x1c 24, 0x28 36바이트. 0x1d 44와 0x16 12는 mt7921과 같다 | uni로 보낸다 | 벤더 구조체와 맞췄다. 스캔에서 본 것처럼 짧으면 답 없이 무시될 수 있다 |
| 채널 허가와 DBDC | CE 0x1c(JOIN)는 `eid 10`으로 허가하고, CE 0x28(DBDC 끄기)은 `eid 78`로 답한다(기기). 벤더도 기본 모드에서는 초기화 때 DBDC를 끈다 | uni ROC | 새로 썼다 |
| 프레임 길 | TX free는 RX3, 관리 프레임은 펌웨어를 거쳐 RX2, 데이터 프레임은 RX0으로 온다(기기, 2.4 GHz) | 위 DMA 줄의 배치 | 자기 링 배치를 쓴다. RX1은 만들지 않았고, 5 GHz 연결에서도 RX1 없이 데이터가 들어왔다 |
| TX A-MPDU | 펌웨어가 묶는다(기기, `tx_stats`의 길이 분포). `ampdu_action`은 `-EOPNOTSUPP`를 돌려준다 | 드라이버가 BA 세션을 명령으로 연다 | 다르다 |

다시 쓰는 꼴:
- mt792x 위에 플랫폼 버스(AXI 표, MT7902식 DMA 배치)를 더한다.
- mcu_init은 `mt792x_load_firmware` 대신 conninfra 전원, MCU 기동, INIT 0x42 순서로 한다.
- NVRAM 명령과 PHY action을 더한다.

## 7. 남은 확인

- **벤더 표에 없는 명령(2026-11-04 기기):** 멀쩡한 펌웨어에서 EXT 0x46·0x4e·0x08은 상태 0으로 답한다. EXT 0x3e·0x05는 상태 칸이 0xfe다(모르는 명령이라는 뜻으로 추정). 명령을 몰아 보내면 명령 링이 먼저 멈추니 그때의 시간 초과는 이 판단에 쓰지 않는다.
- **uni cmd:** 받지 않는다(2026-11-04 기기). uni `DEV_INFO`는 시간 초과이고 CE `GET_NIC_CAPAB`에는 답이 온다. 레거시 명령으로 보낸다(패치 0065).
- **연결(2026-11-04 기기, 2.4 GHz WPA2-PSK, 비밀번호는 사용자가 직접 넣었다):** 인증부터 연결까지 0.24초, `wpa_state` `COMPLETED`까지 3초, DHCP 2초다. 내려받기 38, 107, 138 Mbit/s, 올리기 87 Mbit/s다. 신호는 −51~−41 dBm, 수신 최고 HE-MCS 11 2x2다. 약 10분 유지했고 `tx failed` 0, TCP 재전송 1, `Message … timeout`과 커널 경고가 없다. 연결 중 스캔도 된다.
- **이 길의 범위:** station만 남는다. AP, P2P, monitor, scheduled scan, CSA, wowlan은 uni 명령이라 뺐고, TDLS 상대는 거절한다. 절전은 꺼 둔다.
- **명령 링 멈춤:** DMASHDL을 우회한 채 명령이 몰리면 명령 링이 멈춘다. `firmware running` 뒤에 벤더 값으로 켠 뒤로 첫 적재에서 멈추지 않았다(패치 0061). 원인(이벤트 낼 자리 부족)은 추정이다.
- **칩 리셋(2026-11-04 기기, 패치 0066):** 첫 시도에 돌아오고 리셋 뒤 스캔이 된다. 패치 뒤 12번 모두 그랬다: 연속 리셋, 인터페이스를 내린 채, 스캔 중, 재적재 뒤, 명령 시간 초과로 들어간 리셋, 펌웨어가 답을 끊은 뒤의 리셋. 원인은 재시작 길의 `mt792x_wpdma_reset()`이 DMASHDL 우회 비트(`MT_DMASHDL_SW_CONTROL` bit 28)를 켠 채로 두는 것이다. 그 채로 펌웨어가 뜨면 `STATUS_RD`의 빈 페이지가 0이 되고, DMASHDL을 켜는 순간부터 명령을 읽지 않는다. 처음 적재는 `MT_WFDMA0_RST`의 DMASHDL 리셋이 비트를 우연히 지워서 됐다. 펌웨어를 시작하기 전에 비트를 지운다. 벤더는 AXI에서 이 비트를 쓰지 않는다. 왜 빈 페이지가 0이 되는지는 모른다. **연결 중 리셋 뒤에는 약 9초에 다시 붙고 DHCP와 인터넷까지 돌아온다(패치 0067, 2.4 GHz와 5 GHz에서 3번씩, 그중 1번씩은 HTTP 요청 중).** 펌웨어는 없는 스테이션을 번호로 지우라는 CE 0x14를 받으면 답을 멈춘다. 리셋으로 스테이션이 사라졌는데 mac80211이 끊으면서 그 명령을 보내던 것이 원인이다. 왜 멈추는지는 모른다. 펌웨어가 스테이션을 갖고 있는지 기억해 없으면 보내지 않는다. 벤더는 끊을 때 번호가 아니라 BSS 단위로 지운다. 못 본 것: 인증과 연결 사이의 리셋, 큰 트래픽 중 리셋, 펌웨어 assert로 들어가는 리셋. 리셋이 끝내 실패하면 스캔이 끝나지 않고 모듈을 다시 올려야 하는 것은 그대로다.
- **5 GHz:** 스캔의 5 GHz 프레임은 RXD 밴드 칸이 1이라(DBDC를 꺼도 1) `mt7921_mac_fill_rx`가 버린다. 그 칸을 지운다. 채널은 RXD에 따로 있다. **5 GHz AP 연결이 인터넷까지 된다(2026-11-04 11:17, 5745 MHz, WPA2-PSK, 비밀번호는 사용자가 직접 넣었다).** 3초 만에 `COMPLETED`이고, DHCP로 주소를 받고 `1.1.1.1:443` TCP와 HTTP 응답까지 된다. 신호는 −51 dBm이고, 표시 속도는 수신 720.6 Mbit/s(80 MHz, HE-MCS 7, 2x2), 송신 286.7 Mbit/s(HE-MCS 11, 2x2)다. `tx failed` 0, `rx drop misc` 0이고, RX1 링 없이 데이터가 들어왔다. 규제는 그대로 `00`이다. 프레임 수십 개의 짧은 시험이라 처리량, 오래 유지, DFS 채널은 보지 않았다.
- **RX 헤더 변환:** 전체 스위치 `MT_MDP_DCR0` bit 19를 끈다. `mt7921_mac_init` 전에도 켜져 있었다(리셋이 실패한 상태에서 읽었다). mac80211이 인증 전의 변환 프레임을 버리기 때문이고, 벤더는 변환된 프레임을 받아 처리한다. 켠 채로 시험하지 않아서 꺼야만 되는지는 모른다.
- **RX 순서 정리:** 코드는 펌웨어의 BA 이벤트(0x0a, 0x0b)를 따르는데 이벤트가 오는지 보지 못했다. 첫 내려받기 구간에 `TCPOFOQueue`가 1196이고 끝까지의 `rx drop misc`가 604다. 2.4 GHz 혼잡인지 순서 정리가 안 돌아서인지 구분하지 못했다.
- **규제 정보:** `cfg80211: loaded regulatory.db is malformed or signature is missing/invalid`로 거부되어 규제가 `00`이고 `iw reg set`이 먹지 않는다. 그래서 국가 코드가 든 CE 0x0f는 돌지 않았다. Debian이 고르는 `regulatory.db-debian`의 키가 커널에 없는 것으로 추정한다. 이미지에서 `-upstream`을 고르거나 커널에 키를 넣는 길이 있고, 기기에서 시험하지 않았다.
- **연결 감시(2026-11-05, 패치 0068):** 공용 코드가 `CONNECTION_MONITOR`를 세워 mac80211의 30초 유휴 확인이 꺼져 있었고, 이 길의 `bss_info_changed`는 펌웨어에 비콘 감시를 시키지 않아 아무도 연결을 보지 않았다. SoC 변형에서 깃발을 지워 mac80211이 묻는다. `IEEE80211_VIF_BEACON_FILTER`는 그대로라 mac80211이 비콘을 세지는 않는다. AP가 실제로 사라질 때는 아직 보지 않았다.
- **데이터 경로 멈춤(2026-11-05, 패치 0069):** 차지 펌프 시험 2번 모두 4분 안에 데이터가 조용히 끊겼다. 커널, `wpa_supplicant`, NetworkManager에 기록이 없고 `tx failed`도 0이다. 다시 연결과 rfkill로는 돌아오지 않고 칩 리셋만 듣는다. 멈춘 뒤에는 2.4 GHz 인증도 실패한다. 원인은 모른다. 그때 프레임이 어디 있었는지 기록하지 못해서 송신이 멈춘 것인지 유니캐스트 수신이 멈춘 것인지도 가리지 못했다. 고갈은 아니다: DMA 창이 찼다는 줄이 없고, swiotlb 최고 사용량은 32768칸 중 433칸이다. 프레임이 기다리는데 10초 동안 TX free가 없으면 칩을 다시 시작한다(1분에 한 번까지). **실제 멈춤(2026-11-05, 벽 충전기 145분):** 열 번 났고 여덟 번은 펌프가 꺼져 있었다. 한 번은 입력이 500 mA뿐일 때, 한 번은 충전이 끝나 배터리 0 A일 때다. 전류 값과 전류를 올리는 순간도 조건이 아니다. 감시가 열 번 모두 잡았고 3.8~14.9초 뒤 다시 붙었다. 칩 리셋이 PPS 충전에 준 영향은 보지 못했다. 열 번 모두 토큰 수와 링에 남은 수가 같다(`queued`는 칩 DMA가 디스크립터를 가져가고 정리돼야 줄고, 토큰은 TX free로 줄어든다). 칩이 프레임을 하나도 가져가지 않았거나 그 완료가 호스트에 오지 않은 것이고, 공중으로 나간 뒤의 실패로는 설명되지 않는다. 어느 쪽인지 가르려면 멈춘 순간의 링 인덱스와 DMA 레지스터가 필요하다.
- **멈춤 흉내:** DMA 스케줄러의 데이터 묶음(0~2) 최대 한도를 1페이지로 쓰면(`0x7c026020`~`0x7c026028`에 `0x00010000`) 데이터 프레임만 링에 쌓이고 명령과 수신은 그대로다. 칩 리셋이 값을 되돌린다. 한도 0과 `MT_ARB_SCR`의 TX 끄기는 듣지 않았다. 이 흉내에서 감시는 10.1초에 알아채고 18초 만에 다시 붙으며 NetworkManager는 `connected`를 벗어나지 않는다. 감시 없이는 mac80211이 33초에 끊고 NetworkManager가 49초에 상태를 바꾼다. 실제 펌프 멈춤에서는 아직 보지 않았다.
- **CE 0x12의 `auth_mode`, `enc_status`:** WPA2-PSK, CCMP로 적어 동작했지만 펌웨어가 어디에 쓰는지 모른다. TKIP, WPA3 망도 같은 값을 적는다.
- **PCIe, USB 칩:** 공용 코드의 후크(패치 0064)는 그 칩에서 NULL이다. 패치 전후 기계어를 맞대어 함수 571개 가운데 543개가 같고 28개는 후크 자리, 패딩, 위치만 다른 것을 봤다. 실제 칩에서 돌리지는 않았다.
- **LK가 트리에 쓰는 값:** LK는 노드를 경로로 찾고 status를 보지 않는다(역어셈블). mainline 트리에 실제로 값을 쓰는지는 기기에서 본다. 차례는 [체크리스트](bringup-checklist.md) 9.1절이다.
- **LK의 펌웨어 적재:** 기기에서 EMI+0xB00024의 manifest로 확인한다.
- **직접 레지스터 길:** `-atf` 대신 일반 세계에서 레지스터를 직접 쓸 수 있는지(DAPC). 모르면 SIP를 쓴다.
- **NVRAM과 BT 주소:** gen4m 코드는 `/data/nvram/APCFG/APRDEB/WIFI`를 읽는다. 파티션은 nvdata다. BT 주소의 출처도 기기에서 확인한다(안드로이드 속성 `persist.odm.bt.address`가 있다).

## 출처

- [OnePlus Pad 갈래의 gen4m/chips/soc7_0](https://github.com/OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6897/tree/oneplus/mt6897_b_16.0.0_oneplus_pad/vendor/mediatek/kernel_modules/connectivity/wlan/core/gen4m/chips/soc7_0)
- [OnePlus Pad 갈래의 connectivity](https://github.com/OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6897/tree/oneplus/mt6897_b_16.0.0_oneplus_pad/vendor/mediatek/kernel_modules/connectivity): `wlan/core/gen4m/Kbuild.main`, `chips/common/fw_dl.c`, `conninfra/conn_drv/connv2/platform/mt6897`, `bt/mt66xx/btif`
- [OPPO Pad 3 갈래](https://github.com/oppo-source/android_kernel_modules_and_devicetree_oppo_mt6897/tree/oppo/mt6897_b_16.0_pad_3)
- [Motorola gen4m chips/soc7_0](https://github.com/MotorolaMobilityLLC/vendor-mediatek-kernel_modules-connectivity-wlan-core-gen4m/tree/android-16-release-w1wn36.18-114/chips/soc7_0)
- [MiCode/MTK_kernel_modules `bsp-rodin-v-oss` 갈래: gen4m/chips/soc7_0](https://github.com/MiCode/MTK_kernel_modules/tree/bsp-rodin-v-oss/connectivity/wlan/core/gen4m/chips/soc7_0), [MiCode/MTK_kernel_device_modules `bsp-rodin-v-oss`](https://github.com/MiCode/MTK_kernel_device_modules/tree/bsp-rodin-v-oss)
- [MiCode/MTK_kernel_modules 이슈 #42](https://github.com/MiCode/MTK_kernel_modules/issues/42)
- [Linux mt76 mt7921/pci.c](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/mediatek/mt76/mt7921/pci.c)
- [Linux mt76 mt7915/soc.c](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/mediatek/mt76/mt7915/soc.c)
- [openwrt/mt76](https://github.com/openwrt/mt76), [linux-firmware mediatek](https://gitlab.com/kernel-firmware/linux-firmware/-/tree/main/mediatek)
- 기기 펌웨어: Lolinet 미러 `TB373FU_ROW_OPEN_USER_M21.814_V_ZUI_17.0.04.266_ST_251120.zip`의 `connsys_wifi.img`, `connsys_bt.img`
