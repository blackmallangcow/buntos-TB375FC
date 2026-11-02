# TB375FC bring-up 체크리스트

- 작성일: 2026-11-01
- 기준: [설계안](DESIGN.md) v1.16, 패치 대기열 0001~0049
- 상태: **기기에서 돌린 항목이 아직 없다.** 기기 결과로 고친다
- 관련 문서: [장치 트리 README](../../kernel/tb375fc/dts/README.md), [BSP README](../../bsp/lenovo-tb375fc/README.md), [하드웨어 실측](../hw/lenovo-tb375fc.md), [포팅 재료 조사](porting-materials.md), [buntOS 설계안](../design/ARCHITECTURE.md) 4.2.3절·12.1절

## 1. 쓰는 법

기기에서 할 일을 일어나는 차례로 모았다. 위에서 아래로 한다.

- **아직 아무것도 기기에서 돌리지 않았다.** 사용자의 TB375FC는 잠겨 있다. 기기에서 한 일은 adb 읽기와 업데이트 앱 끄기뿐이다(2026-11-01). 아래 기대값은 펌웨어, OEM 코드, 스톡 모듈, QEMU, 레지스터 모델에서 왔다.
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

맥의 `fastboot`가 이 기기에 닿는지는 아직 보지 않았다. 판을 내리기 전에 `adb reboot bootloader`, `fastboot devices`, `fastboot reboot`로 한 번 본다. 벽돌이 되면 되돌리는 데 Windows가 다시 필요하다.

위험 표시는 `위험:`으로 찾을 수 있다. 표시 안에 먼저 둘 것과 되돌리는 길을 적었다.

| 표시 | 뜻 |
|---|---|
| `위험: 부팅 불가` | 기기가 부팅하지 못하게 될 수 있다 |
| `위험: USB 끊김` | USB 디버그 길이 끊긴다. 포트가 하나라 화면 말고는 유일한 길이다 |
| `위험: 데이터 손실` | 되살릴 수 없는 데이터나 userdata를 잃을 수 있다 |

근거를 나타내는 말은 소스의 말을 그대로 옮겼다.

| 말 | 뜻 |
|---|---|
| 실측 | 사용자 기기에서 adb로 읽었다(2026-11-01) |
| 펌웨어 | 순정 펌웨어에서 읽었다 |
| 코드로 본 것 | 소스나 역어셈블을 읽었고 기기에서 돌려 보지 않았다 |
| 모델 값 | 벤더 자료로 만든 모델이나 시뮬레이터의 값이다. 실제 칩이 아니다 |
| 포럼 보고 | XDA 글타래 등의 보고다. 우리가 기기에서 확인하지 않았다 |
| 판단, 추론, 짐작 | 근거에서 미룬 해석이다. 틀릴 수 있다 |

## 2. 언락 전

**목표.** 언락 go/no-go 넷을 채운다. 기기에 아무것도 굽지 않는다.

**종료 조건.** 2.4절의 넷을 모두 채웠다. 하나라도 빠지면 언락하지 않는다. 2026-11-01 조사로는 넷 모두 길이 보이지만, 모두 기기에서 확인해야 한다.

### 2.1 판과 OTA

1. [ ] 기기의 판을 적는다. 사용자 기기는 중국판 ZUXOS 1.5.10.044(Android 16)다(실측).
2. [ ] OTA를 끈다(go/no-go 4). 언락 길이 ZUI 16에 묶여 있으니 받자마자 끈다. 1.5.10.103 이후 판에서는 판 바꾸기 문제가 많이 보고됐다(포럼 보고). 그래서 1.5.10.044에 머문다. 사용자 기기는 2026-11-01에 이미 껐다.

    ```bash
    adb shell pm disable-user --user 0 com.lenovo.ota
    ```

    되돌리기:

    ```bash
    adb shell pm enable com.lenovo.ota
    ```

### 2.2 adb로 읽기

root 없이 읽기만 한다.

1. [ ] 사용자 기기는 이미 읽었다([실측 보고서](../hw/lenovo-tb375fc.md) 7절): 판, 잠금 상태, RAM 8GB와 저장장치 128GB, 화면(패널 버퍼 가로 2944×1840), 입력 장치 이름, vendor 펌웨어와 모듈 목록, 지문 센서, 센서(LSM6DSO, TCS37013, SCP 뒤). 다른 기기를 쓰면 다시 읽는다.
2. [ ] 차지 펌프가 어느 칩인지 본다.

    ```bash
    adb shell cat /sys/devices/platform/boardinfo/boardinfo
    adb shell ls /sys/bus/i2c/drivers/nu2115 /sys/bus/i2c/drivers/upm6720
    ```

    - 첫째는 순정 `hardware_info`의 `boardinfo`로 충전 칩 이름이 들어 있다(코드로 본 것, 기기에서 돌려 보지 않았다). 둘째는 붙은 주소를 보여 준다. SELinux가 막을 수 있다.
    - 사용자 기기는 UPM6720 @0x65다(실측: `/sys/bus/i2c/drivers/upm6720/10-0065`가 있고, NU2115에는 붙은 장치가 없다). 보드 dts의 `vbus-switch-supply`가 이 칩을 가리킨다.
    - 부품이 다른 기기가 있을 수 있다. 그래서 8절 1단계에서 `i2cdetect -r`로 다시 본다.
3. [ ] `/vendor/firmware`는 이름만 볼 수 있다. adb 셸은 파일을 읽지 못한다(실측). 파일은 2.3절 백업이나 언락 뒤 root로 얻는다(4절 8번). 이름과 판 정보는 [pull-android.sh](../../bsp/lenovo-tb375fc/firmware/pull-android.sh)로 받을 수 있다.

4. [ ] 안드로이드를 떠나기 전에 할 일이 둘 있다. 6.10절 1·2번(전원 꺼짐 알람을 끄고 RTC 시간을 읽는다), 9.4절 0번의 구역 온도다.

### 2.3 전체 백업 (go/no-go 1)

mtkclient로는 안 된다(V6 DA에 DAA·SLA). 순정 펌웨어 묶음의 인증 DA·auth와 SP Flash Tool로 readback한다. 이 길은 포럼 보고로 보았고, 기기에서 확인해야 한다.

> **위험: 데이터 손실.** nvdata·nvram 계열에는 Wi-Fi MAC과 보정값이 있다. 잃으면 되살릴 수 없다. 이 백업을 해시로 확인하기 전에는 아무것도 굽지 않는다. 뒤의 판 내리기, 언락, GPT 고침을 되돌리는 길이 모두 이 백업이다.

1. [ ] SP Flash Tool을 준비한다. Windows 도구다. 인증 DA·auth는 그 기기 판의 레노버 펌웨어 묶음에 든 것을 쓴다. 우리가 본 중국판 묶음에는 인증 DA(`DA_BR.bin`), `da.auth`, SLA 파일이 있다.
    - 공용 컴퓨터에서 한다면 도구, 드라이버, 펌웨어 묶음을 외장 SSD에 담아 가고, 백업은 그 컴퓨터의 디스크가 아니라 외장 SSD(빈 곳이 128GB 넘게)에 쓴다. 백업에 Wi-Fi MAC과 보정값이 들어 있다. 드라이버 설치가 막혔거나 재부팅하면 지워지는 컴퓨터일 수 있으니 미리 본다. 해시는 맥에서 다시 맞댄다. 일반적인 판단이고 해 본 적은 없다.
2. [ ] scatter와 readback 설정을 기기 용량(128·256GB)에 맞춘다. 사용자 기기는 128GB다. XDA 원글 작성자가 이 기기용 scatter와 readback 설정을 만들었다(포럼 보고).
3. [ ] 전체 파티션을 readback한다. 특히 `nvram`, `nvdata`, `nvcfg`, `protect1/2`, `persist` 계열을 본다([buntOS 설계안](../design/ARCHITECTURE.md) 4.2.3절).
4. [ ] 다른 기기에 옮겨 해시로 확인한다.

### 2.4 go/no-go

| 조건 | 지금 | 확인 |
|---|---|---|
| 1. 전체 파티션 백업과 해시 확인 | 길이 있다. 기기에서 확인 | 2.3절 |
| 2. 독립된 보고 둘 이상과 판 일치 | 보고는 여럿이다. 판 확인이 남았다 | 아래 1번 |
| 3. BROM·preloader 복구 길 | 있다(포럼 보고). 기기에서 확인 | 아래 2번 |
| 4. OTA 차단 | 기기에서 한다 | 2.1절 |

1. [ ] 언락할 판(예: 16.0.20.091)으로 된 성공 보고를 확인한다. ZUXOS 기기는 그 판으로 내린 뒤다. 판마다 결과가 갈린다. 모두 포럼 보고다([조사](porting-materials.md) 5절).
    - 16.0.20.022·.041: 원글 작성자와 다른 사용자 여럿이 성공했다(2025-02).
    - 16.0.20.091: TrebleDroid 위키가 언락한 뒤 GSI를 띄웠다(2025-08). 성공 보고가 있다(2025-09, 2026-09). 256GB 기기가 16.0.20.130에서 .091로 내린 뒤 개발용 LK와 sn.img가 모두 실패한 보고도 있다(2025-12).
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

