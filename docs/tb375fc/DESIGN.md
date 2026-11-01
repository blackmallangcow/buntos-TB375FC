# buntOS TB375FC 포팅 최종 설계안 v1.10

- 작성일: 2026-11-01
- v1.1(2026-11-01): 커널 커스텀과 부트 이미지 도구를 만든 뒤 2.3절(2a 장치 트리를 업스트림 꼴로 시작)과 2.4절(`fastboot boot`와 헤더 v4)을 고쳤다
- v1.2(2026-11-01): 오픈소스 쪽 조사(부트 체인, 커뮤니티 포트, SoC 기반, 업스트림 진행, Wi-Fi)를 반영했다. 언락 go/no-go 넷 가운데 셋이 채워지지 않는다(2.1절). UFS·USB에 전원 도메인이 없어 2b에서 뺐다(3절). Wi-Fi·BT 펌웨어는 LK가 싣는다(6·7절)
- v1.3(2026-11-01): 제품 라인을 나눴다(0절). buntOS 원본은 데스크톱 라인이고, 이 저장소는 태블릿 라인의 첫 포크다. XDA 언락 글타래를 브라우저로 확인해 언락 길과 go/no-go를 고쳤다(2.1절. v1.2의 "원글을 찾지 못했다"는 틀렸다). 다음 작업을 다시 적었다(11절)
- v1.4(2026-11-01): 사용자의 기기(중국판 ZUXOS 1.5.10.044, Android 16)에 맞는 언락 정보를 찾아 2.1절과 11절을 고쳤다. v1.3에 적은 "16.0.20.091에서 실패한 보고"는 잘린 글만 읽은 것이었다. 그 글은 시스템으로 한 번 부팅하고 fastboot로 다시 들어가 언락에 성공했다
- v1.5(2026-11-01): 11절의 기기 없이 할 일 다섯을 끝냈다. `pinctrl-mt6897`과 pio 노드, SPMI 고침과 M 버스·MT6319, 이미지용 설정 확인, 포크 정리, buntOS 합치기 차례다. 코드 대조로 찾은 SPMI 결함 둘은 고침을 대기열에 얹었다(4절). 다음 작업을 다시 적었다(11절)
- v1.6(2026-11-01): `clk-mt6897`(패치 0009~0013)과 클럭 컨트롤러 노드를 더했다. 클럭 컨트롤러는 2b까지 끄고, 2a 콘솔(uart0)은 새 클럭 드라이버를 기다리지 않는다(11절)
- v1.7(2026-11-01): 이미지용 커널의 소스를 kernel.org 원본으로 정했다(D1, 4절). Debian 소스 패키지(패치 77개)에는 MediaTek 관련이 없고 차이는 보안 기본값과 패키징이라 쓰지 않는다. 지금까지의 패치 대기열·장치 트리·조각·`debian-config.sh`는 그대로 쓴다. 이미지 빌드에서 정할 것을 4절과 11절에 적었다
- v1.8(2026-11-01): 2b의 PMIC와 UFS를 세션 둘로 나눠 마쳤다(패치 0014~0022). PMIC는 MFD `mtk-spmi-pmic`(업스트림 v12 백포트와 인터럽트 고침), MT6363 키, 보드의 MT6363과 UFS 공급이다. UFS는 바인딩·드라이버와 UFS·USB 노드이고, 클럭·리셋 이름과 OPP는 업스트림 MT8196 시리즈(v14, 병합 전) 꼴을 따랐다. 이미지 커널의 디버그 정보는 Debian처럼 켠다(4절). 부트 명령줄에 `regulator_ignore_unused`를 더했다(D4)
- v1.9(2026-11-01): MT6368(패치 0023~0026)과 보드의 MT6368·USB 공급을 더했다. 업스트림 MT6373 v12와는 레일·레지스터가 달라 7.2.12 `mt6363-regulator`를 틀로 새로 썼다. 7.2.12 MT6363 regulator의 16비트 regmap 결함을 찾아 다음 차례에 적었다(5·10·11절)
- v1.10(2026-11-01): 이미지용 커널 빌드 단계(`build.sh image-config`·`image`)와 mkosi 보드 프로필을 만들고 한 번 빌드했다(4절). 2c(1차) 입력의 NT36532 SPI 터치·펜과 홀 센서(패치 0027~0031)를 더했다(5절). initrd 길과 루트 파티션 자리는 남은 결정이다(4·11절)
- 상태: 확정(기기를 받기 전). 기기 실측과 첫 부팅 결과로 고친다
- 대상: Lenovo Xiaoxin Pad Pro 12.7 (2025, TB375FC). 같은 하드웨어인 Lenovo Idea Tab Pro(TB373FU)도 같은 BSP로 지원한다
- 관련 문서:
  - [포팅 재료 조사](porting-materials.md)
  - [Wi-Fi·Bluetooth soc7_0](wifi-soc7_0.md)
  - [커뮤니티 포트 분석](community-port.md)
  - buntOS의 [하드웨어 실측](../hw/lenovo-tb375fc.md)과 [설계안](../design/ARCHITECTURE.md)(4.2절, 12장)

## 0. 한눈에

**목표.** TB375FC에서 buntOS(mainline 커널, Debian, buntOS 데스크톱)를 태블릿 PC로 쓴다. 터치 10점, 펜, 키보드 커버, Wi-Fi, 배터리·충전, 서스펜드까지 쓸 수 있어야 한다.

**제품 라인.** buntOS 원본은 x86-64·arm64 데스크톱 라인(GPU 가속)이고, 태블릿은 원본의 arm64를 포크해 기기마다 최적화하는 태블릿 라인이다(2026-11-01 결정, buntOS 결정 자료 ADR-013). 이 저장소는 태블릿 라인의 첫 포크다.
- 공용 컴포넌트(컴포지터, 셸, 시스템 설정, 로그인 화면, 에이전트)는 원본에서 받아 합친다(원격 `buntos`).
- 이 포크에는 기기 쪽만 둔다: 커널 커스텀, 장치 트리, BSP, 부트 이미지, 펌웨어 추출, 기기 맞춤 최적화(8절). 공용 고침은 원본에 먼저 낸다.
- arm64만 빌드한다. x86-64 이미지와 CI는 원본의 몫이다.
- 터치 입력과 화면 키보드 같은 공용 기능은 원본에서 온다. 이 기기에 맞추는 것(가장자리 띠, 펜, 키보드 덮개, 회전)은 여기서 한다.

**하드웨어.** 펌웨어 두 판(글로벌 ZUI 17, 중국 ZUI 16)에서 직접 읽었다.

| 항목 | 내용 |
|---|---|
| SoC | MediaTek MT6897 (Dimensity 8300, 태블릿판 이름 MT8792). A510 ×4 + A715 ×4, PSCI 1.0, GICv3, Mali-G615 MC6 |
| 화면 | Novatek NT36532 패널 셋(BOE, Tianma, Tianma 무광) 가운데 하나. 2944×1840, **DSI 두 줄(dual-DSI) + DSC**, 144/120/60/30Hz |
| 터치 | 같은 NT36532 칩의 터치(TDDI), SPI, 동시 10손가락, 펜. 플래시가 없어 부팅마다 펌웨어를 내려 준다 |
| 저장장치·USB | UFS(`mediatek,mt8183-ufshci`, 업스트림이 MT6897 IP 판을 안다), mtu3 + xHCI + xsphy, USB-C(MT6375 TCPC), DP Alt |
| 전원 | PMIC MT6363·MT6368·MT6319 ×3·MT6685·MT6375. 차지 펌프 NU2115 ×2·UPM6720 ×2, 게이지 MM8013 |
| 무선 | connsys soc7_0(CONNAC2X, 칩 ID `0x7961` = MT7921 계열) + RF 칩 MT6637. 펌웨어는 LK가 싣고, 보정값은 nvdata 파티션에 있다 |
| 그 밖 | 앰프 AW883xx ×4, 홀 센서 둘, 센서 허브 SCP(가속도·조도), UART 포고 키보드(Tinno MCU), SAR 둘 |
| 부팅 | LK가 boot v4 + vendor_boot v4(장치 트리는 DT 표) + dtbo를 읽는다. GenieZone이 EL2를 차지할 가능성이 높다 |

**가장 큰 재료 넷**([조사](porting-materials.md)).
1. 이 기기의 벤더 모듈 소스 미러(`mt8792/android_kernel_lenovo_mt8792-modules`, 6.1.138): 패널·터치·앰프·홀·포고 키보드 드라이버가 그대로 있다.
2. 이 기기의 커뮤니티 mainline 포트(`u0n0u/tb375fc-linux`, Linux 7.2): LK가 켠 화면(fbcon)과 UFS, USB 가젯까지 띄웠다. UART는 쓰지 않는다. 터치·Wi-Fi·GPU·충전은 작업 중이고, 2026-09-23 뒤로 멈췄다([분석](community-port.md)).
3. 같은 SoC의 완전한 벤더 트리: OPPO Pad 3·OnePlus Pad(같은 MT8792 태블릿 보드), Motorola(가장 새것). 클럭·전원 도메인·핀·SPMI 표가 셋 모두 같은 파일로 들어 있다.
4. 업스트림 견본: MT8196(clk·pmdomain·SPMI), pinctrl-mt6878, MT6895-Mainline(PMIC 조합이 같다), MT6878-mainline(최소 부팅), mainline에 이미 있는 NT36532 패널·panthor G615

