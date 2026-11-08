# buntOS TB375FC 포팅 최종 설계안 v1.62

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
- v1.11(2026-11-01): initrd 길을 우리 initramfs로, 루트 자리를 userdata를 줄여 만든 새 GPT 파티션으로 정했다(4절). USB 호스트의 VBUS가 MT6375 OTG에서 나와 드라이버가 먼저 필요하므로 USB-C 저장장치 길은 쓰지 않고, USB 호스트를 2b 종료 조건에서 MT6375 OTG 뒤로 옮겼다(3·5·10·11절)
- v1.12(2026-11-01): 업스트림 MT6363 regulator의 결함을 고쳤다(패치 0037~0041, 업스트림에 낼 것): 16비트 값 regmap(벅을 끄면 VS1·VS3·VSRAM이 함께 꺼졌다), VEMC 전압 읽기·쓰기, OC 보고와 해제 때 work 취소, VSRAM 넷의 켜기·LP 레지스터. 보드의 방침(전압 한계 없음, 펌웨어가 늘 켜 두는 레일은 always-on)은 기기에서 확인할 때까지 그대로 둔다(5·10·11절)
- v1.13(2026-11-01): 2c(1차) 입력의 포고 키보드 커버(패치 0032~0036)를 더했다. 스톡 `tn_keyboard.ko`를 읽어 보니 이 기기 키보드는 v6 시리즈의 CRC 틀이 아니라 옛 꼴(시작 0x25, 니블 치환, 8비트 합, 명령 0x61~)이라 그 꼴을 드라이버에 더했다. GPIO26(`hall_2`)은 키보드의 붙음·뗌 줄로 정하고 gpio-keys에서 뺐다. 뒤로 접음은 키보드 MCU가 알리는 것으로 보고 `SW_TABLET_MODE`로 보낸다. 보드 `&ssusb`에 문서와 달리 빠져 있던 `dr_mode = "peripheral"`을 넣었다(5·8·10·11절)
- v1.14(2026-11-01): 루트로 넘기는 initramfs를 만들었다(4절). 넘어간 뒤의 USB 가젯은 진짜 루트에서 새로 띄우고(`buntos.usbdebug=1`), 보드 프로필과 initramfs가 함께 systemd-repart와 gpt-auto 생성기를 가린다. QEMU에서 systemd와 가젯 너머의 SSH까지 시험했다
- v1.15(2026-11-01): USB 호스트의 VBUS 드라이버(패치 0042~0049)를 더했다. 모두 꺼 둔 노드다. 순정을 읽어 보니 MT6375 OTG만으로는 5V가 커넥터까지 나가지 않을 수 있다. 순정 `extcon-mtk-usb`는 부스트를 켜기 전에 차지 펌프(NU2115 또는 UPM6720)에게 입력 FET를 닫게 한다. 그래서 그 스위치를 레귤레이터로 더해 부스트의 supply로 두었다. MT6375 쪽은 프로브와 등록에서 쓰기가 없고, 사용자 공간(`regulator-output`)이 켠다. `vbus-supply`는 otg 모드의 프로브마다 켜지므로 쓰지 않는다(4절). 사용자 기기의 차지 펌프는 UPM6720 @0x65다(adb)
- v1.16(2026-11-01): 포고 키보드 드라이버의 독립 검토를 반영했다(패치 0033·0034). TX 게이트를 에코로 닫고(이전에는 프레임 뒤 4~8 ms 열려 있었다), 서스펜드에서 일을 끄고 키·손가락을 놓으며, SLEEP 응답의 상태를 본다. wake IRQ와 워크큐의 경합, 다시 붙일 때의 Caps LED, `SW_TABLET_MODE`가 실제와 어긋나는 길을 고쳤다. 드라이버를 사용자 공간 시뮬레이터로 돌려 확인했고 기기 시험은 없다(8·10절)
- v1.17(2026-11-01): 기기에서 할 일을 [bring-up 체크리스트](bringup-checklist.md) 하나로 모았다. 세션 보고에만 있던 시험 차례를 저장소로 옮겼다. 3절 표의 낡은 말(2a의 Debian 커널, 2b의 fixed 클럭과 `regulator-fixed`)을 4·11절에 맞췄다
- v1.18(2026-11-01): 전원 도메인을 만들었다(패치 0050~0054). SPM 32개(직접 제어 25, HW voter 7) 표가 OEM 소스·스톡 모듈과 맞는다. MM_INFRA(voter) 밑에 직접 제어 도메인이 있어 MT8196처럼 제어기를 나눌 수 없어, 한 제어기에 함께 둔다. 켜는 순서는 벤더처럼 두 스위치 사이를 기다린다. 7.2.12의 `pd_ignore_unused`는 sync_state의 끄기를 막지 못한다. 그래서 표를 모두 기본 꺼짐으로 두어 probe가 도메인을 건드리지 않게 하고, 장치 트리에는 첫 소비자가 있는 도메인만 적는다(지금은 conn). IFR·VLP syscon은 늘 꺼 둔다(켜면 fw_devlink가 제어기를 10초 붙든다). GPU 쪽 12개는 GPUEB 펌웨어가 켜므로 `mtk-mfg-pmdomain`이 맡는다.
- v1.19(2026-11-02): NT36532 패널 드라이버를 만들었다(패치 0080~0084). 업스트림 드라이버와 바인딩을 백포트하고(`47b823940e38`, `1c523f0a9301`, 드라이버가 쓰는 DSI 플래그 `ffa88de8ddb6`), 패널 셋(BOE, Tianma, Tianma 무광)과 144/120/60/30Hz를 더했다. 초기화 표·타이밍·DSC 값은 미러, 스톡 모듈, LK 표의 셋이 바이트까지 같다. 패널은 노드 하나에 GPIO184 스트랩으로 고른다(터치와 같은 줄, D2의 업스트림 꼴). LK가 찾는 `panel<n>` 경로는 두지 않는다: LK는 화면을 자기 트리로 켜고, 그 경로는 찾지 못해도 기록만 남긴다. 메모와 달랐던 것: "패널 고유 RC 표"는 없다(DSC 1.1 SCR 전 표 그대로이고 첫 줄 오프셋만 13이다). 돌 곳이 없어 기기 시험은 없고, 화면 파이프라인이 올 때 맞출 것을 보고에 적었다
- v1.20(2026-11-02): connsys Wi-Fi(패치 0055~0060)와 Bluetooth(패치 0085·0086)를 더했다. 기기 시험은 없고 노드는 모두 꺼 둔다. LK는 connsys 노드를 compatible이 아니라 경로로 찾고 status를 보지 않으므로(역어셈블), `gps`·`connscp`를 2d까지 트리에서 빼 LK가 메모리를 나누지 않게 한다. uni cmd를 받는지는 기기에서 `DEV_INFO` 하나로 가른다. 레거시 명령 길은 그 답을 본 뒤에 쓰고 번호 0061~0069를 비워 둔다. DMA는 LK가 만든 `no-map` 칸 안에서 자기 `dma_map_ops`로 복사한다(업스트림 꼴은 아니다). FEM은 nofem으로 시작한다. connsys 전원의 첫 시험은 nvdata와 DMA 칸이 필요 없는 Bluetooth로 한다
- v1.21(2026-11-02): MT6375 충전과 TCPC(패치 0090~0097)를 더했다. 노드는 모두 꺼 두었다. 드라이버가 없는 동안의 배터리를 코드로 판단했다: MediaTek LK2 코드 기준으로 LK는 입력과 충전을 500 mA로 두고 충전기 종류와 상관없이 그대로 넘기므로, 화면이 켜져 있으면 PC에 꽂아도 배터리가 줄어들 가능성이 크다(부하 전력은 추정, 기기에서 잰다). 충전은 배터리 설명과 게이지가 장치 트리에 있고 온도를 읽은 뒤에만 넘겨받는다. 값은 순정 트리에서만 왔고, 상한은 드라이버와 MFD 두 곳에서 막는다. 충전 전압 상한은 유지 관리 표에서 가장 낮은 4340 mV다(기기 실측과 같다). adb의 4310은 게이지 값이다. TCPC는 처음에 PD 없이 싱크로만 쓴다
- v1.22(2026-11-02): 충전 드라이버의 독립 재검토 두 번을 반영했다(패치 0090~0097을 그대로의 번호로 고쳤다). 넘겨받기 전에도 말이 안 되는 게이지 값이 세 번 이어지면 충전을 끈다. `inhibit-charge`는 게이지와 상관없이 바로 듣고 실패를 돌려준다. 칩의 고장은 걸쇠로 남겨 새 입력에서만 푼다. 게이지 전압에 범위 검사를 두었다(2.5~5 V). 절전에서 충전을 끄지 못하면 잠들지 않는다. 게이지가 실패하는 동안에도 VBUS를 따라가고, 게이지는 잠금 밖에서 읽는다. 부스트 깃발은 칩과 맞춘다. 상한 값은 그대로다. `rmmod`는 멈추는 길이 아니다(체크리스트 9.3). Wi-Fi도 독립 재검토를 반영했다(패치 0057, 0059, 0060을 같은 번호로 고쳤다): 모듈을 내릴 때 남은 `mac_work`가 해제 뒤 접근을 내던 길을 막았고, `PROTECT_CTRL`(EXT 0x3e)은 실제로 답을 기다려 보낸다는 점을 문서에 바로잡았으며, 5845~5925 MHz와 6 GHz가 꺼지던 초기화를 고쳤고, 리셋이 끝없이 되풀이되지 않게, 앞 재시작에서 10분이 지나지 않은 재시작이 20번 이어지면 멈추게 했다
- v1.23(2026-11-02): 2e의 GPU 전원·열·게이지 패치 0070~0079를 더했다. 모두 꺼 둔 노드다. GPUEB는 "오프셋만 다른 변형"이 아니었다: LK가 띄우고 멈추지 않으며(GHPM·클럭·레귤레이터 없음), 공유 메모리는 LK가 `/soc/gpueb`에 적어 주고, OPP는 CMD_COMMIT이다. LVTS는 센서가 16개가 아니라 도메인 넷에 32개이고, 펌웨어가 켜 둔 것을 읽기만 하며, 보정값은 efuse 블록이 아니라 LK가 넘기는 devinfo에 있다.
- v1.24(2026-11-02): 화면·MDP·코덱의 클럭 게이트를 만들었다(패치 0100~0104, 블록 열하나, 게이트 256). 표가 OEM 소스·스톡 모듈과 맞는다. 이 블록은 전원 도메인이 켜져 있을 때만 답하는데 클럭 노드에 `power-domains`를 달지 않는다: VEN·MDP 도메인은 자기 블록의 게이트를 써서 서로 기다리고, 나머지는 probe가 도메인을 켰다 끄면 MM_INFRA까지 꺼져 LK 화면이 꺼진다. 그래서 게이트에 `.is_enabled`를 두지 않아 클럭 코어가 블록을 읽지 않는다. HW voter 게이트는 벤더 순서로 투표하고, HW voter 한도를 30µs에서 5ms로 올렸다. 노드는 모두 꺼져 있다.
- v1.25(2026-11-02): MT6685 RTC(패치 0110~0113)를 더했다. 노드는 꺼 두었다. 시간 읽기와 쓰기만 한다. 드라이버는 업스트림 v1을 그대로 쓰지 않고 벤더 판을 따라 새로 썼다: 연도 칸은 1968부터(업스트림 v1은 안드로이드의 2026을 2086으로 읽는다), 읽기에 BBPU 쓰기 없음(2025년 뒤 MediaTek 판), 게이트는 드라이버가 직접. 쓰기는 regmap이 게이트 둘·시간 칸 여섯·트리거로 막는다(칸은 벤더처럼 두 바이트로 쓴다). 알람은 뒤로 미뤘다: `spmi-mtk-pmif`가 SID 이벤트를 체인 처리기에서만 지우므로 직접 요청하면 EINT 248이 계속 울린다. 클럭 버퍼 드라이버는 만들지 않는다: 순정 트리에 소비자가 없고 벤더 `clkbuf`도 디버그용이며(프로브 쓰기 0번, `xo-cmd` 없음), 업스트림 "MT6685 클럭" 시리즈는 XO가 아니라 RTC의 SCK_TOP 게이트 다섯이다. 첫 소비자는 단계 3 오디오의 BBCK5다.
- v1.26(2026-11-02): UFS의 안드로이드 파티션을 데스크톱에서 가리는 udev 규칙을 만들었다(`bsp/lenovo-tb375fc/mkosi.extra/usr/lib/udev/rules.d/90-buntos-tb375fc-hide-ufs.rules`, 시험은 `bsp/lenovo-tb375fc/udev/test.sh`). 파티션 이름이 아니라 UFS 호스트(드라이버 `ufshcd*` 또는 플랫폼 장치 `*.ufshci`) 아래의 모든 LU와 파티션에 걸어, `fastboot getvar all` 전에도 빠지는 파티션이 없다. 이미지의 udisks2 2.11.2와 gvfs 1.62.0의 소스를 읽어 `UDISKS_IGNORE`가 숨김일 뿐임을 확인했다: gvfs가 그 볼륨을 만들지 않아 파일 관리자에서 사라지지만 udisksctl과 D-Bus의 붙이기는 막히지 않는다. 막으려면 polkit 규칙이 따로 필요한데 지금은 넣지 않았다(11절). `mkinitramfs.sh`가 가끔 SIGPIPE로 죽던 것도 함께 고쳤다. 진짜 커널과 이미지의 udev(systemd 262)로 QEMU에서 UFS(LU 둘)와 USB 메모리를 함께 달아 시험했다. 기기 시험은 없다
- v1.27(2026-11-02): 충전 드라이버의 독립 검토 하나를 더 반영했다(패치 0093, 0097을 그대로의 번호로 고쳤다). 부스트 중에도 절전에 들 때 `CHG_EN`을 끈다. 부스트 값의 깃발은 값을 쓰기 직전에 세우고 되돌림이 성공했을 때만 내린다. sysfs 언바인드를 막았다(부스트의 소비자가 해제된 메모리를 쓰게 된다). 상한 값은 그대로다. Wi-Fi 독립 재검토를 하나 더 반영했다(패치 0060을 같은 번호로 고쳤다): 재시작 제한을 재시작 사이의 시간으로 바꾸어 느린 되풀이도 멈춘다. debugfs `runtime-pm`을 켠 채 `chip_reset`이 되고, `rmmod`가 타이머를 남기지 않는다.
- v1.28(2026-11-02): 처리기 없는 SID의 RCS를 지우는 PMIF 고침(패치 0114)을 더했다. 체인 처리기가 칸 넷을 다 읽고, 켜지지 않은 SID의 이벤트를 지운 뒤 제한된 로그를 남긴다(벤더 `rcs_irq_handler`와 같은 뜻). SID 4·5의 버스 기록은 늘어난 읽기 둘 말고 그대로다. 안드로이드 알람을 끄고 넘어가라는 규칙은 기기에서 볼 때까지만 둔다: 칩이 이벤트를 곧바로 다시 내면 0114로 막지 못한다.
- v1.29(2026-11-02): 화면 파이프라인을 만들었다(패치 0120~0139). 판은 7.2.12 위의 작은 패치들이다. 재구성 v2는 얹지 않고 DSC v9의 넷과 DSI 고침 둘만 백포트했다. MT6897의 컨트롤러 넷(dispsys, ovlsys 두 벌)을 mediatek-drm 하나에서 쓰고, 주 경로가 두 파이프를 갖는다(OVL_2L → DSC → DSI가 왼쪽과 오른쪽에 하나씩). 표는 벤더 코드를 그대로 돌린 값과 1061항목이 맞고, 실제 드라이버를 QEMU의 레지스터 모델 위에서 돌렸다(LK 화면을 넘겨받는 길 포함). 노드는 모두 꺼 두었고 기기 시험은 없다. 첫 켜기 전에 LK가 켠 레지스터를 읽어 지워야 할 값이 더 없는지 본다. PQ를 건너뛰는 경로는 첫 프레임이 확인이다.
- v1.30(2026-11-02): 기기를 맥에 연결해 잠긴 채로 fastboot까지 처음 보았다. 맥 `fastboot`가 닿고, 슬롯은 `_b`이고(2026-08-18 OTA), `_a`는 successful no·retry 0이다. `max-download-size`는 128MiB, `flashing get_unlock_ability`는 false다. 판을 내린 뒤 슬롯을 보는 항목을 체크리스트 3절에 더했다. RTC와 구역 온도는 셸에 막혀 4절에서 root로 읽는다. 값은 [실측 보고서](../hw/lenovo-tb375fc.md) 7절에 있다.
- v1.31(2026-11-02): 개발자 옵션의 OEM 잠금 해제를 켜자 `get_unlock_ability`가 true가 됐다. 안드로이드 쪽 값은 비어 있어 LK에서만 보인다. 이 값만으로 언락이 되는지는 모른다.
- v1.32(2026-11-02): 백업과 판 내리기의 준비물과 차례를 [Windows 작업 안내](windows-steps.md)로 뺐다. 16.0.20.091 묶음의 파티션 표를 읽어 기기와 맞댔다. 이름 84개와 크기 17개가 같고, `userdata`는 크기 조정 항목이라 128GB용 scatter를 따로 쓰지 않는다. 표가 `_a`에만 구우므로 판을 내린 뒤 슬롯을 바꾼다. 묶음의 efuse 설정과 실행 스크립트는 쓰지 않는다. 전원 + 볼륨 올림이 fastboot로 들어가고 전원 + 볼륨 내림이 나오는 것은 기기에서 확인했다. 나머지 키, 언락의 데이터 삭제, OEM 잠금 해제 조건은 포럼 보고로 체크리스트에 더했다.
- v1.33(2026-11-03): 사용자 기기를 16.0.20.091로 내렸다. SP Flash Tool 백업(`userdata`, preloader, GPT 제외)과 해시 확인, `_a`에 굽기, 잠긴 채 `--set-active=a`, 복구 화면에서 초기화, Android 14 부팅까지 실측으로 확인했다. AVB rollback index는 기기와 묶음이 모두 0이다. 언락은 아직이고, 순정 LK의 `flashing unlock`과 `oem unlock`은 거부됐다(`Sn Image Auth fail`). 개발용 LK 파일의 서명을 검증했고, 아직 굽지 않았다.
- v1.34(2026-11-03): 사용자 기기를 언락했다. 16.0.20.091에서 XDA 원글의 `lk_a`를 굽고, 시스템을 한 번 부팅한 뒤 `flashing unlock_critical`을 승인했고, 순정 LK로 되돌렸다. 언락은 유지됐고 `userdata`가 지워졌다. 이어서 `vbmeta_a`의 flags를 3으로 구워 AVB를 껐고(초기화 한 번 더, verity 장치 0개), Magisk 31.1로 root를 얻어 framebuffer(0xfc16f000, 0x3e90000), pstore 자리, GPT, 입력 장치, 펌웨어를 읽었다. 패널은 Tianma다.
- v1.35(2026-11-03): `framebuffer0`, `framebuffer_mem`, `ramoops`를 켜고, LK의 framebuffer 노드에 `no-map`이 없어 패치 0140을 더했다. `fastboot boot`는 LK에 없어(D3) 첫 부팅은 `_b`에 굽는다.
- v1.36(2026-11-03): **첫 리눅스 부팅 성공.** 7.2.12 시험 커널이 BusyBox 셸과 simpledrm 화면까지 갔다. LK는 서명 없는 `dtbo`를 거부하고, 우리 트리에는 펌웨어 적재가 찾는 벤더 노드가 없어, 지금은 순정 `vendor_boot`·`dtbo`에 트리를 넣은 커널을 쓴다.
- v1.37(2026-11-03): 2a 판정: 커널이 EL1에서 시작해 KVM을 쓸 수 없다(D8, 10절). 부팅 이유 `kernel_panic,init`은 `expdb`에 남은 kedump를 LK가 되살리는 것이었고, 지우자 pstore로 커널 기록 전체를 안드로이드에서 읽을 수 있게 됐다.
- v1.38(2026-11-04): **2b 종료 조건을 지났다.** 시험 커널로 핀, 클럭, SPMI, MT6363, MT6368, MT6319, USB 가젯, UFS가 돈다. GPT를 고쳐 `userdata`를 28.2 GiB로 줄이고 `buntos` 80 GiB를 더했고, 작은 Debian 루트가 UFS에서 3.2초에 뜨고 맥에서 USB로 SSH로 들어간다. UFS는 HS-G4 2레인이고 쓰기 439 MB/s다. 대기열의 0041·0037·0038을 regulator `for-7.4`에 적용된 커밋으로 바꿨다. MT6319는 보드의 전압 한계를 뺐고(설정 레지스터가 표 밖이라 읽기가 `-EINVAL`), USB에 VBUS 덮어쓰기(0141, 0142)를 더하고 XS-PHY 고침(0143, 본가 `02df7fe1297d`)을 옮겼다. UFS 3.1 장치에 HS-G5 속성을 쓰지 않는다(0144). UFS가 클럭을 내릴 때 함께 쓰는 `mmpll`이 꺼져 화면이 깨지던 것은 `simple-framebuffer`의 `clocks`로 막았다. 첫 시험 범위라 `maximum-speed`는 high-speed다. 정식 부팅 길을 [LK가 트리에 요구하는 것](lk-kernel-tree.md)에 정리했다.
- v1.39(2026-11-04): KVM 불가를 받아들이고 이 기기로 그대로 진행한다(D8, 10절, 사용자 결정). 다른 태블릿을 매입하면 그때 사용자가 말해서 다시 시도한다.
- v1.40(2026-11-04): 시험 커널로 2c 입력, 2d Bluetooth, RTC, 2e 열·게이지·충전·GPU가 기기에서 돈다. Wi-Fi 펌웨어는 uni 명령을 받지 않아 레거시 경로가 필요하다. CPU와 GPU에 passive 85도 냉각을 넣었다. CPU 유휴 상태는 PSCI가 거절한다. 결과는 [실측 보고서](../hw/lenovo-tb375fc.md) 7절이다.
- v1.41(2026-11-04): Wi-Fi가 레거시 명령 길(패치 0061~0065)로 돈다. 비밀번호를 사용자가 직접 넣은 2.4 GHz WPA2-PSK 연결이 인터넷까지 되고, 내려받기 최고 138 Mbit/s, 올리기 87 Mbit/s다. 칩 리셋 실패, 5 GHz AP, RX 순서 정리, `regulatory.db` 거부가 남았다. 부팅 블랙리스트는 그대로 둔다. 결과는 [Wi-Fi 문서](wifi-soc7_0.md) 7절이다.
- v1.42(2026-11-04): 5 GHz AP(5745 MHz, 80 MHz, WPA2-PSK) 연결도 인터넷까지 된다. 짧은 시험이라 속도는 재지 않았다. 결과는 [Wi-Fi 문서](wifi-soc7_0.md) 7절이다.
- v1.43(2026-11-04): Wi-Fi 칩 리셋이 첫 시도에 돌아오고 리셋 뒤 스캔이 된다(패치 0066). 연결 중 리셋은 못 봤다. 결과는 [Wi-Fi 문서](wifi-soc7_0.md) 7절이다.
- v1.44(2026-11-04): USB 커넥터 과열 보호를 넣었다(패치 0098·0099). 업스트림 AUXADC가 MT6363의 VIN을 읽지 못해 고쳤고(패치 0150), 벽 충전기가 VBUS를 내렸다 올리면 입력이 500 mA에 머물던 것을 고쳤다(패치 0151). 벽 충전기 2A 10분에 커넥터가 그대로다. 차단 시험의 이상은 구형 케이블 문제로 보고 종결했고 3단계에서 다시 본다. 결과는 [체크리스트](bringup-checklist.md) 9.3절 F다.
- v1.45(2026-11-04): 연결 중 칩 리셋 뒤 약 9초에 다시 붙는다(패치 0067). 결과는 [Wi-Fi 문서](wifi-soc7_0.md) 7절이다.
- v1.46(2026-11-04): 화면을 mediatek-drm으로 옮겼다(패치 0120~0139, 0153). CPU 화면 복사가 없어지고 vsync가 생겼다. 부팅 때 화면, GPU, Wi-Fi, 충전을 서비스가 차례로 올린다. 게이지의 전류 부호를 고쳤다(패치 0152). 결과는 [체크리스트](bringup-checklist.md) 9.5절이다.
- v1.47(2026-11-04): Mesa 26.3을 레시피에 넣었다(`bsp/lenovo-tb375fc/mesa/build.sh`, 꾸러미 `buntos-mesa`). G615는 26.3부터 돌고 이미지의 Debian 스냅숏은 26.1이다. 새 빌드는 기기 GPU에서 픽셀 오류 0, 평균 257 FPS(1080p)다. 터치 펌웨어와 Wi-Fi 보정 파일은 여전히 저장소 밖이다.
- v1.48(2026-11-04): 화면 밝기가 돈다. 패널의 DCS `0x51`이 아니라 SoC의 `disp_pwm0`이 SGM37604A 둘을 움직인다(패치 0154·0155, `pwm-backlight`). LED 칩에는 드라이버가 필요 없다. SM5109는 편향을 껐다 켜면 전압 값을 잃어 고칠 일이 남았다. 결과는 [체크리스트](bringup-checklist.md) 9.5절이다.
- v1.49(2026-11-04): 로그인과 로그아웃 때 패널이 꺼지지 않는다(buntOS main의 화면 넘기기, `comp/src/handover.rs`). 기기에서 전환 넷 내내 CRTC가 켜져 있었다. 결과는 [체크리스트](bringup-checklist.md) 9.5절이다.
- v1.50(2026-11-05): Wi-Fi 데이터 경로가 조용히 멈춰도 알아채고 돌아온다(패치 0068·0069). mac80211이 연결을 보고, 프레임이 돌아오지 않으면 칩을 다시 시작한다. 멈춤의 원인은 아직 모른다. 결과는 [Wi-Fi 문서](wifi-soc7_0.md) 7절이다.
- v1.51(2026-11-05): PD 충전을 넣었다(패치 0170~0177). MT6375로 5 V 3 A와 9 V 2 A를 받고, 충전기가 보낸 하드 리셋에서 스스로 돌아온다. 직충전은 드라이버에 있지만 기본 꺼짐이다. 실제 만충을 봤고 칩이 제대로 끝내지만 숫자는 89%에 머문다. Wi-Fi 멈춤은 펌프가 꺼져 있어도 난다. 결과는 [체크리스트](bringup-checklist.md) 9.3절의 결과 3이다.
- v1.52(2026-11-05): apt가 우리 것을 망가뜨리지 않게 했다(`apt-guard`). 바이너리가 쓰는 라이브러리를 꾸러미 `buntos-runtime`의 의존성으로 잡고, 커널, Mesa, GPU 펌웨어를 고정하고, 서비스 재시작을 막고, apt 앞뒤로 스냅숏과 해시를 남긴다. 기기에서 39개를 실제로 올려 우리 파일이 그대로인 것을 봤다. 설명은 [BSP README](../../bsp/lenovo-tb375fc/README.md)다.
- v1.53(2026-11-05): 만충이 100%로 보이게 했다(패치 0178~0181). 충전기가 4.34 V에서 끝내면 게이지는 89%이고 UPower는 `charging`에 머물렀다. 게이지 드라이버가 88%를 100%로 잡아 `capacity`, `charge_full`, `charge_full_design`을 보고하고, 충전기가 끝내면 `status`를 `Full`로 한다. 배터리에 넣는 전압과 양은 그대로다. 커널 #303으로 기기에서 봤다. 0170~0177을 넣은 커널의 첫 부팅이기도 하다. 결과는 [체크리스트](bringup-checklist.md) 9.3절에 있다.
- v1.54(2026-11-05): 터치를 패널이 켜진 동안만 돌린다(패치 0182·0183). 패널과 터치가 한 칩이라 패널 리셋이 터치 펌웨어를 지운다. 컴포지터가 첫 화면을 늦춘 뒤(buntOS `cb94109`)로 부팅 때 터치 펌웨어가 패널 리셋보다 먼저 올라가, 첫 로그인 화면에서 터치가 죽었다. 커널 #304에서 재시작 없이 터치가 되는 것을 봤다. 결과는 [체크리스트](bringup-checklist.md) 9.5절에 있다.
- v1.55(2026-11-05): 소리를 넣었다(패치 0160~0167). MT6897 AFE(MT8196 틀)의 메모리, ETDM 4, ADDA 캡처, MT6368 코덱의 캡처, 본가 `aw88395`에 GPIO82 펌웨어 고르기, 벤더의 배터리별 출력 상한, 보정 전 음량 상한을 더했다. 앰프는 AW88394이고 칩 ID 0x2049다. 스피커 넷과 AIN0 마이크를 사용자가 듣고 확인했고, PipeWire와 상단 바 슬라이더까지 된다. USB-C 이어폰, 음악 재생, 서스펜드, 설정 앱, 배터리가 낮을 때의 상한은 아직 못 봤다. 결과는 [체크리스트](bringup-checklist.md) 9.6절에 있다.
- v1.56(2026-11-05): apt 보호를 buntOS 공통판으로 바꿨다. 도구와 공통 설정은 `image/`에 있고, 이 기기는 `apt-guard.conf.d/50-tb375fc.conf`와 `preferences.d/70-buntos-tb375fc`만 더한다. 예전 설정이 지키던 항목이 모두 들어 있고 소리 파일을 더 지킨다. 설명은 [BSP README](../../bsp/lenovo-tb375fc/README.md)에 있다.
- v1.57(2026-11-05): 블루투스를 부팅 때 켠다. `buntos-bluetooth.service`가 Wi-Fi 뒤에 드라이버를 올리고, `bluetooth-up`이 기기에서 한 번 만들어 둔 주소를 넣는다. Wi-Fi가 connsys를 쥔 채 BGF 전원을 내리면 다시 켜지지 않아(`power ack`, 원인 미상) 닫을 때 WMT 기능만 끄고 MCU는 둔다(패치 0088). 부팅, 끄고 켜기, 스캔, Wi-Fi 칩 리셋과 함께 쓰기를 기기에서 봤다. 페어링, 이어폰, 입력 장치, 전력은 못 봤다.
- v1.58(2026-11-05): 전원 키와 잠자기가 된다. 짧게 누르면 뗄 때 잠들고, 누르면 깨어나고, 10초 잡으면 커널이 끈다(패치 0185·0186, logind 설정). s2idle이 돌아오도록 부트로더가 켠 감시 타이머를 잠자기 때 멈추고(0184), `univpll`과 화면 전원 도메인을 끄지 않고(0187·0188), 가상 터미널 전환을 뺐고(0189), 잠자기 때 BGF MCU를 펌웨어에 넘기지 않으며(0190), 컴퓨터에 꽂힌 USB 가젯을 잠든 동안 뗀다. 우회가 많아 잠자기 전력은 순정보다 클 것이고 재지 않았다. 잠든 동안 충전은 멈춘다. 음량 키는 buntOS 쪽이다.
- v1.59(2026-11-06): Bluetooth 키보드가 된다. BLE 키보드는 연결되어도 입력이 오지 않았다. BlueZ가 입력 장치를 만드는 `/dev/uhid`가 커널에 없었기 때문이다(`input-hog profile accept failed`). `CONFIG_UHID`와 `CONFIG_INPUT_UINPUT`을 모듈로 켰다. 입력 장치가 생기면 컴포지터가 포인터 모드로 바뀌어 터치 키보드가 뜨지 않는다. 키보드나 마우스 하나만 있어도 된다. AirPods 페어링은 사용자가 확인했다. 블루투스 마우스와 전력은 아직 못 봤고, `CONFIG_BT_RFCOMM`이 꺼져 있어 이어폰을 마이크로 쓰는 프로필은 없다.
- v1.60(2026-11-07): CPU 유휴 상태(cpu-off, cluster-off)가 된다. BL31은 MCUPM 메일박스의 작업 상태가 2나 3이 되어야 PSCI CPU_SUSPEND를 받고, MCUPM은 AP가 시간 기준을 써 줘야 그 상태로 간다(패치 0193~0195, 장치 트리의 mcupm 노드와 방송 타이머). 화면만 켠 전력이 cluster-off로 약 0.06~0.07 W 준다. 켜자 화면 아래쪽이 한 프레임씩 깨졌다: 두 파이프가 프레임 끝 인터럽트에서 평면 설정을 쓰고 곧바로 flip을 끝내, CPU0이 늦게 깨면 컴포지터가 스캔 중인 버퍼에 그렸다. OVL 레이어 레지스터는 그림자라 커밋 때 쓰고, 반영을 확인한 뒤 flip을 끝내게 고쳤다(0197). 사용자가 8개 코어 깊은 유휴에서 깜빡임이 없음을 확인했다
- v1.61(2026-11-09): 직충전을 기본으로 켰다. PPS 충전기에서 UPM6720 차지 펌프로 드라이버가 4.2 A까지 요청해 펌프 입력 약 40 W, 배터리 +8.2 A를 봤고, 표시 잔량 52%에서 81%까지 25분이 걸렸다. 직충전 중 뽑기와 다시 꽂기도 봤다. BSP의 modprobe 설정(`direct_charge=1`)으로 켜고, 드라이버의 기본값은 그대로 꺼짐이다. 첫 4.2 A 단계에서 칩이 한 번 스스로 멈춘 까닭은 찾지 못했다. 직충전 중 UPower에 남은 시간이 안 나오는 것은 UPower가 TCPM의 `online` 값 2를 받지 못해서다(`bringup-checklist.md`).
- v1.62(2026-11-09): 직충전 중에도 UPower가 충전 속도와 남은 시간을 내도록 고친 데몬을 넣었다(`bsp/lenovo-tb375fc/upower/`, 꾸러미 `buntos-upower`). PPS 중 USB 포트의 `online` 값 2를 불리언 속성에 그대로 넣던 것을 0이 아니면 연결로 읽는다. Debian의 upower는 그대로 두고 `/opt/upower`의 데몬을 systemd drop-in으로 띄운다. UPower 통합 시험으로 고치기 전후를 봤고 태블릿에 깔았다. 실제 직충전 중 확인은 다음 충전 때다.
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
| 그 밖 | 앰프 AW88394 ×4, 홀 센서 둘, 센서 허브 SCP(가속도·조도), UART 포고 키보드(Tinno MCU), SAR 둘 |
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
| D3 | **처음에는 LK가 직접 띄운다.** boot v4 + vendor_boot v4(DT 표) + 빈 dtbo, 비운 슬롯 `_b`에 구워서(처음에는 `fastboot boot`로 RAM에서 띄우려 했으나 LK에 `boot` 명령이 없었다). 2b부터는 U-Boot 체인로드로 EFI(systemd-boot, UKI) 부팅을 검토한다 | 안드로이드 슬롯을 건드리지 않는 첫 부팅. EFI가 되면 원본 buntOS(데스크톱 라인)의 arm64 이미지 꼴과 같아진다. MT6878-mainline도 U-Boot를 권한다 |
| D4 | **첫 데스크톱은 simple-framebuffer다.** LK가 dual-DSI + DSC로 켜 둔 화면을 그대로 받는다(`clk_ignore_unused`, `pd_ignore_unused`, `regulator_ignore_unused`). 화면 파이프라인(mediatek-drm의 MT6897 데이터, dual-DSI, DSC)은 7.2.12 위의 작은 패치(0120~0139)로 만들어 두었고, 켜는 것은 뒤에 한다 | 화면이 가장 큰 막힘이다. 그 사이에 나머지(저장장치, USB, 터치, Wi-Fi, GPU)를 진행할 수 있다. `pd_ignore_unused`는 늦은 정리(`genpd_power_off_unused`)만 막고 sync_state는 막지 못한다. 화면 도메인을 지키는 것은 표의 기본 꺼짐과, 장치 트리에 화면 도메인을 적지 않는 것이다. MM_INFRA 아래 소비자(ISP, 카메라, VDE, VEN, MDP, DP)를 처음 붙이기 전에는 `simple-framebuffer`가 `power-domains`로 화면 도메인을 잡아야 한다. genpd는 MM_INFRA를 마지막 사용자와 함께 끈다.. 화면·코덱 클럭 노드(패치 0100~0104)는 도메인의 소비자가 아니다. `power-domains`가 없고 probe가 블록과 도메인에 닿지 않으므로, 켜도 MM_INFRA 규칙에 걸리지 않는다. 그 게이트의 소비자가 규칙에 걸린다. `clk_ignore_unused`를 빼면 이 게이트는 그대로지만 topckgen 먹스가 닫힌다. `simple-framebuffer`가 화면 도메인을 잡으면 그 기본 클럭(`mminfra`, `disp0`, `disp1`, `ovl0`, `ovl1`)은 genpd가 잡아 남는다. |
| D5 | **Wi-Fi는 mt76에 "MT7921의 SoC 변형"을 더한다.** 펌웨어는 LK가 실은 것을 쓰고, conninfra 전원은 TF-A SIP 호출로 켜고, nvdata 보정값은 NVRAM 명령으로 넘긴다. 그 전에는 USB 동글로 우회한다(USB 호스트는 MT6375 OTG가 먼저다). 코드는 패치 0055~0067이다. DMA는 맵 없는 LK 칸 안에서 자기 `dma_map_ops`로 복사한다(업스트림 꼴은 아니다, 6절) | 칩 ID와 구조가 MT7921과 같고, 명령 머리와 레거시 명령 번호도 같다. SoC Wi-Fi 선례(mt7915/soc.c)가 있다([설계](wifi-soc7_0.md)) |
| D6 | **펌웨어는 설치할 때 기기에서 뽑는다.** Mali CSF만 linux-firmware에서 온다. GPUEB와 Wi-Fi·BT는 LK가 올리므로, 그 파티션(GPUEB, `connsys_wifi`, `connsys_bt`)을 지킨다 | 재배포할 수 있는지 모른다(buntOS ADR-012). 안드로이드 슬롯 하나를 남긴다 |
| D7 | **GPU는 panthor + MT6897 GPUEB 변형이다.** 이미지에 Mesa 26.3 이상이 필요하다 | panthor·펌웨어·Mesa가 이미 G615를 안다. GPUEB는 LK가 띄우고 멈추지 않으며 전원·클럭·전압을 모두 맡는다. 커널은 메일박스로 켜고 끄기와 OPP만 요청한다. MFG1~14 도메인과 MFG PLL은 커널이 건드리지 않는다 |
| D8 | **이 기기에서는 가상화를 포기한다.** 커널이 EL1로 시작해 KVM을 쓸 수 없다(2a 판정, 2026-11-03). 일반 개발 환경에는 EL1로 충분해 이 기기로 그대로 진행한다(사용자 결정, 2026-11-04). 다른 태블릿을 매입하면 그때 사용자가 말해서 KVM을 다시 시도한다. gzvm(미병합)은 나중에 검토한다. TEE를 고쳐 EL2를 빼앗는 실험(`mtk-mod-tee-nogz`)은 쓰지 않는다 | 서명 우회가 필요하고 벽돌 위험이 크다 |

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
  - 개발용 LK는 4pda에 올라온 덤프에서 나와 출처를 확인할 수 없다. 2026-11-03에 받은 파일(XDA 원글의 `lk_a`)은 순정 LK와 같은 키로 서명됐고 서명 뒤 고친 흔적이 없다(`userdebug` 빌드, 체크리스트 3절 4번). 안전을 보장하지는 않는다.
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
| init_boot | 헤더 v4, 우리 initramfs(`root=`로 넘어가기. 못 넘어가면 USB 가젯 셸과 nvdata 마운트) | LK가 initrd로 넘긴다. 8MiB 파티션에 3.4MB다(btrfs 포함) |
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
  - LK는 화면을 자기 안의 장치 트리(`lk_main_dtb`, `lk_dtbo`. 순정 vendor_boot·dtbo와 해시가 같다)로 켠다. 커널 트리는 부팅 직전 후처리 훅에서만 본다. 훅 `disp_lcm_fill_lcm_dts_phandle`은 자기가 찾은 패널 번호 n(1~3)으로 `/soc/dsi/panel<n>/port/endpoint`의 phandle을 읽어 `/soc/dsi/ports/port/endpoint`의 `remote-endpoint`에 쓴다. 경로를 못 찾으면 기록만 남기고 넘어간다(LK 역어셈블). 우리 트리에는 이 경로를 두지 않는다. 패널은 GPIO184로 고른다(5절). 2a에서 LK 기록으로 확인한다.
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