1. [ ] ZUXOS(Android 16) 기기는 SP Flash Tool로 중국판 ZUI 16.0.20.091까지 내린다. 사용자 기기가 여기에 든다. 근거는 포럼 보고 하나이고, 1.5.10.044를 말한 글은 찾지 못했다.

    > **위험: 부팅 불가·데이터 손실.**
    >
    > - preloader는 굽지 않는다. 구우면 벽돌이 된다는 보고가 있다.
    > - 글로벌 ROM을 중국판 기기에 굽지 않는다. 부팅이 반복된다는 보고("Red State")가 있다.
    > - Android 16 판(ZUXOS 1.5.10.103 이후, 글로벌 17.5.10.095)에서 다른 판의 이미지를 구우면 "Red State"로 부팅하지 못한다는 보고가 여럿이다. 1.5.10.103부터 LK가 글로벌 이미지를 거부한다는 글도 있으나, 작성자가 AI로 썼다고 밝힌 글이라 따로 확인해야 한다.
    > - 128GB 기기에 256GB 기기의 덤프를 구우면 실패한다(포럼 보고).

2. [ ] 언락할 판이 2.4절 1번에서 확인한 판과 같은지 본다.
3. [ ] 파티션 이름과 슬롯을 본다.

    ```bash
    fastboot getvar all
    ```

4. [ ] 개발용 LK를 순정 LK와 견준다(문자열, 서명 영역). 개발용 LK는 4pda에 올라온 덤프에서 나와 출처를 확인할 수 없다. 남이 고친 부트로더를 굽는 위험이 있다.
5. [ ] 개발용 LK(`lk_a`)를 굽는다. 중국판 ZUI 16의 순정 LK가 `fastboot flash lk`를 받는다(포럼 보고).

    ```bash
    fastboot flash lk <개발용 LK 파일>
    ```

    > **위험: 부팅 불가.** 출처를 확인할 수 없는 부트로더를 굽는다.
    >
    > **먼저 둘 것.** 4번의 비교, 7번에 쓸 순정 LK 파일.

6. [ ] 언락한다. 언락 파일 없이 된다(포럼 보고).

    ```bash
    fastboot flashing unlock_critical
    ```

    - 안 되면: 2026-09 보고는 개발용 LK를 구운 뒤 시스템으로 한 번 부팅하고 fastboot로 다시 들어가야 언락됐다고 적었다.
    - 순정 LK의 길은 개발자 옵션의 OEM 잠금 해제, 일련번호에 묶이고 레노버가 서명한 언락 이미지, `fastboot oem unlock`(또는 `flashing unlock`)이다(LK 문자열). 레노버가 이 모델에 언락 파일을 주는 길은 찾지 못했다.
    - 그래도 안 되면 buntOS ADR-011(기기 선택)을 다시 본다.
7. [ ] 바로 순정 LK로 되돌린다.

    ```bash
    fastboot flash lk <순정 LK 파일>
    ```

8. [ ] AVB를 끈다. vbmeta 셋을 `--disable-verity --disable-verification`으로 다시 굽는다(포럼 보고). 파티션 이름은 3번에서 본다.
9. [ ] 슬롯을 지킨다. 한쪽 슬롯과 recovery는 안드로이드로 남긴다. 정보원이자, 펌웨어를 뽑는 곳이자, 복귀 길이다. recovery는 따로 된 파티션이 없고 vendor_boot 안에 있다. GPUEB, `connsys_wifi`, `connsys_bt` 파티션도 지킨다. 이 펌웨어는 LK가 올린다(설계안 D6).

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

1. [ ] `/proc/iomem`과 살아 있는 트리(`/proc/device-tree`)를 뜬다. root 없이는 `/proc/iomem`이 막혀 있었고, `/proc/device-tree`는 이름만 보이고 값은 막혀 있었다(실측).

    ```bash
    cat /proc/iomem
    ```

2. [ ] framebuffer 주소·크기·형식을 확인한다. `/chosen`의 `atag,videolfb`와 `/reserved-memory`를 본다. LK는 화면 영역을 `mblock-14-framebuffer`로 예약한다(이름은 실측). 패널이 그리는 버퍼는 가로 2944×1840이다(실측, SurfaceFlinger의 `framebufferSpace`. 화면 회전 설정과 상관없다).
    - 후보(커뮤니티 포트): `0xfc16f000`, 2944×1840, stride 11776, `a8r8g8b8`, 예약 크기 `0x3e90000`(LK가 할당한 크기, 세 화면보다 조금 크다). 그 포트는 LK 기록(mblock 할당, `atag,videolfb`)에서 읽어 이 주소로 fbcon을 띄웠다(12GiB 기기). 우리 기기는 8GB다. 같은지 본다.
    - 맞으면: `framebuffer0`을 켠다. 부트로더가 예약하지 않으면 `framebuffer_mem`도 켜고, 이미 예약하면 `framebuffer_mem`을 지운다.
    - 다르면: 읽은 주소로 `simple-framebuffer`를 만든다.
3. [ ] 화면의 실제 크기를 확인해 둔다(벤더 값 273.615×171.009mm, 트리에 273×171을 넣었다). DSI에는 EDID가 없다.
4. [ ] ramoops 틈을 본다. 살아 있는 트리의 예약 영역으로 `0x48090000`~`0x4816ffff`가 비었는지 본다. 앞뒤는 `aee_debug_kinfo`(`0x48080000`+0x10000)와 `minirdump`(`0x48170000`)다.
    - LK는 pstore 자리를 `mblock-2-pstore`로 예약한다(이름은 실측, 값은 root 필요).
    - LK가 명령줄에 `ramoops.*`를 넣으면 노드와 겹쳐 장치가 둘 생긴다. 하나만 둔다.
    - 크기가 `0xe0000`을 넘으면 minirdump를 덮는다.
5. [ ] 예약 메모리의 실제 배치를 본 뒤 pstore 칸을 정한다. 정적 트리에는 동적 할당 셋뿐이고, TEE, GenieZone, connsys, framebuffer의 고정 예약은 preloader·LK가 부팅 때 더한다. BL31 자리는 `0x48800000`, 2MB다.
6. [ ] 입력 장치를 읽는다. 셸로는 열리지 않았다(실측).

    ```bash
    getevent -lp
    ```

7. [ ] 파티션 표를 뜬다. 기기 GPT의 형식 GUID는 아직 모른다. GPT의 userdata 자리를 보고, 새 루트 파티션의 크기와 이름을 정한다(보기: userdata 108G 가운데 32~48G). 여기서는 고치지 않는다. 고치는 일은 6.8절이다.
8. [ ] 펌웨어 파일을 얻는다. 2c의 터치에 `vendor/firmware/novatek_ts_fw_boe.bin`과 `novatek_ts_fw_tm.bin`이 있어야 한다. 파일은 저장소에 넣지 않는다. 고르는 일은 [extract.sh](../../bsp/lenovo-tb375fc/firmware/extract.sh)와 [manifest.tsv](../../bsp/lenovo-tb375fc/firmware/manifest.tsv)가 한다.
9. [ ] efuse 낱말 `0x1cc`(devinfo)의 [5:0]을 읽어 둔다. 6.7절 6번의 U2 INTR_CAL과 견준다.

## 5. 2a 첫 부팅

**목표.** 파티션을 건드리지 않고 `fastboot boot`로 RAM에서 우리 커널을 띄운다. 루트 파일 시스템은 initramfs다.

**종료 조건.** initramfs 셸이 화면이나 UART에 뜬다. 커널 기록으로 EL2를 판정한다.

통로는 화면 콘솔(simple-framebuffer + simpledrm), pstore/ramoops, LK 기록(expdb 파티션), UART다. USB 가젯은 아직 없다. UART를 찾지 못했으면 셸에 입력할 길이 없으므로, [init](../../bsp/lenovo-tb375fc/initramfs/init)이 화면에 찍는 줄(`cmdline:`, `vendor_boot:`, EL 줄)로 본다(이 문서의 해석이다. 10절 5번).

### 5.1 준비