**가장 큰 일과 위험.**
- 큰 일:
  - SoC 기반(클럭·전원 도메인·핀)
  - 화면 파이프라인(dual-DSI + DSC, 업스트림에 없다)
  - Wi-Fi(soc7_0)
  - 오디오(AFE + MT6368)
- 위험:
  - 언락: 커뮤니티의 길(개발용 LK)이 ZUI 16의 판에 묶여 있고, 그 LK의 출처를 확인할 수 없다(2.1절)
  - EL2 점유(가상화 불가)
  - 센서 허브(자동 회전이 막힐 수 있다)
  - 펌웨어 재배포

## 1. 핵심 결정

| # | 결정 | 까닭 |
|---|---|---|
| D1 | **mainline 커널.** 소스는 kernel.org 원본이고, 판은 buntOS 이미지의 Debian forky 커널과 같다(지금 7.2.12). 여기에 제출 대기 패치를 더한다. 개발 빌드는 arm64 defconfig에, 이미지용은 같은 시점 Debian forky 커널 설정에 보드 조각을 얹는다. Debian 소스 패키지의 패치는 쓰지 않고, 필요한 것만 골라 대기열에 넣는다. 벤더 GKI는 쓰지 않는다 | buntOS 설계안 4.2.2절. 필요한 업스트림 기능(6.16~7.0)이 7.2에 다 있다. 7.4 병합 창에 들어온 패널 드라이버와 SPMI 고침만 백포트한다. 원본 소스이면 개발 빌드와 이미지가 같은 트리이고, 시험한 기반이 업스트림과 같아 제출이 쉽다. Debian `linux 7.2.12-1`의 패치 77개(버그 고침 28, 기능 14, Debian 전용 35)에는 MediaTek 관련이 없고, 차이는 보안 기본값과 패키징이다. Debian 패키징으로 빌드하려면 forky 툴체인과 큰 디스크가 든다(디버그 패키지만 약 7.2GiB). 2026-11-01에 견줘 정했다 |
| D2 | **업스트림 꼴로 짠다.** `mt6897.dtsi` + `mt6897-lenovo-tb375fc.dts`. 핀은 업스트림 `pinctrl-mt6878`을, clk·pmdomain·SPMI는 MT8196을 틀로 삼는다. MT6897 데이터는 OEM 공식 소스(OnePlus·OPPO·Motorola, GPL-2.0)에서 가져온다 | 제출할 수 있어야 유지할 수 있다. 커뮤니티 포트는 클럭·핀 드라이버 없이 fixed-clock과 부트로더 상태에 기대고, 장치 트리를 커널에 넣어 LK가 준 트리를 바꿔 끼우는 구조다. 그래서 사실 확인용과 협력 상대로 쓰고, 기반으로는 쓰지 않는다([분석](community-port.md)) |
| D3 | **처음에는 LK가 직접 띄운다.** boot v4 + vendor_boot v4(DT 표) + 빈 dtbo, `fastboot boot`로 RAM에서. 2b부터는 U-Boot 체인로드로 EFI(systemd-boot, UKI) 부팅을 검토한다 | 파티션을 건드리지 않는 첫 부팅. EFI가 되면 원본 buntOS(데스크톱 라인)의 arm64 이미지 꼴과 같아진다. MT6878-mainline도 U-Boot를 권한다 |
| D4 | **첫 데스크톱은 simple-framebuffer다.** LK가 dual-DSI + DSC로 켜 둔 화면을 그대로 받는다(`clk_ignore_unused`, `pd_ignore_unused`, `regulator_ignore_unused`). 화면 파이프라인(mediatek-drm의 MT6897 데이터, dual-DSI, DSC)은 업스트림 시리즈를 따라 뒤에 한다 | 화면이 가장 큰 막힘이다. 그 사이에 나머지(저장장치, USB, 터치, Wi-Fi, GPU)를 진행할 수 있다 |
| D5 | **Wi-Fi는 mt76에 "MT7921의 SoC 변형"을 더한다.** 펌웨어는 LK가 실은 것을 쓰고, conninfra 전원은 TF-A SIP 호출로 켜고, nvdata 보정값은 NVRAM 명령으로 넘긴다. 그 전에는 USB 동글로 우회한다 | 칩 ID와 구조가 MT7921과 같고, 명령 머리와 레거시 명령 번호도 같다. SoC Wi-Fi 선례(mt7915/soc.c)가 있다([설계](wifi-soc7_0.md)) |
| D6 | **펌웨어는 설치할 때 기기에서 뽑는다.** Mali CSF만 linux-firmware에서 온다. GPUEB와 Wi-Fi·BT는 LK가 올리므로, 그 파티션(GPUEB, `connsys_wifi`, `connsys_bt`)을 지킨다 | 재배포할 수 있는지 모른다(buntOS ADR-012). 안드로이드 슬롯 하나를 남긴다 |
| D7 | **GPU는 panthor + MT6897 GPUEB 변형이다.** 이미지에 Mesa 26.3 이상이 필요하다 | panthor·펌웨어·Mesa가 이미 G615를 안다. 전원은 MT8196 드라이버에 공유 메모리 오프셋만 다른 변형을 더한다 |
| D8 | **가상화는 2a에서 EL을 본 뒤 정한다.** GenieZone이 EL2를 차지하면 KVM을 포기하고, gzvm(미병합)을 나중에 검토한다. TEE를 고쳐 EL2를 빼앗는 실험(`mtk-mod-tee-nogz`)은 쓰지 않는다 | 서명 우회가 필요하고 벽돌 위험이 크다 |

## 2. 부팅 길

### 2.1 부트 체인과 언락

- **부트 체인:** BROM → preloader → ATF·TEE·GenieZone → LK → 커널. LK는 AVB로 boot, vendor_boot, dtbo를 검증한다.
- **언락 방식:** 길이 둘이다.
  - 순정 LK의 길: 개발자 옵션의 OEM 잠금 해제, 일련번호에 묶이고 레노버가 서명한 언락 이미지, `fastboot oem unlock`(또는 `flashing unlock`)이다(LK 문자열. 글로벌판과 중국판 LK가 같다). 레노버가 이 모델에 언락 파일을 주는 길은 찾지 못했다.
  - 커뮤니티의 길(XDA 원글, 2025-02): 중국판 ZUI 16의 순정 LK가 `fastboot flash lk`를 받는다. 개발용 펌웨어 덤프에서 꺼낸 LK(`lk_a`)를 구워 `fastboot flashing unlock_critical`로 언락 파일 없이 언락하고, 순정 LK로 되돌린다. AVB는 vbmeta 셋을 `--disable-verity --disable-verification`으로 다시 구워 끈다.
- **언락 보고:** XDA 글타래 둘(언락, 롬 바꾸기)과 TrebleDroid 위키가 있다. 2026-11-01에 브라우저로 원문을 확인했고, 모두 포럼 보고다.
  - 원글 작성자와 다른 사용자 여럿이 ZUI 16.0.20.022·.041에서 성공했다(2025-02). TrebleDroid 위키는 16.0.20.091에서 언락한 뒤 GSI를 띄웠다(2025-08). 2026-06에는 이 기기용 LineageOS 23.2가 나왔다.
  - 판마다 결과가 갈린다.
    - 16.0.20.091에서도 성공 보고가 있다(2025-09, 2026-09). 2026-09 보고는 개발용 LK를 구운 뒤 시스템으로 한 번 부팅하고 fastboot로 다시 들어가야 언락됐다고 적었다.
    - 실패 보고도 있다. 256GB 기기가 16.0.20.130에서 .091로 내린 뒤, 개발용 LK와 sn.img가 모두 실패했다(2025-12).
    - 글로벌판 17.0.04.184(Android 15)에서 내리지 않고 개발용 LK로 언락한 보고가 있다(2025-10).
  - ZUXOS(중국판 Android 16, 1.5.10.x)에서 바로 언락한 보고는 없다. ZUXOS 1.5.10.103 기기를 SP Flash Tool로 16.0.20.091까지 내린 뒤 언락한 보고가 있다(2026-09).
  - 레노버가 발급한 sn.img와 개인이 만든 sn.img 웹 도구는 이 기기에서 인증에 실패했다는 보고가 둘 있다(2025-09, 2025-12).
  - Android 16 판(ZUXOS 1.5.10.103 이후, 글로벌 17.5.10.095)에서 다른 판의 이미지를 구우면 "Red State"로 부팅하지 못한다는 보고가 여럿이다. 1.5.10.103부터 LK가 글로벌 이미지를 거부한다는 글도 있으나, 작성자가 AI로 썼다고 밝힌 글이라 따로 확인해야 한다.
  - 개발용 LK는 4pda에 올라온 덤프에서 나와 출처를 확인할 수 없다. 남이 고친 부트로더를 굽는 위험이 있다.
  - v1.2에 적은 "XDA 원글을 찾지 못했다"는 틀렸다. 사이트가 자동 접근을 막아(403) 앞선 조사에서 열지 못한 것이다.
  - 커뮤니티 mainline 포트 둘은 언락 절차를 적지 않았다.