- 첫 부팅은 비운 슬롯 `_b`에 구워서 한다. 처음에는 `fastboot boot`로 RAM에서 띄우려 했다. 기기에서 `Booting FAILED (remote: 'unknown command')`였고, 명령 표를 읽어 보니 16.0.20.091 순정 LK와 개발용 LK 모두 `boot`를 등록하지 않는다(2026-11-03). 앞서 근거로 삼은 LK 문자열의 `boot`는 파티션 이름이었다. 아래 두 항목은 그때의 검토다.
  - `fastboot boot`는 boot.img 하나만 RAM으로 보낸다. 헤더 v4에서 장치 트리는 vendor_boot에, initramfs는 init_boot에 있다(AOSP 문서).
  - LK는 boot·init_boot·vendor_boot 머리를 모두 읽고, 장치 트리를 예약 메모리 `mb_dtb`로 읽어 커널에 넘긴다(LK 문자열). 그래서 RAM으로 보낸 boot.img에 굽혀 있는 원래 vendor_boot·init_boot를 붙일 가능성이 높다. 이 결합만 기기에서 확인하지 않았다.
  - 그래서 init이 vendor_boot의 표지로 LK가 우리 vendor_boot를 읽었는지 알린다. initramfs를 boot.img에도 넣은 이미지(`mkimages.sh --boot-ramdisk`)로 LK가 그 램디스크를 쓰는지도 본다.
  - 원래 장치 트리로 뜨면 비운 슬롯에 넷을 굽는 길로 간다.
  - 굽기 전에 한 번 더 보려면, 커뮤니티 포트처럼 장치 트리를 커널에 넣은 시험용 커널을 `fastboot boot`로 띄울 수 있다. 이 커널은 LK가 더하는 메모리·예약 값을 버리므로, 그 기기의 메모리 크기에 맞춘 예약을 함께 넣어야 한다. 업스트림에는 내지 않는다.
