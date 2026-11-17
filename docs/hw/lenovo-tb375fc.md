# Lenovo TB375FC 하드웨어 실측 (Phase 0)

- 상태: 진행 중. 펌웨어에서 읽은 것, TB375FC 중국판과 글로벌판이 같은 하드웨어인지 견준 것, 사용자의 기기(ZUXOS 1.5.10.044)에서 ADB로 읽은 것(7절)까지 적었다(2026-11-01). root가 필요한 항목과 12.1절 EL2 판정이 남았다
- 대상: Lenovo Xiaoxin Pad Pro 12.7 (2025, TB375FC). 글로벌판은 Lenovo Idea Tab Pro(TB373FU)이고, Moto Pad 60 Pro(XT2571-1)도 같은 기기로 알려져 있다

## 1. 출처와 방법

**펌웨어.** 글로벌판 순정 펌웨어 `TB373FU_ROW_OPEN_USER_M21.814_V_ZUI_17.0.04.266_ST_251120.zip`(6,114,349,108바이트, ZUI 17, 2025-11-20 빌드)이다. 레노버 공식 파일 이름이고, 커뮤니티 미러 [Lolinet](https://mirrors.lolinet.com/firmware/lenowow/2024/Idea_Tab_Pro_2024/)에서 받았다.
- 레노버 공식 도구(Rescue and Smart Assistant)는 기기 일련번호가 있어야 받을 수 있다.
- 커뮤니티는 TB375FC에 이 글로벌 펌웨어를 그대로 올려 쓴다(XDA "Flashing CN/Global rom for TB375FC, TB373FU, Moto Pad 60 Pro"). 그래서 이 문서는 TB373FU 펌웨어 기준이다.
- TB375FC 중국판 펌웨어와 장치 트리를 견줘 같은 하드웨어임을 확인했다(6절).
- 펌웨어는 레노버 저작물이므로 저장소에 넣지 않는다. 여기에는 읽어 낸 사실만 적는다(ADR-012).

**받은 것.** 미러가 부분 받기(`Accept-Ranges: bytes`)를 지원해 ZIP 목차(104개 항목)를 읽고 7개 항목만 받았다. 압축 상태로 40,226,587바이트이고 CRC32가 모두 맞았다(`tools/hw/zipremote.py`).
- `boot.img`: GKI 커널
- `init_boot.img`: 일반 램디스크
- `vendor_boot.img`: 벤더 램디스크, SoC 장치 트리
- `dtbo.img`: 보드 장치 트리 오버레이
- `lk.img`: 부트로더
- `MT6897_Android_scatter.x`: 파티션 표. 레노버가 암호화해 읽지 못했다
- `version.t`: 버전 정보. 이것도 암호화되어 있다

`super.img`(시스템·벤더 파티션, 압축 5.97GB)는 받지 않았다.

**푸는 법.**

```bash
U=https://mirrors.lolinet.com/firmware/lenowow/2024/Idea_Tab_Pro_2024/TB373FU/TB373FU_ROW_OPEN_USER_M21.814_V_ZUI_17.0.04.266_ST_251120.zip
P=TB373FU_ROW_OPEN_USER_M21.814_V_ZUI_17.0.04.266_ST_251120/image
python3 -I tools/hw/zipremote.py list "$U"
python3 -I tools/hw/zipremote.py get "$U" "$P/vendor_boot.img" raw/vendor_boot.img   # boot, init_boot, dtbo, lk도 같다
python3 -I tools/hw/bootimg.py raw/boot.img raw/vendor_boot.img raw/init_boot.img raw/dtbo.img out
lz4 -d out/boot.kernel out/Image                        # 커널과 램디스크는 LZ4 legacy
fdtoverlay -i out/vendor_boot-0.dtb -o out/merged.dtb out/dtbo-0.dtb   # 보드 오버레이를 입힌다
dtc -I dtb -O dts -o out/merged.dts out/merged.dtb
python3 -I tools/hw/dts_enabled.py out/merged.dts       # 켜진 노드의 compatible
```

받은 파일과 푼 결과는 믿을 수 없는 자료로 다룬다. 새 디렉터리에 두고, 파이썬은 `-I`로 돌리고, 아무것도 실행하지 않는다.

## 2. 부트 구성

| 항목 | 값 |
|---|---|
| boot.img | 헤더 v4. 커널(LZ4 16,543,287바이트, 풀면 35,432,960바이트)만 있고 램디스크는 없다(GKI) |
| init_boot.img | 헤더 v4, 일반 램디스크(LZ4) |
| vendor_boot.img | 헤더 v4, 쪽 크기 4096. 벤더 램디스크 둘(일반 25.9MB, recovery 1.0MB, LZ4), 명령줄 `bootopt=64S3,32N2,64N2` |
| SoC 장치 트리 | vendor_boot의 DTB 영역. FDT를 이어 붙인 꼴이 아니라 DT 표(dtbo와 같은 꼴, magic `0xd7b7ab1e`)이고 항목 하나(411,812바이트) |
| 보드 장치 트리 | dtbo.img의 DT 표, 오버레이 하나(191,097바이트). 기본 트리에 입히면 compatible이 있는 켜진 노드가 1032개다 |
| 부트로더(LK) | 화면 패널을 읽어 장치 트리의 패널 노드 셋 가운데 하나를 고르고 고친다(문자열 `lcm:%s found with spr`). AVB 롤백 검사 문자열이 있다 |
| 커널 명령줄(장치 트리) | `console=tty0 root=/dev/ram … androidboot.hardware=mt6897 … firmware_class.path=/vendor/firmware` |

2a 단계에서 LK에 mainline 커널을 넘길 꼴은 이것을 따른다. 커널은 boot v4, 장치 트리는 vendor_boot의 DT 표에 넣는다.

## 3. 커널

- `Linux version 6.1.138-android14-11-g151cf2b6bfbe-ab13719792`. 구글이 빌드한 GKI이고(Android CI 빌드 ab13719792, clang 17.0.2, 2025-06-28), 벤더 소스로 빌드한 커널이 아니다.
- 커널 설정이 들어 있다(`CONFIG_IKCONFIG=y`, 205,470바이트). GKI 공통 설정이라 `CONFIG_KVM=y`, `CONFIG_MTK_GZVM=m`(GenieZone VM), `CONFIG_GUNYAH=y`가 모두 켜져 있다. 그래서 이 설정으로는 기기가 무엇을 쓰는지 알 수 없다.
- 벤더 드라이버는 모두 모듈이다. vendor_boot 램디스크에 첫 단계 모듈 223개가 있고, 나머지는 super 안의 vendor_dlkm에 있다(받지 않았다).
- 레노버의 커널 소스 공개는 찾지 못했다. 다만 커널 자체는 공개된 GKI다. 비공개인 것은 벤더 모듈의 소스다.

## 4. 하드웨어

보드 오버레이를 입힌 장치 트리에서 켜진 노드와 첫 단계 벤더 모듈로 정리했다. "업스트림" 열은 mainline 포팅 범위를 가늠한 것이고, 각 bring-up 단계에서 다시 확인한다.

| 블록 | 부품 | 근거 | 업스트림 |
|---|---|---|---|
| SoC | MediaTek MT6897 (Dimensity 8300) | `model = "MT6897"`, `compatible = "mediatek,MT6897"` | SoC 지원 없음(4.2.1-2) |
| 화면 | DSI0(`dsi@1400d000`) 비디오 모드 패널 셋 가운데 하나. BOE NT36532, Tianma NT36532, Tianma "paper"(무광) NT36532. LK가 GPIO184·183 삼상(0이면 BOE)과 DCS 0xDA(0이면 Tianma, 5면 Tianma 무광)로 고른다. 실제 크기 273.615×171.009mm. 편향 SM5109(i2c13 0x3e, GPIO187·186), 백라이트 SGM37604A 둘(i2c13·i2c3 0x36, I2C 코드 x PWM), 밝기는 SoC `disp_pwm0`(GPIO131, 104 MHz, 39.38 µs). 패널 드라이버에 DCS 0x51 콜백이 있지만 듣지 않는다(2026-11-04 기기) | 노드 `panel1@0`~`panel3@2`, 모듈 `panel-nt36532-dsi-vdo-{boe,tianma,tianma-paper}.ko` | 패널 드라이버 셋을 새로 써야 한다. |
| 터치 | Novatek NT36532. 화면 구동 칩에 터치를 넣은 TDDI다. SPI2(`spi2@11012000`) CS0, 9.6MHz, 리셋 GPIO 60, 인터럽트 GPIO 9, 감지 채널 18×36, 펜 지원(`novatek,pen-support`) | `compatible = "novatek,NVT-ts-spi"`, 모듈 `nt36532.ko` | **동시 10손가락.** 드라이버가 `max_touch_num`에 10을 넣고 `input_mt_init_slots(dev, 10, 0)`을 부른다(역어셈블). 업스트림 `novatek-nvt-ts`는 I2C 칩용이라 SPI로 붙은 이 칩에는 바로 쓸 수 없다(확인 필요) |
| 외부 화면 | DP 출력(`dp-tx@11e10000`). USB-C DP Alt 모드, Parade PS5170 리드라이버 | `mediatek,mt6897-dp_tx`, `parade,ps5170` | |
| GPU | Arm Mali-Valhall(`mali@13000000`). 전원과 주파수는 GPU 내장 컨트롤러 gpueb(펌웨어 `gpueb.img`)와 gpufreq가 다룬다 | `mediatek,mali`, `arm,mali-valhall` | Panthor + Mesa가 GPU를 지원한다. MT6897 쪽 전원·주파수(gpueb) 통합이 포팅 범위다(4.2.1-3) |
| 저장장치 | UFS(`ufshci@112b0000`), MediaTek UFS PHY | `mediatek,mt8183-ufshci`, 모듈 `ufs-mediatek-mod.ko`, `phy-mtk-ufs.ko` | 업스트림 `ufs-mediatek`이 받는 compatible이다 |
| USB | mtu3 + xHCI(`usb0@11201000`). USB-C는 MT6375의 TCPC, Type-C 먹스, USB·DP 고르기 | `mediatek,mtu3`, `mediatek,mtk-xhci`, `mediatek,mt6375-tcpc` | mtu3·xhci-mtk는 업스트림에 있다. MT6375 TCPC는 확인 필요 |
| USB VBUS 경로 | MT6375 부스트 → 차지 펌프 입력 FET(ACDRV1) → 커넥터. 순정은 부스트 앞에서 FET를 닫는다. 커넥터 과열 보호 GPIO179(`vbus_mos_gpio`, 평소 low, 충전 중 75도에서 high) | `mt6375,otg-vbus`, `nuvolta,nu2115`, `unisemi,upm6720` | 스톡 모듈과 제3자 미러로 확인. 회로도는 없다 |
| PMIC | MT6363(주, SPMI), MT6368(보조: USB VUSB, GPU 코어 VBUCK2, 진동 VIBR, 지문 VFP, 오디오 코덱·이어폰 감지), MT6319 ×3(SPMI 벅), MT6685(RTC·클럭 버퍼), MT6375(충전기·TCPC·ADC, I2C 0x34) | `mediatek,mt6363` 등 | 확인 필요(2b) |
| 충전 | MT6375 충전기, 차지 펌프 Nuvolta NU2115 ×2와 Southchip/Unisemi UPM6720 ×2(고속 충전, PD·PE) | `nuvolta,nu2115`, `southchip,upm6720` | |
| 배터리 | 게이지 Mitsumi MM8013(I2C 0x55) | `mediatek,mm8013`, 모듈 `mm8013a_gauge.ko` | |
| 오디오 | MT6368 코덱, Awinic AW88394 스마트 앰프 ×4(스피커 넷, `i2c6` 0x34~0x37, 칩 ID 0x2049), FSA4480(USB-C 이어폰 스위치) | `awinic,aw883xx`, `fsa,fsa4480-i2c` | 실측 결과는 7장 "소리" |
| 무선 | MediaTek connsys 통합 Wi-Fi·Bluetooth·GPS(`wifi@18000000`, `bt@18000000`, `gps@18c00000`). 펌웨어 `connsys_wifi.img`, `connsys_bt.img`, `connsys_gnss.img` | `mediatek,wifi`, `mediatek,bt`, `mediatek,mt6897-gps` | 외장 칩이 아니라 SoC 통합형이다. 휴대기기용 connsys는 업스트림 드라이버가 없는 것으로 안다(4.2.1-4, 2d에서 확인) |
| 센서 | 센서 허브 SCP(`scp@1c400000`, 펌웨어 `scp.img`), 홀 센서 둘(`hall_1`, `hall_2`: 키보드 커버), SAR 센서 TYHX HX9036·HX9031AS | `mediatek,scp`, `tyhx,hx9031as` | 가속도·조도 센서는 SCP 뒤에 있는 것으로 보인다(확인 필요) |
| 키보드 커버 | 포고 핀 키보드(Tinno), 3핀(전원, 접지, 데이터 한 줄). 키보드 전원 GPIO61, 붙음 GPIO26(`hall_2`) | `tinno,tn_keyboard`, `mediatek,keyboard_gpio` | 드라이버 대기열(0032~0036) |
| 카메라 | 이미지 센서 둘(I2C 0x10 두 곳), AF GT9772B, EEPROM 둘, 플래시 OCP811 | `mediatek,imgsensor` | 범위 밖 |
| 진동·지문 | `regulator-vibrator`와 Goodix 지문(`mediatek,goodix-fp`, `mediatek,finger-goodix`) 노드가 켜져 있다 | 장치 트리 | MediaTek 참조 보드의 노드가 남은 것일 수 있다. 실물은 기기에서 확인 |
| 보안 | GenieZone 위의 Trusty·Nebula, Trustonic Kinibi(`trustonic,mobicore`), Microtrust(`microtrust,utos`) 노드가 켜져 있다 | 장치 트리 | buntOS에는 필요 없다 |

## 5. GenieZone과 가상화 (12.1절 예비)

- 펌웨어에 GenieZone 하이퍼바이저 이미지 `gz.img`(3,883,056바이트)가 있다.
- SoC 장치 트리에 `hypervisor { compatible = "mediatek,geniezone-hyp"; status = "disabled"; }`가 있다. 보드 오버레이를 입혀도 꺼져 있다. 부트로더가 부팅 때 켜는지는 펌웨어만으로는 알 수 없다.
- GenieZone 위에서 도는 TEE(Trusty `trusty-gz-log`, Nebula `nebula-gz-log`)가 켜져 있다.
- **예비 판정:** GenieZone이 EL2를 차지할 가능성이 높다("불가" 쪽). 그러면 리눅스는 EL1로 시작하고 KVM을 쓸 수 없다. 대신 GenieZone의 VM 인터페이스(`gzvm`)가 있다. 최종 판정은 2a 첫 부팅의 커널 기록(EL2로 시작했는지)으로 한다.

## 6. TB375FC 중국판과 견주기

TB375FC 중국판 순정 펌웨어 `TB375FC_CN_OPEN_USER_M1.491_U_ZUI_16.0.20.091_ST_250117.7z`를 통째로 받았다(5,409,124,544바이트, ZUI 16, 2025-01-17 빌드). 같은 미러에서 받았고, 언락 보고(4.2.1-1)가 있는 바로 그 판이다.
- 7z는 부분 받기로 고를 수 없어 다 받은 뒤 macOS의 `bsdtar`(libarchive)로 부트 이미지만 풀었다(2분 11초).
- 버전 파일은 평문이다: `…_SW_P98300DA2_V091_WIFI_LENOVO_PERIDOT_USR_MP`. 기기 코드명이 PERIDOT이다.
- 파티션 표 `MT6897_Android_scatter.xml`도 평문으로 보이나 풀지 않았다.
- 같은 방법(1절)으로 장치 트리를 뽑아 글로벌판(ZUI 17.0.04.266)과 견줬다.

| 견준 것 | 결과 |
|---|---|
| SoC | 둘 다 `model = "MT6897"` |
| 켜진 노드(경로와 compatible, 오버레이를 입힌 트리) | 중국판 1031개, 글로벌판 1032개로 사실상 같다. 다른 것은 둘이다. 화면 밝기 PWM 노드(`disp_pwm0@1100e000`)가 `soc` 아래에서 뿌리로 옮겨졌고, 커널 시험 노드(`ktf-mmdvfs-test`)가 글로벌판에만 있다 |
| 보드 오버레이(dtbo, phandle과 fixup 표를 빼고) | 27줄만 다르다. 지역 표시(`ROW` / `PRC`), 그리고 Wi-Fi 앞단(FEM) 부품 조합 표다. 글로벌판은 조합을 셋 더 받는다(flags-7~9, `epa-elna`). 화면 패널 셋, 터치, 센서, 앰프, 충전, 배터리 게이지, 키보드 노드는 같다 |
| 배터리 곡선 | 같다 |
| 첫 단계 벤더 모듈 | 중국판 222개, 글로벌판 223개. 글로벌판에만 `afw.ko`가 있다 |
| 커널 | 중국판 `6.1.112-android14-11`, 글로벌판 `6.1.138-android14-11`. 둘 다 구글 GKI다 |
| 프로젝트 이름 | preloader 파일 이름이 중국판 `P98300DA2`, 글로벌판 `P98300DA1`이다. 두 LK는 모두 같은 목록(`P98300AA1`, `BA1`, `DA1`, `DA2`, `EA1`)을 안다 |

**판정.** TB375FC 중국판과 TB373FU 글로벌판은 펌웨어가 기술하는 하드웨어가 같다. 남은 차이는 지역 설정과 빌드 세대(ZUI 16과 17)의 소프트웨어 차이다. 그래서 BSP 하나로 두 모델을 지원한다(ADR-011).
- 둘이 판이 달라서, 차이 가운데 일부(FEM 조합 셋, TCPC의 `tcpc,en-fod`, GPU 노드의 `gpueb_mbox`)는 지역이 아니라 새 빌드에서 더해진 것일 수 있다.
- FEM 조합 차이는 생산 시기에 따라 Wi-Fi 앞단 부품이 달라질 수 있다는 뜻이다. BSP에는 가장 새 펌웨어의 조합 표를 따른다.
- Moto Pad 60 Pro(XT2571-1)는 견주지 않았다.

## 7. 실기기 측정 (ADB, 2026-11-01)

사용자의 TB375FC를 USB로 Mac에 붙이고 USB 디버깅으로 읽기만 했다. root는 없고, 언락하기 전이다.
- 날것의 출력은 포크의 `.handoff/tb375fc/device/2026-11-01/`(git 밖)에 두었다.
- 일련번호와 MAC 주소 같은 식별 정보는 여기에 적지 않는다.

| 항목 | 결과 |
|---|---|
| 판 | `TB375FC_CN_OPEN_USER_M21.814_A16_ZUXOS_1.5.10.044_ST_260123`(내부 이름 ZUI 17.5.10.044). Android 16(SDK 36), 보안 패치 2026-02-05, vendor 보안 패치 2024-10-05, 첫 API 34 |
| 커널 | GKI `6.1.138-android14-11`(2025-06-28 빌드). Android 16으로 올라가도 커널과 vendor는 Android 14 세대 그대로다 |
| 잠금 | `ro.boot.flash.locked=1`, `vbmeta.device_state=locked`, `verifiedbootstate=green`. `ro.oem_unlock_supported`와 `sys.oem_unlock_allowed` 속성은 없다 |
| 슬롯 | A/B이고 지금 `_b`다. 동적 파티션이고 Virtual A/B다(`ro.virtual_ab.enabled=true`). 아래 "fastboot와 재부팅"을 본다 |
| 메모리·저장장치 | RAM 8GB(`ro.boot.ddr_size`), 저장장치 128GB(`/data` 108G). 부팅 장치는 UFS `112b0000.ufshci` |
| 화면 | 패널이 그리는 버퍼는 가로 2944×1840이다(SurfaceFlinger의 `framebufferSpace`, 터치 입력의 `physicalFrame`). 이 크기는 화면 회전 설정과 상관없다. Android는 세로(1840×2944)를 기본 방향으로 두고(`primary_display_orientation=270`) 가속도 센서로 자동 회전한다. 측정 때는 자동 회전이 꺼지고 가로(90°)로 고정되어 있었다. 144Hz, 밀도 340 |
| 입력 장치(`dumpsys input`) | `NVTCapacitiveTouchScreen`(터치), `touch-pen`(펜), `hall`, `tn_wakeup`(포고 키보드 깨우기), `gf-keys`(지문), `mtk-pmic-keys`, `mtk-kpd`, `mt6897-mt6368 Headset Jack` |
| 터치·펜 좌표(`dumpsys input`) | `NVTCapacitiveTouchScreen`(버스 0x1C = SPI) X 0~18400·Y 0~29440, 10점, 접촉 크기 0~255. `touch-pen` X 0~18400·Y 0~29440, 압력 0~4095, 거리 0~1, 기울기 ±60. 컨트롤러 좌표는 픽셀의 10배이고 X가 짧은 쪽이다. 덮개 홀 센서(`hall_1`)는 키 이벤트만 낸다 |
| `getevent -lp` | 입력 장치 7개가 `system` 소유라 셸이 열지 못한다. root가 필요하다 |
| 살아 있는 트리 | `/proc/device-tree`의 노드와 속성 이름은 보이고 값은 막혀 있다(root 필요). 이름을 보면 LK가 `chosen`에 `atag,videolfb`(framebuffer 주소·크기)와 `bootargs`를 넣고, `reserved-memory`에 `mblock-*` 44개를 더한다. 그 가운데 `mblock-14-framebuffer`, `mblock-2-pstore`, `mblock-4-BL31-reserved`, `mblock-16-gz`, `mblock-38-consys_emi_reserved`, `mblock-42-me_GPUEB_SHARED`가 있다 |
| `/proc/iomem` | 막혀 있다(root 필요) |
| 가상화 | `/dev/kvm`과 `/dev/gzvm`이 없다. GenieZone 예약(`mblock-16-gz`)이 있다 |
| vendor 펌웨어(`/vendor/firmware`, 40개) | 터치 `novatek_ts_fw_boe.bin`·`novatek_ts_fw_tm.bin`(과 MP 시험용), 앰프 보정 `aw883xx_acf_gyt.bin`·`aw883xx_acf_yc.bin`, `mali_csffw.bin`, `remoteproc_scp`, soc7_0의 BT·MCU RAM 코드(`soc7_0_ram_bt_1_1_hdr.bin`, `soc7_0_ram_mcu_1_1_hdr.bin`과 각 1a·1b판), `BT_FW.cfg`, `wifi.cfg`. 그 밖에 `connfem.cfg`, `conninfra.cfg`, `txpowerctrl.cfg`, `awinic_*params.bin`, FM 라디오 펌웨어, 카메라 `lib3a.ccu`가 있다. Wi-Fi RAM 코드(`WIFI_RAM_CODE_soc7_0…`)는 없다. 이름만 보이고 파일은 내용도 크기도 읽지 못한다(SELinux. `pull-android.sh`로 시도) |
| 커널 모듈 | 453개가 올라와 있다. `tn_keyboard`, `nt36532`, `hall`, `wlan_drv_gen4m_6897`, `bt_drv_6897`, `conninfra`, `gpueb`, `mali_kbase_mt6897_r44`, `snd_soc_aw883xx`, NT36532 패널 셋 |
| 무선 | `persist.vendor.connsys.chipid=0x6897`, `fm_chipid=connac2x` |
| 센서 | 가속도·자이로 LSM6DSO(ST), 조도 TCS37013(ams). 그 위에 MTK의 가상 센서(중력, 회전 벡터, 걸음 수 등)가 있다. 펌웨어의 AP 장치 트리에는 이 센서 노드가 없고 SCP(`scp@1c400000`)가 켜져 있어, SCP 펌웨어(센서 허브)가 다룬다 |
| 지문·진동 | 지문 센서가 있다(`android.hardware.fingerprint`, 센서 하나). 진동은 컨트롤러가 등록되지 않았다. 진동 모터가 없거나 쓰지 않는다 |
| 업데이트 | 시스템 업데이트 앱은 `com.lenovo.ota`다. 판을 지키려고 2026-11-01에 이 앱을 껐다(`pm disable-user --user 0`, 되돌리기는 `pm enable com.lenovo.ota`). AOSP의 자동 시스템 업데이트 설정(`ota_disable_automatic_update`)은 1(꺼짐)이다. 키보드 펌웨어 업데이트 앱(`com.zui.keyboardupdate`, `.olympia`)도 있다 |

포팅에 닿는 것:
- **언락:** Android 16 판이다. 포크 설계안 2.1절대로 중국판 16.0.20.091로 내린 뒤 언락하는 길을 쓴다. 128GB 기기라 SP Flash Tool의 scatter도 128GB에 맞춘다.
- **2a 장치 트리:** framebuffer 꼴(2944×1840, stride 11776)이 맞다. framebuffer 주소와 pstore·BL31 자리는 값이 막혀 있어 언락하고 root를 얻은 뒤 읽는다. LK가 화면 영역을 `mblock-14-framebuffer`로 예약하므로, 우리 트리에도 같은 예약을 더하면 `framebuffer_mem` 노드는 필요 없다.
- **화면 방향:** 패널의 자연 방향은 가로다. buntOS가 가로를 기본으로 쓰면 화면과 터치를 돌리지 않아도 된다. 세로로 쓸 때는 컴포지터가 돌린다.
- **자동 회전:** 센서는 있지만 SCP 뒤에 있다. buntOS에서 자동 회전을 하려면 SCP 길이 필요하다(포크 설계안 8절). 그 전에는 수동 회전이다.
- **펌웨어:** 터치, 앰프, BT·MCU RAM 코드, Wi-Fi 설정, SCP 펌웨어가 `/vendor/firmware`에 있다. 다만 adb 셸로는 읽지 못하므로, root(언락 뒤)나 SP Flash Tool readback으로 뜬 super에서 vendor를 풀어 얻는다(ADR-012). Wi-Fi RAM 코드는 LK가 싣는다.
- **지문 센서**가 있다. 포팅 블록 표(포크 설계안 5절)에 아직 없다.
- 차지 펌프: `/sys/bus/i2c/drivers/upm6720`에 `10-0065`가 붙어 있고 `nu2115`에는 없다(2026-11-01). 이 기기의 VBUS 입력 FET는 I2C10의 UPM6720 @0x65가 다룬다. 순정 트리에는 후보 넷이 있어 다른 기기는 부품이 다를 수 있다

### 배터리와 충전 값 (순정 ZUXOS, USB에 꽂혀 100%, 29.7도, 22회)

| 읽은 곳 | 값 | 뜻 |
|---|---|---|
| `primary_chg` `CONSTANT_CHARGE_VOLTAGE` | 4340 | MT6375의 CV 레지스터(mV). 실제 충전 전압이다 |
| `battery`, `battery_mm8013` `CONSTANT_CHARGE_VOLTAGE` | 4310 | 게이지의 `REG_CHARGEVOLT`(0x30, mV). 순정이 유지 관리 표의 게이지 값을 쓴다. 게이지가 만충 판단에 쓰는 것으로 보이나 자료가 없어 기기에서 확인한다 |
| `VOLTAGE_NOW` | 4326 | 게이지가 잰 전압 |
| `CHARGE_FULL_DESIGN` | 10200 | 순정 드라이버의 상수다(`10200000`). 게이지 레지스터가 아니다. 배터리 이름의 `10200mAh`와는 맞는다 |
| `PRECHARGE_CURRENT` | 100 | 전류가 아니다. 순정이 이 칸으로 건강 %를 돌려준다 |
| `primary_chg` `CALIBRATE` | 4375 | 칩 ADC의 VBAT. 게이지보다 49 mV 높다. 까닭은 기기에서 본다 |
| `primary_chg` 입력·충전·MIVR·종료, `USB_TYPE` | 2000 mA, 3000 mA, 4600 mV, 300 mA, SDP | Android의 값이다. LK의 500 mA를 부정하지 않는다 |

Android의 `CAPACITY`는 순정 커널이 매끄럽게 만든 값이다. 충전이 끝나면 게이지 RSOC와 차이가 5 미만인 동안 100을 붙든다. Linux의 `capacity`는 게이지 RSOC 그대로다

`battery_maintenance_on=1`, `persist.sys.battery.battery_maintenance_v2.enabled=1`이다. 순정의 '배터리 유지 관리'는 충전 전압을 표로 정한다. 충전기/게이지 값은 v10이 사이클에 따라 150회 미만 4500/4530, 150~1019회 4480/4470, 1020~1049회 4430/4410, 1050회부터 4340/4310이고, v20은 사이클과 상관없이 4340/4310이다. 이 기기는 22회인데 4340/4310이므로 v20이다.

### fastboot와 재부팅 (2026-11-02)

잠긴 채로 맥에서 `adb reboot bootloader`, `fastboot getvar`, `fastboot flashing get_unlock_ability`, `fastboot reboot`를 돌렸다. 기기에 쓴 것은 없고, 안드로이드가 1분 안에 같은 슬롯과 같은 판으로 돌아왔다.
- `getvar`와 `by-name` 목록은 포크의 `.handoff/tb375fc-device-readings/`(git 밖)에 두었다.
- `getvar all`은 `Bootloader_SN_Part1`, `Bootloader_SN_Part2`도 준다. 기기 식별 값으로 보여 저장하지 않았다.

| 항목 | 결과 |
|---|---|
| 맥 fastboot | 닿는다. 맥 포트에 바로 꽂았다. `is-userspace: no`라 LK의 fastboot다. `is-logical` 항목이 없어 super 안의 논리 파티션은 다루지 못한다 |
| LK | `product: P98300DA2`, `version-bootloader: P98300DA2_fa07c1604_202601221851`, `hwboardid: MT8792_8+128_27`, `version: 0.5`, `protocol_version: 2` |
| 잠금 | `unlocked: no`, `secure: yes`. `flashing get_unlock_ability`는 `unlock_ability is false`였고, 개발자 옵션의 OEM 잠금 해제를 켜고 재부팅한 뒤 `true`가 됐다. 안드로이드의 `oem_unlock_allowed`와 `sys.oem_unlock_allowed`는 켠 뒤에도 비어 있어 이 값은 LK에서만 보인다. `flash_lock: no`는 켜기 전후가 같고 뜻은 모른다. `flash lk`와 `flashing unlock`은 시험하지 않았다 |
| 슬롯 | `current-slot: b`. `b`는 successful yes·retry 1, `a`는 successful no·retry 0이고 둘 다 unbootable no다 |
| 전송 한도 | `max-download-size: 0x8000000`(128MiB) |
| 키와 USB ID | 사용자가 눌러 맥에서 확인했다. 전원 + 볼륨 올림은 LK fastboot(`0E8D:201C`)로 들어가고, 전원 + 볼륨 내림은 기기를 끈다. USB가 꽂혀 있으면 배터리 화면이 나오고(`off-mode-charge: 1`), 이때 USB는 `0E8D:2046`("LENOVO", MIDI 인터페이스만)이고 adb와 fastboot가 없다. MIDI로 보이는 까닭은 모른다. 안드로이드의 adb 상태는 `17EF:201C`다 |
| 파티션 크기 | `boot`, `vendor_boot` 64MiB, `lk` 16MiB, `init_boot`, `dtbo`, `vbmeta`, `vbmeta_system`, `vbmeta_vendor` 8MiB(슬롯마다), `super` 9GiB(슬롯 없음), `userdata` 108.2GiB(`0x1b0d7f8000`, f2fs). `getvar`는 이 18개만 크기를 준다 |
| OTA 기록 | `persist.sys.boot.reason.history`는 `reboot,ota` 2026-08-18, `shutdown,battery` 2026-09-26, 2026-10-01(KST)이다. OTA 앱을 마지막으로 쓴 때도 2026-08-18이다. `_b`는 이때 넘어온 슬롯으로 본다 |
| 기다리는 업데이트 | 없다고 본다. 31일 부팅에서 `update_engine`의 CPU 합이 27초였고, `lenovo_ota_new_version_found=0`, `lenovo_ota_process=0`, `csdk_control_ota_downloadStatus=0`이고, `snapuserd`가 돌지 않았다. LK의 `current-slot`이 안드로이드의 슬롯과 같았고, 재부팅 뒤 판이 같다 |
| mapper | `system_b-cow`, `product_b-cow`, `odm_dlkm_b-cow`가 있다. 8월 OTA가 super에 만든 COW가 남은 것으로 본다(짐작). 병합 중이면 돌아야 할 `snapuserd`가 없다 |
| 배터리(`dumpsys battery`) | 100%, 4254mV, 20.5도, `Charge counter` 10098000, health good, USB 5V 500mA |
| 온도(`dumpsys thermalservice`) | CPU·GPU 33.8도, NPU·TPU·SoC 34.1도, 배터리 22.3도, 피부 25.2도, USB 포트 28.0도(충전 중, 부팅 31일째) |
| 온도 문턱(같은 곳, 심함·치명·비상·종료) | CPU·GPU·NPU 105·90·100·117, TPU·SoC 85·90·100·117, 피부 48·53·58·80(가벼움 43, 보통 45), 배터리 50·55·59·60. 차례가 맞지 않는 값도 HAL이 준 그대로다. 우리 트리의 critical 115도는 종료 문턱 아래다 |
| 알람 | `dumpsys alarm`의 `Next alarm clock information`이 비어 있다 |
| 막힌 것 | `/sys/class/rtc/*`, `/sys/class/thermal/thermal_zone*`(68개)의 `type`과 `temp`, `/proc/partitions`. `/sys/devices/platform/boardinfo/boardinfo`는 없다 |

포팅에 닿는 것:
- **판 내리기:** `_a`가 successful no, retry 0이다. 판 내리기가 `_a`에 구우면 슬롯을 바꿔야 할 수 있다(체크리스트 3절 1번).
- **슬롯:** Virtual A/B라 지금 super의 논리 파티션은 `_b` 쪽만 온전하다고 본다(AOSP 동작, 기기에서 확인하지 않았다). `_a`의 boot 계열은 OTA 전 판으로 보이고, 백업에서 확인한다.
- **이미지 크기:** 지금 커널로 만든 이미지(boot 18MB, init_boot 1.5MB, vendor_boot 57KB)는 파티션과 전송 한도 안이다.

### SP Flash Tool 읽기 (2026-11-03)

사용자가 Windows에서 [작업 안내](../tb375fc/windows-steps.md) 3·4절대로 읽었다. 내용은 열지 않고 파일 이름, 크기, 해시 목록만 맞댔다. 예외로 사용자 허락을 받고 `vbmeta_b`, `vbmeta_system_b`, `vbmeta_vendor_b`, `boot_b`의 AVB 머리글에서 rollback index만 읽었다.

| 항목 | 값 |
|---|---|
| 연결 | 잠긴 채로 preloader 연결과 묶음의 인증 DA·`da.auth`로 읽기가 됐다. 드라이버는 Microsoft 카탈로그 3.0.1504.0이다 |
| 시간 | 10.96GiB를 약 6분에 읽었다(파일 시각 기준, 약 30MB/s) |
| 파일 | 81개다. 기기의 파티션 이름 84개에서 `userdata`(뺐다)와 `preloader_raw_a`, `preloader_raw_b`(목록에 없다)를 뺀 수다 |
| 해시 | Windows에서 적은 SHA-256 81개를 맥에서 다시 계산해 모두 맞았다. 옮기는 동안 깨지지 않았다는 뜻이고, 도구가 기기에서 바르게 읽었다는 증명은 아니다 |
| 파티션 표 | `Read PT` 목록은 UFS LU2의 0x8000부터 0x1dcaff8000까지 빈틈이 없고, fastboot `partition-size` 18개와 모두 같다. 묶음의 표와도 시작 주소와 크기가 같다. 다른 것은 `userdata`(`NEEDRESIZE`, 기기 0x1b0d7f8000)와 디스크 끝에 붙는 `mrdump`, `flashinfo`의 시작 주소뿐이다 |
| 슬롯끼리 크기가 다른 것 | `logo_a` 15MiB와 `logo_b` 9MiB, `tee_a` 5MiB와 `tee_b` 7MiB. 묶음의 표도 같다 |
| 전부 0인 파티션 | `boot_para`, `lenovoraw`, `md_sec`, `mrdump`, `otp`, `pstore`, `pvmfw_a`, `pvmfw_b`, `sec1`, `vb`. 해시로 판정했다. 정말 빈 것인지 도구가 0으로 돌려준 것인지는 모른다. 묶음의 `lenovoraw.img`도 전부 0이다 |
| 슬롯 내용 | 둘 다 0인 `pvmfw`를 빼면 `_a`와 `_b`의 26쌍이 모두 다르다 |
| 빠진 것 | `userdata`, `preloader_a`, `preloader_b`, `pgpt`, `sgpt`. `userdata`는 일부러 뺐고, 나머지 넷은 언락 뒤 root로 읽는다(체크리스트 4절) |
| 판 내리기 표 | 묶음에서 굽는 항목은 32줄이고, `preloader_a`, `preloader_b`, `userdata`를 빼면 29줄이다. 이미지는 모두 기기의 파티션 크기 안에 든다 |
| AVB rollback index | 기기의 `vbmeta_b`, `vbmeta_system_b`, `vbmeta_vendor_b`, `boot_b`와 묶음의 `vbmeta.img`, `vbmeta_system.img`, `vbmeta_vendor.img`, `boot.img`가 모두 0이다. 묶음의 `vbmeta`는 `boot`(위치 3), `vbmeta_system`(2), `vbmeta_vendor`(4)를 연결한다. 기기에 저장된 값은 읽지 못했고, LK와 preloader의 자체 검사는 이 비교로 알 수 없다 |
| 묶음의 판 | `vbmeta.img`의 지문이 `TB375FC:14/UP1A.231005.007/ZUI_16.0.20.091_250117_PRC`다. ZUI 16.0.20.091은 Android 14다 |
| 사본 | 같은 SSD의 다른 파티션(APFS)에 복사해 해시 81개를 모두 맞댔다. SSD 고장까지는 막지 못한다 |

### 판 내리기, 슬롯, 언락 (2026-11-03)

사용자가 Windows에서 16.0.20.091을 `_a` 쪽에 구운 뒤(작업 안내 6절) 맥에서 잠긴 채로 읽었다.

| 항목 | 값 |
|---|---|
| `--set-active=a` | OKAY다. 같은 LK 세션에서는 `current-slot: b`, `slot-retry-count:a: 0`으로 남았다 |
| `fastboot reboot bootloader` 뒤 | `current-slot: a`, `slot-retry-count:a: 6`, `slot-successful:a: no`, `slot-unbootable:a: no`다. `_b`는 successful yes, retry 1 그대로다 |
| LK 판 | `P98300DA2_fa07c1604_202601221851`에서 `P98300DA2_8fd2b0938_202501171227`로 바뀌었다. preloader는 굽지 않았으니, 기기의 preloader가 2025-01 LK를 받아 fastboot까지 올라온 것으로 본다 |
| `flashing get_unlock_ability` | 2025-01 LK에서도 `unlock_ability is true`다 |
| 잠금 | `unlocked: no`, `secure: yes` |
| 첫 부팅 | 16.0.20.091의 recovery가 `Can't load Android system` 화면을 띄웠고, `Factory data reset` 뒤 안드로이드로 부팅했다 |
| 부팅 뒤 | `TB375FC_CN_OPEN_USER_M1.491_U_ZUI_16.0.20.091_ST_250117`, Android 14, 패치 2024-11-05, `_a`, locked, green이다. `sys.oem_unlock_allowed`는 1이다(1.5.10.044에서는 비어 있었다). mapper에 `_b`의 COW가 없다 |
| OTA | `com.lenovo.ota`를 `disabled-user`로 껐다(`enabled=3`, force-stop 뒤 `stopped=true`). 재부팅 뒤에도 프로세스가 뜬다. `persistent=true`라서로 본다(판단). CPU 시간은 0이고 `update_engine`도 0이다 |
| 순정 LK 언락 시도 | `fastboot flashing unlock`은 `Please usr fastboot oem unlock`, `Failed to unlock the device`로, `fastboot oem unlock`은 `ERROR: Sn Image Auth fail`로 거부됐다. 잠금, `unlock_ability`(true), 슬롯은 그대로이고 안드로이드로 돌아왔다 |
| 부팅 이유 | `fastboot reboot`와 `adb reboot` 뒤 모두 `ro.boot.bootreason`이 `kernel_panic`, `sys.boot.reason`이 `kernel_panic,init`다. dropbox `SYSTEM_LAST_KMSG`의 패닉은 2.13초의 `Attempted to kill init! exitcode=0x00007f00`이고 커널이 6.1.138이다. 지금 커널은 6.1.112다. 판을 내린 직후 `_b`로 부팅을 시도하다 남은 기록을 계속 읽는 것으로 본다(판단). `fastboot reboot` 뒤 커널이 뜨기까지 약 3분 걸린 일이 한 번 있었고 원인은 모른다 |
| 개발용 LK | XDA 원글의 `lk_a`를 잠긴 순정 LK가 받았다. 16MiB 전송이 51초(약 0.3MB/s)다. `P98300DA2_19ce4236_202411042159`로 떴고, 안드로이드가 36초에 locked, green으로 부팅했다 |
| 언락 | 시스템을 한 번 부팅한 뒤 `flashing unlock_critical`이 `Start unlock flow`, `Please Implement check key function`을 찍고 OKAY로 끝났다(볼륨 키 승인). `unlocked: yes`, `secure: no`다 |
| 되돌림 | 순정 LK를 0으로 채운 16MiB 이미지를 `lk_a`에 구웠다. `P98300DA2_8fd2b0938_202501171227`로 돌아왔고 언락은 유지됐다. 잠겼을 때와 `getvar all`에서 달라진 값은 `unlocked`, `secure`뿐이다. 다음 부팅은 설정 마법사부터 시작했다 |
| 언락 뒤 안드로이드 | `ro.boot.flash.locked` 0, `device_state` unlocked, `verifiedbootstate` orange, `veritymode` enforcing, `_a`다. 설정 마법사를 거치며 OTA가 다시 켜져 있어(`update_engine` CPU 0) 다시 껐다. 초기화 뒤 부팅 이유는 `reboot,factory_reset`이다 |
| AVB 끄기 | `vbmeta_a`의 flags를 3으로 구웠다. 다음 부팅은 `Can't load Android system` 복구 화면이었고 초기화가 필요했다. 초기화 뒤 `verifiedbootstate` orange, `ro.boot.veritymode`와 `ro.boot.vbmeta.device_state`는 비어 있고, mapper의 verity 장치가 0개다(켜져 있을 때는 `system-verity` 등이 있었다) |
| 순정 LK 파일(16.0.20.091) | 구역 `lk`, `bl2_ext`, `aee`, `lk_main_dtb`, `lk_dtbo` 뒤마다 `cert1`, `cert2`가 있다. `flashing unlock_critical` 문자열은 없다. 확인 화면 문구는 `Yes (Volume UP or Volume Down)`, `No (Auto exit after 5 seconds)`다 |

### root로 읽은 값 (16.0.20.091, 2026-11-03)

Magisk 31.1로 읽었다. 원본은 `.handoff/tb375fc-device-readings/root-091/`에 있다(git 밖). 명령줄과 트리에는 식별자가 들어 있어 여기에 옮기지 않았다.

| 항목 | 값 |
|---|---|
| root | 앱에서 패치한 `init_boot_a`로 부팅했다. Magisk v31.1은 허용 창이 없어 Superuser 탭에서 `Shell`을 미리 허용해야 한다(공식 변경 내역, 실측) |
| framebuffer | `atag,videolfb-fb_base`가 0xfc16f000, `vramSize`가 0x3e90000이다. `/reserved-memory/mblock-14-framebuffer`가 같은 값이고 `no-map`이 없다. 커뮤니티 포트의 후보와 같다. stride와 형식은 트리에 없다 |
| 패널 | `atag,videolfb`의 이름이 `nt36532_dsi_vdo_144hz_tianma_lcm_drv`다. 이 기기는 Tianma 패널이다 |
| ramoops | LK가 `mblock-2-pstore`(0x48090000, 0xe0000, `no-map`)를 예약하고, 명령줄에 `ramoops.mem_address=0x48090000 ramoops.mem_size=0xe0000 ramoops.pmsg_size=0x80000 ramoops.console_size=0x40000`을 넣는다 |
| pstore | `console-ramoops-0`의 패닉은 6.1.138 커널의 것이다(2.13초, `Attempted to kill init! exitcode=0x00007f00`). 지금 커널은 6.1.112다. 부팅 이유 `kernel_panic,init`은 이 옛 기록에서 온 것으로 본다(판단) |
| 명령줄 | `console=ttynull console=tty0 root=/dev/ram bootconfig 8250.nr_uarts=4 swiotlb=noforce arm64.nomte`와 위의 `ramoops.*`가 있다. 나머지는 이름만 적어 두었다 |
| 메모리 | `/memory`는 0x40000000부터 0x200000000(8GiB)이다. BL31은 `mblock-4-BL31-reserved` 0x48800000, 2MiB다 |
| GPT | LU2(`sdc`), 4KiB LBA, 항목 82개, 빈틈 없음. 형식 GUID는 모두 `ebd0a0a2-b9e5-4433-87c0-68b6b72699c7`이다. `userdata`는 LBA 2803712~31170551(110808MiB, 0x2ac800000부터)이고 뒤에 `mrdump`, `flashinfo`가 있다 |
| 입력 | `NVTCapacitiveTouchScreen`, `touch-pen`, `gf-keys`, `tn_wakeup`, `mtk-kpd`, `mtk-pmic-keys`, `hall`, `mt6897-mt6368 Headset Jack`, `uinput_nav` |
| efuse | `mtk-devinfo0`의 0x1cc [5:0]은 2026-11-03에 읽었다(기기마다 다른 보정값이라 숫자는 적지 않는다) |
| RTC | `mt6685-rtc`. `date -u`와 같은 시각을 읽었다(2026-11-03) |
| 온도 | 구역 68개를 읽었다. 값은 `root-091/thermal.txt`에 있다 |
| 펌웨어 | `/vendor/firmware` 파일 40개를 받았다(git 밖). `novatek_ts_fw_tm.bin`, `novatek_ts_fw_boe.bin`이 있다. `extract.sh`는 맥의 `install`과 맞지 않아 멈췄고 VM에서 돌려야 한다 |

### 첫 리눅스 부팅 (2026-11-03)

슬롯 `_b`에 구워 시험했다. `_a`의 안드로이드는 건드리지 않았다. LK 기록은 `expdb` 파티션에 남는다.

| 항목 | 값 |
|---|---|
| `fastboot boot` | LK에 없다. `Booting FAILED (remote: 'unknown command')`이고, 순정·개발용 LK의 명령 표에도 없다 |
| A/B 되돌림 | `_b`가 실패하면 재시도 6번 뒤 LK가 `_a`로 돌아간다. `slot-unbootable:b`가 yes가 된다 |
| 우리 `dtbo`(DT 표, 서명 없음) | `img_auth_required = 1`, `dtbo cert chain vfy fail (0x10100d)`, `ASSERT FAILED load_image.c:167`. 언락해도 서명을 요구한다 |
| `dtbo`를 0으로 | `non of known hdr detected`, `load_dtbo fail`을 찍고 overlay 없이 넘어간다(코드대로) |
| 우리 트리로 LK 통과 | 트리는 읽지만 `app_load_scp(fdt, b_mode) == NO_ERROR` ASSERT로 멈춘다. 적재 차례는 scp, ccu, spm, apu, apusys_rv, dpm, modem, adsp, gpueb, vcp, connsys이고 벤더 노드(`mediatek,scp`, `mediatek,sleep`, `/soc/gpueb`, `mediatek,vcp`, `/soc/consys` 등)를 찾는다 |
| 시험 커널 | 순정 `vendor_boot`, 순정 `dtbo`, 우리 `boot`(내장 트리, 내장 initramfs, uart0 끔, 예약 43개와 메모리 8GiB를 살아 있는 트리에서 옮김). **7.2.12가 약 0.93초에 /init을 돌려 BusyBox 셸까지 갔다** |
| 화면 | `simpledrmdrmfb 2944,1840`. stride 11776, `a8r8g8b8`이 맞다(글자가 또렷하다). LK의 Orange State 글자가 같은 버퍼에 남아 있다 |
| 안 되는 것(이 단계에서 끈 것) | `no UDC within 30s`, `nvdata`·`nvcfg`·`persist: no partition`, 입력 장치 없음 |
| 키 | 리눅스에서 전원 + 볼륨 올림은 fastboot로 들어간다. 전원 + 볼륨 내림은 반응이 없었다 |
| pstore | 안드로이드의 ramoops는 record 0x1000, console 0x40000, ftrace 0x1000, pmsg 0x80000이고 보드 dts도 같은 값으로 맞췄다. 따뜻한 재부팅이면 리눅스에서 리눅스로, 리눅스에서 안드로이드로 기록이 넘어간다(`console-ramoops-0`, 30233바이트). 전원 + 볼륨 올림 리셋 뒤에는 비어 있다 |
| EL | **`CPU: All CPU(s) started at EL1`, `kvm [1]: HYP mode not available`.** 이 기기에서 KVM을 쓸 수 없다. 예약 메모리의 `mblock-16-gz`로 보아 GenieZone이 EL2를 차지한 것으로 본다(판단) |
| EL1에서 되는 것 | 8코어(A510 넷, A715 넷) 모두 같은 기능 줄이다. PAC, BTI, SVE2(128비트), AMU, PMU(A510 카운터 7, A715 21)가 잡힌다. 네임스페이스 여섯 종(user, pid, mount, uts, ipc, net)을 만들어 봤다. MTE는 `arm64.nomte`로 껐고 순정 명령줄에도 같은 인자가 있어, EL2 때문인지는 모른다. EL1에서는 EL2 레지스터를 읽지 못해 stage-2 변환 여부와 그 손해를 잴 수 없다. 리눅스에 보이는 메모리는 7.36 GiB이고, 순정 트리의 GenieZone 예약은 26 MiB, `hypervisor` 노드는 `disabled`다 |
| 부팅 시간 | `initcall_debug`를 빼면 0.16초에 `/init`을 돌린다(켜면 0.93초) |
| `kernel_panic,init`의 원인 | `expdb` 앞에 LK의 kedump가 남아 있다(머리 `0xaee0dead`, 항목 0x12. 판 내리기 직전 백업에서는 전부 0이었다). LK가 부팅마다 `kedump: found content in expdb`로 저장된 pstore와 ram console을 메모리에 되살린다(`SYS_PSTORE_RAW`, `SYS_RAMCONSOLE_RAW`). 그래서 부팅 이유가 계속 `kernel_panic`이고 pstore에 6.1.138 패닉이 남았다. `expdb` 앞 12KB를 0으로 하자 부팅 이유가 `reboot,shell`로 돌아왔다 |
| 덤프가 다시 쌓임 (2026-11-17) | 2026-11-04 04:43의 패닉(커널 #267, Wi-Fi 시험 빌드: `napi` 스레드의 `mt7921_mac_sta_poll` → `ieee80211_sta_register_airtime`에서 NULL 읽기)을 LK가 다시 expdb에 적었고, 그 뒤 13일 동안 모든 부팅에서 되살렸다. `/sys/fs/pstore`의 기록은 부팅마다 같은 것이었고(`[ 1533.143067] Kernel panic`), 직전 부팅의 콘솔은 넘어오지 않았다. 덤프는 머리(`0xaee0dead`, 판 0x12, 머리 크기 0x2030) 뒤 0x30부터 0x40바이트짜리 항목 49개(종류, 쓰는지, 자리, 크기, 쓴 크기, 0x20부터 이름)이고 끝이 34MiB다: `SYS_RAMCONSOLE_RAW`(0x3000, 0x1000), `SYS_PSTORE_RAW`(0x4000, 0xe0000), `KEDUMP_CRC`, 그 뒤로 CPU 버스·DRAM·SPM·GPU·GenieZone 기록과 BL33 메모리 조각(12MiB)이 있다. LK 기록에는 정상 부팅마다 `kedump: found content in expdb`, `read SYS_PSTORE_RAW from offset 0x4000 size 0xe0000`, `mboot_params_should_restore`가 있다. `buntos-kedump-clear.service`가 부팅 때 덤프를 `/var/lib/buntos/kedump`에 gzip으로 두고(34MiB가 7.4MiB, root만 읽는다, 새것 둘) 머리 12KiB를 0으로 한다. 기기에 넣어 한 번 돌렸고 머리가 0이 됐다. 다음 재부팅에서 직전 부팅의 콘솔이 넘어오는지는 아직 보지 못했다. LK가 지금 커널의 패닉을 예외로 알아보고 새 덤프를 적는지도 모른다(최근 부팅은 `exp_type 0x0, wdt_status 0x2`로 정상 흐름이다) |
| 커널 기록 | CPU 8개(0~3 Cortex-A510 `0x411fd461`, 4~7 Cortex-A715 `0x411fd4d0`), PSCI 1.1, GICv3 SPI 960개, 메모리 7447704K/8388608K, `mtk-wdt 1c00a000` 켜짐(31초), 성능 도메인 셋, 약 0.15초에 `/init`이다. 오류는 없고, A715 쪽에 `SANITY CHECK: Unexpected variation in SYS_ID_AA64MMFR1_EL1`이 나온다. 원문은 `.handoff/tb375fc-device-readings/boot-b-7/`에 있다 |

### 2b 단계별 시험 (2026-11-03)

시험 커널에서 노드를 `test-stage.dtsi`로 하나씩 켰다. 앞 단계는 안드로이드로 돌아온 뒤 pstore에서 읽었고, USB 셸이 뜬 뒤로는 셸에서 읽었다.

| 항목 | 값 |
|---|---|
| 핀, 클럭 | 통과. 미뤄진 장치 없음. mainpll 2184 MHz, univpll 2496 MHz, tvdpll 약 594 MHz, ulposc 588 MHz로 문서의 짐작과 맞다. 먹스는 LK 값: `uart` 26 MHz, `ufs` 약 458 MHz, `mminfra` 약 687 MHz, `usb_top`·`ssusb_xhci` 124.8 MHz, `vlp_spmi_m`·`vlp_pwrap_ulposc` 29.4 MHz |
| SPMI | M 버스와 PMIC 다섯(MT6363, MT6368, MT6319 셋) 통과. `Err: 0`, `mtk-spmi-pmic` 줄이 모두 0으로 인터럽트 폭주가 없다. "Cannot mask interrupts", "no interrupt group", "Could not clear RCS flag", `cleared event`가 없다. 안드로이드에서 재부팅한 경우와 리눅스에서 재부팅한 경우만 봤다 |
| MT6363 | `TOP_TRAP`(`0x6`)이 0이다. 레일이 모두 등록되고 전압을 읽는다. 키 `mtk-pmic-keys`가 뜬다. VS1 2000 mV, VS2 1450 mV, VS3 1050 mV, VCORE 725 mV, VIO18 1820 mV, VUFS18 1800 mV, VUFS12 1200 mV |
| VEMC | `0x1b10` = 0xb6, `0x1b1b` = 0xf5로 VOSEL 6, VOCAL 5다. `vufs_vcc`가 2550 mV로 읽힌다(옛 7.2.12의 1.2V·1.3V 오독이 고쳐졌다). 벤더 커널은 VOCAL을 더하지 않고 2500 mV로 보인다. 안드로이드에서 VEMC는 UFS 쪽이 껐다 켰다 한다(읽을 때마다 `disabled`, `enabled`로 다르다) |
| VSRAM | `mt6363_vsram_cpub·cpum·cpul`이 743, 750, 750 mV이고 켜기 비트(`0x1dd7`, `0x1ded`, `0x1e57`의 bit 0)가 1이다 |
| MT6368 | 레일이 모두 등록된다. `vusb33` 3000 mV, `mt6368_vio28` 1800 mV, `vgpu` 600 mV, `mt6368_vtp` 3300 mV |
| MT6319 | `vmm`, `vproc1~3`, `mt6319_7_vbuck3`, `vufs_vccq`. 전압 한계를 주면 @8이 probe에서 `vufs_vccq: failed to get the current voltage: -EINVAL`로 실패한다. @8 벅 3의 설정값 `0x144d`(ELR4)는 0xff이고 실제 값 `0x1599`(DBG0)는 0xbf, 곧 1193.75 mV다. 드라이버 표는 0xbf까지이고 업스트림은 ELR을 읽는다. 벤더는 켜져 있으면 DBG0을 읽어 1193750 µV로 보인다. 보드에서 한계를 빼자 통과했고, 레일 요약에는 0mV로 나온다 |
| USB 가젯 | 맥에서 `TB375FC initramfs`, 480 Mb/s, `/dev/cu.usbmodembuntos1`, `en9`가 172.16.42.2를 받는다. 직렬 셸, ping 약 1 ms, telnet이 된다. `mtu3`는 `IP version 0x1007(U3 IP)`이다. SS는 시험하지 않았다(리드라이버 드라이버 전) |
| U2 PHY | 부트로더 뒤의 `0x11e40368`(U2PHYDTM0)이 0x02be00d4다. 서스펜드, 풀다운 둘, XCVRSEL, TERMSEL이 모두 강제다. 7.2.12의 XS-PHY는 이 비트를 지우지 않아 D+가 오르지 않고 인터럽트도 없다. 본가는 7.4에서 고쳤다(`02df7fe1297d`, 패치 0143) |
| VBUS | 컨트롤러의 VBUS 입력이 케이블을 따르지 않는다. 가젯이 도는 중에 `0x11201c84`(MISC_CTRL)의 덮어쓰기를 지우자 맥에서 곧바로 끊겼고, 8초 뒤 되돌리자 다시 붙었다(패치 0141, 0142) |
| PHY 값 | INTR_CAL(`0x11e40330` [29:24])은 LK가 쓴 값과 efuse 값이 하나 다르다(기기마다 다른 보정값이라 숫자는 적지 않는다). eye(`0x11e40304`)는 [14:12] = 7, [10:8] = 3으로 벤더 값과 같다. U3 CDR(`0x11e43808` [23:16])은 0x0b이고 벤더가 MT6897에 넣는 값은 0x08이다 |
| UFS 장치 | Micron `MT128GBCAV2U31`, UFS 3.1(`wspecversion` 0x0310). HS-G4 Rate B, 2레인, Fast 모드로 붙는다. MCQ 없이 단일 도어벨이다. 먹스는 LK 값 그대로 `ufs` 약 458 MHz, `aes_ufsfde` 546 MHz다 |
| UFS 나눔 | `sda`, `sdb` 각 4 MiB(파티션 없음), `sdc` 약 119 GiB(GPT 82개, 이름이 `PARTNAME`으로 나온다). `nvcfg` `sdc6`, `nvdata` `sdc7`, `persist` `sdc9`, `lk_a` `sdc35`, `boot_b` `sdc66`, `vendor_boot_b` `sdc67`, `dtbo_b` `sdc75` |
| UFS 읽기 | **쓰지 않았다.** `lk_a`, `boot_b`, `vendor_boot_b`, `dtbo_b`의 해시가 맥의 사본과 같다. `sdc` 앞 2 GiB의 해시가 두 번 같다. 1 MiB 직접 읽기로 약 790 MB/s다. `nvdata`, `nvcfg`, `persist`는 `blockdev --setro`와 `ro,noload`로 붙었다. 뒤이어 `_a` 안드로이드가 정상으로 부팅했다 |
| UFS 절전 | 유휴 뒤 `SLEEP`, `HIBERN8`로 내려가고 읽으면 돌아온다. 6번 되풀이에 오류가 없다. VEMC 켜기 비트(`0x1bf3`)가 절전에 0, 활동에 1로 벤더처럼 오간다. 오류 통계가 모두 0이다(`Logical Unit Resets: 1`은 초기화의 장치 리셋이다) |
| UFS 기록 | `vufs_vccq: mode operation not allowed`가 절전 전환마다 남는다(보드가 MT6319 모드를 막았다). 약 3분에 18번이고 동작에는 영향이 없다. HS-G5 속성(`0x15d6`)을 UFS 3.1 장치가 거부해 나던 오류와 7.2.12의 재시도 경고는 패치 0144로 사라졌다 |
| 화면이 희게 깨짐 | UFS를 켠 시험 커널에서 커널 글자가 잠깐 보인 뒤 화면 전체가 희게 변하고, 세로줄이 섞인 회백색으로 남았다(영상으로 확인). 화면 메모리는 그대로 정상이었다. `ufs`의 부모 `mmpll_d6`와 `disp0`, `ovl0`, `mminfra`의 부모 `mmpll_d4`가 같은 PLL이다. 커널은 화면 드라이버가 없어 이 사용자를 모르므로, UFS가 클럭을 내리면 사용자 수가 0이 되어 PLL을 끄는 투표를 보낸다. 그 뒤로 CPU가 `dispsys0`, `mipitx`를 읽으면 버스가 멈추거나 SError가 난다. `simple-framebuffer`의 `clocks`로 `mmpll`을 잡아 두자 UFS 절전과 복귀를 오가도 양쪽 경로 레지스터가 읽히고, 화면도 사용자가 눈으로 보아 정상이다 |
| 화면 경로 값(LK 그대로) | OVL0 주소 `0xFC16F000`, OVL1 `0xFC1706D4`(1461화소 뒤), 피치 `0x2E00`, ROI 1483×1840(겹침 11화소). DSC `0x10081`, 조각 너비 736. DSI 둘 다 4레인 영상 모드, VSA 2, VBP 182, VFP 26, 세로 1840, 줄당 `0x5C0`바이트. `DSI_INTSTA`의 언더런 비트(bit 12)는 0이다. HW voter의 화면 비트는 LK가 넘길 때부터 0이고 전원은 SPM에서 켜져 있다(`0xe9c`~`0xeac`) |
| 화면 설정 반영 | OVL 레이어 레지스터는 그림자이고 다음 프레임 시작(SOF)에 반영된다. 작업 레지스터는 OVL_EN bit 20(READ_WRK_REG)으로 읽는다. 상태 기계는 FLOW_CTRL_DBG(0x240)의 [9:0](1 idle, 2 wait_SOF, 0x20 처리 중). 프레임 끝에서 다음 SOF까지 약 0.30 ms. 두 파이프는 커밋 때 쓰고 반영을 확인한 뒤 flip을 끝낸다(0197). mutex의 0x24 칸은 이 세대에서 GET이 아니다(모두 3으로 읽힌다) |
| 프리로더 포트 | 부팅마다 프리로더가 약 3초 동안 `MT65xx Preloader`(ACM)로 뜬다. 우리 가젯과 같은 `/dev/cu.usbmodem*`로 보이니 제품 이름으로 가린다 |
| pstore | ECC가 없다. 2단계 기록 한 벌에서 글자 몇 개가 깨졌다(`368x115`가 `3688115`, `simple`이 `simpLe`). 다른 기록에서는 보지 못했고 원인은 모른다 |

### 루트 파티션과 UFS 루트 (2026-11-04)

| 항목 | 값 |
|---|---|
| 원래 GPT | 항목 82개로 꽉 차 있다. 빈 칸이 없다. 앞 표는 LBA 1(항목 LBA 2), 뒤 표 헤더는 LBA 31240191(항목 31240184), 쓸 수 있는 범위 8~31240183 |
| 항목 수 한도 | 프리로더 `parse_gpt_header`: 항목 193개 미만(`cmp w22, #0xc1`), 항목 크기 128(`0x80`), 읽기 크기 16 KiB 이하(코드로 읽음). LK는 헤더의 값대로 할당한다. 83개로 늘렸다 |
| 새 표 | `userdata` LBA 2803712~10198783(28.2 GiB), `buntos` LBA 10198784~31170551(80.0 GiB, 항목 83, 형식 root-arm64, 1 MiB 정렬). `mrdump`, `flashinfo`는 그대로다 |
| 쓰기 | 리눅스 가젯 셸에서 `dd`로 뒤 표 → 앞 항목 → 앞 헤더 차례로 썼다. `userdata`, `metadata`의 앞부분을 먼저 지웠다 |
| 부트로더 | 프리로더와 LK가 새 표로 부팅한다. 재부팅에서 커널까지 약 18초다 |
| UFS 쓰기 | 2 GiB 무작위 값을 439 MB/s로 썼고, 캐시를 비운 뒤와 재부팅 뒤에 해시가 같다. 오류 통계 0 |
| 루트 전환 | 시험 커널의 initramfs가 `root=PARTLABEL=buntos`를 찾아 넘어간다. Debian forky systemd가 `running`까지 약 3.2초(커널 1.4초 + 사용자 공간 1.8초)다. btrfs가 `discard=async`로 붙고 80 GiB까지 저절로 늘어난다 |
| USB 길 | `buntos.usbdebug=1`로 진짜 루트의 가젯, 직렬 셸, SSH가 뜬다. 맥에서 `ssh root@172.16.42.1`로 들어간다 |
| 커널 갈기 | fastboot 없이 리눅스에서 `boot_b`에 `dd`로 굽고 해시를 맞댔다. 다음 부팅이 그 커널로 뜬다 |
| 안드로이드 슬롯 | `_b`의 재시도를 다 쓰자 `_a`가 새 표에서 부팅해, 약 47초 뒤 USB 장치(`联想小新Pad Pro 12.7`)로 올라왔다. 화면에 초기 설정이 나온다(사용자 확인). 초기화 상태라 ADB는 꺼져 있다 |
| fastboot | `getvar partition-size:userdata`는 `0x70d700000`(28.2 GiB)이고 `buntos`는 값이 비어 있다. `--set-active=b` 뒤 `_b`의 재시도는 6이다 |
| A/B 정보 | `misc`의 오프셋 0x800(AOSP `bootloader_control`, 매직 `BCAB`). `_b`의 재시도는 LK가 부팅마다 줄이고, 다 쓰면 `_a`로 간다. `successful_boot`를 세우는 쪽이 아직 없다 |

### 2c·2d·2e 기기 시험 (2026-11-04)

시험 커널에서 본 값이다. 재부팅해도 같은 트리가 돈다. 전력계 없이 칩 설정값과 게이지로 봤다.

| 항목 | 값 |
|---|---|
| 터치 | NT36532. 컨트롤러 자체 CS로는 칩이 0만 돌려주고, GPIO12를 소프트웨어 CS로 쓰면 trim ID `32 65 03`이 온다. 1 MHz와 4 MHz에서 같다. 패널 ID(GPIO184)는 1이고 LK 기록의 Tianma와 맞다. 터치를 열면 펌웨어 약 600 KB가 SPI로 내려간다. 축은 `touchscreen-swapped-x-y`와 `touchscreen-inverted-x`다. 화면에 점을 찍어 사용자가 확인했다. 두 손가락 확인, 보고 간격 약 7 ms |
| 키 | 전원과 볼륨 올림은 MT6363 키, 볼륨 내림은 SoC 키패드(`mt6779-keypad`, 행 0 열 0, 32 ms)다 |
| 덮개, 펜, 키보드 | 미확인(액세서리 없음). 입력 장치는 올라온다. UART3 패드 `0x11036050`은 `0x10`, 키보드가 없을 때 GPIO26은 1이다 |
| RTC | MT6685. `hctosys`로 시스템 시간이 맞고 3초에 3씩 는다. 쓰기는 아직 안 했다 |
| connsys | conn 도메인이 켜지고 꺼진다. A-die `0x66378a01`, 클럭 52 MHz co-clock. SIP `init_atf_data`, `afe_sw_patch`는 -1을 돌려주고 벤더처럼 계속 간다 |
| Bluetooth | `BGF: version 02080000`, `WMT function on: status 0`, 보정 status 0(1020바이트), HCI 6.0, 제조사 70. 스캔에서 주변 장치 17개. 주소는 시험값이다. 실패한 Wi-Fi 위에서 켜면 BGF ROM이 돌지 않았다(`radios 0x9`, `MCU ROM: 00000000`). Wi-Fi가 정상일 때 다시 본다 |
| Wi-Fi | 2.4 GHz WPA2-PSK 연결이 인터넷까지 된다(비밀번호는 사용자가 직접 넣었다). 인증부터 연결까지 0.24초, DHCP 2초, 내려받기 38, 107, 138 Mbit/s, 올리기 87 Mbit/s, 신호 −51~−41 dBm, 수신 최고 HE-MCS 11 2x2, 약 10분 유지, `tx failed` 0, 연결 중 스캔 정상. 펌웨어 빌드 `t-neptune-main-soc7_0-2104-PONSOT_SOC7_0_E1_ASIC-20250320113502`. uni 명령에는 멀쩡한 펌웨어에서도 답이 없어 레거시 명령으로 보낸다(패치 0061~0065). 칩 리셋은 첫 시도에 돌아오고 리셋 뒤 스캔이 된다(패치 0066, 12번). 연결 중 리셋 뒤에는 약 9초에 다시 붙고 인터넷까지 돌아온다(패치 0067, 6번). `regulatory.db`가 거부되어 규제가 `00`이다. 5 GHz(5745 MHz) WPA2-PSK 연결도 DHCP와 인터넷까지 된다. 표시 속도는 수신 720.6 Mbit/s(80 MHz, HE-MCS 7, 2x2), 송신 286.7 Mbit/s이고 `tx failed` 0이다. 짧은 시험이라 처리량과 오래 유지는 보지 않았다. RX 순서 정리는 못 봤다 |
| 열 센서 | MCU, AP 유휴 38~42도. 컨트롤러에 PROTCTL 전략, PROTTC 약 119도, MONINT bit 31이 서 있어 하드웨어 리셋이 켜진 것으로 본다. AP 셋째 컨트롤러(모뎀 센서)만 `PROTCTL` 낮은 절반에 `0x3FFF`가 남아 리셋이 미뤄져 있다. 측정값은 다른 컨트롤러와 같은 범위다. GPU·APU 컨트롤러는 그쪽 전원이 꺼지면 `CLKEN`이 0이다 |
| CPU 냉각 | 냉각 없이 전 코어 부하에서 5초 안에 93도. passive 85도와 cpufreq 냉각을 넣으면 40초 동안 87~93도, cpu7 약 2.0 GHz. 멈추면 5초 안에 46~53도 |
| CPU 유휴 | `cpu-off`, `cluster-off`를 쓴다(2026-11-07). BL31은 MCUPM 메일박스의 작업 상태가 2나 3이 되어야 CPU_SUSPEND를 받고, MCUPM은 AP가 시간 기준을 써야 그 상태로 간다(패치 0193~0194). 그 전에는 사용 0, `rejected`만 1천만 회 넘게 쌓였다. 화면만 켠 상태에서 cluster-off로 약 0.06~0.07 W 준다. cpu-off만으로는 차이가 없다 |
| 게이지 | MM8013. 4.27 V, 87%, 28.7도, `charge_full` 9676 mAh, 설계 10000 mAh, 사이클 23. `current_now`는 방전이 양수다. `0x30`은 이번에 `0x11b2`(4530)였다 |
| 충전 칩 | MT6375 `0x72`, TCPC `cf 29 75 63`, 차지 펌프 UPM6720 @0x65. LK 값: CV 4.35 V, ICHG·AICR 0.5 A, CHG_EN 1, 워치독 꺼짐, `0x33` = `0x80`. 읽기 전용 드라이버에서 쓰기 0 |
| 충전 넘겨받기 | CV 4.34 V, 입력·충전 0.5 A. `inhibit-charge`로 `CHG_EN` 0, `auto`로 복귀. 게이지를 떼면 27초 뒤 `Lost the fuel gauge`로 멈추고 다시 묶으면 돌아온다 |
| USB 커넥터 온도 | MT6363 AUXADC VIN4의 NTC(100 kΩ 풀업). 방 온도에서 21.6도(게이지 23.1도), 손으로 쥐면 1분에 22.4도에서 25.9도. VIN5(플래시 LED) 23.6도, VIN6(메인보드) 22.8도. 업스트림 `mt6359-auxadc`는 MT6363의 VIN을 읽지 못해 고쳤다(패치 0150) |
| 커넥터 과열 보호 | 75도를 넘으면 GPIO179 high, 충전 끔, 입력 100 mA. 65도 아래가 3번이면 돌아온다(패치 0099). 실제로 뜨거워진 적은 없고 값을 강제로 넣어 봤다. 센서를 못 읽으면 10초 안에 입력 500 mA. 80도면 충전기와 TCPC 모두 VBUS를 못 보고, 값이 돌아온 뒤 25~30초에 복귀한다. GPIO179가 끊는지 단락하는지는 모른다 |
| 벽 충전기 2A | 5 V 2 A로 10분. 배터리 약 +1.17 A(4.5 W), 부하 약 4.2 W, 커넥터 23.5도 그대로, 배터리 23.5도. 맥 포트는 1.5 A에 배터리 +0.55 A다. 케이블 양방향과 켠 채 뽑았다 꽂기가 된다. PD 응답이 없으면 충전기가 꽂은 뒤 약 20초에 VBUS를 잠깐 내렸다 올린다. TCPM이 분리됨에 머물러 500 mA로 고정되던 것을 고쳤다(패치 0151) |
| 차단 시험의 이상(종결) | 둘째 차단 시험에서 GPIO179를 올린 뒤 19초에 CC가 14초 끊기고, 그동안 커넥터가 23도에서 33도로 오른 뒤 1분 안에 돌아왔다. 첫 시험에는 없던 일이고 사용자는 손댄 기억이 없다. 그 뒤 충전기가 CC를 잡은 채 VBUS를 주지 않았고, 20분 뒤 다시 꽂았을 때도 GPIO179가 low인데 VBUS가 없었다. 구형 케이블 문제로 보고 종결했다(사용자 판단). 재현 시험을 제대로 못 돌려 확정한 것은 아니고, 같은 케이블로 2A 10분은 문제없었다. 3단계 고속 충전 시험 때 다시 본다 |
| USB-C 싱크 | 맥 포트 Rp 1.5A. 입력 1.5 A, 배터리에 약 520 mA, 4.217 V에서 4.252 V, 1분 동안 온도 변화 없음. 가젯은 그대로이고, 드라이버를 내리면 `0x1a` `0x0a`, 입력 0.5 A다 |
| GPU | GPUEB `65 GPU OPPs, 0 stack OPPs, shader cores 0x130013`. panthor: Mali-G615 `0xb8a3`, CSF v2.8.0(linux-firmware arch11.8). devfreq 265 MHz~1.4 GHz. 런타임 서스펜드에서 `off-0`이다 |
| Mesa | 26.1.6은 G615를 몰라 llvmpipe로 내려간다. 26.3.0-rc3은 `Mali-G615 MC6 (Panfrost)`, ES 3.1이다. 1080p 전 픽셀 맞대기 오류 0, 무거운 셰이더 평균 230~248 FPS. 85도 passive로 약 1.13 GHz, 86도에서 안정된다 |
| 개발 환경 | 기기에서 gcc, git, make, Podman(`debian:stable-slim`, `--network=host`), 네임스페이스 여섯 종이 된다 |
| 대기 전류 | 화면 켬, 입력 0.5 A, 시험 커널에서 배터리 약 450~550 mA 방전 |
| 슬롯 | 재시도는 부팅마다 준다. `buntos-slot-keep`이 `misc`의 `_b` 재시도를 되돌린다 |

### 소리 (2026-11-05)

읽은 기기 한 대의 값이다. 보정값과 GPIO82는 기기마다 다를 수 있다.

| 항목 | 값 |
|---|---|
| 앰프 | AW88394 넷, 칩 ID 0x2049. 리셋 GPIO78~81(1이면 동작), GPIO82가 스피커 공급사를 구분한다. 풀업 `MTK_PUPD_SET_R1R0_10`에서 1을 읽어 `aw883xx_acf_yc.bin`을 쓴다 |
| 링크 | ETDM 4, TDM 4슬롯, 슬롯마다 32비트, 48 kHz, FMT 5. BCK GPIO65, LRCK GPIO4, 출력 GPIO62, 입력 GPIO5, 함수 5 |
| 스피커 자리 | 슬롯 0 = 0x35 왼쪽 아래, 슬롯 1 = 0x37 오른쪽 아래, 슬롯 2 = 0x34 왼쪽 위, 슬롯 3 = 0x36 오른쪽 위. 사용자가 든 방향 기준이고, 방향은 기록하지 않았다 |
| 스피커 보정 | `persist`의 `factory/aw_cali.bin` `6403,7128,7023,6824,`(mΩ), `aw_cali_f0.bin` `675,629,644,644,`(Hz). 앰프 차례는 순정 트리의 `sound-channel`과 같다 |
| 마이크 | AIN0, AIN2, AIN3에 신호가 있고 AIN1은 바닥값이다(프리앰프 12 dB에서 실효 -89.5 dBFS). 사용자가 들어 보고 AIN0을 골랐다 |
| 클럭 | APLL1 180.6336 MHz, APLL2 196.608 MHz. 코덱의 26 MHz는 MT6685 BBCK5 |
| 코덱 링크 | MTKAIF 프로토콜 2, 보정 위상 1/1/1, 주기 3/3/3. 두 번의 부팅에서 같았다 |

## 8. 남은 것

- super의 vendor는 ZIP에서 내려받으면서 풀 수 있다(`.handoff/tb375fc-pogo/wip/super_stream.py`, 5.7GB 스트리밍, 디스크는 고른 파티션만 쓴다). 2026-11-01에 이렇게 `tn_keyboard.ko`와 키보드 HAL을 읽었다. 파티션은 모두 EROFS다
- super.img의 vendor·vendor_dlkm에서 모듈과 펌웨어 파일을 뽑는다. 목록은 기기에서 읽었다(7절). 파일 자체는 root나 super.img가 필요하다(ADR-012의 재배포 분류). SP Flash Tool readback으로 super를 뜨면 `extract.sh`에 super를 풀어 `vendor/`를 만드는 단계(lpunpack 등)를 더해야 한다.
- root가 필요한 것(언락 뒤): `getevent -lp`, 살아 있는 트리의 값(framebuffer 주소·크기, pstore·BL31 자리, 메모리 배치), `/proc/iomem`, 파티션 표
- 12.1절 EL2 판정(2a 첫 부팅)
- 지문 센서의 칩과 연결(SPI 등)을 장치 트리에서 확인한다