- **슬롯:** 한쪽 슬롯과 recovery는 안드로이드로 남긴다. 정보원이자, 펌웨어를 뽑는 곳이자, 복귀 길이다.

**go/no-go.** 진행 조건은 buntOS 설계안 4.2.3절의 넷이다. 하나라도 빠지면 언락하지 않는다. 2026-11-01 조사로는 넷 모두 길이 보이지만, 모두 기기에서 확인해야 한다. 근거는 [조사](porting-materials.md) 5절에 있다.

| 조건 | 지금 | 까닭 |
|---|---|---|
| 1. 전체 파티션 백업과 해시 확인 | 길이 있다. 기기에서 확인 | mtkclient로는 안 된다(V6 DA에 DAA·SLA). 대신 순정 펌웨어 묶음의 인증 DA·auth와 SP Flash Tool로 readback한다. XDA 원글 작성자가 이 기기용 scatter와 readback 설정을 만들었다(포럼 보고). 우리가 본 중국판 묶음에도 인증 DA(`DA_BR.bin`), `da.auth`, SLA 파일이 있다. scatter와 readback은 기기 용량(128·256GB)에 맞춰야 한다 |
| 2. 독립된 보고 둘 이상과 판 일치 | 보고는 여럿이다. 판 확인이 남았다 | 위 "언락 보고". 판마다 결과가 달라, 언락할 판(예: 16.0.20.091)으로 된 성공 보고를 확인한다. ZUXOS 기기는 그 판으로 내린 뒤다 |
| 3. BROM·preloader 복구 길 | 있다(포럼 보고). 기기에서 확인 | mtkclient는 BROM 익스플로잇이 없다. SP Flash Tool과 순정 묶음의 인증 DA·auth로 굽고 되살린 보고가 여럿이다. 중국판 백업을 되돌린 뒤 OTA가 되고 다시 잠글 수 있었다는 보고도 있다 |
| 4. OTA 차단 | 기기에서 한다 | 언락 길이 ZUI 16에 묶여 있으니 받자마자 끈다 |

- 언락 순서:
  1. 받은 기기의 ZUI 판을 적고 OTA를 끈다.
  2. SP Flash Tool readback으로 전체 백업을 뜨고, 다른 기기에 옮겨 해시로 확인한다.
  3. 그 판의 성공 보고를 확인한다.
  4. 개발용 LK를 순정 LK와 견준 뒤 언락하고, 바로 순정 LK로 되돌린다.
- ZUXOS(Android 16) 기기는 SP Flash Tool로 중국판 ZUI 16.0.20.091까지 내린 뒤 언락한다(포럼 보고 하나). 내리기 전에 readback 백업이 먼저다. preloader는 굽지 않는다(구우면 벽돌이 된다는 보고가 있다). 글로벌 ROM을 중국판 기기에 구워 부팅이 반복된다는 보고("Red State")도 있다.
- SP Flash Tool은 Windows 도구다. 인증 DA·auth는 그 기기 판의 레노버 펌웨어 묶음에 든 것을 쓴다.
- nvdata·nvram 계열에는 Wi-Fi MAC과 보정값이 있다. 잃으면 되살릴 수 없다.

### 2.2 이미지 꼴

| 이미지 | 넣는 것 | 메모 |
|---|---|---|
| boot | 헤더 v4, mainline 커널(Image, LZ4 legacy) | 원래 커널도 LZ4 legacy다 |
| vendor_boot | 헤더 v4, DTB 영역에 **DT 표**(magic `0xd7b7ab1e`) 꼴로 우리 장치 트리, 명령줄 | MT6897 LK는 DTB를 DT 표에서 찾는다 |
| init_boot | 헤더 v4, 우리 initramfs(USB 가젯, nvdata 마운트) | LK가 initrd로 넘긴다 |
| dtbo | 빈 오버레이 하나 | LK가 dtbo를 기본 트리에 입힌다. 벤더 dtbo를 남기면 벤더 노드 경로를 기대하는 fixup이 실패할 수 있다 |

- 이미지 만들기는 이 폴더의 `boot/`에 둔다. buntOS `tools/hw/bootimg.py`의 반대 방향으로, `mkbootimg` v4 꼴과 DT 표를 그대로 따른다.
- **LK와 `/chosen`:** 커뮤니티 포트에 그 우회가 있다(코드로 확인). 커널에 넣은 장치 트리로 LK가 준 트리를 바꿔 끼우고, 그 트리의 `/chosen`에서 명령줄을 다시 읽는다([분석](community-port.md) 4절).
  - 우회하는 까닭은 "LK가 boot·vendor_boot 명령줄을 무시하고 `/chosen`을 덮어쓴다"는 것이다. 다만 MT6895 xaga 포트에서 물려받은 주석이고, MT6897 LK에 vendor_boot DT 표로 트리를 넣어 시험한 기록은 없다.
  - 이 우회는 LK가 더하는 메모리 크기, mblock 예약, bootargs를 버린다. 그래서 커뮤니티 포트는 12GiB 기기의 예약을 고정값으로 적었고, 8GB 판에는 맞지 않는다.
  - 우리는 커널 명령줄을 boot·vendor_boot 이미지의 명령줄로 준다. 2a에서 `/proc/cmdline`으로 들어왔는지 보고, 모자라면 커널 `CMDLINE`과 `CMDLINE_EXTEND`를 쓴다.
  - DTB를 커널에 넣는 우회는 기본 길로 삼지 않는다(2.4절의 시험용 커널만 예외).
  - 2a에서 LK가 우리 트리의 무엇을 고치는지 살아 있는 트리로 확인한다.

### 2.3 장치 트리 방침

- **2a:** 처음부터 업스트림 꼴로 짠다(v1.0의 "벤더 트리를 줄여 출발"을 바꿨다). 커뮤니티 포트가 이 꼴의 트리를 커널에 넣어 LK에서 화면 콘솔과 저장장치까지 띄웠다. 주소와 인터럽트는 커뮤니티 포트의 `mt6897.dtsi`, 벤더 `mt6897.dts`, 우리가 뽑은 트리 셋을 대조한다(`kernel/tb375fc/dts/README.md`).
  - LK가 찾는 패널 경로(`/soc/dsi@1400d000/panel1@0` 등)는 화면을 붙이는 2c에서 더한다.
  - 첫 부팅 뒤 LK가 우리 트리의 무엇을 고치는지 살아 있는 트리로 견준다.
- **2b부터:** 꺼 둔 노드(UFS, USB, SPMI 등)를 핀·클럭 패치와 함께 하나씩 켠다. UFS와 USB에는 전원 도메인이 없다(펌웨어 트리, 벤더 scpsys 표).
- **최종 꼴:** `mt6897.dtsi`(SoC)와 `mt6897-lenovo-tb375fc.dts`(보드)다. TB373FU는 같은 dts에 `model`만 다르다.
- **framebuffer:** 정적 트리에 없다. LK가 부팅 화면을 켠 영역을 기기에서 읽는다(안드로이드의 `/proc/iomem`, 살아 있는 트리). 그 주소로 `simple-framebuffer`를 만든다. 화면은 1472×1840 두 줄을 합친 2944×1840이고, LK가 DSC까지 켜 둔다.
  - 커뮤니티 포트는 LK 기록(mblock 할당, `atag,videolfb`)에서 읽은 `0xfc16f000`으로 fbcon을 띄웠다(12GiB 기기). 8GB 판에서도 같은지 기기에서 본다.
- **예약 메모리:** 정적 트리에는 동적 할당 셋(modem 버퍼, CMA 256MB, 커널 보호)뿐이다. TEE, GenieZone, connsys, framebuffer의 고정 예약은 preloader·LK가 부팅 때 더한다.
  - 그래서 우리 트리에도 `/reserved-memory`(셀 2)를 남긴다.
  - LK가 예약을 더하지 않는 길(커널에 넣은 장치 트리 등)에서는 BL31(`0x48800000`, 2MB)을 직접 예약해야 한다. 커뮤니티 포트에 따르면, 안 하면 보조 CPU와 서스펜드가 멈춘다.
  - 기기에서 실제 배치를 본 뒤 pstore 칸을 정한다.

### 2.4 처음 띄우는 법과 디버그 채널