- 디버그 채널:
  1. **화면 콘솔:** simple-framebuffer + simpledrm. 커뮤니티 포트도 LK 화면 위의 fbcon을 첫 통로로 썼다. 부팅 단계마다 화면에 블록을 그리는 카나리와 `panic=0`(패닉 화면을 남긴다)도 쓸 수 있다
  2. **pstore/ramoops:** 직전 부팅의 커널 기록을 남긴다. 벤더와 같은 자리(LK가 명령줄로 주는 `0x48090000`)에 두고, 순정 recovery로 재부팅해 읽는다
  3. **LK 기록:** expdb 파티션에 남는다. 순정 recovery에서 뜬다
  4. **USB 가젯(2b):** configfs ACM + NCM. 포트가 하나이고 호스트 모드가 없어 화면 말고는 유일한 길이다. 스크립트 하나(`usb-gadget`)를 둘이 쓴다.
     - 넘어가지 못했을 때: initramfs가 띄운다. ttyGS0 셸, `buntos.telnet=1`이면 telnetd
     - 넘어간 뒤: 진짜 루트의 서비스 셋이 띄운다. ttyGS0의 암호 없는 root 셸과 SSH(172.16.42.1에서만 듣는다). 명령줄에 `buntos.usbdebug=1`이 있을 때만 돈다
  5. **UART:** 커뮤니티 포트도 쓰지 않았고(uart0을 꺼 둠), 시험 패드 위치와 전압이 공개된 곳이 없다. 기기를 열어 찾는다. 포고 핀 UART3(GPIO180·181)은 키보드 줄이다.
- recovery는 따로 된 파티션이 없고 vendor_boot 안에 있다(중국판 펌웨어 묶음에 recovery.img가 없다). 그래서 우리 vendor_boot를 구운 슬롯에서는 순정 recovery를 쓸 수 없다. 남겨 둔 슬롯으로 바꿔 recovery를 쓴다.
- 루트 파일 시스템:
  - 2a는 initramfs다.
  - 2b부터는 명령줄의 `root=`가 가리키는 파티션이다. 자리는 4절에 적는다.

## 3. 단계별 bring-up

