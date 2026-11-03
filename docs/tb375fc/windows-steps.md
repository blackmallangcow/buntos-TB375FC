# TB375FC Windows 작업 안내: 백업과 판 내리기

- 작성일: 2026-11-02
- 대상 기기: 중국판 TB375FC. 이 안내를 만든 기기는 ZUXOS 1.5.10.044, 8GB/128GB, 잠김, 슬롯 `_b`였다
- 상태: 3·4·6·7절을 2026-11-03에 했고, 해시는 맥에서 맞댔다. **되돌리기(9절)는 해 보지 않았다.** 절차는 포럼 보고이고, 파티션과 파일 이름은 펌웨어 묶음과 기기에서 읽었다. 화면이 이 문서와 다르면 멈추고 묻는다
- 관련 문서: [체크리스트](bringup-checklist.md) 2.3절과 3절 1번, [실측 보고서](../hw/lenovo-tb375fc.md) 7절

Windows가 필요한 일은 둘이다. 전체 백업과 중국판 16.0.20.091로 판 내리기다. 백업을 맞대 볼 다른 컴퓨터(이 안내는 맥을 썼다)를 함께 가져가면 한 번에 끝난다. 시간은 3시간을 잡는다(포럼 보고: 백업 25~30분, 굽기 15~25분).

## 하지 말 것

| 하지 말 것 | 까닭 |
|---|---|
| 묶음의 `运行我，刷机.cmd` 실행 | 레노버의 스크립트인데 `Firmware Upgrade`를 곧바로 돌린다(읽었다). 그 안에 preloader 굽기가 들어 있다. SSD에서는 이름을 바꿔 `fw\_do_not_use`로 옮겨 두었다 |
| `Format All + Download`, `Format` 탭 | 벽돌이 됐다는 보고가 있다. 이 기기만의 값이 든 파티션까지 지울 수 있다 |
| `Firmware Upgrade` | preloader를 굽고 파티션 표를 바꿀 수 있다. 성공 보고도 있지만 1.5.10.044의 preloader를 옛 판으로 내려도 되는지 모른다. 이번 길에는 쓰지 않는다. 오류가 나도 혼자 이쪽으로 넘어가지 않는다 |
| `preloader_a`, `preloader_b` 체크 | 구우면 벽돌이 된다는 보고가 있다 |
| 이름에 `efuse`가 든 것을 고르기 | `_efuse` 파티션 표만 `efuse.img`를 굽는다(묶음에서 읽었다). efuse는 한 번 쓰면 되돌릴 수 없다. SSD에서는 `image`의 파일을 `fw\_do_not_use`로 옮겨 두었고, `tool\write_efuse.xml`, `read_efuse.xml`은 도구 폴더에 그대로 있다 |
| 작업 중 케이블을 뽑거나 PC 끄기 | 백업 중 뽑았더니 기기가 몇 시간 먹통이었다는 보고가 있다 |
| 백업 공유, 클라우드 업로드 | 일련번호, MAC, 보정값, DRM 키가 들어 있다 |

## 1. 준비물

- [ ] **Windows 10·11 64비트 PC.** 드라이버를 설치할 수 있어야 한다. 허브를 거치지 않고 PC 포트에 바로 꽂는다. Apple Silicon 맥의 ARM용 Windows 가상 머신은 쓰지 못한다. 드라이버가 x86·x64뿐이다.
- [x] **외장 SSD.** Windows와 맥이 함께 읽는 exFAT 파티션(여기서는 이름이 `TB375FC`인 64GB)에 둔다. 도구와 묶음 7GiB에 백업 11GiB가 더해져도 남는다. 백업에는 기기만의 값이 들어 있으니 잃어버리지 않게 챙긴다.
- [ ] **케이블.** 태블릿용 데이터 케이블과 SSD용 케이블. PC 포트 모양(USB-A, USB-C)에 맞춘다.
- [ ] **맥(또는 fastboot가 되는 다른 컴퓨터)과 충전기.** 백업을 맞대 보고 슬롯을 바꾸는 데 쓴다.
- [ ] **태블릿 충전.** 100%로 간다.

## 2. 집에서 (맥)

