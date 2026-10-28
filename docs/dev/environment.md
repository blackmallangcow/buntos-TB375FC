# 개발 환경

buntOS 이미지는 Linux에서 mkosi로 빌드한다. Apple Silicon Mac에서는 Lima로 Debian 13 VM을 띄워 빌드 호스트로 쓰고, aarch64 이미지는 그 VM 안에서 중첩 가상화(KVM)로 띄운다(ADR-010).

## 요구 사항

- Apple Silicon M3 이상, macOS 15 이상 (VM 안에서 KVM을 쓰기 위한 중첩 가상화 조건)
- Homebrew
- 디스크 여유 공간: VM 디스크는 최대 60GiB이며 실제로 쓴 만큼만 커진다. 빌드 도구와 두 아키텍처 이미지까지 Mac 쪽 VM 디스크 파일이 약 8GB가 됐다

## 1. VM 만들기 (Mac)

```bash
brew install lima
```

저장소 루트에서:

```bash
limactl create --name=buntos-dev --tty=false --mount-only "$(pwd):w" tools/devvm/buntos-dev.yaml
```

```bash
limactl start buntos-dev
```

VM에는 저장소 폴더만 쓰기 가능하게 연결되고, VM 안에서도 경로가 같다. 홈 디렉터리의 다른 폴더는 연결하지 않는다. `limactl shell`은 Mac에서 명령을 실행한 폴더와 같은 경로에서 시작한다.

## 2. 빌드 도구 설치 (VM)

저장소 루트에서:

```bash
limactl shell buntos-dev bash tools/devvm/setup.sh
```

mkosi(버전은 스크립트에 고정), QEMU, UEFI 펌웨어, TPM 에뮬레이터, x86-64 교차 빌드용 binfmt, 이미지 도구를 설치한다. 여러 번 실행해도 된다. kvm 그룹 추가는 새 세션부터 적용되므로 처음 한 번은 VM을 다시 시작한다:

```bash
limactl stop buntos-dev
```

```bash
limactl start buntos-dev
```

## 3. 확인 (VM)

```bash
limactl shell buntos-dev
```

VM 안에서 다음을 확인한다.

| 확인 | 명령 | 기대 결과 |
|---|---|---|
| 중첩 가상화 | `sudo dmesg \| grep "started at EL"` | `EL2` |
| KVM 장치 | `ls -l /dev/kvm` | 장치가 있다 |
| 권한 | `id -nG` | `kvm` 포함 |
| mkosi | `mkosi --version` | 스크립트에 고정한 버전 |

## 4. 이미지 빌드와 e2e

레시피는 `image/`(공통)와 `bsp/<보드>/`(아키텍처별 차이)로 나뉜다. 보드 이름은 `qemu-aarch64`, `x86_64-generic`이다. 산출물은 VM 쪽 디스크의 `~/buntos-build/<보드>/`에 생긴다. 저장소 루트에서:

```bash
limactl shell buntos-dev tools/image/build.sh qemu-aarch64
```

e2e 1번(부팅 → 로그인 → 셸)을 돌린다:

```bash
limactl shell buntos-dev tests/e2e/boot-to-shell.sh qemu-aarch64
```

빌드한 이미지를 직접 띄워 본다. 안에서 바꾼 내용이 이미지에 남지 않게 임시 복사본으로 띄운다. 이미지 전체를 복사하므로 메모리가 빠듯하면 부팅이 1분 넘게 늦어질 수 있다. QEMU를 끝내려면 `Ctrl-A` 다음 `X`를 누른다:

```bash
limactl shell buntos-dev tools/image/build.sh qemu-aarch64 --ephemeral=yes vm
```

화면을 보며 기능을 확인할 때는 `tools/image/vmctl.sh`로 VM을 띄워 두고 다른 명령에서 조작한다. VM 셸(`limactl shell buntos-dev`)에서:

- 빌드한 이미지 위에 임시 덮어쓰기(qcow2)를 씌워 QEMU를 직접 띄운다. 화면, 가상 키보드·태블릿, 가상 사운드 장치가 붙는다.
- 직렬 콘솔은 root로 자동 로그인한다.
- `VMCTL_ID`로 이름을 주면 여러 대를 따로 띄운다. 빌드 VM 메모리 때문에 한 번에 두 대까지만 뜨고, 나머지는 자리가 날 때까지 기다린다.

```bash
VMCTL_ID=mine tools/image/vmctl.sh start &      # VM이 끝날 때까지 돌아오지 않는다
VMCTL_ID=mine tools/image/vmctl.sh wait-login
VMCTL_ID=mine tools/image/vmctl.sh sh 'systemctl --failed'
VMCTL_ID=mine tools/image/vmctl.sh qmp click:1200,14 sleep:1 screen:/tmp/s.png
VMCTL_ID=mine tools/image/vmctl.sh stop
```

**동작 목록.** `qmp` 동작은 `tools/image/qmp.py` 머리말에 있다: 누르기, 끌기, 키, 글자 입력, 화면 저장.

**여럿이 함께 쓸 때.**
- 같은 이미지는 한 번에 하나만 빌드한다(`<이미지 디렉터리>.lock`). vmctl은 빌드가 끝나기를 기다렸다 띄운다.
- 띄운 뒤에는 빌드가 이미지를 새로 만들어도 VM은 이미 연 파일을 계속 쓴다.
- 다른 사람이 기본 이미지로 VM을 띄워 둔 동안 자기 변경을 따로 시험하려면 `BUNTOS_IMAGE_DIR`로 다른 디렉터리에 빌드하고 같은 변수로 띄운다:

  ```bash
  BUNTOS_IMAGE_DIR=~/buntos-build/mine tools/image/build.sh qemu-aarch64
  BUNTOS_IMAGE_DIR=~/buntos-build/mine VMCTL_ID=mine tools/image/vmctl.sh start &
  ```

- mkosi `vm`(e2e, `screenshot.sh`)은 도는 동안 이미지를 잠가서, 그동안의 빌드는 2분까지 기다린 뒤 실패한다.

x86-64는 보드 이름만 `x86_64-generic`으로 바꾸면 된다. Apple Silicon에서는 패키지 설치 스크립트를 qemu-user로 에뮬레이션해 빌드하고, 부팅도 KVM 없이 소프트웨어 에뮬레이션(TCG)으로 한다. CI의 x86-64 러너에서는 KVM을 쓴다([설계안](../design/ARCHITECTURE.md) 9.4).

### 그래픽 세션 보기

이미지 빌드는 자체 컴포넌트(bunt-comp, bunt-shell, bunt-settings)를 먼저 컴파일해 이미지에 넣는다. 화면 장치가 있으면 tty1에서 개발용 사용자 `buntos`로 자동 로그인해 그래픽 세션을 연다. 컴포지터가 셸(bunt-shell)과 입력기(fcitx5)를 띄운다.