| 단계 | 할 일 | 종료 조건 |
|---|---|---|
| 2a 첫 부팅 | 업스트림 꼴 트리(CPU, GIC, 타이머, PSCI, 메모리, simple-framebuffer, ramoops, UART), 이미지 꼴, kernel.org 원본과 대기열의 커널 | initramfs 셸이 화면이나 UART에 뜬다. 커널 기록으로 EL2 판정(12.1절) |
| 2b 저장장치·디버그 | UFS(`ufs-rtff-mtcmos`, 처음에는 MCQ 끔), USB 주변 장치(mtu3·xsphy), 핀·EINT(mt6878 틀), 클럭 컨트롤러 여섯(`clk_ignore_unused`는 둔다), SPMI(MT8196 데이터, P 버스 끔), MT6319(DT만), MFD·MT6363, cpufreq-hw. PMIC 쪽은 핀(EINT) → SPMI → MT6319 → MFD·MT6363 순서다 | UFS의 Debian 루트, USB 가젯 SSH |
| 2c 화면·입력 (1차) | simple-framebuffer로 buntOS 데스크톱, NT36532 SPI 터치(펌웨어 내려받기), 펜, 홀 센서(gpio-keys), 포고 키보드(serdev) | 2944×1840으로 buntOS 로그인 화면, 터치·펜 조작, 키보드 커버를 붙이면 포인터 모드 |
| 2d 무선 | conninfra(SIP 호출, PMIC 전원, conn 전원 도메인), mt76 soc7_0(LK 적재본 기동), nvdata 보정값, FEM, Bluetooth(BEIF) | Wi-Fi 연결, Bluetooth 키보드·이어폰 |
| 2e GPU·전원 | panthor + GPUEB 변형, Mesa 26.3, 클럭 드라이버 완성(`clk_ignore_unused` 빼기. 화면·코덱 게이트는 패치 0100~0104에 있고 빼도 닫히지 않는다. 남은 것은 먹스(화면 먹스 다섯은 `framebuffer0`이 도메인을 잡으면 genpd가 쥔다), UFS 게이트, `mfgsc_ref`다. MFG PLL과 GPU 먹스는 만들지 않는다. EB 모드 벤더도 쓰지 않는다), LVTS 열, MT6375 충전·TCPC, MM8013, 서스펜드 | GLES로 컴포지터, 서스펜드·복귀 반복, 배터리·충전 표시 |
| 2c 화면 (2차) | mediatek-drm의 MT6897 데이터, dual-DSI, DSC, NT36532 패널 셋(업스트림 드라이버 + compatible 셋) | 주사율·밝기 조절, 화면 끄고 켜기 |
| 3 최적화 | 8절, 오디오, 외부 화면 | buntOS 설계안 9.3절 스모크 시험 |

- 화면 2차를 2e 뒤에 켜는 것은 첫 데스크톱이 simple-framebuffer로 돌기 때문이다(D4). 업스트림의 DSC·mediatek-drm 재구성 시리즈는 7.4 병합 창에 들어가지 않아 빨라야 7.5이므로(2026-11-02), 코드는 7.2.12 위의 작은 패치 0120~0139로 먼저 만들었다.
- USB 호스트(xHCI, 저장장치·동글·키보드)는 2b에서 뺐다. 태블릿의 VBUS는 MT6375 OTG에서 나와 차지 펌프의 입력 FET를 지난다(4절). 드라이버는 패치 0042~0049에 있고 노드는 꺼 두었다. UFS 루트와 USB 가젯이 기기에서 된 뒤에 켠다. 2d의 USB 동글 우회도 이것을 기다린다.
- 전원 도메인은 UFS·USB에 필요 없어 2b에서 뺐다. 드라이버와 표는 패치 0050~0054에 있고, 노드는 꺼져 있다. 장치 트리에는 소비자가 생길 때 그 도메인부터 더한다: 2d(conn), 2c 2차(화면). GPU는 `mtk-mfg-pmdomain`이다.

## 4. 커널 구성

- **기반(D1):** kernel.org 원본 소스다. 판은 buntOS 이미지의 Debian forky 커널과 같고(지금 7.2.12), 포팅이 진행되면 그때의 Debian 판을 따라간다. 7.2.y stable 고침은 kernel.org에서 바로 따라갈 수 있다.
- **이미지용 커널:** 같은 원본 소스와 대기열을 이미지와 같은 시점의 Debian forky 커널 설정 + 보드 조각으로 빌드한다(`kernel/tb375fc/debian-config.sh`가 설정을 만든다). 그래서 Debian 커널과 달라지는 것은 Debian 패치에만 있는 설정 셋(binder 모듈, UEFI Secure Boot 잠금, perf 제한)과 우리 대기열·조각이다.
  - 빌드(v1.10): `build.sh image`가 자기 작업 트리와 출력(`linux-7.2.12-image`, `out-image/`)을 써서 공용 트리를 잠그지 않는다. 설정 검사는 대기열을 다 얹은 트리로 하고, 조각의 값이 하나라도 빠지면 멈춘다. 꾸러미는 `linux-image-7.2.12-tb375fc`, `linux-headers-…`, `linux-image-…-dbg`, `linux-libc-dev`이고, 판은 `7.2.12-<n>+g<커밋>`이다(n은 `kernel/tb375fc`와 `image/mkosi.conf`를 바꾼 커밋 수, 커밋하지 않은 고침이 있으면 `~dirty`). 커널은 압축하지 않은 Image를 `/boot/vmlinuz-<판>`으로 싣는다(Debian forky arm64와 같고, UKI와 LK가 그대로 쓴다)
  - 크기와 시간(6코어 빌드 VM, 0001~0022): 컴파일 약 54분, 꾸러미 약 9분. image 97.5MiB(설치 256MiB), headers 9.7MiB, -dbg 1.64GiB(설치 4.4GiB). 빌드 디스크는 작업 트리 2.4GB와 출력 17GB가 정점이라 맥 여유가 약 28GB일 때 시작한다. -dbg는 객체 파일을 지운 뒤 따로 만들고, 끝나면 출력을 지운다. 그래서 다음 이미지 빌드는 처음부터 하고, 이어 빌드는 개발 트리가 맡는다. 고친 흐름을 처음부터 끝까지 한 번에 돌리는 확인은 디스크가 날 때 한다
  - 툴체인: 빌드 VM의 trixie gcc-14로 한다. bindgen이 없어 Rust 쪽(Rust PHY 둘, 패닉 화면 QR 등)이 빠지고, gcc 15 이상의 `__counted_by`, binutils의 LSUI, 32비트 compat vDSO도 빠진다. 이 기기가 쓰는 것은 없다. Debian과 같은 컴파일러가 필요해지면 forky 컨테이너를 쓴다
  - 디버그 정보(2026-11-01 결정): Debian 설정 그대로 DWARF·BTF를 켠다(배포판의 정석). 설치하는 커널·모듈은 strip하고 디버그 기호는 `-dbg` 꾸러미로 둔다. 빌드 VM에서는 디스크가 빠듯해 디버그 섹션을 zstd로 줄인다(`DEBUG_INFO_COMPRESS=none`이면 줄이지 않고, 출력이 약 24GB가 된다). 배포 커널은 같다
  - mkosi(v1.10): 보드 프로필(`bsp/lenovo-tb375fc/mkosi.conf`)이 지역 꾸러미 디렉터리의 커널을 넣고, mkosi가 `/boot/vmlinuz-<판>`을 `/usr/lib/modules/<판>/vmlinuz`로 복사해 UKI를 만든다. 공통 레시피의 `Bootable=yes`·`Bootloader=systemd-boot`는 보드 프로필에서 바꿀 수 없다(mkosi가 보드 프로필을 먼저 읽는다). 그래서 ESP·systemd-boot·UKI가 그대로 생기고, D3의 EFI 길과 맞으므로 지금은 그대로 둔다. LK에는 루트 파티션만 굽고, 부트 이미지는 같은 꾸러미의 Image·장치 트리·모듈로 만든다
  - initrd 길(2026-11-01 결정): 우리 initramfs에 루트 전환을 더한다. `root=`를 붙여 `switch_root`하고, 실패하면 USB 가젯 셸로 남는다. mkosi의 systemd initrd는 38MB라 init_boot 8MiB에 들어가지 않는다. 이 코드는 루트 자리에 매이지 않는다
    - 만든 것(v1.14): `initramfs/init`의 `try_root`. `root=`(PARTLABEL·PARTUUID·UUID·경로), `rootfstype=`, `rootflags=`, `rw`, `ro`, `init=`을 읽고, 장치를 `buntos.wait`초 기다려 `/sysroot`에 붙인 뒤 `/proc`·`/sys`·`/dev`·`/run`을 옮겨 `switch_root`한다. configfs·debugfs·pstore·devpts는 함께 옮겨지고, systemd는 이미 붙은 것을 그대로 쓰므로 같은 옵션으로 붙인다. `/run`이 tmpfs라 `/run/buntos`의 기록이 남는다. 이때 도는 프로세스는 이 셸뿐이다
    - 실패 길: `switch_root`는 initramfs를 지운 뒤에 exec하므로 그 뒤의 실패는 패닉이다. 그래서 앞에서 모두 걸러 낸다. `run-init -n`(같은 코드의 시험 운전), 머리 20바이트(arm64 ELF나 `#!`), `LD_TRACE_LOADED_OBJECTS`로 로더와 라이브러리. `/proc`을 옮기다 실패하면 되돌린다. 어느 경우든 까닭을 남기고 busybox init과 USB 가젯 셸로 이어 간다
    - LK의 값: 순정 트리의 `/chosen`은 `root=/dev/ram`이고 탭이 섞여 있다. `PARTLABEL=` 꼴을 경로보다 먼저 고르고, 램디스크·dm·별칭과 `init=/init`을 버린다. `rw`가 있으면 `ro`를 무시한다
    - 안드로이드 파티션: 넘어가는 길에서는 건드리지 않는다. `--late`의 nvdata 마운트는 실패 길에서만 돈다. 진짜 루트에서 nvdata를 쓰는 일은 2d다
  - 루트 자리(2026-11-01 결정): userdata를 줄여 만든 새 GPT 파티션이다. 2b부터 이 자리를 쓴다. 형식 GUID는 root-arm64이고 명령줄은 `root=PARTLABEL=<이름>`이다. 안드로이드 슬롯은 남고, userdata는 한 번 지운다
    - 순서: 2b의 USB 가젯 셸에서 UFS를 읽기 전용으로 확인하고 파티션 표를 읽는다 → 크기와 이름을 정한다 → 고치기 전의 GPT를 떠 둔 뒤 GPT를 고친다 → 루트 파티션 파일을 굽는다(fastboot, 안 되면 가젯으로 `dd`)
    - 되돌리는 길은 언락 전에 뜬 SP Flash 백업이다. LK의 fastboot가 새 항목을 아는지, preloader·LK가 고친 GPT를 어떻게 읽는지는 기기에서 본다
    - userdata 전체·super·루프 파일은 안드로이드 슬롯을 지키지 못하거나 metadata 암호화로 열 수 없다
    - 안전(v1.14): 이미지가 기기에서 GPT를 고치지 못하게 막는다. `systemd-repart`(공통 레시피의 `Type=root`)는 루트가 든 디스크의 GPT를 다시 써서 뒤의 빈 자리까지 늘린다. `systemd-gpt-auto-generator`는 EFI가 없어도 그 디스크의 GPT를 읽어 형식이 맞는 파티션을 스왑, `/home`, `/srv`, `/var`, `/var/tmp`로 붙인다(systemd 262의 `add_mounts`는 EFI를 보지 않는다. `LoaderDevicePartUUID`는 루트 찾기에만 쓴다). 보드 프로필이 둘을 `/etc`에서 가리고, initramfs가 `/run`에서 또 가린다. 그래서 다른 보드의 루트를 잘못 구워도 막힌다
    - 늘리기: `systemd-growfs-root.service`를 `local-fs.target`에 건다. `BTRFS_IOC_RESIZE`로 파일 시스템만 파티션 끝까지 늘리고 파티션 표는 건드리지 않는다. fstab의 `x-systemd.growfs`는 쓰지 않는다. 파티션 이름을 이미지에 적어야 하기 때문이다
  - USB-C 저장장치 길은 쓰지 않는다(2026-11-01). 처음에 골랐다가, 순정 트리와 7.2.12 드라이버에서 전제를 확인하고 바꿨다
    - VBUS: 태블릿이 저장장치에 주는 5V는 MT6375 OTG 레귤레이터에서 나오고(`mt6375_otg_vbus`, 순정 트리에서 `extcon-usb`만 쓴다) 차지 펌프의 입력 FET를 지나 커넥터로 나간다. 순정은 켤 때 FET(`charger_dev_typec_mux_otg_enable`) → 5V·1.8A → 부스트 차례이고, 끌 때는 그 반대다. 스톡 모듈과 제3자 미러의 "enable ovp mos by dvchg"로 확인했다. 회로도는 없어 FET 없이 5V가 나가는지는 기기에서 본다. GPIO179(`vbus_mos_gpio`)는 별개로 USB 커넥터 과열 보호다: 평소 low, 충전기가 붙어 있을 때 `usb_board_ntc`가 75도를 넘으면 high(스톡 `usb_temp_protect`). OTG 중에는 순정도 이 검사를 돌리지 않는다
    - 포트 하나: 호스트 모드에서는 USB 가젯 셸을 쓸 수 없다. 그동안 디버그는 화면과 pstore에 기댄다. 역할은 `dr_mode = "otg"` + `usb-role-switch` + `role-switch-default-mode = "peripheral"`로 바꾼다. `/sys/class/usb_role/11201000.usb-role-switch/role`을 사용자 공간에서 쓸 수 있다(`mtu3_dr.c`의 `allow_userspace_control`). `enable-manual-drd`는 호스트로 시작해 2b 가젯이 뜨지 않으므로 쓰지 않는다
    - VBUS를 켜는 길: TCPC 전까지는 `regulator-output`의 `state`다. `&ssusb`의 `vbus-supply`는 쓰지 않는다. 바인딩에서 deprecated이고, otg 모드에서는 `ssusb_host_setup()`이 프로브마다 켠다(PC가 붙어 있어도)
    - 데이터 줄: D+/D−는 xsphy 초기화가 BC1.1 경로를 지워 컨트롤러로 간다(`u2_phy_instance_init`). USB3는 PS5170 리드라이버(I2C2 0x28, GPIO16 켜기, PMIC VS 투표, TCPC 방향)가 있어야 해 드라이버 전에는 HS로만 붙는다. CC는 TCPC가 LK 상태 그대로이고, Rp·VCONN을 내지 않는다
    - 커뮤니티 포트는 USB를 주변 장치로만 쓰고, MT6375의 USB 쪽 노드를 일부러 뺐다. 주석에 따르면 probe가 실패하는 도중에 Type-C 스위치를 건드려 USB가 끊길 수 있다. 호스트 모드를 이 기기의 mainline에서 돌린 사례는 찾지 못했다
  - 남은 결정: 기기에서 커널을 바꾸는 길(LK 부트 이미지를 다시 만들어 쓴다)
  - Debian 패치에서 골라 넣을 후보: binder 모듈(안드로이드 앱 컨테이너를 쓸 때), perf 제한(보안 기본값), 무선 규제 DB 인증서(2d, Debian이 서명한 DB를 쓸 때)