1. [ ] 4절의 값으로 보드 dts의 `framebuffer0`·`ramoops`를 켠다. `uart0`은 켜져 있다. 2a 동안 uart0의 `bus`는 clk26m이다.
2. [ ] 이미지 넷(boot, vendor_boot, init_boot, dtbo)을 만든다. [BSP README](../../bsp/lenovo-tb375fc/README.md) "만들기" 1~3번이다. 입력, 명령줄, 해시는 `images.txt`에 남는다. 기기 없이 할 수 있는 시험(`boot/test.sh`, QEMU 맛보기, `initramfs/test.sh`)도 그 README에 있다.
3. [ ] `mkimages.sh --boot-ramdisk`로 만든 boot.img도 만들어 둔다. initramfs를 boot.img에도 넣은 이미지다.
4. [ ] 멈춤을 볼 도구를 고른다. 부팅 단계마다 화면에 블록을 그리는 카나리와 `panic=0`(패닉 화면을 남긴다)을 쓸 수 있다.
5. [ ] Android에서 100%까지 충전하고 `dumpsys battery`로 전압과 온도를 적어 둔다. 충전 드라이버가 없는 2a~2d 동안 배터리는 다음과 같다(MediaTek LK2 코드로 본 것이고 기기의 LK는 더 새 판이다).
    - LK는 입력과 충전을 500 mA로 두고 충전기 종류와 상관없이 그대로 넘긴다. 5 V 500 mA면 약 2.2 W이고 화면과 CPU 부하는 그보다 클 것으로 본다(추정, 측정 아님). PC나 벽 충전기에 꽂아 두어도 천천히 줄 수 있다.
    - 게이지가 없으면 잔량 경고도 저전압 종료도 없다. 마지막 방어선은 PMIC UVLO(순정 값 2.5 V)이고, 거기까지 가면 예고 없이 꺼지며 로그도 사라진다. 순정이 종료하는 값은 3.0 V 근처다.
    - 과충전 쪽 위험은 작다. 드라이버가 없으면 아무도 칩에 쓰지 않고, 칩 자체 보호는 LK가 둔 그대로다. 다만 온도에 따라 멈추는 소프트웨어 JEITA는 없다.
    - 몇 시간짜리 시험이면 중간에 잔량을 본다. 싼 차례는 게이지(I2C7 @0x55, 드라이버가 없어도 `i2cget`으로 읽기만), MT6363 AUXADC(`mt6359-auxadc`, 노드를 더해야 한다), Android로 재부팅, USB-C 전력계(입력이 500 mA에 붙어 있으면 부하가 입력보다 크다)다.
    - 전원을 끈 뒤 USB를 꽂으면 LK는 꺼진 채 충전하지 않고 커널을 띄운다(코드 기준). 충전이 필요할 때를 위해 Android로 돌아갈 길을 끝까지 남긴다. buntOS 커널을 굽는 시점에 다른 슬롯이나 되돌릴 이미지를 먼저 챙긴다.

### 5.2 `fastboot boot`

1. [ ] RAM으로 띄운다. LK의 fastboot 명령 표에 `download:`와 `boot`가 있다(LK 문자열). 파티션을 건드리지 않는 길이다(설계안 D3). 멈췄을 때 기기를 되살리는 키는 아직 모른다(10절 4번).

    ```bash
    fastboot boot boot.img
    ```

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

1. [ ] 굽기 전에 한 번 더 보려면 장치 트리를 커널에 넣은 시험용 커널을 `fastboot boot`로 띄운다. 이 커널은 LK가 더하는 메모리·예약 값을 버리므로, 그 기기의 메모리 크기에 맞춘 예약을 함께 넣어야 한다. BL31(`0x48800000`, 2MB)도 직접 예약한다. 안 하면 보조 CPU와 서스펜드가 멈춘다(커뮤니티 포트). 그 포트의 12GiB 고정값은 8GB 판에 맞지 않는다. 업스트림에는 내지 않는다.
2. [ ] 비운 슬롯에 넷을 굽는다. 파티션 이름과 슬롯은 `fastboot getvar all`로 확인한다. 우리 장치 트리로 띄우는 확실한 길이다.

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

1. [ ] 화면 콘솔이 뜨는지 본다. LK가 dual-DSI + DSC로 켜 둔 2944×1840 화면을 그대로 받는다.
2. [ ] 따뜻한 재부팅 뒤 ramoops가 지워지지 않는지 본다. init이 직전 기록을 `/run/buntos/pstore`에 떠 둔다. pstore 기록이 있는 경우는 QEMU에 ramoops 백엔드가 없어 시험하지 못했다.
3. [ ] 순정 recovery로 재부팅해 pstore와 LK 기록을 읽는 길을 본다. LK 기록은 expdb 파티션에 남고 순정 recovery에서 뜬다. 우리 vendor_boot를 구운 슬롯에서는 남겨 둔 슬롯으로 바꿔 recovery를 쓴다.
4. [ ] UART0을 본다(TX GPIO207, RX GPIO208, 921600bps). 부트로더가 UART0 클럭을 끄고 넘기면 레지스터 접근이 멈출 수 있다. 그러면 `uart0`을 끄고 화면과 pstore로 본다.

### 5.6 안정성

1. [ ] 워치독이 커널이 넘겨받은 뒤 다시 켜지지 않는지 본다(`CONFIG_MEDIATEK_WATCHDOG=y`).
2. [ ] idle·cpufreq를 본다. 멈추면 `cpuidle.off=1`, `initcall_blacklist=mtk_cpufreq_hw_driver_init`로 하나씩 뺀다.
3. [ ] systimer는 첫 부팅이 된 뒤에 켜 본다. 펌웨어가 같은 채널(`+0x40`)을 쓰지 않는지(멈춤, 인터럽트 폭주) 본다. 그 전에는 `disabled`이고 hrtimer broadcast가 대신한다.

### 5.7 전원 도메인 상태 (읽기만)

1. [ ] `0x1c001fb0`, `0x1c001fb4`(SPM 상태), `0x1c001e04`(conn), `0x10321410`(HW voter EN). 화면이 켜져 있으면 EN의 bit 0, 3, 4, 5, 6이 설 것으로 본다(짐작). 다르면 패치 0054의 표를 의심한다. 물리 주소 읽기는 1절의 시험 도구를 쓴다.

### 5.8 EL2 판정

1. [ ] 커널 기록의 `CPU: All CPU(s) started at EL2`(또는 `EL1`)와 kvm 초기화 메시지를 본다([buntOS 설계안](../design/ARCHITECTURE.md) 12.1절). 안드로이드 쪽 예비 판정은 GenieZone이 EL2를 차지할 가능성이 높다는 것이다.
    - 가능: 커널이 EL2에서 시작하고 `/dev/kvm`이 생긴다.
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

1. [ ] `pio`를 켠다. 드라이버는 패치 0004~0006, 0027, 0028이다. 켜면 probe가 EINT 마스크를 모두 세운다(업스트림 동작). 핀 상태(`pinctrl-0`)는 쓰는 노드가 생길 때 더한다.

### 6.2 클럭

드라이버는 패치 0009~0013이다. 표는 OEM 코드에서 옮겼고 기기 시험은 없다.

1. [ ] 클럭 컨트롤러 여섯(topckgen, apmixedsys, vlpckgen, pericfg_ao, ufscfg_ao, ufscfg_pdn)과 HW voter syscon(`0x10320000`)을 켠다. uart0의 `bus`를 pericfg_ao 게이트로 바꾼다. `clk_ignore_unused`는 그대로 둔다. 없으면 simple-framebuffer가 쓰는 화면 먹스(disp·ovl·mminfra)와 쓰지 않는 UFS 게이트가 늦은 초기화에서 닫힌다.
2. [ ] apmixedsys가 뜨는지 **먼저** 본다. mainpll은 critical이라 등록할 때 바로 HW voter로 투표한다.
    - 패치 0012의 순서가 틀리면 `critical clk 'mainpll' failed to prepare`가 난다. 그러면 apmixedsys가 실패해 모든 클럭이 고아가 되고, UART·UFS가 클럭을 기다리며 뜨지 않는다.
    - 처음 부팅은 화면 콘솔과 pstore로 본다.
3. [ ] 클럭 값을 읽는다.

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
7. [ ] `clk32k`의 펌웨어 값 32000 Hz가 맞는지(26 MHz에서 나눈 32K인지, RTC 수정의 32768인지) 안드로이드나 커널 기록으로 본다. 2b 소비자에는 영향이 없다.

### 6.3 SPMI

고침은 패치 0002·0003(백포트)과 0007·0008, 0114다.

1. [ ] `pio`와 함께 SPMI를 `okay`로 바꾼다. PMIC 여섯(MT6363 @4, MT6368 @5, MT6319 @6~8, MT6685 @9)은 모두 M 버스에 있다. M 자식만 켜고 P 자식은 끈다. 별칭은 `spmi0` 하나다. MT6685 @9는 6.10절에서 켠다.
2. [ ] probe가 클럭 때문에 실패하지 않는지 본다. 업스트림은 자식마다 `rcs` 인터럽트(pio EINT 248)와 클럭 셋(`pmif_sys_ck`, `pmif_tmr_ck`, `spmimst_clk_mux`)을 요구하고, 없으면 probe가 실패한다. PMIF 클럭으로 `vlp_pwrap_ulposc`를 준 것은 추론이다. `vlp_pwrap_ulposc`·`vlp_spmi_m`은 게이트가 없어 켜기가 아무 일도 하지 않는다.
3. [ ] 패치 0007을 확인한다. 고치기 전 rcs 디먹스는 `bank = bit / 7`이라 bit 7, 14–15, 21–23, 28–31에서 bank가 하나 커지고, 원래 비트가 남아 체인 IRQ 처리기가 끝나지 않을 수 있었다(코드로 짐작). 6.4절 3번과 6.5절 4번에서 인터럽트가 가만히 있는지 본다.
4. [ ] 패치 0008을 확인한다. 고치기 전에는 채널 설정이 버스 등록 뒤에 있어, 내장(=y) PMIC 드라이버가 먼저 probe하면 SWINF 상태 대신 `PMIF_INIT_DONE`을 읽고 명령도 그 자리에 썼다(코드로 짐작). rcs 인터럽트나 클럭을 기다리다 PMIF probe가 미뤄지면 이 경우에 든다. MT6363, MT6368, MT6319의 probe가 되는지 본다.
5. [ ] 패치 0114를 확인한다. 고치기 전에는 처리기 없는 SID의 RCS를 아무도 지우지 않아 EINT 248이 계속 울릴 수 있었다(코드로 짐작). SID 4·5의 체인 처리는 그대로이고 칸 넷을 읽는 것만 는다(벤더도 인터럽트마다 다 읽는다). `dmesg | grep 'cleared event'`에 줄이 있으면 그 SID와 값을 적어 둔다. MT6319 @6~8에서 나올 수도 있다.

    - `Err:`가 계속 늘면(이벤트 없이 부모가 돈다) P 버스 칸을 의심한다. 벤더는 EINT 248 하나로 M과 P의 칸을 함께 읽고 지운다. 우리는 P를 끈 채 M만 보므로 P 칸에 남은 값을 지우지 못한다. 줄이 두 버스의 OR인지는 코드로 모른다. P 칸은 `0x1cc00020`~`0x1cc0002c`이다. 읽기 전에 `clk_summary`에서 `vlp_spmi_p`가 켜져 있는지 본다(꺼져 있으면 읽기가 버스를 멈출 수 있다).

    ```bash
    for a in 20 24 28 2c; do busybox devmem 0x1cc000$a 32; done
    ```

