# TB375FC bring-up 체크리스트

- 작성일: 2026-11-01
- 기준: [설계안](DESIGN.md) v1.16, 패치 대기열 0001~0049
- 상태: 2절, 3절, 4절의 읽기를 기기에서 했다. 리눅스는 아직 기기에서 돌리지 않았다. 기기 결과로 고친다
- 관련 문서: [장치 트리 README](../../kernel/tb375fc/dts/README.md), [BSP README](../../bsp/lenovo-tb375fc/README.md), [하드웨어 실측](../hw/lenovo-tb375fc.md), [포팅 재료 조사](porting-materials.md), [buntOS 설계안](../design/ARCHITECTURE.md) 4.2.3절·12.1절

## 1. 쓰는 법

기기에서 할 일을 일어나는 차례로 모았다. 위에서 아래로 한다.

- **리눅스는 아직 기기에서 돌리지 않았다.** 사용자의 TB375FC는 2026-11-03에 언락했다. 기기에서 한 일은 adb 읽기, 업데이트 앱 끄기, 잠긴 채로 fastboot 읽기와 재부팅(2026-11-01·02), SP Flash Tool 읽기와 판 내리기, 슬롯 변경, 초기화, 개발용 LK로 언락하고 순정 LK로 되돌리기(2026-11-03)뿐이다. 아래 기대값은 펌웨어, OEM 코드, 스톡 모듈, QEMU, 레지스터 모델에서 왔다.
- 단계마다 목표와 종료 조건으로 시작한다. 종료 조건을 채운 뒤 다음 단계로 간다. 결과는 이 포크의 문서에 남긴다(설계안 11절). 일련번호와 MAC 주소는 적지 않는다.
- 항목의 차례를 지킨다. 읽기만 하는 확인이 켜는 일보다 먼저다.
- 장치 트리의 노드는 대부분 꺼져 있다. 그 단계에서 말한 노드만 `status = "okay"`로 바꾸고 이미지를 다시 만든다. 노드의 값과 출처는 [장치 트리 README](../../kernel/tb375fc/dts/README.md)에, 이미지 만들기는 [BSP README](../../bsp/lenovo-tb375fc/README.md)의 "만들기"에 있다.
- 시험 도구: 진짜 루트에는 보드 프로필의 `evtest`, `i2c-tools`, `gpiod`가 있다. 물리 주소는 `busybox devmem <주소> 32`로 읽는다. 이미지용 커널은 `CONFIG_IO_STRICT_DEVMEM=y`라 드라이버가 잡은 곳을 막으므로, 그런 주소를 읽을 때는 명령줄에 `iomem=relaxed`를 더한다. 개발용 커널은 이 값이 꺼져 있다.
- 기본 커널 명령줄은 `console=ttyS0,921600n8 console=tty0 earlycon clk_ignore_unused pd_ignore_unused regulator_ignore_unused`다. 뒤의 셋은 소비자가 아직 없는 클럭·전원 도메인·레귤레이터를 늦은 정리에서 끄지 않게 한다. `pd_ignore_unused`는 늦은 정리만 막고 sync_state의 끄기는 막지 못한다(7.2.12). LK가 켠 도메인이 꺼지지 않는 것은 패치 0054의 표가 모두 기본 꺼짐이고 장치 트리에 conn만 있기 때문이다. 이 목록의 단계에서는 빼지 않는다(`clk_ignore_unused`를 빼는 일은 2e다).

**어느 컴퓨터로 하나.** SP Flash Tool이 Windows 도구라서 아래 첫 줄만 Windows가 있어야 한다. 빌드 VM(Lima)은 USB를 받지 못하므로 기기에 닿는 명령은 맥에서 친다.

| 일 | 컴퓨터 |
|---|---|
| 전체 백업(2.3절), 판 내리기(3절 1번), 백업 되돌리기(3절 "되돌리기") | Windows |
| `adb`, `fastboot`(언락, LK 굽기, `fastboot boot`, 슬롯에 굽기), USB 가젯 셸 | 맥 |
| 커널과 이미지 만들기 | 맥의 빌드 VM |

맥의 `fastboot`는 이 기기에 닿는다(실측, 2026-11-02. `adb reboot bootloader`, `fastboot devices`, `fastboot getvar`, `fastboot reboot`를 잠긴 채로 돌렸다). 벽돌이 되면 되돌리는 데 Windows가 다시 필요하다.

위험 표시는 `위험:`으로 찾을 수 있다. 표시 안에 먼저 둘 것과 되돌리는 길을 적었다.

| 표시 | 뜻 |
|---|---|
| `위험: 부팅 불가` | 기기가 부팅하지 못하게 될 수 있다 |
| `위험: USB 끊김` | USB 디버그 길이 끊긴다. 포트가 하나라 화면 말고는 유일한 길이다 |
| `위험: 데이터 손실` | 되살릴 수 없는 데이터나 userdata를 잃을 수 있다 |

근거를 나타내는 말은 소스의 말을 그대로 옮겼다.

| 말 | 뜻 |
|---|---|
| 실측 | 사용자 기기에서 adb와 fastboot로 읽었다(2026-11-01·02) |
| 펌웨어 | 순정 펌웨어에서 읽었다 |
| 코드로 본 것 | 소스나 역어셈블을 읽었고 기기에서 돌려 보지 않았다 |
| 모델 값 | 벤더 자료로 만든 모델이나 시뮬레이터의 값이다. 실제 칩이 아니다 |
| 포럼 보고 | XDA 글타래 등의 보고다. 우리가 기기에서 확인하지 않았다 |
| 판단, 추론, 짐작 | 근거에서 미룬 해석이다. 틀릴 수 있다 |

## 2. 언락 전

**목표.** 언락 go/no-go 넷을 채운다. 기기에 아무것도 굽지 않는다.

**종료 조건.** 2.4절의 넷을 모두 채웠다. 하나라도 빠지면 언락하지 않는다. 2026-11-01 조사로는 넷 모두 길이 보이지만, 모두 기기에서 확인해야 한다.

### 2.1 판과 OTA

1. [ ] 기기의 판을 적는다. 사용자 기기는 중국판 ZUXOS 1.5.10.044(Android 16)였고(실측), 2026-11-03에 16.0.20.091(Android 14)로 내렸다.
2. [ ] OTA를 끈다(go/no-go 4). 언락 길이 ZUI 16에 묶여 있으니 받자마자 끈다. 1.5.10.103 이후 판에서는 판 바꾸기 문제가 많이 보고됐다(포럼 보고). 그래서 1.5.10.044에 머문다. 사용자 기기는 2026-11-01에 이미 껐다.

    ```bash
    adb shell pm disable-user --user 0 com.lenovo.ota
    ```

    되돌리기:

    ```bash
    adb shell pm enable com.lenovo.ota
    ```

    앱을 꺼도 이미 올라간 업데이트는 다음 부팅에 적용될 수 있으니(판단), 재부팅 전에 기다리는 업데이트가 없는지 본다. 사용자 기기는 2026-11-02에 보았고 흔적이 없었다. 재부팅 뒤에도 슬롯과 판이 같다([실측 보고서](../hw/lenovo-tb375fc.md) 7절의 "fastboot와 재부팅").

### 2.2 adb로 읽기

root 없이 읽기만 한다.

1. [ ] 사용자 기기는 이미 읽었다([실측 보고서](../hw/lenovo-tb375fc.md) 7절): 판, 잠금 상태, RAM 8GB와 저장장치 128GB, 화면(패널 버퍼 가로 2944×1840), 입력 장치 이름, vendor 펌웨어와 모듈 목록, 지문 센서, 센서(LSM6DSO, TCS37013, SCP 뒤). 다른 기기를 쓰면 다시 읽는다.
2. [ ] 차지 펌프가 어느 칩인지 본다.

    ```bash
    adb shell cat /sys/devices/platform/boardinfo/boardinfo
    adb shell ls /sys/bus/i2c/drivers/nu2115 /sys/bus/i2c/drivers/upm6720
    ```

    - 첫째는 순정 `hardware_info`의 `boardinfo`로 충전 칩 이름이 들어 있다고 보았으나(코드로 본 것) 사용자 기기에는 이 경로가 없다(실측, 2026-11-02). 둘째는 붙은 주소를 보여 준다. SELinux가 막을 수 있다.
    - 사용자 기기는 UPM6720 @0x65다(실측: `/sys/bus/i2c/drivers/upm6720/10-0065`가 있고, NU2115에는 붙은 장치가 없다). 보드 dts의 `vbus-switch-supply`가 이 칩을 가리킨다.
    - 부품이 다른 기기가 있을 수 있다. 그래서 8절 1단계에서 `i2cdetect -r`로 다시 본다.
3. [ ] `/vendor/firmware`는 이름만 볼 수 있다. adb 셸은 파일을 읽지 못한다(실측). 파일은 2.3절 백업이나 언락 뒤 root로 얻는다(4절 8번). 이름과 판 정보는 [pull-android.sh](../../bsp/lenovo-tb375fc/firmware/pull-android.sh)로 받을 수 있다.

4. [ ] 안드로이드를 떠나기 전에 할 일이 둘 있다. 6.10절 1·2번(전원 꺼짐 알람을 끄고 RTC 시간을 읽는다), 9.4절 0번의 구역 온도다. 사용자 기기는 2026-11-02에 보았다. 시계 알람은 없고, RTC와 구역 온도는 셸에 막혀 4절 10번으로 미뤘다.

### 2.3 전체 백업 (go/no-go 1)

mtkclient로는 안 된다(V6 DA에 DAA·SLA). 순정 펌웨어 묶음의 인증 DA·auth와 SP Flash Tool로 readback한다. 이 길은 포럼 보고로 보았고, 기기에서 확인해야 한다. 준비물과 차례, 명령은 [Windows 작업 안내](windows-steps.md)에 있다.

> **위험: 데이터 손실.** nvdata·nvram 계열에는 Wi-Fi MAC과 보정값이 있다. 잃으면 되살릴 수 없다. 이 백업을 해시로 확인하기 전에는 아무것도 굽지 않는다. 뒤의 판 내리기, 언락, GPT 고침을 되돌리는 길이 모두 이 백업이다.

1. [x] SP Flash Tool을 준비한다. Windows 도구다. 인증 DA·auth는 16.0.20.091 묶음에 든 것을 쓴다. 1.5.10.044 묶음은 구한 적이 없고, 더 새 판(1.5.10.103)에 이 묶음을 구운 보고가 있다. 우리가 본 중국판 묶음에는 인증 DA(`DA_BR.bin`), `da.auth`, SLA 파일이 있다.
    - 공용 컴퓨터에서 한다면 도구, 드라이버, 펌웨어 묶음을 외장 SSD에 담아 가고, 백업은 그 컴퓨터의 디스크가 아니라 외장 SSD에 쓴다. `userdata`를 빼면 약 11GiB다. 백업에 Wi-Fi MAC과 보정값이 들어 있다. 드라이버 설치가 막혔거나 재부팅하면 지워지는 컴퓨터일 수 있으니 미리 본다. 해시는 맥에서 다시 맞댄다. 일반적인 판단이고 해 본 적은 없다.
2. [x] 파티션 표를 본다. 16.0.20.091 묶음의 표는 `userdata`가 크기 조정 항목(`NEEDRESIZE`)이고, 사용자 기기와 파티션 이름 84개, 읽을 수 있던 크기 17개가 같다(2026-11-02). 그래서 128GB용 scatter를 따로 받지 않는다(판단). `Auto` readback은 기기의 표를 읽는다(포럼 보고).
3. [x] `userdata`만 빼고 전체 파티션을 readback한다. 개인 자료는 먼저 따로 옮긴다. 특히 `nvram`, `nvdata`, `nvcfg`, `protect1/2`, `persist` 계열을 본다([buntOS 설계안](../design/ARCHITECTURE.md) 4.2.3절). 이 기기의 파티션 이름에는 `proinfo`, `seccfg`, `frp`, `otp`, `efuse`, `sec1`, `md_sec`, `lenovocust`, `lenovolock`, `lenovoraw`도 있다(이름은 실측, 내용은 모른다). 빠뜨리지 않는다.
4. [x] 다른 기기에 옮겨 해시로 확인한다.

**결과(2026-11-03).** 81개를 읽어 맥에서 해시를 모두 맞댔다. 값은 [실측 보고서](../hw/lenovo-tb375fc.md) 7절의 "SP Flash Tool 읽기"에 있다. `userdata`, preloader, GPT는 빠졌고, 도구가 바르게 읽었는지와 되돌리기는 아직 확인하지 않았다. 사본은 같은 SSD의 APFS 쪽에 있다.

### 2.4 go/no-go

| 조건 | 지금 | 확인 |
|---|---|---|
| 1. 전체 파티션 백업과 해시 확인 | 했다(2026-11-03). `userdata`, preloader, GPT는 빠졌다 | 2.3절 |
| 2. 독립된 보고 둘 이상과 판 일치 | 보고는 여럿이다. 판 확인이 남았다 | 아래 1번 |
| 3. BROM·preloader 복구 길 | preloader 연결과 인증 DA 읽기는 됐다(2026-11-03). 쓰기와 BROM은 포럼 보고다 | 아래 2번 |
| 4. OTA 차단 | 판을 내린 뒤 2026-11-03에 다시 껐다 | 2.1절 |

1. [ ] 언락할 판(예: 16.0.20.091)으로 된 성공 보고를 확인한다. ZUXOS 기기는 그 판으로 내린 뒤다. 판마다 결과가 갈린다. 모두 포럼 보고다([조사](porting-materials.md) 5절).
    - 16.0.20.022·.041: 원글 작성자와 다른 사용자 여럿이 성공했다(2025-02).
    - 16.0.20.091: TrebleDroid 위키가 언락한 뒤 GSI를 띄웠다(2025-08). 성공 보고가 있다(2025-09, 2025-11, 2026-09). 256GB 기기가 16.0.20.130에서 .091로 내린 뒤 개발용 LK와 sn.img가 모두 실패한 보고도 있다(2025-12).
    - 글로벌판 17.0.04.184(Android 15): 내리지 않고 개발용 LK로 언락한 보고가 있다(2025-10).
    - ZUXOS 1.5.10.x: 바로 언락한 보고는 없다. 1.5.10.103 기기를 SP Flash Tool로 16.0.20.091까지 내린 뒤 언락한 보고가 있다(2026-09).
    - 레노버가 발급한 sn.img와 개인이 만든 sn.img 웹 도구는 인증에 실패했다는 보고가 둘 있다(2025-09, 2025-12).
2. [ ] 복구 길을 확인한다. mtkclient는 BROM 익스플로잇이 없다. SP Flash Tool과 순정 묶음의 인증 DA·auth로 굽고 되살린 보고가 여럿이다. 중국판 백업을 되돌린 뒤 OTA가 되고 다시 잠글 수 있었다는 보고도 있다(모두 포럼 보고).
3. [ ] 예비 기기 1대를 둔다(설계안 10절). 살 때는 ZUI 16 기기가 낫다.

### 2.5 UART

1. [ ] 기기를 열어 UART 시험 패드를 찾는다. GPIO207이 TX, GPIO208이 RX다(벤더 pinfunc). 위치와 921600bps를 확인한다. 위치와 전압이 공개된 곳이 없다. 포고 핀 UART3(GPIO180·181)은 키보드 줄이다. 커뮤니티 포트는 UART를 쓰지 않았고, 그 포트의 "UART0이 키보드 줄"이라는 주석은 펌웨어와 맞지 않는다.

## 3. 언락

**목표.** 사용자 기기를 언락하고 바로 순정 LK로 되돌린다. 한쪽 슬롯과 recovery는 안드로이드로 남긴다.

**종료 조건.** 기기가 언락 상태다(서명하지 않은 이미지를 `fastboot boot`로 올리려면 이 상태여야 한다). LK가 순정이고, AVB를 껐고, 남겨 둔 안드로이드 슬롯으로 돌아갈 수 있다.

> **위험: 부팅 불가·데이터 손실.** 이 절 전체가 그렇다. 언락은 포럼 보고로만 아는 길이고 판에 묶여 있다.
>
> **먼저 둘 것.** 2.3절 백업과 해시 확인, 2.4절 go/no-go 넷, 예비 1대.
>
> **되돌리기.** 같은 도구(SP Flash Tool)와 순정 펌웨어 묶음으로 2.3절 백업을 되돌린다.

1. [x] ZUXOS(Android 16) 기기는 SP Flash Tool로 중국판 ZUI 16.0.20.091까지 내린다. 사용자 기기가 여기에 든다. 근거는 포럼 보고 하나였고, 1.5.10.044에서 되는 것을 이 기기로 확인했다(2026-11-03).

    > **위험: 부팅 불가·데이터 손실.**
    >
    > - preloader는 굽지 않는다. 구우면 벽돌이 된다는 보고가 있다.
    > - 글로벌 ROM을 중국판 기기에 굽지 않는다. 부팅이 반복된다는 보고("Red State")가 있다.
    > - Android 16 판(ZUXOS 1.5.10.103 이후, 글로벌 17.5.10.095)에서 다른 판의 이미지를 구우면 "Red State"로 부팅하지 못한다는 보고가 여럿이다. 1.5.10.103부터 LK가 글로벌 이미지를 거부한다는 글도 있으나, 작성자가 AI로 썼다고 밝힌 글이라 따로 확인해야 한다.
    > - 128GB 기기에 256GB 기기의 덤프를 구우면 실패한다(포럼 보고).

    - 판을 내린 뒤 슬롯을 바꾼다. 사용자 기기는 `_b`로 돌고 `_a`는 successful no, retry 0이다(실측, 2026-11-02). 16.0.20.091 묶음의 파티션 표는 `_a`에만 굽는다(묶음에서 읽었다). 그래서 LK가 그대로 `_b`로 가서 부팅이 반복될 것으로 본다. `fastboot --set-active=a`로 풀었다는 보고가 여럿이고, 잠긴 기기에서 받았다는 보고도 있다(포럼 보고). 안 받으면 SP Flash Tool로 되돌린다.
    - AVB rollback index는 기기의 `_b`와 묶음이 모두 0이다(2026-11-03, [실측 보고서](../hw/lenovo-tb375fc.md) 7절). 이 값 때문에 막히지는 않는다고 본다(판단). LK와 preloader의 자체 검사는 모른다.
    - 사용자 기기는 2026-11-03에 `_a`에 구웠고, 잠긴 채로 `--set-active=a`가 됐다. 지금의 preloader가 2025-01 LK를 받아 fastboot까지 올라왔다(실측). 복구 화면에서 초기화한 뒤 Android 14로 부팅했고, `_a`, locked, green이다. OTA는 다시 껐다.
    - 차례는 [Windows 작업 안내](windows-steps.md) 6·7절에 있다.

2. [x] 언락할 판이 2.4절 1번에서 확인한 판과 같은지 본다. 사용자 기기는 16.0.20.091이다.
3. [ ] 파티션 이름과 슬롯을 본다.

    ```bash
    fastboot getvar all
    ```

    잠긴 1.5.10.044에서 읽은 값은 [실측 보고서](../hw/lenovo-tb375fc.md) 7절에 있다. 판을 내린 뒤 다시 본다. `Bootloader_SN_Part1`, `Part2`와 일련번호는 문서에 옮기지 않는다.

4. [x] 개발용 LK를 순정 LK와 견준다(문자열, 서명 영역). 개발용 LK는 4pda에 올라온 덤프에서 나와 출처를 확인할 수 없다.

    **결과(2026-11-03).** 사용자가 XDA 원글의 `lk_a`(MD5 `2c01c2ecf1768555d75abf0e42cf7a78`)를 받았다. 구역 다섯의 `cert1`이 16.0.20.091 순정 LK와 바이트까지 같고, `cert2` 서명이 그 안의 키로 검증되며, 구역 내용과 머리글의 SHA-256이 `cert2` 값과 맞는다. 곧 순정 LK와 같은 키로 서명했고 그 뒤 고치지 않았다. 빌드는 `19ce4236_202411042159`(`userdebug`, 서명 2024-11-05)이고, 순정의 `flashing unlock` 거부 문구가 없다. 안전을 보장하지는 않는다(키 유출을 배제할 수 없고 코드는 문자열만 보았다). 기기에는 아직 굽지 않았다.
5. [x] 개발용 LK(`lk_a`)를 굽는다. 중국판 ZUI 16의 순정 LK가 잠긴 채로 `fastboot flash lk`를 받았다(실측, 2026-11-03).

    ```bash
    fastboot flash lk <개발용 LK 파일>
    ```

    `has-slot:lk`가 yes라(실측) 이 명령은 그때의 `current-slot`에 굽는다. 먼저 `fastboot getvar current-slot`을 본다. 7번도 같은 슬롯에 되돌린다. 2026-09 보고는 `lk_a`와 `lk_b`에 모두 구웠다.

    > **위험: 부팅 불가.** 출처를 확인할 수 없는 부트로더를 굽는다.
    >
    > **먼저 둘 것.** 4번의 비교, 7번에 쓸 순정 LK 파일.

6. [x] 언락한다. 언락 파일 없이 됐다(실측, 2026-11-03). 개발용 LK로 시스템을 한 번 부팅하고 fastboot로 돌아와 실행했고, 볼륨 키로 승인하자 `unlocked: yes`, `secure: no`가 됐다.

    ```bash
    fastboot flashing unlock_critical
    ```

    - 안 되면: 2026-09 보고는 개발용 LK를 구운 뒤 시스템으로 한 번 부팅하고 fastboot로 다시 들어가야 언락됐다고 적었다. 2025-11 보고는 OEM 잠금 해제를 켜지 않아 실패했다가 켠 뒤 됐다.
    - 순정 LK의 길은 개발자 옵션의 OEM 잠금 해제, 일련번호에 묶이고 레노버가 서명한 언락 이미지, `fastboot oem unlock`(또는 `flashing unlock`)이다(LK 문자열). 레노버가 이 모델에 언락 파일을 주는 길은 찾지 못했다. 사용자 기기의 16.0.20.091 순정 LK에서 `flashing unlock`은 `oem unlock`을 쓰라며 거부했고, `oem unlock`은 `ERROR: Sn Image Auth fail`로 거부됐다(실측, 2026-11-03). 잠금은 그대로다.
    - 잠긴 1.5.10.044의 순정 LK에서 `fastboot flashing get_unlock_ability`는 `unlock_ability is false`였고, 개발자 옵션의 OEM 잠금 해제를 켠 뒤 `true`가 됐다(실측, 2026-11-02). 이 값만으로 언락이 되는지는 모른다. 판을 내린 뒤 다시 보고, `false`면 안드로이드에서 다시 켠다. `getvar`의 `flash_lock`은 켜기 전후 모두 `no`이고 뜻은 모른다.
    - 그래도 안 되면 buntOS ADR-011(기기 선택)을 다시 본다.
7. [x] 바로 순정 LK로 되돌린다. 2026-11-03에 16.0.20.091 LK를 16MiB로 채워 구웠다. 판이 `8fd2b0938`로 돌아왔고, 언락은 유지됐으며, `userdata`가 지워져 설정 마법사부터 시작했다.

    ```bash
    fastboot flash lk <순정 LK 파일>
    ```

8. [x] AVB를 끈다. 포럼 보고는 vbmeta 셋을 `--disable-verity --disable-verification`으로 다시 굽는다. 사용자 기기(2026-11-03)에서 fastboot 36.0.2는 이 옵션으로 `vbmeta`를 구울 때 `Failed to find AVB_MAGIC at offset: 0`로 멈췄다(파일에는 `AVB0`이 있었다). 그래서 `vbmeta.img`의 123번 바이트만 3으로 바꾼 파일을 옵션 없이 구웠다. 다음 부팅은 복구 화면이었고 `Factory data reset` 뒤 부팅했다. `verifiedbootstate`는 orange, verity 장치는 0개다. 되돌리기는 원본 `vbmeta.img`를 다시 굽는 것이다(해 보지 않았다).
9. [ ] 슬롯을 지킨다. 한쪽 슬롯과 recovery는 안드로이드로 남긴다. 정보원이자, 펌웨어를 뽑는 곳이자, 복귀 길이다. recovery는 따로 된 파티션이 없고 vendor_boot 안에 있다. GPUEB, `connsys_wifi`, `connsys_bt` 파티션도 지킨다. 이 펌웨어는 LK가 올린다(설계안 D6).
    - Virtual A/B라(실측) super의 논리 파티션은 마지막으로 구웠거나 OTA로 병합한 슬롯 쪽만 온전하다고 본다(AOSP 동작, 기기에서 확인하지 않았다). 남겨 둘 슬롯은 3번에서 본 `current-slot`, 곧 판을 내린 쪽이다.

## 4. 언락 뒤 root로 읽기

**목표.** 2a에 쓸 값을 남겨 둔 안드로이드에서 읽는다. 장치 트리의 framebuffer·ramoops를 켤 값과 루트 파티션 자리를 정할 표를 얻는다.

**종료 조건.** framebuffer 주소·크기·형식, ramoops 틈, 예약 메모리 배치, 파티션 표를 적었다. 2a를 위해 `framebuffer0`·`ramoops`를 켤 수 있다.

읽기만 한다.