- **설정과 패치:** 보드 설정 조각(`kernel/config/tb375fc.config`)과 업스트림 제출 대기 패치 대기열(`kernel/patches/`)을 둔다. 대기열에 처음 들어갈 것:
  - NT36532 패널 드라이버 백포트(7.4 병합 창, `47b823940e38`)와 BOE·Tianma 패널 compatible
  - `spmi-mtk-pmif` 고침 백포트(패치 0002·0003): FSM 멈춤 우회(`84c72fc05f73`, 7.4 병합 창), 클럭 참조 누수(`636c4c026f47`). 코드 대조에서 나온 결함 둘은 고침을 0007·0008로 얹었다. 처리기 없는 SID의 RCS를 아무도 지우지 않는 것은 0114로 고쳤다. rcs 디먹스가 SID당 8비트를 7로 나눠 처리기가 끝나지 않을 수 있고, 채널 설정이 버스 등록 뒤에 있어 내장(=y) PMIC 드라이버가 채널 없이 버스를 쓸 수 있다. 기기에서 확인한 뒤 업스트림에 낸다
  - `pinctrl-mt6897`(EINT 포함, 패치 0004~0006: 바인딩, pinmux 매크로, 드라이버). GPIO가 없는 EINT 232~249는 업스트림 MT8196처럼 가상 핀이다
  - `clk-mt6897`(패치 0009~0013): 바인딩, HW voter syscon, FENC 없는 HW voter 먹스와 PLL HW voter(업스트림 틀에 더한 것), 2b 블록 일곱의 드라이버
  - MFD `mtk-spmi-pmic`(패치 0014~0016): v12에 리뷰 지적과 인터럽트 고침(마스크 레지스터, 초기 마스크, RCS 완료, 깨우기)을 얹었다. 새 판이 나오면 따른다. MT6363 키(0017·0018)는 새로 썼다. MT6685는 패치 0112다: 인터럽트 그룹이 없어 regmap도 칩 읽기도 없이 자식만 띄운다(버스 접근 0번). MT6319는 MFD 없이 `mt6315-regulator`에 DT만 쓴다
  - UFS(패치 0019~0022): MT6897 호스트·M-PHY 바인딩과 드라이버. 클럭·리셋 이름과 OPP는 업스트림 MT8196 시리즈(v14, 병합 전) 꼴이고, 그 시리즈가 들어오면 그 위로 다시 쓴다
  - MT6363 regulator 고침(패치 0037~0041): 8비트 값 regmap, VEMC 전압, VSRAM 레지스터는 regulator `for-7.4`에 적용된 커밋이다. OC 처리(0039, 0040)는 기기 시험 뒤 낸다
  - UFS 3.1 장치에서 HS-G5 동기 길이 건너뛰기(패치 0144, 낼 후보)
  - USB 장치 모드(패치 0141~0143): `mediatek,force-vbus`(본가에 없음, 낼 후보), XS-PHY가 부트로더의 강제 비트를 지운다(본가 `02df7fe1297d` 백포트)
  - `mt6897` 전원 도메인(패치 0050~0054): 바인딩 둘, probe 실패 때 기본 꺼짐 도메인을 끄지 않는 고침, 한 제어기에 직접 제어와 HW voter, 표. 노드는 꺼 두었다
  - NT36532 SPI 터치와 펜(패치 0027~0031: 핀 drive-strength와 pinmux 헤더 고침, SPI 바인딩의 MT6897, 터치 바인딩·드라이버)
  - 포고 키보드(패치 0032~0036: v6 백포트와 리뷰 지적, 레노버 옛 꼴, UART 바인딩의 MT6897과 패드 모드, 8250_mtk 고침)
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
| 전원 도메인 | `mtk-pm-domains`(MT8196) | MT6897 표(직접 제어 25, HW voter 7. 벤더의 33번째 APU는 레지스터가 없다)와 한 제어기에 두 종류를 두는 고침. 패치 0050~0054와 꺼 둔 노드(conn 하나)가 있다. UFS·USB에는 없다. GPU 12개는 GPU 전원 줄로 | 중간 | 2d·2c(2차) |
| 클럭 | `clk-mt8196-*`(틀) | `clk-mt6897`(OEM 표). FENC 없는 HW voter 먹스와 PLL HW voter를 더한다. 2b 블록은 패치 0009~0013, I2C 게이트는 0043·0044, 화면·MDP·코덱 게이트는 0100~0104에 있다. 카메라·ISP 블록 20개(게이트 136)와 오디오·SCP 블록은 없다 | 큼 | 2b~2e |
| SPMI·MT6363·MT6319 | `spmi-mtk-pmif`, `mt6363-regulator`, `mt6315-regulator` | SPMI는 MT8196 대체 compatible(P 버스 끔)과 고침 다섯(0002·0003·0007·0008·0114), MT6319는 DT만, MT6363은 MFD·키 패치(0014~0018)와 regulator 고침(0037~0041). SPMI 노드, MT6363(키, 레일 23개), MT6319, UFS 공급은 장치 트리에 있다 | 작음~중간 | 2b |
| MT6368·MT6685·MT6375 | 없음 | MT6368은 패치 0023~0026(regulator 바인딩·드라이버, MFD 데이터)과 보드 노드(펌웨어가 늘 켜 두는 레일 6개, USB `vusb33`). 진동(VIBR)·지문(VFP)·오디오(VAUD18) 레일은 소비자와 함께 3에서 노드를 넣는다. MT6685는 패치 0110~0113(바인딩 둘, MFD는 자식만 띄움, RTC). 시간 읽기와 쓰기만, 쓰기는 게이트 둘·시간 칸 여섯·트리거로 막았다. 알람과 깨우기는 뒤다. 클럭 버퍼는 소비자가 없어 만들지 않는다. 단계 3 오디오의 BBCK5가 첫 소비자다. MT6375는 패치 0045~0047(OTG 부스트)과 0090~0097(충전, 인터럽트, TCPC): 프로브 쓰기 0번은 인터럽트 없는 기본 상태에서 그대로다. 게이지·ADC와 PD 고속 충전은 뒤 일이다 | 중간~큼 | 2b·2e·3 |
| UFS | `ufs-mediatek`, `phy-mtk-ufs` | 패치 0019~0022: MT6897 compatible(호스트는 match data로 `rtff-mtcmos`·`broken-rtc`·`pmc-via-fastauto`·`tx-skew-fix`), 리셋 셋, M-PHY 새 판(클럭·전원 순서 없음). VCC·VCCQ는 보드. 클럭 스케일링과 MCQ(큐 인터럽트 여덟)는 뒤로 | 작음~중간 | 2b |
| USB | `mtu3`, `xhci-mtk`, `phy-mtk-xsphy` | DT(펌웨어의 eye 값). 호스트 모드는 VBUS 드라이버(0045~0049, 꺼 둠) 뒤에 otg 모드로, 역할은 사용자 공간에서 바꾼다. 자동 전환은 TCPC 뒤. xsphy MT6897 꼴(U2 INTR_CAL 칸과 efuse, U3 CDR 값)과 깨우기 접점(벤더 판 103)은 드라이버 고침이 필요하다 | 작음~중간 | 2b·3 |
| I2C | `i2c-mt65xx`(mt8188 데이터) | 바인딩 한 줄(0042), 게이트 블록 다섯(0043·0044), dtsi의 컨트롤러 14개(꺼 둠). 벤더 mt6897 데이터가 mt8188과 같다. 벤더의 FIFO 길은 업스트림이 안 쓴다 | 작음 | 2c·2e |
| 화면 파이프라인 | MT8195·MT8188까지. DSC v9·재구성 v2 시리즈는 미병합, dual-DSI는 아무도 맡지 않았다 | 패치 0120~0139: MT6897 데이터, dual-DSI(포치 나누기, 두 링크 동시 명령, 링크 속도 998.137 Mbps, 영상 중 명령), DSC(패널이 채운 값을 그대로 쓴다). 기기 시험 전이다. 업스트림 시리즈가 들어오면 백포트 여섯을 빼고 경로·뮤텍스·컨트롤러 코드를 새 API로 옮긴다. 파이프 나누기와 두 링크 DSI는 남는다 | 큼 | 2c(2차) |
| 패널 NT36532 | `panel-novatek-nt36532`(7.4 병합 창, 패치 0080~0082로 백포트). master와 next-20261030에 고침 없음 | 패치 0083·0084: `lenovo,tb375fc-nt36532` 하나에 GPIO184로 BOE·Tianma를 고른다. 모드 넷과 주사율 레지스터, DSC 1.1(첫 줄 오프셋 13), 편향 전원. 밝기는 패널이 아니라 `disp_pwm0`과 `pwm-backlight`다(0154·0155). SGM37604A 둘은 LK가 맞춘 값을 지켜 드라이버가 필요 없다. SM5109는 편향을 껐다 켜면 전압 값을 잃는다 | 작음 | 2c(2차) |
| 터치 NT36532 SPI + 펜 | I2C 드라이버뿐(SPI RFC 둘은 미병합) | 새 드라이버 `novatek-nvt-spi`(패치 0030·0031): 페이지 단위 SPI 접근, 열 때와 복귀 때 펌웨어 내려받기(ILM·DLM CRC 확인), 손가락 10점, 펜은 둘째 입력 장치(압력 4095, 기울기 ±60°, 단추 둘). 패널 ID GPIO184로 펌웨어를 고른다. SPI 컨트롤러 여덟은 dtsi, 터치 노드는 보드(꺼 둠) | 중간 | 2c |
| 홀 센서 | `gpio-keys` | 보드 노드(꺼 둠): GPIO1 = `SW_LID`(덮개 닫힘이 0). 펌웨어의 `hall_2`(GPIO26)는 키보드의 `plug-gpios`라 키보드 드라이버가 가진다 | 작음 | 2c |
| 포고 키보드 | v6 시리즈(미병합, Qualcomm 기기용) | 패치 0032~0034: v6 백포트(리뷰 지적 반영, 서스펜드)에 레노버 판을 더한다. 스톡 모듈로 보면 같은 Tinno MCU의 옛 꼴이다: `55×8, 0x25, cmd, e(len), e(payload), e(합), 0x2C, 55×4`, e()는 무작위로 고른 표 여덟 가운데 하나로 니블마다 치환, 명령은 키 0x61·미디어 0x62·터치패드 0x63·매개변수 0x6A·동기 0x7B·일반 0x92. CRC는 없다. 붙음·뗌은 GPIO26, RX 핀은 뗀 동안 GPIO, 깨우기는 GPIO40 상승 에지다. UART3 패드 모드(pericfg_ao `0x50`)는 8250_mtk(패치 0035·0036) | 중간 | 2c |
| Wi-Fi·BT | `mt7921`, `mt7915/soc.c`(선례) | soc7_0 버스, conninfra, 펌웨어, FEM, BT 전송 | 큼 | 2d |
| GPU | panthor(G615), linux-firmware CSF | DT. compatible `mediatek,mt6897-mali`(바인딩만, 드라이버는 일반 compatible). ASN hash 없음 | 작음 | 2e |
| GPU 전원 | `mtk-mfg-pmdomain`, `mtk-gpueb-mailbox`(MT8196) | 패치 0070~0073. MT6897 변형: LK가 띄운 GPUEB, 메일박스 11채널, 공유 메모리는 LK가 줌, 오프셋 셋, CMD_COMMIT, 절전 noirq 단계의 IPI(`IRQF_NO_SUSPEND`). MFG1~14는 GPUEB 몫 | 중간 | 2e |
| 열 | `lvts_thermal` | 패치 0075~0078. 도메인 넷(AP·MCU·APU·GPU), 센서 32개, 읽기 전용, 센서별 계수, devinfo NVMEM 새 드라이버 | 중간 | 2e |
| 게이지 | `mm8013` | 패치 0079(접근 전 4 ms, 읽기 재시도, 배터리 ID 0x0103), DT | 작음 | 2e |
| 차지 펌프 | 없음 | 입력 FET 스위치는 패치 0048·0049, 칩 한계와 직충전은 패치 0176·0177(드라이버 기본은 꺼짐, BSP가 켠다) | 중간 | 3 |
| 오디오 | MT8196·MT8189 AFE, MT6359 코덱, AW88395, FSA4480 | MT6897 AFE, MT6368 코덱(캡처까지, 패치 0160~0167), 재생과 accdet은 아직, 앰프 ACF | 큼 | 3 |
| SAR | `hx9023s` | HX9031AS를 hx9023s로 시도, HX9036 새로 | 작음/중간 | 3 |
| 지문 센서 | 모름 | 칩과 연결을 장치 트리에서 확인한다(실기기에 센서가 있고, 입력 장치 `gf-keys`가 있다) | 모름 | 3 |
| 센서 허브 | MT8183~MT8195 SCP만 | MT6897 SCP(nanohub) 길 조사 | 큼 | 3 |
| 외부 화면 | `mtk_dp`(MT8195), PS5169/70 패치 | 화면·TCPC 뒤 | 큼 | 3 |
| 비디오 코덱·카메라 | — | 범위 밖. 코덱의 클럭 게이트만 있다(0104). 소비자가 오려면 VCP 드라이버와 펌웨어가 먼저다: `vdec` 먹스의 게이트는 스톡에서 VCP가 IPI로 여닫는다 | — | — |

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
| Bluetooth 전송은 EMI 안의 공유 메모리 링(BEIF)이고, WMT 머리는 업스트림 btmtk와 같다. 링 머리는 호스트가 켤 때마다 쓴다. 펌웨어가 주는 값은 위치 둘뿐이다. 트리의 IRQ 셋 가운데 892는 BEIF 빌드에서 쓰지 않는다 | 벤더 `bt/mt66xx/btif`(`SUPPORT_BEIF=1`) |
| MCU 명령·이벤트의 머리와 레거시 명령 번호가 MT7921과 같다. uni cmd는 받지 않는다(2026-11-04 기기) | [Wi-Fi 문서](wifi-soc7_0.md) 6절 |
| 업스트림에 soc7_0 움직임은 없다 | patchwork 제목 검색, `openwrt/mt76` |