빌드 VM과 아키텍처가 다른 x86-64 이미지에는 자체 컴포넌트가 아직 들어가지 않는다. 필수 드라이버를 넣은 x86-64 이미지는 빌드와 e2e 1번까지 확인했고(6장), 자체 컴포넌트는 CI에서 빌드한다.

**Dock.**
- 앱: 파일(Nautilus), 모든 앱, 웹 브라우저(Firefox ESR), 터미널(foot), 텍스트 편집기, 계산기, 이미지 보기(Loupe), 문서 보기(Papers), 시스템 모니터, 시스템 설정(bunt-settings)
- 실행 중인 앱은 아이콘 아래에 점이 붙는다. 터미널에서는 nano, vim, htop을 쓸 수 있다.

**메뉴 막대.**
- 배경은 macOS처럼 투명하고 글씨는 흰색이다. 앞에 있는 창을 최대화하거나 전체 화면으로 하면 검은색이 된다.
- 왼쪽: 로고 메뉴, 그 옆에 앞에 있는 앱의 이름과 메뉴
- 오른쪽(macOS 순서): 앱이 넣는 항목, 입력 소스, 사운드, 배터리(전원), 네트워크(Wi-Fi), 제어 센터, 날짜·시간
- 어떤 항목을 보일지는 시스템 설정의 메뉴 막대 페이지에서 정한다([디자인 스펙](../design/DESIGN-SPEC.md) 5.2).

**네트워크와 장치.**
- 네트워크는 NetworkManager가 관리하며, QEMU에서는 유선으로 연결된다.
- 시험 VM(`vmctl.sh`)과 Mac 창(`view-mac.sh`)에는 가상 사운드 장치가 붙어 음량을 시험할 수 있다.
- QEMU에는 배터리, Wi-Fi, Bluetooth 장치가 없다. 그래서 전원 항목은 전원 어댑터로 보이고, Bluetooth는 "사용할 수 없음"이다.

Mac에서 창으로 본다. Mac 터미널의 저장소 루트에서 실행하며, QEMU가 필요하다(`brew install qemu`):

```bash
tools/image/view-mac.sh
```

빌드 VM에서 이미지(약 4GB)를 `~/Library/Caches/buntos/`로 가져온 뒤 HVF 가속으로 띄운다. 이미지는 snapshot 모드로 띄우므로 안에서 바꾼 내용은 창을 닫으면 사라진다. 같은 이미지를 다시 볼 때는 `--no-copy`를 붙이면 가져오는 단계를 건너뛰며, 이때는 빌드 VM이 꺼져 있어도 된다. Mac 메모리가 16GB 이하이면 이미지를 가져온 뒤 빌드 VM을 끄고(`limactl stop buntos-dev`) `--no-copy`로 띄우는 편이 낫다. 빌드 VM이 8GiB, 보기용 QEMU가 최대 4GiB를 쓴다.

화면은 1280×800으로 뜬다. 화면 크기(배율)는 시스템 설정의 디스플레이 페이지에서 고르고, 실제 해상도는 같은 페이지의 "고급"에서 바꾼다. 1280×800에서 고를 수 있는 크기는 100%와 125%(1024 × 640처럼 보임)다. 더 큰 배율은 해상도를 올리면 나온다(1920×1200은 175%까지). bunt-comp는 화면이 알려 주는 선호 해상도가 바뀌면 따라간다(사용자가 해상도를 고정하지 않았을 때).

| 조작 | 방법 |
|---|---|
| 앱 실행 | Dock 아이콘을 누른다. 터미널은 Option+Return으로도 연다 |
| 시스템 설정, 다시 시작, 시스템 종료 | 메뉴바 왼쪽의 로고(b) 메뉴 |
| 앞에 있는 앱의 메뉴 | 로고 옆의 앱 이름과 파일·창·도움말 메뉴. 창 메뉴에 확대/축소와 전체 화면이 있다 |
| 알림 센터 | 메뉴바 오른쪽의 날짜와 시간을 누른다 |
| 제어 센터 | 메뉴바 오른쪽의 스위치 두 개 아이콘. Wi-Fi·Bluetooth, 방해 금지, 다크 모드, 저전력 모드, 음량(밝기는 내장 화면이 있을 때) |
| 전원 모드 | 메뉴바의 배터리(전원) 아이콘. 저전력 모드·자동·고성능을 고르면 바로 적용된다 |
| 입력 소스 | 메뉴바의 "A"/"한"을 눌러 고르거나 Ctrl+Space, 한/영 키. 입력 칸에 포커스가 없어도 바뀐다 |
| 한자 변환 | 한글을 쓰는 중에 한자 키 또는 F9. 숫자나 Enter로 고르고 Esc로 닫는다 |
| 모든 앱 | Dock의 앱 단추를 누르면 화면 가운데에 패널이 뜬다. 분류 칩으로 거르고, 이름을 치면 찾고, Enter로 첫 앱을 실행한다. 바깥을 누르거나 Esc로 닫는다 |
| 배경화면 | 시스템 설정의 배경화면 페이지. "사진 추가…"로 내 사진을 쓴다 |
| 시스템 설정의 페이지 바로 열기 | `bunt-settings wifi`처럼 페이지 이름을 준다. 이미 떠 있으면 페이지만 바뀐다. 이름은 [디자인 스펙](../design/DESIGN-SPEC.md) 9장. 설정 창에서 Ctrl+F는 검색, Alt+←/→는 뒤로·앞으로 |
| 창 옮기기 | 제목줄을 끈다. 제목줄은 메뉴 막대 위로 올라가지 않는다 |
| 창 크기 조절 | 창 가장자리를 끈다. 제목줄을 두 번 누르면 최대화와 복원 |
| 창 닫기 | 제목줄의 X 버튼, 또는 Option+F4 (Mac 노트북 키보드는 fn도 함께) |
| 세션 끝내기 | Ctrl+Option+Delete(⌫). tty1 셸로 돌아온다 |
| 끝내기 | QEMU 창을 닫는다 (종료 확인 창이 뜨면 Quit) |

**Wi-Fi 시험.** QEMU에는 Wi-Fi 장치가 없다. 시험 VM 안에서 가짜 무선 장치를 만들어 시험한다(root 직렬 콘솔에서).
1. 장치를 만든다: `modprobe mac80211_hwsim radios=3`
2. 핫스팟에 필요한 패키지를 넣는다. 이미지에는 넣지 않는다: `apt-get update && apt-get install -y --no-install-recommends dnsmasq-base`
3. wlan0에 WPA2 핫스팟을 띄운다(열린 핫스팟은 wifi-sec 없이 wlan1에):

   ```bash
   nmcli con add type wifi ifname wlan0 con-name lab-ap autoconnect no ssid buntOS-Lab mode ap ipv4.method shared 802-11-wireless.band bg wifi-sec.key-mgmt wpa-psk wifi-sec.psk buntos1234 && nmcli con up lab-ap
   ```