1. [ ] 개인 자료를 옮긴다. 이 백업은 `userdata`를 빼고, 판을 내리면 초기화해야 한다.
2. [ ] 태블릿의 Google 계정을 지운다. 초기화 뒤 계정 잠금(FRP)에 걸리지 않게 하려는 것이다(포럼 보고). 레노버 계정과 화면 잠금도 지워 두면 안전하다(판단).
3. [x] 묶음과 드라이버를 SSD의 `TB375FC` 파티션에 넣는다. 구성은 다음과 같다.
    - `fw\TB375FC_ZUI_16.0.20.091\`: 묶음을 푼 것이다. 파일 96개의 이름과 크기를 묶음 목록과 맞댔다.
    - `fw\_do_not_use\`: 실수로 실행하거나 고르기 쉬운 파일 다섯을 옮겨 두었다(`.cmd`는 `.txt`를 붙였다). 도구가 쓰는 `flash.xml`과 `MT6897_Android_scatter.xml`은 이 파일들을 가리키지 않는다.
    - `driver\`: Microsoft 카탈로그의 드라이버다(`cdc-acm.inf` 3.0.1504.0, Microsoft 서명). preloader, DA, BROM의 USB ID가 들어 있다.
    - `windows-steps.txt`: 이 문서다.
4. [x] 받은 묶음의 원본은 맥 쪽 폴더(아래 `<묶음 폴더>`)에 둔다. 묶음은 5,409,124,544바이트이고 SHA-256은 `ab7c25e4e0c9149b7ea7b7fb9264dc45610bb0834541b4683fff8887ad8e6ca7`이다. 포럼에 올라온 값과 같다. 다시 풀어야 하면 맥에서 다음을 돌린다.

    ```bash
    bsdtar -xf "<묶음 폴더>"/TB375FC_CN_OPEN_USER_M1.491_U_ZUI_16.0.20.091_ST_250117.7z -C /Volumes/TB375FC/fw && dot_clean -m /Volumes/TB375FC
    ```

5. [ ] (선택) 8절의 키를 눌러 맞는지 본다. 태블릿을 끄고 볼륨 올림을 누른 채 맥에 꽂으면서 Claude에게 말하면, preloader 연결(USB ID `0E8D:2000`)이 보이는지 맥에서 함께 본다. Windows에 가기 전에 키와 케이블을 시험하는 일이다.

## 3. Windows: 연결 시험

여기까지는 기기에 아무 변화가 없다. 안 되면 그대로 돌아오면 된다. 아래에서 SSD의 `TB375FC` 드라이브는 `E:`로 적었으니 PC에서 잡힌 문자로 바꾼다.

1. [x] SSD를 꽂아 `TB375FC`가 탐색기에 보이는지 본다. **포맷하겠냐는 창이 뜨면 반드시 취소한다.** 같은 SSD에 다른 파티션이 있으면 그 자료까지 지워질 수 있다.
2. [x] 관리자 권한 PowerShell에서 드라이버를 넣는다.

    ```powershell
    pnputil /add-driver E:\driver\*.inf /subdirs /install
    ```

3. [x] 장치 관리자의 "포트(COM & LPT)"를 펼쳐 둔다. USB를 뽑은 채 태블릿을 끈다. 꽂혀 있으면 배터리 화면이 나오니 뽑는다(실측). 볼륨 올림을 누른 채 USB를 꽂는다. 화면은 꺼진 채다.
    - `MediaTek PreLoader USB VCOM (Android)`로 뜨면 맞다. 몇 초 뒤 사라지고 태블릿이 켜질 수 있다.
    - `USB Serial Device`로 뜨면 드라이버가 안 먹은 것이다. 이 상태의 백업은 37KB/s였다는 보고가 있다(맞는 드라이버는 7MB/s). 그 보고의 드라이버는 3.0.1512.0이고 SSD의 것은 3.0.1504.0이다. 3.0.1504.0으로 약 30MB/s가 나왔다(실측, 2026-11-03).
    - 장치 속성의 하드웨어 ID로도 볼 수 있다. preloader는 `VID_0E8D&PID_2000`, 굽는 동안은 `PID_2001`, BROM은 `PID_0003`이다(묶음의 `usb_settings.xml`).
    - 시험이 끝나면 태블릿이 켜졌는지 보고, 켜졌으면 다시 끈다.

## 4. Windows: 전체 백업

프로그램은 SSD에서 바로 돌린다. 그래야 백업이 PC에 남지 않는다.

1. [x] USB를 뽑고 태블릿을 끈다.
2. [x] `E:\fw\TB375FC_ZUI_16.0.20.091\tool\SPFlashToolV6.exe`를 연다. SmartScreen 경고가 뜨면 "추가 정보", "실행"을 고른다. 도구가 자기 업데이트를 물으면 거절한다.
3. [x] `Download-XML`에 `image\flash.xml`, `Authentication File`에 `image\da.auth`를 고른다. 레노버 스크립트가 쓰는 파일이다. `image\download_agent\flash.xml`도 같은 파티션 표를 가리킨다(묶음에서 읽었다).
4. [x] `Readback` 탭 → `Auto` → `Read PT`를 누른 뒤, 볼륨 올림을 누른 채 USB를 꽂는다. 파티션 목록이 채워진다.
5. [x] `userdata`만 빼고 모두 체크한다. 저장 위치가 `E:`인지 본다.
6. [x] `Read Back`을 누른다. 약 11GiB이고 약 6분 걸렸다(실측, 2026-11-03. 25~30분이라는 보고도 있다). 속도가 KB/s면 멈추고 3절로 돌아간다.
7. [x] 끝나면 PowerShell로 해시를 적는다. 폴더 경로는 화면에서 확인한다(아래는 도구 폴더의 `Readback`으로 가정).

    ```powershell
    cd E:\fw\TB375FC_ZUI_16.0.20.091\tool\Readback
    Get-ChildItem -File | Get-FileHash -Algorithm SHA256 | ForEach-Object { "$($_.Hash.ToLower())  $(Split-Path $_.Path -Leaf)" } | Out-File -Encoding ascii E:\readback.sha256
    ```

8. [ ] USB를 뽑고 전원 버튼을 길게 눌러 태블릿을 켠다. 안드로이드가 떠야 한다. 켜지지 않으면 전원 + 볼륨 올림을 누른다. 이 키는 fastboot로 들어가니(실측), 그때는 전원 + 볼륨 내림으로 나온다. SSD를 안전하게 뺀다.

preloader와 파티션 표(pgpt)는 이 목록에 나오지 않는다는 보고가 있다. 이번 길에서 쓰지 않으므로 그대로 둔다. 언락 뒤 root로 뜬다(체크리스트 4절).

## 5. 맥: 백업 확인과 결정

여기가 멈출 수 있는 마지막 지점이다.

1. [x] SSD를 맥에 꽂고 해시를 맞댄다. 아무것도 나오지 않고 `확인 끝`만 나오면 맞다.

    ```bash
    cd /Volumes/TB375FC/fw/TB375FC_ZUI_16.0.20.091/tool/Readback && tr -d '\r' < /Volumes/TB375FC/readback.sha256 | shasum -a 256 -c - | grep -v ': OK$'; echo 확인 끝
    ```

2. [x] 백업 파일의 이름과 크기를 파티션 표와 맞대 빠진 것을 찾는다. 내용은 읽지 않는다. AVB 머리글의 rollback index만 더 읽었고, 기기와 묶음이 모두 0이었다(2026-11-03).
    사본을 다른 디스크에도 두고 해시를 맞대 둔다.
3. [x] 체크리스트 2.4절의 넷으로 go/no-go를 정한다. 예비 기기를 둘지도 이때 정한다.

## 6. Windows: 판 내리기

1. [x] USB를 뽑고 태블릿을 끈다. 같은 프로그램에서 `Download-XML`과 `Authentication File`이 그대로인지 본다.
2. [x] `Download` 탭의 목록에서 `Download Only`를 고르고 `Auto Reboot`를 체크한다.
3. [x] 표에서 `preloader_a`, `preloader_b`, `userdata`의 체크를 뺀다. 차례가 섞여 있으니 끝까지 내려 본다.
    - 체크가 29줄 남아야 한다. `_a`로 끝나는 것과 `lenovocust`, `lenovoraw`, `super`다.
    - `_b` 줄과 `efuse`, `nvram`, `nvdata`, `proinfo`에는 체크가 없어야 한다(묶음의 파티션 표에서 읽었다). 다르면 멈춘다.
    - `Download`를 누르기 전에 화면 사진을 Claude에게 보내면 함께 볼 수 있다.
4. [x] `Download`를 누른 뒤, 볼륨 올림을 누른 채 USB를 꽂는다. 15~25분 걸린다는 보고가 있다. 초록 표시가 나올 때까지 기다린다.
5. [ ] USB를 뺀다. 도구의 기록은 PC의 `C:\ProgramData\SP_FT_V6_Logs`에 쌓이니(묶음의 `option.ini`) 빌린 PC면 이 폴더를 지운다.

## 7. 맥: 슬롯 바꾸기와 첫 부팅

**로고에서 멈추거나 부팅이 반복되는 것은 예상한 일이다.** 묶음은 `_a`에만 굽는데(파티션 표), 기기는 아직 `_b`를 고른다. 같은 증상을 슬롯을 바꿔 푼 보고가 여럿이다.

1. [x] 전원 + 볼륨 올림으로 fastboot에 들어간다(안드로이드가 켜진 상태에서 실측). 안 되면 전원 + 볼륨 내림으로 끄고, 켜면서 로고에서 볼륨 올림을 누른 뒤 볼륨 내림으로 fastboot를 고른다(포럼 보고). 맥에 꽂는다.
2. [x] 슬롯을 바꾼다. 잠긴 채로 됐다(실측, 2026-11-03). 거부하면 멈춘다. 같은 LK 세션에서는 `current-slot`이 `b`로 남으니(실측), `fastboot reboot bootloader`로 다시 들어와 `a`인지 본 뒤 재부팅한다.

    ```bash
    fastboot getvar current-slot
    ```

    ```bash
    fastboot --set-active=a
    ```

    ```bash
    fastboot reboot
    ```

3. [x] "data may be corrupt"가 나오면 Factory reset을 고른다(실측: `Can't load Android system` 화면의 `Factory data reset`). 안 나오고 계속 멈추면 8절의 복구 화면에서 wipe data를 고른다.
4. [x] 설정 마법사에서 Wi-Fi는 건너뛸 수 있으면 건너뛴다. 새 판을 받지 않게 하려는 것이다.
5. [x] 개발자 옵션을 켜고 USB 디버깅과 **OEM 잠금 해제를 다시 켠다.** 이 값을 켜지 않아 언락이 안 됐던 보고가 있다.
6. [x] OTA를 끄고 판을 본다. 초기화하면 이 설정도 사라진다는 보고가 있다.

    ```bash
    adb shell pm disable-user --user 0 com.lenovo.ota
    ```

    ```bash
    adb shell getprop ro.build.display.id
    ```

    ```bash
    adb shell getprop ro.boot.slot_suffix
    ```