- 첫 부팅은 `fastboot boot`로 RAM에서 한다. LK의 fastboot 명령 표에 `download:`와 `boot`가 있어 이 길을 지원한다(LK 문자열). 서명하지 않은 이미지라 언락 상태여야 한다.
  - `fastboot boot`는 boot.img 하나만 RAM으로 보낸다. 헤더 v4에서 장치 트리는 vendor_boot에, initramfs는 init_boot에 있다(AOSP 문서).
  - LK는 boot·init_boot·vendor_boot 머리를 모두 읽고, 장치 트리를 예약 메모리 `mb_dtb`로 읽어 커널에 넘긴다(LK 문자열). 그래서 RAM으로 보낸 boot.img에 굽혀 있는 원래 vendor_boot·init_boot를 붙일 가능성이 높다. 이 결합만 기기에서 확인하지 않았다.
  - 그래서 init이 vendor_boot의 표지로 LK가 우리 vendor_boot를 읽었는지 알린다. initramfs를 boot.img에도 넣은 이미지(`mkimages.sh --boot-ramdisk`)로 LK가 그 램디스크를 쓰는지도 본다.
  - 원래 장치 트리로 뜨면 비운 슬롯에 넷을 굽는 길로 간다.
  - 굽기 전에 한 번 더 보려면, 커뮤니티 포트처럼 장치 트리를 커널에 넣은 시험용 커널을 `fastboot boot`로 띄울 수 있다. 이 커널은 LK가 더하는 메모리·예약 값을 버리므로, 그 기기의 메모리 크기에 맞춘 예약을 함께 넣어야 한다. 업스트림에는 내지 않는다.
- 디버그 채널:
  1. **화면 콘솔:** simple-framebuffer + simpledrm. 커뮤니티 포트도 LK 화면 위의 fbcon을 첫 통로로 썼다. 부팅 단계마다 화면에 블록을 그리는 카나리와 `panic=0`(패닉 화면을 남긴다)도 쓸 수 있다
  2. **pstore/ramoops:** 직전 부팅의 커널 기록을 남긴다. 벤더와 같은 자리(LK가 명령줄로 주는 `0x48090000`)에 두고, 순정 recovery로 재부팅해 읽는다
  3. **LK 기록:** expdb 파티션에 남는다. 순정 recovery에서 뜬다
  4. **USB 가젯(2b):** configfs ACM + NCM. initramfs 첫 단계에서 띄워 호스트에서 SSH로 들어간다
  5. **UART:** 커뮤니티 포트도 쓰지 않았고(uart0을 꺼 둠), 시험 패드 위치와 전압이 공개된 곳이 없다. 기기를 열어 찾는다. 포고 핀 UART3(GPIO180·181)은 키보드 줄이다.
- recovery는 따로 된 파티션이 없고 vendor_boot 안에 있다(중국판 펌웨어 묶음에 recovery.img가 없다). 그래서 우리 vendor_boot를 구운 슬롯에서는 순정 recovery를 쓸 수 없다. 남겨 둔 슬롯으로 바꿔 recovery를 쓴다.
- 루트 파일 시스템:
  - 2a는 initramfs다.
  - 2b부터는 UFS의 비운 슬롯이나 userdata 일부, 또는 USB-C 저장장치를 쓴다.

## 3. 단계별 bring-up

| 단계 | 할 일 | 종료 조건 |
|---|---|---|
| 2a 첫 부팅 | 업스트림 꼴 트리(CPU, GIC, 타이머, PSCI, 메모리, simple-framebuffer, ramoops, UART), 이미지 꼴, Debian 커널 그대로 | initramfs 셸이 화면이나 UART에 뜬다. 커널 기록으로 EL2 판정(12.1절) |
| 2b 저장장치·디버그 | UFS(`ufs-rtff-mtcmos`, 처음에는 MCQ 끔), USB(mtu3·xHCI·xsphy), 핀·EINT(mt6878 틀), 클럭(처음에는 fixed + `clk_ignore_unused`), SPMI(MT8196 데이터, P 버스 끔), MT6319(DT만), MFD·MT6363, cpufreq-hw. UFS는 LK가 켜 둔 클럭과 `regulator-fixed`로 먼저 띄울 수 있다. PMIC 쪽은 핀(EINT) → SPMI → MT6319 → MFD·MT6363 순서다 | UFS의 Debian 루트, USB 가젯 SSH, USB 호스트(동글·키보드) |
| 2c 화면·입력 (1차) | simple-framebuffer로 buntOS 데스크톱, NT36532 SPI 터치(펌웨어 내려받기), 펜, 홀 센서(gpio-keys), 포고 키보드(serdev) | 2944×1840으로 buntOS 로그인 화면, 터치·펜 조작, 키보드 커버를 붙이면 포인터 모드 |
| 2d 무선 | conninfra(SIP 호출, PMIC 전원, conn 전원 도메인), mt76 soc7_0(LK 적재본 기동), nvdata 보정값, FEM, Bluetooth(BEIF) | Wi-Fi 연결, Bluetooth 키보드·이어폰 |
| 2e GPU·전원 | panthor + GPUEB 변형, Mesa 26.3, 클럭 드라이버 완성(`clk_ignore_unused` 빼기), LVTS 열, MT6375 충전·TCPC, MM8013, 서스펜드 | GLES로 컴포지터, 서스펜드·복귀 반복, 배터리·충전 표시 |
| 2c 화면 (2차) | mediatek-drm의 MT6897 데이터, dual-DSI, DSC, NT36532 패널 셋(업스트림 드라이버 + compatible 셋) | 주사율·밝기 조절, 화면 끄고 켜기 |
| 3 최적화 | 8절, 오디오, 외부 화면 | buntOS 설계안 9.3절 스모크 시험 |

- 화면 2차를 2e 뒤로 둔 것은 업스트림의 DSC·mediatek-drm 재구성 시리즈를 기다리기 위해서다(D4). 두 시리즈 모두 7.4 병합 창에 들어가지 않아 빨라야 7.5다(2026-11-01).
- 전원 도메인은 UFS·USB에 필요 없어 2b에서 뺐다. 필요한 단계에서 그 도메인부터 더한다: 2d(conn), 2e(GPU), 2c 2차(화면).

## 4. 커널 구성

- **기반(D1):** kernel.org 원본 소스다. 판은 buntOS 이미지의 Debian forky 커널과 같고(지금 7.2.12), 포팅이 진행되면 그때의 Debian 판을 따라간다. 7.2.y stable 고침은 kernel.org에서 바로 따라갈 수 있다.
- **이미지용 커널:** 같은 원본 소스와 대기열을 이미지와 같은 시점의 Debian forky 커널 설정 + 보드 조각으로 빌드한다(`kernel/tb375fc/debian-config.sh`가 설정을 만든다). 그래서 Debian 커널과 달라지는 것은 Debian 패치에만 있는 설정 셋(binder 모듈, UEFI Secure Boot 잠금, perf 제한)과 우리 대기열·조각이다.
  - 빌드(v1.10): `build.sh image`가 자기 작업 트리와 출력(`linux-7.2.12-image`, `out-image/`)을 써서 공용 트리를 잠그지 않는다. 설정 검사는 대기열을 다 얹은 트리로 하고, 조각의 값이 하나라도 빠지면 멈춘다. 꾸러미는 `linux-image-7.2.12-tb375fc`, `linux-headers-…`, `linux-image-…-dbg`, `linux-libc-dev`이고, 판은 `7.2.12-<n>+g<커밋>`이다(n은 `kernel/tb375fc`와 `image/mkosi.conf`를 바꾼 커밋 수, 커밋하지 않은 고침이 있으면 `~dirty`). 커널은 압축하지 않은 Image를 `/boot/vmlinuz-<판>`으로 싣는다(Debian forky arm64와 같고, UKI와 LK가 그대로 쓴다)
  - 크기와 시간(6코어 빌드 VM, 0001~0022): 컴파일 약 54분, 꾸러미 약 9분. image 97.5MiB(설치 256MiB), headers 9.7MiB, -dbg 1.64GiB(설치 4.4GiB). 빌드 디스크는 작업 트리 2.4GB와 출력 17GB가 정점이라 맥 여유가 약 28GB일 때 시작한다. -dbg는 객체 파일을 지운 뒤 따로 만들고, 끝나면 출력을 지운다. 그래서 다음 이미지 빌드는 처음부터 하고, 이어 빌드는 개발 트리가 맡는다. 고친 흐름을 처음부터 끝까지 한 번에 돌리는 확인은 디스크가 날 때 한다
  - 툴체인: 빌드 VM의 trixie gcc-14로 한다. bindgen이 없어 Rust 쪽(Rust PHY 둘, 패닉 화면 QR 등)이 빠지고, gcc 15 이상의 `__counted_by`, binutils의 LSUI, 32비트 compat vDSO도 빠진다. 이 기기가 쓰는 것은 없다. Debian과 같은 컴파일러가 필요해지면 forky 컨테이너를 쓴다
  - 디버그 정보(2026-11-01 결정): Debian 설정 그대로 DWARF·BTF를 켠다(배포판의 정석). 설치하는 커널·모듈은 strip하고 디버그 기호는 `-dbg` 꾸러미로 둔다. 빌드 VM에서는 디스크가 빠듯해 디버그 섹션을 zstd로 줄인다(`DEBUG_INFO_COMPRESS=none`이면 줄이지 않고, 출력이 약 24GB가 된다). 배포 커널은 같다
  - mkosi(v1.10): 보드 프로필(`bsp/lenovo-tb375fc/mkosi.conf`)이 지역 꾸러미 디렉터리의 커널을 넣고, mkosi가 `/boot/vmlinuz-<판>`을 `/usr/lib/modules/<판>/vmlinuz`로 복사해 UKI를 만든다. 공통 레시피의 `Bootable=yes`·`Bootloader=systemd-boot`는 보드 프로필에서 바꿀 수 없다(mkosi가 보드 프로필을 먼저 읽는다). 그래서 ESP·systemd-boot·UKI가 그대로 생기고, D3의 EFI 길과 맞으므로 지금은 그대로 둔다. LK에는 루트 파티션만 굽고, 부트 이미지는 같은 꾸러미의 Image·장치 트리·모듈로 만든다
  - 남은 결정: initrd 길(권함: 우리 initramfs에 루트 전환을 더한다. mkosi의 systemd initrd는 38MB라 init_boot 8MiB에 들어가지 않는다), 루트 파티션 자리(권함: 2b는 USB-C 저장장치로 시작하고, 파티션 표를 읽은 뒤 userdata를 줄여 새 GPT 파티션을 만든다. userdata 전체·super·루프 파일은 안드로이드 슬롯을 지키지 못하거나 metadata 암호화로 열 수 없다), 기기에서 커널을 바꾸는 길(LK 부트 이미지를 다시 만들어 쓴다)
  - Debian 패치에서 골라 넣을 후보: binder 모듈(안드로이드 앱 컨테이너를 쓸 때), perf 제한(보안 기본값), 무선 규제 DB 인증서(2d, Debian이 서명한 DB를 쓸 때)