4. wlan2가 메뉴 막대와 시스템 설정의 Wi-Fi 장치가 된다.

시스템 설정은 직렬 콘솔에서 `sudo -u buntos`로 띄우면 그래픽 로그인 세션 밖이라 polkit이 네트워크 변경을 거절한다. Dock에서 띄우고, 페이지만 `bunt-settings <페이지>`로 넘긴다.

빌드 VM 안에서 화면을 캡처할 수도 있다. 결과는 VM의 `~/buntos-build/qemu-aarch64/screen.png`이며, 클릭·키 입력 동작을 이어서 줄 수 있다(`tools/image/qmp.py` 참고):

```bash
limactl shell buntos-dev tools/image/screenshot.sh qemu-aarch64
```

## 5. 일상 사용

| 할 일 | 명령 (Mac) |
|---|---|
| VM 셸 열기 | `limactl shell buntos-dev` |
| 끄기 / 켜기 | `limactl stop buntos-dev` / `limactl start buntos-dev` |
| 지우기 | `limactl delete buntos-dev` |

VM은 메모리 8GiB를 잡으므로 쓰지 않을 때는 꺼 둔다.

## 6. 검증 기록

2026-10-27에 1차 빌드와 그래픽 세션 v0를, 2026-10-28에 기본 앱, 시스템 설정, macOS형 데스크톱(메뉴 막대 항목, 제어 센터, 모든 앱), 한글 입력기, 필수 드라이버, x86-64 이미지를 확인했다.