6. [ ] 0007·0008·0114는 기기에서 확인한 뒤 업스트림에 낸다.

### 6.4 MT6319·MT6363

패치 0014~0018(MFD, 키)과 0037~0041(regulator 고침)이다. MT6319는 DT만 쓴다. 고침은 코드와 레지스터 모델로 확인했을 뿐 기기에서 돌려 보지 않았다. 모델도 벤더 자료로 만든 것이다.

이 단계에서는 보드 방침을 바꾸지 않는다. 전압 한계를 주지 않고, 펌웨어가 늘 켜 두는 레일은 always-on으로 둔다. 바꿀지는 `TOP_TRAP`과 VEMC 전압을 본 뒤 정한다. 벅 끄기를 일부러 시험하지 않는다. 소비자가 생길 때 VS1·VS3가 그대로인지 `regulator_summary`로 본다.

1. [ ] probe를 본다. MT6363 MFD·regulator·키, MT6319 셋이다.

    ```bash
    dmesg | grep -i -E "spmi|mt6363|pmic|regulator"
    ```

    - `regulators@30` 아래 레일 등록을 본다. 노드가 있는 것은 23개이고 드라이버 표는 35개다. probe가 표 전체를 돌므로 `regulator_summary`에 몇 줄이 나올지는 기기에서 본다.
    - 없어야 하는 줄: "Cannot mask interrupts", "Interrupt N is in no interrupt group", "Could not clear RCS flag".
2. [ ] 레일을 읽는다.

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
6. [ ] `TOP_TRAP`과 VEMC 필드를 읽는다.

    ```bash
    cat /sys/kernel/debug/regmap/0-04-regulator/registers
    ```

    - 주소: `0x6`(TOP_TRAP, 절대 주소 `0x36`), `0x1b10`(VOSEL), `0x1b1b`(VOCAL). 0x30을 뺀 값이고, 8비트가 되어 칸마다 한 레지스터다.
    - TOP_TRAP이 1이면 VOCAL의 [7:4]와 [3:0]을 함께 적는다. TOP_TRAP=1의 VOCAL 위 네 비트는 MT6363에 대한 직접 근거가 없는 추정이고, 모델도 그 추정을 품는다. 틀리면 VOCAL 위 네 비트를 잘못 읽고 쓴다. 그 값이 11 이상으로 읽히면 다음 범위 전압이 되고, 선택자가 176을 넘을 수도 있다.
    - TOP_TRAP이 0이면 이 길을 타지 않는다.
    - TOP_TRAP이 1보다 크면 VEMC 콜백이 `-EINVAL`을 낸다.
7. [ ] `regulator_summary`의 `vufs_vcc`(VEMC)를 본다. 7.2.12에서 1.2V나 1.3V로 나오던 값이 VOSEL 값과 같은 전압으로 나와야 한다(VOSEL 6 = 2.5V 등).
8. [ ] 다른 LDO의 VOCAL이 11~15인 곳이 있는지 본다. 범위 모델은 VOCAL을 0~10으로 보고, 11 이상이면 다음 범위로 읽는다.
9. [ ] `mt6363_vsram_cpub·cpum·cpul`이 켜져 있는지, 그 레지스터(`0x1dd7`, `0x1ded`, `0x1e57`)의 비트 0을 본다. 부트로더가 0으로 두었다면 always-on이 켜므로 부팅 기록에 이상이 없는지 본다(패치 0041).

### 6.5 MT6368

패치 0023~0026이다. 보드는 펌웨어가 늘 켜 두는 레일 6개를 둔다. ssusb는 이 PMIC의 VUSB(`vusb33-supply = <&mt6368_vusb>`)를 기다린다. `CONFIG_REGULATOR_MT6368=y`여야 하고, SPMI M 버스와 MFD가 함께 있어야 한다.

1. [ ] probe를 본다.

    ```bash
    dmesg | grep -i -E "mt6368|spmi|regulator"
    ```

    `pmic@5` MFD probe(칩 ID는 SWCID 0xB), `0-05-regulator` 하위 장치, 레일 등록을 본다. 드라이버 표는 24개이고 노드가 있는 것은 6개다. "Cannot mask interrupts", "Interrupt N is in no interrupt group", "Could not clear RCS flag"가 없어야 한다.
2. [ ] `regulator_summary`에서 `vgpu`, `vusb33`, vbuck5, vbuck6, vio28, vtp가 켜져 있고 늦은 정리에서 꺼지지 않는지 본다.
    - 전압: vio28은 펌웨어대로 1.8V여야 한다. vusb33은 mtu3 바인딩이 말하는 USB AVDD 3.3V 공급이다. vgpu는 부트로더 값이다.
    - VOCAL 모델이 맞는지 이것으로 본다. LDO 전압이 표 값 + 10mV×VOCAL과 어긋나면(특히 VOCAL이 11~15) 모델을 고친다.
3. [ ] regmap debugfs로 VOCAL 레지스터 몇 개(vusb `0x1d94`, vio28 `0x1d9c`, 벤더 주소)를 읽어 둔다. 드라이버의 주소는 벤더 주소 − 0x30이다.
4. [ ] `/proc/interrupts`의 `mtk-spmi-pmic` 줄이 가만히 있고 `Err:`가 계속 늘지 않는지 본다. SID 5의 RCS가 pmif에서 bank 1(비트 8~15)로 오는지는 패치 0007과 함께 본다.
5. [ ] 끈 뒤 전류가 0에 가까운지 본다(ELR5로 MT6368이 끄기 순서를 따르는지). 재부팅에 문제가 없는지 본다.

    ```bash
    poweroff
    ```

### 6.6 UFS

패치 0019~0022이다. 클럭·리셋 이름과 OPP는 업스트림 MT8196 시리즈(v14, 병합 전) 꼴이다. 호스트 클럭은 게이트 여섯(`main`, `aes`, `unipro_sysclk`, `tx_symbol`, `rx_symbol0`, `rx_symbol1`)이고 PHY는 클럭을 받지 않는다. 클럭 스케일링은 없다. `vcc`는 MT6363 VEMC, `vccq`는 MT6319(SPMI 8) vbuck3다.

> **위험: 데이터 손실.** UFS에는 안드로이드 파티션과 nvdata가 함께 있다. 6.8절의 GPT 고침 전에는 읽기만 한다. initramfs의 실패 길은 nvdata·nvcfg·persist를 읽기 전용(`blockdev --setro`, `ro,noload`)으로 붙인다.

1. [ ] probe 차례를 본다: ufscfg_ao·pdn(클럭·리셋) → `112a0000.ufs-phy`(MT6897 ops) → `112b0000.ufshci`.
    - 기대: 기록에 `caps: 0x7c0`, `clk "main" present` 등 여섯 줄, "Clk-scaling not ready. Feature disabled."가 나온다.
    - "Failed to get resets"가 나오면 리셋 기술이 틀린 것이다. 리셋은 ufscfg_pdn의 set `0x48`·clr `0x4c` bit 0·1·2(UniPro, crypto, HCI)다.
    - "mode operation not allowed"는 MT6319 벅에 `regulator-allowed-modes`가 없어 남는 기록이다. 동작에는 상관없다.