- **설정과 패치:** 보드 설정 조각(`kernel/config/tb375fc.config`)과 업스트림 제출 대기 패치 대기열(`kernel/patches/`)을 둔다. 대기열에 처음 들어갈 것:
  - NT36532 패널 드라이버 백포트(7.4 병합 창, `47b823940e38`)와 BOE·Tianma 패널 compatible
  - `spmi-mtk-pmif` 고침 백포트(패치 0002·0003): FSM 멈춤 우회(`84c72fc05f73`, 7.4 병합 창), 클럭 참조 누수(`636c4c026f47`). 코드 대조에서 나온 결함 둘은 고침을 0007·0008로 얹었다. rcs 디먹스가 SID당 8비트를 7로 나눠 처리기가 끝나지 않을 수 있고, 채널 설정이 버스 등록 뒤에 있어 내장(=y) PMIC 드라이버가 채널 없이 버스를 쓸 수 있다. 기기에서 확인한 뒤 업스트림에 낸다
  - `pinctrl-mt6897`(EINT 포함, 패치 0004~0006: 바인딩, pinmux 매크로, 드라이버). GPIO가 없는 EINT 232~249는 업스트림 MT8196처럼 가상 핀이다
  - `clk-mt6897`(패치 0009~0013): 바인딩, HW voter syscon, FENC 없는 HW voter 먹스와 PLL HW voter(업스트림 틀에 더한 것), 2b 블록 일곱의 드라이버
  - MFD `mtk-spmi-pmic`(패치 0014~0016): v12에 리뷰 지적과 인터럽트 고침(마스크 레지스터, 초기 마스크, RCS 완료, 깨우기)을 얹었다. 새 판이 나오면 따른다. MT6363 키(0017·0018)는 새로 썼다. MT6685는 범위 밖이다. MT6319는 MFD 없이 `mt6315-regulator`에 DT만 쓴다
  - UFS(패치 0019~0022): MT6897 호스트·M-PHY 바인딩과 드라이버. 클럭·리셋 이름과 OPP는 업스트림 MT8196 시리즈(v14, 병합 전) 꼴이고, 그 시리즈가 들어오면 그 위로 다시 쓴다
  - `mt6897` 전원 도메인 데이터(2d 뒤)
  - NT36532 SPI 터치와 펜(패치 0027~0031: 핀 drive-strength와 pinmux 헤더 고침, SPI 바인딩의 MT6897, 터치 바인딩·드라이버), 포고 키보드(v6 시리즈 위, compatible별 CRC 초기값)
  - conninfra(SIP 호출과 PMIC 전원), mt76 mt7921의 SoC(AXI) 변형, Bluetooth BEIF 전송
  - GPUEB MT6897 변형
- **2a에 필요한 설정**(Debian 기본에 대부분 있다):
  - `ARCH_MEDIATEK`, `SERIAL_8250_MT6577`
  - `DRM_SIMPLEDRM`
  - `PSTORE_RAM`
  - `USB_CONFIGFS_ACM`·`NCM`
- **buntOS 빌드:** `bsp/lenovo-tb375fc/`에 mkosi 보드 프로필(v1.10), 부트 이미지 만들기, 첫 부팅 initramfs, 펌웨어 추출을 둔다. 이 폴더에서 다듬은 것을 buntOS 저장소로 옮긴다.

## 5. 블록별 포팅 계획

크기는 작음(DT나 compatible만), 중간(새 드라이버나 변형 하나), 큼(여러 드라이버나 새 SoC 지원)이다. 근거와 확인 여부는 [조사](porting-materials.md) 2절에 있다.

| 블록 | 업스트림에 있는 것 | 할 일 | 크기 | 단계 |
|---|---|---|---|---|
| CPU·GIC·PSCI·타이머 | 전부 | DT | 작음 | 2a |
| CPU 주파수 | `mediatek-cpufreq-hw` | DT(도메인 셋), 더미 `cpu-supply` | 작음 | 2b |
| 핀 제어 | `pinctrl-mt6878`(틀) | `pinctrl-mt6897`(OEM 표: 핀 232, EINT 250). 벤더 전용 기능은 업스트림 mt6878처럼 `DRV_ADV`로 바꾸거나 뺀다. 패치 0004~0006과 pio 노드가 있다 | 중간 | 2b |
| 전원 도메인 | `mtk-pm-domains`(MT8196) | MT6897 데이터(SPM 33개, GPU 12개, 그 가운데 HW voter 7개). UFS·USB에는 없다 | 중간 | 2d·2e·2c(2차) |
| 클럭 | `clk-mt8196-*`(틀) | `clk-mt6897`(OEM 표). FENC 없는 HW voter 먹스와 PLL HW voter를 더한다. 2b 블록은 패치 0009~0013에 있다 | 큼 | 2b~2e |
| SPMI·MT6363·MT6319 | `spmi-mtk-pmif`, `mt6363-regulator`, `mt6315-regulator` | SPMI는 MT8196 대체 compatible(P 버스 끔)과 고침 넷(0002·0003·0007·0008), MT6319는 DT만, MT6363은 MFD·키 패치(0014~0018). SPMI 노드, MT6363(키, 레일 23개), MT6319, UFS 공급은 장치 트리에 있다 | 작음~중간 | 2b |
| MT6368·MT6685·MT6375 | 없음 | MT6368은 패치 0023~0026(regulator 바인딩·드라이버, MFD 데이터)과 보드 노드(펌웨어가 늘 켜 두는 레일 6개, USB `vusb33`). 진동(VIBR)·지문(VFP)·오디오(VAUD18) 레일은 소비자와 함께 3에서 노드를 넣는다. MT6685·MT6375는 MT6895 포크 견본으로 새로 | 중간~큼 | 2b·2e·3 |
| UFS | `ufs-mediatek`, `phy-mtk-ufs` | 패치 0019~0022: MT6897 compatible(호스트는 match data로 `rtff-mtcmos`·`broken-rtc`·`pmc-via-fastauto`·`tx-skew-fix`), 리셋 셋, M-PHY 새 판(클럭·전원 순서 없음). VCC·VCCQ는 보드. 클럭 스케일링과 MCQ(큐 인터럽트 여덟)는 뒤로 | 작음~중간 | 2b |
| USB | `mtu3`, `xhci-mtk`, `phy-mtk-xsphy` | DT(펌웨어의 eye 값). 역할 전환은 TCPC 뒤. xsphy MT6897 꼴(U2 INTR_CAL 칸과 efuse, U3 CDR 값)과 깨우기 접점(벤더 판 103)은 드라이버 고침이 필요하다 | 작음~중간 | 2b·3 |
| 화면 파이프라인 | MT8195·MT8188까지. DSC v9·재구성 v2 시리즈는 미병합, dual-DSI는 아무도 맡지 않았다 | MT6897 데이터, dual-DSI, DSC(1.1과 패널 RC 표) | 큼 | 2c(2차) |
| 패널 NT36532 | `panel-novatek-nt36532`(7.4 병합 창). 패널 쪽 dual-DSI와 DSC가 있다 | compatible 셋, 초기화 순서. 업스트림 패널은 DSC 1.2이고 이 기기 BOE 패널은 DSC 1.1이다 | 작음 | 2c(2차) |
| 터치 NT36532 SPI + 펜 | I2C 드라이버뿐(SPI RFC 둘은 미병합) | 새 드라이버 `novatek-nvt-spi`(패치 0030·0031): 페이지 단위 SPI 접근, 열 때와 복귀 때 펌웨어 내려받기(ILM·DLM CRC 확인), 손가락 10점, 펜은 둘째 입력 장치(압력 4095, 기울기 ±60°, 단추 둘). 패널 ID GPIO184로 펌웨어를 고른다. SPI 컨트롤러 여덟은 dtsi, 터치 노드는 보드(꺼 둠) | 중간 | 2c |
| 홀 센서 | `gpio-keys` | 보드 노드(꺼 둠): GPIO1 = `SW_LID`(덮개 닫힘이 0). GPIO26 = `SW_TABLET_MODE` 후보(포고 키보드 `plug-gpios`와 같은 핀이라 기기에서 정한다) | 작음 | 2c |
| 포고 키보드 | v6 시리즈(미병합, Qualcomm 기기용) | serdev 드라이버. 같은 Tinno 단선 프로토콜로 보이나 이 기기 드라이버 소스는 없다. compatible별 CRC 초기값, TX 핀 pinctrl 전환, UART3 설정(pericfg_ao `0x11036050`) | 중간 | 2c |
| Wi-Fi·BT | `mt7921`, `mt7915/soc.c`(선례) | soc7_0 버스, conninfra, 펌웨어, FEM, BT 전송 | 큼 | 2d |
| GPU | panthor(G615), linux-firmware CSF | DT | 작음 | 2e |
| GPU 전원 | `mtk-mfg-pmdomain`, `mtk-gpueb-mailbox`(MT8196) | MT6897 변형(공유 메모리 오프셋, 메일박스 표) | 중간 | 2e |
| 열 | `lvts_thermal` | MT6897 데이터(센서 16개) | 중간 | 2e |
| 게이지 | `mm8013` | DT | 작음 | 2e |
| 차지 펌프 | 없음 | 새로(고속 충전) | 중간 | 3 |
| 오디오 | MT8196·MT8189 AFE, MT6359 코덱, AW88395·AW88399·AW88166, FSA4480 | MT6897 AFE, MT6368 코덱·accdet, 앰프 칩 확인, ACF | 큼 | 3 |
| SAR | `hx9023s` | HX9031AS를 hx9023s로 시도, HX9036 새로 | 작음/중간 | 3 |
| 지문 센서 | 모름 | 칩과 연결을 장치 트리에서 확인한다(실기기에 센서가 있고, 입력 장치 `gf-keys`가 있다) | 모름 | 3 |
| 센서 허브 | MT8183~MT8195 SCP만 | MT6897 SCP(nanohub) 길 조사 | 큼 | 3 |
| 외부 화면 | `mtk_dp`(MT8195), PS5169/70 패치 | 화면·TCPC 뒤 | 큼 | 3 |
| 비디오 코덱·카메라 | — | 범위 밖 | — | — |