| 항목 | 결과 |
|---|---|
| 호스트 | Apple M4, 16GB, macOS 27.0.1, Lima 2.2.1 |
| VM | Debian 13, 커널 6.12(cloud-arm64), 6코어·8GiB. 커널이 EL2에서 시작하고 KVM nVHE 모드 초기화, `/dev/kvm` 확인 |
| 빌드 도구 | mkosi 27.1, QEMU 10.0.13 |
| 1차 빌드 arm64 | 81초(패키지 캐시 있음). 디스크 이미지 1.6G(btrfs 루트), Debian forky 스냅샷 `20261027T083111Z`, 커널 7.2.12, systemd 262 |
| e2e 1번 arm64 | 통과(KVM). 로그인 15초, 셸 17초, 시스템 상태 `running`, 전원 꺼짐 19초. 디스크에 임시 덮어쓰기를 씌워 띄우며, 빌드한 이미지의 수정 시각이 바뀌지 않음을 확인 |
| 1차 빌드 x86-64 | 133초(패키지 캐시 있음, 교차 빌드). 처음 받을 때는 330초. 디스크 이미지 1.3G |
| e2e 1번 x86-64 | 통과(TCG). 로그인 32초, 셸 35초, 시스템 상태 `running`, 전원 꺼짐 37초 |
| 그래픽 세션 arm64 | QEMU virtio-gpu + Mesa 소프트웨어 렌더러. 배경·메뉴바(한국어 시계)·Dock·커서 표시, Dock 클릭으로 터미널 실행, 키 입력, Alt+Enter 확인(빌드 VM 안에서 QMP로 조작하고 화면 캡처). 이미지 2.9G, 빌드 65초(캐시 있음) |
| 기본 앱과 시스템 설정 arm64 (10-28) | 이미지 4.0G(사용 3.7G), 빌드 83초(캐시 있음. 새 패키지를 처음 받을 때는 576초). Dock의 앱 8개가 모두 뜨고(GNOME 앱은 한국어 화면), 새 창은 쓸 수 있는 영역 가운데에 놓인다. 메뉴바 로고 메뉴, 창 이동·최대화·복원·크기 조절, 앱의 팝업 메뉴(밖을 누르면 닫힘), 드롭다운 검색 입력을 확인했다. 시스템 설정에서 바꾼 밝게·어둡게(메뉴바, Dock, 설정 창, GNOME 앱), 24시간제, 시간대(메뉴바 시계에 바로 반영), 기기 이름, 언어(영어)가 메뉴의 다시 시작 뒤에도 유지됐다. 빌드 VM 안에서 QMP로 조작하고 화면을 캡처했다 |
| e2e 1번 arm64 (10-28) | 통과(KVM). 로그인 23초, 셸 26초, 시스템 상태 `running`, 전원 꺼짐 27초. 10-27보다 로그인이 8초 늦다. 첫 부팅마다 도는 `ldconfig`(2.9초)와 새로 들어온 `udisks2`(1.0초)가 크다 |
| CI (10-28) | 비공개 저장소 main(`6eb348d`)에 올려 GitHub 호스티드 러너에서 REUSE 검사 통과. 이미지 빌드와 e2e는 아직 CI에 없다 |
| Mac에서 보기 | Mac의 QEMU 11.1.2(HVF)로 같은 이미지를 cocoa 창으로 띄워 1280×800 바탕화면과 메뉴바, Dock이 뜨는 것을 QMP 화면 캡처로 확인. 이미지 복사 10초. 이어서 사용자가 창에서 Mac 키보드와 마우스로 조작해 시계(한국 시간), 화면, 입력이 정상임을 확인 |
| macOS형 메뉴바 arm64 (10-28) | 이미지 4.0G, 빌드 90~120초(캐시 있음). 앞에 있는 앱의 메뉴(텍스트 편집기: 정보·종료, 새 창·닫기, 확대/축소·전체 화면, 도움말)와 메뉴 사이를 포인터로 옮겨 다니기, 확대/축소와 전체 화면에서 메뉴바가 검은색으로 바뀌고 전체 화면에서 Dock이 숨었다 돌아오는 것, 모든 앱 목록의 검색과 Enter 실행, Dock의 실행 중 표시, 알림(D-Bus로 보낸 시험 알림)의 배너·알림 센터·달력 위젯, 네트워크 아이콘(유선 연결)과 상태 문장, 메뉴로 앱을 끝내면 남은 창이 활성화되는 것을 확인했다(빌드 VM 안에서 QMP로 조작). NetworkManager로 QEMU 유선 연결이 인터넷까지 닿았다(`nmcli` connectivity full). e2e 1번도 다시 통과(로그인 15초, 상태 `running`) |
| 필수 드라이버 arm64 (10-28) | non-free-firmware의 Wi-Fi·Bluetooth·GPU·오디오 펌웨어, wpasupplicant, bluez, PipeWire, UPower, power-profiles-daemon, iio-sensor-proxy와 Firefox ESR을 넣은 이미지 5.7G(사용 5.5G). 시험 VM에서 NetworkManager·wpa_supplicant·upower·power-profiles-daemon이 돌고, PipeWire가 가상 사운드 장치(virtio-snd)를 출력으로 잡는다. bluetooth.service는 어댑터가 없어 조건대로 뜨지 않는다 |
| x86-64 (10-28) | 필수 드라이버와 CPU 마이크로코드(amd64-microcode, intel-microcode), thermald를 넣은 지금 레시피로 빌드했다. 이미지 5.4G, 교차 빌드, amd64 패키지를 처음 받아 20분. e2e 1번 통과(TCG): 로그인 65초, 셸 72초, 상태 `running`, 전원 꺼짐 79초. 커널 모듈 initrd를 줄인 뒤 다시 빌드해(UKI 79MB) 다시 통과했다(로그인 73초. 다른 VM과 함께 돌렸다). 자체 컴포넌트(그래픽 세션)는 아직 빠져 있다 |
| 메뉴 막대 항목·제어 센터·모든 앱 arm64 (10-28) | 시험 VM에서 QMP로 조작해 확인했다.<br>- 메뉴 막대: 입력 소스(메뉴에서 2벌식을 고르면 "A"→"한"), 사운드(음량 막대, 출력 장치), 전원(전원 어댑터, 전원 모드 저전력·자동), 네트워크(유선 상태)<br>- 제어 센터: 다크 모드를 켜면 셸 전체가 바로 어두워진다. 방해 금지를 켜면 메뉴 막대에 달 아이콘이 생긴다<br>- 모든 앱 패널: 가운데 유리 패널, 분류 칩, 검색(영문·한글), Esc·바깥 누르기로 닫기<br>- 배경화면: 설정 값을 바꾸면 바로 바뀐다<br>- 앱 항목: 시험용 StatusNotifierItem 스크립트로 아이콘 표시, 메뉴(dbusmenu)의 누르기 이벤트 전달, 앱이 끝나면 사라짐 |
| 한글 입력기·컴포지터 API arm64 (10-28) | gnome-text-editor와 foot에 한글 입력(조합 중 글자 표시), Ctrl+Space·한/영 전환(입력 칸에 포커스가 없어도), 한자 후보 창이 글자 커서 아래에 뜨는 것을 확인했다. fcitx5를 끄면 컴포지터가 다시 띄운다. 컴포지터 API의 화면 모드(1024×768)·배율(2배)·키 반복이 바로 적용되고 다시 로그인한 뒤에도 남는다. VNC로 QEMU 화면 크기를 1440×900으로 바꾸면 컴포지터가 따라간다 |
| 시스템 설정 v1 arm64 (10-28) | 페이지 12개를 명령줄과 Dock으로 열고 사이드바 검색, 뒤로·앞으로를 확인했다.<br>- Wi-Fi: mac80211_hwsim과 NetworkManager 핫스팟으로 검색, 암호 입력, 연결, 세부사항, 지우기, 틀린 암호, 열린 네트워크 연결<br>- 배경화면: 고르면 셸이 바로 바꾼다<br>- 디스플레이: 배율을 컴포지터 API로 바꾼다<br>- 키보드: fcitx5 입력 소스 목록을 읽는다<br>- 다크 모드에서도 모든 틀이 토큰대로 바뀐다<br>- Bluetooth·배터리·밝기는 QEMU에 장치가 없어 "없음" 상태만 확인했다 |
| 통합 이미지 arm64 (10-28) | 세 갈래 작업을 합친 이미지 5.7G(UKI 87MB). e2e 1번 통과: 로그인 18초, 셸 21초, 상태 `running`, 전원 꺼짐 22초. 같은 이미지에서 확인한 것:<br>- 모든 앱 패널: 흐린 유리, "calc" 검색<br>- 제어 센터<br>- 텍스트 편집기에서 Ctrl+Space 뒤 "안녕하세요" 입력, 메뉴 막대가 "한"으로 바뀜<br>- 시스템 설정의 키보드·Wi-Fi(장치 없음) 페이지<br>- 메뉴 막대 메뉴: Esc로 닫히고 닫은 뒤 입력이 앞 창으로 감, 열 때 칠해진 줄 없음<br>- cargo fmt·clippy(경고 0)·test(52개), reuse lint 통과 |
| 화면 배율 arm64 (10-28) | Firefox ESR을 최대화한 채로 100%↔125%를 오가면 Firefox가 새 영역에 맞게 다시 그린다(이 VM에서는 다시 그리는 데 몇 초 걸린다). 해상도를 1920×1200으로 올리면 100~175%를 고를 수 있고, 175%에서 Firefox·시스템 설정·Dock이 화면 안에 선명하게 들어간다. 화면 밖으로 끌어 둔 시스템 설정 창은 125%로 바꾸면 화면 안으로 옮겨지고 높이가 줄어든다. 배율을 바꿔도 창이 쌓인 순서는 그대로다. 125%에서 제어 센터의 흐림이 판과 맞고, 모든 앱 패널이 메뉴 막대와 Dock 사이에 들어간다. 3840×2160에서 다섯 개 선택지(100, 150, 200, 275, 325%)가 나오고 셸과 시스템 설정은 200%로 바뀐다(이때 2GB 시험 VM의 메모리가 모자라 Firefox가 꺼졌다). 디스플레이 페이지의 크기 고르기, "~처럼 보임", 고급의 해상도를 한국어·영어로, 시스템 설정의 뒤로·앞으로 알약(가리키면 반쪽만 칠함)을 확인했다 |
| 막대 자동 가리기·설정 페이지 arm64 (10-28) | Mac 창(QEMU cocoa, HVF)에서 QMP로 조작해 확인했다.<br>- 메뉴 막대와 Dock을 "최대화하거나 전체 화면일 때"로 두고(Dock에서는 이 동작이 뒤에 "전체 화면일 때"가 됐다) 텍스트 편집기를 최대화하면 두 막대가 가장자리 너머로 미끄러져 숨고(중간 프레임도 잡혔다) 창이 1280×800을 채운다<br>- 위 끝을 가리키면 메뉴 막대가 나온다(검은 배경, 지금 시각). 떠나고 0.1초 뒤에는 남아 있고 1.1초 뒤에는 숨는다. Dock은 아래 끝에서 나온다<br>- 메뉴 막대의 파일 메뉴를 연 채로 포인터가 떠나도 1.5초 뒤까지 남고, Esc로 닫으면 숨는다<br>- 메뉴 막대 "안 함": 최대화한 창이 메뉴 막대 아래로 다시 맞춰진다. Dock "항상": 보통 창일 때도 숨고, 꺼낸 Dock의 계산기를 누르면 계산기가 뜬다<br>- 기본값("전체 화면일 때")에서 F11 전체 화면도 같다. 전체 화면을 끝내면 두 막대가 바로 돌아온다<br>- 이름을 바꾼 "자동으로 Dock 가리기"(10-28): 선택지가 항상, 전체 화면일 때, 안 함 셋이다. 처음에는 "전체 화면일 때"가 진짜 전체 화면에서만 숨어서, 창의 확대 단추(□)로 최대화하면 "안 함"과 똑같이 Dock이 남았다(사용자가 찾았다). 최대화한 창도 전체 화면으로 보게 고친 뒤 확인했다: 확대 단추로 최대화하면 Dock이 숨고 창이 화면 아래 끝까지 채우며 아래 끝을 가리키면 나온다. 최대화한 채로 "안 함"을 고르면 Dock이 돌아오고 창이 그 위로 다시 맞춰진다. "안 함"이면 F11 전체 화면 창도 Dock 위까지만 채운다(메뉴 막대는 제 설정대로 숨는다)<br>- 기본 앱: image/png는 이미지 뷰어(Loupe), text/html은 Firefox ESR, PDF는 Papers, 폴더는 파일, text/plain은 텍스트 편집기<br>- 시스템 설정의 데스크탑 및 Dock(크기·투명도가 Dock에 바로 반영), 메뉴 막대, 기본 앱(파일 형식 시트 검색) 페이지<br>- 컴포지터·셸 경고 0. cargo fmt·clippy(경고 0)·test(67개) 통과 |
| 입력·성능·Dock·슬라이더 arm64 (10-28) | Mac 창(QEMU cocoa, HVF)에서 QMP와 직렬 콘솔로 확인했다.<br>- 시스템 설정의 "입력 소스 추가" 시트를 띄운 채 부모 창을 눌러도 시트가 위에 남고, 글자는 시트로 들어간다. GTK가 xdg-dialog-v1로 시트를 모달로 알린다(WAYLAND_DEBUG). 취소한 뒤 부모 창의 누르기가 먹는다<br>- 컴포지터 CPU(셸 흐림 켬, llvmpipe): 다른 창에 다 가려진 시스템 모니터 39% → 0%(시스템 모니터 4% → 0%). 시스템 모니터가 보이는 채로 포인터를 계속 움직일 때 124% → 68%(60Hz 제한). 아무 창도 움직이지 않을 때 1% 이하<br>- 보이는 시스템 모니터가 있으면 컴포지터가 여전히 약 40%를 쓴다. 주 스레드는 2.5%이고 나머지는 llvmpipe가 그 창의 그래프를 합성하는 데 쓴다(초당 약 20프레임)<br>- 메모리: 시스템 설정 페이지 40번 넘기기와 앱 8번 열고 닫기를 두 번 했을 때 두 번째에는 늘지 않았다(누수 없음)<br>- Dock: 크기 96 → 32 → 48에서 판이 1120 → 480 → 640px로 줄고 늘며 가운데(x=640)에 있다. 96px에 실행 중인 앱 3개를 더하면 아이콘이 줄어 판이 화면 안(x 30~1252)에 들어간다<br>- 슬라이더: 데스크탑 및 Dock과 키보드 페이지의 네 트랙이 모두 x 810~1045(236px)다 |
| 마우스·커서·성능 arm64 (10-28) | Mac 창에서 QMP와 직렬 콘솔로 확인했다.<br>- 커서 평면: `planes primary 1, cursor 1`이고 debugfs에서 plane-1이 crtc-0에 붙어 있다<br>- 커서 모양: GTK가 `wp_cursor_shape_device_v1.set_shape`를 보내고 커서 표면을 보내지 않는다(WAYLAND_DEBUG)<br>- 마우스 페이지: 연결된 마우스에 "QEMU Virtio Tablet"이 보인다. 포인터 크기를 끌면 컴포지터가 16px로 바꿔 저장하고(`comp-state.toml`의 `[pointer]`) `org.gnome.desktop.interface cursor-size`도 16이 된다<br>- 창 옮기기: 시스템 설정의 제목 막대를 y=5까지 끌어도 창 위쪽 끝이 y=28(메뉴 막대 아래)에 멈춘다<br>- 성능: 위 알려진 문제의 수치(시스템 모니터 7%, 창 넷 9%, 끌기 62%). 시스템 모니터 그래프가 계속 바뀌고 남은 조각이 없다<br>- cargo fmt·clippy(경고 0)·test 통과 |
| 확인하지 않은 것 | Mac 창에 보이는 커서의 모양과 자리(QMP 화면 캡처에는 커서 평면이 나오지 않는다). 실제 USB·Bluetooth 마우스의 이름·연결 방식과 이동 속도·자연스러운 스크롤·보조 클릭(QEMU에는 가상 태블릿뿐이고 절대 좌표 장치에는 가속이 없다). `tools/devvm/buntos-dev.yaml`의 컨테이너 런타임 끄기 설정(검증한 VM은 그 설정을 넣기 전에 만들었다). x86-64 그래픽 세션(자체 컴포넌트를 x86-64로 빌드하는 것은 CI에서 한다). Bluetooth 연결·페어링, 배터리 표시, 화면 밝기, 실제 Wi-Fi 칩(QEMU에 장치가 없다). 실제 앱이 보낸 알림과 실제 앱의 메뉴 막대 항목(시험 알림과 시험 스크립트만 썼다). 분수 배율의 실제 HiDPI 화면(QEMU 화면은 약 100ppi라 기본 배율은 늘 100%다). Mac 창(view-mac.sh)에서 사람이 조작한 배율 변경 |