2. [ ] RTFF MTCMOS SiP 호출을 본다. 초기화에서 MTCMOS를 끄고 켠다(벤더와 같은 순서). `caps` 줄 바로 뒤에서 멈추면 이 길이다. 그때는 match data에서 `UFS_MTK_CAP_RTFF_MTCMOS`를 빼 본다(드라이버 한 줄).
3. [ ] 호스트 리셋을 본다. HCE enable 때 리셋 셋을 100µs 걸었다 푼다. "Host reset assert/deassert failed"가 없고 링크가 서는지 본다.
4. [ ] 먹스 부모를 본다. `clk_summary`에서 `ufs`·`aes_ufsfde`의 주파수를 읽는다. LK가 26 MHz(부모 0)로 두었으면 느리다. 그때는 dtsi에 `assigned-clocks`로 벤더의 낮은 부모(mainpll_d4_d2, univpll_d6)를 준다. UFS 3.x 장치면 벤더도 낮은 쪽으로 고정한다(`ufs_mtk_fix_clock_scaling`).
5. [ ] 기어와 전력 모드를 본다: HS 기어·rate, PMC via FASTAUTO 길, 장치 판(`wspecversion`).
6. [ ] 클럭 게이팅과 런타임 PM을 본다. idle 뒤 hibern8과 ref 클럭 SiP 핸드셰이크("missing ack of refclk req"가 없어야 한다), 런타임 suspend의 MTCMOS 끄기·켜기 뒤 I/O를 본다.
7. [ ] UFS가 뜬 뒤 VEMC가 그대로 켜져 있고 런타임 PM에서 문제없는지 본다.
8. [ ] M-PHY를 본다. 커널은 M-PHY 레지스터를 건드리지 않는다. 높은 기어에서 링크가 흔들리면 벤더(ATF·UniPro 속성)와 견준다.

### 6.7 USB 가젯

mtu3·xsphy, 보드 `dr_mode = "peripheral"`, configfs ACM + NCM이다. 호스트 쪽은 같은 커널의 `cdc_acm`·`cdc_ncm`으로만 시험했고 가짜 UDC(`dummy_hcd`) 위였다. macOS·Windows의 NCM은 보지 못했다.

> **위험: USB 끊김.** ssusb를 켤 때 SPMI M 버스와 MT6368(MFD·regulator)이 켜져 있지 않으면 mtu3가 공급을 기다리며 뜨지 않는다. 포트가 하나이고 호스트 모드가 없어 이 길이 화면 말고는 유일하다.
>
> **먼저 둘 것.** 6.3~6.5절의 노드, 화면 콘솔과 pstore.

1. [ ] mtu3가 `vusb33`을 받아 뜨는지 본다. 더미 공급 경고 "supply vusb33 not found"가 없어야 한다.
2. [ ] `dr_mode`와 UDC 이름, 실제 열거를 본다. `dummy_udc`와 달리 초기화 순서가 다를 수 있어 서비스가 `buntos.wait`초 기다린다.
3. [ ] 호스트에서 직렬 포트에 들어간다. 호스트의 장치 이름은 macOS `/dev/cu.usbmodem*`, Linux `/dev/ttyACM0`이다. USB 제품 문자열로 어느 쪽 가젯인지 안다(`TB375FC initramfs`, `TB375FC`).

    ```bash
    screen /dev/ttyACM0 115200
    ```

4. [ ] 네트워크를 본다. 기기는 172.16.42.1/24, 호스트는 DHCP로 172.16.42.2를 받는다. initramfs에서는 `buntos.telnet=1`이면 telnetd가 뜬다. 옵션은 [init](../../bsp/lenovo-tb375fc/initramfs/init) 머리말에 있다.
5. [ ] 속도를 본다. USB3는 PS5170 리드라이버가 있어야 해 그 드라이버 전에는 HS로만 붙는다(설계안 4절). SS가 흔들리면 벤더가 MT6897에만 넣는 U3 RX CDR 값부터 본다(U3 포트 `+0x408`의 [23:16] `RG_XTP_LN0_RX_CDR_RESERVE` = 0x8).
6. [ ] U2 INTR_CAL을 본다. `0x11e40330`의 [29:24]를 읽어 LK가 둔 값을 적고, 4절 9번의 efuse 값과 견준다. 다르면 xsphy MT6897 꼴(칸과 nvmem)을 만든다.
7. [ ] eye 값을 본다. `0x11e40304`의 [14:12] = 7, [10:8] = 3인지 본다.

### 6.8 루트 파티션

자리는 userdata를 줄여 만든 새 GPT 파티션이다. 형식 GUID는 root-arm64(`b921b045-1df0-41c3-af44-4c6f280d3fae`)이고 명령줄은 `root=PARTLABEL=<이름>`이다. USB-C 저장장치 길은 쓰지 않는다. 전체 이미지(`tools/image/build.sh lenovo-tb375fc`)를 만든 기록은 아직 없고, 작은 시험 이미지로만 봤다.

이미지의 systemd가 UFS의 GPT를 고치지 못하게 `systemd-repart`와 `systemd-gpt-auto-generator`를 보드 프로필과 initramfs에서 가렸다. 이 가림을 풀지 않는다. QEMU에서 GPT와 다른 파티션의 해시로 확인했다.

1. [ ] 가젯 셸에서 UFS를 읽기 전용으로 확인하고 파티션 표를 읽는다. 커널이 MediaTek GPT를 읽어 `PARTNAME`이 나오는지 본다. LU 여럿에서 같은 이름이 없는지 본다. 지금은 먼저 찾은 것을 쓰고 겹침을 따로 알리지 않는다.
2. [ ] 크기와 이름을 정한다(4절 7번).
3. [ ] 고치기 전의 GPT(앞·뒤 표)를 떠서 기기 밖에 둔다.
4. [ ] GPT를 고친다.

    > **위험: 부팅 불가·데이터 손실.** preloader·LK가 고친 GPT를 읽지 못하면 부팅이 막힌다. userdata를 한 번 지운다(공장 초기화). 안드로이드 슬롯은 남는다.
    >
    > **먼저 둘 것.** 3번의 GPT, 2.3절 백업. 언락 직후라 잃을 데이터가 적을 때 한다.
    >
    > **되돌리기.** 언락 전에 뜬 SP Flash 백업이다. 고친 GPT는 SP Flash Tool scatter와 달라지므로 고친 GPT도 떠 둔다.

5. [ ] preloader·LK가 고친 GPT를 어떻게 읽는지, LK의 fastboot가 새 항목을 아는지 본다.
6. [ ] 루트 파티션 파일(`buntos.root-arm64.raw`)을 굽는다. fastboot로 굽고(sparse), 안 되면 가젯으로 `dd`한다. 이미지의 ESP와 UKI는 LK가 쓰지 않으므로, 이미지에서는 이 파일만 굽는다.

    > **위험: 데이터 손실.** `dd`의 대상은 새 파티션뿐이다. userdata 전체, super, 루프 파일은 쓰지 않는다. 안드로이드 슬롯을 지키지 못하거나 metadata 암호화로 열 수 없다.

7. [ ] 명령줄을 넣어 부트 이미지를 다시 만든다. `<이름>`은 2번에서 정한 값이다(`PARTUUID=`, `UUID=`, `/dev/…`도 받는다). 공개 키는 ed25519를 쓴다. RSA 4096은 명령줄 한도에 빠듯하다. 값의 뜻은 [BSP README](../../bsp/lenovo-tb375fc/README.md)의 "루트로 넘기기"에 있다.

    ```bash
    limactl shell --workdir "$PWD" buntos-dev -- bash -lc 'x=$(mktemp -d) && dpkg-deb -x ~/buntos-build/tb375fc/deb/linux-image-7.2.12-tb375fc_*_arm64.deb $x && bsp/lenovo-tb375fc/initramfs/mkinitramfs.sh --modules $x && bsp/lenovo-tb375fc/boot/mkimages.sh --kernel $x/boot/vmlinuz-7.2.12-tb375fc --dtb $x/usr/lib/linux-image-7.2.12-tb375fc/mediatek/mt6897-lenovo-tb375fc.dtb --initramfs ~/buntos-build/tb375fc/images/initramfs.cpio.lz4 --cmdline "console=ttyS0,921600n8 console=tty0 earlycon clk_ignore_unused pd_ignore_unused regulator_ignore_unused root=PARTLABEL=<이름> rootfstype=btrfs rw systemd.firstboot=no buntos.usbdebug=1 systemd.set_credential_binary=ssh.authorized_keys.root:$(base64 -w0 <공개 키 파일>)"'
    ```

8. [ ] LK가 `root=`와 `buntos.usbdebug=1`을 커널에 넘기는지 본다. `/proc/cmdline`이 `/run/buntos/boot.log`에 남는다. LK가 자기 `root=`나 `ro`를 덧붙여도 우리 값이 이긴다.

    > **위험: USB 끊김.** 진짜 루트의 USB 길은 명령줄에 `buntos.usbdebug=1`이 있을 때만 돈다. 없으면 넘어간 뒤 가젯이 없다.
    >
    > **먼저 둘 것.** LK가 이 값을 넘기는지 `/proc/cmdline`으로 본다. 안 넘어오면 커널 `CMDLINE`(+`CMDLINE_EXTEND`)로 주고 `buntos.usbdebug=1`을 함께 넣는다. 그때 공개 키는 커널 설정에 넣지 말고 ttyACM 셸에서 넣는다.