7. [x] 여기서 멈추고 Claude를 부른다. 언락은 맥에서 함께 한다(체크리스트 3절 2번부터). 이제 Windows는 되돌릴 때만 필요하다.

## 8. 키 조합

| 목적 | 키 | 근거 |
|---|---|---|
| fastboot 들어가기 | 전원 + 볼륨 올림. 또는 `adb reboot bootloader` | 실측(2026-11-02, 사용자가 눌러 맥에서 확인) |
| 리눅스에서 빠져나오기 | 전원 + 볼륨 올림은 fastboot로 들어간다. 전원 + 볼륨 내림은 반응이 없었다 | 실측(2026-11-03, 시험 커널) |
| fastboot 나오기 | `fastboot reboot`는 안드로이드로 간다. 전원 + 볼륨 내림은 꺼지고, USB가 꽂혀 있으면 배터리 화면이 나온다 | 실측 |
| 강제로 끄기 | 전원 + 볼륨 내림 | fastboot에서는 실측. 다른 상태에서는 포럼 보고 |
| 복구 화면 | 전원 + 볼륨 올림 + 볼륨 내림, 로고가 나오면 전원만 놓기 | 포럼 보고 |
| 도구에 연결(preloader) | 끈 상태에서 볼륨 올림을 누른 채 USB 꽂기. 레노버 안내(묶음의 `notice.txt`)는 끈 채 꽂기만 하라고 적었다 | 포럼 보고 |
| 최후의 복구(BROM) | 끈 상태에서 볼륨 둘을 누른 채 USB 꽂기 | 포럼 보고 |