**설계.**
- 방식: mt76에 "MT7921의 SoC 변형"을 더한다. 할 일은 다음과 같다.
  - AXI MMIO 버스와 MT7902식 DMA 링 배치
  - conninfra 전원 순서(TF-A SIP 호출, PMIC 전원)
  - LK 적재본 기동: EMI 주소를 받아 remap하고 시작 명령을 보낸다. DMA는 LK가 연 20 MiB 칸 안에서만 한다
  - nvdata 보정값을 NVRAM 명령(CMD 0x48)으로 넘기기
  - FEM 고르기
  - 펌웨어가 uni cmd를 받지 않아 BSS·STA·키·ROC를 레거시 명령으로 보내는 길(패치 0064·0065)
  - Bluetooth: `drivers/bluetooth/btmtkbeif.c`(패치 0085·0086). H4 틀은 `h4_recv_buf`, WMT 구조체와 `btmtk_set_bdaddr`는 btmtk를 쓰고, WMT 동기 함수는 다른 btmtk 드라이버처럼 드라이버 안에 둔다
  - DMA: LK가 만든 칸은 `no-map`이고 compatible이 `shared-dma-pool`로 정해져 있다. 그래서 restricted-dma-pool(선형 맵 필요, 자기 compatible 필요)도 swiotlb(선형 맵 필요)도 못 쓴다. 벤더의 복사 길과 같은 꼴을 `dma_map_ops`로 썼다. `ARCH_HAS_DMA_OPS`가 필요하다(arm64는 XEN일 때만 고른다. defconfig와 Debian 설정은 XEN=y다). 업스트림에 낼 때 다시 정해야 한다. 소켓이 쥔 RX 페이지도 칸을 차지하므로 칸이 찰 수 있다. 칸이 차면 RX가 서지만 100 ms마다 다시 채워 되살아난다. 그 동안 명령도 실패할 수 있다. 압박이 있을 때 프레임을 복사해 칸을 비우는 길은 아직 없다.
  - uni cmd: 이미지와 벤더 코드로는 가릴 수 없어 런타임에 uni `DEV_INFO`를 보내 봤다. 멀쩡한 펌웨어에서도 시간 초과라(2026-11-04) 시험을 없애고 레거시 길(CE 0x07·0x11~0x14·0x1c)로 갔다
- 순서: 2b의 클럭·핀·PMIC가 먼저다.
- 우회: 그 전에는 USB 동글(mt7921u)이나 USB 이더넷을 쓴다.
- 함께 일하기: 커뮤니티 포트는 벤더 gen4m과 conninfra를 옮겨 왔고, WF ROM이 아직 뜨지 않는다. 코드는 겹치지 않으므로 레지스터 사실만 나눈다([분석](community-port.md)). MT6878도 soc7_0이라 그 mainline 포트와도 나눌 수 있다.

## 7. 펌웨어 방침

| 펌웨어 | 어디서 | 누가 올리나 | 재배포 |
|---|---|---|---|
| Mali CSF(`arm/mali/arch11.8/mali_csffw.bin`) | linux-firmware | 커널(panthor) | 가능 |
| GPUEB(`gpueb.img`) | 기기 파티션 | **LK**(벤더 커널에 request_firmware가 없다) | 해당 없음. LK는 부팅 중 `gpueb` 파티션을 읽어 올린다. 읽기에 실패하거나 MFG0이 꺼져 있으면 `app_load_gpueb(fdt, b_mode) == NO_ERROR` 단언에 걸려 LK가 멈춘다(LK 역어셈블). 그래서 이 파티션은 지우면 안 된다. |
| 터치 NT36532(`novatek_ts_fw_boe.bin`, `novatek_ts_fw_tm.bin`) | 기기 vendor → `lenovo/tb375fc/` | 커널(터치가 열려 있고 패널이 켜질 때) | 미확인. 기기에서 뽑는다 |
| Wi-Fi·BT soc7_0 | 기기 파티션(`connsys_wifi`, `connsys_bt`) | **LK**(conninfra EMI에 싣는다. 커널은 시작만 시킨다) | 해당 없음. 파티션을 지킨다 |
| Wi-Fi 보정값·MAC | 기기 nvdata | 커널(NVRAM 명령으로 넘긴다. initramfs가 nvdata를 붙인다) | 기기마다 다르다 |
| 앰프 AW88394 ACF(`aw883xx_acf_gyt.bin`, `aw883xx_acf_yc.bin`) | 기기 vendor | 커널 | 확인함(GPIO82로 고른다) |
| SCP·오디오 DSP | 기기 파티션 | 커널 | 미확인 |

- 펌웨어 추출 스크립트는 `firmware/`에 두고, 펌웨어 파일 자체는 저장소에 넣지 않는다(buntOS ADR-012).
- 설치기는 남겨 둔 안드로이드 슬롯이나 처음 백업에서 펌웨어를 가져온다.

## 8. 기기 맞춤 최적화 (buntOS 쪽)

| 항목 | 할 일 | buntOS에서 바꿀 곳 |
|---|---|---|
| 화면 배율 | 12.7″ 2944×1840은 약 273ppi다. buntOS 규칙(내장 화면은 논리 125ppi에 가깝게)이면 기본 배율은 **225%**(논리 1308×818)이고, 고를 수 있는 것은 100~275%다. DSI에는 EDID가 없으므로 패널(또는 simple-framebuffer 노드)이 실제 크기 약 273×171mm(벤더 값 273.615×171.009mm. 패널 드라이버는 273×171을 주고, simple-framebuffer 단계는 `framebuffer0`의 `panel` = `panel0`의 `width-mm`·`height-mm`로 simpledrm에 준다. 파이프라인을 켤 때 `panel`을 뺀다: fw_devlink가 패널이 붙을 때까지 simpledrm을 붙잡는다)를 알려야 이 규칙이 돈다 | 패널 드라이버의 `width_mm`·`height_mm`, simplefb DT의 물리 크기 |
| 주사율 | simple-framebuffer 단계는 LK가 정한 모드 하나다. mediatek-drm 단계에서 144/120/60/30Hz를 열고, 움직임이 없으면 낮추는 적응형 주사율(macOS ProMotion처럼)을 둔다. 주사율 전환은 지금 전체 모드 설정(패널 다시 켜기)이다. 깜빡임 없는 전환은 벤더의 `dyn_fps`처럼 포치만 바꾸는 길이 있어야 하고, 아직 없다 | `comp`의 출력 모드와 프레임 일정 |
| 터치 | 10점을 그대로 받는다(컴포지터에 손가락 수 제한이 없다). 손바닥 거부와 가장자리 띠(위 24px, 아래 20px)를 실기기에서 맞춘다 | `comp/src/touch.rs` |
| 펜 | NT36532 펜을 tablet-v2(압력·기울기·단추)로 앱에 보낸다. 컴포지터에 smithay tablet 틀이 있고, 펜 디지타이저는 입력 모드 계산에서 이미 뺀다 | `comp` tablet-v2, 시스템 설정 펜 페이지(나중) |
| 키보드 커버 | 포고 키보드가 붙으면 포인터 모드, 떼면 터치 모드다(지금 규칙 그대로). 붙음은 GPIO26 홀 센서(펌웨어 `hall_2`이자 `plug-gpios`, 낮으면 붙음)로 알고, 드라이버는 그때만 키보드·터치패드 입력 장치를 만든다. 덮개 홀 센서(GPIO1)는 닫힘을 알린다. 뒤로 접음을 재는 센서는 태블릿에 없다. 키보드 MCU가 일반 응답(0x93, sub 3)으로 켬·끔을 밀어 보내고, 스톡은 이것을 `KEY_KB_ENABLE`·`KEY_KB_DISABLE`로 올린다. 이것이 뒤로 접음으로 판단해(접을 때 이 프레임이 오는지는 기기에서 본다) 드라이버가 `SW_TABLET_MODE`로 보낸다(꺼짐이나 키보드 없음이 1). 그때는 키보드·터치패드를 세지 않는 규칙을 더한다(iPad Magic Keyboard처럼). 잠든 사이 잃은 상태는 키나 손가락이 눌리면 `SW_TABLET_MODE`를 0으로 되돌린다. 잠든 사이 접은 것은 키보드가 다시 알려야 안다(기기에서 본다) | `comp/src/input_mode.rs`, 포고 키보드 드라이버 |
| 회전 | 가속도·자이로(LSM6DSO)와 조도(TCS37013)가 있지만 SCP(nanohub) 뒤에 있다. AP 장치 트리에 노드가 없다(2026-11-01 실측). 그래서 자동 회전이 막힐 수 있다. 처음에는 시스템 설정의 수동 회전이고, SCP 길이 열리면 iio-sensor-proxy로 자동 회전한다. 패널의 자연 방향은 가로다 | 시스템 설정 디스플레이 |
| 전원 | A715 ×4 + A510 ×4의 EAS, cpufreq-hw, GPU 주파수, s2idle, 덮개(홀)로 잠자기, 45W PD 충전, LVTS 열 관리 | 커널, buntOS 전원 설정 |
| 오디오 | 스피커 넷(AW88394)과 AIN0 마이크가 ALSA UCM(`conf.d/TB375FC`)으로 PipeWire에 보인다. 음량은 PipeWire 소프트웨어 음량이고, 앰프 감쇠는 -18 dB로 고정한다. 볼륨 키는 PMIC 키다 | UCM, WirePlumber, `buntos-sound.service` |
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
    mkosi.conf                  mkosi 보드 프로필 (이미지용 커널 .deb, 파티션별 출력, 개발용 SSH)
    mkosi.postinst.chroot       GPT를 고치는 것 가리기, 파일 시스템 늘리기
    mkosi.extra/                진짜 루트에 넣는 것 (개발용 USB 길, 프리셋, UFS를 데스크톱에서 가리는 udev 규칙)
    boot/                       boot·vendor_boot·init_boot·dtbo 만들기 (헤더 v4, DT 표)
    initramfs/                  첫 부팅용 initramfs (busybox, 루트로 넘기기, USB 가젯, nvdata), QEMU 시험
    udev/                       UFS 가리기 규칙의 시험 (규칙 검사, QEMU에서 UFS·USB)
    firmware/                   기기에서 펌웨어와 보정값을 뽑는 스크립트 (파일은 두지 않는다)
  tools/hw/                     펌웨어 받기·풀기 도구, 화면 레지스터 읽기