9. [ ] 못 넘어갔으면 까닭을 본다. 패닉 없이 셸이 뜨고, 까닭은 화면, dmesg, pstore, `/run/buntos/boot.log`에 남는다. 찾지 못했으면 `/run/buntos/block.txt`에 장치와 GPT 이름 목록이 있다. UFS가 뜨는 시간도 본다(`buntos.wait` 기본 30초면 되는가).
10. [ ] 들어간다. 넘어가는 동안 USB가 잠깐 없다. 가젯은 진짜 루트에서 새로 띄운다.

    ```bash
    ssh root@172.16.42.1
    ```

    공개 키가 아직 없거나 네트워크가 안 되면 ttyACM의 암호 없는 root 셸을 쓴다. 셋 다 다른 서비스를 기다리지 않아, sysinit이 멈추거나 emergency.target으로 넘어가도 들어갈 수 있다.
11. [ ] 실제 UFS의 btrfs를 본다. `discard=async`가 저절로 켜진다(QEMU의 마운트 옵션에서 봤다). UFS의 UNMAP과 안드로이드 쪽 영향은 파티션 안이라 없을 것으로 보지만 기기에서 본다.
12. [ ] 넘어간 뒤의 화면을 본다. simpledrm의 fbcon이 systemd, greetd를 지나도 사는지 본다.
13. [ ] 워치독을 본다. `mtk_wdt`는 내장이고 넘어가도 커널이 먹인다(`WATCHDOG_HANDLE_BOOT_ENABLED=y`). systemd의 `RuntimeWatchdogSec`을 켜지 않아도 오래 사는지 본다.
14. [ ] 안드로이드 파티션이 70개쯤일 때 `systemd-udevd`와 `blkid`가 걸리는 시간을 본다. UFS를 가리는 규칙(`90-buntos-tb375fc-hide-ufs.rules`)은 부모 사슬만 보고 프로그램을 돌리지 않는다(시간은 재지 않았다).
15. [ ] 실제 크기의 btrfs 늘리기와 첫 부팅 시간, `systemd-firstboot`(`systemd.firstboot=no`)를 본다. 늘리기는 `systemd-growfs-root.service`이고 파일 시스템만 늘린다.
16. [ ] pstore를 본다. ramoops 자리를 정한 뒤 `/run/buntos/pstore`와 `systemd-pstore`(`/var/lib/systemd/pstore`)에 남는지 본다.
17. [ ] 가상화 판별이 `none`인지 본다. `buntos-vm-sleep.service`가 `ConditionVirtualization=vm`이라 기기에서는 돌지 않아야 한다.

    ```bash
    systemd-detect-virt
    ```

### 6.9 2b 뒤에 남는 일

기기에서 UFS·USB가 뜬 뒤에 한다.

- UFS 클럭 스케일링(먹스 부모, 게이트의 `CLK_SET_RATE_PARENT`).
- MCQ. 공통 바인딩의 인터럽트 수를 고치고, 큐 인터럽트 SPI 177~184를 넣고, `mediatek,ufs-disable-mcq`를 뺀다. 이 속성을 뺀 시험은 그 뒤에 한다.
- xsphy MT6897 꼴(U2 INTR_CAL·efuse, U3 RX CDR), mtu3 깨우기 접점(벤더 판 103).
- U-Boot 체인로드로 EFI(systemd-boot, UKI) 부팅을 검토한다(설계안 D3).
- EFI 길이 열리거나 LK의 벤더 램디스크 한도를 기기에서 재면 mkosi initrd를 vendor_boot에 넣는 길을 다시 볼 수 있다. 지금은 우리 initramfs다.
- 기기에서 커널을 바꾸는 길은 남은 결정이다(LK 부트 이미지를 다시 만들어 쓴다).

### 6.10 MT6685 RTC

패치 0110~0113이고, 남은 알람은 PMIF 고침 0114와 함께 본다. 코드와 레지스터 모델로 확인했을 뿐 기기에서 돌려 보지 않았다. 모델도 벤더 자료로 만든 것이다. 2b의 USB 가젯 셸이 뜬 뒤에 한다. 안드로이드와 LK가 같은 칩을 쓰므로 읽기부터 한다.

**넘어가기 전(안드로이드, adb, 잠긴 채로 된다):**

1. [ ] 11번을 볼 때까지는 안드로이드의 전원 꺼짐 알람을 끄고 넘어간다. 알람 시계 앱의 알람을 모두 끄고 재부팅한다. 11번에서 이벤트가 한 번 지워지고 끝나면 이 줄은 뺀다.

    - 까닭: 벤더 드라이버는 전원 꺼짐 알람이 있으면 종료 때 `IRQ_EN.AL`을 켠 채 둔다. 그 시각이 리눅스에서 오면 칩이 SID 9로 RCS를 낼 수 있다. 패치 0114가 처리기 없는 SID의 이벤트를 지우므로, 이벤트가 한 번이면 EINT 248은 계속 울리지 않는다.
    - 0114가 막지 못하는 경우: 칩이 원인(`SCK_TOP_INT_STATUS0`, `RTC_IRQ_STA`)이 지워질 때까지 RCS를 곧바로 다시 내면 처리기가 계속 돈다. 리눅스는 그 칸을 건드리지 않는다. 칩이 어느 쪽인지는 코드로 알 수 없다(벤더 코드에는 MT6685에 RCS 완료를 쓰는 곳이 없어 MT63xx의 뜻을 옮길 수 없다).

2. [ ] 안드로이드에서 시간을 읽어 둔다(막히면 건너뛴다. 까닭: 레지스터가 UTC인지, 리눅스가 같은 값을 읽을지 맞대려는 것이다).

    ```bash
    adb shell 'date -u; cat /sys/class/rtc/rtc0/date /sys/class/rtc/rtc0/time /sys/class/rtc/rtc0/since_epoch'
    ```

**리눅스:**

3. [ ] `pmic@9`만 `okay`로 바꾼다(`rtc@580`은 그대로). 이 드라이버 때문에 버스에 닿는 것이 없다(MFD는 자식만 띄운다).

    ```bash
    ls -l /sys/bus/spmi/devices/0-09/driver    # mtk-spmi-pmic
    ls /sys/class/rtc                          # 비어 있다
    ```

4. [ ] `rtc@580`도 `okay`로 바꾼다. 부팅 로그를 본다.

    ```bash
    dmesg | grep -i -E 'rtc|mt6685'
    cat /sys/class/rtc/rtc0/name /sys/class/rtc/rtc0/date /sys/class/rtc/rtc0/time /sys/class/rtc/rtc0/hctosys
    ```

    - `registered as rtc0`, 그리고 안드로이드에서 넘어온 뒤 날짜와 시각이 맞아야 한다(1~2번의 값). 처음부터 칩이 깨끗하면 2010년 근처일 수 있다.
    - **연도가 2086이나 2094면** 연도 칸의 해석이 틀린 것이다. 여기서 멈추고 `hwclock -w`를 하지 않는다. 안드로이드로 돌아가면 그 값을 읽는다.
    - 칩이 연도 칸 0·1(1968·1969)이나 틀린 날짜를 담고 있으면 `-EINVAL`이고 시스템 시간을 건드리지 않는다.

5. [ ] 시간이 흐르는지 본다. **이것이 드라이버의 가장 큰 가정이다.** 레지스터가 클럭 게이트를 잠깐 여는 것만으로 카운터를 따라온다고 본다(OnePlus 커널과 2025년 뒤 MediaTek 판). 이 기기의 스톡 모듈은 더 옛 판이라 BBPU `RELOAD`를 쓴다.

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

**첫 소비자 메모:** 오디오(단계 3)의 `mt6368` 코덱은 벤더에서 `mt6685_set_dcxo()`로 BBCK5(`DCXO_EXTBUF5_CW0` `0x79e`)를 켠다. 그때까지 XO에 쓰는 코드는 없다.

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

2. [ ] 터치 펌웨어를 넣는다. 드라이버는 터치를 열 때 루트에서 읽는다(`TOUCHSCREEN_NOVATEK_NVT_SPI=m`).
    - `vendor/firmware/novatek_ts_fw_boe.bin` → `/lib/firmware/lenovo/tb375fc/novatek_ts_fw_boe.bin`
    - `vendor/firmware/novatek_ts_fw_tm.bin` → `/lib/firmware/lenovo/tb375fc/novatek_ts_fw_tm.bin`
3. [ ] 노드를 켠다. pio·클럭 컨트롤러에 더해 `spi2`, `gpio-keys`, `vddio_1v8`, `uart3`이다. `vddio_1v8`(GPIO63)은 패널과 터치가 같이 쓰는 I/O 전원이라 simple-framebuffer 동안 꺼지면 안 된다. 그래서 `regulator-always-on`이다.
4. [ ] 패널 노드와 `framebuffer0`의 `panel`이 트리에 있다(꺼 둠). simpledrm 커넥터의 크기(`modetest` 또는 `/sys/class/drm`)가 273×171mm인지, 기본 배율이 225%인지 본다.
5. [ ] 전체 이미지에서 `greetd`·`bunt-comp`와의 관계, 공통 설치 뒤 스크립트와의 순서(보드 것이 먼저 돈다)를 본다. 작은 시험 이미지에는 데스크톱 꾸러미가 없어 보지 못했다.

### 7.2 터치

패치 0027~0031이다. 프로토콜은 OEM 코드, 패널 판별과 펌웨어 이름은 제3자 미러, 좌표 범위는 실측에서 왔다. 기기 시험은 없다.