## 6. Wi-Fi·Bluetooth

자세한 것은 [docs/wifi-soc7_0.md](wifi-soc7_0.md)다.

| 확인한 것 | 근거 |
|---|---|
| 내장 connsys soc7_0, 칩 ID `0x7961`(CONNAC2X), AXI·PCIe 두 빌드 | 벤더 소스 `soc7_0.h`의 `SOC7_0_CHIP_ID 0x7961`, `soc7_0.c` |
| RF 칩(A-die) MT6637 | 기기 Wi-Fi 펌웨어의 소스 경로 `wifi/base/soc7_0/hal/phy/6637/…` |
| 기기 펌웨어가 soc7_0용 | `connsys_wifi.img`·`connsys_bt.img`의 빌드 `t-neptune-main-soc7_0-2104-…-20250320113502` |
| 벤더 드라이버 소스 | gen4m `chips/soc7_0`: Xiaomi rodin(MT6899), OnePlus Pad A16, OPPO Pad 3, Motorola gen4m 저장소. Dimensity 8300 Xiaomi 갈래와 OPPO Reno13 A15에는 없다 |
| 업스트림 길 | mt76 `mt7921`이 0x7961을 PCIe로 지원하고, `mt7915/soc.c`가 SoC Wi-Fi + SPI A-die 선례다. DMA 링 배치는 MT7902 분기가 선례다 |
| 보정값 | nvdata 파티션의 `APCFG/APRDEB/WIFI`. MT6895 xaga의 initramfs는 이것을 펌웨어 폴더로 복사하고, 커뮤니티 커널은 `mediatek/mt6897/WIFI`로 읽는다(공개된 TB375FC initramfs는 nvdata를 마운트하지 않는다) |
| 펌웨어는 LK가 conninfra EMI에 싣고, 드라이버는 시작 명령(INIT 0x42)만 보낸다 | gen4m `CFG_WLAN_LK_FWDL_SUPPORT=1`(MT6897·MT6878), `fw_dl.c`의 주석 |
| conninfra 전원 순서의 대부분을 TF-A SIP 호출로 한다 | 기기 트리의 `mediatek,mt6897-consys-atf`, 벤더 `conninfra/conn_drv/connv2/platform/mt6897` |
| Bluetooth 전송은 EMI 안의 공유 메모리 링(BEIF)이고, WMT 머리는 업스트림 btmtk와 같다 | 벤더 `bt/mt66xx/btif`(`SUPPORT_BEIF=1`) |
| MCU 명령·이벤트의 머리와 레거시 명령 번호가 MT7921과 같다. uni cmd를 받는지는 모른다 | [Wi-Fi 문서](wifi-soc7_0.md) 6절 |
| 업스트림에 soc7_0 움직임은 없다 | patchwork 제목 검색, `openwrt/mt76` |

**설계.**
- 방식: mt76에 "MT7921의 SoC 변형"을 더한다. 할 일은 다음과 같다.
  - AXI MMIO 버스와 MT7902식 DMA 링 배치
  - conninfra 전원 순서(TF-A SIP 호출, PMIC 전원)
  - LK 적재본 기동: EMI 주소를 받아 remap하고 시작 명령을 보낸다. DMA는 LK가 연 20 MiB 칸 안에서만 한다
  - nvdata 보정값을 NVRAM 명령(CMD 0x48)으로 넘기기
  - FEM 고르기
  - 펌웨어가 uni cmd를 받지 않으면 BSS·STA·키·ROC를 레거시 명령으로 보내는 길
  - Bluetooth: BEIF 전송 드라이버를 새로 쓰고, HCI와 WMT는 btmtk를 다시 쓴다
- 순서: 2b의 클럭·핀·PMIC가 먼저다.
- 우회: 그 전에는 USB 동글(mt7921u)이나 USB 이더넷을 쓴다.
- 함께 일하기: 커뮤니티 포트는 벤더 gen4m과 conninfra를 옮겨 왔고, WF ROM이 아직 뜨지 않는다. 코드는 겹치지 않으므로 레지스터 사실만 나눈다([분석](community-port.md)). MT6878도 soc7_0이라 그 mainline 포트와도 나눌 수 있다.

## 7. 펌웨어 방침

| 펌웨어 | 어디서 | 누가 올리나 | 재배포 |
|---|---|---|---|
| Mali CSF(`arm/mali/arch11.8/mali_csffw.bin`) | linux-firmware | 커널(panthor) | 가능 |
| GPUEB(`gpueb.img`) | 기기 파티션 | **LK**(벤더 커널에 request_firmware가 없다) | 해당 없음 |
| 터치 NT36532(`novatek_ts_fw_boe.bin`, `novatek_ts_fw_tm.bin`) | 기기 vendor → `lenovo/tb375fc/` | 커널(터치를 열 때와 깨어날 때) | 미확인. 기기에서 뽑는다 |
| Wi-Fi·BT soc7_0 | 기기 파티션(`connsys_wifi`, `connsys_bt`) | **LK**(conninfra EMI에 싣는다. 커널은 시작만 시킨다) | 해당 없음. 파티션을 지킨다 |
| Wi-Fi 보정값·MAC | 기기 nvdata | 커널(NVRAM 명령으로 넘긴다. initramfs가 nvdata를 붙인다) | 기기마다 다르다 |
| 앰프 AW883xx ACF, SCP·오디오 DSP | 기기 vendor·파티션 | 커널 | 미확인 |

- 펌웨어 추출 스크립트는 `firmware/`에 두고, 펌웨어 파일 자체는 저장소에 넣지 않는다(buntOS ADR-012).
- 설치기는 남겨 둔 안드로이드 슬롯이나 처음 백업에서 펌웨어를 가져온다.

## 8. 기기 맞춤 최적화 (buntOS 쪽)