```

- 빌드 산출물은 저장소 밖(빌드 VM의 `~/buntos-build/tb375fc/`)에 둔다.
- 원본 buntOS로 돌려줄 만한 것(펌웨어 도구, 데스크톱 쪽 고침)은 원본에 따로 낸다.

## 10. 위험과 대응

| 위험 | 영향 | 대응 |
|---|---|---|
| 언락 중 벽돌, nvdata 손실 | 개발 중단, Wi-Fi 보정값 잃음 | 예비 1대, go/no-go 넷. SP Flash Tool readback으로 전체 백업을 뜨고 해시로 확인한 뒤에만 언락한다. 복구도 같은 도구와 순정 펌웨어 묶음으로 한다(2.1절) |
| 내부 디스크가 파일 관리자에 보여 안드로이드 파티션을 붙인다 | nvdata·persist가 바뀌거나 슬롯이 깨짐, 복귀 길을 잃음 | UFS 호스트 아래의 모든 블록 장치에 `UDISKS_IGNORE=1`(규칙 `90-buntos-tb375fc-hide-ufs.rules`). 숨김일 뿐이라 `udisksctl`로 직접 붙이는 길은 polkit 인증만 받으면 열려 있다(11절) |
| 언락 길이 판에 묶임 | 받은 기기의 판에서 언락이 안 될 수 있다 | 받자마자 OTA를 끄고 판을 적는다. 그 판의 성공 보고를 확인한다. 안 되면 중국판 ZUI 16으로 내릴지 따로 정하고, 그래도 안 되면 buntOS ADR-011(기기 선택)을 다시 본다 |
| 개발용 LK의 출처를 모름 | 남이 고친 부트로더를 굽는다 | 굽기 전에 순정 LK와 견준다(문자열, 서명 영역). 언락한 뒤에는 바로 순정 LK로 되돌린다 |
| GenieZone의 EL2 점유 | KVM 불가 | **판정: 불가(2026-11-03, 2a에서 EL1로 시작).** D8대로 KVM을 포기하고 기기는 유지한다(사용자 결정 2026-11-04, buntOS ADR-011). 막히는 것은 태블릿 위의 VM이다. gzvm은 미병합이고 순정 트리에서도 `hypervisor` 노드가 꺼져 있어 나중 |
| 화면 파이프라인을 기기에서 처음 켤 때 버스가 멈춤 | 기기가 멈추고 키로 다시 켜야 한다 | 체크리스트 9.5절의 차례대로 켠다. `modprobe.blacklist=mediatek_drm`으로 시작하고 손으로 올린다. 켜지 못해도 simple-framebuffer 길은 그대로다 |
| PQ를 건너뛰는 경로가 이 기기에서 돈 적이 없다(오른쪽은 PANEL_COMP_OUT 크로스바 1을 왼쪽에서 유추) | 한쪽 절반이 검거나 프레임이 멈춘다. 레지스터 접근은 그대로라 버스 멈춤은 아닐 것이다 | LK와 스톡은 모두 PQ와 크로스바 0을 쓴다. 첫 프레임에서 절반씩 본다. 안 되면 PQ 길로 간다 |
| IOMMU 없이 물리 주소로 스캔아웃한다 | 라브의 MMU 비트가 1이면 스캔아웃이 틀린 주소를 읽는다. 또 GPU 버퍼를 바로 스캔아웃하지 못한다 | 0단계에서 라브 비트를 읽는다(LK는 라브0·1을 지운다). 컴포지터는 KMS 쪽에서 버퍼를 받는다. IOMMU 드라이버는 나중 일이다 |
| LK가 돌리는 블록의 PLL을 커널 장치와 함께 쓴다 | 그 장치가 클럭을 내리면 PLL이 꺼져 블록이 죽는다(실측: UFS와 화면의 `mmpll`) | `simple-framebuffer`의 `clocks`로 잡는다. 새 장치마다 `clk_summary`로 부모를 본다(체크리스트 6.2절) |
| DVFS와 버스 QoS가 없다 | 144Hz에서 언더런이 날 수 있다 | 먹스는 LK 값 그대로다(`clk_ignore_unused`). 언더런이 나면 60Hz로 내리고, MMQoS와 HRT 일은 2e 뒤로 둔다 |
| 센서 허브(SCP nanohub) | 자동 회전·조도 없음 | 수동 회전, SCP 길은 3단계에서 조사 |
| MFD `mtk-spmi-pmic` 미병합 | PMIC가 늦어진다 | 패치 대기열에 넣고 업스트림 판을 따라간다. v12(2026-05) 뒤로 새 판이 없고, BayLibre가 이어받겠다고 했다(2026-10). v12의 인터럽트 결함(마스크가 늘 첫 그룹에 쓰인다)은 대기열에서 고쳤다. 업스트림에 알릴지는 정할 것이다. 병합된 MT6363 regulator(7.2.12)의 16비트 regmap, VEMC 전압, VSRAM 레지스터 결함은 대기열에서 고쳤고(0037·0038·0041) 업스트림에 낸다. 코드와 레지스터 모델로 확인했을 뿐 기기에서 돌려 보지 않았으니, 기기에서 전압을 읽어 보기 전에는 전압 한계를 주지 않는다 |
| Mesa 26.3 의존 | GPU 가속이 늦어진다 | 26.1은 G615를 모르고 26.3.0-rc3은 기기에서 맞게 그린다(2026-11-04). Debian forky는 26.1.6, sid는 26.2.6이다. 이미지의 Mesa 판 확인, 그 전에는 CPU 렌더링(buntOS는 이미 llvmpipe에서 돈다) |
| CPU 유휴 상태와 절전 | 대기 전력, 서스펜드 깊이 | cpu-off, cluster-off는 MCUPM에 시간 기준을 주어 켰다(0193~0195, 2026-11-07). 그 때문에 드러난 화면 깨짐은 0197로 고쳤다. mcusysoff와 system 상태는 `irq-remain`, SPM 조건, 자원 제약 SMC가 더 필요하다. 잠자기 전력은 아직 재지 않았다 |
| 잠자기 우회 | `univpll`, 화면 전원 도메인, BGF MCU가 잠든 동안 켜져 있다. 대기 전력이 크다(재지 않았다) | HW voter가 PLL과 OVL 도메인을 놓지 못하는 까닭, MCU를 펌웨어에 넘긴 뒤 찾아오는 길(벤더는 SMC와 `bgfsys_clr_host_csr`를 거친다)을 찾으면 뺀다 |
| Wi-Fi 펌웨어가 uni cmd를 받지 않음 | 2d 지연 | **판정: 받지 않는다(2026-11-04).** 레거시 명령 길(패치 0061~0065)로 2.4 GHz와 5 GHz 연결이 인터넷까지 된다 |
| Wi-Fi 칩 리셋이 실패함 | 답을 기다리는 명령이 시간 초과되면 모듈을 다시 올릴 때까지 Wi-Fi가 죽는다. 패닉은 없다 | **판정: 고쳤다(2026-11-04, 패치 0066).** 첫 시도에 돌아오고 리셋 뒤 스캔이 된다. 연결 중 리셋 뒤에는 약 9초에 다시 붙는다(패치 0067). 부팅 블랙리스트(`modprobe.blacklist=mt7921soc`)를 풀려면 자동 연결 설정과 `regulatory.db` 거부가 남았다 |
| Wi-Fi 데이터 경로가 조용히 멈춤 | 연결된 것으로 보이는데 통신이 안 되고, 상단 아이콘도 그대로다 | **판정: 알아채고 돌아오게 했다(2026-11-05, 패치 0068·0069). 원인은 모른다.** 벽 충전기에 꽂힌 145분 동안 열 번 났고 여덟 번은 펌프가 꺼져 있었다. 펌프 잡음은 크게 약해졌다. 감시가 열 번 모두 잡았고 3.8~14.9초 뒤 다시 붙었다. 상단 아이콘과 AP가 꺼질 때는 아직 보지 않았다 |
| Wi-Fi 펌웨어가 벤더 표에 없는 명령(EXT 0x3e, 0x46)에 답하지 않음 | wlan0이 생기지 않거나 올라오지 않는다. 버스는 멈추지 않는다 | **판정: 막히지 않는다(2026-11-04).** EXT 0x46·0x4e·0x08은 상태 0, 0x3e·0x05는 상태 0xfe로 답한다 |
| LK가 mainline 트리의 connsys 노드에 `emi-addr`·`memory-region`을 써 주지 않음 | Wi-Fi·BT가 펌웨어를 찾지 못한다 | LK(`platform_load_connsys`)는 compatible이 아니라 경로 `/soc/consys`→`/consys`, `/soc/gps`→`/gps`, `/soc/wifi`→`/wifi`, `/soc/connscp`→`/connscp`로 찾는다(유닛 주소 무관, status 안 봄). 하나라도 없거나 크기가 0이면 -2로 돌아오고 아무것도 싣지 않는다. 복구(b_mode 2)와 전원 끈 충전(8)에서는 부르지 않는다. 그래서 mainline 트리가 이 이름과 크기를 그대로 두면 값을 써 준다고 본다(역어셈블). 실제 기기에서 확인한다. 2d 전까지는 `gps`·`connscp`를 트리에서 빼 두어 LK가 싣지 않게 한다 |
| Bluetooth 첫 보정이 Wi-Fi에 기댐 | 벤더 주석: pre-cal 없이 BT를 먼저 켜면 보정이 실패하기 쉽다 | 2d에서 Wi-Fi 없이와 함께 켠 경우를 견준다. 차이가 크면 Wi-Fi 뒤에 켜거나 보정값 복원을 옮긴다 |
| BT 대기 전력 | 열려 있는 동안 MCU를 깨워 둔다(driver own) | 수면 길은 2e에 넣는다 |
| connsys 전체 리셋 없음 | 펌웨어가 요청해도 BT만 다시 켠다. Wi-Fi까지 묶인 멈춤이면 재부팅이 필요할 수 있다 | 기기에서 일어나면 connsys 드라이버에 맡길 일이다 |
| conninfra가 벤더 TF-A SIP 호출에 기댐 | 업스트림 제출이 어렵다 | 업스트림 UFS의 SIP 호출이 선례다. 일반 세계에서 레지스터를 직접 쓸 수 있는지도 본다 |
| 펌웨어 재배포 불가 | 배포 꼴 | 설치할 때 기기에서 뽑기(D6) |
| 소스 출처 | 법적 불확실 | 이 기기 모듈 미러는 제3자다. 참고만 하고 코드는 업스트림 꼴로 다시 쓰며, GPL 출처(OEM 공식 저장소)를 우선한다 |
| 커뮤니티 포트와 겹침 | 중복 작업 | 이슈 트래커로 연락할 수 있다. 나눌 일은 `pinctrl-mt6897`(그쪽 터치 IRQ를 푼다), 터치, 살아 있는 트리·LK 기록이다. 그쪽은 2026-09-23 뒤로 멈췄다 |
| 생산 시기별 부품 차이 | 일부 기기만 다름 | Wi-Fi FEM은 가장 새 펌웨어의 표, 패널은 GPIO184 스트랩으로 고른다(스톡 터치와 같다). LK는 GPIO184·183 삼상이 0이면 BOE, 아니면 DCS 0xDA가 0이면 Tianma, 5면 Tianma 무광으로 고른다(LK 역어셈블). 두 Tianma는 초기화가 같다 |
| 루트 파티션을 위해 GPT를 고침 | preloader·LK가 읽지 못하면 부팅이 막힌다. userdata를 지운다 | 고치기 전의 GPT를 떠 둔다. 되돌리는 길은 언락 전 SP Flash 백업이다. 언락 직후라 잃을 데이터가 적을 때 한다(4절) |
| 이미지의 systemd가 UFS의 GPT를 고침 | 안드로이드 파티션과 슬롯을 잃을 수 있다 | `systemd-repart`와 gpt-auto 생성기를 보드 프로필과 initramfs에서 가린다. 파일 시스템만 늘린다(4절). QEMU에서 GPT와 다른 파티션의 해시로 확인했다. 대조 시험에서는 가리지 않으면 파티션을 늘린다 |
| USB 호스트의 VBUS가 MT6375와 차지 펌프에 달림 | USB 저장장치·동글·키보드·오디오 우회가 드라이버와 기기 확인을 기다린다 | 패치 0042~0049(꺼 둠). 그 전에는 USB 가젯으로 일한다 |
| VBUS를 잘못 켜 USB 경로가 끊김 | 가젯이 유일한 디버그 길이라 기기를 구할 길이 줄어든다 | 프로브와 등록에서 쓰기 0번(실제 regulator 코어 위에서 시험), regmap 쓰기 허용 목록, 외부 VBUS가 있으면 켜지 않음, `vbus-supply` 안 씀, 꺼 둔 채 대기. 처음 켤 때의 차례는 [체크리스트](bringup-checklist.md) 8절 |
| 켠 채 충전기나 PC를 꽂음 | TCPC가 없어 두 전원이 VBUS에서 맞선다. 가드는 켜는 순간만 본다 | TCPC 전까지 사용자 공간에서 손으로만 켠다. 시험 때 꽂지 않는다 |
| USB 커넥터 과열 보호 | 접점이 뜨거워져도 배터리 쪽은 모른다 | 넣었다(패치 0098·0099, 0150). 75도에서 GPIO179, 충전 끔, 입력 100 mA. 값을 강제로 넣어 시험했고 실제로 뜨거워진 적은 없다. OTG 중에는 순정도 돌리지 않는다 |
| 충전의 남은 시험 | 무인 충전을 아직 다 믿을 수 없다 | 온도 단계, 과전압, 가드, 차단(새 충전기로 2번 깨끗), 만충을 봤다(2026-11-05). 10시간 타이머, 펌프가 도는 중의 하드 리셋과 뽑기, 드라이버로 4.2 A는 남았다. 실제 과열과 칩 자체 OVP는 일부러 만들지 않았다. 패치 0170~0177은 모듈로만 돌렸고 그 커널로 부팅한 적은 없다 |
| 만충이 89%로 보임 | 충전이 끝나도 숫자가 100%에 닿지 않고 UPower가 `fully-charged`가 되지 않는다 | **표시를 맞췄다(사용자 결정 2026-11-05, 패치 0178~0181, 커널 #303에서 봤다).** 칩은 4.335 V에서 제대로 끝낸다. 게이지의 `charge_full`이 CV 4.34 V보다 높은 전압 기준으로 보인다(순정 `battery-cv` 4.5 V, 추정). CV를 올리지 않았으니 넣는 양은 그대로이고, 100%는 4.34 V까지 찬 것을 뜻한다. 게이지가 끝에서 읽는 값이 달라지면 장치 트리의 88을 고친다 |
| 패널 리셋이 터치 펌웨어를 지움 | 터치가 패널보다 먼저 열리면 터치가 죽고, 키보드가 없으면 로그인할 수 없다 | **고쳤다(패치 0182·0183, 커널 #304에서 봤다).** 드라이버가 패널을 따라가 패널이 켜진 뒤 펌웨어를 올리고 꺼지기 전에 멈춘다. 화면을 끄고 켠 뒤와 서스펜드 뒤는 아직 보지 않았다 |
| `apt upgrade`가 우리 것을 망가뜨림 | 우리 바이너리는 dpkg가 몰라 쓰는 라이브러리가 지워질 수 있고, 다음 로그인에 화면이 뜨지 않는다 | **판정: 막았다(2026-11-05, `apt-guard`).** 일이 나면 알리고 스냅숏으로 되돌린다. 올린 뒤의 재부팅은 봤고(#303, 실패한 서비스 없음), 되돌린 뒤의 부팅은 아직 보지 않았다. 데스크톱 라인과 같은 도구를 쓴다(buntOS `image/`) |
| 드라이버 없는 동안 배터리가 줄어듦 | 2.2 W 입력보다 부하가 크면 꽂아 두어도 방전되고, 게이지가 없으면 경고 없이 UVLO에서 꺼진다 | 시험 전 Android에서 충전. Android로 돌아갈 길을 남김. 잔량은 [체크리스트](bringup-checklist.md) 5.1절의 차례로 본다 |
| MT6375 쓰기가 배터리를 해침 | 잘못된 CV·전류나 BATFET_DIS | MFD의 값 검사(4.34 V, 2 A, 2 A, BATFET_DIS 거절)와 드라이버의 상한, 배터리 설명 검증. 실제 커널 위에서 시험 |
| 잠든 동안 충전되지 않음 | 절전에 들면 충전을 끈다. 온도를 볼 작업이 돌지 않기 때문이다. 순정은 10초마다 깨어 온도를 본다. 전원 키로 잠들 수 있게 되어 실제로 겪는다(2026-11-05) | 아직 정하지 않았다. 깨우는 타이머가 없어(RTC 깨우기 없음) 순정처럼 하려면 먼저 그것이 필요하다 |
| TCPC가 VBUS를 끊음 | 가젯이 끊긴다. 시험한 적 없는 칩이다 | `self-powered` 없음, 싱크 먼저, PD 끔, VBUS 명령 안 보냄, 시간 제한 스크립트로 시험 |
| 넘겨받기 전 게이지가 답하지 않음 | LK 값(CV 4.35 V로 추정)으로 감시 없이 충전이 계속된다. 온도를 볼 수 없다 | 로그를 한 번 남긴다. 체크리스트 B의 조건(사람이 곁에, 멈추는 길 먼저). 말이 안 되는 값이면 멈춘다 |
| LVTS 보정을 잘못 읽으면 온도가 낮게 읽힌다 | 과열을 모른다 | 계수와 golden을 같은 조건으로 고르고, 말이 안 되는 보정과 측정하지 않는 컨트롤러는 프로브를 거절한다. 읽기 실패는 조용히 넘기지 않는다. 기기에서 안드로이드 값과 맞대 본 뒤 켠다 |
| 펌웨어가 하드웨어 리셋을 켜 두었는지 모른다 | 소프트웨어 critical(115 °C)이 유일한 보호일 수 있고, 구역이 없으면 보호가 없다 | 벤더 표의 119 °C는 벤더 드라이버가 sysfs로 trip을 쓸 때만 하드웨어에 간다. LK에는 LVTS 주소가 없다. 프로브가 `PROTCTL`·`PROTTC`·`MONINT`를 기록하니 기기에서 본다. 꺼져 있으면 켜는 패치를 따로 정한다 |
| GPU 첫 전원에서 SoC가 멈춘다(커뮤니티 보고) | 다시 부팅 | 켜는 차례를 넷으로 나눈다: 열, GPUEB(GPU 전원 없음), GPU. 무엇을 보내기 전에 읽기만으로 확인한다 |
| 안드로이드가 남긴 MT6685 알람 | 전원 꺼짐 알람이 켜진 채 리눅스로 넘어오면 그 시각에 SID 9의 RCS가 날 수 있다. 0114가 그 이벤트를 지운다. 칩이 곧바로 다시 내면 EINT 248이 계속 울릴 수 있다(칩이 어느 쪽인지는 기기에서만 안다). 그래도 처리기는 SID 4·5를 처리하니 전원 키를 잃지는 않지만 그 CPU가 묶인다 | 체크리스트 6.10의 11번을 볼 때까지는 넘어가기 전에 알람을 끈다 |
| RTC 읽기의 가정 | 게이트를 잠깐 여는 것으로 레지스터가 따라온다고 본다(2025년 뒤 MediaTek 판). 이 기기의 스톡 모듈은 옛 판이라 BBPU RELOAD를 쓴다 | 안 흐르면 체크리스트의 RELOAD 판으로 바꾼다. 해는 없다: 시간이 멈춰 보일 뿐이다 |
| 리눅스 끄기 | 벤더는 종료마다 BBPU PWREN을 내리고 `TOP2_ELR1` bit 0을 올린다. 드라이버는 하지 않는다 | `poweroff`에서 본다. 필요하면 `.shutdown`에 넣는다 |
| 포고 키보드 프로토콜을 스톡 모듈에서 읽음 | 다른 판 키보드에서 다르면 키가 안 든다 | 글로벌판(TB373FU) 모듈 기준이다. 중국판 모듈과 실제 프레임은 기기에서 `dyndbg`로 본다. 코드는 사실만 옮겨 새로 썼다(표 8×16은 통신 상수다) |
| 포고 키보드 TX 게이트와 MCU 응답 지연 | 응답이 약 100~150 µs보다 빠르면(모델 값) 앞부분이 host TX와 부딪혀 다시 보낸다 | 스톡은 APDMA 완료 + 300 µs에 8250 콜백에서 닫는다. 이 드라이버는 PIO라 에코로 닫는다. 기기에서 `dyndbg`의 "gate shut after … reply after …"로 잰다. 모자라면 RX 콜백에서 닫고, 그래도 모자라면 8250 쪽 콜백을 정한다 |

## 11. 일정과 다음 작업

1. **끝낸 것(기기 없이):**
   - 2a 장치 트리(업스트림 꼴), 커널 커스텀(`7.2.12-tb375fc`), `boot/` 이미지 도구와 첫 부팅 initramfs
   - 커뮤니티 포트의 커밋 13개와 겹침 정리([분석](community-port.md))
   - 오픈소스 쪽 조사(v1.2)와 제품 라인 정리, 언락 길 확인(v1.3)
   - v1.5에 끝낸 것:
     - `pinctrl-mt6897`(EINT 포함, 패치 0004~0006)과 pio 노드. 표는 OnePlus 공식 MT6897 커널에서 옮겼다
     - SPMI: 고침 다섯(0002·0003 백포트, 0007·0008·0114), M 버스 노드(rcs는 EINT 248, 별칭 `spmi0`, P 버스는 끔), MT6319 셋(DT만). 소비자가 아직 없어 벅은 모두 always-on이다. pio와 SPMI는 2b까지 꺼 둔다
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
   - v1.12에 끝낸 것: MT6363 regulator 고침(패치 0037~0041, 업스트림에 낼 것).
     - 0037: regmap 값을 8비트로 했다. 16비트이면 SPMI 쓰기가 2바이트라 모든 쓰기가 다음 레지스터도 썼다. 벅을 끄면 VS1·VS3·VSRAM_DIGRF·MDFE·MODEM이 함께 꺼지고, 그 다섯을 끄면 벅 LP 비트가 지워졌다. 키 쓰기는 스택을 넘어 읽었다
     - 0038: VEMC 전압. 범위 찾기 조건이 반대라 VEMC 전압을 늘 틀리게 읽었고, TOP_TRAP=1일 때 VOSEL_1을 쓰지 않았다. TOP_TRAP=1의 VOCAL 위 네 비트는 근거가 없어 그대로 두었다
     - 0039·0040: Mark Brown이 MT6373 v12에서 짚은 OC 처리 둘(켜져 있을 때만 알림, 해제 때 work를 멈추지 않음). MT6368(0024)의 work 초기화도 같은 자리인 프로브로 옮겼다
     - 0041: VSRAM_CPUB·CPUM·CPUL·APU의 켜기·LP 레지스터가 VS1 켜기와 VS3 LP를 가리켰다. 이 넷을 끄면 VS1이 꺼졌다. 보드에서는 always-on 레일 셋이 이제 자기 비트를 읽고, 비트가 0이면 코어가 켠다
     - 기기 없이 벤더 표와 맞대고(헤더 379항목, 레일 표 746항목), 실제 드라이버 파일을 레지스터 모델 위에서 패치마다 돌렸다. 모델도 벤더 자료로 만든 것이라 실제 PMIC로 확인한 것은 아니다. 보드 방침을 바꿀지는 기기에서 `TOP_TRAP`과 VEMC 전압을 본 뒤 정한다
   - v1.13에 끝낸 것: 2c(1차) 입력의 포고 키보드 커버(패치 0032~0036과 장치 트리). 업스트림 v6에 리뷰 지적을 반영하고, 스톡 모듈에서 읽은 옛 꼴을 프로토콜 표로 더했다. CRC 초기값은 필요 없다(이 기기는 합을 쓰고, 0xA5C9는 OnePlus `dunhuang`의 값이다). GPIO26은 키보드 드라이버가 가진다. UART3 패드 모드는 OEM `uarthub` 코드의 뜻(bit 4: 1 패드, 0 허브)대로 8250_mtk가 probe·startup·resume에서 맞춘다
     - 보드 `&ssusb`에 `dr_mode = "peripheral"`을 넣었다. 문서에는 있다고 적혀 있었으나 파일에 없었고, 없으면 mtu3가 호스트 역할로 시작해 2b의 USB 가젯 셸이 뜨지 않는다
   - v1.14에 끝낸 것: 루트로 넘기는 initramfs(4절). 넘어가지 못하는 경우 열아홉 가지가 패닉 없이 셸로 돌아오고, 진짜 systemd에서 `/run/buntos` 기록, 디스크 안전, 파일 시스템 늘리기, 가짜 UDC(`dummy_hcd`) 위의 ttyACM 셸·DHCP·SSH를 봤다. 시험은 `initramfs/test.sh`다. 기기에서 볼 것은 [체크리스트](bringup-checklist.md) 6.8절에 있다
   - v1.15에 끝낸 것: USB 호스트의 VBUS(패치 0042~0049). I2C 바인딩·게이트·컨트롤러 14개, MT6375 MFD와 OTG 부스트, 차지 펌프 입력 FET 스위치, 보드 노드(모두 꺼 둠). 레지스터와 순서는 OEM·스톡 모듈과 맞댔고, 실제 커널의 i2c·regmap·regulator 코어 위에서 레지스터 모델로 돌렸다. 기기 시험은 없다
   - v1.18에 끝낸 것: 전원 도메인(패치 0050~0054). 컴파일한 표 1114개 값이 벤더와 맞고, LK가 켠 상태에서 probe의 쓰기는 0번이다(실제 커널을 QEMU의 가짜 레지스터 위에서 돌렸다). 기기 시험은 없다. 2d에서는 `&spm`만 켜고 IFR·VLP syscon은 켜지 않는다
   - v1.19에 끝낸 것: NT36532 패널 드라이버(패치 0080~0084). 표 935항목이 미러·스톡 모듈·LK 표와 맞다. 화면 파이프라인이 없어 지금은 돌 곳이 없고, 기기 시험은 없다. 패널은 노드 하나에 GPIO184로 고른다. LK가 찾는 형제 노드(`panel1@0`~`panel3@2`)는 두지 않았고, 역어셈블로는 못 찾아도 기록만 남기므로 2a에서 LK 기록으로 확인한다
   - v1.20에 끝낸 것: connsys 전원과 mt76 SoC 변형(패치 0055~0060), Bluetooth 전송(패치 0085·0086). 노드를 켜기 전에는 돌지 않고 기기 시험은 없다. Wi-Fi 벤더 대조의 어긋남 넷은 벤더 표에 없는 MT7921 명령이고 코드 오류가 아니다(`MAC_INIT_CTRL`은 `ip link up`에서 처음 본다). Bluetooth는 벤더 코드와 같은 모델에서 레지스터 쓰기와 링 메모리의 어긋남이 0이다. 켜는 차례는 [체크리스트](bringup-checklist.md) 9.1절과 9.2절이다
   - v1.21에 끝낸 것: MT6375 충전과 TCPC(패치 0090~0097). 켜는 단계는 읽기, 충전, TCPC 싱크, DRP·호스트, PD 다섯이고 앞에서 멈출 수 있다([체크리스트](bringup-checklist.md) 9.3절). 충전 코드는 언락 전에 독립 검토를 한 번 더 받는다(고친 뒤 코드는 다시 검토받지 않았다). 사이클 표를 따라 상한을 올릴지, 절전 중 충전을 어떻게 할지는 기기를 본 뒤 정한다
   - v1.22~v1.25에 끝낸 것: 충전과 Wi-Fi의 독립 재검토 반영(같은 번호), GPU 전원·열·게이지(0070~0079), 화면·MDP·코덱 클럭 게이트(0100~0104), MT6685 RTC(0110~0113). 모두 꺼 둔 노드이고 기기 시험은 없다. 켜는 차례는 [체크리스트](bringup-checklist.md) 6.10절(RTC), 9.3절(충전), 9.4절(GPU·열·게이지)이다. GPU 쪽 커널에 남은 것: `clk_ignore_unused`를 뺄 때 `mfgsc_ref`에도 `CLK_IGNORE_UNUSED`(0013은 `mfg_ref`에만 있다), GPU 쿨링 지도와 `dynamic-power-coefficient`, I/O 코히어런시. 게이지의 충전 전압(`0x30`)은 쓰지 않고 기기에서 읽은 뒤 정한다
   - v1.26에 끝낸 것: UFS의 안드로이드 파티션 가리기. 전체 이미지에는 `udisks2`가 들어온다(`nautilus` → `gvfs`). 내부 디스크는 자동으로 붙지 않지만 파일 관리자에 보이고, 관리자 인증 뒤 쓰기 가능으로 붙을 수 있다. 규칙(`90-buntos-tb375fc-hide-ufs.rules`)이 UFS 호스트의 모든 블록 장치에 `UDISKS_IGNORE=1`(숨김), `UDISKS_SYSTEM=1`, `UDISKS_AUTO=0`(UFS의 LU가 이동식으로 보여도 인증을 받고 자동으로 붙지 않는다)을 준다. USB 저장장치와 SD 카드는 부모가 달라 걸리지 않는다. buntOS 루트도 같은 디스크라 함께 걸리는데, `/`에 붙은 루트는 gvfs가 어차피 내놓지 않아 잃는 것이 없다. **숨김일 뿐 막지는 않는다**: udisks2는 `UDISKS_IGNORE`를 `Block.HintIgnore` 속성으로 알릴 뿐 스스로 쓰지 않으므로(`udiskslinuxblock.c`), `udisksctl mount`나 D-Bus의 `Mount()`는 polkit 인증(`filesystem-mount-system`, 기본 `auth_admin_keep`)만 받으면 붙는다. 인증 뒤 붙이기까지 막는 polkit 규칙은 지금 넣지 않았다. 실수로 붙이는 길은 숨김으로 닫혔고, 일부러 붙이는 것까지 막을지는 뒤에 정한다(UFS는 늘 `-system` 동작을 고르므로 `org.freedesktop.udisks2.*-system`을 거부하면 되고, QEMU에서 UFS는 거부되고 USB 메모리는 붙었다. root는 polkit이 늘 허용해 못 막는다). `computer:///`에는 볼륨 없는 드라이브가 남는다(Nautilus 사이드바에는 이동식이 아닌 빈 드라이브가 나오지 않는다). 규칙이 기대는 이름(`ufshcd-mtk`, `112b0000.ufshci`)은 QEMU로 닿지 않으므로 기기에서 체크리스트 7.1절 1번으로 보기 전에는 끝난 것이 아니다
   - v1.27·v1.28에 끝낸 것: 충전과 Wi-Fi의 마지막 재검토 반영(패치 0060, 0093, 0097), PMIF 고침(0114). 기기 시험은 없다. 독립 재검토는 여기까지 하고, 다음 재검토는 기기 시험 뒤로 미룬다
   - v1.29에 끝낸 것: 화면 파이프라인(패치 0120~0139). 노드를 켜기 전에는 돌지 않고 기기 시험은 없다. 켜는 차례는 [체크리스트](bringup-checklist.md) 9.5절이다