1. [ ] spi2 probe를 본다. `/sys/kernel/debug/clk`에서 spi2 먹스의 부모와 값을 읽는다.
    - SCLK는 9.6 MHz 아래여야 한다. 부모 182 MHz면 9.1 MHz, 208 MHz면 9.45 MHz다(코드로 본 것).
    - 전송이 깨지면 `cs-gpios = <&pio 12 GPIO_ACTIVE_LOW>`(GPIO12를 GPIO로)를 쓴다. OEM의 소프트웨어 CS를 업스트림이 쓰지 않는다. 이 드라이버는 전송 하나짜리 메시지만 써서 차이가 작을 것으로 보지만 기기에서 본다.
2. [ ] 터치 probe를 본다. `dyndbg`로 `novatek_nvt_spi`를 켠다.
    - trim ID: `xx xx xx 32 65 03`.
    - 패널 ID(GPIO184): 0이면 BOE(`novatek_ts_fw_boe.bin`), 1이면 Tianma와 Tianma 무광(`novatek_ts_fw_tm.bin`). LK 기록의 패널과 맞는지 본다. 패널 드라이버도 이 줄로 고른다.
3. [ ] 열 때를 본다(`evtest`). 펌웨어는 터치를 열 때 내려받는다. 리셋 상태 0xA0·0xA1, 펌웨어 판, 손가락 좌표를 본다. 컨트롤러 좌표는 X 0~18400, Y 0~29440이고 픽셀의 10배다(실측).
4. [ ] 화면 네 모서리를 눌러 축을 정한다. `touchscreen-inverted-x/y`를 여기서 정한다. `touchscreen-swapped-x-y`는 보드에 있다.
5. [ ] 서스펜드·복귀 뒤 터치와 시간을 본다. 복귀 때 펌웨어를 다시 올린다.
6. [ ] 펌웨어 워치독 복구를 본다. 드물다. 일어나면 기록에 "controller was reset"이 남는다.

### 7.3 펜

1. [ ] 압력, 기울기, 거리(호버), 단추 둘을 본다. 범위는 압력 0~4095, 기울기 ±60°, 거리 0~1이다(실측).
2. [ ] 나오지 않으면 펜 켜기 인자(`0x7B 0x01`)부터 본다. 이 기기 드라이버(제3자 미러)는 `7B 01`이 켜기이고, TB371FC 커뮤니티 포트의 펌웨어에서는 `7B 01`이 끄기다. 명령이 실패해도 드라이버는 경고만 한다.

### 7.4 홀 센서

1. [ ] 덮개를 닫고 열 때 `SW_LID`를 본다(`evtest`). GPIO1이 0이면 덮개 닫힘이다. 보드 노드는 `gpio-keys`의 `switch-lid`다(active low, 100 ms, `wakeup-source`).
2. [ ] 펌웨어의 `hall_2`(GPIO26)는 키보드의 붙음·뗌 줄이다. `gpio-keys`에 없고 키보드 드라이버가 가진다. 7.5절 2번에서 본다.

### 7.5 포고 키보드

패치 0032~0036이다. 프로토콜은 글로벌판(TB373FU) 스톡 모듈을 읽은 것이고, 사용자 기기는 중국판 ZUXOS다. 키보드 MCU는 액세서리 쪽이라 프로토콜이 같을 것으로 본다(판단). 실물 확인은 이 절이다. 드라이버는 사용자 공간 시뮬레이터로 돌렸고 기기 시험은 없다. 3번이 프로토콜 판단을 가르는 첫 확인이다.

1. [ ] UART3 패드 모드(`mediatek,syscon-pad-mode`, pericfg_ao `0x50`)를 본다. 켠 뒤 `0x11036050`의 값은 bit 4 = 1, bit 0 = 0이어야 한다. 스톡 로그의 "first get uart3_def_value"처럼 바뀌기 전 값(리셋 값)도 본다. 이 비트 없이는 UART3이 패드로 나가지 않을 수 있어 필요하다고 판단했다.
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
4. [ ] 시험 시간은 수십 초로 하고 커넥터 온도를 손으로 본다. 과열 보호가 없다(순정의 `usb_board_ntc` 드라이버가 이 커널에 없다). 게이지(MM8013)를 켰으면 배터리 전류도 본다.

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

2d 무선의 차례는 9.1절과 9.2절에, 2e 충전과 TCPC는 9.3절에, 2e GPU·열·게이지는 9.4절에 있다. 2c 화면(2차)과 3 최적화의 목록은 나중에 더한다.

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
- 2e: 전원 키로 s2idle에서 깨는지, mcusys-off·system idle 상태와 s2idle(SPM), DSU PMU(SPI 35), `#cooling-cells`, mtu3 깨우기 접점, 커넥터 과열 보호(`usb_board_ntc`, GPIO179). `clk_ignore_unused`를 뺄 때 `mfgsc_ref`에도 `CLK_IGNORE_UNUSED`를 준다(패치 0013은 `mfg_ref`에만 있다).
- 2c 2차(화면 파이프라인이 온 뒤):
    1. 패널이 뜨는지(첫 prepare 깜빡임 포함), 144·120·60·30Hz 전환과 실제 주사율을 본다. 벤더 PHY 기준으로 계산한 값은 143.97, 119.57Hz다.
    2. DCS 0x51로 밝기가 도는지, 0에서 어두워지는지, 화면을 끌 때 LED가 꺼지는지(SGM37604A), 대기 전류를 본다.
    3. 편향을 껐다 켠 뒤 전압이 유지되는지 본다(SM5109). SGM37604A 둘과 SM5109의 드라이버는 아직 없다. 이 결과로 필요한지 정한다.
    4. 터치를 패널 팔로워로 잇는다(`drm_panel_follower`). 벤더 터치는 화면 통지로 재시작하고 패치 0031은 시스템 PM뿐이라, 화면을 끄고 켠 뒤 터치 펌웨어가 죽을 수 있다(TDDI). 패널 리셋(GPIO41)과 터치 리셋(GPIO60)은 다르다. 끌 때 GPIO184를 0으로 둘지도 본다.
    5. 패널 노드를 켜면 GPIO184가 터치와 공유되어 `gpio-shared-proxy` 모듈이 있어야 한다. 없으면 터치와 패널이 둘 다 미뤄진다. udev가 올리고, 터치나 패널을 initramfs에 넣으면 `modules.list`에 함께 넣는다.
    6. 중국판 펌웨어의 패널 모듈이 같은지 본다. 사용자 기기에서 vendor_boot를 뜨면 글로벌판 모듈과 표를 맞댄다.
- 3: 볼륨 내림(SoC 키패드)이 업스트림 `mt6779-keypad`로 되는지 본다.

### 9.1 2d Wi-Fi

아래는 코드와 자료로 짠 차례다. 기기에서 돌린 적이 없다. 2a(부팅, 콘솔), 2b(SPMI·PMIC·핀·USB 가젯)가 된 뒤에 한다.
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
| `unified command probe: N` | **가장 중요한 답이다.** 0이나 다른 값이면 uni cmd를 받는다. `does not answer`면 레거시 길이 필요하다(아래 7) |

uni 시험을 지난 뒤 답이 없을 때의 줄이다. 명령마다 답을 끊어 드라이버를 레지스터 모델 위에서 돌려 본 결과이고, 모델은 펌웨어가 어떻게 답하는지 말해 주지 않는다.

| 줄 | 명령 | 뜻과 다음 |
|---|---|---|
| `Message 0004008a (seq N) timeout` 뒤 `hardware init failed` | CE `GET_NIC_CAPAB` 0x8a | 벤더 표에 있다(`GET_NIC_CAPABILITY_V2`). uni 시험을 지난 뒤라면 이벤트 꼴을 본다 |
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
| up | uni `DEV_INFO`, `BSS_INFO` | 벤더는 쓰지 않는다. 5단계의 시험이 이 길을 가른다 | `Message 00020001` 또는 `00020002`, `ip`가 실패한다 |
| up | CE `SET_EDCA_PARMS` 0x1d, `SET_RX_FILTER` 0x0a | 있음 | 기다리지 않는다 |
| up | EXT `CHANNEL_SWITCH` 0x08 | 있음 | `Message 000008ed`. `ip`는 성공하고 mac80211의 `ieee80211_hw_conf_init` 경고가 한 번 나온다 |
| scan | CE `START_HW_SCAN` 0x03 | 있음(`SCAN_REQ_V2`) | 기다리지 않는다. 끝 이벤트(0x0d)가 없으면 스캔이 끝나지 않는다 |
| 연결 | uni `DEV_INFO`, `BSS_INFO`, `STA_REC` | 벤더는 쓰지 않는다 | 모델에서 돌리지 않았다(AP가 없다) |