| 항목 | 할 일 | buntOS에서 바꿀 곳 |
|---|---|---|
| 화면 배율 | 12.7″ 2944×1840은 약 273ppi다. buntOS 규칙(내장 화면은 논리 125ppi에 가깝게)이면 기본 배율은 **225%**(논리 1308×818)이고, 고를 수 있는 것은 100~275%다. DSI에는 EDID가 없으므로 패널(또는 simple-framebuffer 노드)이 실제 크기 약 273×171mm를 알려야 이 규칙이 돈다 | 패널 드라이버의 `width_mm`·`height_mm`, simplefb DT의 물리 크기 |
| 주사율 | simple-framebuffer 단계는 LK가 정한 모드 하나다. mediatek-drm 단계에서 144/120/60/30Hz를 열고, 움직임이 없으면 낮추는 적응형 주사율(macOS ProMotion처럼)을 둔다 | `comp`의 출력 모드와 프레임 일정 |
| 터치 | 10점을 그대로 받는다(컴포지터에 손가락 수 제한이 없다). 손바닥 거부와 가장자리 띠(위 24px, 아래 20px)를 실기기에서 맞춘다 | `comp/src/touch.rs` |
| 펜 | NT36532 펜을 tablet-v2(압력·기울기·단추)로 앱에 보낸다. 컴포지터에 smithay tablet 틀이 있고, 펜 디지타이저는 입력 모드 계산에서 이미 뺀다 | `comp` tablet-v2, 시스템 설정 펜 페이지(나중) |
| 키보드 커버 | 포고 키보드가 붙으면 포인터 모드, 떼면 터치 모드다(지금 규칙 그대로). plug GPIO26으로 붙음을 안다. 펌웨어의 두 번째 홀 센서(`hall_2`)가 바로 이 GPIO26이고 드라이버가 없다. 덮개 홀 센서(GPIO1)는 닫힘을 알린다. 뒤로 접은 것을 무엇이 알리는지 기기에서 보고, 그것으로 `SW_TABLET_MODE`를 보내 그때는 키보드·터치패드를 세지 않는 규칙을 더한다(iPad Magic Keyboard처럼) | `comp/src/input_mode.rs`, gpio-keys DT |
| 회전 | 가속도·자이로(LSM6DSO)와 조도(TCS37013)가 있지만 SCP(nanohub) 뒤에 있다. AP 장치 트리에 노드가 없다(2026-11-01 실측). 그래서 자동 회전이 막힐 수 있다. 처음에는 시스템 설정의 수동 회전이고, SCP 길이 열리면 iio-sensor-proxy로 자동 회전한다. 패널의 자연 방향은 가로다 | 시스템 설정 디스플레이 |
| 전원 | A715 ×4 + A510 ×4의 EAS, cpufreq-hw, GPU 주파수, s2idle, 덮개(홀)로 잠자기, 45W PD 충전, LVTS 열 관리 | 커널, buntOS 전원 설정 |
| 오디오 | 오디오가 되기 전에는 USB·Bluetooth 오디오다. 그 뒤 스피커 넷(AW883xx)의 보호 설정과 ALSA UCM을 둔다. 볼륨 키는 PMIC 키다 | UCM, WirePlumber |
| 메모리 | 8·12GB. zram과 GPU 공유 메모리를 맞춘다 | 이미지 설정 |
| 그래픽 | Mesa 26.3 이상(G615). PanVK는 실험 플래그가 필요해 GL(Panfrost)로 시작한다 | 이미지의 Mesa 판, `GSK_RENDERER` 기본값 |

## 9. 저장소 구조

이 저장소는 buntOS의 포크(`buntos-TB375FC`)다. buntOS 구조에 기기 쪽을 더한다.

```
buntos-TB375FC/                 태블릿 라인의 첫 포크 (원격 buntos = 원본, 데스크톱 라인)
  docs/tb375fc/                 이 문서, 조사, Wi-Fi 설계
  docs/hw/lenovo-tb375fc.md     펌웨어 실측
  kernel/tb375fc/
    build.sh                    Linux 7.2.x 받기·패치·설정·빌드, 이미지용 커널 .deb (image)
    debian-config.sh            이미지용 설정 (Debian forky 설정 + 조각)
    config/tb375fc.config       보드 설정 조각
    patches/                    업스트림 제출 대기 패치 (series)
    dts/mediatek/               mt6897.dtsi, mt6897-lenovo-tb375fc.dts, -tb373fu.dts
  bsp/lenovo-tb375fc/
    mkosi.conf                  mkosi 보드 프로필 (이미지용 커널 .deb, 파티션별 출력)
    boot/                       boot·vendor_boot·init_boot·dtbo 만들기 (헤더 v4, DT 표)
    initramfs/                  첫 부팅용 initramfs (busybox, USB 가젯, nvdata)
    firmware/                   기기에서 펌웨어와 보정값을 뽑는 스크립트 (파일은 두지 않는다)
  tools/hw/                     펌웨어 받기·풀기 도구
```

- 빌드 산출물은 저장소 밖(빌드 VM의 `~/buntos-build/tb375fc/`)에 둔다.
- 원본 buntOS로 돌려줄 만한 것(펌웨어 도구, 데스크톱 쪽 고침)은 원본에 따로 낸다.

## 10. 위험과 대응

| 위험 | 영향 | 대응 |
|---|---|---|
| 언락 중 벽돌, nvdata 손실 | 개발 중단, Wi-Fi 보정값 잃음 | 예비 1대, go/no-go 넷. SP Flash Tool readback으로 전체 백업을 뜨고 해시로 확인한 뒤에만 언락한다. 복구도 같은 도구와 순정 펌웨어 묶음으로 한다(2.1절) |
| 언락 길이 판에 묶임 | 받은 기기의 판에서 언락이 안 될 수 있다 | 받자마자 OTA를 끄고 판을 적는다. 그 판의 성공 보고를 확인한다. 안 되면 중국판 ZUI 16으로 내릴지 따로 정하고, 그래도 안 되면 buntOS ADR-011(기기 선택)을 다시 본다 |
| 개발용 LK의 출처를 모름 | 남이 고친 부트로더를 굽는다 | 굽기 전에 순정 LK와 견준다(문자열, 서명 영역). 언락한 뒤에는 바로 순정 LK로 되돌린다 |
| GenieZone의 EL2 점유 | KVM 불가 | 2a에서 판정(D8). "불가"면 buntOS ADR-011 재검토, gzvm은 미병합이라 나중 |
| 화면: dual-DSI + DSC가 업스트림에 없음 | 주사율·밝기·화면 끄기가 늦어진다 | simple-framebuffer로 데스크톱(D4), 업스트림 DSC·재구성 시리즈를 따라가고 dual-DSI는 우리가 맡을 수도 있다 |
| 센서 허브(SCP nanohub) | 자동 회전·조도 없음 | 수동 회전, SCP 길은 3단계에서 조사 |
| MFD `mtk-spmi-pmic` 미병합 | PMIC가 늦어진다 | 패치 대기열에 넣고 업스트림 판을 따라간다. v12(2026-05) 뒤로 새 판이 없고, BayLibre가 이어받겠다고 했다(2026-10). v12의 인터럽트 결함(마스크가 늘 첫 그룹에 쓰인다)은 대기열에서 고쳤다. 업스트림에 알릴지는 정할 것이다. 병합된 MT6363 regulator(7.2.12)의 16비트 regmap은 벅을 끌 때 옆 레일을 함께 끈다. 고침 전에는 MT6363 벅을 끄거나 모드를 바꾸지 않는다 |
| Mesa 26.3 의존 | GPU 가속이 늦어진다 | 이미지의 Mesa 판 확인, 그 전에는 CPU 렌더링(buntOS는 이미 llvmpipe에서 돈다) |
| Wi-Fi 펌웨어가 uni cmd를 받지 않음 | 2d 지연 | 레거시 명령 경로(BSS·STA·키·ROC), USB 동글 우회 |
| LK가 mainline 트리의 connsys 노드에 `emi-addr`·`memory-region`을 써 주지 않음 | Wi-Fi·BT가 펌웨어를 찾지 못한다 | 2d에서 살아 있는 트리로 확인한다. 안 되면 LK가 찾는 노드 이름과 compatible을 맞춘다 |
| conninfra가 벤더 TF-A SIP 호출에 기댐 | 업스트림 제출이 어렵다 | 업스트림 UFS의 SIP 호출이 선례다. 일반 세계에서 레지스터를 직접 쓸 수 있는지도 본다 |
| 펌웨어 재배포 불가 | 배포 꼴 | 설치할 때 기기에서 뽑기(D6) |
| 소스 출처 | 법적 불확실 | 이 기기 모듈 미러는 제3자다. 참고만 하고 코드는 업스트림 꼴로 다시 쓰며, GPL 출처(OEM 공식 저장소)를 우선한다 |
| 커뮤니티 포트와 겹침 | 중복 작업 | 이슈 트래커로 연락할 수 있다. 나눌 일은 `pinctrl-mt6897`(그쪽 터치 IRQ를 푼다), 터치, 살아 있는 트리·LK 기록이다. 그쪽은 2026-09-23 뒤로 멈췄다 |
| 생산 시기별 부품 차이 | 일부 기기만 다름 | Wi-Fi FEM은 가장 새 펌웨어의 표, 패널은 LK가 고른 노드를 읽는다 |

## 11. 일정과 다음 작업