## 9. 막히면

**오류가 나면 다른 모드로 바꿔 가며 시도하지 않는다.** 화면 사진을 찍어 Claude에게 보낸다.

| 증상 | 할 일 |
|---|---|
| 드라이버 설치가 막힘, SSD가 안 보임 | 그날은 돌아온다. 기기는 그대로다. Windows 노트북을 빌리는 쪽을 본다 |
| `Read PT`를 눌러도 목록이 안 뜸 | 같은 증상의 글(2025-09)이 있고 답은 없었다. `Read PT`를 누르고 바로 키와 USB를 다시 해 본다. 안 되면 돌아온다 |
| 인증 실패(DA, auth) | 멈춘다. 지금까지 기기에 쓴 것은 없다 |
| 백업 중 끊겨 먹통 | 기다린다. 몇 시간 뒤 살아났다는 보고가 있다 |
| `GPT layout change` | 멈춘다. `Firmware Upgrade`나 `Format All`로 넘어가지 않는다. `Read PT` 목록과 묶음의 표는 시작 주소와 크기까지 같았으니(실측, 2026-11-03. `userdata`, `mrdump`, `flashinfo`의 위치만 다르다) 나오면 뜻밖의 일이다 |
| 굽다가 실패, 화면이 검음 | preloader 모드는 남는다는 보고가 있다. 8절로 들어가는지 보고 Claude를 부른다 |
| Red State | 멈추고 Claude를 부른다 |