## 7. 알려진 문제

| 문제 | 원인 | 대처 |
|---|---|---|
| 첫 빌드가 `/var/cache/mkosi` 권한 오류로 실패 | mkosi는 일반 사용자로 실행해도 이 경로에 패키지 캐시를 둔다 | `setup.sh`가 사용자 소유로 만든다 |
| "systemd-boot binary was not found" 빌드 오류 | Debian은 systemd-boot의 EFI 파일을 `systemd-boot-efi` 패키지로 나눈다 | 레시피 패키지 목록에 넣었다 |
| 자동 로그인 직후 로그인이 반복되며 셸이 뜨지 않음 | Debian에서 `login`은 Priority: required지만 Essential이 아니어서 mkosi가 설치하지 않는다. `apt`, `passwd`도 같다 | 레시피 패키지 목록에 넣었다 |
| 부팅이 시간대 입력 화면에서 멈춤 | systemd-firstboot의 대화형 설정 | 레시피에 로캘·시간대·키맵·호스트 이름을 정하고 `systemd.firstboot=no`로 끈다. 실제 최초 설정은 따로 설계해야 한다 |
| `/dev/tpm0`, `/dev/tpmrm0`를 기다리다 90초씩 지연 (arm64) | mkosi가 붙이는 에뮬레이션 TPM을 게스트 커널이 장치로 잡지 못했다(원인 미확인) | 레시피에서 `TPM=no`로 끈다. 원인은 따로 확인한다 |
| `--console=read-only`, `interactive`를 쓸 수 없음 | Debian 13 호스트에 `systemd-pty-forward`가 없다 | 기본(native) 콘솔을 쓴다 |
| 홈 디렉터리가 root 소유라 셸 설정을 읽지 못함 | 이미지를 비특권으로 빌드하면 파일 소유자를 바꿀 수 없다 | 부팅 때 tmpfiles로 만든다 (`/usr/lib/tmpfiles.d/buntos-home.conf`) |
| 시계가 UTC로 나옴 | `tzdata`도 Priority: required지만 Essential이 아니어서 설치되지 않았다 | 레시피 패키지 목록에 넣었다 |
| 화면 장치가 없는 VM에서 `getty@tty1` 실패 | 그래픽 세션이 바로 끝나며 자동 로그인이 반복됐다 | 화면 장치가 있을 때만 세션을 열고, 세션이 끝나면 셸로 돌아온다 |
| GTK 앱이 CPU로 그린다 | 컴포지터가 아직 dmabuf(linux-dmabuf)를 지원하지 않는다 | 세션에서 `GSK_RENDERER=cairo`. dmabuf 지원은 다음 단계 |
| 배율을 높이면 Firefox 본문이 한 번 더 확대돼 화면 밖으로 잘림 | Firefox는 본문을 하위 표면에 그리고 그 크기를 wp_viewporter로 알린다. 컴포지터에 viewporter가 없어 2배로 그린 본문을 1배 버퍼로 받아 다시 2배로 키웠다. 또 1280×800에서 200%를 허용해 논리 화면(640×400)이 Dock과 앱의 최소 크기보다 작았다 | wp_viewporter, wp_fractional_scale_v1, wl_compositor v6를 넣었다. 논리 화면이 1024×640보다 작아지는 배율은 두지 않는다. 배율이 바뀌면 창을 화면 안에 맞춘다([디자인 스펙](../design/DESIGN-SPEC.md) 3.1절) |
| 창 최소화가 없다 | bunt-comp 범위 밖 | 다음 단계. 창 버튼은 최대화와 닫기만 보인다 |
| GTK 앱이 시스템 설정(글꼴, 밝게·어둡게, 창 버튼)을 따르지 않음 | 지금의 GTK는 이 설정을 xdg-desktop-portal로 읽는데 포털이 없었다 | 포털과 GTK 구현(`xdg-desktop-portal-gtk`)을 넣고, 세션 환경을 bunt-shell보다 먼저 D-Bus에 알린다(`comp.toml`의 `prepare`) |
| 강조 색을 바꿔도 앱에 반영되지 않음 | GTK 포털은 `org.freedesktop.appearance`의 `color-scheme`만 주고 `accent-color`는 주지 않는다 | 시스템 설정 v0에서 강조 색을 뺐다. 강조 색을 전하는 포털 구현이 생기면 넣는다 |
| sudo가 매번 "unable to resolve host"를 경고 | 호스트 이름을 `/etc/hosts`에 적지 않았다 | `libnss-myhostname`을 넣었다. 시스템 설정에서 바꾼 이름도 찾는다 |
| 셸의 메뉴바·Dock이 보통 창(제목 표시줄 "bunt-shell")으로 뜸 | gtk4-layer-shell은 libwayland-client보다 먼저 링크돼야 동작한다. 창 목록을 받으려고 넣은 wayland-client 크레이트가 libwayland-client를 직접 링크해 순서가 바뀌었다 | wayland-client를 dlopen으로 연다(`shell/Cargo.toml`). `readelf -d`로 링크 순서를 확인할 수 있다 |
| Dock이나 알림 배너를 숨기면 셸의 연결이 끊김 | smithay 0.7은 레이어 표면 객체가 없어지면 표면의 레이어 상태를 기본값으로 되돌리면서 크기를 확인하는 커밋 훅은 남긴다. 숨긴 뒤 빈 버퍼를 커밋하면 이미 없는 객체의 프로토콜 오류가 된다 | 레이어 표면 객체가 없어질 때 컴포지터가 상태를 손본다(`comp/src/state.rs`). smithay에 알릴 거리다 |
| 메뉴바 메뉴를 닫은 뒤 키 입력이 앱으로 가지 않음 | smithay는 팝업 그랩이 끝나면 키보드 포커스를 팝업의 부모(키보드를 받지 않는 메뉴바)로 돌린다 | 키보드를 받지 않는 레이어로 포커스가 가면 앞에 있던 창으로 돌려준다 |
| GNOME 앱의 창 안 메뉴(☰)가 메뉴바로 옮겨지지 않음 | GNOME 앱은 창 안의 메뉴를 밖으로 내보내지 않는다 | 메뉴바의 앱 메뉴는 앱이 D-Bus로 내보내는 동작(정보, 설정, 새 창, 종료, 도움말)과 창 조작으로 만든다 |
| 팝업 메뉴를 닫은 직후 QMP로 누른 곳이 엉뚱하게 눌림 | smithay는 팝업 그랩이 끝난 뒤 첫 포인터 이동을 그랩을 푸는 데 쓰고 버린다. QMP 클릭은 이동이 한 번뿐이었다 | `qmp.py`가 누르기 전에 포인터를 두 번 옮긴다. 실제 마우스는 이동이 이어져서 영향이 없다 |
| 테스트로 띄운 이미지가 바뀜 | mkosi `vm`은 이미지를 직접 쓴다. 이미지를 `-blockdev`로 붙여서 QEMU `-snapshot`도 효과가 없다. `--ephemeral`은 이미지 전체를 복사하는데, Mac 메모리가 빠듯할 때 첫 부팅이 1분 넘게 늦어졌다 | e2e와 화면 캡처는 VM을 멈춘 상태로 띄우고 QMP로 디스크에 임시 qcow2 덮어쓰기를 씌운 뒤 실행한다(`tools/image/qmp.py`의 `overlay`) |
| Mac 창에서 바탕화면이 640×400으로 뜸 | QEMU cocoa는 창 크기를 게스트에 해상도로 알려 준다. zoom-to-fit을 켜면 처음 창(640×480)을 화면 비율에만 맞춰 그 크기가 된다 | `view-mac.sh`는 zoom-to-fit 없이 띄워 1280×800으로 둔다. bunt-comp는 이제 선호 해상도가 바뀌면 따라가고(2026-10-28), 해상도는 시스템 설정의 디스플레이 페이지에서도 바꾼다 |
| 테스트 VM이 떠 있으면 이미지 빌드가 "Cannot lock … locked by another process"로 실패 | mkosi `vm`은 VM이 도는 동안 이미지를 잠근다 | `tools/image/vmctl.sh`는 QEMU를 직접 띄워 이미지에 qcow2 덮어쓰기를 씌운다. 빌드는 이미지별 잠금으로 차례를 지킨다. mkosi `vm`(e2e, `screenshot.sh`)이 도는 동안의 빌드는 2분까지 기다린 뒤 실패한다 |
| 빌드 중에 `build.sh`를 고치면 그 빌드가 엉뚱한 줄을 실행할 수 있음 | bash는 스크립트를 실행하면서 파일을 이어 읽는다. 같은 파일을 덮어쓰면 읽던 자리가 어긋난다 | 빌드가 도는 동안에는 새 파일로 쓴 뒤 바꿔 넣는다(rename). 이미 연 파일은 그대로 남는다 |
| 모든 앱 패널 검색 칸에 글자가 들어가지 않음, 패널을 Esc로 닫은 직후 첫 클릭이 사라짐 | bunt-comp가 keyboard_interactivity가 exclusive인 레이어 표면에 키보드 포커스를 주지 않았고, 숨긴 표면에 포커스를 남겨 입력기의 키보드 그랩이 풀리지 않았다 | wlr-layer-shell 명세대로 exclusive 레이어가 나타나면 포커스를 주고, 사라지면 창으로 돌려준다(on_demand는 누르면 준다) |
| 유리 패널 뒤 창의 글자가 비쳐 겹침 | 컴포지터에 흐림이 없었다. macOS의 유리는 뒤를 흐리게 한다 | bunt-comp가 셸 표면 뒤를 흐리게 한다(`[effects] blur`, 기본 켬). 소프트웨어 렌더러에서 메뉴를 열 때 한두 프레임 늦어질 수 있다. 끄면 유리 토큰의 불투명도를 높인다([디자인 스펙](../design/DESIGN-SPEC.md) 2.2) |
| Ctrl+Space를 앱이 받지 못함 | 입력 소스 전환을 컴포지터가 전역으로 가로챈다(macOS와 같다) | 단축키를 바꾸는 설정은 다음 작업이다 |
| 부하가 클 때 셸과 앱이 1분쯤 늦게 뜸 | GTK가 xdg-desktop-portal을 기다리다 시간이 초과된다(25초×2, `Cannot get portal org.freedesktop.host.portal.Registry version`). 빌드 VM에 테스트 VM 세 대와 빌드가 함께 돌 때 보였다 | 시험할 때 테스트 VM 수를 줄인다. 셸 쪽 대처는 다음 작업이다 |
| 저장된 Wi-Fi 연결의 암호가 바뀌면 다시 묻지 못함 | NetworkManager 비밀 에이전트가 없다 | 새로 연결할 때 틀린 암호는 만든 연결을 지우고 알린다. 셸에 에이전트를 두는 것이 다음 작업이다 |
| PIN이나 확인 번호가 필요한 Bluetooth 기기(키보드 등)를 짝지을 수 없음 | BlueZ 페어링 에이전트가 없다 | 다음 작업 |
| `gdk_pixbuf::Pixbuf::from_file`이 그림을 못 읽음 | Debian forky의 gdk-pixbuf는 그림 형식 로더 없이 glycin에 맡긴다 | 그림은 `gdk::Texture::from_file`(GTK)로 읽는다 |
| fcitx5의 키 반복이 바뀐 값을 바로 따르지 않음 | fcitx5는 키보드를 잡을 때 받은 반복 값으로 반복을 직접 흉내 낸다 | 이미 포커스된 입력 칸은 다음 포커스부터 새 값을 쓴다 |
| 필수 펌웨어를 넣은 뒤 부팅이 30초 넘게 늦어짐 | mkosi는 UKI에 모든 커널 모듈과 그 모듈이 쓰는 펌웨어를 담은 initrd를 붙인다. 펌웨어 패키지를 넣자 initrd가 220MB가 됐다 | 커널 모듈 initrd에는 루트를 여는 모듈만 넣는다(`image/mkosi.conf`의 `KernelModulesInitrdInclude`). 나머지는 루트로 바꾼 뒤 읽는다. UKI 268MB→87MB, 로그인 57초→19초 |
| 메뉴 막대 메뉴를 열면 포인터가 메뉴 밖에 있어도 맨 아래 항목이 가리킨 것처럼 칠해짐 | 셸이 팝오버가 뜬 뒤(show)에 내용을 채워 크기가 바뀌고 다시 배치됐고, GTK가 메뉴가 뜰 때 포인터가 메뉴 위에 있다고 보고 hover를 줬다. 컴포지터는 팝업에 포인터 이벤트를 보내지 않았다(WAYLAND_DEBUG로 확인) | 메뉴 단추의 create-popup에서 띄우기 전에 채우고, 가리킨 줄은 포인터가 메뉴 안에서 실제로 움직인 뒤에만 칠한다(`shell/src/extras.rs`의 pointer-moved) |
| 셸에서 메뉴 막대·Dock을 숨기면 막대가 다시 그려지지 않음 | 셸이 창을 최대화한 직후 막대를 숨기면(CSS 클래스와 입력 영역) gtk4-layer-shell 표면이 커밋을 멈추고, 포인터가 막대 자리에 들어와야 다시 그렸다. GTK 쪽 원인은 찾지 못했다 | macOS의 WindowServer처럼 컴포지터가 막대를 숨기고 꺼낸다(`comp/src/panels.rs`). 셸은 막대 표면을 그대로 둔다 |
| 빌드 VM 안의 시험 VM에서 애니메이션 시간을 잴 수 없음 | 화면 없이 띄운 QEMU가 vblank(플립 완료)를 고르지 않게, 몇 초씩 늦게 준다 | 애니메이션과 타이밍은 Mac 창(QEMU cocoa, HVF)에서 확인한다 |
| 시스템 설정에서 누르기가 먹지 않음(가리키면 칠해지기는 한다). Esc를 누르면 풀린다 | 시트(모달 창, 예: 입력 소스 추가)를 띄운 채 부모 창을 누르면 컴포지터가 부모를 시트 위로 올려 시트가 뒤에 숨었다. GTK는 모달 창이 떠 있는 동안 부모의 누르기를 버리고, Esc는 숨은 시트에 가서 시트를 닫았다(QMP로 재현) | 부모가 있는 창(대화 상자)은 부모와 함께 앞으로 와서 부모 위에 남는다. 모달 대화 상자(xdg-dialog-v1)가 있으면 그 대화 상자가 활성화되고 키보드를 받는다(`comp/src/state.rs`의 `focus_window`) |
| 포인터를 움직이면 컴포지터가 CPU 한 코어 넘게 씀(시스템 모니터를 띄운 채 130%) | 커서 이미지를 Abgr8888로 만들어서, smithay가 커서를 커서 평면(크기 64×64, Argb8888)에 올리지 못하고 프레임마다 화면에 합성했다. virtio-gpu의 커서 큐 인터럽트가 0이었다 | 커서 버퍼를 xcursor 파일 바이트 그대로 Argb8888로 만든다(`comp/src/cursor.rs`). 빨강과 파랑이 바뀌어 보이던 문제도 함께 고쳐졌다(기본 화살표는 무채색이라 보이지 않았다) |
| 보이지 않는 창이 CPU를 씀(다른 창 뒤의 시스템 모니터에 컴포지터 39%, 대부분 llvmpipe) | 모든 표면에 프레임마다 프레임 알림(frame callback)을 보내서, 가려진 창의 애니메이션도 화면 주사율(QEMU 창은 Mac 화면을 따라 120~144Hz)로 그려졌다. Dock 아래에 걸친 창은 멀리 떨어진 곳을 그려도 커밋할 때마다 Dock 흐림을 다시 했다 | 이번 프레임에 보인 표면에만 바로 알리고, 다 가려진 표면과 숨은 막대는 1초에 한 번 알린다(`comp/src/udev.rs`). 흐림은 아래 요소의 손상이 흐릴 범위에 닿을 때만 다시 한다(`comp/src/blur.rs`) |
| 직렬 콘솔에서 띄운 GTK 앱이 메모리를 많이 씀(시스템 설정 475MB) | 세션 밖에서 띄우면 `GSK_RENDERER=cairo`가 없어 GTK가 GL(llvmpipe)로 그린다 | 콘솔에서 앱을 띄울 때 `GSK_RENDERER=cairo`를 함께 준다. 같은 조건에서 시스템 설정은 페이지를 처음 열 때만 늘고(102→164MB) 다시 돌아도 늘지 않았다 |
| Dock 크기를 키운 뒤 줄이면 아이콘만 작아지고 Dock 판은 큰 채로 남고 아이콘이 왼쪽 위로 몰림 | gtk4-layer-shell은 표면 크기를 전체 화면 창의 크기처럼 GTK에 알리고, GTK는 그 크기를 계속 쓴다. 크기를 바꿀 수 있는 창은 내용이 최소 크기보다 클 때만 늘리므로 줄지 않았다. 96px에서 32px로 바꾸면 창이 1154×152로 남았다(맞는 크기 514×88) | Dock 창을 크기를 바꿀 수 없는 창으로 둔다(`shell/src/dock.rs`의 `set_resizable(false)`). GTK가 창 크기를 내용 크기로 맞춰, 96→32→48에서 1154×152 → 514×88 → 674×104가 됐다 |
| 시스템 설정의 슬라이더 길이가 행마다 다름(크기가 투명도보다 김) | 슬라이더의 hexpand가 행까지 퍼져, 제목이 남긴 너비를 제목과 나눠 가졌다. 제목이 짧을수록 슬라이더가 길었다: 크기 390px, 투명도 384px, 키 반복 속도 370px, 반복 지연 시간 364px. 양 끝 이름도 트랙보다 12px씩 바깥에 있었다 | 슬라이더 행의 조작 너비를 260px로 고정하고 hexpand를 끈다(`settings/src/widgets.rs`의 `slider_row`). 네 트랙 모두 236px로 같은 자리에 있고, 양 끝 이름은 트랙 끝 아래에 온다 |
| 하드웨어 커서 평면을 쓰지 않아 포인터를 움직일 때마다 화면을 다시 합성함 | 가상 GPU(virtio-gpu)의 커널은 커서 핫스폿을 알리겠다고 한(DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT) 클라이언트에게만 커서 평면을 보인다. smithay 0.7은 이 기능을 모른다(기록: `planes primary 1, cursor 0`). DrmDevice를 만든 뒤에 켜면 smithay의 원자적 모드 표면이 그 평면을 몰라 출력 설정이 `UnknownPlane`으로 실패하고 컴포지터가 꺼졌다 | DrmDevice보다 먼저 원자적 모드와 핫스폿 기능을 켠다(`comp/src/udev.rs`). 커서 버퍼는 Argb8888로 화면 픽셀 크기와 같게 만든다. 이제 커서가 커서 평면(plane-1)에 오른다(`/sys/kernel/debug/dri/0/state`). QMP 화면 캡처(screendump)에는 커서가 나오지 않는다 |
| GTK 앱이 조금만 바뀌어도 컴포지터가 창 전체를 다시 그림(시스템 모니터 하나에 42~48%) | 세션은 `GSK_RENDERER=cairo`이고, GTK의 cairo 렌더러는 그릴 때마다 버퍼 전체를 손상으로 알린다(시스템 모니터는 초당 10번 `damage_buffer(0, 0, 850, 718)`, WAYLAND_DEBUG로 확인). 창 그림자 여백이 Dock 밑에 닿아 Dock 흐림도 매번 다시 했다. perf로 보면 컴포지터 시간의 84%가 llvmpipe의 그리기였다 | 버퍼 거의 전체를 손상으로 알리는 SHM 커밋은 앞 커밋의 사본과 64×32 타일로 견주어 바뀐 타일만 손상으로 남긴다(`comp/src/shmdiff.rs`). 시스템 모니터를 띄운 채 42~48% → 7%, 창 넷을 띄운 채 50% → 9% |
| 여러 창 위에서 큰 창을 빠르게 옮기면 CPU가 크게 오름 | GPU가 없는 VM에서 컴포지터가 옮기는 창의 앞뒤 자리를 llvmpipe로 60Hz에 다시 합성한다(컴포지터 주 스레드는 3~4%). Mac의 Homebrew QEMU에는 GPU 가속(virtio-gpu-gl, virglrenderer)이 없다 | 손상 줄이기 뒤 700×520 창을 창 셋 위에서 원을 그리며 옮길 때 62%(전 72%). 소프트웨어 렌더러에서는 앞에 있지 않은 창의 프레임 알림을 30Hz로 줄인다. 더 줄이려면 QEMU의 GPU 가속이 필요하다 |
| 창 제목 막대를 메뉴 막대 밑으로 옮기면 다시 잡을 수 없음 | 컴포지터가 창을 어디로든 옮기게 했다 | macOS처럼 창의 위쪽 끝은 메뉴 막대 아래 끝을 넘지 못한다(옮기기와 위쪽 가장자리 크기 조절). Dock 쪽은 넘어가도 된다 |
| 메뉴 막대 메뉴가 Esc로 닫히지 않음 | 글자 칸에 포커스가 있으면 입력기(fcitx5)가 키보드를 잡고 있는데, 컴포지터가 이것을 다른 그랩으로 보고 메뉴의 키보드 그랩을 거절했다. 레이어 포커스 정리가 열린 팝업의 포커스를 창으로 되돌리기도 했다 | 입력기 그랩은 팝업 그랩을 막지 않고, 열린 팝업의 포커스는 건드리지 않는다(`comp/src/state.rs`) |