1. **끝낸 것(기기 없이):**
   - 2a 장치 트리(업스트림 꼴), 커널 커스텀(`7.2.12-tb375fc`), `boot/` 이미지 도구와 첫 부팅 initramfs
   - 커뮤니티 포트의 커밋 13개와 겹침 정리([분석](community-port.md))
   - 오픈소스 쪽 조사(v1.2)와 제품 라인 정리, 언락 길 확인(v1.3)
   - v1.5에 끝낸 것:
     - `pinctrl-mt6897`(EINT 포함, 패치 0004~0006)과 pio 노드. 표는 OnePlus 공식 MT6897 커널에서 옮겼다
     - SPMI: 고침 넷(0002·0003 백포트, 0007·0008), M 버스 노드(rcs는 EINT 248, 별칭 `spmi0`, P 버스는 끔), MT6319 셋(DT만). 소비자가 아직 없어 벅은 모두 always-on이다. pio와 SPMI는 2b까지 꺼 둔다
     - 이미지용 설정: `kernel/tb375fc/debian-config.sh`가 이미지와 같은 시점의 Debian forky 7.2.12 설정에 조각을 얹어 본다. Debian은 SCSI·sd·USB·xHCI를 모듈로 두어 조각의 UFS·USB 값이 내려갔다. 조각에서 이 넷을 y로 올려 빠지는 값이 없다
     - 포크 정리: 포크는 x86-64 이미지와 CI를 돌리지 않는다. GitHub 원격이 없어 `.github/workflows`가 돌 곳도 없다. 파일은 지우지 않는다. 지우면 buntOS가 그 파일을 고칠 때마다 합치기에서 충돌이 난다
     - buntOS 합치기: buntOS main에 커밋이 쌓이면 포크 main에서 `git merge buntos/main`을 한다. 첫 합치기(c433a32)의 충돌은 README 머리뿐이었다
   - v1.6에 끝낸 것: `clk-mt6897`(패치 0009~0013, OEM 공식 표). 블록 일곱(topckgen, apmixedsys, vlpckgen, pericfg_ao, ufscfg_ao·pdn, HW voter)과 UART·SPMI·USB·UFS의 소비자 클럭을 장치 트리에 넣었다. 클럭 컨트롤러는 2b까지 꺼 두고, uart0은 2a 동안 clk26m을 버스 클럭으로 쓴다
   - v1.8에 끝낸 것(세션 둘을 병렬로):
     - PMIC(패치 0014~0018): MFD `mtk-spmi-pmic`(업스트림 v12 백포트와 고침)과 MT6363 키, 보드 dts의 MT6363(키, 레일 23개)과 UFS 공급. USB `vusb33`은 MT6368 VUSB라 드라이버가 생길 때까지 비워 둔다. VEMC 전압 읽기 결함은 업스트림 고침 후보로 남긴다
     - UFS(패치 0019~0022): 바인딩은 MT6897 호스트(클럭 여섯, OPP 둘, 리셋 셋)와 M-PHY(클럭 없음, 리셋은 선택). 드라이버는 M-PHY에 전원 연산 없는 ops를 주고, 호스트는 펌웨어의 속성 넷을 compatible로 정하고 리셋을 바인딩 이름(`unipro`·`crypto`·`hci`)으로 받는다. dtsi는 `mediatek,ufs-disable-mcq`로 시작한다. USB는 펌웨어의 `maximum-speed`와 U2 eye 값을 넣었다
     - 부트 명령줄에 `regulator_ignore_unused`를 더했다. 소비자가 아직 없는 레일(UFS가 붙기 전의 VEMC 등)이 늦은 정리에서 꺼지지 않는다
   - v1.9에 끝낸 것: MT6368(패치 0023~0026). 업스트림 MT6373 regulator v12와는 레지스터·레일이 달라(벅 VBUCK1~6와 VPA, LDO 은행 배치, FCCM·키 보호·ELR5 주소) 백포트하지 않고, 7.2.12 `mt6363-regulator`를 틀로 새로 썼다. regmap은 8비트 값이다. MT6373 v12에 대한 리뷰 셋(`regmap_bulk_write` 개수, OC 오류 보고, 해제 때 work 정지)을 반영했다. 보드는 펌웨어가 늘 켜 두는 레일 6개(vbuck2 GPU 코어, vbuck5·6, vio28, vtp, vusb)를 두고 ssusb `vusb33`을 MT6368 VUSB에 이었다. 그래서 2b에서 USB를 켤 때 SPMI와 MT6368도 함께 켜야 한다
   - v1.10에 끝낸 것:
     - 이미지용 커널 빌드 단계(`build.sh image-config`·`image`)와 mkosi 보드 프로필. 대기열 0001~0022로 한 번 빌드했다. 꾸러미의 커널·모듈·장치 트리로 LK 부트 이미지 넷이 파티션 크기 안에 들고, 같은 커널과 initramfs가 QEMU virt에서 init까지 돈다. 이 프로필로 만든 작은 mkosi 이미지에서 커널 꾸러미가 깔리고 UKI가 생긴다. 0001~0031의 이미지 설정도 빠진 값이 없다
     - 2c(1차) 입력의 터치·펜과 홀 센서(패치 0027~0031과 장치 트리). 업스트림 `novatek-nvt-ts`(I2C, 플래시 있는 칩)에 더하지 않고 새 드라이버로 썼다. SPI 페이지 접근·펌웨어 내려받기·펜이 이 드라이버의 대부분이고, 바인딩도 따로 두며 compatible에 버스 이름을 넣지 않았다(2024 RFC 리뷰). 표와 프로토콜은 OnePlus 공식 MT6897 커널에 든 레노버 NT36532 드라이버에서 옮겼다. SPI 컨트롤러 여덟과 패널 VDDIO(GPIO63)를 더했고, 핀 드라이버의 drive-strength(0027)와 pinmux 헤더 include(0028)를 고쳤다
2. **다음(기기 없이, 이 차례로):**
   1. 2c(1차) 입력: 포고 키보드(serdev). GPIO26(plug, 펌웨어의 `hall_2`)을 누가 가질지 함께 정한다
   2. 루트로 넘기는 initramfs(4절의 initrd 길): `initramfs/init`이 `root=`를 붙여 `switch_root`하고, 실패하면 USB 가젯 셸로 남는다. 루트 파티션 자리를 정한 뒤에 한다
   3. MT6363 regulator 고침 둘(업스트림에 낼 것): 16비트 값 regmap이 쓸 때 다음 레지스터도 써서, 벅 하나를 끄면 VS1·VS3·VSRAM이 함께 꺼진다. VEMC 전압 읽기의 범위 조건이 반대다. 지금 보드는 그 길을 타지 않는다(벅은 always-on이거나 노드에 없고, 전압 한계를 주지 않는다)
   4. 기기에서 UFS·USB가 뜬 뒤: UFS 클럭 스케일링(먹스 부모, 게이트의 `CLK_SET_RATE_PARENT`), MCQ(공통 바인딩의 인터럽트 수, 큐 인터럽트 SPI 177~184), xsphy MT6897 꼴(U2 INTR_CAL·efuse, U3 RX CDR), mtu3 깨우기 접점(벤더 판 103)
3. **기기:** 사용자의 TB375FC는 중국판 ZUXOS 1.5.10.044(Android 16)다(2026-11-01). 이 판에서 바로 언락한 보고는 없으니, 16.0.20.091로 내린 뒤 언락하는 길을 쓴다(2.1절). 예비 기기를 살 때는 ZUI 16 기기가 낫다.
4. **언락 전(지금 할 수 있는 것):**
   - 자동 업데이트를 끄고 1.5.10.044에 머문다(go/no-go 4번). 1.5.10.103 이후 판에서는 판 바꾸기 문제가 많이 보고됐다. 2026-11-01에 레노버 업데이트 앱(`com.lenovo.ota`)을 껐다(되돌리기는 `pm enable`)
   - 펌웨어: adb 셸은 `/vendor/firmware`의 이름만 보고 파일은 읽지 못한다(2026-11-01). 파일은 아래 readback 백업이나 언락 뒤 root로 얻는다
   - SP Flash Tool readback으로 전체 백업과 해시(go/no-go 1번). 인증 DA·auth는 그 판의 레노버 펌웨어 묶음에서 쓴다
   - ADB로 잰 것(2026-11-01, [실측 보고서](../hw/lenovo-tb375fc.md) 7절): 판, 잠금 상태, RAM 8GB와 저장장치 128GB, 화면(패널 버퍼 가로 2944×1840), 입력 장치 이름, vendor 펌웨어와 모듈 목록, 지문 센서, 센서(LSM6DSO, TCS37013, SCP 뒤)
   - 파티션 표를 읽은 뒤 루트 파티션 자리와 크기를 정한다(4절): GPT의 userdata 자리, LK fastboot가 새 항목에 굽는지
   - root가 필요한 것(언락 뒤): `getevent -lp`, framebuffer 주소와 살아 있는 트리의 값(`/proc/device-tree`는 이름만 보이고 값은 막혀 있다), `/proc/iomem`, 파티션 표
   - UART 위치는 기기를 열어 찾는다
5. **언락:** 2.1절의 언락 순서대로 한다. 그 뒤 root로 framebuffer 주소와 살아 있는 트리를 읽어 장치 트리의 framebuffer·ramoops를 켜고, 2a를 한다(`fastboot boot`).
6. **2b → 2c(1차) → 2d → 2e → 2c(2차) → 3:** 각 단계의 종료 조건을 이 포크의 문서에 남긴다.

## 12. 출처

[포팅 재료 조사](porting-materials.md) 6절, [Wi-Fi 문서](wifi-soc7_0.md), [커뮤니티 포트 분석](community-port.md)의 출처를 따른다.