**되돌리는 길.** `_b`의 부트 계열은 판 내리기가 건드리지 않는다. 바뀌는 것은 `_a`, `super`, `lenovocust`, `lenovoraw`이고 초기화하면 `userdata`와 `metadata`도 바뀐다. 그래서 백업의 그 파일을 되돌리고 `_b`로 바꾸면 돌아갈 수 있다고 본다(판단, 해 보지 않았다). **되돌릴 때 부트 계열은 `_b` 파일이 기준이다.** 포럼 글은 `_a` 파일을 쓰라고 적었지만, 이 기기의 `_a`는 8월 OTA 전의 판이다. 되돌릴 폴더는 Claude가 맥에서 만든다.

## 출처

포럼 글은 틀릴 수 있다. 블로그와 제3자 파일(scatter, 설정 묶음)은 쓰지 않았다. 묶음의 파일 내용은 직접 읽었다(`.handoff/tb375fc-device-readings/bundle-091/`, git 밖). 묶음의 파티션 표가 `userdata`를 크기 조정 항목(`NEEDRESIZE`)으로 두고, 나머지 크기는 기기와 같아서 128GB용 scatter가 따로 필요해 보이지 않는다(판단).

- 절차의 뼈대(하드웨어 같은 8GB/128GB 기기, 2026-10, 작성자가 AI 요약이라고 밝혔다): https://xdaforums.com/t/flashing-cn-global-rom-for-tb375fc-tb373fu-moto-pad-60-pro.4720245/post-90767730
- 원글(`Download Only`, 체크 해제, `userdata` 빼기, 슬롯): https://xdaforums.com/t/flashing-cn-global-rom-for-tb375fc-tb373fu-moto-pad-60-pro.4720245/
- 슬롯을 바꿔 푼 보고와 케이블 경고: https://xdaforums.com/t/flashing-cn-global-rom-for-tb375fc-tb373fu-moto-pad-60-pro.4720245/post-90279781
- preloader와 BROM 진입: https://xdaforums.com/t/flashing-cn-global-rom-for-tb375fc-tb373fu-moto-pad-60-pro.4720245/post-90382547
- `GPT layout change` 사례: https://xdaforums.com/t/flashing-cn-global-rom-for-tb375fc-tb373fu-moto-pad-60-pro.4720245/post-90001370
- OEM 잠금 해제를 빠뜨린 사례와 묶음 해시: https://xdaforums.com/t/unofficial-unlocking-lenovo-xiaoxin-pad-pro-2025-tb375fc.4717695/post-90352927
- ZUXOS 1.5.10.103에서 16.0.20.091로 내린 뒤 언락한 사례: https://xdaforums.com/t/unofficial-unlocking-lenovo-xiaoxin-pad-pro-2025-tb375fc.4717695/post-90740025
- 벽돌 복구(`Format All + Download`, BROM): https://xdaforums.com/t/tb375fc-lenovo-xiaoxin-pad-pro-12-7-how-to-fix-hard-brick-soft-brick-bootloop-black-screen-faulty-rom.4767187/
- 드라이버(공식): https://www.catalog.update.microsoft.com/Search.aspx?q=MediaTek%20PreLoader%20USB%20VCOM