root는 Magisk를 1안으로 둔다. 기기에서 확인하지 않았다. [공식 안내](https://topjohnwu.github.io/Magisk/install.html)대로 램디스크가 든 파티션의 이미지를 기기에서 패치해 fastboot로 굽는다. 이 기기는 `init_boot`다.

```bash
fastboot flash init_boot <패치한 이미지>
```

> **위험: 부팅 불가·데이터 손실.** 안내는 남이 패치한 이미지를 쓰지 말고, 같은 모델이라도 다른 기기에서 패치하지 말라고 적었다. vbmeta를 끄는 일은 데이터를 지울 수 있다고 적었다.
>
> **먼저 둘 것.** 2.3절 백업, 패치 전의 `init_boot`.
>
> **되돌리기.** 패치 전의 `init_boot`를 다시 굽는다.

1. [x] `/proc/iomem`과 살아 있는 트리(`/proc/device-tree`)를 뜬다(2026-11-03, git 밖에 저장). root 없이는 `/proc/iomem`이 막혀 있었고, `/proc/device-tree`는 이름만 보이고 값은 막혀 있었다(실측).

    ```bash
    cat /proc/iomem
    ```

2. [x] framebuffer 주소·크기·형식을 확인한다. 주소와 크기는 아래 후보와 같고 LK가 예약한다(실측, 2026-11-03). stride와 형식은 트리에 없어 첫 부팅에서 본다. `/chosen`의 `atag,videolfb`와 `/reserved-memory`를 본다. LK는 화면 영역을 `mblock-14-framebuffer`로 예약한다(이름은 실측). 패널이 그리는 버퍼는 가로 2944×1840이다(실측, SurfaceFlinger의 `framebufferSpace`. 화면 회전 설정과 상관없다).
    - 후보(커뮤니티 포트): `0xfc16f000`, 2944×1840, stride 11776, `a8r8g8b8`, 예약 크기 `0x3e90000`(LK가 할당한 크기, 세 화면보다 조금 크다). 그 포트는 LK 기록(mblock 할당, `atag,videolfb`)에서 읽어 이 주소로 fbcon을 띄웠다(12GiB 기기). 우리 기기는 8GB다. 같은지 본다.
    - 맞았다. `framebuffer0`과 `framebuffer_mem`을 켰다. LK의 `mblock-14-framebuffer`에는 `no-map`이 없어(실측) 그것만으로는 simpledrm이 매핑하지 못한다. LK의 노드가 트리의 앞에 오므로(실측) 패치 0140으로 `no-map`을 먼저 처리한다.
    - 다르면: 읽은 주소로 `simple-framebuffer`를 만든다.
3. [ ] 화면의 실제 크기를 확인해 둔다(벤더 값 273.615×171.009mm, 트리에 273×171을 넣었다). DSI에는 EDID가 없다.
4. [x] ramoops 틈을 본다. LK가 이 자리를 `mblock-2-pstore`(0x48090000, 0xe0000)로 예약하고 명령줄에 `ramoops.*`를 넣는다(실측, 2026-11-03). 노드는 명령줄이 없을 때를 위해 둔다. 살아 있는 트리의 예약 영역으로 `0x48090000`~`0x4816ffff`가 비었는지 본다. 앞뒤는 `aee_debug_kinfo`(`0x48080000`+0x10000)와 `minirdump`(`0x48170000`)다.
    - LK는 pstore 자리를 `mblock-2-pstore`로 예약한다(이름은 실측, 값은 root 필요).
    - LK가 명령줄에 `ramoops.*`를 넣는다(실측). 노드도 두었다. 드라이버는 하나만 받고, 둘째는 `already initialized`로 실패한다(코드로 본 것).
    - 크기가 `0xe0000`을 넘으면 minirdump를 덮는다.
5. [x] 예약 메모리의 실제 배치를 본 뒤 pstore 칸을 정한다. 값은 [실측 보고서](../hw/lenovo-tb375fc.md) 7절에 있다. 정적 트리에는 동적 할당 셋뿐이고, TEE, GenieZone, connsys, framebuffer의 고정 예약은 preloader·LK가 부팅 때 더한다. BL31 자리는 `0x48800000`, 2MB다.
6. [x] 입력 장치를 읽는다(2026-11-03). 셸로는 열리지 않았다(실측).

    ```bash
    getevent -lp
    ```

7. [x] 파티션 표를 뜬다. 기기 GPT의 형식 GUID는 모두 `ebd0a0a2-b9e5-4433-87c0-68b6b72699c7`이다(실측, 2026-11-03). 크기와 이름은 아직 정하지 않았다. GPT의 userdata 자리를 보고, 새 루트 파티션의 크기와 이름을 정한다(보기: userdata 108G 가운데 32~48G). 여기서는 고치지 않는다. 고치는 일은 6.8절이다.
8. [x] 펌웨어 파일을 얻는다. `/vendor/firmware`를 git 밖에 받았고 `extract.sh`는 VM에서 다시 돌린다(2026-11-03). 2c의 터치에 `vendor/firmware/novatek_ts_fw_boe.bin`과 `novatek_ts_fw_tm.bin`이 있어야 한다. 파일은 저장소에 넣지 않는다. 고르는 일은 [extract.sh](../../bsp/lenovo-tb375fc/firmware/extract.sh)와 [manifest.tsv](../../bsp/lenovo-tb375fc/firmware/manifest.tsv)가 한다.
9. [x] efuse 낱말 `0x1cc`(devinfo)의 [5:0]을 읽어 둔다. 2026-11-03에 읽었다(기기마다 다른 보정값이라 숫자는 적지 않는다). 6.7절 6번의 U2 INTR_CAL과 견준다.
10. [x] 안드로이드 셸에 막혔던 값을 읽는다(실측, 2026-11-02). root로 둘 다 읽었다(2026-11-03). RTC 시간(6.10절 2번)과 구역 온도(9.4절 0번)다.

    ```bash
    date -u; cat /sys/class/rtc/rtc0/date /sys/class/rtc/rtc0/time /sys/class/rtc/rtc0/since_epoch
    for z in /sys/class/thermal/thermal_zone*; do echo $(cat $z/type) $(cat $z/temp); done
    ```

## 5. 2a 첫 부팅

**목표.** 파티션을 건드리지 않고 `fastboot boot`로 RAM에서 우리 커널을 띄운다. 루트 파일 시스템은 initramfs다.

**종료 조건.** initramfs 셸이 화면이나 UART에 뜬다. 커널 기록으로 EL2를 판정한다.

통로는 화면 콘솔(simple-framebuffer + simpledrm), pstore/ramoops, LK 기록(expdb 파티션), UART다. USB 가젯은 아직 없다. UART를 찾지 못했으면 셸에 입력할 길이 없으므로, [init](../../bsp/lenovo-tb375fc/initramfs/init)이 화면에 찍는 줄(`cmdline:`, `vendor_boot:`, EL 줄)로 본다(이 문서의 해석이다. 10절 5번).

### 5.1 준비

1. [x] 4절의 값으로 보드 dts의 `framebuffer0`·`ramoops`를 켠다(2026-11-03). `uart0`은 켜져 있다. 2a 동안 uart0의 `bus`는 clk26m이다.
2. [x] 이미지 넷(boot, vendor_boot, init_boot, dtbo)을 만든다. [BSP README](../../bsp/lenovo-tb375fc/README.md) "만들기" 1~3번이다. 입력, 명령줄, 해시는 `images.txt`에 남는다. 기기 없이 할 수 있는 시험(`boot/test.sh`, QEMU 맛보기, `initramfs/test.sh`)도 그 README에 있다.
3. [x] `mkimages.sh --boot-ramdisk`로 만든 boot.img도 만들어 둔다. initramfs를 boot.img에도 넣은 이미지다.
4. [ ] 멈춤을 볼 도구를 고른다. 부팅 단계마다 화면에 블록을 그리는 카나리와 `panic=0`(패닉 화면을 남긴다)을 쓸 수 있다.
5. [ ] Android에서 100%까지 충전하고 `dumpsys battery`로 전압과 온도를 적어 둔다(99%, 4466mV, 25.7도, 2026-11-03). 충전 드라이버가 없는 2a~2d 동안 배터리는 다음과 같다(MediaTek LK2 코드로 본 것이고 기기의 LK는 더 새 판이다).
    - LK는 입력과 충전을 500 mA로 두고 충전기 종류와 상관없이 그대로 넘긴다. 5 V 500 mA면 약 2.2 W이고 화면과 CPU 부하는 그보다 클 것으로 본다(추정, 측정 아님). PC나 벽 충전기에 꽂아 두어도 천천히 줄 수 있다.
    - 게이지가 없으면 잔량 경고도 저전압 종료도 없다. 마지막 방어선은 PMIC UVLO(순정 값 2.5 V)이고, 거기까지 가면 예고 없이 꺼지며 로그도 사라진다. 순정이 종료하는 값은 3.0 V 근처다.
    - 과충전 쪽 위험은 작다. 드라이버가 없으면 아무도 칩에 쓰지 않고, 칩 자체 보호는 LK가 둔 그대로다. 다만 온도에 따라 멈추는 소프트웨어 JEITA는 없다.
    - 몇 시간짜리 시험이면 중간에 잔량을 본다. 싼 차례는 게이지(I2C7 @0x55, 드라이버가 없어도 `i2cget`으로 읽기만), MT6363 AUXADC(`mt6359-auxadc`, 노드를 더해야 한다), Android로 재부팅, USB-C 전력계(입력이 500 mA에 붙어 있으면 부하가 입력보다 크다)다.
    - 전원을 끈 뒤 USB를 꽂으면 LK는 꺼진 채 충전하지 않고 커널을 띄운다(코드 기준). 충전이 필요할 때를 위해 Android로 돌아갈 길을 끝까지 남긴다. buntOS 커널을 굽는 시점에 다른 슬롯이나 되돌릴 이미지를 먼저 챙긴다.

### 5.2 `fastboot boot`

**막혔다(2026-11-03). 첫 부팅은 5.3절 1번의 시험 커널로 성공했다([실측 보고서](../hw/lenovo-tb375fc.md) 7절).** 기기에서 `Booting FAILED (remote: 'unknown command')`였고, 명령 표를 읽어 보니 16.0.20.091 순정 LK와 개발용 LK 모두 `boot`를 등록하지 않는다(2026-11-03). 앞서 근거로 삼은 LK 문자열의 `boot`는 파티션 이름이었다. 그래서 5.3절 2번으로 바로 간다. 아래 항목에서 LK가 무엇을 붙이는지, 명령줄이 들어오는지는 구운 슬롯에서 본다.

1. [ ] RAM으로 띄운다. LK의 fastboot 명령 표에 `download:`와 `boot`가 있다(LK 문자열). 파티션을 건드리지 않는 길이다(설계안 D3). 멈췄을 때 되살리는 키는 전원 + 볼륨 올림으로 본다. 안드로이드 상태에서 fastboot로 들어가는 것은 실측했고, 리눅스가 멈췄을 때 듣는지는 모른다(10절 4번).

    ```bash
    fastboot boot boot.img
    ```

    `max-download-size`가 128MiB다(실측). 이보다 큰 boot.img는 받지 못한다. `mkimages.sh`가 64MiB 파티션 크기로 먼저 막는다.

2. [ ] LK가 이미지를 받는지 본다. 원래 이미지와 달리 AVB 꼬리, 파티션 크기 채움, dtbo 뒤의 MediaTek 서명 블록(cert1·cert2)이 없다. LK가 언락 상태에서 이것을 요구하는지는 기기에서 본다.
3. [ ] LK가 어느 vendor_boot·init_boot를 붙였는지 본다.
    - `fastboot boot`는 boot.img 하나만 RAM으로 보낸다. 헤더 v4에서 장치 트리는 vendor_boot에, initramfs는 init_boot에 있다.
    - LK는 세 머리를 모두 읽고, 장치 트리를 예약 메모리 `mb_dtb`로 읽어 커널에 넘긴다(LK 문자열). 그래서 굽혀 있는 원래 vendor_boot·init_boot를 붙일 가능성이 높다. 이 결합만 기기에서 확인하지 않았다.
    - init이 표지(`/etc/buntos/vendor_boot`)로 알린다. 표지가 없으면 `vendor_boot: no buntOS marker (the stock vendor_boot may be in use)`를 찍는다.
    - `--boot-ramdisk`로 만든 boot.img로 LK가 그 램디스크를 쓰는지 본다.
    - 원래 장치 트리로 뜨면 5.3절로 간다.
4. [ ] 명령줄이 들어왔는지 본다. LK가 무엇을 덧붙이는지와 명령줄 길이 한도도 적는다. boot v4의 한도는 1536바이트다.

    ```bash
    cat /proc/cmdline
    ```

    - 근거: MT6895 xaga 포트의 주석이 LK가 boot·vendor_boot 명령줄을 버린다고 적었다. MT6897 LK에 vendor_boot DT 표로 트리를 넣어 시험한 기록은 없다.
    - 순정 트리의 `/chosen`은 `root=/dev/ram`이고 탭이 섞여 있다. LK가 순정 vendor_boot를 쓰면 이 값이 온다.
    - 안 들어오면: 커널 `CMDLINE`과 `CMDLINE_EXTEND`로 준다.
    - DTB를 커널에 넣는 커뮤니티 포트의 우회는 기본 길로 삼지 않는다.

### 5.3 비운 슬롯에 굽기 (원래 장치 트리로 떴을 때)

1. [x] 장치 트리를 커널에 넣은 시험용 커널을 `_b`에 구워 띄웠다(2026-11-03, `fastboot boot`는 LK에 없다. 도구는 [kernel/tb375fc/test-kernel](../../kernel/tb375fc/test-kernel/README.md)). 처음 계획: 굽기 전에 한 번 더 보려면 장치 트리를 커널에 넣은 시험용 커널을 `fastboot boot`로 띄운다. 이 커널은 LK가 더하는 메모리·예약 값을 버리므로, 그 기기의 메모리 크기에 맞춘 예약을 함께 넣어야 한다. BL31(`0x48800000`, 2MB)도 직접 예약한다. 안 하면 보조 CPU와 서스펜드가 멈춘다(커뮤니티 포트). 그 포트의 12GiB 고정값은 8GB 판에 맞지 않는다. 업스트림에는 내지 않는다.
2. [ ] 비운 슬롯에 넷을 굽는다. 2026-11-03 시도: 서명 없는 `dtbo`는 LK가 거부하고, 0으로 채우면 넘어가지만 우리 트리에 LK의 펌웨어 적재가 찾는 벤더 노드가 없어 `app_load_scp`에서 멈춘다. 그 노드를 더해야 이 길이 열린다. 파티션 이름과 슬롯은 `fastboot getvar all`로 확인한다. 우리 장치 트리로 띄우는 확실한 길이다.

    > **위험: 부팅 불가.** 구운 슬롯이 부팅하지 못할 수 있고, 우리 vendor_boot를 구운 슬롯에서는 순정 recovery를 쓸 수 없다.
    >
    > **먼저 둘 것.** 남겨 둘 안드로이드 슬롯, 2.3절 백업.
    >
    > **되돌리기.** 남겨 둔 슬롯으로 바꾼다. LK의 fastboot 명령 표에 `set_active:`가 있다([조사](porting-materials.md) 5절).

### 5.4 LK가 고치는 것

1. [ ] `/sys/firmware/fdt`를 떠서 우리 dtb와 견준다. init이 같은 파일을 `/run/buntos/fdt.dtb`에 뜬다.
    - `/chosen`의 자식 노드가 남는지 본다. 사라지면(MT6895 xaga 포트에서 보고됨) framebuffer를 루트로 옮긴다.
    - `/chosen`을 다시 쓰면 `stdout-path`도 사라질 수 있다. 그래서 명령줄에 `console=`을 넣었다.
    - `/memory`가 실제 크기로 바뀌는지 본다. 우리 트리는 1GiB 자리이고, 부트로더가 8·12GiB로 고친다.
    - `/reserved-memory`에 `mblock-*` 노드가 붙는지 본다. BL31(`0x48800000`, 2MB)이 들어오지 않으면 직접 예약한다.
    - `/cpus` 노드를 이름으로 건드리는지 본다. 우리 이름(`cpu@100`)이 펌웨어 이름(`cpu@100` = 첫 A715)과 가리키는 코어가 다르다.
2. [ ] LK 기록에 `get panel node:... failed`가 나오고 부팅이 이어지는지 본다. 우리 트리에는 LK가 찾는 패널 경로가 없다. 역어셈블로는 기록만 남기는 길이고, 멈추면 2a를 막는 일이다. 같은 기록의 `we will use lcm: …`로 이 기기의 패널을 적어 두고 GPIO184 값과 맞는지 본다.

### 5.5 화면, pstore, UART

1. [x] 화면 콘솔이 뜨는지 본다(2026-11-03, 시험 커널). LK가 dual-DSI + DSC로 켜 둔 2944×1840 화면을 그대로 받는다.
2. [x] 따뜻한 재부팅 뒤 ramoops가 지워지지 않는다(실측, 2026-11-03. `buntos.autoreboot`로 시험했다. 전원 + 볼륨 올림 리셋 뒤에는 비어 있다). 처음 항목: 따뜻한 재부팅 뒤 ramoops가 지워지지 않는지 본다. init이 직전 기록을 `/run/buntos/pstore`에 떠 둔다. pstore 기록이 있는 경우는 QEMU에 ramoops 백엔드가 없어 시험하지 못했다.
3. [ ] 순정 recovery로 재부팅해 pstore와 LK 기록을 읽는 길을 본다. LK 기록은 expdb 파티션에 남고 순정 recovery에서 뜬다. 우리 vendor_boot를 구운 슬롯에서는 남겨 둔 슬롯으로 바꿔 recovery를 쓴다.
4. [ ] UART0을 본다(TX GPIO207, RX GPIO208, 921600bps). 부트로더가 UART0 클럭을 끄고 넘기면 레지스터 접근이 멈출 수 있다. 그러면 `uart0`을 끄고 화면과 pstore로 본다.

### 5.6 안정성

1. [x] 워치독이 커널이 넘겨받은 뒤 다시 켜지지 않는지 본다(`CONFIG_MEDIATEK_WATCHDOG=y`). 셸에서 1분 넘게 두어도 재부팅되지 않았다(2026-11-03).
2. [ ] idle·cpufreq를 본다. 멈추면 `cpuidle.off=1`, `initcall_blacklist=mtk_cpufreq_hw_driver_init`로 하나씩 뺀다.
3. [ ] systimer는 첫 부팅이 된 뒤에 켜 본다. 펌웨어가 같은 채널(`+0x40`)을 쓰지 않는지(멈춤, 인터럽트 폭주) 본다. 그 전에는 `disabled`이고 hrtimer broadcast가 대신한다.

### 5.7 전원 도메인 상태 (읽기만)

1. [ ] `0x1c001fb0`, `0x1c001fb4`(SPM 상태), `0x1c001e04`(conn), `0x10321410`(HW voter EN). 물리 주소 읽기는 1절의 시험 도구를 쓴다. 2026-11-04 실측: 화면이 켜져 있어도 EN은 커널 시작부터 `0x80000000`이다(bit 0, 3, 4, 5, 6이 설 것이라는 짐작은 틀렸다). 화면 전원은 SPM `PWR_CON`에서 보인다: MDP(`0xe94`, `0xe98`)는 꺼져 있고 그다음 `0xe9c`~`0xea8`이 켜져 있다(`0xC200224D`, 순서로 본 DIS0, DIS1, OVL0, OVL1). `conn`(`0xe04`)은 꺼져 있다(`0x02000012`). `0x1c001fb0`, `0x1c001fb4`는 아직 읽지 않았다.

### 5.8 EL2 판정

1. [x] **판정: EL1, KVM 불가(2026-11-03).** `CPU: All CPU(s) started at EL1`, `kvm [1]: HYP mode not available`이다. 커널 기록의 `CPU: All CPU(s) started at EL2`(또는 `EL1`)와 kvm 초기화 메시지를 본다([buntOS 설계안](../design/ARCHITECTURE.md) 12.1절). 안드로이드 쪽 예비 판정은 GenieZone이 EL2를 차지할 가능성이 높다는 것이다.
    - 가능: 커널이 EL2에서 시작하고 `/dev/kvm`이 생긴다.
    - 결정(2026-11-04, 사용자): 일반 개발 환경에는 EL1로 충분해 이 기기로 그대로 간다. 다른 태블릿을 매입하면 그때 다시 시도한다.
    - 불가(EL2 점유): 커널이 EL1에서 시작하고 GenieZone 흔적이 있다. KVM을 포기하고 gzvm(미병합)은 나중에 검토한다(설계안 D8). buntOS ADR-011을 다시 본다(설계안 10절).
    - TEE를 고쳐 EL2를 빼앗는 실험(`mtk-mod-tee-nogz`)은 쓰지 않는다. 서명 우회가 필요하고 벽돌 위험이 크다.

## 6. 2b 저장장치·디버그

**목표.** UFS의 Debian 루트로 넘어가고, USB 가젯으로 기기에 들어간다.

**종료 조건.** UFS의 Debian 루트, USB 가젯 SSH.

- 꺼 둔 노드(UFS, USB, SPMI 등)를 핀·클럭 패치와 함께 하나씩 켠다. PMIC 쪽은 핀(EINT) → SPMI → MT6319 → MFD·MT6363 순서다.
- UFS와 USB에는 전원 도메인이 없다(펌웨어 트리, 벤더 scpsys 표).
- USB 호스트는 이 단계에 없다. `dr_mode = "peripheral"`로 둔다.
- 읽기 명령은 셸이 있어야 한다. UART를 찾지 못했으면 USB 가젯이 뜨기 전에는 입력할 길이 없다. 가젯은 pio, 클럭, SPMI, MT6368이 있어야 뜬다(6.7절). 그 경우 6.1~6.5절의 노드를 켠 채 6.7절까지 간 뒤 앞 절의 읽기를 하고, 그때까지는 화면 콘솔과 pstore로 본다(이 문서의 해석이다. 10절 5번).

### 6.1 핀(pio)

1. [x] `pio`를 켠다(2026-11-03, 시험 커널. 결과는 [실측 보고서](../hw/lenovo-tb375fc.md) 7절). 드라이버는 패치 0004~0006, 0027, 0028이다. 켜면 probe가 EINT 마스크를 모두 세운다(업스트림 동작). 핀 상태(`pinctrl-0`)는 쓰는 노드가 생길 때 더한다.

### 6.2 클럭

드라이버는 패치 0009~0013이다. 표는 OEM 코드에서 옮겼고 기기 시험은 없다.

1. [x] 클럭 컨트롤러 여섯(2026-11-03, 시험 커널. uart0은 꺼 둔 채다)(topckgen, apmixedsys, vlpckgen, pericfg_ao, ufscfg_ao, ufscfg_pdn)과 HW voter syscon(`0x10320000`)을 켠다. uart0의 `bus`를 pericfg_ao 게이트로 바꾼다. `clk_ignore_unused`는 그대로 둔다. 없으면 simple-framebuffer가 쓰는 화면 먹스(disp·ovl·mminfra)와 쓰지 않는 UFS 게이트가 늦은 초기화에서 닫힌다.
2. [x] apmixedsys가 뜨는지 **먼저** 본다(떴다). mainpll은 critical이라 등록할 때 바로 HW voter로 투표한다.
    - 패치 0012의 순서가 틀리면 `critical clk 'mainpll' failed to prepare`가 난다. 그러면 apmixedsys가 실패해 모든 클럭이 고아가 되고, UART·UFS가 클럭을 기다리며 뜨지 않는다.
    - 처음 부팅은 화면 콘솔과 pstore로 본다.
3. [x] 클럭 값을 읽는다(2026-11-03. PLL 넷이 짐작과 맞고 먹스는 LK 값이다. `aes_ufsfde`는 6.6절에서 본다).

    ```bash
    cat /sys/kernel/debug/clk/clk_summary
    ```

    - OEM 분수에서 거꾸로 짐작한 값과 견준다: mainpll 2184 MHz, univpll 2496 MHz, tvdpll 약 594 MHz, ulposc 588 MHz.
    - 먹스의 부모가 LK가 정한 대로 읽히는지 본다: `uart`, `usb_top`, `ssusb_xhci`, `ufs`, `aes_ufsfde`, `vlp_spmi_m`, `vlp_pwrap_ulposc`.
    - 화면·코덱 게이트(패치 0100~0104)를 켠 뒤에는 그 줄의 `hardware enable`이 `?`다. 이 게이트는 레지스터를 읽지 않게 만들었다. 실제 상태는 해당 전원 도메인이 켜져 있는 것을 먼저 확인하고 레지스터로 읽는다.
    - **`/sys/kernel/debug/regmap/dummy-clock-controller@0x00000000<주소>/registers`를 이 블록(`14000000`, `14200000`, `14400000`, `14600000`, `1600f000`, `1602f000`, `17000000`, `17800000`, `1e800000`, `1f000000`, `1f800000`)에서 읽지 않는다.** 도메인이 꺼져 있으면 버스가 멈출 수 있다. 이 블록을 클럭 코어가 스스로 읽는 길은 없고, 이 파일만 남는다.
4. [ ] HW voter 시간을 본다. 먹스·게이트는 5 ms 안에 DONE과 게이트 비트를 본다(패치 0101. 업스트림은 30µs이고 OEM은 몇 초까지 기다린다). `-ETIMEDOUT`이 나면 늘린다. 2b에서 실제로 쓰는 HWV 클럭은 PLL이다. `i2c` 먹스(HWV CG 21)는 I2C 장치가 붙을 때 처음 쓴다(8절).
5. [ ] UART 콘솔을 본다. 8250_mtk가 pericfg_ao 게이트를 기다리므로 probe가 pericfg_ao 뒤로 밀린다. earlycon이 그 사이를 메운다. 게이트를 켜도 부모 `uart` 먹스가 LK 상태(26M일 것)를 유지하는지 본다.
6. [ ] USB는 `usb_top`·`ssusb_xhci` 게이트 먹스와 frmcnt 게이트를 켠다. 나머지는 LK 상태 그대로다. UFS 호스트의 클럭은 6.6절에서 본다.
    - **LK가 돌리는 블록의 부모 PLL을 다른 장치와 함께 쓰면 꺼질 수 있다(2026-11-04 실측).** `clk_ignore_unused`는 쓰지 않는 클럭을 늦은 초기화에서 끄지 않을 뿐, 다른 장치가 켰다 끈 뒤 사용자 수가 0이 된 PLL은 막지 못한다. UFS와 화면이 `mmpll`을 함께 써서 화면이 죽었고, 시험 층에서 `framebuffer0`의 `clocks`로 잡아 두었다. 새 장치를 켤 때 `clk_summary`에서 부모 PLL을 보고, 화면 쪽과 겹치면 같이 잡는다.
7. [ ] `clk32k`의 펌웨어 값 32000 Hz가 맞는지(26 MHz에서 나눈 32K인지, RTC 수정의 32768인지) 안드로이드나 커널 기록으로 본다. 2b 소비자에는 영향이 없다.

### 6.3 SPMI

고침은 패치 0002·0003(백포트)과 0007·0008, 0114다.

1. [x] `pio`와 함께 SPMI를 `okay`로 바꾼다(2026-11-03, 시험 커널). PMIC 여섯(MT6363 @4, MT6368 @5, MT6319 @6~8, MT6685 @9)은 모두 M 버스에 있다. M 자식만 켜고 P 자식은 끈다. 별칭은 `spmi0` 하나다. MT6685 @9는 6.10절에서 켠다.
2. [x] probe가 클럭 때문에 실패하지 않는지 본다(통과). 업스트림은 자식마다 `rcs` 인터럽트(pio EINT 248)와 클럭 셋(`pmif_sys_ck`, `pmif_tmr_ck`, `spmimst_clk_mux`)을 요구하고, 없으면 probe가 실패한다. PMIF 클럭으로 `vlp_pwrap_ulposc`를 준 것은 추론이다. `vlp_pwrap_ulposc`·`vlp_spmi_m`은 게이트가 없어 켜기가 아무 일도 하지 않는다.
3. [ ] 패치 0007을 확인한다. 고치기 전 rcs 디먹스는 `bank = bit / 7`이라 bit 7, 14–15, 21–23, 28–31에서 bank가 하나 커지고, 원래 비트가 남아 체인 IRQ 처리기가 끝나지 않을 수 있었다(코드로 짐작). 6.4절 3번과 6.5절 4번에서 인터럽트가 가만히 있는지 본다.
4. [ ] 패치 0008을 확인한다. 고치기 전에는 채널 설정이 버스 등록 뒤에 있어, 내장(=y) PMIC 드라이버가 먼저 probe하면 SWINF 상태 대신 `PMIF_INIT_DONE`을 읽고 명령도 그 자리에 썼다(코드로 짐작). rcs 인터럽트나 클럭을 기다리다 PMIF probe가 미뤄지면 이 경우에 든다. MT6363, MT6368, MT6319의 probe가 되는지 본다.
5. [ ] 패치 0114를 확인한다. 고치기 전에는 처리기 없는 SID의 RCS를 아무도 지우지 않아 EINT 248이 계속 울릴 수 있었다(코드로 짐작). SID 4·5의 체인 처리는 그대로이고 칸 넷을 읽는 것만 는다(벤더도 인터럽트마다 다 읽는다). `dmesg | grep 'cleared event'`에 줄이 있으면 그 SID와 값을 적어 둔다. MT6319 @6~8에서 나올 수도 있다.

    - `Err:`가 계속 늘면(이벤트 없이 부모가 돈다) P 버스 칸을 의심한다. 벤더는 EINT 248 하나로 M과 P의 칸을 함께 읽고 지운다. 우리는 P를 끈 채 M만 보므로 P 칸에 남은 값을 지우지 못한다. 줄이 두 버스의 OR인지는 코드로 모른다. P 칸은 `0x1cc00020`~`0x1cc0002c`이다. 읽기 전에 `clk_summary`에서 `vlp_spmi_p`가 켜져 있는지 본다(꺼져 있으면 읽기가 버스를 멈출 수 있다).

    ```bash
    for a in 20 24 28 2c; do busybox devmem 0x1cc000$a 32; done
    ```

6. [ ] 0007·0008·0114는 기기에서 확인한 뒤 업스트림에 낸다.

### 6.4 MT6319·MT6363

패치 0014~0018(MFD, 키)과 0037~0041(regulator 고침)이다. MT6319는 DT만 쓴다. 0041·0037·0038은 regulator `for-7.4`에 적용된 커밋(`8aed8b66751e`, `7318d3833067`, `a8d72f246edb`)이고, 대기열 차례에 맞춰 줄 번호만 고쳤다. 0039·0040은 기기 시험 뒤 낸다. 고침은 코드와 레지스터 모델로 확인했고, 모델도 벤더 자료로 만든 것이다.

이 단계에서는 보드 방침을 바꾸지 않는다. 전압 한계를 주지 않고, 펌웨어가 늘 켜 두는 레일은 always-on으로 둔다. 바꿀지는 `TOP_TRAP`과 VEMC 전압을 본 뒤 정한다. 벅 끄기를 일부러 시험하지 않는다. 소비자가 생길 때 VS1·VS3가 그대로인지 `regulator_summary`로 본다.

1. [x] probe를 본다. MT6363 MFD·regulator·키, MT6319 셋이다(2026-11-03, 시험 커널. MT6319 @8은 보드의 전압 한계를 빼고 통과했다).

    ```bash
    dmesg | grep -i -E "spmi|mt6363|pmic|regulator"
    ```

    - `regulators@30` 아래 레일 등록을 본다. 노드가 있는 것은 23개이고 드라이버 표는 35개다. probe가 표 전체를 돌므로 `regulator_summary`에 몇 줄이 나올지는 기기에서 본다.
    - 없어야 하는 줄: "Cannot mask interrupts", "Interrupt N is in no interrupt group", "Could not clear RCS flag".
2. [x] 레일을 읽는다(2026-11-03. 값은 실측 보고서 7절. `vufs_vccq`는 드라이버가 ELR을 읽어 0mV로 나온다).

    ```bash
    cat /sys/kernel/debug/regulator/regulator_summary
    ```

    - MT6363 23개와 MT6319 6개의 이름·켜짐을 본다.
    - 기본 명령줄의 `regulator_ignore_unused` 때문에 늦은 정리는 레일을 끄지 않는다. 이 값을 뺀 시험에서는 30초 뒤 "disabling"이 나오는 것이 VEMC뿐이어야 하고, 그것도 `ufshci`가 켜져 있으면 나오지 않아야 한다.
3. [ ] 인터럽트 폭주를 본다. `mtk-spmi-pmic` 줄이 가만히 있고 `Err:`가 계속 늘지 않아야 한다. `spmi_rcs`와 EINT 248 줄은 없다(7.2.12는 체인 처리기가 붙은 인터럽트를 보여 주지 않는다). **차가운 부팅과 안드로이드에서 재부팅한 경우를 둘 다 본다.**

    ```bash
    cat /proc/interrupts
    ```

    - 앞 커널이 켜 둔 인터럽트는 패치 0016의 초기 마스크가 끈다.
    - MT6363 그룹 HK는 80~87(레지스터 하나)이다. 벤더 번호 AUXADC_IMP·NAG_C_DLTV(88·89)가 켜져 있으면 HK 상태가 지워지지 않아 RCS가 거듭될 수 있다. 벤더도 그 둘을 쓰지 않으므로 꺼져 있을 것으로 본다.
4. [ ] 키를 본다. `evtest`의 장치 이름은 `mtk-pmic-keys`다. 전원·볼륨 올림의 누름과 뗌을 본다. 아주 짧게 눌렀을 때 뗌이 누름보다 먼저 오지 않는지 본다(두 스레드 차례). 볼륨 내림은 SoC 키패드(`kp@1c00e000`, `mediatek,kp`, 행렬 0번 `KEY_VOLUMEDOWN`)라 아직 없다.

    ```bash
    evtest
    ```

5. [ ] 길게 누름 리셋을 본다. 전원+볼륨 올림을 길게 누르면 리셋되고 전원만으로는 되지 않아야 한다(보드는 모드 2, 시간 0. STRUP_CON11 `0xa0e` bit 2, STRUP_CON12 `0xa0f`). 문제가 있으면 `keys`의 `mediatek,long-press-mode`를 1(전원만)로 바꾼다. 8절의 되돌리는 길이 이 리셋이다.
6. [x] `TOP_TRAP`과 VEMC 필드를 읽는다(2026-11-03. `TOP_TRAP` 0, VOSEL 6, VOCAL 5).

    ```bash
    cat /sys/kernel/debug/regmap/0-04-regulator/registers
    ```

    - 주소: `0x6`(TOP_TRAP, 절대 주소 `0x36`), `0x1b10`(VOSEL, trap 0은 [3:0], trap 1은 [7:4]), `0x1b1b`(trap 0의 VOCAL), `0x1b1c`(trap 1의 VOCAL). 0x30을 뺀 값이고, 8비트가 되어 칸마다 한 레지스터다.
    - 어느 칸을 쓸지는 프로브 때 TOP_TRAP을 읽어 정한다(패치 0038).
    - TOP_TRAP이 1보다 크면 레귤레이터 probe가 "Failed to select VEMC bank"로 실패해 MT6363 레일이 하나도 등록되지 않는다.
7. [x] `regulator_summary`의 `vufs_vcc`(VEMC)를 본다(2026-11-03. 2550 mV, VOSEL 6의 2.5V에 VOCAL 5). 7.2.12에서 1.2V나 1.3V로 나오던 값이 VOSEL 값과 같은 전압으로 나와야 한다(VOSEL 6 = 2.5V 등).
8. [ ] 다른 LDO의 VOCAL이 11~15인 곳이 있는지 본다. 범위 모델은 VOCAL을 0~10으로 보고, 11 이상이면 다음 범위로 읽는다.
9. [x] `mt6363_vsram_cpub·cpum·cpul`이 켜져 있는지(2026-11-03. 켜져 있고 비트 0이 1이다. 부팅 기록에 이상이 없다), 그 레지스터(`0x1dd7`, `0x1ded`, `0x1e57`)의 비트 0을 본다. 부트로더가 0으로 두었다면 always-on이 켜므로 부팅 기록에 이상이 없는지 본다(패치 0041).

### 6.5 MT6368

패치 0023~0026이다. 보드는 펌웨어가 늘 켜 두는 레일 6개를 둔다. ssusb는 이 PMIC의 VUSB(`vusb33-supply = <&mt6368_vusb>`)를 기다린다. `CONFIG_REGULATOR_MT6368=y`여야 하고, SPMI M 버스와 MFD가 함께 있어야 한다.

1. [x] probe를 본다(2026-11-03, 시험 커널).

    ```bash
    dmesg | grep -i -E "mt6368|spmi|regulator"
    ```

    `pmic@5` MFD probe(칩 ID는 SWCID 0xB), `0-05-regulator` 하위 장치, 레일 등록을 본다. 드라이버 표는 24개이고 노드가 있는 것은 6개다. "Cannot mask interrupts", "Interrupt N is in no interrupt group", "Could not clear RCS flag"가 없어야 한다.
2. [ ] `regulator_summary`에서 `vgpu`, `vusb33`, vbuck5, vbuck6, vio28, vtp가 켜져 있고 늦은 정리에서 꺼지지 않는지 본다.
    - 전압: vio28은 펌웨어대로 1.8V여야 한다. vusb33은 mtu3 바인딩이 말하는 USB AVDD 3.3V 공급이다. vgpu는 부트로더 값이다.
    - VOCAL 모델이 맞는지 이것으로 본다. LDO 전압이 표 값 + 10mV×VOCAL과 어긋나면(특히 VOCAL이 11~15) 모델을 고친다.
3. [ ] regmap debugfs로 VOCAL 레지스터 몇 개(vusb `0x1d94`, vio28 `0x1d9c`, 벤더 주소)를 읽어 둔다. 드라이버의 주소는 벤더 주소 − 0x30이다.
4. [x] `/proc/interrupts`의 `mtk-spmi-pmic` 줄이 가만히 있고 `Err:`가 계속 늘지 않는지 본다(2026-11-03. 0이다). SID 5의 RCS가 pmif에서 bank 1(비트 8~15)로 오는지는 패치 0007과 함께 본다.
5. [ ] 끈 뒤 전류가 0에 가까운지 본다(ELR5로 MT6368이 끄기 순서를 따르는지). 재부팅에 문제가 없는지 본다.

    ```bash
    poweroff
    ```

### 6.6 UFS

패치 0019~0022이다. 클럭·리셋 이름과 OPP는 업스트림 MT8196 시리즈(v14, 병합 전) 꼴이다. 호스트 클럭은 게이트 여섯(`main`, `aes`, `unipro_sysclk`, `tx_symbol`, `rx_symbol0`, `rx_symbol1`)이고 PHY는 클럭을 받지 않는다. 클럭 스케일링은 없다. `vcc`는 MT6363 VEMC, `vccq`는 MT6319(SPMI 8) vbuck3다.

> **위험: 데이터 손실.** UFS에는 안드로이드 파티션과 nvdata가 함께 있다. 6.8절의 GPT 고침 전에는 읽기만 한다. initramfs의 실패 길은 nvdata·nvcfg·persist를 읽기 전용(`blockdev --setro`, `ro,noload`)으로 붙인다.

1. [x] probe 차례를 본다(2026-11-04, 시험 커널. 읽기만 했다. 값은 [실측 보고서](../hw/lenovo-tb375fc.md) 7절): ufscfg_ao·pdn(클럭·리셋) → `112a0000.ufs-phy`(MT6897 ops) → `112b0000.ufshci`.
    - 기대: 기록에 `caps: 0x7c0`, `clk "main" present` 등 여섯 줄, "Clk-scaling not ready. Feature disabled."가 나온다.
    - "Failed to get resets"가 나오면 리셋 기술이 틀린 것이다. 리셋은 ufscfg_pdn의 set `0x48`·clr `0x4c` bit 0·1·2(UniPro, crypto, HCI)다.
    - "mode operation not allowed"는 MT6319 벅에 `regulator-allowed-modes`가 없어 남는 기록이다. 동작에는 상관없다. 실측으로 절전 전환마다 남는다(약 3분에 18번). 줄이려면 8/vbuck3에 벤더처럼 모드를 허용하는데, VCCQ가 LP 모드를 타므로 쓰기 시험과 함께 본다.
2. [x] RTFF MTCMOS SiP 호출을 본다(2026-11-04. 지나간다). 초기화에서 MTCMOS를 끄고 켠다(벤더와 같은 순서). `caps` 줄 바로 뒤에서 멈추면 이 길이다. 그때는 match data에서 `UFS_MTK_CAP_RTFF_MTCMOS`를 빼 본다(드라이버 한 줄).
3. [x] 호스트 리셋을 본다(2026-11-04. 링크가 선다). HCE enable 때 리셋 셋을 100µs 걸었다 푼다. "Host reset assert/deassert failed"가 없고 링크가 서는지 본다.
4. [x] 먹스 부모를 본다(2026-11-04. LK 값 그대로 `ufs` 약 458 MHz, `aes_ufsfde` 546 MHz. `ufs`의 부모가 화면과 같은 `mmpll`이라 화면을 죽였다. 6.2절의 주의를 본다). `clk_summary`에서 `ufs`·`aes_ufsfde`의 주파수를 읽는다. LK가 26 MHz(부모 0)로 두었으면 느리다. 그때는 dtsi에 `assigned-clocks`로 벤더의 낮은 부모(mainpll_d4_d2, univpll_d6)를 준다. UFS 3.x 장치면 벤더도 낮은 쪽으로 고정한다(`ufs_mtk_fix_clock_scaling`).
5. [x] 기어와 전력 모드를 본다(2026-11-04. HS-G4 Rate B 2레인, UFS 3.1): HS 기어·rate, PMC via FASTAUTO 길, 장치 판(`wspecversion`).
6. [x] 클럭 게이팅과 런타임 PM을 본다(2026-11-04. `SLEEP`, `HIBERN8`과 복귀를 되풀이해 오류가 없고 refclk 기록도 없다. 쓰기와 함께는 아직이다). idle 뒤 hibern8과 ref 클럭 SiP 핸드셰이크("missing ack of refclk req"가 없어야 한다), 런타임 suspend의 MTCMOS 끄기·켜기 뒤 I/O를 본다.
7. [x] UFS가 뜬 뒤 VEMC가 런타임 PM에서 문제없는지 본다(2026-11-04. 절전에 꺼지고 활동에 켜진다. 벤더도 같다).
8. [ ] M-PHY를 본다. 커널은 M-PHY 레지스터를 건드리지 않는다. 높은 기어에서 링크가 흔들리면 벤더(ATF·UniPro 속성)와 견준다.

### 6.7 USB 가젯

mtu3·xsphy, 보드 `dr_mode = "peripheral"`, configfs ACM + NCM이다. 2026-11-03에 맥(HS)에서 직렬 셸과 NCM이 됐다. Windows는 보지 못했다. 업스트림만으로는 뜨지 않아 패치 0141~0143이 필요하다(실측 보고서 7절).

> **위험: USB 끊김.** ssusb를 켤 때 SPMI M 버스와 MT6368(MFD·regulator)이 켜져 있지 않으면 mtu3가 공급을 기다리며 뜨지 않는다. 포트가 하나이고 호스트 모드가 없어 이 길이 화면 말고는 유일하다.
>
> **먼저 둘 것.** 6.3~6.5절의 노드, 화면 콘솔과 pstore.

1. [x] mtu3가 `vusb33`을 받아 뜨는지 본다(2026-11-03). 더미 공급 경고 "supply vusb33 not found"가 없어야 한다.
2. [x] `dr_mode`와 UDC 이름, 실제 열거를 본다(2026-11-03. `11201000.usb`, 맥에서 `configured`). `dummy_udc`와 달리 초기화 순서가 다를 수 있어 서비스가 `buntos.wait`초 기다린다.
3. [x] 호스트에서 직렬 포트에 들어간다(2026-11-03). 호스트의 장치 이름은 macOS `/dev/cu.usbmodem*`, Linux `/dev/ttyACM0`이다. 부팅 때 프리로더도 약 3초 동안 같은 꼴의 포트를 내니 거기에 쓰지 않는다. USB 제품 문자열로 어느 쪽 가젯인지 안다(`TB375FC initramfs`, `TB375FC`).

    ```bash
    screen /dev/ttyACM0 115200
    ```

4. [x] 네트워크를 본다(2026-11-03. 맥의 `en9`가 172.16.42.2를 받고 ping과 telnet이 된다). 기기는 172.16.42.1/24, 호스트는 DHCP로 172.16.42.2를 받는다. initramfs에서는 `buntos.telnet=1`이면 telnetd가 뜬다. 옵션은 [init](../../bsp/lenovo-tb375fc/initramfs/init) 머리말에 있다.
5. [ ] 속도를 본다. USB3는 PS5170 리드라이버가 있어야 해 그 드라이버 전에는 HS로만 붙는다(설계안 4절). SS가 흔들리면 벤더가 MT6897에만 넣는 U3 RX CDR 값부터 본다(U3 포트 `+0x408`의 [23:16] `RG_XTP_LN0_RX_CDR_RESERVE` = 0x8).
6. [ ] U2 INTR_CAL을 본다(2026-11-03. LK 값과 efuse 값이 하나 다르다. HS 열거는 된다. 신호 품질을 볼 때 다시 본다). `0x11e40330`의 [29:24]를 읽어 LK가 둔 값을 적고, 4절 9번의 efuse 값과 견준다. 다르면 xsphy MT6897 꼴(칸과 nvmem)을 만든다.
7. [x] eye 값을 본다(2026-11-03. 맞다). `0x11e40304`의 [14:12] = 7, [10:8] = 3인지 본다.

### 6.8 루트 파티션

자리는 userdata를 줄여 만든 새 GPT 파티션 `buntos`다. 형식 GUID는 root-arm64(`b921b045-1df0-41c3-af44-4c6f280d3fae`)이고 명령줄은 `root=PARTLABEL=buntos`다. USB-C 저장장치 길은 쓰지 않는다. 2026-11-04에 작은 Debian 이미지로 2b 종료 조건을 지났다. 전체 이미지(`tools/image/build.sh lenovo-tb375fc`)를 기기에 올린 기록은 아직 없다. 값은 [실측 보고서](../hw/lenovo-tb375fc.md) 7절에 있다.

이미지의 systemd가 UFS의 GPT를 고치지 못하게 `systemd-repart`와 `systemd-gpt-auto-generator`를 보드 프로필과 initramfs에서 가렸다. 이 가림을 풀지 않는다. QEMU에서 GPT와 다른 파티션의 해시로 확인했다.

1. [x] 가젯 셸에서 UFS를 읽기 전용으로 확인하고 파티션 표를 읽는다(2026-11-04. 표는 `sdc`에만 있고 `PARTNAME` 82개가 나온다). 커널이 MediaTek GPT를 읽어 `PARTNAME`이 나오는지 본다. LU 여럿에서 같은 이름이 없는지 본다. 지금은 먼저 찾은 것을 쓰고 겹침을 따로 알리지 않는다.
2. [x] 크기와 이름을 정한다(4절 7번). `buntos` 80 GiB, `userdata` 28.2 GiB(2026-11-04).
3. [x] 고치기 전의 GPT(앞·뒤 표)를 떠서 기기 밖에 둔다(2026-11-04. 맥과 SSD).
4. [x] GPT를 고친다(2026-11-04. 가젯 셸에서 `dd`, 뒤 표 → 앞 항목 → 앞 헤더. 항목 82개로 꽉 차 있어 83개로 늘렸다).

    > **위험: 부팅 불가·데이터 손실.** preloader·LK가 고친 GPT를 읽지 못하면 부팅이 막힌다. userdata를 한 번 지운다(공장 초기화). 안드로이드 슬롯은 남는다.
    >
    > **먼저 둘 것.** 3번의 GPT, 2.3절 백업. 언락 직후라 잃을 데이터가 적을 때 한다.
    >
    > **되돌리기.** 언락 전에 뜬 SP Flash 백업이다. 고친 GPT는 SP Flash Tool scatter와 달라지므로 고친 GPT도 떠 둔다.

5. [x] preloader·LK가 고친 GPT를 어떻게 읽는지, LK의 fastboot가 새 항목을 아는지 본다(2026-11-04). 프리로더와 LK는 새 표로 부팅하고, `_a` 안드로이드도 뜬다. fastboot의 `getvar partition-size:userdata`는 새 크기(`0x70d700000`)를 내지만 `buntos`는 값이 비어 있다. 새 항목에 fastboot로 굽는 것은 해 보지 않았고, 가젯 셸의 `dd`로 굽는다.
6. [x] 루트 파티션 파일(`buntos.root-arm64.raw`)을 굽는다(2026-11-04. 작은 이미지 861 MB를 가젯 네트워크로 받아 `dd`, 23초). fastboot로 굽고(sparse. `max-download-size`가 128MiB라(실측) `fastboot`가 나눠 보낸다), 안 되면 가젯으로 `dd`한다. 이미지의 ESP와 UKI는 LK가 쓰지 않으므로, 이미지에서는 이 파일만 굽는다.

    > **위험: 데이터 손실.** `dd`의 대상은 새 파티션뿐이다. userdata 전체, super, 루프 파일은 쓰지 않는다. 안드로이드 슬롯을 지키지 못하거나 metadata 암호화로 열 수 없다.

7. [ ] 명령줄을 넣어 부트 이미지를 다시 만든다. `<이름>`은 2번에서 정한 값이다(`PARTUUID=`, `UUID=`, `/dev/…`도 받는다). 공개 키는 ed25519를 쓴다. RSA 4096은 명령줄 한도에 빠듯하다. 값의 뜻은 [BSP README](../../bsp/lenovo-tb375fc/README.md)의 "루트로 넘기기"에 있다.

    ```bash
    limactl shell --workdir "$PWD" buntos-dev -- bash -lc 'x=$(mktemp -d) && dpkg-deb -x ~/buntos-build/tb375fc/deb/linux-image-7.2.12-tb375fc_*_arm64.deb $x && bsp/lenovo-tb375fc/initramfs/mkinitramfs.sh --modules $x && bsp/lenovo-tb375fc/boot/mkimages.sh --kernel $x/boot/vmlinuz-7.2.12-tb375fc --dtb $x/usr/lib/linux-image-7.2.12-tb375fc/mediatek/mt6897-lenovo-tb375fc.dtb --initramfs ~/buntos-build/tb375fc/images/initramfs.cpio.lz4 --cmdline "console=ttyS0,921600n8 console=tty0 earlycon clk_ignore_unused pd_ignore_unused regulator_ignore_unused root=PARTLABEL=<이름> rootfstype=btrfs rw systemd.firstboot=no buntos.usbdebug=1 systemd.set_credential_binary=ssh.authorized_keys.root:$(base64 -w0 <공개 키 파일>)"'
    ```

8. [ ] LK가 `root=`와 `buntos.usbdebug=1`을 커널에 넘기는지 본다(시험 커널은 트리의 명령줄을 쓰므로 LK 길에서 본다). `/proc/cmdline`이 `/run/buntos/boot.log`에 남는다. LK가 자기 `root=`나 `ro`를 덧붙여도 우리 값이 이긴다.

    > **위험: USB 끊김.** 진짜 루트의 USB 길은 명령줄에 `buntos.usbdebug=1`이 있을 때만 돈다. 없으면 넘어간 뒤 가젯이 없다.
    >
    > **먼저 둘 것.** LK가 이 값을 넘기는지 `/proc/cmdline`으로 본다. 안 넘어오면 커널 `CMDLINE`(+`CMDLINE_EXTEND`)로 주고 `buntos.usbdebug=1`을 함께 넣는다. 그때 공개 키는 커널 설정에 넣지 말고 ttyACM 셸에서 넣는다.

9. [ ] 못 넘어갔으면 까닭을 본다. 패닉 없이 셸이 뜨고, 까닭은 화면, dmesg, pstore, `/run/buntos/boot.log`에 남는다. 찾지 못했으면 `/run/buntos/block.txt`에 장치와 GPT 이름 목록이 있다. UFS가 뜨는 시간도 본다(`buntos.wait` 기본 30초면 되는가).
10. [x] 들어간다(2026-11-04. 맥에서 SSH, 시험 커널). 넘어가는 동안 USB가 잠깐 없다. 가젯은 진짜 루트에서 새로 띄운다.

    ```bash
    ssh root@172.16.42.1
    ```

    공개 키가 아직 없거나 네트워크가 안 되면 ttyACM의 암호 없는 root 셸을 쓴다. 셋 다 다른 서비스를 기다리지 않아, sysinit이 멈추거나 emergency.target으로 넘어가도 들어갈 수 있다.
11. [x] 실제 UFS의 btrfs를 본다(2026-11-04. `discard=async`가 켜지고, 2 GiB를 439 MB/s로 쓰고 재부팅 뒤에도 해시가 같다. 안드로이드 쪽 영향은 아래 안드로이드 부팅에서 본다). `discard=async`가 저절로 켜진다(QEMU의 마운트 옵션에서 봤다). UFS의 UNMAP과 안드로이드 쪽 영향은 파티션 안이라 없을 것으로 보지만 기기에서 본다.
12. [ ] 넘어간 뒤의 화면을 본다. simpledrm의 fbcon이 systemd, greetd를 지나도 사는지 본다.
13. [ ] 워치독을 본다. `mtk_wdt`는 내장이고 넘어가도 커널이 먹인다(`WATCHDOG_HANDLE_BOOT_ENABLED=y`). systemd의 `RuntimeWatchdogSec`을 켜지 않아도 오래 사는지 본다.
14. [ ] 안드로이드 파티션이 70개쯤일 때 `systemd-udevd`와 `blkid`가 걸리는 시간을 본다. UFS를 가리는 규칙(`90-buntos-tb375fc-hide-ufs.rules`)은 부모 사슬만 보고 프로그램을 돌리지 않는다(시간은 재지 않았다).
15. [x] 실제 크기의 btrfs 늘리기와 첫 부팅 시간, `systemd-firstboot`(`systemd.firstboot=no`)를 본다(2026-11-04. 80 GiB까지 늘어나고 첫 부팅이 3.5초다). 늘리기는 `systemd-growfs-root.service`이고 파일 시스템만 늘린다.
16. [ ] pstore를 본다. ramoops 자리를 정한 뒤 `/run/buntos/pstore`와 `systemd-pstore`(`/var/lib/systemd/pstore`)에 남는지 본다.
17. [x] 가상화 판별이 `none`인지 본다(2026-11-04). `buntos-vm-sleep.service`가 `ConditionVirtualization=vm`이라 기기에서는 돌지 않아야 한다.

    ```bash
    systemd-detect-virt
    ```

### 6.9 2b 뒤에 남는 일

기기에서 UFS·USB가 뜬 뒤에 한다.

- [x] 재시도 다루기(2026-11-04). LK가 `_b`의 재시도를 부팅마다 줄이고 `successful_boot`를 세우는 쪽이 없어, 잘 도는 시스템도 7번 부팅하면 `_a`로 넘어간다. `buntos-slot-keep`이 부팅 뒤 `misc`(0x800)의 재시도를 7로 되돌린다. `successful_boot`는 세우지 않아, 뜨지 않는 커널은 그대로 `_a`로 돌아간다. 안드로이드로 일부러 가려면 이 서비스를 끄고 재부팅한다(`.handoff`의 `to-a.sh`). 처음 판은 경로 조건을 써서 부팅 때 늘 건너뛰었고, 재시도를 다 써 안드로이드로 넘어갔다. 지금은 장치 유닛을 기다린다.
- 시험 층(`test-stage.dtsi`)의 노드를 보드 dts로 옮긴다. 이때 `framebuffer0`의 `clocks`도 함께 옮긴다. 빠지면 UFS가 화면을 죽인다(6.2절).
- UFS 클럭 스케일링(먹스 부모, 게이트의 `CLK_SET_RATE_PARENT`).
- MCQ. 공통 바인딩의 인터럽트 수를 고치고, 큐 인터럽트 SPI 177~184를 넣고, `mediatek,ufs-disable-mcq`를 뺀다. 이 속성을 뺀 시험은 그 뒤에 한다.
- xsphy MT6897 꼴(U2 INTR_CAL·efuse, U3 RX CDR), mtu3 깨우기 접점(벤더 판 103).
- U-Boot 체인로드로 EFI(systemd-boot, UKI) 부팅을 검토한다(설계안 D3).
- EFI 길이 열리거나 LK의 벤더 램디스크 한도를 기기에서 재면 mkosi initrd를 vendor_boot에 넣는 길을 다시 볼 수 있다. 지금은 우리 initramfs다.
- 기기에서 커널을 바꾸는 길은 남은 결정이다(LK 부트 이미지를 다시 만들어 쓴다).

### 6.10 MT6685 RTC

패치 0110~0113이고, 남은 알람은 PMIF 고침 0114와 함께 본다. 코드와 레지스터 모델로 확인했을 뿐 기기에서 돌려 보지 않았다. 모델도 벤더 자료로 만든 것이다. 2b의 USB 가젯 셸이 뜬 뒤에 한다. 안드로이드와 LK가 같은 칩을 쓰므로 읽기부터 한다.

**넘어가기 전(안드로이드, adb, 잠긴 채로 된다):**

1. [ ] 11번을 볼 때까지는 안드로이드의 전원 꺼짐 알람을 끄고 넘어간다. 알람 시계 앱의 알람을 모두 끄고 재부팅한다. 11번에서 이벤트가 한 번 지워지고 끝나면 이 줄은 뺀다. 사용자 기기는 2026-11-02에 시계 알람이 없었다(`dumpsys alarm`의 `Next alarm clock information`이 비었다). 안드로이드를 떠나기 직전에 다시 본다.

    - 까닭: 벤더 드라이버는 전원 꺼짐 알람이 있으면 종료 때 `IRQ_EN.AL`을 켠 채 둔다. 그 시각이 리눅스에서 오면 칩이 SID 9로 RCS를 낼 수 있다. 패치 0114가 처리기 없는 SID의 이벤트를 지우므로, 이벤트가 한 번이면 EINT 248은 계속 울리지 않는다.
    - 0114가 막지 못하는 경우: 칩이 원인(`SCK_TOP_INT_STATUS0`, `RTC_IRQ_STA`)이 지워질 때까지 RCS를 곧바로 다시 내면 처리기가 계속 돈다. 리눅스는 그 칸을 건드리지 않는다. 칩이 어느 쪽인지는 코드로 알 수 없다(벤더 코드에는 MT6685에 RCS 완료를 쓰는 곳이 없어 MT63xx의 뜻을 옮길 수 없다).

2. [ ] 안드로이드에서 시간을 읽어 둔다(막히면 건너뛴다. 까닭: 레지스터가 UTC인지, 리눅스가 같은 값을 읽을지 맞대려는 것이다).

    ```bash
    adb shell 'date -u; cat /sys/class/rtc/rtc0/date /sys/class/rtc/rtc0/time /sys/class/rtc/rtc0/since_epoch'
    ```

    사용자 기기의 셸은 `/sys/class/rtc`에서 막혔다(실측, 2026-11-02). 4절 10번에서 root로 읽는다.

**리눅스:**

3. [x] `pmic@9`만 `okay`로 바꾼다(`rtc@580`은 그대로). 이 드라이버 때문에 버스에 닿는 것이 없다(MFD는 자식만 띄운다).

    ```bash
    ls -l /sys/bus/spmi/devices/0-09/driver    # mtk-spmi-pmic
    ls /sys/class/rtc                          # 비어 있다
    ```

4. [x] (2026-11-04: 맞는 시각, 연도 정상) `rtc@580`도 `okay`로 바꾼다. 부팅 로그를 본다.

    ```bash
    dmesg | grep -i -E 'rtc|mt6685'
    cat /sys/class/rtc/rtc0/name /sys/class/rtc/rtc0/date /sys/class/rtc/rtc0/time /sys/class/rtc/rtc0/hctosys
    ```

    - `registered as rtc0`, 그리고 안드로이드에서 넘어온 뒤 날짜와 시각이 맞아야 한다(1~2번의 값). 처음부터 칩이 깨끗하면 2010년 근처일 수 있다.
    - **연도가 2086이나 2094면** 연도 칸의 해석이 틀린 것이다. 여기서 멈추고 `hwclock -w`를 하지 않는다. 안드로이드로 돌아가면 그 값을 읽는다.
    - 칩이 연도 칸 0·1(1968·1969)이나 틀린 날짜를 담고 있으면 `-EINVAL`이고 시스템 시간을 건드리지 않는다.

5. [x] (2026-11-04: 3초에 3씩 는다) 시간이 흐르는지 본다. **이것이 드라이버의 가장 큰 가정이다.** 레지스터가 클럭 게이트를 잠깐 여는 것만으로 카운터를 따라온다고 본다(OnePlus 커널과 2025년 뒤 MediaTek 판). 이 기기의 스톡 모듈은 더 옛 판이라 BBPU `RELOAD`를 쓴다.

    ```bash
    for i in 1 2 3; do cat /sys/class/rtc/rtc0/time /sys/class/rtc/rtc0/since_epoch; sleep 3; done
    ```

    - `since_epoch`가 3씩 늘어야 한다. 멈춰 있어도 드라이버는 알지 못하고 성공을 돌려준다. hctosys도 그 값을 넣는다.
    - 안 흐르면 RELOAD 판이 필요한 것이다. `BBPU_L`에 `(읽은 값 & 0x0f) | 0x20`, `BBPU_H`에 `0x43`, `WRTGR`에 `1`을 게이트 사이에 넣는다(스톡 차례). 지금은 regmap이 막으므로 표도 함께 고친다. 이 쓰기는 PWREN·AUTO를 그대로 두고 명령 비트(CLRPKY 등)를 넣지 않는다.

6. [ ] 쓴다.

    ```bash
    date -u -s "$(date -u '+%Y-%m-%d %H:%M:%S')"   # 또는 NTP로 맞춘 뒤
    hwclock -w -u; echo $?
    hwclock -r -u
    dmesg | grep 'did not take the time'
    cat /sys/kernel/debug/regmap/0-09-rtc/registers
    ```

    - `did not take the time`이면 `-EIO`다. 부트로더가 RTC를 잠갔거나 되읽기가 2 ms로 모자란 것이다. 벤더는 이 검사에 실패해도 경고만 하고 성공을 돌려주므로, 시간은 들어갔는데 오류가 날 수도 있다. `hwclock -r -u`로 값을 다시 본다. 벤더 커널은 잠금을 풀지 않으므로 잠금일 가능성은 낮다.
    - `-ETIMEDOUT`이면 CBUSY(`BBPU` bit 6)가 내려오지 않은 것이다.
    - debugfs 파일은 열 칸만 읽는다: `0x008` 칩 ID(벤더의 `HWID_MT6685`), `0x514` 게이트(끝나면 bit 4·5가 1), `0x588` BBPU(끝나면 `CBUSY` 없이 부팅 값 그대로), 시간 칸 여섯, `0x5d8`의 bit 7:6 잠금 상태(`SPAR_PROT_STAT`).

7. [ ] NTP 뒤에 11분 동기화를 본다(`CONFIG_RTC_SYSTOHC`). `dmesg`에 오류가 없고 `hwclock -r -u`가 맞으면 된다. 되읽기가 실패하면 커널이 1~2초마다 다시 쓴다(로그는 제한된다). 그러면 6번으로 돌아간다.

8. [ ] 재부팅을 본다.

    - 리눅스 → LK → 리눅스: 시간이 흐른 만큼 맞다.
    - 리눅스 → 안드로이드: `date -u`가 맞다. 안드로이드 알람이 울린다. 부팅 사유가 이상하지 않다. LK가 RTC를 다시 초기화하지 않는다(날짜가 2010 근처로 돌아가지 않는다).
    - 안드로이드 → 리눅스: 다시 맞다.

9. [ ] `poweroff`를 본다. **드라이버가 벤더의 종료 쓰기를 하지 않는다.** 벤더는 종료마다 BBPU의 PWREN을 내리고(`0x588`) `TOP2_ELR1`(`0x12f`) bit 0을 올린다("Normal sequence power off when PON falling"). LK의 종료 콜백도 `0x129`에 1을 쓰고 `0x12f` bit 0을 올린다(`RG_SEQ_OFF`).

    - 정말 꺼지는가(전류가 떨어지는가, 전원 키로 다시 켜지는가). 다시 켠 뒤 LK가 이상한 부팅 사유를 말하지 않는가.
    - 필요하면 그때 RTC나 MFD의 `.shutdown`에 넣는다. 지금은 넣지 않았다(이 칸은 시간과 알람이 아니다).

10. [ ] 인터럽트가 가만히 있는지 본다.

    ```bash
    grep -E 'Err:|mtk-spmi-pmic' /proc/interrupts
    dmesg | grep 'cleared event'
    ```

    - `spmi_rcs`와 EINT 248 줄은 찾지 않는다: 7.2.12는 체인 처리기가 붙은 인터럽트를 `/proc/interrupts`에 보여 주지 않는다.
    - `Err:`가 계속 늘면 안 된다(부모가 돌았는데 이벤트가 없는 경우를 센다). 지운 쓰기보다 EINT unmask가 먼저 닿으면 이벤트마다 하나쯤 늘 수 있고, 그것은 괜찮다. 계속 늘면 P 버스 칸을 의심한다(6.3절 5번).
    - `cleared event` 줄이 없어야 한다. 있으면 그 SID가 처리기 없이 RCS를 낸 것이고 줄에 SID와 값이 있다.

11. [ ] 맨 나중에, 위험: 남은 알람의 이벤트가 어떻게 되는지 본다. 먼저 둘 것: 시리얼이나 pstore, 저장하지 않은 일이 없을 것. 되돌리는 길: 전원 키를 길게 눌러 리셋.

    - 안드로이드 알람 시계에 10분 뒤 알람을 걸고 재부팅해 리눅스로 넘어간 뒤 그 시각을 넘긴다.

    ```bash
    dmesg | grep -E 'cleared event|callbacks suppressed'
    evtest    # mtk-pmic-keys: 전원 키의 누름과 뗌
    ```

    - `Err:`와 `/proc/interrupts`로는 판단하지 않는다. 지우는 길은 어느 수에도 잡히지 않는다. 이 로그가 유일한 증거다.
    - **한 번 나오고 멎는다**(`cleared event 0x.. of SID 9: not enabled`): 0114가 막은 것이다. 셸이 살아 있고 전원 키가 듣는지 본다. 그러면 1번의 줄을 뺀다.
    - **계속 나온다**(5초에 10줄까지, 그 뒤 `callbacks suppressed` 수가 는다. 느려지거나 멈출 수 있다): 칩이 RCS를 다시 내는 것이다. 0114로 모자란다. 1번의 줄을 지키고, 12번에서 칩 쪽 원인을 지우는 처리를 먼저 넣는다.
    - **아무 줄도 없다:** `SCK_TOP_INT_CON0`이 재부팅을 넘어 남지 않거나 LK가 지우는 것이다. 위험이 없으니 1번의 줄을 뺀다.

12. [ ] 알람은 이 뒤에 정한다. 인터럽트 길을 기기에서 본 다음이다.

**첫 소비자 메모:** 오디오(단계 3)의 `mt6368` 코덱이 BBCK5(`DCXO_EXTBUF5_CW0` `0x79e`)를 켠다. 벤더의 `mt6685_set_dcxo()` 대신 코덱이 `mediatek,dcxo`로 MT6685에 직접 쓴다(패치 0162).

**connsys 메모:** MFD가 칩을 읽지 않으므로 RTC를 켜지 않았다면 connsys의 DCXO 읽기가 이 칩의 첫 접근이다(0055 뒤로 늘 그랬다). `mt6363-regulator` 주석은 부트로더가 잠재운 PMIC의 첫 읽기가 실패할 수 있다고 적었다. 벤더의 MT6685 코드에는 그런 처리가 없다. `can't read the DCXO`가 첫 번에만 나오면 이것이다.

## 7. 2c 입력

**목표.** simple-framebuffer로 buntOS 데스크톱을 띄우고, NT36532 SPI 터치, 펜, 홀 센서, 포고 키보드를 쓴다.

**종료 조건.** 2944×1840으로 buntOS 로그인 화면, 터치·펜 조작, 키보드 커버를 붙이면 포인터 모드.

### 7.1 준비

1. [ ] 데스크톱 전에 안드로이드 파티션이 가려졌는지 본다. 전체 이미지에는 `udisks2`가 들어온다(`nautilus` → `gvfs`). 규칙은 `90-buntos-tb375fc-hide-ufs.rules`(이미지의 `/usr/lib/udev/rules.d/`)이고, 파티션 이름이 아니라 UFS 호스트 아래의 모든 블록 장치에 `UDISKS_IGNORE=1`, `UDISKS_SYSTEM=1`, `UDISKS_AUTO=0`을 준다. QEMU에서는 시험했고(`bsp/lenovo-tb375fc/udev/test.sh --root …`) 기기에서만 볼 수 있는 것이 아래다.

    > **위험: 데이터 손실.** 내부 디스크는 자동으로 붙지 않지만 파일 관리자에 보이고, 관리자 인증 뒤 쓰기 가능으로 붙을 수 있다. `nvdata`, `persist`는 ext4라 저널을 되살리며 바뀐다.
    >
    > **먼저 둘 것.** 데스크톱(`greetd`)을 켜기 전에 아래 a~d를 본다. 명령줄에 `systemd.unit=multi-user.target`을 주면 데스크톱 없이 올라온다. 숨김은 막는 것이 아니다. 기기의 안드로이드 파티션은 `ro`로도 붙이지 않는다(ext4는 블록 장치가 쓰기 가능이면 읽기 전용 마운트에서도 저널을 되살린다). 붙여 보는 시험은 QEMU에서 했다.

    - a. 규칙이 기대는 두 값을 본다. 하나라도 맞으면 걸리지만, 둘 다 없으면 규칙이 빗나간다.

      ```bash
      udevadm info -a -n /dev/sda | grep -E 'DRIVERS|KERNELS' | head -n 12
      ```

      기대: `DRIVERS=="ufshcd-mtk"`와 `KERNELS=="112b0000.ufshci"`(부모 사슬의 위쪽). 순정 안드로이드의 부팅 장치도 `112b0000.ufshci`다(실측 보고서).
    - b. UFS의 모든 LU(`sda`, `sdb` …)와 파티션에 값이 붙었는지 본다. `fastboot getvar all`의 파티션 목록과 개수가 맞는지도 본다.

      ```bash
      for d in /sys/class/block/sd*; do n=${d##*/}; echo "$n $(udevadm info -q property -n /dev/$n | grep -E '^UDISKS_(IGNORE|SYSTEM|AUTO)=' | tr '\n' ' ')"; done
      ```

      기대: 줄마다 `UDISKS_AUTO=0 UDISKS_IGNORE=1 UDISKS_SYSTEM=1`. 비어 있는 줄이 있으면 그 장치의 `udevadm info -a`를 읽는다.
    - c. LU가 이동식으로 보이는지 본다: `cat /sys/block/sd*/removable`. 1이어도 규칙의 `UDISKS_SYSTEM`·`UDISKS_AUTO`가 인증과 자동 붙임을 막는다. 값만 적어 둔다.
    - d. udisks2가 속성을 받았는지 본다: `udisksctl info -b /dev/sda3 | grep -E 'Hint(Ignore|System|Auto)'` → `HintAuto: false`, `HintIgnore: true`, `HintSystem: true`.
    - e. 데스크톱에서 파일 관리자(Nautilus)의 사이드바에 UFS 파티션이 하나도 없고, 파일 선택 창(앱의 열기·저장)의 목록에도 없는지 본다. 규칙을 가린 대조는 기기에서 하지 않는다(안드로이드 파티션이 목록에 나타난다. QEMU 시험이 이 대조를 한다). 주소 표시줄에 `computer:///`를 치면 볼륨 없는 드라이브가 남는 것이 정상이다.
    - f. USB 메모리는 그대로 보여야 한다. USB 호스트가 되는 8절 6단계 뒤에 꽂아 사이드바에 나타나고 붙는지 본다. `udevadm info -q property -n /dev/sdX1 | grep UDISKS_`는 비어 있어야 한다.
    - g. 숨김일 뿐 막지 않는다는 것은 소스와 QEMU에서 확인했으므로 기기에서 붙여 보지 않는다.

2. [x] 터치 펌웨어를 넣는다. 드라이버는 터치를 열 때 루트에서 읽는다(`TOUCHSCREEN_NOVATEK_NVT_SPI=m`).
    - `vendor/firmware/novatek_ts_fw_boe.bin` → `/lib/firmware/lenovo/tb375fc/novatek_ts_fw_boe.bin`
    - `vendor/firmware/novatek_ts_fw_tm.bin` → `/lib/firmware/lenovo/tb375fc/novatek_ts_fw_tm.bin`
3. [x] 노드를 켠다. pio·클럭 컨트롤러에 더해 `spi2`, `gpio-keys`, `vddio_1v8`, `uart3`이다. `vddio_1v8`(GPIO63)은 패널과 터치가 같이 쓰는 I/O 전원이라 simple-framebuffer 동안 꺼지면 안 된다. 그래서 `regulator-always-on`이다.
4. [ ] 패널 노드와 `framebuffer0`의 `panel`이 트리에 있다(꺼 둠). simpledrm 커넥터의 크기(`modetest` 또는 `/sys/class/drm`)가 273×171mm인지, 기본 배율이 225%인지 본다.
5. [ ] 전체 이미지에서 `greetd`·`bunt-comp`와의 관계, 공통 설치 뒤 스크립트와의 순서(보드 것이 먼저 돈다)를 본다. 작은 시험 이미지에는 데스크톱 꾸러미가 없어 보지 못했다.

### 7.2 터치

패치 0027~0031이다. 프로토콜은 OEM 코드, 패널 판별과 펌웨어 이름은 제3자 미러, 좌표 범위는 실측에서 왔다. 2026-11-04에 1~4번을 기기에서 확인했다. 서스펜드와 복귀(5번)는 아직이다.

1. [x] spi2 probe를 본다. `/sys/kernel/debug/clk`에서 spi2 먹스의 부모와 값을 읽는다(182 MHz). 컨트롤러 자체 CS로는 0만 돌아와 보드 dts를 `cs-gpios`로 바꿨다.
    - SCLK는 9.6 MHz 아래여야 한다. 부모 182 MHz면 9.1 MHz, 208 MHz면 9.45 MHz다(코드로 본 것).
    - 전송이 깨지면 `cs-gpios = <&pio 12 GPIO_ACTIVE_LOW>`(GPIO12를 GPIO로)를 쓴다. OEM의 소프트웨어 CS를 업스트림이 쓰지 않는다. 이 드라이버는 전송 하나짜리 메시지만 써서 차이가 작을 것으로 보지만 기기에서 본다.
2. [x] 터치 probe를 본다(Tianma, `_tm` 펌웨어). `dyndbg`로 `novatek_nvt_spi`를 켠다.
    - trim ID: `xx xx xx 32 65 03`.
    - 패널 ID(GPIO184): 0이면 BOE(`novatek_ts_fw_boe.bin`), 1이면 Tianma와 Tianma 무광(`novatek_ts_fw_tm.bin`). LK 기록의 패널과 맞는지 본다. 패널 드라이버도 이 줄로 고른다.
3. [x] 열 때를 본다(`evtest`). 펌웨어는 터치를 열 때 내려받는다(패널이 꺼져 있으면 켜진 뒤에, 패치 0183). 리셋 상태 0xA0·0xA1, 펌웨어 판, 손가락 좌표를 본다. 컨트롤러 좌표는 X 0~18400, Y 0~29440이고 픽셀의 10배다(실측).
4. [x] 축을 정한다. 결과는 `touchscreen-inverted-x`다(위아래가 반대였다). 화면에 점을 찍는 시험 도구로 사용자가 확인했다. `touchscreen-swapped-x-y`는 보드에 있다.
5. [x] 서스펜드·복귀 뒤 터치를 본다(2026-11-05, 잠금 화면에 터치로 암호를 넣었다. 패널 팔로워가 펌웨어를 다시 올린다). 시간은 잠금 화면 시계로만 봤다.
6. [ ] 펌웨어 워치독 복구를 본다. 드물다. 일어나면 기록에 "controller was reset"이 남는다.

### 7.3 펜

1. [ ] 압력, 기울기, 거리(호버), 단추 둘을 본다. 범위는 압력 0~4095, 기울기 ±60°, 거리 0~1이다(실측).
2. [ ] 나오지 않으면 펜 켜기 인자(`0x7B 0x01`)부터 본다. 이 기기 드라이버(제3자 미러)는 `7B 01`이 켜기이고, TB371FC 커뮤니티 포트의 펌웨어에서는 `7B 01`이 끄기다. 명령이 실패해도 드라이버는 경고만 한다.

### 7.4 홀 센서

1. [ ] 덮개를 닫고 열 때 `SW_LID`를 본다(`evtest`). GPIO1이 0이면 덮개 닫힘이다. 보드 노드는 `gpio-keys`의 `switch-lid`다(active low, 100 ms, `wakeup-source`).
2. [ ] 펌웨어의 `hall_2`(GPIO26)는 키보드의 붙음·뗌 줄이다. `gpio-keys`에 없고 키보드 드라이버가 가진다. 7.5절 2번에서 본다.

### 7.5 포고 키보드

패치 0032~0036이다. 프로토콜은 글로벌판(TB373FU) 스톡 모듈을 읽은 것이고, 사용자 기기는 중국판 ZUXOS다. 키보드 MCU는 액세서리 쪽이라 프로토콜이 같을 것으로 본다(판단). 실물 확인은 이 절이다. 드라이버는 사용자 공간 시뮬레이터로 돌렸고 기기 시험은 없다. 3번이 프로토콜 판단을 가르는 첫 확인이다.

1. [x] (2026-11-04: `0x10`) UART3 패드 모드(`mediatek,syscon-pad-mode`, pericfg_ao `0x50`)를 본다. 켠 뒤 `0x11036050`의 값은 bit 4 = 1, bit 0 = 0이어야 한다. 스톡 로그의 "first get uart3_def_value"처럼 바뀌기 전 값(리셋 값)도 본다. 이 비트 없이는 UART3이 패드로 나가지 않을 수 있어 필요하다고 판단했다.
2. [ ] GPIO26이 키보드를 붙이면 0, 떼면 1인지 본다. GPIO61(전원)이 따라가는지 본다.
3. [ ] 처음 프레임을 본다. `dyndbg`로 `oneplus_pogo_kbd`를 켠다.
    - 기대: 전원 뒤 첫 프레임이 0x7B로 풀리고 "bad sum"이 없다. 그러면 프로토콜 판단이 맞다.
    - 다르면: "bad sum"·"unknown symbol"과 원시 바이트(`pogo rx:`)로 본다. 전 판과 다르면 여기서 보인다.
4. [ ] `evtest`로 키, 미디어 키, 터치패드(손가락 넷, 2944×1840), Caps Lock LED(0x6A/0x27)를 본다. 터치패드 해상도는 실측으로 정한다. 실제 크기(mm)를 모른다. libinput은 해상도 없이도 돌지만 가속이 어긋날 수 있다.
5. [ ] 키보드를 뒤로 접을 때 0x93/0x03 프레임과 `SW_TABLET_MODE`를 본다.
    - 이 프레임을 뒤로 접음으로 본 것은 판단이다. 자료로는 "키보드가 스스로 꺼짐"까지만 확인된다.
    - 드라이버는 값 0이면 1, 값 1이면 0을 보낸다. 키보드가 없을 때도 1이다.
    - libinput이 이 스위치로 키보드·터치패드를 멈추려면 BUS_HOST 키보드가 내장으로 잡혀야 한다. 기기에서 본다.
6. [ ] 921600에서 PIO로 넘침이 있는지 본다. 스톡은 APDMA를 쓴다. 넘치면 APDMA 노드(`mediatek,mt6985-uart-dma`, 펌웨어 `0x11300b80`)를 더한다.

    ```bash
    cat /proc/tty/driver/serial
    ```

    `oe` 칸을 본다.
7. [ ] 서스펜드를 본다: 0x92/0x02 응답, 키로 깨우기(GPIO40), 붙이고 떼어 깨우기(GPIO26). 응답의 상태 바이트가 1이 아니면 드라이버는 실패로 본다. MT6897에서는 꺼 둔 동안의 에지가 있어도 키로 깨울 수 있다고 읽었다(코드로 본 것). 이 확인으로 본다.
8. [ ] TX 게이트와 응답 지연을 잰다. `dyndbg`로 `oneplus_pogo_kbd`를 켜면 `pogo_xfer`가 `gate shut after N us, reply after M us`를 찍는다. 로직 분석기가 있으면 GPIO71과 선을 함께 본다.
    - 닫힘 지연: N에서 프레임 시간(25바이트면 271 µs)을 뺀다. 부하를 걸고도 본다.
    - MCU 응답 지연: M에서 응답 프레임 시간(31바이트면 340 µs, 0x92 응답)을 뺀다.
    - 모델 값: 닫힘은 프레임 끝부터 52~158 µs다. RX 문턱이 16이면(ST16650V2로 잡힐 때. 부트 로그의 `is a …`로 본다) 104~217 µs다. 실측이 아니다.
    - "no reply"나 "bad sum"이 응답에서 나면: 응답이 약 100~150 µs(모델 값)보다 빠른 키보드다. RX 콜백에서 게이트를 바로 닫는다. 그래도 모자라면 8250 쪽 콜백이 필요하고, 이것은 업스트림 꼴을 다시 정해야 한다.
9. [ ] 잠든 사이 펼치기와 접기를 본다. 깬 뒤 SLEEP 0 다음에 MCU가 0x93/0x03을 다시 보내는지 본다. 보내지 않으면 접은 것을 깬 뒤 알 수 없다. 잠든 사이 펼친 것은 키나 손가락이 눌리면 `SW_TABLET_MODE`를 0으로 되돌린다.

## 8. USB 호스트

**목표.** 태블릿이 USB 메모리에 5V를 주고 xHCI로 붙인다. VBUS는 MT6375 OTG 부스트 → 차지 펌프 입력 FET → 커넥터로 나간다. TCPC 전까지 사용자 공간이 `regulator-output`의 `state`로 켠다. 드라이버는 패치 0042~0049이고 노드는 모두 꺼져 있다.

**종료 조건.** 소스에 따로 적힌 것이 없어 6·7단계의 기대를 옮긴다. USB 메모리가 HS로 붙고, 끈 뒤 VBUS가 0V로 내려가고, 충전과 가젯이 전처럼 된다.

**때.** UFS 루트와 USB 가젯이 기기에서 된 뒤에만 한다. 2d의 USB 동글 우회도 이것을 기다린다.

> **위험: USB 끊김.** 포트가 하나라서 VBUS를 켜 있는 동안 가젯 셸은 없다. 그동안 디버그는 화면과 pstore에 기댄다. 단계마다 되돌릴 길을 먼저 둔다.

근거의 한계는 다음과 같다.

- 회로도는 없다. 순정 `extcon-mtk-usb`가 부스트 앞에서 차지 펌프에게 입력 FET를 닫게 한다는 것은 스톡 모듈과 제3자 미러로 확인했다. 이것 없이 5V가 나가는지는 기기에서만 안다(나가지 않을 가능성이 크다고 본다).
- 드라이버는 실제 커널의 i2c·regmap·regulator 코어 위에서 레지스터 모델로 돌렸다. 프로브·등록·늦은 정리·읽기에서 쓰기가 0번인 것은 이 범위에서 증거가 된다. 칩이 PWR_RDY·VAC1을 언제 올리는지, 비밀 코드와 숨은 레지스터가 어떻게 도는지는 모델이 해석을 품으므로 증거가 되지 않는다. 실제 `i2c-mt65xx`와 클럭은 돌리지 않았다.
- 이 절의 `0x1xx` 주소는 은행을 윗바이트에 넣은 regmap 주소다(패치 0046). I2C `0x34`에서는 아래 바이트로 읽는다(`0x1E0` → `0xE0`).

### 0단계. 준비

1. [ ] 배터리 50% 이상(LBP는 2.8V이지만 부하에 처지지 않게).
2. [ ] USB-C 브레이크아웃과 멀티미터.
3. [ ] 전력이 작은 USB 메모리. 첫 시험에 허브·SSD는 금지다. TCPC 드라이버가 없어 CC는 LK 상태 그대로이고 Rp·VCONN을 내지 않는다. USB-A 변환 젠더와 단순한 USB-C 메모리는 CC를 보지 않지만, 허브·독은 Rp를 기다리다 붙지 않을 수 있다.
4. [ ] 충전기와 PC를 꽂은 채 켜지 않는다.

### 1단계. 읽기만 (모듈 없이)

1. [ ] 2b의 `&pio`·클럭 컨트롤러에 `&imp_iic_wrap_n`, `&imp_iic_wrap_en`, `&i2c5`, `&i2c10`만 더 켠 dtb로 부팅하고 가젯이 그대로인지 본다. 두 버스는 400 kHz로 시작한다(펌웨어 1 MHz). `i2cdetect`와 `i2cget`은 진짜 루트의 `i2c-tools`나 initramfs의 busybox에 있다. 이미지용 커널에서는 `/dev/i2c-*`가 모듈이라 먼저 `modprobe i2c-dev`를 한다.
2. [ ] 칩을 읽는다. `i2cdetect`는 범위를 꼭 준다. `-r`은 읽기 탐지다.

    ```bash
    i2cdetect -y -r 5 0x34 0x34
    i2cdetect -y -r 10 0x65 0x6e
    i2cget -f -y 5 0x34 0x00    # 0x7X
    ```

3. [ ] 차지 펌프가 어느 칩인지, 스트랩(NU2115 `0x2E` bit 4:3, UPM6720 `0x12` bit 1:0, 값 1이 slave)이 slave가 아닌지 본다. 순정 트리에는 넷이 있다: NU2115 @0x6d·@0x6e, UPM6720 @0x65·@0x66. 순정이 `primary_dvchg`로 부르는 칩이 FET를 닫는다. @0x6d·@0x65는 스트랩이 master나 standalone일 때, @0x6e·@0x66은 standalone일 때 그 이름을 쓴다. 그 노드만 켠다. 보드 dts의 `vbus-switch-supply`는 `charge_pump_65_switch`다(사용자 기기 UPM6720 @0x65). 다른 칩이면 그 레귤레이터로 고친다.
4. [ ] 차지 펌프의 ID를 읽어 맞춘다: NU2115 `0x31`, UPM6720 `0x22`. 실제 칩의 값이 다르면 드라이버 프로브가 실패하고 값을 찍는다(쓰기 없음). 벤더는 ID가 달라도 넘어가고, 이 드라이버는 안전한 쪽으로 거절한다.
5. [ ] 아래를 기록해 둔다. 끈 뒤와 견줄 값이다.
    - `i2cget -f -y 5 0x34`: `0x20`, `0x21`, `0x29`, `0x2A`, `0x30`, `0x31`, `0x32`, `0x34`, `0x35`, `0x40`, `0x50`, `0xE0`, `0xE1`, `0xE4`
    - `0x4E`: `0x1A`, `0x1C`
    - 차지 펌프: `0x2F`, `0x30`, `0x31`, `0x0D`(UPM6720은 `0x06`, `0x0E`, `0x0F`, `0x15`, `0x22`)
    - `/sys/kernel/debug/pinctrl`에서 GPIO147·148·179
    - `clk_summary`에서 `i2c`, `impn_i2c5`
6. [ ] LK가 남긴 상태를 이 값으로 본다: HZ·BUCK_EN(`0x120`), 네 boost 값의 평소 값(순정의 buck 값과 같은지, 패치 0047의 표), BC1.2(`0x150` bit 7), 충전 워치독(`0x12A` bit 3), CC 저항(TCPC `0x4E`의 `0x1A`). I2C 소스 클럭(`i2c` 먹스), HW voter 시간, AP-DMA가 이 버스들에서 도는지도 본다(벤더는 짧은 전송에 FIFO를 쓴다).

### 2단계. 가드 비트 확인

1. [ ] 1단계의 읽기를 스크립트로 UFS에 남겨 케이블이 있을 때와 없을 때를 견준다. 있으면 PWR_RDY(`0x1E0` bit 0)와 VAC1(`0x2F` bit 7, UPM6720 `0x15` bit 4)이 1, 없으면 0이어야 한다. **아니면 멈추고 가드를 고친다.** NU2115의 비트는 인터럽트 표의 `stat` 칸에서 읽었다. 틀리면 외부 VBUS가 있어도 켜진다.

### 3단계. 드라이버 (켜지 않고)

1. [ ] `&mt6375`, 고른 차지 펌프, `&usb_vbus_output`을 켠 dtb로 부팅하고 모듈(`mt6375`, `mt6375-charger`, `nu2115_charger`, `userspace-consumer`)을 `modprobe`한다. `&pio`가 꺼져 있으면 충전기 쪽이 GPIO179 상태를 기다리며 뜨지 않는다. 안전하게 멈춘다.
2. [ ] 프로브에서 칩 쓰기는 0번이다. GPIO179는 이때 low로 간다. GPIO179는 VBUS를 막는 스위치가 아니라 충전 중 커넥터 과열 보호다. 1단계 값과 전부 같아야 한다.
3. [ ] `/sys/class/regulator`의 `usb-otg-vbus`는 `off`, `state`는 `disabled`여야 한다.
4. [ ] 가드를 본다. PC를 꽂은 채 쓴다. 이것이 PC를 꽂고 켜는 유일한 항목이다.

    > **위험: USB 끊김.** 가드 비트의 뜻이 틀리면 외부 VBUS가 있어도 켜지고, 두 전원이 VBUS에서 맞선다.
    >
    > **먼저 둘 것.** 2단계를 통과했어야 한다.

    ```bash
    echo enabled > /sys/devices/platform/usb-vbus-output/state
    ```

    기대: 거절되고(`VAC1 is present`), PC 쪽 USB가 끊기지 않고, 레지스터 변화가 0이다.

### 4단계. 처음 켜기 (부하 없이)

> **위험: USB 끊김.** 케이블을 뽑고 VBUS를 켠다. 가드는 켜는 순간만 본다.
>
> **먼저 둘 것.** 0단계, 2·3단계 통과. 저절로 끄는 배경 스크립트.
>
> **되돌리기.** 스크립트가 끈다. 멈추면 전원+볼륨 올림을 길게 누른다(MT6363 리셋, 6.4절 5번). 그 뒤 VBUS가 살아 있으면 LK가 끄지 않는다는 뜻이니 기록한다.

1. [ ] 케이블을 뽑기 전에 배경에서 시작해 저절로 끄게 한다. `dump`는 1단계 읽기다.

    ```bash
    OUT=/sys/devices/platform/usb-vbus-output/state
    ( sleep 30; echo enabled > $OUT; sleep 70; cat $OUT; dump > /var/log/vbus-on.txt; echo disabled > $OUT; dump > /var/log/vbus-off.txt ) &
    ```

2. [ ] 케이블을 뽑고 브레이크아웃을 꽂아 VBUS를 잰다. 기대: 약 5.0V, 끈 뒤 0V로 내려감.
3. [ ] 0V이면 `status`와 FET 레지스터(NU2115 `0x2F`·`0x30`, UPM6720 `0x0F`)를 본다. `on`인데 0V면 FET나 칩 고름이 틀렸다. `error`면 부스트가 멈췄다(`0x34`, `0x1E3`의 OTG 깃발).
4. [ ] 70초를 켜 두는 까닭은 MT6375 충전 워치독이다. 1단계에서 `0x2A` bit 3이 1이었으면 만료(순정 주기 40초)에 VBUS가 떨어지는지 본다. 순정은 프로브에서 이것을 끄지만 이 드라이버는 건드리지 않는다. 만료가 부스트에 영향을 주는지 코드로는 모른다.
5. [ ] `state` 쓰기는 실패해도 성공을 돌려준다. 꼭 다시 읽어 확인한다.
6. [ ] VBUS를 켠 채 `userspace-consumer` 모듈을 빼지 않는다. 끄지 않고 빠지고, 다시 올리면 supply가 한 번 더 켜져 FET가 닫힌 채 남는다.

### 5단계. FET가 필요한지

1. [ ] 전제를 닫으려면 `vbus-switch-supply`를 뺀 dtb로 4단계를 한 번 더 한다. 0V면 전제가 맞다.

### 6단계. 호스트

> **위험: USB 끊김.** otg 모드도 프로브에서 잠깐 호스트로 시작해 기본 역할로 간다. 그래서 2b 끝까지 `peripheral`로 두었다.
>
> **먼저 둘 것.** 앞 단계의 dtb. `role`과 `state`를 배경 스크립트로 되돌리는 차례.

1. [ ] `dr_mode = "otg"`, `usb-role-switch`, `role-switch-default-mode = "peripheral"`, `&xhci0`을 켠 dtb로 부팅하고 가젯이 그대로인지 먼저 본다. `enable-manual-drd`는 쓰지 않는다(호스트로 시작해 가젯이 뜨지 않는다). `&ssusb`와 xhci의 `vbus-supply`도 쓰지 않는다(otg 모드에서는 프로브마다 켠다).
2. [ ] 4단계의 배경 스크립트에 역할 전환을 더한다. 켜는 차례는 `role`에 `host` → `state`에 `enabled`, 끄는 차례는 `state`에 `disabled` → `role`에 `device`다.

    ```bash
    echo host > /sys/class/usb_role/11201000.usb-role-switch/role    # VBUS 앞
    echo device > /sys/class/usb_role/11201000.usb-role-switch/role  # VBUS 뒤
    ```

3. [ ] USB 메모리를 꽂고 기록을 파일로 남긴다.

    ```bash
    dmesg
    lsusb -t
    ```

    기대: HS로 붙는다. SS는 PS5170 전까지 안 된다. USB3 장치도 HS로 내려 붙어야 한다(스펙 동작, 기기에서 본다). 안 되면 `mediatek,u3p-dis-msk = <1>`이 후보다.

    데스크톱이 떠 있으면 USB 메모리가 파일 관리자에 보이고 붙는지도 본다. UFS를 가리는 규칙(7.1절 1번)은 USB를 건드리지 않아야 한다.
4. [ ] 시험 시간은 수십 초로 하고 커넥터 온도를 손으로 본다. 과열 보호(9.3절 F)를 켠 dtb가 아니면 보호가 없다. 게이지(MM8013)를 켰으면 배터리 전류도 본다.

### 7단계. 끈 뒤

1. [ ] `vbus-off.txt`와 1단계 값을 견준다. 달라도 되는 것은 `0x30`~`0x32`와, 1단계 값이 순정 buck 값과 달랐다면 네 값이다.
2. [ ] 충전기를 꽂아 충전이 전처럼 되는지, PC를 꽂아 가젯이 다시 붙는지 본다.
3. [ ] `0x29` bit 7이 1로 남았을 때 입력 전류가 늘었는지 본다. 순정은 충전기가 붙으면 `CHG_VSYS` bit 7을 0으로 고치는데, 충전 드라이버가 없는 이 커널에서는 그대로다. 블리드 전류만큼 충전 입력에서 샐 수 있다.
4. [ ] UPM6720이면 풀다운을 본다. 끌 때 `BUS_PD_EN`·`VAC1_PD_EN`을 켜고 그대로 둔다. 순정은 shutdown과 프로브의 칩 리셋으로 지우지만, 이 커널에는 그 리셋이 없어 재부팅을 넘어 남을 수 있다. 충전 입력에서 조금 샐 수 있고, 영향은 작을 것으로 본다.

### 8단계. 내려가는 길

1. [ ] VBUS를 켠 채 끄면 VBUS도 꺼져야 한다(`.shutdown`).

    ```bash
    poweroff
    ```

2. [ ] 패닉·길게 누름 리셋 때 VBUS가 남는지 본다. 남으면 그 기록을 남긴다. 드라이버는 다음 부팅에서 켜진 것으로 받는다(쓰기 없이). 사용자 공간에서 끌 수 있는지 함께 본다.

### 되돌아 흐름

켠 채 충전기나 PC를 꽂으면 두 전원이 VBUS에서 맞선다. 가드는 켜는 순간만 본다. 칩이 어떻게 버티는지는 코드로 모르고, 시험할 일도 아니다. TCPC 전까지 손으로만 켜고, 꽂기 전에 끈다.

## 9. 나머지 단계

2d 무선의 차례는 9.1절과 9.2절에, 2e 충전과 TCPC는 9.3절에, 2e GPU·열·게이지는 9.4절에 있다. 2c 화면(2차)은 9.5절에 있다. 3 최적화의 목록은 나중에 더한다.

이미 적힌 확인만 잃지 않게 옮겨 둔다.

- 2d 전원 도메인:
    1. `&spm`만 켠다(`ifr_bus`·`vlpcfg_bus`는 켜지 않는다). `/sys/kernel/debug/pm_genpd/pm_genpd_summary`에 conn이 `off-0`인지 본다. 부팅 로그의 `A default off power domain has been ON`은 LK가 켠 도메인이다.
    2. `scpsys`가 뜬 뒤, 그리고 conn을 켜고 끈 뒤 fbcon이 그대로인지 본다. 꺼지면 EN과 PWR_CON을 먼저 읽는다. `위험: 부팅 불가`는 아니지만 화면 콘솔을 잃는다. USB 가젯 셸을 먼저 띄워 둔다.
    3. conn을 켤 때 ACK가 오지 않으면 두 가지로 보인다. 프로브 전에 플랫폼 코어가 붙일 때의 실패는 `__genpd_dev_pm_attach`가 `-EPROBE_DEFER`로 바꾸므로 드라이버 메시지 없이 미뤄진다(`/sys/kernel/debug/devices_deferred`). 프로브 뒤 `pm_runtime_resume_and_get`의 실패는 `-ETIMEDOUT`이다(`can't power the CONN domain`). `0x1c001e04`의 bit 30·31을 본다.
    4. conn을 끌 때 버스 보호 SET이 시간 초과면 벤더의 EN 재시도와 되돌리기가 필요한지 본다(메인라인에 없다). 켜고 끄기를 몇 번 돌린다.
- 2c 2차와 코덱·카메라 앞: MM_INFRA 아래 첫 소비자 전에 화면·코덱 클럭(패치 0100~0104)과 전원 도메인을 이 차례로 켠다.
    1. [ ] 켜기 전에 2a 셸에서 읽어 둔다. 순서를 지킨다. 도메인 비트가 선 블록만 읽는다.

        ```bash
        busybox devmem 0x10321410 32   # HWV EN. bit 0 MM_INFRA, 1 VDE0, 2 VDE1, 3 DIS0, 4 DIS1, 5 OVL0, 6 OVL1
        busybox devmem 0x1c001e5c 32   # VEN0 PWR_CON (0xe60 VEN1, 0xe94 MDP0, 0xe98 MDP1)
        busybox devmem 0x10000020 32   # CLK_CFG_1: disp0 bit 7, disp1 15, ovl0 23, ovl1 31이 0이면 열림
        busybox devmem 0x10000030 32   # CLK_CFG_2: mdp0 bit 7, mdp1 15, mminfra 23
        busybox devmem 0x100000d0 32   # CLK_CFG_12: venc bit 31
        busybox devmem 0x100000e0 32   # CLK_CFG_13: vdec bit 7 (순정에서 VCP가 다룬다)
        # EN의 해당 비트가 섰을 때만
        busybox devmem 0x14000100 32   # dispsys0 CG0 (bit 3), 0x14000110 CG1, 0x140001a0 CG2
        busybox devmem 0x14200100 32   # dispsys1 CG0 (bit 4), 0x14200110, 0x142001a0
        busybox devmem 0x14400100 32   # ovlsys0 CG (bit 5)
        busybox devmem 0x14600100 32   # ovlsys1 CG (bit 6)
        busybox devmem 0x1e800100 32   # mminfra CG0 (bit 0), 0x1e800110 CG1
        busybox devmem 0x10320018 32   # dispsys0 HWV 투표 0, 그다음 0x10320020, 0x10320028
        busybox devmem 0x10321c0c 32   # dispsys0 HWV DONE 0
        ```
    2. [ ] `spm`을 켜고 MM_INFRA, DIS0, DIS1, OVL0, OVL1 도메인을 적는다. 기본 클럭만 적는다(`mm_infra`, `disp`, `disp1`, `ovl`, `ovl1`). 순정 트리의 `disp0-0`, `mm_infra-0` 같은 이름은 옮기지 않는다: 벤더 HW voter 길은 읽지 않고, 업스트림 드라이버는 켜려 든다.
    3. [ ] `framebuffer0`에 `power-domains`(DIS0, DIS1, OVL0, OVL1)를 같은 변경에서 건다.
    4. [ ] `dispsys0`, `dispsys1`, `ovlsys0`, `ovlsys1`, `mminfra`를 켠다. 화면이 그대로인지 본다. 레지스터 쓰기가 없어야 한다.
    5. [ ] 코덱·MDP는 소비자와 함께: `vencsys`, `vencsys_c1`, `mdpsys0`, `mdpsys1`을 먼저 켜고, 도메인 노드(`ven0-0` 등 부분 클럭 포함)를 그 뒤에 또는 함께 적는다. 꺼 둔 채 도메인만 적으면 `scpsys` 전체가 지연된다.
    6. [ ] `framebuffer0`에 이 블록의 게이트를 `clocks`로 적지 않는다. simpledrm은 클럭을 켠 뒤에 도메인을 잡아, LK가 화면을 켜지 않은 부팅에서 꺼진 블록에 쓴다.
- 2d: `consys@18001000`(`mediatek,mt6897-consys`), `wifi@18000000`(`mediatek,mt6897-wifi`), `bluetooth@18800000`. 전원 도메인(위)이 먼저이고 차례는 9.1절과 9.2절이다. 진짜 루트에서 nvdata를 붙일 때는 initramfs처럼 `blockdev --setro`와 `ro,noload`로 붙인다.
- 2e: 전원 키로 s2idle에서 깨는지, mcusys-off·system idle 상태와 s2idle(SPM), DSU PMU(SPI 35), `#cooling-cells`, mtu3 깨우기 접점. `clk_ignore_unused`를 뺄 때 `mfgsc_ref`에도 `CLK_IGNORE_UNUSED`를 준다(패치 0013은 `mfg_ref`에만 있다).
- 볼륨 내림(SoC 키패드)은 업스트림 `mt6779-keypad`로 된다(2026-11-04, 패치 0145).

### 9.1 2d Wi-Fi

**결과(2026-11-04).** 시험 커널은 트리를 안에 넣어 LK의 값을 받지 못하므로 `gen_dts.py`가 순정 트리의 `emi-addr`, `memory-region`을 옮겨 적는다. 1~5단계의 줄이 `firmware running`까지 나왔다. 펌웨어가 uni 명령을 받지 않아(`unified command probe: -110`, 시험을 건너뛰면 CE 0x8a에 답이 온다) 레거시 명령 길(패치 0061~0065)로 바꾸고 시험을 없앴다. 7단계까지 간다: 스캔이 되고, 비밀번호를 사용자가 직접 넣은 2.4 GHz WPA2-PSK 연결이 4-way, DHCP, 인터넷까지 돈다. 내려받기 최고 138 Mbit/s, 올리기 87 Mbit/s, 약 10분 유지, `tx failed` 0이다. 5 GHz(5745 MHz, 80 MHz) WPA2-PSK 연결도 인터넷까지 된다(짧은 시험이라 속도는 재지 않았다). 처음 본 EXT 0x3e 시간 초과는 명령을 몰아 보내 링이 멈춘 뒤의 일이었고, 멀쩡한 펌웨어에서는 EXT 명령에 답이 온다. 보정 파일은 기기 파티션을 열지 않고 백업에서 꺼내 넣었다. 남은 것: RX 순서 정리, `regulatory.db` 거부, AP 소실 감지, 6단계의 FEM 맞대기([Wi-Fi 문서](wifi-soc7_0.md) 7절). 부팅 블랙리스트는 그대로 두었다.

아래는 코드와 자료로 짠 차례다. 2a(부팅, 콘솔), 2b(SPMI·PMIC·핀·USB 가젯)가 된 뒤에 한다.
위에서 아래로 하고, 앞 단계를 채운 뒤 다음으로 간다. 장치 트리는 단계마다 적은 노드를 켠다.

1·2단계 다음에는 9.2절의 Bluetooth를 먼저 한다. nvdata도 DMA 칸도 필요 없어 connsys 전원의 첫 시험으로 가볍다. 다만 Wi-Fi 없이 켜면 보정이 나쁠 수 있으므로, 전원 길의 시험과 RF 품질은 나눠 본다.

#### 공통 준비

- USB 가젯 셸(`buntos.usbdebug=1`)로 들어가 `dmesg -w`를 PC에 받아 둔다. 드라이버는 단계마다 한 줄씩 남기므로, 멈추면 **마지막 줄이 멈춘 곳**이다.
  화면 콘솔(`console=tty0`)도 켠다. 멈추면 USB도 함께 끊길 수 있다.
- 켜 보는 동안 자동 적재를 막는다: 명령줄에 `modprobe.blacklist=mt7921soc,mtk_connsys`. 모듈은 셸에서 손으로 올린다.
- **멈췄을 때 되돌리기:** 전원 키와 볼륨(홈) 키를 길게 눌러 PMIC 초기화(순정 트리 설정)로 다시 켠다. 그래도 안 되면 배터리가 빠질 때까지 둔다.
  명령줄에 블랙리스트가 있으므로 다시 켜면 셸까지 온다. 그 전 단계 dtb로 돌아가려면 앞 부트 이미지를 다시 굽거나 `fastboot boot`를 쓴다. nvdata는 어느 단계에서도 쓰지 않는다.
- 모든 쓰기는 PMIC 레일, 패드, connsys 레지스터뿐이다. 파티션은 읽기만 한다.

#### 1. LK가 메모리를 나누는지 (드라이버 없음, 레일 없음)

`mt6897.dtsi`의 루트에 **`gps`·`connscp` 노드만** 넣는다(아래). consys·wifi는 disabled로 둔다. 드라이버가 돌지 않으므로 커널 쪽 위험은 없다.
LK는 status를 보지 않으므로 이 순간부터 펌웨어를 싣는다. 이것이 기기에서 처음 지나는 길이다.

```dts
connscp {
	scp-shm-size = <0x20000>;
	scp-remap-offset = <0x2100000>;
	scp-remap-size = <0x20000>;
};

gps {
	emi-size = <0x100000>;
	emi-alignment = <0x100000>;
	emi-max-addr = <0xc0000000>;
};
```

위험: 부팅 불가. LK가 mainline 트리에서 이 길을 지난 적이 없다. 되돌리는 길은 노드 없는 dtb다.

볼 것 (`/run/buntos/fdt.dtb` 또는 `/sys/firmware/fdt`를 `dtc -I dtb`로 푼다):
- `/soc/consys@18001000`의 `emi-addr`가 0이 아닌 8바이트인가
- `/soc/wifi@18000000`에 `memory-region`이 있고, 그 phandle의 `/reserved-memory/mblock-*`에 `shared-dma-pool`, `no-map`, 크기 0x1400000이 있는가. 주소가 4 GiB 아래인가(0x2_5000_0000 위면 드라이버가 멈춘다)
- `dmesg`의 reserved-memory 줄, `/proc/iomem`
- 복구나 충전 모드가 아닌지(LK는 그때 싣지 않는다). `fastboot boot`일 때도 싣는지 여기서 안다

안 되면(`emi-addr`가 0): LK 기록(uart)의 `platform_load_connsys` 줄을 본다. 값이 빠지면 -2나 -1을 남기고 돌아온다.

#### 2. consys 드라이버 (레일만 잡고 아직 켜지 않음)

`status = "okay"`: consys, `pmic@9`, 레일 다섯(`vcn13`, `vrf-io18`, `vant18`, `vcn33-1`, `vcn33-2`), `&spm`. wifi는 아직 disabled. 전원 도메인의 확인이 먼저다(`pm_genpd_summary`에 conn이 `off-0`). IFR·VLP syscon은 켜지 않는다.

`modprobe mtk-connsys` 뒤에 볼 것:
- `firmware region at …, 0xc00000 bytes` 한 줄과 바인드(`/sys/bus/platform/drivers/mtk-connsys`). `emi-addr`가 0이면 `the bootloader has not loaded the firmware`로 멈춘다
- `no DCXO device`가 나오면 `pmic@9`가 안 잡힌 것이다. 레일을 기다리면 PMIC 드라이버 순서를 본다
- `/sys/kernel/debug/regulator/regulator_summary`에 다섯 레일이 꺼진 채 있는가. 2.5 V 같은 낯선 전압이 없는가
- `modprobe mtk-connsys`가 CONN 전원 도메인을 **처음 켜는 곳**이다. 드라이버가 아니라 플랫폼 코어가 프로브 앞에서 켜고, 프로브 뒤에 `genpd_dev_pm_sync`가 끈다. 레일은 꺼진 채이고 드라이버의 줄은 없다. `emi-addr`가 0인 부팅에서도 그렇다. `firmware region` 줄이 나오기 전에 멈추면 전원 도메인 쪽이다. 벤더도 같은 플랫폼 코어를 지난다(추정).

이 단계에서 SIP 호출과 레일 켜기는 일어나지 않는다. 전원 도메인은 위처럼 한 번 켜졌다 꺼진다.

#### 3. connsys 전원 켜기 (첫 SIP 호출)

`wifi`를 `status = "okay"`로 하고, 먼저 보정값을 둔다.

```
blockdev --setro /dev/disk/by-partlabel/nvdata
mount -t ext4 -o ro,noload /dev/disk/by-partlabel/nvdata /mnt/vendor/nvdata
install -D -m 0600 /mnt/vendor/nvdata/APCFG/APRDEB/WIFI /usr/lib/firmware/lenovo/tb375fc/wifi-nvram.bin
```

위험: 부팅 불가가 아니라 그 자리 멈춤이다. `modprobe mt7921soc`을 치면 아래 줄이 차례로 나와야 한다. 마지막 줄이 멈춘 곳이다.

| 줄 | 뜻 | 여기서 멈추면 |
|---|---|---|
| `DMA window at …, 20 MiB` | LK의 칸 | 1단계의 값과 같은가. `the bootloader gave no DMA region`이면 1단계로 돌아간다 |
| `calibration data: …, N of M bytes in the list` | 파일 | `no calibration data`면 probe가 **아무것도 켜지 않고** 멈춘다. 파일을 두고 다시 올린다 |
| `A-die clock: 26 MHz co-clock` | DCXO 읽기 | SPMI, MT6685 |
| `SIP init_atf_data(0, 0)` | TF-A에 값을 넘김 | TF-A가 이 호출을 받지 않는다. SMC는 돌아와야 한다 |
| `rail VRFIO18`, `rail VCN13` | MT6363 레일 | SW_OP_EN 쓰기, 레귤레이터 오류 메시지 |
| `CONN power domain` | MTCMOS, 버스 보호. 도메인은 2단계에서 이미 한 번 켜졌다 꺼졌다. `can't power the CONN domain: -110`이면 ACK다 | 전원 도메인 표 |
| `SIP polling_chipid(0, 0)` | D-die 응답 | 0이 아니면 `the D-die does not answer`로 돌아온다 |
| `SIP a_die_cfg(0, 8)` | A-die SPI | `A-die version`이 0x6637 쪽 값인가 |
| `SIP afe_wbg_cal`, `subsys_pll_initial` | 클럭·PLL | 레일 전압, 클럭 값 |
| `rail VCN33_1`, `VCN33_2`, `VANT18` | RF 레일 | 레귤레이터 오류 |
| `radio 3 power on: 0, radios 0x8` | 공통부 완료 | |

SIP 줄에 `returned N`이 붙으면 그 값을 적는다. 0이 아니어도 벤더처럼 계속 가는 호출이 있다.

`SIP init_atf_data` 줄은 부팅에 한 번만 나온다(벤더와 같다). 그래서 이 호출 뒤로 `mtk_connsys` 모듈은 내려가지 않는다(`Module mtk_connsys is in use`). `rmmod mt7921soc`만으로 라디오 전원은 꺼진다.

#### 4. WF MCU와 ROM

| 줄 | 여기서 멈추거나 실패하면 |
|---|---|
| `WF: wake conninfra` | `conninfra version: … = …`이면 값을 벤더의 0x02050400과 맞댄다 |
| `WF: top on` 뒤 `wfsys reset` | 실패 메시지와 값 |
| `WF: first read of the WF side` | 버스 보호가 안 풀린 채 읽어 멈춘 경우다. 그 전 폴링이 통과했어야 한다 |
| `WF: CPU out of reset` | |
| `no firmware own after the ROM` | 경고일 뿐이다(벤더는 기다리지 않는다). 다음 줄을 본다 |
| `WF: ROM idle` | `WF ROM: 18060b10 = …`이면 `ROM code index`와 함께 적는다. 0x1D1E가 안 오면 WF가 ROM에서 멈춘 것이다(커뮤니티 포트가 멈춘 자리, 그쪽은 벤더 gen4m을 옮긴 것이고 포럼이 아닌 그 저장소의 기록이다) |
| `ASIC revision: 79610000, driver own` | `driver own failed` |
| `DMA rings` | 링을 만들고 WFDMA를 켠다. 여기서 멈추면 WFDMA 레지스터나 칸 접근이다 |

#### 5. 펌웨어 기동

| 줄 | 볼 것 |
|---|---|
| `firmware in memory: …` | `t-neptune-main-soc7_0-…`이어야 한다. `(unknown)`이면 LK가 싣지 않았거나 읽는 칸이 틀렸다 |
| `PHY action: calibrate, N bytes of data, FEM 0x0` | 보정은 수 초 걸린다 |
| `PHY action: event 0x81, status 0, temperature T` | event가 0x81(`FORCE_CAL_RSP`)이고 T가 그럴듯한가 |
| `starting the firmware the bootloader loaded` → `firmware running` | `Message … timeout`이면 INIT 명령의 링이나 이벤트 링이 틀렸다. `/sys/kernel/debug/ieee80211/phy*/mt76/`의 queues, `MT_WFDMA0_HOST_INT_STA` |

답이 없을 때의 줄이다. 명령마다 답을 끊어 드라이버를 레지스터 모델 위에서 돌려 본 결과이고, 모델은 펌웨어가 어떻게 답하는지 말해 주지 않는다. 기기에서는 패치 0061 뒤로 첫 적재에서 링이 멈추지 않았고 연결 동안 `Message … timeout`이 없었다(2026-11-04). 패치 0066 전에는 칩 리셋에서 CE 0x8a 시간 초과가 났다(7단계).

| 줄 | 명령 | 뜻과 다음 |
|---|---|---|
| `Message 0004008a (seq N) timeout` 뒤 `hardware init failed` | CE `GET_NIC_CAPAB` 0x8a | 벤더 표에 있다(`GET_NIC_CAPABILITY_V2`). 이벤트 꼴을 본다 |
| `Message 00003eed (seq N) timeout` 뒤 `hardware init failed` | EXT `PROTECT_CTRL` 0x3e | **벤더 표에 없는 번호다.** 답을 기다리는 명령이라 wlan0이 생기지 않고, `hw_init_done` 전이라 리셋도 돌지 않는다. 버스는 멈추지 않고 `rmmod`로 내려간다. 이 명령만 건너뛰게 할지 그때 정한다 |
| `Message 000500c0 (seq N) timeout`이 되풀이 | CE `REG_READ` 0xc0 | 벤더 표에 있다(`ACCESS_REG`). `SET_RATE_TX_POWER` 묶음마다 하나씩 기다리므로 3초씩 쌓인다. wlan0이 그만큼 늦게 생긴다(모델에서 68초). 멈춘 것이 아니다 |
| `time sync failed: -110` | INIT 0x09 | 경고일 뿐이다. 다음 줄로 간다 |

그 밖에 이 길의 CE 0xc5(`FW_LOG_2_HOST`), 0x48(`SET_NVRAM_SETTINGS`), 0x0f, 0x5d, 0xca는 답을 기다리지 않는다. 답이 없어도 줄이 나오지 않는다.

#### 6. FEM 맞대기

안드로이드 슬롯에서 connfem 기록을 받는다: `FemInfo, id:`. 순정 트리에는 `hwid`가 없어 0번(nofem/nofem)이 뽑히지만 `/vendor/firmware/connfem.cfg`가 바꿀 수 있다(adb로 못 읽었다).
값이 0이 아니면 `mediatek,fem-parts`, `mediatek,fem-pins`, `pinctrl-0`을 순정 트리의 그 조합(`qm42195-qm45197` 등)에서 옮긴다. 순정 트리의 표는 아래와 같다(가장 새 값).

| i | 2.4 GHz | 5 GHz |
|---|---|---|
| 0 | nofem | nofem |
| 1, 2 | wlan7207h | wlan7207c |
| 3, 4 | qm42195 | qm45197 |
| 5, 6 | nofem | sky85732 |
| 7 | qm42195 | nofem |
| 8 | vc536411 | nofem |
| 9 | nofem | qm45197 |

vid/pid: nofem 0/0, wlan7207h 2/4, wlan7207c 2/7, qm42195 3/1, qm45197 3/2, sky85732 1/3, vc536411 5/1. 핀은 순정 트리의 같은 이름 pinctrl 노드에 있다.

#### 7. 인터페이스

`ip link set wlan0 up` → `iw dev wlan0 scan` → 연결. 여기서 처음 쓰는 명령 가운데 벤더 표에 없는 것이 있다.

| 단계 | 명령 | 벤더 표 | 답이 없으면 |
|---|---|---|---|
| up | EXT `MAC_INIT_CTRL` 0x46 | 없음 | `Message 000046ed`, `ip`가 `Connection timed out`으로 실패한다. 그 뒤 리셋이 돈다 |
| up | CE `SET_CHAN_DOMAIN` 0x0f | 있음 | 기다리지 않는다 |
| up | EXT `SET_RX_PATH` 0x4e | 벤더 헤더에서 같은 번호가 `EFUSE_BUFFER_RD`다 | `Message 00004eed`, `ip`가 실패한다. 그 뒤 리셋이 돈다 |
| up | CE `SET_RATE_TX_POWER` 0x5d, `REG_READ` 0xc0 | 있음 | 0x5d는 기다리지 않는다. 0xc0은 묶음마다 3초 기다리고, 시간 초과마다 리셋이 걸리므로 오래 걸린다(모델에서 203초) |
| up | EXT `ID_RADIO_ON_OFF_CTRL` 0x05 | 있음 | 기다리지 않는다 |
| up | CE 0x11(인터페이스), 0x28(DBDC 끄기) | 있음(레거시 길, 패치 0065) | 기기에서 동작했다. 0x28은 `eid 78`로 답한다 |
| up | CE `SET_EDCA_PARMS` 0x1d, `SET_RX_FILTER` 0x0a | 있음 | 기다리지 않는다 |
| up | EXT `CHANNEL_SWITCH` 0x08 | 있음 | `Message 000008ed`. `ip`는 성공하고 mac80211의 `ieee80211_hw_conf_init` 경고가 한 번 나온다 |
| scan | CE `START_HW_SCAN` 0x03 | 있음(`SCAN_REQ_V2`) | 기다리지 않는다. 끝 이벤트(0x0d)가 없으면 스캔이 끝나지 않는다. 기기는 1186바이트를 무시해서 1192바이트로 보낸다(패치 0063) |
| 연결 | CE 0x1c(채널 시간), 0x12(BSS), 0x13·0x14(스테이션), 0x07(키) | 있음(레거시 길, 패치 0065) | 2.4 GHz와 5 GHz의 WPA2-PSK에서 동작했다 |

- **칩 리셋은 기기에서 첫 시도에 돌아온다(2026-11-04, 패치 0066).** `Message … timeout`으로 들어간 리셋과 펌웨어가 답을 끊은 뒤의 리셋도 2.5초 뒤 `firmware running`이 찍히고 스캔이 된다. 연결 중 리셋 뒤에는 약 9초에 다시 붙고 DHCP와 인터넷까지 돌아온다(패치 0067). 아래의 되풀이와 멈춤은 모델에서 본 것이다. 리셋이 끝내 실패하면(패치 0066 전에 봤다) `chip reset failed` 뒤 `iw scan`이 끝나지 않고, 모듈을 다시 올리면 돌아온다.
- **데이터 경로가 조용히 멈추면 패치 0069가 칩을 다시 시작한다(2026-11-05).** 로그에 `TX stalled: none back in … ms, … tokens`가 남는다. 감시가 듣기 전에 mac80211이 끊으면(0068) NetworkManager의 재시도 대기(5분)에 걸릴 수 있다. 자세한 것은 [Wi-Fi 문서](wifi-soc7_0.md) 7절이다.
- `Message … timeout`이 나면 cmd와 함께 적는다. 그 뒤 `mt792x_reset`이 WF 전원을 다시 돌리고 보정을 다시 한다. 한 번의 리셋은 10번까지 시도한다. 켜지기는 하는데 펌웨어가 답하지 않거나 잠시 돌다 멈추면 리셋이 다시 걸려 되풀이된다. 앞 재시작에서 10분이 지나지 않은 재시작이 **20번 이어지면** `no more WF restarts: 20, under 10 minutes apart`로 멈춘다. 10분 넘게 잘 돈 뒤의 재시작은 새로 센다. WF는 꺼진 채 남고, 명령은 곧바로 거절된다(`-EIO`).
- 멈추려면 `rmmod mt7921soc`. 지금 도는 시도만 기다리고 남은 시도는 건너뛴다. 시도는 보정과 시간 초과로 13초쯤까지이고, `REG_READ`(0xc0)에 답이 없는 시도는 1분이 넘는다(모델에서 71초). 다시 켜 보려면 `echo 1 > /sys/kernel/debug/ieee80211/phy*/mt76/chip_reset`이나 `modprobe`다. 멈춘 뒤의 `chip_reset`은 마지막 재시작에서 10분이 지나야 돈다. 그 전의 `chip_reset`은 거절되고, 이 시간을 늘리지 않는다.
- 다시 켜기가 실패하면 `WF restart failed`가 나오고, 그 뒤로 드라이버는 WF 쪽과 WFDMA를 건드리지 않는다(`… while the WF part is off`가 찍힌다). `rmmod`로 내려도 된다(링 레지스터는 그동안 메모리를 가리킨다). 다시 올려도 안 되면 재부팅으로 되돌린다.
- 이 상태에서 `ip link set wlan0 up`은 `Input/output error`로 실패한다. `ip link set wlan0 down`과 `rmmod`는 된다.
- `irq N: nobody cared`와 `Disabling IRQ #N`이 나오면 이 선을 WFDMA 말고 다른 것이 올리고 있다. 벤더는 이 선을 인터럽트 컨트롤러에서 막으므로 벤더 코드로는 없다고 말할 수 없다. 커널이 선을 끄고 100 ms마다 폴링하므로 명령은 느리게라도 돌 수 있고, 모듈을 다시 올려야 풀린다. `HOST_CSR_IRQ_EN`(`0x18001600`)의 `0x3ff`를 줄여 볼 자리다. `/proc/interrupts`도 본다.
- `DMA window full` 줄은 20 MiB 칸이 찼다는 뜻이다. 소켓이 안 읽고 쥔 RX 페이지가 칸을 차지한다. 그 동안 RX가 서고 명령과 TX가 `-ENOMEM`으로 실패할 수 있다. 드라이버는 100 ms마다 다시 채운다. 소켓이 읽으면 되살아나야 한다. `/sys/kernel/debug/ieee80211/phy*/mt76/rx-queues`로 본다.
- debugfs `runtime-pm`은 켜도 소유권을 내주지 않는다(하드웨어에 닿지 않는다). `deep-sleep`은 쓰지 않는다. 펌웨어의 `KeepFullPwr`를 내리는 명령이고 이 길은 시험한 적이 없다.
- CNM: 패치 0065에서 `cnm=` 매개변수를 뺐다. 채널 시간은 레거시 CE 0x1c로 받고, 허가는 `eid 10`으로 온다.

#### 끝날 때

- 끄기: `rmmod mt7921soc`. `WF:` 끄기 폴링 실패 줄이 없는가, 레일이 꺼졌는가(`regulator_summary`), 다시 올렸을 때 같은 줄이 나오는가(`SIP init_atf_data`는 빼고). `mtk_connsys`는 내려가지 않는다. 시스템 절전은 시험하지 않는다(wowlan을 내놓지 않는다).
- 2b의 USB 가젯 길: 이 드라이버는 USB를 건드리지 않는다. 레일과 전원 도메인은 connsys 것만 쓴다. 끊기면 보고한다.
- 소비 전력: 레일을 켠 채 두므로 대기 전력이 순정보다 크다. 2e에서 잰다.

### 9.2 2d Bluetooth

**결과(2026-11-04).** 1~2단계의 줄이 모두 나왔고 `WMT function on: status 0`, 보정 status 0이다. 시험값 주소를 넣으면 어댑터가 켜지고 스캔에서 주변 장치 17개가 보인다. 펌웨어가 LMP에 Synchronization Train을 세우고 명령 표에는 올리지 않아 HCI 초기화가 멈췄다(패치 0087). 남은 것: 페어링, 부팅 때 주소 넣기, Wi-Fi와 함께 쓰기.

**결과(2026-11-05, 패치 0088·0190, `buntos-bluetooth.service`).** 재부팅 뒤 서비스가 5.8초에 끝나고 어댑터가 켜진다(재부팅마다 봤다). 주소는 기기에서 처음 켤 때 만들어 `/var/lib/buntos/bluetooth-address`(root 600)에 둔다. 공장 주소를 쓰려면 이 파일을 바꾼다.

| 시험(Wi-Fi 연결한 채) | 결과 |
|---|---|
| `bluetoothctl power off`와 `on` 7번, `rfkill` 1번 | 모두 돌아온다. `WMT function on: status 0`, 보정 status 0. 스캔 0~18대(장소에 따라 31대까지), Wi-Fi 유지 |
| BT를 켠 채, 끈 채 Wi-Fi 칩 리셋 | `firmware running` 4초, 연결과 데이터가 돌아오고 BT도 그대로다 |
| BT를 켠 채, 끈 채 `rmmod mt7921soc`, `modprobe` | 7초에 다시 연결. BT를 켠 채 내린 동안 스캔은 0대이고, BT를 끄고 켜거나 Wi-Fi를 올리면 돌아온다 |
| `rmmod btmtkbeif` 뒤 Wi-Fi를 켠 채 `modprobe` | `power ack: 00424 = 00000000`으로 실패하고 서비스도 실패로 끝난다. `rmmod mt7921soc`까지 해 connsys를 내렸다 올리면 돌아온다 |
| 시스템 잠자기 | 돌아온다. 0190 전에는 `no driver own: 00000004, MCU pc 00000000`으로 죽었다 |
| Wi-Fi 속도(2437 MHz, 한 번씩 10초) | BT 꺼짐 2.7~2.9 MB/s, 유휴 3.0~4.2 MB/s, 스캔 중 0.8 MB/s |

- **원인을 찾지 못한 것:** Wi-Fi가 connsys를 쥔 채 BGF 전원을 내리면 다음 켜기의 전원 응답이 오지 않는다. 끄는 단계를 하나씩 빼기(보호, FUNC_EN, CPU 리셋, A-die 시계, 세마포어, EMI, 라디오 비트), 끄기 전 펌웨어에 넘기기, 켜기 앞 보호 풀기, `0x18000144` bit 4 지우기로는 풀리지 않았다. 안드로이드에서 같은 동작을 보지는 않았다.
- **못 본 것(2026-11-06 기준):** 블루투스 마우스, 긴 A2DP 재생, 대기 전력, 펌웨어가 죽은 뒤의 복구(위 실패와 같은 길을 탄다).
- 켤 때마다 `Failed to read local supported codecs (-95)`가 난다. 오프로드 코덱 목록을 읽는 명령이라 연결에는 쓰지 않는 것으로 본다(소리로 확인하지 않았다).

**결과(2026-11-06, `CONFIG_UHID`).** AirPods는 사용자가 페어링했고 기본 출력으로 잡힌다. BLE 키보드는 페어링과 연결이 되어도 입력이 오지 않았고 터치 키보드가 그대로 떴다. bluetoothd 기록은 `input-hog profile accept failed`이고 `/dev/uhid`가 없었다. `CONFIG_UHID=m`, `CONFIG_INPUT_UINPUT=m` 모듈을 재부팅 없이 넣고 키보드를 다시 연결하자 `hid-generic`이 붙고, 컴포지터 기록이 `input mode: pointer`로 바뀌고, 입력이 된다(사용자 확인). `CONFIG_BT_LE`는 arm64 defconfig대로 꺼진 채이고 그래도 BLE 페어링과 연결이 된다.

- `CONFIG_BT_RFCOMM`이 꺼져 있어 부팅 때 `RFCOMM server failed for Hands-Free`가 난다. 이어폰을 마이크로 쓰는 프로필이 없다.

아래는 코드와 자료로 짠 차례다. 2a(부팅, 콘솔), 2b(SPMI·PMIC·핀·USB 가젯)가 된 뒤에 한다.
9.1절의 1·2단계가 먼저다. 그 뒤로 Bluetooth와 Wi-Fi는 어느 쪽을 먼저 해도 된다.

#### 공통 준비

- USB 가젯 셸(`buntos.usbdebug=1`)로 들어가 `dmesg -w`를 PC에 받아 둔다. 드라이버는 단계마다 한 줄씩 남기므로, 멈추면 **마지막 줄이 멈춘 곳**이다. 화면 콘솔(`console=tty0`)도 켠다.
- 명령줄에 `modprobe.blacklist=btmtkbeif`를 넣어 자동 적재를 막고 손으로 올린다. `hci_register_dev`가 곧바로 전원을 켜므로 **`modprobe`가 곧 첫 켜기**다.
- 멈췄을 때 되돌리는 길은 9.1절과 같다(전원 키 길게, 블랙리스트 덕에 다시 켜면 셸까지 온다). 파티션은 읽지도 쓰지도 않는다.
- 만지는 것: `0x18000000~0x18080000`, `0x18800000~0x18821000`의 레지스터, EMI의 위 1 MiB(링 32 KiB에 쓰고 로그 머리를 읽는다). 펌웨어 RO 칸은 읽지 않는다.

#### 1. 먼저 할 일 (9.1절 1·2단계)

- `gps`·`connscp`를 넣어 LK가 `emi-addr`를 쓰는지 본다. `consys` 드라이버가 `firmware region at …`을 남기는지 본다.
- 여기까지 Bluetooth 코드는 돌지 않는다. `bluetooth@18800000`은 `disabled`다.

#### 2. 첫 켜기

`bluetooth@18800000`을 `status = "okay"`로 켜고 `modprobe btmtkbeif`를 친다. 프로브 자체는 하드웨어를 건드리지 않지만, 프로브 끝의 `hci_register_dev`가 곧바로 전원을 켠다. `rfkill`로는 막지 못한다(HCI 설정 단계는 rfkill을 보지 않는다, `hci_dev_open_sync`). 위험: 부팅 불가가 아니라 그 자리 멈춤이다.

프로브에서 끝나는 경우:

| 줄 | 여기서 멈추면 |
|---|---|
| 아무 줄 없이 묶이지 않음 | consys 드라이버가 안 묶였다(`EPROBE_DEFER`로 조용히 기다린다). `/sys/bus/platform/drivers/mtk-connsys`, `/sys/kernel/debug/devices_deferred` |
| `can't map the rings` | LK가 연 connsys 칸이 `no-map`이 아니다. `/proc/iomem`, `/reserved-memory`를 본다. 드라이버 고칠 곳이다 |

켜기:

| 줄 | 뜻 | 여기서 멈추거나 실패하면 |
|---|---|---|
| `radio 0 power on: 0, radios 0x1` | connsys 공통부(또는 `radios 0x9`면 Wi-Fi가 켠 위에) | 9.1절 3단계의 표 |
| `BGF: wake conninfra` | `0x180601A8`로 깨움 | `conninfra version: …`이면 값을 `0x02050400`과 맞댄다 |
| `BGF: top on, CPU in reset` | BGF 전원 | `top isolation`: `0x18060A10` |
| `BGF: function on` | `0x18001208` | `power ack`, `second power ack`: `0x18000424` |
| `BGF: conn2bt sleep protection off`, `bt2conn` | GALS 보호 | `receive/transmit sleep protection` |
| `BGF: first read of the BGF side` | **여기서 조용히 멈추면** 버스다. 그 앞 폴링이 지났는데 읽기가 안 돌아오는 경우다 | 첫 의심은 CONN 전원 도메인과 레일 |
| `BGF: version 02080000` | IP 판 | 다른 값이면 SoC 판이 다르다 |
| `BGF: CPU out of reset` | MCU를 놓음 | |
| `BGF: MCU ROM done after N polls` | ROM 끝 `0x1D1E` | `BGF: MCU ROM: …, pc …`: **LK가 `connsys_bt`를 싣지 않았거나 EMI 재매핑이 틀렸다.** LK 기록의 `CONN_RO BT load`를 본다 |
| `firmware: t-neptune-main-soc7_0-…` | 판 문자열. 실기기 속성 `vendor.connsys.bt_fw_ver`와 같은 꼴이어야 한다 | `release string at 0x…, not read`는 실패가 아니다. 벤더도 0을 넘어간다 |
| `BGF: driver own` | `0x18060030` | `no driver own: …, MCU pc …`: MCU가 안 돈다 |
| `BGF: up, ring of 16304 bytes each way` | 링 준비 | |
| `can't set the antennas: …` | 경고일 뿐 간다. 첫 WMT 명령이라 **`WMT command 0x55 timed out`이면 링이나 초인종부터 본다** | 아래 줄과 같은 표 |
| `WMT function on: status 0` | **가장 중요한 답이다.** 링과 초인종과 인터럽트가 모두 맞고 펌웨어가 켜졌다는 뜻이다 | `WMT command 0x6 timed out`과 그 뒤 `BEIF: key … guards … to chip N/M, from chip …`: N과 M이 같으면 펌웨어가 읽었고 답이 안 온 것, 다르면 초인종이나 펌웨어 쪽이다. status가 0이 아니면 보정 실패일 수 있다(아래 3) |
| `WMT calibration: status S, sub 3, N bytes` | 보정 결과. 켜지기는 켜졌어도 S가 0이 아니면 RF가 나쁠 수 있다 | `no word on the calibration: …`: 펌웨어가 답하지 않는다. 벤더도 무시한다 |

#### 3. 보정

벤더는 부팅 때 Wi-Fi를 함께 켜고 BT를 켜 보정하고, 그 값을 SYSRAM에 되돌린다. 주석은 "Wi-Fi가 꺼진 채 첫 켜기는 보정에 실패하기 쉽다"고 적는다. 이 드라이버는 켤 때마다 펌웨어가 보정한다.

- Wi-Fi를 올리지 않고 켠 경우와 `modprobe mt7921soc` 뒤에 켠 경우를 나눠 본다: `WMT function on`과 `WMT calibration`의 status와 걸린 시간, RSSI(`hcitool`, `btmgmt`), 연결의 안정성.
- 차이가 크면 Wi-Fi가 켜진 뒤에만 Bluetooth를 켜게 하거나 벤더의 보정값 복원 길을 옮긴다(`WMT_OPCODE_RF_CAL` 0x14, sub 3, `0x5AD02EA5`).

#### 4. HCI

- **주소가 먼저다.** MCU가 켤 때마다 ROM부터 시작하고 저장소가 없으므로, 드라이버는 주소를 시스템에서 받는 꼴이다(`HCI_QUIRK_USE_BDADDR_PROPERTY`, `btqcomsmd`와 같다). 주소가 없으면 `WMT function on` 뒤에 어댑터가 **설정 전(unconfigured) 상태**로 멈춘다(`btmgmt info`의 `unconfigured`). 정상이다.
  - 처음 볼 때는 시험용 값을 넣는다: `btmgmt --index 0 public-addr <값>`. 값은 저장소에 적지 않는다.
  - 스택이 값을 기억해 켤 때마다 다시 넣는다. 재부팅하면 사라지므로 나중에 BSP가 부팅 때 넣는다. `local-bd-address`를 트리에 적어도 되지만 기기마다 달라 이미지에 넣지 않는다.
  - 안드로이드에는 `persist.odm.bt.address` 속성이 있다(값은 적지 않는다). 뽑는 길은 6단계의 표다.
- 넣은 값이 그대로 보이는지 본다(`btmgmt info`). 바이트 순서가 뒤집히면 업스트림 `btmtk_set_bdaddr`와 펌웨어가 다른 것이다.
- `btmgmt info`, `hciconfig -a`: 제조사 70, 판 번호. 스택의 HCI Reset과 초기화 명령이 처음 지나는 곳이다.
- 명령이 시간을 넘기면 `BGF: a command timed out`과 링 상태를 남기고 스택이 닫고 다시 연다.
- `bluetoothctl power on`, `scan on`: 근처 기기가 보이는가. `pair`, A2DP 싱크나 HID로 큰 흐름을 본다(C2H 칸을 넘기는 데이터). `hcidump`나 `btmon`으로 본다.

#### 5. 함께 쓰기와 끄기

| 시험 | 기대 |
|---|---|
| BT 켠 뒤 `modprobe mt7921soc` | `radio 3 power on: 0, radios 0x9`. BT 흐름이 끊기지 않는다 |
| Wi-Fi 켠 뒤 BT 켜기 | `radio 0 power on: 0, radios 0x9` |
| `bluetoothctl power off`(Wi-Fi 켠 채) | `WMT function off`, `BGF: the MCU stays up`. 다시 켜면 `BGF: up again`, `WMT function on: status 0`. Wi-Fi가 끊기지 않는다 |
| `rmmod btmtkbeif`, 다시 `modprobe` | `BGF: power off`, `radio 0 power off, radios 0x8` 뒤 Wi-Fi가 켜져 있으면 `power ack`로 실패한다(패치 0088). 둘 다 내리면 돌아온다 |
| 둘 다 끄기 | `regulator_summary`에서 레일이 꺼진다 |
| `power off` 때 `the bus to it is not quiet` | 벤더는 여기서 connsys 전체를 리셋한다. 이 드라이버는 하지 않는다. 보고한다 |

#### 6. 벤더와 맞대 볼 것 (안드로이드 슬롯, 기기 값은 저장소에 적지 않는다)

| 무엇 | 명령 | 까닭 |
|---|---|---|
| `BT_FW.cfg` | `adb shell cat /vendor/firmware/BT_FW.cfg`(권한으로 막히면 그 판의 vendor 이미지에서) | 안테나 태그(`bt_antswap[…]`, `wifi_antswap`)가 있으면 그 명령을 드라이버에 더해야 한다. 지금 드라이버는 파일이 없을 때의 벤더 길이다. 기기 값이 아니라 판별 값이라 저장소에 적어도 된다 |
| 켠 뒤의 HCI 차례 | 개발자 옵션의 HCI 스누프를 켜고 BT를 껐다 켠 뒤 `adb pull /data/misc/bluetooth/logs/btsnoop_hci.log` | HAL이 보내는 벤더 명령을 안다. 주소가 들어 있어 저장소에 넣지 않는다. 명령 번호만 옮겨 적는다 |
| 펌웨어 판 | `adb shell getprop vendor.connsys.bt_fw_ver` | 이미 읽었다: 빌드 `t-neptune-main-soc7_0-2104-LEPIN_SOC7_0_E1_ASIC-20211206113401`. 2단계의 `firmware:` 줄과 맞댄다 |
| 주소의 자리 | `adb shell getprop persist.odm.bt.address`로 있음만 보고, nvdata의 `APCFG/APRDEB/BT_Addr`와 같은 값인지 PC에서 비교 | BSP에서 어디서 뽑을지 정한다 |
| 모뎀 | `ls /sys/class/ccci_node` | 모뎀 알림을 뺀 근거. 순정 `modules.txt`에는 ccci가 없다 |

#### 끝날 때

- 대기 전력: 열려 있는 동안 MCU가 깨어 있다(driver own). 2e에서 수면 길을 넣기 전까지 큰 값이 나온다.
- 서스펜드: MCU는 드라이버가 쥔 채로 둔다(패치 0190). 깨우기 원천이 아니라 잠든 동안 온 데이터는 깨어나서 받는다.
- 되풀이 실패: 켜자마자 죽으면 스택이 닫고 여는 것을 세 번 하고 `it keeps failing, not starting it again`을 남긴다. 60초 넘게 살아 있었으면 다시 센다.

### 9.3 2e 충전과 TCPC

기본 상태에서는 새 노드가 모두 `disabled`이고 `dr_mode = "peripheral"`이다. 아래는 한 단계씩 고치는 줄이다. 앞 단계를 기기에서 확인한 뒤 다음 단계로 간다.

**결과 3(2026-11-05, 사용자 동석, 전력계 없음, 레노버 MC-680 68 W와 새 케이블).** 재부팅 없이 시험 모듈로 살아 있는 트리를 고쳐 봤다. 값은 칩의 ADC와 게이지이고 정확도는 모른다.

| 조건 | 입력(태블릿 쪽) | 배터리 |
|---|---|---|
| PD 5 V 3 A | 12.4 W | +1.25 A |
| PD 9 V 2 A | 15.8 W | +1.96 A |
| 맥 포트 PD 5 V 3 A | 9.7 W | +0.59 A |
| 직충전, 드라이버 2.5 A | 22 W | +3.45 A |
| 직충전, 시험 스크립트 4.2 A | 38.1 W | +6.9 A |

- **안전:** 온도 단계(46도 CV 4.24 V, 50도 멈춤, -1도 멈춤과 복귀), 과전압(4.45 V 멈춤), 가드 스크립트, 차단(GPIO179, 새 충전기로 2번 깨끗)을 봤다. 최고 온도는 배터리 34.1도, 커넥터 37.4도, UPM6720 60.4도, MT6375 50도다.
- **충전기가 보낸 하드 리셋(패치 0171):** 전에는 다시 꽂을 때까지 포트가 죽어 입력이 500 mA로 묶이고 배터리가 -1.4 A로 빠졌다. 칩이 알림 마스크를 기본값으로 되돌리고, 남은 고장 표시가 인터럽트 선을 막고, 소프트 리셋이 칩 윗단 마스크(`0x1ff`)까지 잠그기 때문이다. 고친 뒤 7번 모두 1.14초에 9 V 2 A로 돌아왔다. 펌프가 돌지 않는 상태에서 봤다.
- **가짜 만충(패치 0172):** 칩은 CV가 아니어도 CV-VREC(4.14 V) 위에서 전류가 IEOC(350 mA) 아래면 충전을 끝낸다. SoC 73%에서 `Full`, 전류 0으로 멈춘 것을 봤다. 전압이 CV보다 100 mV 넘게 낮으면 되돌린다.
- **만충:** 9 V 2 A에서 4.335 V, +0.34 A 뒤 칩이 끝냈고(04:42) 15분 동안 그대로였다. 73%에서 끝까지 82분이다. **숫자는 89%에 머문다**(`charge_now` 8.56 Ah, `charge_full` 9.68 Ah). UPower는 `charging` 0 W로 남는다. 표시를 맞출지 CV를 올릴지는 정하지 않았다.
- **못 본 것:** 10시간 타이머, 펌프가 도는 중의 하드 리셋과 재시도(SoC 70% 아래), 직충전 중 뽑기, 드라이버로 4.2 A(SoC 40%대 아래), 직충전 중 UPower와 메뉴 문구, 다시 꽂을 때의 가짜 만충.
- **이때는 직충전을 켜지 않았다.** `direct_charge`는 기본 꺼짐이고, 끄면 패치 0177은 동작을 바꾸지 않는다. 2026-11-09에 시험하고 BSP에서 켰다(아래).
- **순서:** 9 V 계약 전에 `nu2115_charger`가 칩 한계를 써야 한다(리셋 값은 VAC OVP 6.5 V). `charge-up`이 이 드라이버를 먼저 올리고, `upm6720-direct`가 없으면 TCPC를 올리지 않는다.

**직충전을 켰다(2026-11-09, 커널 #321, PPS 충전기, Wi-Fi SSH로 관찰).** 패치 0170~0177은 이미지에 들어 있었지만 `direct_charge`가 꺼져 있어 PPS 충전기에서도 PD 9 V 2 A(18 W)로만 받았다. 사용자 곁에서 실행 중에 켜고 표시 잔량 52%에서 시작했다.

| 시점 | 충전기 요청 | 펌프 입력 | 배터리 | 온도(배터리, 커넥터, 칩) |
|---|---|---|---|---|
| 첫 4.2 A 단계 | 4.2 A | 35.9 W | +6.98 A | 24.7, 25.8, 41도 |
| 재시도 뒤 4.2 A | 4.2 A(실측 4.40 A) | 40.1 W | +8.2 A | 25.9, 28.5, 50도 |
| 줄이는 중(잔량 81%) | 1.8 A | 15.1 W | +3.1 A | 29.1, 28.6, 31도 |

- 표시 잔량 52%에서 81%까지 25분이다. 펌프는 표시 86%에서(시작에서 38분, 게이지 4.206 V, 칩 4.282 V) 끝내고 벅(9 V 2 A)으로 넘겼다. 충전기 쪽 약 45 W는 케이블을 드라이버 값 250 mΩ로 어림한 것이고 재지 않았다.
- **첫 4.2 A 단계에서 칩이 스스로 멈췄다**(직충전 시작 18초 뒤). 로그는 `Charge pump stopped by itself, flags 06 80 34 59 d0`이고, FLAG1~5(0x18~0x1C)의 뜻은 데이터시트가 없어 모른다. 드라이버가 벅으로 넘기고 1분 뒤 다시 켰고, 그 뒤로 4.2 A를 유지했다.
- **직충전 중 뽑기:** 펌프가 바로 멈추고 배터리로 넘어갔다. 커널 로그에 `Source stays in PPS: -95`가 한 번 남는다. **다시 꽂기:** 약 4초 만에 직충전이 다시 시작됐고 가짜 만충은 없었다.
- **UPower와 메뉴:** 직충전 중에는 "충전 중"으로만 뜨고 남은 시간이 나오지 않는다. `energy-rate`가 늘 0 W다. 펌프가 벅으로 넘긴 뒤(TCPM `online` 1)에는 11.7 W, 남은 시간 26.8분이 나오고 경고도 없다. 까닭은 UPower 1.91.5 쪽이다. PPS 중 TCPM의 `online`은 2인데(`sysfs-class-power` ABI의 Online Programmable), `up_device_supply_refresh_line_power()`가 이 값을 불리언 속성에 그대로 넣는다. GLib은 값을 버리고 경고하지만 `notify`는 보내서, 데몬이 배터리를 `UP_REFRESH_LINE_POWER`로 다시 읽고 `up_device_battery_report()`가 10초 동안 속도를 0으로 둔다. 저장된 값이 1에 머물러 PPS 요청이 바뀔 때마다 되풀이되므로 10초가 끝나지 않는다. **고침:** `bsp/lenovo-tb375fc/upower/`에서 0이 아니면 연결로 읽는다. 꾸러미 `buntos-upower`가 고친 데몬을 `/opt/upower`에 넣고 drop-in으로 띄운다. UPower 통합 시험에 PPS 시험을 더해 고치기 전에는 기기와 같은 경고와 `EnergyRate` 0, 고친 뒤에는 통과를 봤다(AC·배터리 시험 26건도 통과). 태블릿에 깔았고 벅 충전 중에는 속도와 남은 시간이 나온다. 실제 직충전 중에는 아직 못 봤다(다음 충전 때 본다).
- **못 본 것:** 펌프가 도는 중의 하드 리셋, 한 충전기 8시간 타이머, 이 설정으로 재부팅한 뒤의 동작.
- **기본값:** `bsp/lenovo-tb375fc/mkosi.extra/usr/lib/modprobe.d/90-buntos-tb375fc-charge.conf`가 `direct_charge=1`을 준다. 드라이버의 기본값은 그대로 꺼짐이다.

**만충 표시(패치 0178~0181, 2026-11-05, 커널 #303, 맥 PD 5 V 3 A).** 게이지는 끝에서 89%와 90%를 읽었다. 그 아래인 88을 골랐다. 표시 = 게이지 × 100 / 88이고 100에서 멈춘다(게이지 44 → 50, 73 → 83, 80 → 91, 87 → 99, 88 이상 → 100). 이 패치 뒤 `capacity`와 `charge-status`의 `soc=`는 표시 값이고, 이 절의 다른 SoC는 게이지 값이다. `charge_full`과 설계값은 88%로 줄이고, `charge_now`는 그대로 두되 `charge_full`을 넘지 않게 한다(쉰 뒤 8.86 Ah까지 읽혔다. 넘으면 UPower가 최대 용량을 올려 잡는다). 충전기와 펌프는 게이지의 전압, 전류, 온도만 읽어 충전 동작은 바뀌지 않는다. 사용자 공간 대역 시험(`.handoff/tb375fc-full-display/model`, 40개)은 통과했다.

패치 전과 뒤(둘 다 충전기는 `Full`, 4.325 V, 0 A, 게이지 값 90%):

| 값 | 패치 전 #298 | 패치 뒤 #303 |
|---|---|---|
| `capacity` | 90 | 100 |
| 게이지 `status` | `Charging` | `Full` |
| `charge_full`, 설계값 | 9.676 Ah, 10 Ah | 8.515 Ah, 8.8 Ah |
| `charge_now` | 8.859 Ah | 8.515 Ah(게이지 8.876 Ah에서 자름) |
| UPower 상태 | `charging` | `fully-charged` |
| UPower 숫자 | 90% | 100% |
| UPower 최대 용량 | 96.76% | 96.76% |

칩은 4.335 V, +0.31 A 뒤 끝냈고(08:46), 게이지 `status`는 그 자리에서 `Full`이 됐고 UPower는 30초 안에 따라왔다. UPower는 충전 끝을 다음 폴링에서 본다. 충전 드라이버가 칩의 끝을 알리지 않기 때문이다. 끝 전에도 숫자는 이미 100이다. 0170~0177을 넣은 커널의 첫 부팅이기도 하다: 재부팅 32초 뒤 USB SSH가 돌아왔고, 실패한 서비스가 없고, 새 `charge-up`으로 `upm6720-direct`와 TCPC가 올라 맥에서 PD 5 V 3 A를 받았다. apt로 39개를 올린 뒤의 첫 재부팅이기도 해서, 로그인 화면까지 뜨고 `apt-guard check`가 문제 없다고 했다.

1. [x] 끝까지 채우면 `mm8013/capacity` 100, `mm8013/status` `Full`, UPower `fully-charged`가 된다
2. [ ] 로그인한 뒤 상단바 메뉴에 "충전 완료됨"이 뜨는지 사용자가 본다
3. [x] 끝에 가까울 때 남은 시간이 나오지 않는다(전에는 3.4시간)
4. [x] UPower의 최대 용량이 전과 같다(96.76%). 설정 화면의 글자는 사용자가 본다
5. [ ] 뽑으면 `Discharging`으로 돌아오고 숫자가 100에서 천천히 내려오는지 본다
6. [ ] 다시 꽂을 때의 가짜 만충(게이지 73%)에서 `Full`이 보이지 않는지 본다. 표시 90% 아래면 충전기의 말을 믿지 않는다
7. [ ] 끝에서 게이지가 읽는 값을 몇 번 더 모은다(`i2cget -f -y 14 0x55 0x2c w`). 지금까지 89와 90이다. 88보다 낮게 끝나면 `Full`인데 100%가 아니다
8. [ ] 벽 충전기에서 이 커널로 9 V 계약을 받는지 본다. 지금까지 시험 모듈로만 봤다

단계 A와 B는 충전, C와 D는 TCPC다. 8절(VBUS)은 단계 D로 합쳐진다. D에서는 `usb_vbus_output`을 켜지 않는다.

**결과(2026-11-04, 사용자 동석, 전력계 없음, 배터리 82%).** A, B의 1·2·4번, C의 1·2·4·7번을 확인했다. i2c-mt65xx가 TCPC의 `vbus-supply`를 자기 공급으로 잡아 버스가 뜨지 못했다(패치 0148). TCPC는 VBUS 레귤레이터의 입력 때문에 `charge_pump_65`를 켜야 뜬다. 맥 포트에서 입력 1.5 A, 배터리 약 520 mA다. 70% 조건은 입력 0.5 A가 부하보다 작아 CV에 닿지 않으므로 넘겼다. 남은 것: B의 타이머, C의 케이블 뒤집기와 충전기 시험, 만충, D, E. 부팅 때 충전 드라이버를 올리는 결정도 남았다.

**결과 2(2026-11-04, 사용자 동석, 전력계 없음, 배터리 36~39%).** 케이블 양방향(`normal`, `reverse` 모두 SSH와 충전 정상), 켠 채 뽑았다 꽂기, 벽 충전기 2A 10분(C.5, C.6)을 봤다. F를 더했다. 남은 것: B의 타이머, C.8 만충, 배터리 온도 멈춤과 과전압의 실제 시험, 자동 적재. 충전 드라이버는 부팅 때 막아 둔 그대로다. 3단계 고속 충전 시험 때 다시 본다.

UFS 루트와 USB 가젯이 기기에서 된 뒤에 한다. 모든 시험은 **시간이 지나면 저절로 되돌아가는 배경 스크립트**로 하고, 결과는 UFS에 남긴다. 멈추면 전원 키 길게. 먼저 Android에서 100%까지 충전하고 `dumpsys battery`로 전압과 온도를 적어 둔다. 게이지 값과 견줄 기준이다.

#### A. 읽기만 (쓰기 0번)

8절 1단계와 같은 버스 확인을 끝낸 뒤.

```dts
&i2c5 { status = "okay"; };        /* 클럭 컨트롤러 &imp_iic_wrap_n/en과 함께 */
&mt6375 { status = "okay"; };
```

모듈: `mt6375`, `mt6375-charger`. 결과: `/sys/class/power_supply/mt6375-charger`(쓰기 불가), dmesg에 LK가 남긴 값 한 줄. 충전은 LK 상태 그대로다.

1. [ ] `modprobe mt6375 mt6375-charger`. dmesg 한 줄이 LK가 남긴 값이다: `CV .. ICHG .. AICR .. CHG_EN .. BUCK_EN .. HZ .. WDT_EN .. BAT_COMP ..`.
2. [ ] 전후 레지스터가 같아야 한다.

    ```bash
    for r in 0x20 0x21 0x22 0x23 0x24 0x25 0x26 0x27 0x28 0x29 0x2a 0x33 0x34 0x50 0xe0 0xe1 0xe4; do echo "$r $(i2cget -f -y 5 0x34 $r)"; done
    ```

    `0x33`(IR 보상)은 dmesg의 `BAT_COMP`와 같아야 한다. bit 2:0이 32 mV 단위의 한계, bit 6:4가 16.7 mOhm 단위의 저항이고 순정은 둘 다 1이다(`0x11`). `값 & 0x77`이 `0x00`이나 `0x11`이면 된다(bit 7과 bit 3의 뜻은 자료에 없다). 더 크면 멈추고 알린다. 충전 전압이 CV 위로 전류에 따라 더 오른다(같은 계열 칩의 공식. 순정 값이면 2 A에서 32 mV, 500 mA에서 약 8 mV). 드라이버는 이 레지스터에 쓰지 않는다.

3. [ ] 볼 값: AICR·ICHG가 0.5 A인가, CV가 얼마인가(4.35 V면 LK2와 같다. 넘겨받기 전에는 이 값으로 충전된다), `0x27` bit 7(타이머), `0x2a` bit 3(워치독)과 bit 1:0(주기: 0부터 8, 40, 80, 160초). 워치독이 켜져 있으면 곁에 있으면서 그 주기보다 오래 두고(모르면 160초가 넘게) 값이 바뀌는지 본다. 만료 때 값이 칩 기본값으로 바뀔 수 있고(같은 계열 칩의 자료, 추정) A에는 고칠 드라이버가 없다. **이것이 칩 기본값을 알 유일한 기회다.**
4. [ ] I2C7을 켰으면 게이지의 만충 값을 읽는다: `i2cget -f -y 7 0x55 0x30 w`. Android를 거치고 재부팅한 뒤에도 4310(`0x10d6`)인지, 전원을 껐다 켜도 남는지 본다.
5. [ ] `online`, `status`가 케이블에 따라 맞게 바뀌는지 본다. 쓰기 파일은 모두 읽기 전용이어야 한다.
6. [ ] 부하 전력: PC에 꽂고 USB-C 전력계로 입력을 잰다. 500 mA에 붙어 있으면 배터리가 줄고 있다. 5.1절의 추정을 이 값으로 고친다.

#### B. 충전 넘겨받기

게이지(패치 0079와 `&i2c7`, 9.4절 6번)가 기기에서 돈 뒤.

```dts
&battery { status = "okay"; };
&mt6375 {
	charger {
		monitored-battery = <&battery>;
		mediatek,fuel-gauge = <&fuel_gauge>;
	};
};
```

결과: 온도를 따라 충전을 끄고 켠다. 입력과 충전 전류는 500 mA 그대로다(C 전에는 포트 정보가 없다). CV는 4.34 V가 된다(45도 위와 충전을 못 할 때는 4.24 V).

**넘겨받기 전.** 게이지를 못 읽으면 dmesg에 `Cannot read the fuel gauge`가 한 번 찍히고 LK 상태 그대로다. 값이 말이 안 되면(−40도 밑, 100도 위, 2.5 V 밑, 5 V 위, 온도만 읽히고 전압을 못 읽음) 세 번째 폴링에 `Fuel gauge makes no sense`가 찍히고 `CHG_EN`만 0이 된다. 좋은 값이 오면 평소처럼 넘겨받는다. 넘겨받으면 `Took over charging`이 찍힌다. 넘겨받기 전에 잠들면 LK 상태로 충전이 계속된다. 다만 멈출 까닭이 있으면 끄고, 못 끄면 잠들지 않는다.

**조건:** 게이지가 돌고 온도가 방 온도와 맞을 것, 전압이 Android 값과 맞을 것. 배터리 30~70%(CV 구간을 피한다). 전력계를 끼우고, 사람이 곁에 있고, 태블릿을 단단한 바닥에 둔다. 부스트는 꺼 둔다.

**재는 것:** VBUS, IBUS(전력계), 배터리 전압·전류·온도(게이지), `status`, `health`, 레지스터. 뒷면 온도를 손으로 본다.

**멈추는 길(먼저 시험한다):** `echo inhibit-charge > /sys/class/power_supply/mt6375-charger/charge_behaviour`. 넘겨받기 전에도 바로 듣고, 쓰기가 실패하면 오류를 돌려준다. `echo`의 성공만 믿지 않는다. `status`가 `Not charging`인지, `i2cget -f -y 5 0x34 0x20`의 bit 0이 0인지, 전력계의 입력이 시스템 부하만큼으로 줄었는지 본다. 그다음 `auto`로 되돌린다. 넘겨받기 전이면 LK가 둔 값으로 돌아간다. 다른 길: 케이블을 뽑는다, 전원 키 길게. **`rmmod mt6375-charger`는 멈추는 길이 아니다.** 0.5 A로 내리고 감시만 끝나며, 충전은 LK 상태처럼 계속된다.

1. [ ] B를 더한 dtb로 모듈을 막아 부팅한다. `mt6375-charger`는 장치 트리 별칭으로 저절로 올라오고 게이지는 내장이라, 막지 않으면 무엇을 걸기 전에 넘겨받는다. 커널 명령줄의 `modprobe.blacklist=mt6375_charger`나 `/etc/modprobe.d/`의 `blacklist mt6375_charger`로 막는다. 레지스터를 기록한다.
2. [ ] 10분 뒤 멈추는 스크립트를 걸고 나서 모듈을 올린다. 넘겨받은 뒤 레지스터를 전후로 본다.

    ```bash
    P=/sys/class/power_supply/mt6375-charger
    ( sleep 600; echo inhibit-charge > $P/charge_behaviour; echo "rc=$?"; cat $P/status; i2cget -f -y 5 0x34 0x20 ) > /var/log/b-stop.txt 2>&1 &
    modprobe mt6375-charger
    ```

3. [ ] 전류가 500 mA 안에 머무는지 본다.
4. [ ] 게이지를 멈추었을 때(`/sys/bus/i2c/drivers/mm8013/unbind`) 40초 안에 `CHG_EN`이 0이 되는지 본다. 세 번째 실패까지 폴링 간격에 따라 30초가 걸린다. 다시 바인드하면 돌아와야 한다. 언바인드 순간에 전원 장치 코어의 경고가 한 번 보일 수 있다(폴링이 게이지를 읽는 사이의 틈, 창은 몇 ms). 그 틈에서는 드물게 게이지의 regmap을 해제한 뒤에 접근해 커널이 멈추거나 메모리가 깨질 수도 있다. `power_supply_unregister`가 진행 중인 읽기를 기다리지 않는 전원 장치 코어의 한계라 드라이버로 막지 못한다. 먼저 `sync`를 하고, dmesg에 경고가 보이면 재부팅한 뒤 다음 시험으로 간다. C.4의 `rmmod`도 같다.
5. [ ] 10시간 타이머를 본다. 넘겨받을 때 이미 칩이 고장 상태여도 같은 걸쇠가 선다. 500 mA로는 만충까지 못 간다고 본다: 10,200 mAh라 30%에서 만충까지 부하 없이 14시간이 넘고, 부하 1 W면 70%에서 시작해도 10시간을 넘는다(2.5 W 입력, 효율 90%로 어림). AICR 구간에서 타이머가 절반 속도라면(같은 계열 칩의 자료) 20시간이다. 칩이 멈추면 dmesg에 `Charger fault, stopping until the source goes`, `status`는 `Not charging`, `health`는 `Unspecified failure`다. **고장이 아니라 기대 결과다.** `inhibit-charge` 뒤 `auto`, 온도 변화, 절전 복귀로 풀리지 않는지 본다. 케이블을 10초 넘게 뽑았다 꽂으면 다시 충전되어야 한다(C 전에는 폴링으로 보므로 10초보다 짧으면 못 본다). 만충 시험은 C.8로 옮겼다.

#### C. 인터럽트와 TCPC, 싱크만

```dts
&mt6375 {
	interrupt-parent = <&pio>;
	interrupts = <7 IRQ_TYPE_LEVEL_LOW>;

	charger {
		mediatek,usb-port = <&mt6375_tcpc>;
	};
};
&mt6375_tcpc { status = "okay"; };
```

모듈을 더한다: `typec`, `tcpm`, `tcpci`, `tcpci_mt6375`(조각에 한 줄). 결과: 프로브가 칩 인터럽트를 모두 가리고 TCPC를 초기화한다. CC는 Rd 그대로이고 Rp를 내지 않는다. 포트가 1.5A나 3A를 알리면 입력이 올라간다(상한 2A). `usb-role-switch`가 없어 가젯은 건드리지 않는다.

1. [ ] **먼저 읽기만:** PC를 꽂은 채 `i2cget -f -y 5 0x4e 0x1e`와 `0x9f`, `0x1a`, `0x1d`. `0x1e` bit 2(VBUS_PRES)가 1이고 빼면 0인가? `0x9f` bit 5와 같은가? **다르면 멈춘다.** 글루에 벤더 레지스터를 읽는 길을 더해야 한다.
2. [ ] ID: `0x00`~`0x03`이 `cf 29 75 63`이어야 한다. 다르면 프로브가 실패하고 쓰기는 0번이다.
3. [ ] 인터럽트를 켠 dtb로 부팅해 가젯이 그대로인지 본다(마스크만 쓴다). GPIO7 인터럽트 수가 멈춰 있어야 한다(`/proc/interrupts`).
4. [ ] PC를 꽂은 채 시간 제한으로 올린다: `( sleep 20; modprobe tcpci_mt6375; sleep 60; rmmod tcpci_mt6375 ) &`. SSH가 끊기는지 본다.
5. [ ] **케이블을 뒤집어 같은 시험을 한다.** TCPM이 잠깐 CC2를 연다. PC가 VBUS를 내리면 멈추고 보고한다.
6. [ ] **처음으로 2 A가 되는 단계다. B의 조건을 다시 갖춘다:** 전력계를 끼우고, 사람이 곁에 있고, 단단한 바닥에 두고, 뒷면 온도를 손으로 본다. USB 커넥터 둘레의 온도도 본다. 커넥터 과열 보호는 아래 F다. 뜨거워지면 바로 멈추고 알린다. `/sys/class/typec/port0`, `typec-power-opmode`, 충전기의 입력 값. 1.5A·3A 충전기로 입력이 오르는지, 뽑으면 바로 0.5 A로 돌아오는지 전력계로 본다.
7. [ ] `rmmod` 뒤 `0x1a`가 `0x0a`이고 가젯이 그대로인지, 재부팅 뒤 fastboot가 PC에 보이는지 본다.
8. [ ] 만충까지 한 번, 곁에서(B.5에서 옮겼다). 6번의 조건으로 2 A 충전기를 쓴다. 게이지 전압이 Android 실측(4326 mV)처럼 4340 mV 밑에서 멈추고 `Full`이 되는지, 4.41 V 멈춤이 잘못 걸리지 않는지(dmesg `Battery at .. uV, stopping charging`) 본다. 걸리면 임계값을 고친다. 게이지가 범위 안의 틀린 값을 낸 것인지도 함께 본다(풀리려면 4.14 V 밑이 세 번 이어져야 한다). 그때 게이지 잔량이 100%인지도 본다(게이지 만충 값은 Android가 쓴 값이다).

#### D. DRP와 호스트

```dts
&mt6375_tcpc {
	connector {
		data-role = "dual";
		power-role = "dual";
		try-power-role = "sink";

		ports {
			#address-cells = <1>;
			#size-cells = <0>;

			port@0 {
				reg = <0>;

				typec_hs: endpoint {
					remote-endpoint = <&usb_role_sw>;
				};
			};
		};
	};
};

&ssusb {
	dr_mode = "otg";
	usb-role-switch;
	role-switch-default-mode = "peripheral";	/* 빼면 TCPM의 역할 없음이 호스트가 된다 */

	port {
		usb_role_sw: endpoint {
			remote-endpoint = <&typec_hs>;
		};
	};
};

&xhci0 { status = "okay"; };
```

`&usb_vbus_output`은 꺼 둔다(두 소비자가 부스트를 켜지 않게). 부스트와 FET는 TCPM이 켠다. 8절 4단계를 TCPM 길로 한다. 차례: 가젯이 그대로인지 → 브레이크아웃으로 5 V와 뺀 뒤 0 V → 작은 USB 메모리 → 켠 채 재부팅(부스트 꺼짐, CC Rd). 켠 채 PC를 꽂으면 TCPM이 어떻게 하는지 본다(Try.SNK). VCONN은 Ra가 있는 케이블에서 처음 나간다. 커넥터 온도를 본다. 이 차례에서 처음으로 부트 직후 호스트 역할을 거칠 수 있다(otg 프로브 동작). `role-switch-default-mode`가 꼭 있어야 한다.

부스트 중에 `inhibit-charge`를 걸어 VBUS가 그대로인지 본다. `CHG_EN`은 벤더의 부스트 값에 없어 영향이 없을 것으로 보지만, 확인한 적은 없다. 그다음 `auto`로 되돌린다. 부스트가 끝난 뒤 다음 폴링에 반영된다.

부스트 중 절전에서 VBUS가 그대로인지 본다. 깨우는 길을 정한 뒤에 한다. 드라이버는 부스트 중에도 절전에 들 때 `CHG_EN`을 0으로 쓴다(`i2cget -f -y 5 0x34 0x20`의 bit 0). 부스트가 잠든 사이에 끝나도 감시 없이 충전하지 않게 하려는 것이다. 한계와 부스트 값은 쓰지 않는다. 깨어난 뒤에도 부스트가 끝날 때까지 0이고, 끝나면 다음 폴링에 1로 돌아와야 한다. `CHG_EN`은 벤더의 부스트 값에 없어 VBUS에 영향이 없을 것으로 보지만, 확인한 적은 없다.

#### E. PD

`pd-disable`과 `typec-power-opmode`를 빼고 넣는다.

```dts
source-pdos = <PDO_FIXED(5000, 1500, PDO_FIXED_USB_COMM)>;   /* pd,source-pdo-data */
sink-pdos = <PDO_FIXED(5000, 2000, PDO_FIXED_USB_COMM)>;     /* pd,sink-pdo-data */
op-sink-microwatt = <10000000>;
```

`dt-bindings/usb/pd.h`를 보드에 더한다. 5 V만 받으니 고속 충전이 아니다. 협상이 실패하면 하드 리셋으로 상대가 VBUS를 내리므로 D가 안정된 뒤에 한다. 전압을 올리는 일은 차지 펌프 충전과 함께 3단계에서 정한다.

D가 안정된 뒤 5 V 충전기와 PC에서 한 번씩. 하드 리셋이 나는지 `tcpm` 로그를 본다.

#### F. USB 커넥터 과열 보호

패치 0098·0099, 0150. 시험 층에 넣었고 순정의 `usb_temp_protect`와 같은 센서와 GPIO를 쓴다.

```dts
&mt6363_adc { status = "okay"; };
&usb_conn_ntc { status = "okay"; };
&mt6375 {
	charger {
		mediatek,connector-thermal-zone = "usb-conn-thermal";
		mediatek,vbus-protect-gpios = <&pio 179 GPIO_ACTIVE_HIGH>;
	};
};
```

**먼저 할 일:** `mt6359-auxadc.ko`에 0150이 들었는지 md5로 맞춘다. 고치기 전의 모듈은 VIN을 읽을 때마다 MT6363 `0x0005`에 쓰려 한다. 표에 `SDMADC_CON0`이 없어서다.

결과(2026-11-04):

1. [x] `in_voltage12_raw`가 방 온도에서 약 17300이고 구역 온도가 게이지와 맞는다(21.6도, 23.1도). 손으로 쥐면 오르고 배터리는 그대로다.
2. [x] `emul_temp`에 -274000(못 읽음), -40000(센서 끊김)을 넣으면 10초 안에 입력 500 mA, 0을 넣으면 돌아온다.
3. [x] `emul_temp`에 80000을 넣으면 `USB connector at 80000 mC, cutting the input`, GPIO179 high, `CHG_EN` 0, 입력 100 mA, `Overheat`. VBUS가 충전기와 TCPC에서 사라진다. 0을 넣으면 낮은 값 3번 뒤 돌아온다. 맥 포트에서는 하지 않는다. GPIO179가 단락이라면 맥 포트에 무리가 간다.
4. [x] 벽 충전기 2A 10분: 커넥터 23.5도, 배터리 23.5도.
5. [ ] 실제로 75도에 닿는 시험은 하지 않았다.

**이상과 종결.** 둘째 차단 시험에서 GPIO179를 올린 뒤 19초에 CC가 14초 끊기고, 그동안 커넥터가 23도에서 33도로 올랐다가 1분 안에 돌아왔다. 첫 시험에는 없던 일이고 사용자는 손댄 기억이 없다. 그 뒤 충전기가 CC를 잡은 채 VBUS를 주지 않았고, 20분 뒤 다시 꽂았을 때도 GPIO179가 low인데 VBUS가 없었다. 재현 시험은 충전기가 VBUS를 주지 않아 돌지 못했다. 구형 케이블 문제로 보고 종결했다(사용자 판단). 확정한 것은 아니고, 같은 케이블로 2A 10분은 문제없었다. 3단계 고속 충전 시험 때 다른 케이블로 차단 시험을 다시 돌려 본다.

### 9.4 2e GPU·열·게이지

패치 0070~0079다.

**결과(2026-11-04).** 0~4번과 6번을 기기에서 봤다. 0번의 값은 표와 같다. 하드웨어 리셋은 켜진 것으로 본다. 다만 AP 셋째 컨트롤러는 펌웨어가 오프셋을 남겨 미뤄져 있어 경고만 남기고 쓴다(패치 0146). GPU·APU 구역은 전원이 켜질 때까지 조용히 기다린다(패치 0147). 냉각 장치가 없으면 전 코어 부하에서 5초에 93도라 CPU와 GPU에 passive 85도를 넣었다. 커뮤니티 포트가 멈춘 GPU 첫 전원은 문제없이 지났다. Mesa는 26.3.0-rc3으로 렌더링 결과까지 맞았다. 컴포지터와 서스펜드(7번)는 아직이다. CPU 유휴 상태는 PSCI 거절로 뺐다가, MCUPM에 시간 기준을 주어 켰다(2026-11-07, 패치 0193~0195).

코드, 벤더 자료, 모의 시험으로 짠 차례다. 아래는 기기에서만 알 수 있는 것이다. 각 단계는 앞 단계가 맞을 때만 넘어간다. 노드를 켜는 차례 자체가 안전장치다. 첫 판단 거리는 하드웨어 열 리셋이 켜져 있는지다(1번). 게이지(6번)는 9.3절 B보다 먼저다.

위험: 부팅 불가가 아니라 그 자리 멈춤이다. 커뮤니티 포트는 GPU 첫 전원 근처에서 SoC가 멈췄다는 보고가 있다(포럼 보고). 되돌리는 길은 전원 키 길게와 그 앞 단계의 dtb다.

#### 0. 켜기 전, 읽기만

언락 전 안드로이드에서 구역 온도를 적어 둔다(읽을 수 있으면). 사용자 기기의 셸은 막혔다(실측, 2026-11-02). HAL이 주는 값(`adb shell dumpsys thermalservice`)은 [실측 보고서](../hw/lenovo-tb375fc.md) 7절에 적었고, 구역별 값은 4절 10번에서 root로 읽는다.

```bash
adb shell 'for z in /sys/class/thermal/thermal_zone*; do echo $(cat $z/type) $(cat $z/temp); done'
```

언락 뒤 root나 첫 리눅스의 initramfs에서 `busybox devmem`으로, **이 차례대로** 읽는다.

| 주소 | 무엇 | 기대 |
|---|---|---|
| `0x1c001f00` | `SPM_MFG0_PWR_CON` | [31:30]=3, bit 12=0, bit 13=1. **아니면 아래를 읽지 않는다** |
| `0x13c2fd24` | GPR2(`GPUEB_INIT_FOOTPRINT`) | `0x55667788`. 벤더 커널이 읽지 않는 값이라 부팅 뒤에 바뀌었을 수 있다. 다르면 값을 적고, 프로브 검사를 고칠지 본다 |
| `0x13c2fd50` | GPR13(IPI 매직) | 0이 아님. 벤더 안드로이드에서는 읽고 지워서 0일 수 있다. 리눅스에서는 값이 있어야 한다 |
| `0x13c60618` | `GPUEB_CFGREG_WDT_CON` | 포럼 보고 `0x800f0000` |
| `/proc/device-tree/soc/gpueb@*/gpueb_mem_addr`·`_size` | LK가 적은 공유 메모리 | 크기 `0x180000`, 주소가 `reserved-memory/mblock-*-me_GPUEB_SHARED`의 `reg`와 같다 |
| `/proc/device-tree/chosen/atag,devinfo` | 퓨즈 사본 | 첫 낱말 320(0x500바이트). `0x310` 낱말의 [7:0]과 [15:8]이 보통 60, 170 근처 |

`gpueb` 노드가 켜지지 않아도 위 속성은 있어야 한다(LK는 상태를 보지 않는다).

#### 1. 열: `efuse`, `lvts_ap`, `lvts_mcu`

쓰기가 없으니 가장 위험이 작다.

- 기록: `controller at 0x…: normal calibration, golden N, MONCTL0 …, PROTCTL …, PROTTC …, MONINT …, ID …`. MCU는 고온 보정일 수 있다.
- **이 기록으로 하드웨어 리셋을 판단한다.** `PROTCTL[19:16]`(전략과 센서)이 0이 아니고 `PROTTC`가 119도의 raw 근처이며 `MONINT` bit 31(`STAGE3_INT_EN`)이 서 있으면 켜져 있을 가능성이 높다. 모두 0이면 꺼진 것으로 본다. 그러면 폴링 1초의 소프트웨어 critical(115도, 우리 값이다)이 유일한 보호이고 쿨링 장치도 아직 없다. 여기서 멈추고, 그대로 둘지 커널이 `PROTTC`를 써서 켜는 패치를 낼지 정한다(벤더 `set_tc_hw_reboot_threshold`가 그 차례를 보여 준다). 값을 적어 둔다.
- `Bad golden`, `Bad calibration count`, `is not clocked`, `does not measure this sensor`, `adds an offset` 중 하나로 도메인이 거절되면 **그 도메인은 보호가 없다. 부하를 걸지 말고** 값을 본다.
- `fuse count … firmware has …`가 나오면 멈추고 본다. 펌웨어의 `LVTS_EDATA`가 퓨즈 count와 같으리라는 추정이 틀린 것이라 계수를 어느 쪽으로 만들지 다시 봐야 한다. 전부 같으면 이 경고를 `-ENODATA` 오류로 올린다. 퓨즈가 맞는 자리에서 왔다는 가장 좋은 증거다.
- `… is not usable: …`가 나오면 `… is usable again`이 나올 때까지 그 센서의 보호가 없는 것이다. APU·GPU 도메인에서 전원에 따라 raw 0이 오가면 여기서 보인다.
- `/sys/class/thermal/*/temp`가 실온 + 약간인지, 안드로이드에서 적은 값과 게이지 온도에 견주어 10도 안쪽인지 본다. 범위 안의 틀린 golden은 드라이버가 잡지 못한다.
- **가열 시험:** `stress`나 `dd`로 CPU를 돌려 CPU 구역이 **오르는지**, 멈추면 내리는지 본다. 내리면 식이 뒤집힌 것이니 바로 끈다.
- 디버그 파일(`/sys/kernel/debug/*thermal-sensor/controller*/registers`)에서 `LVTS_MSR*`의 bit 16과, 값이 시간에 따라 움직이는지(멈춰 있지 않은지) 본다.

#### 2. `lvts_apu`, `lvts_gpu`

버스가 멈추면 이 단계다. 벤더는 전원 상태를 보지 않고 두 도메인을 읽지만 `0x13ff0000`은 MFG, `0x19004000`은 APU 쪽이다. GPU가 꺼져 있을 때도 읽히는지 본다.

#### 3. GPUEB: `gpueb_mbox`, `gpueb`

GPU 전원은 요청하지 않는다. 기대 기록:

```text
shared memory at [mem …-…]
GPUEB is running
giving the shared memory to the GPUEB
65 GPU OPPs, 0 stack OPPs, shader cores 0x…
```

- OPP 수와 코어 마스크가 벤더 표와 맞는가(65, `0x130013`). 레노버 변종이면 다를 수 있다. `Unexpected shader core mask`나 `EB did not initialise`면 배치가 다른 것이다.
- `Dropped a message on channel …`이 나오면 그 채널을 적는다. 벤더 기준으로는 AP가 알리기 전까지 이벤트를 보내지 않는다(`GPUFDVFS_IPI_SET_GED_READY`). 자주 나오면 버리지 말고 받아야 할 메시지인지 본다.

#### 4. GPU: `gpu`

`panthor`는 모듈이니 자동으로 올리지 말고 손으로 올린다(`modprobe.blacklist=panthor` 뒤 `modprobe panthor`). 커뮤니티의 멈춤은 이 근처다.

- 기대: `asking the GPUEB to power the GPU` → `the GPU is powered` → `mali-g615 id …` → CSF 펌웨어 로드.
- 코어 전원 전이가 시간 초과면 PDCA를 의심한다. 벤더 Mali에 리셋 뒤 `0x13c620a8` bit 16을 기다리는 길이 있다(`gpufreq_pdca_polling_ack`, `CONFIG_MALI_MTK_MFG2_BACKDOOR_ACK`일 때). 이 SoC에서 켜져 있는지는 설정으로 확인하지 못했다.
- `MFG0 is not on`으로 전원이 거절되면 그 값을 적는다. 드라이버는 IPI마다 보고, 걸리면 다음 부팅까지 GPU가 없다. 첫 켜기 뒤에 bit 13이 평소에도 내려가는 칩이면 확인을 줄여야 한다.
- GPU 메모리 문제가 보이면 `dma-coherent`와 ASN hash를 본다.

#### 5. Mesa

26.3에서 GLES로 컴포지터.

#### 6. 게이지: `i2c7`

`/sys/class/power_supply/mm8013/`의 `voltage_now`, `capacity`, `temp`를 본다. 첫 부팅에서 드라이버는 게이지에 쓰지 않는다.

- 전압을 스무 번 읽어 윗바이트가 0x00·0xFF로 튀는지 본다. 튀면 4 ms로 모자라거나 어댑터(`i2c-mt65xx` DMA)다. `current_now`의 부호도 충전기를 꽂아 본다. 배터리 ID가 셋 밖이면 `MM8013 not found`다.
- `charge_full`, `charge_full_design`, `status`는 순정이 읽지 않던 레지스터에서 온다. 값이 말이 되는지 본다.
- 게이지의 충전 전압: `i2cget -f -y 7 0x55 0x30 w`. Android를 거치고 재부팅한 뒤, 전원을 껐다 켠 뒤에도 4310(`0x10d6`)인지 본다(9.3절 A 4번).
- 충전이 끝날 때까지 두고 읽는다: `0x2c`(RSOC), `0x0a`(FLAGS, bit 9가 FC), `0x0e`(FCC), `0x3c`(설계), `0x30`. RSOC가 100에 닿는지, 꽂아 둔 채 내려가는지 본다.
- **쓰기 시험(사람이 확인한 뒤에만).** `0x30`에 순정 표의 다른 값 4530(`0x11b2`)을 쓰고 `0x2c`·`0x0e`가 움직이는지 본 뒤, 4310으로 되돌린다. 순정이 쓰는 값이지만 게이지 안에서 무엇이 바뀌는지는 모른다. 움직이지 않으면 게이지가 이 값을 만충 판단에 쓴다는 추정이 틀린 것이다. 이 값을 커널에서 쓸지는 그 뒤에 정한다.

#### 7. 서스펜드

QEMU에서 본 것은 드라이버의 차례뿐이다. 실제 SoC에서는 저전력 펌웨어(SPM)가 MFG0과 GPUEB를 어떻게 다루는지 모른다. GPU를 켠 채로 s2idle과 복귀를 본다.

- `power_control failed`, `GPUEB did not answer`, `Giving up on the GPUEB`가 없어야 한다. 복귀가 늦으면(10초) 응답이 안 온 것이다.
- `MFG0 is not on`이 나오는지, 복귀 뒤 GPU를 다시 쓸 수 있는지, LVTS 값이 움직이는지 본다.

#### 멈췄을 때 볼 것

- 마지막 `dev_info`가 단계다. 켠 뒤 멈추면 그 앞 단계의 노드만 켠다.
- 살아 있으면 읽을 것(MFG0 확인 뒤): `0x13c2fd30` LOW_POWER_FOOTPRINT, `0x13c2fd60` GPUFREQ_FOOTPRINT, `0x13c2fd5c` DRAM_RES, `0x13c6071c`·`720`·`724` GPUEB PC·LR·SP, `0x13c62000` 메일박스 TX, `0x13c62078` RX, `0x13f91200` `MFG_RPC_PWR_CON_STATUS`.
- 벤더의 `gpueb_dump_status()`가 보는 것과 같다. **`0x13fbf000`(MFG 설정)과 `0x13000000`(GPU)은 MFG1이 켜지기 전에 읽으면 안 된다.**
- 리셋되어 다시 부팅되면 pstore와 LK의 `SYS_GPUEB_LAST_LOG`·`SYS_GPU_MFGSYS_LATCH_LOG`(expdb)를 본다.
- `GPUEB did not answer`(10초)와 `Giving up on the GPUEB`면 GPUEB가 멈춘 것이다. 그 뒤로 드라이버는 말을 걸지 않으니 다음 부팅까지 GPU는 없다. `CMD_POWER_CONTROL` 뒤의 시간 초과는 전원 길이 문제이고, `GPUFREQ_FOOTPRINT` 값이 어디까지 갔는지 말해 준다(벤더 `__gpufreq_footprint_power_step`).

### 9.5 2c 2차 화면 파이프라인

**결과(2026-11-04).** 기기가 mediatek-drm으로 부팅해 로그인과 세션까지 뜨고, CPU 화면 복사가 없어졌다. 사람이 사진과 눈으로 확인했다(양쪽 절반이 맞고, 가로줄과 어긋남과 왼쪽 검은 줄이 없다). 시험 패치 0999는 필요 없어졌다. 복사가 없다는 근거: simpledrm 장치가 사라지고, 컴포지터가 GPU로 그린 선형 버퍼 둘이 CMA에 있고, OVL 주소 레지스터가 그 물리 주소를 가리킨다.

| 항목(로그인 화면의 키보드, 표본 하나) | simpledrm + 0999 | mediatek-drm |
|---|---|---|
| 커밋 간격 중앙값 | 약 65 ms | 약 7 ms |
| fps | 15.6~18.6 | 68~128 |
| 커밋 하나당 kworker 시간 | 약 62 ms | 약 2 ms |
| vsync | 없음 | 있음, 143.88 Hz. 119.34, 59.67, 29.83 Hz도 돈다 |

- 부팅 순서: `buntos-display.service`(PHY, mtk-mmsys, 패널, mediatek-drm 차례로 올리고 DSI 커넥터를 기다린다) → `buntos-gpu.service`(panthor) → 로그인 화면 → `buntos-wifi.service`, `buntos-charge.service`. 드라이버는 명령줄의 `modprobe.blacklist`로 자동 적재를 막고 서비스가 올린다. 전력 시험 전이라 부팅 때 켜는 값은 시험 커널 기준이다.
- 고친 것: 패치 0153(CRTC를 끄면 OVL 도메인의 투표만 지워지고 전원 끄기가 끝나지 않아, 다시 켠 뒤 기기가 멈췄다. 한 번 봤고 원인은 추정이다. 화면 전원 도메인을 끄지 않게 한 임시 고침이다). 컴포지터는 커넥터가 있는 카드를 고른다.
- 달라진 것: 커넥터가 `DSI-1`이라 내장 화면으로 계산해 UI 배율이 2.5에서 2.25가 됐다.
- 남은 것: 화면 전원 도메인을 실제로 끄는 길(suspend와 대기 전력이 여기서 막힌다), SM5109 전압(아래 6단계의 3번), 커서 평면, 앱 버퍼마다 뜨는 `swiotlb` 경고(화면에는 영향이 없다).
- 로그인과 로그아웃: 컴포지터끼리 CRTC를 끄지 않고 넘긴다(buntOS main의 `comp/src/handover.rs`). 옛 판은 로그인 때 0.50~0.60초, 로그아웃 때 1.30~1.41초 꺼졌고, 고친 판은 전환 넷 내내 `active=1`, 모드 변경 0번이다. 측정을 켠 채 잰 값이고, 패널을 눈으로 본 결과는 아직 없다.
- 밝기: 패널의 DCS `0x51`이 아니라 SoC의 `disp_pwm0`(GPIO131)이 정한다. 순정 트리(`mediatek,pwm-leds`, 모드 5)와 LK 기록(`brightness set by disp_pwm`)이 같고, 듀티를 손으로 바꾸니 화면 전체가 고르게 변했다. SGM37604A 둘은 LK가 맞춘 `I2C 코드 x PWM` 모드(`0x11=0x75`, 코드 `0xdb6`, 듀티 100%에서 줄마다 21.4 mA)로 PWM 하나를 함께 따르므로 드라이버가 필요 없다. 패치 0154·0155와 `pwm-backlight` 노드로 `/sys/class/backlight/backlight`(0~100, CIE 1931)가 생겼다. 사람이 눈으로 확인했다: 고르게 변하고 깜빡임이 없다, 레벨 1은 미세하게 보인다, LED를 끄면 어두운 방에서 빛샘이 없다, 설정 앱과 제어 센터의 슬라이더가 듣는다.
- 시험 안 한 것: 긴 시간 사용, 전력, 144 Hz 언더런을 레지스터로 보기. suspend는 9.7절이다. 화면 전원 도메인을 끄지 않아야 돌아온다(패치 0188).

아래는 코드와 자료로 짠 차례다. 위 결과에 이르기까지 기기에서 차례로 돌렸다. 위에서 아래로 하고, 앞 단계를 채운 뒤 다음으로 간다. 켜지 못해도 앞 단계의 dtb로 돌아가면 simple-framebuffer 데스크톱이 그대로다.

> **위험: 멈춤.** 전원이나 클럭이 꺼진 화면 블록을 건드리면 버스가 멈춘다. 전원 키와 볼륨(홈) 키를 길게 눌러 다시 켠다(순정 트리의 PMIC 초기화). 명령줄의 블랙리스트 때문에 다시 켜면 셸까지 온다.

공통 준비: USB 가젯 셸과 pstore를 먼저 둔다. 명령줄에 `modprobe.blacklist=mediatek_drm`을 넣어 4단계까지 드라이버가 저절로 올라오지 않게 한다. `clk_ignore_unused`와 `pd_ignore_unused`는 그대로 둔다.

#### 0. 읽기 (드라이버 없음, 쓰기 없음)

LK가 화면을 켠 채로 `tools/hw/read-display-regs.sh > lk-regs.txt`를 돌린다(SPM 전원이 켜지고 게이트가 열린 블록만 읽는다). `mmpll`을 잡아 둔 커널에서 돌린다(6.2절). 없으면 UFS가 클럭을 내린 뒤에 읽는 순간 버스가 멈춘다. 결과를 저장소 밖에 둔다. 2026-11-04에 UFS까지 켠 시험 커널로 한 번 떠 두었다(502칸).

1. [x] 첫 줄 `PWR_CON` 넷의 ACK(bit 30, 31)가 선다(2026-11-04. 넷 다 `0xC200224D`). 안 서면 LK가 화면을 켜지 않은 부팅이다. 다음으로 가지 않는다.
2. [ ] `mutex0`, `mutex1`, `ovlmutex0`, `ovlmutex1`의 `0x20`이 1이고, 같은 번호에 모듈과 SOF가 있다. LK가 0번 뮤텍스를 쓰지 않으면 어느 번호인지 적는다(드라이버는 자기가 쓰는 번호만 지운다. 다른 번호가 켜져 있으면 고쳐야 한다).
3. [ ] LK 경로가 코드에서 읽은 것과 같은지 본다. 글로벌판 LK(`bl2_ext`의 경로표) 값이다.
    - 두 `dispsys`: `0x34`=1, `0xf48`=2, `0xf4c`=0, `0xf60`=0, `0xf74`=5, `0xf98`=`0x11`, `0xfa4`=`0x20`, `0xea4`=1, `0xf0c`=1.
    - 두 `ovlsys`: `0xf08`=`0x1a`, `0xf10`=2, `0xf14`=4, `0xf38`=0, `0xf40`=1.
    - **같으면** 패치 0124의 표가 그 값을 지운다. 다른 칸에 값이 있으면(중국판 펌웨어 등) 적고, 4단계 전에 표에 지우는 줄을 더한다. 전원이 그대로인 채 켜면 남은 비트가 경로를 둘로 몰아 프레임이 멈춘다.
    - **이 값으로는 패치의 경로를 확인할 수 없다.** LK는 PQ를 지나는 크로스바 0을 쓰고, 패치는 PQ를 건너뛰는 크로스바 1을 쓴다(벤더의 `DRM_BYPASS_PQ`). 왼쪽은 벤더 코드에 있고 오른쪽은 왼쪽에서 유추했다. 확인은 4단계의 첫 프레임이다.
4. [ ] `dispsys0`의 `0x170`이 `0x00a43210`, `0x174`가 0이다. `mipitx0`, `mipitx1`의 `0x320`=1, `0x40`의 LPRX1이 5다.
5. [ ] DSC 두 벌의 값이 패널의 값과 같다(`0x18`=`0x01ea05c0`, `0x1c`=`0x072f072f`, `0x20`=`0x00f502e0`, `0x24`=`0x405b0013`, `0x84`=`0x02900200`). 다르면 LK의 모드가 무엇인지 적는다.
6. [ ] DSI 두 벌의 `0x14`(모드), `0x20`~`0x2c`, `0x50`~`0x58`, `0x110`~`0x11c`, `0x400`~`0x438`을 적는다. 패치 값은 아래 표다. PLL `0x2c`가 `0x2663ccf5`인지 본다. 다르면 패널 표의 `hs_rate`를 본다.
7. [ ] 라브의 `0x380`~`0x3bc` bit 0. LK는 라브0·1을 지우고 라브20·21은 건드리지 않는다. 모두 0이면 그대로 간다. **1인 칸이 있으면 OVL 주소가 IOMMU를 지난다.** 이 판은 IOMMU가 없으므로 그대로 켜면 스캔아웃이 틀린 곳을 읽는다. 전원을 껐다 켠 뒤의 기본값은 6단계에서 다시 본다.
8. [ ] OVL 두 벌의 `0x20`(ROI)이 `0x073005c0`이고 `0x24`, `0x324`의 값을 적는다. 패치는 둘을 통째로 쓴다(`0x07000001`, 0). LK는 `ovlsys`마다 OVL 셋을 사슬로 잇는다(`0x14402000`~`0x14404000`, `0x14602000`~`0x14604000`). 켜진 레이어가 어느 OVL에 있는지 `0x0c`와 `0x20`으로 보고, 그 OVL의 `0xf40`이 `framebuffer0` 주소와 절반 오프셋인지 본다(오른쪽은 `0xfc16f000` + 1472×4). 패치는 첫 OVL만 쓰고 뒤의 둘은 끊어 둔다.

패치가 쓰는 값이다(BOE 패널, 두 링크 같다). LK의 값과 다르면 다른 칸을 적고, 무엇을 고칠지 켜기 전에 정한다. 벤더 코드를 돌린 값과 맞췄지만 LK와 맞대 본 적은 없다.

| 주사율 | `0x20` VSA | `0x24` VBP | `0x28` VFP | `0x2c` VACT | `0x50` HSA | `0x54` HBP | `0x58` HFP |
|---|---|---|---|---|---|---|---|
| 144 Hz | `0x2` | `0xb6` | `0x1a` | `0x730` | `0x8` | `0x54` | `0x84c00060` |
| 120 Hz | `0x2` | `0xb6` | `0x1a` | `0x730` | `0x14` | `0x100` | `0x84c00104` |
| 60 Hz | `0x2` | `0xb6` | `0x81c` | `0x730` | `0x14` | `0x100` | `0x84c00104` |
| 30 Hz | `0x2` | `0xb6` | `0x1820` | `0x730` | `0x14` | `0x100` | `0x84c00104` |

- DSI 공통: `0x14`=1, `0x100`=`0x55`, `0x18`=`0x3c`, `0x1c`=`0x505c0`, `0x38`=`0x73001eb`, `0x64`=`0x5c0`, `0x110`=`0x0d130c0c`, `0x114`=`0x133c1230`, `0x118`=`0x0c280103`, `0x11c`=`0x0013150a`, `0x400`=1, `0x410`=`0x4978f`, `0x418`=`0x1b7`, `0x424`=`0x197`, `0x428`=`0x188`, `0x42c`=`0x188`, `0x430`=`0x168`, `0x434`=`0xeb`, `0x438`=`0xdb`. `0x414`(SODI 상한)은 먹스 속도에 따른다(엔진 클럭 MHz×3/32를 채움률로 `1364 − 12×(채움률−15)/10`).
- PHY: `0x04`=`0x3fff0280`, `0x0c`=`0xffff00f0`, `0x28`=1, `0x2c`=`0x2663ccf5`, `0x30`=`0x101`, `0x3c`=`0x00ff12e0`, `0x40`=`0x500`, `0x320`=1.
- OVL: `0x0c`=1, `0x20`=`0x073005c0`, `0x24`=`0x07000001`, `0x2c`=`0x101`, `0xc8`=`0x03ff03ff`, `0xd0`=`0x04000000`, `0x1e0`=`0x20ff01ff`, `0x1f8`=`0xf1ffffff`, `0x1fc`=`0xffff`, `0x20c`=`0x8040`, `0x324`=0, `0x740`=`0x678`.
- 뮤텍스(0번, 뮤텍스 `0x08`은 패치가 0으로 쓴다): dispsys `0x2c`=`0x41`, `0x30`=`0x140200`. ovlsys0 `0x2c`=`0x41`, `0x30`=`0x8001`. ovlsys1 `0x2c`=`0x82`, `0x30`=`0x8001`.
- 경로(패치가 켠 뒤): 두 dispsys `0x008`=0, `0x034`=0, `0x0f4`=`0x13f6c0`, `0x0f8`=0, `0xf48`=0, `0xf4c`=2, `0xf60`=1, `0xf74`=4, `0xf98`=0, `0xfa4`=2, `0xea4`=1, `0xeac`=0, `0xf0c`=1. 두 ovlsys `0x008`=0, `0xf08`=1, `0xf10`=0, `0xf14`=0, `0xf38`=1, `0xf40`=0. `0x0f0`의 `0x3fffc`는 ovlsys0 `0x28000`, ovlsys1 0.

#### 1. PHY

1. [ ] `mipi_tx0`, `mipi_tx1`만 켠다. `modprobe phy-mtk-mipi-dsi-drv`. probe는 트림 레지스터를 읽기만 한다. 화면이 그대로인지, 멈추지 않는지 본다.

#### 2. 전원과 클럭

위 "2c 2차와 코덱·카메라 앞" 차례(`spm`, MM_INFRA와 화면 도메인, `framebuffer0`의 `power-domains`, 게이트 블록)를 먼저 끝낸다. 그 차례의 `dispsys0` 등 노드는 이제 `syscon@`이다. `mtk-mmsys`가 붙고 클럭 드라이버가 그 아래에 뜬다.

1. [ ] `mtk-mmsys`가 네 블록에 붙고 `/sys/bus/platform/drivers/clk-mt6897-mmsys/`에 `clk-mt6897-dispsys0.*` 등 넷이 보인다. 화면이 그대로다. `mediatek-drm` 장치가 하나 생겨 있어도 드라이버는 블랙리스트로 묶여 있다.

#### 3. 노드 켜기

1. [ ] `disp_mutex0`, `disp_mutex1`, `ovl_mutex0`, `ovl_mutex1`, `dsc0`, `dsc1`, `ovl_2l0`, `ovl_2l1`, `dsi0`, `dsi1`, `panel0`, `lcd_avdd`, `lcd_avee`를 켠다. `framebuffer0`의 `panel`은 꺼진 크기 노드(`panel_size`)를 가리키게 한다. 패널 노드를 가리키면 fw_devlink가 simpledrm을 붙들고, 빼면 simpledrm이 화면 크기를 잃어 배율이 1이 된다. `gpio-shared-proxy`가 있어야 한다(GPIO184).
2. [ ] 부팅이 끝까지 간다. `mtk-mutex`가 붙고 DRM 드라이버는 아직 없다. 화면이 그대로다.

#### 4. 첫 모드 설정 (손으로)

```bash
dmesg -w &                          # PC로 받는다
modprobe panel-novatek-nt36532
modprobe mediatek-drm
```

순서: mediatek-drm이 도메인을 잡는다 → simpledrm이 내려가도 LK 화면은 그대로 돈다 → 첫 커밋(`DRM_FBDEV_EMULATION`이 켜져 있어 모듈을 올리면 곧 온다)에서 DSI 두 링크가 프레임 끝에 멈춘다 → 뮤텍스와 경로를 다시 쓴다 → 패널 리셋과 초기화 → 영상 시작. 성공하면 콘솔 그림으로 바뀐다. 이 차례에서 패널 리셋까지 그림이 잠깐 꺼지는 것은 정상이다.

1. [ ] 멈추면: pstore와 마지막 `dmesg`로 어디까지 갔는지 본다. 멈출 만한 곳을 차례로: 경로 쓰기(클럭), OVL, DSI 켜기, 첫 명령.
2. [ ] `/sys/class/drm/card*-DSI-1/modes`에 모드 넷. `cat /sys/kernel/debug/dri/*/state`에 `active=1`.
3. [ ] 프레임이 돈다: `modetest -v`나 `/sys/kernel/debug/dri/*/` 의 vblank. 안 돌면 `read-display-regs.sh`로 경로와 뮤텍스(`0x20`, `0x2c`, `0x30`)를 읽고, 신호 상태 `0xc00`~`0xc2c`로 어느 쪽에서 막혔는지 본다. 왼쪽만 돌면 오른쪽 크로스바 추론이 틀린 것이다. 그때는 LK처럼 크로스바 0과 PQ 길로 간다(`DRM_BYPASS_PQ`가 없는 벤더 경로).
4. [ ] 왼쪽과 오른쪽 절반이 모두 맞는다. 패널 위에 절반씩 다른 색 그림을 낸다. 한쪽만 검으면 그쪽 경로다. 절반이 바뀌면 `dual_swap`을 확인한다. 세로 띠나 글자 깨짐이면 DSC(`0x84`~`0xcc`)나 RC 값이다.
5. [ ] 첫 줄이 어긋나거나 가로로 흐르면 DSI 포치(`0x50`~`0x58`)와 `0x38`을 본다.

#### 5. 평면과 컴포지터

1. [ ] 커서를 화면 가운데로 움직인다. 1472 근처에서 끊기거나 튀지 않는다.
2. [ ] `comp`가 켜진다. 버퍼를 KMS 쪽(더미)에서 받아야 한다. IOMMU가 없어 GPU의 흩어진 버퍼는 스캔아웃하지 못한다. CMA 256 MB로 모자라지 않은지 본다(`/proc/meminfo`의 `CmaFree`).

#### 6. 나머지

1. [ ] 패널이 뜨는지(첫 prepare 깜빡임 포함), 144·120·60·30Hz 전환과 실제 주사율을 본다. 벤더 PHY 기준으로 계산한 값은 143.97, 119.57Hz다.
2. [x] 밝기가 도는지, 0에서 어두워지는지, 화면을 끌 때 LED가 꺼지는지 본다. DCS 0x51은 듣지 않아 `disp_pwm0`으로 옮겼다(위 결과). 화면을 끄면 듀티가 0이 되고(`bl_power=4`) 켜면 돌아온다. `CONFIG_BACKLIGHT_CLASS_DEVICE`는 DRM처럼 내장이어야 한다. 모듈이면 `drm_panel_of_backlight()`가 아무 일도 하지 않아 패널이 켜고 끄기를 넘기지 않는다. 대기 전류는 아직 재지 않았다.
3. [ ] 편향을 껐다 켠 뒤 전압이 유지되는지 본다(SM5109). **유지되지 않는다(2026-11-04):** 켠 뒤 `0x00`·`0x01`이 LK의 `0x12`가 아니라 `0x0e`로 읽히고, 꺼진 동안에는 I2C에 답하지 않는다. 순정은 켤 때마다 `0x12`를 쓴다. 이때는 로그인 때 컴포지터가 CRTC를 꺼서 첫 끄고 켜기 뒤로 이 값으로 돌았다. 화면 넘기기 뒤로는 로그인 때 끄지 않으므로 언제 값을 잃는지 다시 봐야 한다. TPS65132 식으로는 5.8 V가 5.4 V가 된다(SM5109 자료는 없어 식은 추정이다). 눈에 보이는 문제는 아직 없다. 드라이버나 켤 때의 쓰기가 필요하다. SGM37604A 둘은 끄고 켠 뒤에도 값을 지켜 드라이버가 필요 없다.
4. [x] 터치를 패널 팔로워로 잇는다(`drm_panel_follower`, 패치 0182·0183). 패널 리셋(GPIO41)과 터치 리셋(GPIO60)은 다르지만, 패널 리셋이 터치 펌웨어를 지운다(TDDI). 벤더 터치는 화면 통지로 재시작한다. 드라이버는 터치가 열려 있으면 패널이 켜진 뒤(`panel_enabled`) 펌웨어를 올리고, 꺼지기 전(`panel_disabling`)에 멈춘다. 화면을 끄고 켠 뒤와 서스펜드 뒤는 아직 보지 않았다. 끌 때 GPIO184를 0으로 둘지도 본다.

   **결과(2026-11-05).** 컴포지터가 첫 화면을 늦춘 뒤(buntOS `cb94109`)로 터치를 여는 때가 패널 리셋(첫 모드 설정) 앞으로 왔다. 재부팅 뒤 첫 로그인 화면에서 터치가 되는지를 가른 것이 이 순서다.

   | 부팅 | 터치 열기와 첫 모드 설정 | 첫 로그인 화면의 터치 |
   |---|---|---|
   | 2026-11-04 22:00 무렵 둘 | 모드 설정 0.7초 뒤에 연다 | 된다(재시작 없이 로그인) |
   | 그 뒤 둘(22:45, 23:00) | 모드 설정 0.5초 앞에 연다 | 알 수 없다. 로그인 전에 다른 세션이 로그인 화면을 재시작했다 |
   | #303 | 모드 설정 0.5초 앞에 연다 | **죽었다.** 누르는 13분 동안 인터럽트 2개. greetd를 재시작하니 살아났다 |
   | #304(패치) | 모드 설정 0.5초 앞에 연다 | **된다.** 펌웨어는 패널이 켜진 뒤에 올라가고, 누르면 인터럽트가 는다. 키보드가 뜨고 로그인되는 것을 사용자가 봤다 |

   재시작하면 되는 까닭은 패널이 켜진 채로 있어 리셋되지 않기 때문이다. 가방 안에서 책에 눌렸을 때는 손가락 3~5개가 초당 144번 보고됐다. 고장이 아니라 실제 접촉이다. 인터럽트 수는 `grep spi0.0 /proc/interrupts`로 본다.
5. [ ] 패널 노드를 켜면 GPIO184가 터치와 공유되어 `gpio-shared-proxy` 모듈이 있어야 한다. 없으면 터치와 패널이 둘 다 미뤄진다. udev가 올리고, 터치나 패널을 initramfs에 넣으면 `modules.list`에 함께 넣는다.
6. [ ] 중국판 펌웨어의 패널 모듈이 같은지 본다. 사용자 기기에서 vendor_boot를 뜨면 글로벌판 모듈과 표를 맞댄다.
7. [ ] DPMS 끄고 켜기, 도메인이 꺼지는지(`pm_genpd_summary`), 켠 뒤 화면과 밝기가 그대로인지 본다. **첫 켜기는 LK 상태 위에서 돌고, 전원을 껐다 켠 뒤의 켜기는 여기서 처음 돈다.** 여기서만 안 되면 LK가 쓰고 이 판이 되쓰지 않는 값을 본다: `0xf4`=`0xf500`과 `0xf8`=3(이 판은 벤더 커널 값 `0x13f6c0`과 0을 쓴다), SMI 서브커먼 `+0x100`=`0xa`, 라브 `0x380` 칸의 기본값. 패널 초기화 명령이 `-110`으로 끝나면 레인이 LP-11로 돌아오지 않은 것이다. 벤더는 `mtk_dsi_exit_ulps`로 깨운다(PHY를 소프트웨어 제어로 잡고 `SLEEP_MODE`, `SLEEPOUT_START`). 이 판은 업스트림처럼 `WAKEUP_EN`으로 깨운다. 이 차례를 먼저 옮겨 본다. 화면이 꺼지는 동안 밝기를 바꿔도 멈추지 않는다(`-ENODEV`로 끝난다).
8. [ ] 144Hz에서 언더런을 본다. 줄이 튀거나 찢어지면 60Hz와 비교한다. 먹스(`disp0`, `ovl0`)는 LK 값이다. `clk_summary`로 적는다. SODI 상한은 엔진 클럭으로 계산한다.
9. [x] 두 OVL의 레이어 설정은 프레임 끝 인터럽트에서 CPU로 쓰고 곧바로 flip을 끝냈다. CPU 유휴를 켜자 CPU0이 늦게 깰 때 화면 아래쪽이 한 프레임 깨졌다. 레이어 레지스터는 그림자라 커밋 때 쓰고, 반영을 확인한 뒤 flip을 끝내게 고쳤다(0197, 2026-11-07). CMDQ는 여전히 없다.
10. [ ] `clk_ignore_unused`를 뺄 때: 화면 패치의 게이트는 소비자가 켜 두므로 닫히지 않는다. 먹스 쪽을 본다.
11. [x] CPU0까지 깊은 유휴를 켠 채 키보드나 글자 커서를 움직여 화면 아래쪽에 분홍빛 프레임이 없는지 본다(0197 시험 모듈, 2026-11-07 사용자 확인).

### 9.6 3 소리

패치 0160~0167, 시험 커널 #305~#307(2026-11-05). 기기 쓰기는 `boot_b`와 `buntos` 안의 파일뿐이고, `persist`는 읽기 전용 루프 장치로만 읽었다. 스피커 시험은 사용자가 곁에서 듣는 동안만 했고, 음량은 올리기 전마다 물었다.

1. [x] 읽기: APLL 값, 핀, `persist`의 앰프 보정 파일(`docs/hw/lenovo-tb375fc.md` 7장).
2. [x] 앰프 없이 AFE: 첫 적재에 충돌 없음, 도메인 켜고 끄기 정상. ETDM 되돌림(`I2SIN4 Loopback`)으로 네 슬롯 96,000 프레임이 비트까지 같다. 슬롯 맞바꾸기, 슬롯 하나만 켜기, S16 입력, 20초 연속(960,000 프레임, 끊김 0)도 맞다.
   - 되돌림의 데이터는 패드를 거치지 않아 ETDM_IN4의 표본 시점이 달라진다(벤더 주석 "3: pad top 5: no pad top"). 3으로 두면 값이 왼쪽으로 2비트 밀려 돌아온다. 패드 쪽 값은 벤더와 같다.
   - GPIO82는 R1R0 형 핀이라 값 없는 `bias-pull-up`은 `Invalid pull argument`로 실패하고, `i2c6`이 메시지 없이 묶이지 않는다. 값은 순정과 같은 `MTK_PUPD_SET_R1R0_10`이다.
3. [x] 앰프: 넷 다 칩 ID 0x2049, `start success`, 재생 중 SYSST 0x0311. 대기 때 SYSCTRL 0x4347(전원 내림, 음소거).
4. [x] 스피커 넷: 사용자가 하나씩 듣고 자리를 확인했다. 왼쪽 둘, 오른쪽 둘의 순서도 맞다. 앰프의 전압·전류 측정값은 켠 앰프만 오르고, 12 dB 올리면 3.1~4.8배가 된다.
5. [x] 내장 마이크: AIN0, AIN2, AIN3에 신호가 있다. 사용자가 녹음을 들어 보고 AIN0을 골랐다. 프리앰프 24 dB에서 실효 -45.3 dBFS, 말이 없을 때 약 -50 dBFS. 이 바닥값이 방 소음인지 회로 잡음인지는 가리지 못했다.
6. [x] PipeWire: UCM으로 `Built-in Audio Speakers`와 `Internal Microphone`이 기본 장치가 된다. 상단 바 슬라이더를 움직이면 앰프 측정값이 함께 바뀐다. 재생과 녹음을 함께 25초 돌려 ERR 0.
7. [x] 앰프 감쇠: 사용자가 슬라이더 100%를 들어 보고 -18 dB를 골랐다. 앰프 컨트롤은 감쇠라 `PlaybackMixerElem`으로 주지 않는다.
8. [x] 재부팅: `buntos-sound.service`가 부팅 4~5초에 모듈과 보정값을 넣는다. 첫 로그인 화면의 터치와 로그인 정상. 시동음이 들렸고 "좀 작았다"고 해서 기본 슬라이더를 40%에서 55%로 올렸다(`50-buntos-tb375fc-volume.conf`). **55%의 시동음은 다음 부팅에 듣는다.**
9. [ ] 음악 재생, 긴 재생의 끊김과 왜곡, 설정 앱의 슬라이더와 입력 레벨.
10. [ ] 서스펜드 뒤 복귀. 잠자기 뒤에 앰프 넷이 `start success`로 켜지는 것은 봤다(2026-11-05, 9.7절). 소리를 듣고 확인하지는 않았다.
11. [ ] 배터리가 낮을 때 Vmax. 벤더 표를 옮겼지만 시험 때 배터리가 4.33 V로 표의 0 칸이라 값이 줄어드는 것을 못 봤다.
12. [ ] USB-C 이어폰. 코덱의 재생 경로, FSA4480, accdet, 포트의 DRP가 필요하고, 충전과 맥 연결에 영향이 없는지 따로 본다. 사용자의 이어폰 종류에 따라 일이 다르다.
13. [ ] 화면 방향에 따른 스피커 채널 돌리기. 순정 트리의 `spin-data`에 네 방향의 슬롯이 있다. 지금은 슬롯이 고정이다.
14. [ ] `DL1_FE: no backend DAIs` 한 줄이 탐지 때 남는다. 프로 오디오 탐지로 보이고 동작에는 영향이 없었다.

### 9.7 전원 키와 잠자기

**결과(2026-11-05, 사용자 동석, 커널 #315·#316, USB를 꽂은 채와 뺀 채).** 잠자기 15번이 모두 돌아왔고 실패, 잠금 화면 충돌, 재부팅은 없었다.

| 동작 | 어떻게 되는가 |
|---|---|
| 전원 키를 눌렀다 2초 안에 뗌 | 뗄 때 드라이버가 `KEY_POWER`를 누름과 뗌으로 함께 알리고, logind가 잠든다(`HandlePowerKey=suspend`). 잠들 때 컴포지터가 백라이트를 먼저 꺼서 바로 꺼진 것처럼 보인다 |
| 잠든 동안 전원 키 | 깨어난다. 깨운 누름은 알리지 않아 다시 잠들지 않는다(복귀 뒤 0.5초까지 같다) |
| 2초에서 10초 사이에 뗌 | 아무 일도 없다 |
| 10초 잡음 | 8초에 `emergency_sync`, 10초에 `kernel_power_off`. 두 번 다 `TOPSTATUS`로 키가 정말 눌려 있는지 본다. 기록: `Emergency Sync complete`, 2초 뒤 `the power key was held, powering off` |
| 볼륨 올림을 함께 누름 | 강제 종료를 하지 않는다. 전원+볼륨 올림은 지금처럼 PMIC 리셋이다 |

`TOPSTATUS`(`0x1e`)는 손을 떼면 `0x1e`, 전원 키를 누르면 `0x1c`, 볼륨 올림을 누르면 `0x16`이다(0.25초 간격으로 읽었다). MT6359와 같은 자리다.

**s2idle이 돌아오기까지 하나씩 나온 것:**

| 증상 | 원인 | 고침 |
|---|---|---|
| USB가 컴퓨터에 꽂히면 바로 돌아온다(`mtu3: ip sleep failed`, `-16`) | 컨트롤러가 잠들지 못한다 | `system-sleep/buntos-usb-gadget`가 잠든 동안 UDC를 뗀다 |
| 잠든 지 약 1분 뒤 재시작 | LK가 켠 감시 타이머(`WDT_MODE` `0x7d`, 31초)를 코어가 대신 치는데, `/dev/watchdog`를 아무도 열지 않아 `mtk_wdt_suspend`가 멈추지 않는다. 두 번째 만료까지 최대 62초 | 패치 0184. 본가 master도 같다 |
| 깨면 화면이 꺼진 채이고 다음 잠자기에서 재시작 | `univpll: hardware voter did not release the PLL: -110`. I2C, USB, 백라이트 PWM이 모두 `-110`이고, I2C가 suspended로 남아 MT6375 인터럽트 오류가 쏟아진다 | 패치 0187. PLL을 끄지 않는다 |
| 깨면 패널 절반이 회색이고 약 1분 뒤 재시작 | genpd가 시스템 잠자기에서 OVL 도메인을 끈다. HW voter의 끄기가 조용히 실패한다(9.5절) | 패치 0188. 화면 도메인 다섯을 항상 켠다 |
| 깨면서 깜빡이거나 어둡다가 밝아진다 | 가상 터미널 전환으로 세션이 잠깐 멈추고, 그때 잠금 화면이 `SIGSEGV`로 죽기도 한다(20번 중 4번). 죽는 자리는 못 봤다 | 패치 0189 뒤로 입력 장치를 떼지 않고, 잠금 화면 충돌도 15번 중 0번이다 |
| 첫 잠자기 뒤 Bluetooth가 죽는다 | 9.2절 | 패치 0190 |

- **못 본 것:** 잠자기 전력, 몇 시간 재워 두기, 잠든 동안 온 전화나 알림 같은 깨우기, 덮개(홀), 소리 재생 중 잠자기.
- **잠든 동안 충전되지 않는다:** `mt6375_chg_suspend`가 온도를 볼 작업이 돌지 않으니 충전을 끈다. RTC로 깨우지 못해(`/dev/rtc0 not enabled for wakeup events`) 순정처럼 10초마다 깨어 볼 수 없다.
- **USB를 꽂은 채 강제 종료:** 꺼진 뒤 약 19초에 새 부팅 기록이 있다. 충전기가 있어 저절로 켜졌는지 사용자가 켰는지는 가리지 못했다.
- 기록을 잠든 동안 남기려면 커널 메시지가 복귀 뒤에야 journal에 들어가므로, 멈춘 지점을 보려면 `/dev/kmsg`를 파일에 적어 fsync하는 도구를 잠들기 전에 돌린다. pstore는 11월 4일 기록에 묶여 있어 쓸 수 없다.

## 10. 아직 비어 있는 것

자료가 없어 비워 둔 것이다. 정해지면 이 문서에 넣는다.

1. **백업에 쓸 펌웨어 묶음.** "그 기기 판의 묶음"을 쓰라는 보고뿐이다. 우리가 본 묶음은 중국판 16.0.20.091과 글로벌판 17.0.04.266이고, ZUXOS 1.5.10.044의 묶음을 구한 기록은 없다.
2. **판을 내린 뒤의 OTA와 언락의 데이터.** 앱을 끈 설정은 초기화하면 사라진다(포럼 보고). 그래서 판을 내린 뒤 다시 끈다. 언락은 볼륨 키로 동의하면 데이터를 지운다(포럼 보고).
3. **명령 꼴.** vbmeta 셋의 파티션 이름과 명령, 비운 슬롯에 넷을 굽는 명령, GPT를 뜨고 고치는 도구를 정하지 않았다. 그래서 글로만 적었다. 파티션 이름은 3절 3번에서 본다.
4. **멈췄을 때.** 안드로이드 상태에서 전원 + 볼륨 올림은 LK fastboot로 들어가고, 전원 + 볼륨 내림은 fastboot에서 나온다(실측, 2026-11-02. [Windows 작업 안내](windows-steps.md) 8절). MT6363 드라이버가 뜨기 전(2a, 2b 초반)에 리눅스가 멈췄을 때도 이 키가 듣는지는 모른다. 첫 부팅에서 본다. [커뮤니티 포트 분석](community-port.md)에는 그 포트의 주석 "멈추면 키를 눌러 fastboot로 되살린다"만 있고 어느 키인지는 없다.
5. **입력 통로.** UART를 찾지 못하면 2a와 2b 초반에 셸에 입력할 길이 없다. 5절과 6절 머리의 차례(화면 출력으로 보고, 가젯 뒤에 읽는다)는 이 문서의 해석이다. `/sys/firmware/fdt`를 기기 밖으로 옮기는 일도 그래서 가젯 뒤가 된다.