2. **다음(기기 없이, 이 차례로):**
   1. 그다음: 업스트림 제출 준비(MT6363의 확실한 패치 둘은 2026-11-02에 regulator 유지자와 메일링 리스트로 보냈다). SM5109 전압을 켤 때마다 쓰기(기기에서 값을 잃는 것을 봤다, 체크리스트 9.5)
   2. 출시 전에: 개발용 USB 길을 이미지에서 빼는 프로필, initramfs의 암호 없는 셸을 없애거나 막는 길, 공통 레시피의 `Autologin=yes`
   3. 기기에서 UFS·USB가 뜬 뒤: UFS 클럭 스케일링(먹스 부모, 게이트의 `CLK_SET_RATE_PARENT`), MCQ(공통 바인딩의 인터럽트 수, 큐 인터럽트 SPI 177~184), xsphy MT6897 꼴(U2 INTR_CAL·efuse, U3 RX CDR), mtu3 깨우기 접점(벤더 판 103)
      - USB 호스트: 사용자 기기의 차지 펌프는 UPM6720 @0x65다(2026-11-01 adb, `/sys/bus/i2c/drivers/upm6720/10-0065`. NU2115에는 붙은 장치가 없다). 부품이 다른 기기가 있을 수 있어 `i2cdetect -r`로 다시 본 뒤 그 노드를 켠다. 시험 차례는 [체크리스트](bringup-checklist.md) 8절
3. **기기:** 차례와 명령은 [bring-up 체크리스트](bringup-checklist.md)에 모았다. 사용자의 TB375FC는 중국판 ZUXOS 1.5.10.044(Android 16)다(2026-11-01). 이 판에서 바로 언락한 보고는 없으니, 16.0.20.091로 내린 뒤 언락하는 길을 쓴다(2.1절). 예비 기기를 살 때는 ZUI 16 기기가 낫다.
4. **언락 전(지금 할 수 있는 것):**
   - 자동 업데이트를 끄고 1.5.10.044에 머문다(go/no-go 4번). 1.5.10.103 이후 판에서는 판 바꾸기 문제가 많이 보고됐다. 2026-11-01에 레노버 업데이트 앱(`com.lenovo.ota`)을 껐다(되돌리기는 `pm enable`)
   - 펌웨어: adb 셸은 `/vendor/firmware`의 이름만 보고 파일은 읽지 못한다(2026-11-01). 파일은 아래 readback 백업이나 언락 뒤 root로 얻는다
   - SP Flash Tool readback으로 전체 백업과 해시(go/no-go 1번). 인증 DA·auth는 그 판의 레노버 펌웨어 묶음에서 쓴다
   - ADB로 잰 것(2026-11-01, [실측 보고서](../hw/lenovo-tb375fc.md) 7절): 판, 잠금 상태, RAM 8GB와 저장장치 128GB, 화면(패널 버퍼 가로 2944×1840), 입력 장치 이름, vendor 펌웨어와 모듈 목록, 지문 센서, 센서(LSM6DSO, TCS37013, SCP 뒤)
   - 언락 뒤 root나 2b의 가젯 셸에서 파티션 표를 읽고 새 루트 파티션의 크기와 이름을 정한다(4절): GPT의 userdata 자리, LK fastboot가 새 항목에 굽는지
   - root가 필요한 것(언락 뒤): `getevent -lp`, framebuffer 주소와 살아 있는 트리의 값(`/proc/device-tree`는 이름만 보이고 값은 막혀 있다), `/proc/iomem`, 파티션 표
   - UART 위치는 기기를 열어 찾는다
5. **언락:** 2.1절의 언락 순서대로 한다. 그 뒤 root로 framebuffer 주소와 살아 있는 트리를 읽어 장치 트리의 framebuffer·ramoops를 켜고, 2a를 한다(비운 슬롯 `_b`에 굽는다).
6. **2b → 2c(1차) → 2d → 2e → 2c(2차) → 3:** 각 단계의 종료 조건을 이 포크의 문서에 남긴다.

## 12. 출처

[포팅 재료 조사](porting-materials.md) 6절, [Wi-Fi 문서](wifi-soc7_0.md), [커뮤니티 포트 분석](community-port.md)의 출처를 따른다.