- `Message … timeout`이 나면 cmd와 함께 적는다. 그 뒤 `mt792x_reset`이 WF 전원을 다시 돌리고 보정을 다시 한다. 한 번의 리셋은 10번까지 시도한다. 켜지기는 하는데 펌웨어가 답하지 않거나 잠시 돌다 멈추면 리셋이 다시 걸려 되풀이된다. 앞 재시작에서 10분이 지나지 않은 재시작이 **20번 이어지면** `no more WF restarts: 20, under 10 minutes apart`로 멈춘다. 10분 넘게 잘 돈 뒤의 재시작은 새로 센다. WF는 꺼진 채 남고, 명령은 곧바로 거절된다(`-EIO`).
- 멈추려면 `rmmod mt7921soc`. 지금 도는 시도만 기다리고 남은 시도는 건너뛴다. 시도는 보정과 시간 초과로 13초쯤까지이고, `REG_READ`(0xc0)에 답이 없는 시도는 1분이 넘는다(모델에서 71초). 다시 켜 보려면 `echo 1 > /sys/kernel/debug/ieee80211/phy*/mt76/chip_reset`이나 `modprobe`다. 멈춘 뒤의 `chip_reset`은 마지막 재시작에서 10분이 지나야 돈다. 그 전의 `chip_reset`은 거절되고, 이 시간을 늘리지 않는다.
- 다시 켜기가 실패하면 `WF restart failed`가 나오고, 그 뒤로 드라이버는 WF 쪽과 WFDMA를 건드리지 않는다(`… while the WF part is off`가 찍힌다). `rmmod`로 내려도 된다(링 레지스터는 그동안 메모리를 가리킨다). 다시 올려도 안 되면 재부팅으로 되돌린다.
- 이 상태에서 `ip link set wlan0 up`은 `Input/output error`로 실패한다. `ip link set wlan0 down`과 `rmmod`는 된다.
- `irq N: nobody cared`와 `Disabling IRQ #N`이 나오면 이 선을 WFDMA 말고 다른 것이 올리고 있다. 벤더는 이 선을 인터럽트 컨트롤러에서 막으므로 벤더 코드로는 없다고 말할 수 없다. 커널이 선을 끄고 100 ms마다 폴링하므로 명령은 느리게라도 돌 수 있고, 모듈을 다시 올려야 풀린다. `HOST_CSR_IRQ_EN`(`0x18001600`)의 `0x3ff`를 줄여 볼 자리다. `/proc/interrupts`도 본다.
- `DMA window full` 줄은 20 MiB 칸이 찼다는 뜻이다. 소켓이 안 읽고 쥔 RX 페이지가 칸을 차지한다. 그 동안 RX가 서고 명령과 TX가 `-ENOMEM`으로 실패할 수 있다. 드라이버는 100 ms마다 다시 채운다. 소켓이 읽으면 되살아나야 한다. `/sys/kernel/debug/ieee80211/phy*/mt76/rx-queues`로 본다.
- debugfs `runtime-pm`은 켜도 소유권을 내주지 않는다(하드웨어에 닿지 않는다). `deep-sleep`은 쓰지 않는다. 펌웨어의 `KeepFullPwr`를 내리는 명령이고 이 길은 시험한 적이 없다.
- CNM: 기본은 꺼 둔 길이다. 스캔과 연결이 안 되면 `modprobe mt7921soc cnm=1`로 ROC 길을 본다. 펌웨어 파일이 없어 미리 읽을 수 없다.

#### 끝날 때

- 끄기: `rmmod mt7921soc`. `WF:` 끄기 폴링 실패 줄이 없는가, 레일이 꺼졌는가(`regulator_summary`), 다시 올렸을 때 같은 줄이 나오는가(`SIP init_atf_data`는 빼고). `mtk_connsys`는 내려가지 않는다. 시스템 절전은 시험하지 않는다(wowlan을 내놓지 않는다).
- 2b의 USB 가젯 길: 이 드라이버는 USB를 건드리지 않는다. 레일과 전원 도메인은 connsys 것만 쓴다. 끊기면 보고한다.
- 소비 전력: 레일을 켠 채 두므로 대기 전력이 순정보다 크다. 2e에서 잰다.

### 9.2 2d Bluetooth

아래는 코드와 자료로 짠 차례다. 기기에서 돌린 적이 없다. 2a(부팅, 콘솔), 2b(SPMI·PMIC·핀·USB 가젯)가 된 뒤에 한다.
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
| `bluetoothctl power off`(Wi-Fi 켠 채) | `WMT function off`, `BGF: power off`, `radio 0 power off, radios 0x8`. Wi-Fi가 끊기지 않는다. 레일이 켜져 있다 |
| `rmmod btmtkbeif`, 다시 `modprobe` | 같은 줄이 다시 나온다 |
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
- 서스펜드: 들어갈 때 링이 비어 있으면 MCU를 펌웨어에 돌려준다(`BGF: firmware own`), 보낼 것이 생기면 찾아온다(`BGF: driver own`). 깨우기 원천이 아니라 잠든 동안 온 데이터는 깨어나서 받는다. 2e에서 본다.
- 되풀이 실패: 켜자마자 죽으면 스택이 닫고 여는 것을 세 번 하고 `it keeps failing, not starting it again`을 남긴다. 60초 넘게 살아 있었으면 다시 센다.

### 9.3 2e 충전과 TCPC

기본 상태에서는 새 노드가 모두 `disabled`이고 `dr_mode = "peripheral"`이다. 아래는 한 단계씩 고치는 줄이다. 앞 단계를 기기에서 확인한 뒤 다음 단계로 간다.

단계 A와 B는 충전, C와 D는 TCPC다. 8절(VBUS)은 단계 D로 합쳐진다. D에서는 `usb_vbus_output`을 켜지 않는다.

코드와 자료로 짠 차례이고 기기에서 돌린 적이 없다. UFS 루트와 USB 가젯이 기기에서 된 뒤에 한다. 모든 시험은 **시간이 지나면 저절로 되돌아가는 배경 스크립트**로 하고, 결과는 UFS에 남긴다. 멈추면 전원 키 길게. 먼저 Android에서 100%까지 충전하고 `dumpsys battery`로 전압과 온도를 적어 둔다. 게이지 값과 견줄 기준이다.

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
6. [ ] **처음으로 2 A가 되는 단계다. B의 조건을 다시 갖춘다:** 전력계를 끼우고, 사람이 곁에 있고, 단단한 바닥에 두고, 뒷면 온도를 손으로 본다. USB 커넥터 둘레의 온도도 본다. 이 드라이버에는 커넥터 과열 보호가 없다(순정은 `usb_board_ntc`가 75도를 넘으면 GPIO179로 VBUS를 끊는다). 뜨거워지면 바로 멈추고 알린다. `/sys/class/typec/port0`, `typec-power-opmode`, 충전기의 입력 값. 1.5A·3A 충전기로 입력이 오르는지, 뽑으면 바로 0.5 A로 돌아오는지 전력계로 본다.
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

### 9.4 2e GPU·열·게이지

패치 0070~0079다. 코드, 벤더 자료, 모의 시험으로 확인했고 기기에서 돌리지 않았다. 아래는 기기에서만 알 수 있는 것이다. 각 단계는 앞 단계가 맞을 때만 넘어간다. 노드를 켜는 차례 자체가 안전장치다. 첫 판단 거리는 하드웨어 열 리셋이 켜져 있는지다(1번). 게이지(6번)는 9.3절 B보다 먼저다.

위험: 부팅 불가가 아니라 그 자리 멈춤이다. 커뮤니티 포트는 GPU 첫 전원 근처에서 SoC가 멈췄다는 보고가 있다(포럼 보고). 되돌리는 길은 전원 키 길게와 그 앞 단계의 dtb다.

#### 0. 켜기 전, 읽기만

언락 전 안드로이드에서 구역 온도를 적어 둔다(읽을 수 있으면).

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

## 10. 아직 비어 있는 것

자료가 없어 비워 둔 것이다. 정해지면 이 문서에 넣는다.

1. **백업에 쓸 펌웨어 묶음.** "그 기기 판의 묶음"을 쓰라는 보고뿐이다. 우리가 본 묶음은 중국판 16.0.20.091과 글로벌판 17.0.04.266이고, ZUXOS 1.5.10.044의 묶음을 구한 기록은 없다.
2. **판을 내린 뒤의 OTA와 언락의 데이터.** 판을 내리면 OTA를 다시 꺼야 하는지, 언락이 userdata를 지우는지 자료가 없다.
3. **명령 꼴.** vbmeta 셋의 파티션 이름과 명령, 비운 슬롯에 넷을 굽는 명령, GPT를 뜨고 고치는 도구를 정하지 않았다. 그래서 글로만 적었다. 파티션 이름은 3절 3번에서 본다.
4. **멈췄을 때.** MT6363 드라이버가 뜨기 전(2a, 2b 초반)에 기기를 강제로 끄는 법과, 우리 슬롯에서 LK fastboot로 들어가는 키를 모른다. [커뮤니티 포트 분석](community-port.md)에는 그 포트의 주석 "멈추면 키를 눌러 fastboot로 되살린다"만 있고 어느 키인지는 없다. 굽기 전에 안드로이드 상태에서 찾아 적는다.
5. **입력 통로.** UART를 찾지 못하면 2a와 2b 초반에 셸에 입력할 길이 없다. 5절과 6절 머리의 차례(화면 출력으로 보고, 가젯 뒤에 읽는다)는 이 문서의 해석이다. `/sys/firmware/fdt`를 기기 밖으로 옮기는 일도 그래서 가젯 뒤가 된다.
