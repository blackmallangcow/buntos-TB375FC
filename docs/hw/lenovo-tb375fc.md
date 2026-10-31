# Lenovo TB375FC 하드웨어 실측 (Phase 0)

- 상태: 진행 중. 기기 없이 펌웨어에서 읽은 것까지 적었다(2026-11-01). 기기를 받으면 [설계안](../design/ARCHITECTURE.md) 12장의 ADB 항목과 12.1절 EL2 판정을 더한다
- 대상: Lenovo Xiaoxin Pad Pro 12.7 (2025, TB375FC). 글로벌판은 Lenovo Idea Tab Pro(TB373FU)이고, Moto Pad 60 Pro(XT2571-1)도 같은 기기로 알려져 있다

## 1. 출처와 방법

**펌웨어.** 글로벌판 순정 펌웨어 `TB373FU_ROW_OPEN_USER_M21.814_V_ZUI_17.0.04.266_ST_251120.zip`(6,114,349,108바이트, ZUI 17, 2025-11-20 빌드)이다. 레노버 공식 파일 이름이고, 커뮤니티 미러 [Lolinet](https://mirrors.lolinet.com/firmware/lenowow/2024/Idea_Tab_Pro_2024/)에서 받았다.
- 레노버 공식 도구(Rescue and Smart Assistant)는 기기 일련번호가 있어야 받을 수 있다.
- 커뮤니티는 TB375FC에 이 글로벌 펌웨어를 그대로 올려 쓴다(XDA "Flashing CN/Global rom for TB375FC, TB373FU, Moto Pad 60 Pro"). 그래서 이 문서는 TB373FU 펌웨어 기준이다.
- TB375FC 중국판 펌웨어(같은 미러의 `TB375FC_CN`, 7z 5.4~6.0GB)와 장치 트리를 견주는 일은 남았다(5절).
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
| 화면 | DSI0(`dsi@1400d000`) 비디오 모드 패널 셋 가운데 하나. BOE NT36532, Tianma NT36532, Tianma "paper"(무광) NT36532. LK가 패널을 읽어 고른다 | 노드 `panel1@0`~`panel3@2`, 모듈 `panel-nt36532-dsi-vdo-{boe,tianma,tianma-paper}.ko` | 패널 드라이버 셋을 새로 써야 한다. 해상도와 주사율(공개 사양 2944×1840, 144Hz)은 펌웨어에서 확인하지 않았다 |
| 터치 | Novatek NT36532. 화면 구동 칩에 터치를 넣은 TDDI다. SPI2(`spi2@11012000`) CS0, 9.6MHz, 리셋 GPIO 60, 인터럽트 GPIO 9, 감지 채널 18×36, 펜 지원(`novatek,pen-support`) | `compatible = "novatek,NVT-ts-spi"`, 모듈 `nt36532.ko` | **동시 10손가락.** 드라이버가 `max_touch_num`에 10을 넣고 `input_mt_init_slots(dev, 10, 0)`을 부른다(역어셈블). 업스트림 `novatek-nvt-ts`는 I2C 칩용이라 SPI로 붙은 이 칩에는 바로 쓸 수 없다(확인 필요) |
| 외부 화면 | DP 출력(`dp-tx@11e10000`). USB-C DP Alt 모드, Parade PS5170 리드라이버 | `mediatek,mt6897-dp_tx`, `parade,ps5170` | |
| GPU | Arm Mali-Valhall(`mali@13000000`). 전원과 주파수는 GPU 내장 컨트롤러 gpueb(펌웨어 `gpueb.img`)와 gpufreq가 다룬다 | `mediatek,mali`, `arm,mali-valhall` | Panthor + Mesa가 GPU를 지원한다. MT6897 쪽 전원·주파수(gpueb) 통합이 포팅 범위다(4.2.1-3) |
| 저장장치 | UFS(`ufshci@112b0000`), MediaTek UFS PHY | `mediatek,mt8183-ufshci`, 모듈 `ufs-mediatek-mod.ko`, `phy-mtk-ufs.ko` | 업스트림 `ufs-mediatek`이 받는 compatible이다 |
| USB | mtu3 + xHCI(`usb0@11201000`). USB-C는 MT6375의 TCPC, Type-C 먹스, USB·DP 고르기 | `mediatek,mtu3`, `mediatek,mtk-xhci`, `mediatek,mt6375-tcpc` | mtu3·xhci-mtk는 업스트림에 있다. MT6375 TCPC는 확인 필요 |
| PMIC | MT6363(주, SPMI), MT6368(보조, 오디오 코덱·이어폰 감지), MT6319 ×3(SPMI 벅), MT6685(RTC·클럭 버퍼), MT6375(충전기·TCPC·ADC, I2C 0x34) | `mediatek,mt6363` 등 | 확인 필요(2b) |
| 충전 | MT6375 충전기, 차지 펌프 Nuvolta NU2115 ×2와 Southchip/Unisemi UPM6720 ×2(고속 충전, PD·PE) | `nuvolta,nu2115`, `southchip,upm6720` | |
| 배터리 | 게이지 Mitsumi MM8013(I2C 0x55) | `mediatek,mm8013`, 모듈 `mm8013a_gauge.ko` | |
| 오디오 | MT6368 코덱, Awinic AW883xx 스마트 앰프 ×4(스피커 넷, I2C 0x34~0x37), FSA4480(USB-C 이어폰 스위치) | `awinic,aw883xx`, `fsa,fsa4480-i2c` | |
| 무선 | MediaTek connsys 통합 Wi-Fi·Bluetooth·GPS(`wifi@18000000`, `bt@18000000`, `gps@18c00000`). 펌웨어 `connsys_wifi.img`, `connsys_bt.img`, `connsys_gnss.img` | `mediatek,wifi`, `mediatek,bt`, `mediatek,mt6897-gps` | 외장 칩이 아니라 SoC 통합형이다. 휴대기기용 connsys는 업스트림 드라이버가 없는 것으로 안다(4.2.1-4, 2d에서 확인) |
| 센서 | 센서 허브 SCP(`scp@1c400000`, 펌웨어 `scp.img`), 홀 센서 둘(`hall_1`, `hall_2`: 키보드 커버), SAR 센서 TYHX HX9036·HX9031AS | `mediatek,scp`, `tyhx,hx9031as` | 가속도·조도 센서는 SCP 뒤에 있는 것으로 보인다(확인 필요) |
| 키보드 커버 | 포고 핀 키보드(Tinno), 키보드 전원 GPIO | `tinno,tn_keyboard`, `mediatek,keyboard_gpio` | |
| 카메라 | 이미지 센서 둘(I2C 0x10 두 곳), AF GT9772B, EEPROM 둘, 플래시 OCP811 | `mediatek,imgsensor` | 범위 밖 |
| 진동·지문 | `regulator-vibrator`와 Goodix 지문(`mediatek,goodix-fp`, `mediatek,finger-goodix`) 노드가 켜져 있다 | 장치 트리 | MediaTek 참조 보드의 노드가 남은 것일 수 있다. 실물은 기기에서 확인 |
| 보안 | GenieZone 위의 Trusty·Nebula, Trustonic Kinibi(`trustonic,mobicore`), Microtrust(`microtrust,utos`) 노드가 켜져 있다 | 장치 트리 | buntOS에는 필요 없다 |

## 5. GenieZone과 가상화 (12.1절 예비)

- 펌웨어에 GenieZone 하이퍼바이저 이미지 `gz.img`(3,883,056바이트)가 있다.
- SoC 장치 트리에 `hypervisor { compatible = "mediatek,geniezone-hyp"; status = "disabled"; }`가 있다. 보드 오버레이를 입혀도 꺼져 있다. 부트로더가 부팅 때 켜는지는 펌웨어만으로는 알 수 없다.
- GenieZone 위에서 도는 TEE(Trusty `trusty-gz-log`, Nebula `nebula-gz-log`)가 켜져 있다.
- **예비 판정:** GenieZone이 EL2를 차지할 가능성이 높다("불가" 쪽). 그러면 리눅스는 EL1로 시작하고 KVM을 쓸 수 없다. 대신 GenieZone의 VM 인터페이스(`gzvm`)가 있다. 최종 판정은 2a 첫 부팅의 커널 기록(EL2로 시작했는지)으로 한다.

## 6. 남은 것

- TB375FC 중국판 펌웨어(7z, 통째로 받아야 한다)의 장치 트리와 견줘 글로벌판과 같은 하드웨어인지 확인한다(ADR-011).
- super.img의 vendor·vendor_dlkm에서 모듈 전체와 펌웨어 목록을 뽑는다(압축 5.97GB, ADR-012의 재배포 분류).
- 기기를 받은 뒤 할 것:
  - 12장의 ADB 항목: `getevent -lp`, 디스플레이 모드, 메모리, 전원, 파티션
  - 12.1절 EL2 판정
  - 진동 모터와 지문 센서가 실제로 있는지 확인
