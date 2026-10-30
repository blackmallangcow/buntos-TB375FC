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

mkosi(버전은 스크립트에 고정), QEMU, UEFI 펌웨어, TPM 에뮬레이터, x86-64 교차 빌드용 binfmt, 이미지 도구와 자체 컴포넌트를 빌드하는 개발 패키지(GTK4, gtk4-layer-shell, libinput, PAM 등)를 설치한다. 여러 번 실행해도 된다. 스크립트에 패키지가 더해지면(잠금 화면의 `libpam0g-dev`, 화면 기록 부호기 시험의 GStreamer 등) 다시 실행한다. 컴포지터의 화면 기록 부호기 시험(`recording::tests`)은 `gst-launch-1.0`이 없으면 알리고 건너뛴다. kvm 그룹 추가는 새 세션부터 적용되므로 처음 한 번은 VM을 다시 시작한다:

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

e2e 1번(부팅 → 로그인 화면 → 셸), e2e 5번(에이전트의 앱 간 제어), e2e 6번(키보드 단축키), e2e 4번(한글 입력), e2e 7번(화면 잠금)을 이 차례로 한 부팅에서 돌린다.
- 시험 계정을 자격 증명으로 넘겨 부팅하고, 로그인 화면에 QMP로 암호를 쳐 들어간 뒤 sudo가 암호를 묻는지 보고, 에이전트로 그래픽 셸을 확인한 다음 e2e 5번(`tests/e2e/agent-apps.sh`)을 돌린다.
- e2e 6번은 QMP로 단축키를 넣고 게스트 쪽 도우미(`tests/e2e/shortcuts.sh`)로 결과를 읽는다.
- e2e 4번은 QMP로 2벌식 자판의 키를 쳐(`dkssud` → 안녕) fcitx5가 조합하게 하고, 게스트 쪽 도우미(`tests/e2e/hangul.sh`)가 앱의 글(에이전트), 입력기 상태(`fcitx5-remote`, fcitx5 D-Bus `DebugInfo`), 메뉴 막대의 입력 소스 표시, 터미널(foot)의 cat이 받은 바이트를 읽는다. 텍스트 편집기, 시스템 설정, foot, Firefox(새 프로필)를 쓴다.
- e2e 7번은 QMP로 ⌃⌘Q를 넣어 화면을 잠그고 잠금 화면에 암호를 쳐 푼다. 게스트 쪽 도우미(`tests/e2e/session-lock.sh`)가 컴포지터 API(`GetLocked`)와 저널로 잠김과 에이전트 거절을 읽는다.
- 자체 컴포넌트를 넣지 않은 이미지(그래픽 세션이 없는 지금의 x86-64)에서는 e2e 5·6·4·7번을 건너뛴다.

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

- `wait-login`은 직렬 콘솔의 자동 로그인을 기다린 뒤 로그인 화면에 QMP로 암호를 쳐 그래픽 세션까지 들어간다(`VMCTL_GREETER_LOGIN=0`이면 들어가지 않는다). `login`은 로그인 화면에서 다시 들어간다(로그아웃한 뒤 등).
- `VMCTL_ACCOUNT="계정 이름:별칭:암호"`로 계정을 바꾸고, `VMCTL_ACCOUNT=`(빈 값)으로 계정 없이 띄워 처음 켤 때의 계정 만들기를 본다. QMP는 영문 소문자와 숫자만 치므로 암호도 그렇게 정한다.
- 임시 덮어쓰기는 기본 16G(`VMCTL_DISK_SIZE`, 빈 값이면 이미지 크기 그대로)이고, buntOS가 처음 켤 때 루트를 디스크 끝까지 늘린다. 덮어쓰기는 쓴 만큼만 자리를 차지한다.
- `push 파일 경로`는 빌드 VM의 파일을 직렬 콘솔로 게스트의 경로에 넣는다(실행 권한을 주고 sha256으로 견준다). 시험용 프로그램을 넣을 때 쓴다. 예: 예제 클라이언트 `comp/examples/glass-window`(앱 창 흐림과 dmabuf 시험)는 `cargo build --release -p bunt-comp --example glass-window`로 빌드해 `strip`한 뒤(약 0.9MB) 넣고, 세션 사용자로 띄운다: `sudo -u buntos env XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1 /tmp/glass-window --toggle --seconds=20`
- `VMCTL_GPU=virgl`이면 3D 가상 GPU(virtio-gpu-gl, virglrenderer)로 띄워 GPU 경로(linux-dmabuf, GTK의 GL)를 본다. 게스트의 GL은 빌드 VM이 그린다(GPU가 없으면 llvmpipe라 빨라지지는 않는다). 화면은 egl-headless로 받으므로 QMP 화면 저장이 된다.
  - 빌드 VM에 QEMU의 OpenGL 모듈(`qemu-system-modules-opengl`, `setup.sh`가 깐다)과 렌더 노드(`/dev/dri/renderD*`, `VMCTL_RENDERNODE`로 고른다)가 있어야 한다. 렌더 노드는 render 그룹이 연다.
  - Lima VM에는 GPU가 없다. vgem 커널 모듈(`sudo modprobe vgem`)로 렌더 노드를 만든다. vgem이 없는 커널이면 `VMCTL_RENDERNODE=/dev/null`로 Mesa의 소프트웨어 GBM을 써 본다(QEMU 8.2·Mesa 25.2에서 EGL 초기화까지만 확인했다).
  - Mac 창(`view-mac.sh`)으로는 볼 수 없다. QEMU의 cocoa 화면에 GL이 없고, Homebrew QEMU에는 virglrenderer가 없다.

**동작 목록.** `qmp` 동작은 `tools/image/qmp.py` 머리말에 있다: 누르기, 끌기, 단추를 누른 채 두기와 떼기(`press`, `release`), 키, 글자 입력, 화면 저장. 단축키는 `keys:meta_l+c`처럼 QEMU 키 이름으로 넣는다(⌘는 `meta_l`, ⌥는 `alt`, ⌃는 `ctrl`, ⇧는 `shift`).

**여럿이 함께 쓸 때.**
- 같은 이미지는 한 번에 하나만 빌드한다(`<이미지 디렉터리>.lock`). vmctl은 빌드가 끝나기를 기다렸다 띄운다.
- 띄운 뒤에는 빌드가 이미지를 새로 만들어도 VM은 이미 연 파일을 계속 쓴다.
- 다른 사람이 기본 이미지로 VM을 띄워 둔 동안 자기 변경을 따로 시험하려면 `BUNTOS_IMAGE_DIR`로 다른 디렉터리에 빌드하고 같은 변수로 띄운다. e2e도 같은 변수의 이미지를 띄운다:

  ```bash
  BUNTOS_IMAGE_DIR=~/buntos-build/mine tools/image/build.sh qemu-aarch64
  BUNTOS_IMAGE_DIR=~/buntos-build/mine VMCTL_ID=mine tools/image/vmctl.sh start &
  BUNTOS_IMAGE_DIR=~/buntos-build/mine tests/e2e/boot-to-shell.sh qemu-aarch64
  ```

- 다른 소스 트리(git worktree)에서 빌드할 때는 cargo 대상 디렉터리도 `CARGO_TARGET_DIR`로 따로 준다(기본은 함께 쓰는 `~/buntos-build/cargo-target`). cargo는 작업 공간 안의 경로를 상대 경로로 기억하므로, 두 소스 트리가 한 디렉터리를 쓰면 서로의 산출물을 다시 빌드하거나 소스가 더 오래된 쪽이 다른 트리의 바이너리를 그대로 이미지에 넣는다. 처음 빌드는 2분쯤 걸리고 디렉터리는 약 2GB다:

  ```bash
  BUNTOS_IMAGE_DIR=~/buntos-build/mine CARGO_TARGET_DIR=~/buntos-build/mine-cargo tools/image/build.sh qemu-aarch64
  ```

- mkosi `vm`(e2e, `screenshot.sh`)은 도는 동안 이미지를 잠가서, 그동안의 빌드는 2분까지 기다린 뒤 실패한다.

x86-64는 보드 이름만 `x86_64-generic`으로 바꾸면 된다. Apple Silicon에서는 패키지 설치 스크립트를 qemu-user로 에뮬레이션해 빌드하고, 부팅도 KVM 없이 소프트웨어 에뮬레이션(TCG)으로 한다. 이 빌드에는 자체 컴포넌트가 들어가지 않는다(빌드 VM과 아키텍처가 다르다). 자체 컴포넌트를 넣은 x86-64 이미지는 CI(`.github/workflows/x86-64.yml`, main에서는 손으로 돌린다)가 GitHub 호스티드 러너(`ubuntu-26.04`, KVM)에서 빌드하고 e2e 1·5·6·4·7번을 돌린다([설계안](../design/ARCHITECTURE.md) 9.4).

- 자체 컴포넌트는 이미지와 같은 Debian 스냅숏(`image/mkosi.conf`의 `Release=`·`Snapshot=`)의 컨테이너에서 `rust-toolchain.toml`의 툴체인으로 빌드한다(`tools/image/cargo-debian.sh`, `build.sh`의 `BUNTOS_CARGO`). 러너(Ubuntu)의 라이브러리는 이미지와 다르다.
- mkosi는 `setup.sh`에 고정한 판이고, mkosi가 부르는 이미지 도구(systemd, systemd-repart·ukify, bootctl, mkfs, QEMU, OVMF)는 빌드 VM과 같은 Debian 13의 도구 트리에서 쓴다(`tools/image/tools-tree.sh`, `build.sh`의 `BUNTOS_TOOLS_TREE`). 두 스크립트는 docker가 있는 Linux에서 쓴다.
- e2e가 실패하면 직렬 콘솔, 게스트 저널, 그때의 화면이 아티팩트(`e2e-x86_64-generic`)로 남는다. 로컬에서도 `E2E_LOG_DIR=디렉터리`를 주면 같은 기록(`console.log`, `journal.log`, 실패하면 `screen.png`)을 남긴다.
- CI에서 드러난 x86-64 문제는 `BOARD=x86_64-generic tools/image/vmctl.sh start`로 화면을 보며 재현한다(q35 기계와 OVMF. Apple Silicon에서는 TCG라 느리다).

### 그래픽 세션 보기

이미지 빌드는 자체 컴포넌트(bunt-comp, bunt-shell, bunt-settings, bunt-greeter, 에이전트)를 먼저 컴파일해 이미지에 넣는다.

이미지에는 사용자 계정이 없다. 처음 켜면 로그인 화면이 계정 만들기를 보인다. 개발·시험용 VM 도구(`tools/image/vmctl.sh`, `tools/image/view-mac.sh`, e2e)는 부팅 때 systemd 자격 증명 `buntos.account`로 시험 계정 `buntos`(uid 1000, 암호 `buntos`, 관리자)를 만들어 주므로 로그인 화면이 뜬다. 암호를 치고 들어가면 컴포지터가 셸(bunt-shell)과 입력기(fcitx5)를 띄운다. 직렬 콘솔은 그대로 root로 자동 로그인한다.

직렬 콘솔에서 앱을 띄우는 명령(`sudo -u buntos env ... setsid <앱> &`)은 그대로 쓸 수 있다. 이 sudo는 root가 쓰는 것이라 암호를 묻지 않는다. 그래픽 세션 안의 sudo는 `buntos` 암호를 묻는다.

빌드 VM과 아키텍처가 다른 x86-64 이미지에는 자체 컴포넌트가 들어가지 않는다. 자체 컴포넌트를 넣은 x86-64 이미지는 CI에서 빌드하고 e2e 1·5·6·4·7번으로 확인한다(4장 위, 6장).

**Dock.**
- 앱: 파일(Nautilus), 모든 앱, 웹 브라우저(Firefox ESR), 터미널(foot), 텍스트 편집기, 계산기, 이미지 보기(Loupe), 문서 보기(Papers), 시스템 모니터(GNOME Resources, 한국어 번역이 없어 영어로 보인다), 시스템 설정(bunt-settings)
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

빌드 VM에서 이미지(약 5.9GB)를 `~/Library/Caches/buntos/`로 가져온 뒤 HVF 가속으로 띄운다. 로그인 화면에서 `buntos`를 치고 들어간다.
- 디스크: 가져온 이미지를 `--size=크기`(기본 16G)로 성기게 늘리고, buntOS가 처음 켤 때 루트를 디스크 끝까지 늘린다. 그래서 VM에 앱(Claude 데스크톱 앱 등)을 설치할 자리가 있다. 이미지 파일은 게스트가 쓴 만큼만 Mac 자리를 차지한다.
- 바꾼 내용: 기본은 snapshot 모드라 창을 닫으면 사라진다. `--keep`이면 설치한 앱, 로그인, 키링이 이미지에 남는다. 이때 끌 때는 buntOS에서 시스템 종료를 한다.
- 한 번 띄워 바뀐 이미지는 새 빌드로 덮어쓰지 않고 멈춘다. 그 이미지를 계속 쓰려면 `--no-copy`로 띄우고, 새 이미지를 받으려면 지운다. `--setup`이면 계정 없이 띄워 계정 만들기를 본다. 같은 이미지를 다시 볼 때는 `--no-copy`를 붙이면 가져오는 단계를 건너뛰며, 이때는 빌드 VM이 꺼져 있어도 된다. Mac 메모리가 16GB 이하이면 이미지를 가져온 뒤 빌드 VM을 끄고(`limactl stop buntos-dev`) `--no-copy`로 띄우는 편이 낫다. 빌드 VM이 8GiB, 보기용 QEMU가 최대 4GiB를 쓴다.

**⌘ 단축키.** Mac 키보드의 Command(⌘)가 buntOS의 ⌘(Super)다. QEMU 창이 입력을 잡았을 때(포인터가 창 안에 있을 때)만 ⌘ 조합이 VM으로 가고, ⌃⌥G로 입력을 놓는다. ⌘⇥, ⌘스페이스, ⌃스페이스, ⇧⌘3·⇧⌘4처럼 macOS가 먼저 받는 단축키는 `--full-grab`을 붙여야 VM으로 간다. 이때 macOS 시스템 설정 > 개인정보 보호 및 보안 > 손쉬운 사용에서 이 스크립트를 실행하는 앱(터미널 앱)을 허용해야 한다. macOS는 QEMU보다 QEMU를 띄운 앱의 권한을 본다. 권한이 없으면 QEMU가 "Could not create event tap, system key combos will not be captured" 경고를 내고, 그 단축키는 macOS가 받는다(2026-10-29 확인).

화면은 1280×800으로 뜬다. 화면 크기(배율)는 시스템 설정의 디스플레이 페이지에서 고르고, 실제 해상도는 같은 페이지의 "고급"에서 바꾼다. 1280×800에서 고를 수 있는 크기는 100%와 125%(1024 × 640처럼 보임)다. 더 큰 배율은 해상도를 올리면 나온다(1920×1200은 175%까지). bunt-comp는 화면이 알려 주는 선호 해상도가 바뀌면 따라간다(사용자가 해상도를 고정하지 않았을 때).

| 조작 | 방법 |
|---|---|
| 앱 실행 | Dock 아이콘을 누른다. 터미널은 Option+Return으로도 연다 |
| 시스템 설정, 강제 종료, 잠자기, 다시 시작, 시스템 종료, 화면 잠금, 로그아웃 | 메뉴바 왼쪽의 로고(b) 메뉴 (macOS의 Apple 메뉴 순서). VM에서는 잠자기가 흐리다(잠든 VM을 깨울 장치가 없다) |
| 화면 잠금 | ⌃⌘Q 또는 로고 메뉴의 "화면 잠금". 잠자기에 들어갈 때도 잠근다. 로그인 암호로 푼다. 잠금 화면 모양만 볼 때는 `bunt-lock --preview` |
| 로그아웃 | ⇧⌘Q 또는 로고 메뉴의 "<별칭> 로그아웃…". 60초 뒤에 스스로 로그아웃한다. ⌥⇧⌘Q는 묻지 않는다. 로그인 화면으로 돌아간다 |
| 강제 종료 | ⌥⌘⎋ 또는 로고 메뉴의 "강제 종료…". 응답하지 않는 앱에는 "(응답 없음)"이 붙는다 |
| 앞에 있는 앱의 메뉴 | 로고 옆의 앱 이름과 파일·창·도움말 메뉴. 창 메뉴에 확대/축소와 전체 화면이 있다 |
| 알림 센터 | 메뉴바 오른쪽의 날짜와 시간을 누른다 |
| 제어 센터 | 메뉴바 오른쪽의 스위치 두 개 아이콘. Wi-Fi·Bluetooth, 방해 금지, 다크 모드, 저전력 모드, 음량(밝기는 내장 화면이 있을 때) |
| 전원 모드 | 메뉴바의 배터리(전원) 아이콘. 저전력 모드·자동·고성능을 고르면 바로 적용된다 |
| 입력 소스 | 메뉴바의 "A"/"한"을 눌러 고르거나 ⌃스페이스(Ctrl+Space), 한/영 키. 입력 칸에 포커스가 없어도 바뀐다. Mac 창에서 ⌃스페이스는 `--full-grab`이 필요할 수 있다 |
| 단축키 | macOS와 같다: ⌘C·⌘V·⌘X·⌘Z·⌘A, ⌘Q, ⌘W, ⌘M, ⌘H, ⌘⇥, ⌘스페이스, ⇧⌘3·⇧⌘4 등. 목록과 바꾸기는 시스템 설정 > 키보드 > 키보드 단축키…. Ctrl 단축키도 그대로 된다 |
| 한자 변환 | 한글을 쓰는 중에 한자 키 또는 F9. 숫자나 Enter로 고르고 Esc로 닫는다 |
| 모든 앱 | Dock의 앱 단추를 누르면 화면 가운데에 패널이 뜬다. 분류 칩으로 거르고, 이름을 치면 찾고, Enter로 첫 앱을 실행한다. 바깥을 누르거나 Esc로 닫는다 |
| 배경화면 | 시스템 설정의 배경화면 페이지. "사진 추가…"로 내 사진을 쓴다 |
| 시스템 설정의 페이지 바로 열기 | `bunt-settings wifi`처럼 페이지 이름을 준다. 이미 떠 있으면 페이지만 바뀐다. 이름은 [디자인 스펙](../design/DESIGN-SPEC.md) 9장. 설정 창에서 Ctrl+F는 검색, Alt+←/→는 뒤로·앞으로 |
| 창 옮기기 | 제목줄을 끈다. 제목줄은 메뉴 막대 위로 올라가지 않는다 |
| 창 크기 조절 | 창 가장자리를 끈다. 제목줄을 두 번 누르면 최대화와 복원 |
| 창 닫기 | 제목줄의 X 버튼, ⌘W(탭 또는 창), 또는 Option+F4 (Mac 노트북 키보드는 fn도 함께) |
| 최소화, 가리기 | ⌘M(창), ⌘H(앱), ⌥⌘H(다른 앱). Dock 아이콘이나 ⌘⇥로 돌아온다 |
| 스크린샷 | ⇧⌘3(화면), ⇧⌘4(끌어서 영역). ⌃를 더하면 클립보드. 파일은 ~/Pictures에 생긴다. Mac 창에서는 `--full-grab`이 필요하다 |
| Mac 창에서의 ⌃⌘Q, ⇧⌘Q, ⌥⌘⎋ | macOS의 화면 잠금, 로그아웃, 강제 종료 단축키와 같아 macOS가 먼저 받을 수 있다(Mac이 잠기거나 Mac의 로그아웃 확인이 뜬다). 로고 메뉴를 쓰거나 `--full-grab`으로 띄운다 |
| 세션 끝내기 | Ctrl+Option+Delete(⌫). tty1 셸로 돌아온다 |
| 끝내기 | QEMU 창을 닫는다 (종료 확인 창이 뜨면 Quit) |

**Wi-Fi 시험.** QEMU에는 Wi-Fi 장치가 없다. 시험 VM 안에서 가짜 무선 장치를 만들어 시험한다(root 직렬 콘솔에서).
1. 장치를 만든다: `modprobe mac80211_hwsim radios=3`
2. 핫스팟에 필요한 패키지를 넣는다. 이미지에는 넣지 않는다: `apt-get update && apt-get install -y --no-install-recommends dnsmasq-base`
3. 마지막 장치 wlan2에 WPA2 핫스팟을 띄운다(열린 핫스팟은 wifi-sec 없이 wlan1에):

   ```bash
   nmcli con add type wifi ifname wlan2 con-name lab-ap autoconnect no ssid buntOS-Lab mode ap ipv4.method shared 802-11-wireless.band bg wifi-sec.key-mgmt wpa-psk wifi-sec.psk buntos1234 && nmcli con up lab-ap
   ```

   `connection.permissions user:root`는 이제 필요 없다. 셸이 시스템 설정처럼 핫스팟 연결(mode ap)을 알고 있는 네트워크에서 뺀다.
4. 메뉴 막대와 시스템 설정은 핫스팟으로 쓰지 않는 첫 Wi-Fi 장치를 쓴다. 핫스팟을 wlan0에 띄워도 wlan1로 연결한다.
5. 저장된 네트워크의 암호가 바뀐 경우: `nmcli con modify lab-ap wifi-sec.psk newpass99 && nmcli con up lab-ap`. 클라이언트가 다시 붙다 실패하면 NetworkManager가 셸의 비밀 에이전트에게 새 암호를 묻는다(20초쯤 걸린다). 이 창은 키보드를 가져가지 않으므로 눌러서 친다.
6. 다른 프로그램이 켠 연결: 만든 연결을 지운 뒤 세션의 터미널에서 `nmcli dev wifi connect buntOS-Lab`(암호 없이)을 하면, NetworkManager가 USER_REQUESTED를 붙여 묻더라도 창은 키보드를 가져가지 않는다. 키보드는 메뉴 막대에서 사람이 방금 고른 네트워크에만 간다.

시스템 설정은 직렬 콘솔에서 `sudo -u buntos`로 띄우면 그래픽 로그인 세션 밖이라 polkit이 네트워크 변경을 거절한다. Dock에서 띄우고, 페이지만 `bunt-settings <페이지>`로 넘긴다.

**에이전트로 화면 읽기.** 시험 VM 안에서 앱의 접근성 트리를 ref가 달린 짧은 글로 읽는다(ARCHITECTURE 6장). 직렬 콘솔(root)에서는 사용자 세션의 런타임 디렉터리를 같이 준다:

```bash
sudo -u buntos XDG_RUNTIME_DIR=/run/user/1000 bunt agent snapshot -i
```

- 앱과 창 목록은 `bunt agent apps`로 본다.
- 범위는 `--app <이름>`(그 앱의 창), `--ref e12`(그 아래만)로 좁힌다.
- `--diff`는 지난번과 달라진 줄만 낸다.
- 데몬 기록(요청마다 걸린 시간)은 `journalctl _SYSTEMD_USER_UNIT=buntd-agent.service`로 본다.

**에이전트로 앱 조작하기.** 읽기와 같은 방법으로 부른다. 동작의 답은 화면이 멈춘 뒤 그 창의 diff다.

```bash
A() { sudo -u buntos XDG_RUNTIME_DIR=/run/user/1000 bunt agent "$@"; }
A launch org.gnome.Calculator     # 이름("계산기", "calculator")도 된다. 열려 있으면 앞으로 가져온다
A click e185                      # ref는 launch나 snapshot이 준다
A type "안녕하세요" --ref e5       # 글 칸에 글 입력으로 넣는다. 입력기가 한글 모드여도 된다
A press "ctrl+a BackSpace" --ref e5
```

- 직렬 콘솔의 `sudo -u buntos …`는 관리자 요청이라 접근 정책과 상관없이 받는다. 허용하지 않은 AI처럼 시험하려면 root 조상이 없게 사용자 서비스로 띄운다: `sudo -u buntos XDG_RUNTIME_DIR=/run/user/1000 systemd-run --user --pipe --wait python3 <스크립트>`. 거절된 요청은 `~/.local/state/buntos/agent-requests.toml`에 남는다.
- 정책을 손으로 시험할 때는 root로 `/etc/buntos/agent-access.toml`을 쓴다. 데몬이 요청마다 다시 읽는다.
- 데몬이나 CLI를 바꿔 넣을 때(`install`) 데몬을 멈추지 않아도 된다. 옛 데몬이 다음 연결 전에 끝나고 소켓이 새 데몬을 띄운다. 컴포지터는 `/usr/bin/buntd-agent`와 같은 파일인 프로세스만 받으므로, 다른 경로의 개발 빌드는 컴포지터 API를 쓰지 못한다.
- 세션을 다시 띄운 직후 처음 여는 GTK 앱은 포털을 기다리느라 20초 넘게 걸릴 수 있다. `launch`는 이때 "아직 창이 없다"고 답한다.

**진짜 관리자 인증 시험.**
- 이미지 준비:
  - 개발용 무인증 규칙(`50-buntos-dev.rules`, sudoers)이 없는 이미지는 polkit이 계정 암호를 묻는다.
  - 예전 이미지면 직렬 콘솔에서 `echo buntos:<암호> | chpasswd`로 암호를 주고 `/etc/polkit-1/rules.d/50-buntos-dev.rules`를 지운다.
- 앱 띄우기:
  - root 콘솔에서 `sudo -u buntos`로 띄운 앱은 root의 콘솔 세션에 남아 polkit이 인증 창을 찾지 못한다.
  - 사용자 서비스로 띄운다:
    `sudo -u buntos XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus systemd-run --user --setenv=WAYLAND_DISPLAY=wayland-1 --setenv=LANG=ko_KR.UTF-8 bunt-settings agent`
  - GTK의 렌더러(`GSK_RENDERER`)는 주지 않는다. 컴포지터가 정해 세션을 열 때 systemd 사용자 환경에 넣으므로(GPU가 없으면 `cairo`), 사용자 서비스는 그 값을 받는다
- QMP `type:`은 영문 소문자·숫자만 되므로 시험 암호도 그렇게 고른다.
- 도우미 바꿔 넣기:
  - 도우미는 `/usr/libexec/buntos/bunt-access-helper`, polkit 동작은 `/usr/share/polkit-1/actions/org.buntos.agent.access.policy`다.
  - polkitd는 동작 파일을 바로 다시 읽는다.
  - bunt-shell(인증 창)을 바꿨으면 세션을 다시 띄운다.

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

셸의 GTK 시험(`menu_names`)은 화면이 있을 때만 돌고, 없으면 건너뛴다고 알린다. 화면 없는 곳(빌드 VM, 컨테이너)에서 돌리려면 weston 헤드리스를 띄워 준다: `weston --backend=headless-backend.so --socket=wayland-test &` 뒤 `WAYLAND_DISPLAY=wayland-test cargo test --locked -p bunt-shell`.

`cargo test -p bunt-agent`는 REPL 워커를 seccomp로 가둔 스레드와 실제 워커 프로세스(`buntd-agent --repl-worker`)를 띄운다. 갇힌 뒤 죽는지 보는 시험은 시험 프로그램 자신을 `BUNT_SANDBOX_CRASH=1`로 다시 띄운다. 시험용 데몬은 `XDG_RUNTIME_DIR`을 시험 디렉터리로 두고 뜬다. root로 돌리면 관리자 길을, 사용자로 돌리면 허용하지 않은 AI의 거절 길을 본다. 시간 상한을 넘기는 워커를 보는 시험 몇은 5초씩 걸린다. rquickjs-sys가 QuickJS-ng의 C 소스를 빌드하므로 빌드 호스트에 C 컴파일러(`build-essential`, setup.sh에 있다)가 있어야 한다.

## 6. 검증 기록

2026-10-27에 1차 빌드와 그래픽 세션 v0를, 2026-10-28에 기본 앱, 시스템 설정, macOS형 데스크톱(메뉴 막대 항목, 제어 센터, 모든 앱), 한글 입력기, 필수 드라이버, x86-64 이미지를 확인했다. 2026-10-29에는 에이전트 읽기·동작 경로, AI 접근 정책과 인증 창, 계정과 로그인 화면, 키보드 단축키, AI 접근 요청 창, 에이전트 사용 후기 반영, VM 디스크와 login 키링, 세션 데스크톱 이름, 에이전트 2차 사용 후기 반영, 알림 배너와 Dock 그림자, 알림 센터 깜빡임, 입력기의 GNOME 셸 알림과 긴 알림 카드를 확인했다. 2026-10-30에는 창 최소화와 단축키 화면, 그리고 클라우드 세션 셋(bunt-comp 기본기, 화면 잠금과 로그아웃, 시스템 서비스 에이전트)을 합친 이미지를 확인했고, 오후에는 서비스 창의 검토 반영과 버스 고침, 한글 입력 e2e(4번)를 더 합쳐 확인했다. 2026-10-31에는 GPU 렌더 루프, 알려진 문제(작은 것) 고침, 단축키 화면의 남은 것(⇧⌘5 선택 영역, 최소화된 윈도우 애니메이션, 화면 기록)을 확인했다.

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
| 음량 막대 arm64 (10-28) | Mac 창에서 QMP와 직렬 콘솔로 확인했다.<br>- 음량 0, 2, 5, 10, 25, 50, 90, 100%에서 흰 채움이 막대 왼쪽 끝(둥근 끝)부터 손잡이 가운데까지 이어진다. 전에는 GTK 기본 테마가 남긴 1px 강조색 테두리가 채움을 둘렀고, 50% 밑에서는 채움 끝이 손잡이 앞에서 따로 둥글게 끝났으며, 10% 밑에서는 좁은 채움이 세로로 눌린 알약이 됐다<br>- 막대 높이가 22px다(전에는 기본 테마의 테두리로 24px)<br>- 막대의 x=1100을 누르면 음량 44%, 손잡이 가운데가 그 자리에 온다. 1180까지 끌면 78%, 손잡이가 포인터와 같이 움직인다<br>- Tab으로 막대에 오면 막대에 붙은 2px 강조색 고리가 보이고 3초 뒤 사라진다. 마우스로만 열고 누를 때는 고리가 없다<br>- 다크 모드, 메뉴 막대의 사운드 메뉴도 같다. 셸의 GTK 경고 0 |
| 에이전트 읽기 경로 arm64 (10-29) | Mac 창(QEMU cocoa, HVF)에서 QMP와 직렬 콘솔로 확인했다.<br>- at-spi2-core를 넣자 GTK 앱이 접근성 버스에 붙는다. `buntd-agent.socket`에 처음 붙을 때 데몬이 뜬다<br>- `bunt agent apps`: 셸, 시스템 설정, 텍스트 편집기, 포털과 그 창, 활성 창<br>- 시스템 설정: 첫 스냅샷 16ms(객체 121개), 이후 3~5ms. 화면과 같은 사이드바 15항목, 뒤로·앞으로(사용할 수 없음), 창 단추, 페이지 글이 28줄로 나온다<br>- 키보드 페이지: 막대 두 개(값과 범위), 시험 칸, 입력 소스 행과 지우기 단추, 스위치(켬). 스위치를 끄면 `--diff`가 그 한 줄만 바꾼다. `--ref`로 입력 소스 목록만 읽는다. 없는 ref는 오류다<br>- 셸: 메뉴 막대(로고, 앱 메뉴, 입력 소스, 사운드, 전원, 네트워크, 제어 센터, 시계)와 Dock 10개. 제어 센터를 열면 diff에 `[expanded]`와 모듈, 음소거 단추, 음량 막대가 나온다<br>- 텍스트 편집기: 9ms, 10줄. 친 글이 diff에 `[value="hello agent"]`로 나온다<br>- 파일(Nautilus): 11ms, 객체 139개가 `-i`로 24줄. 파일 칸(`cell "Downloads. 폴더"`), 도구 막대 단추, 가장자리 목록이 이름과 함께 나온다<br>- 처음에는 시스템 설정의 막대·스위치와 셸의 음량 막대·음소거 단추에 이름이 없었고, 열린 제어 센터는 메뉴 단추 이름이 안의 글을 모두 이어 붙인 것이었다. 고친 뒤 위와 같다<br>- 활성 창: 제어 센터를 닫은 뒤에도 GTK가 메뉴 막대 창을 활성으로 알려, 시스템 설정과 두 창이 활성이었다(키 입력은 시스템 설정으로 갔다). 컴포지터의 `GetFocus`로 고른 뒤에는 제어 센터가 열려 있으면 메뉴 막대 창을, 시스템 설정을 누르면 시스템 설정 창을 읽는다<br>- e2e 1번 통과(최종 이미지, 5.8G): 로그인 16초, 콘솔 셸 19초, 그래픽 셸(에이전트) 22초에 누를 수 있는 요소 20개, 전원 꺼짐 23초. 처음에는 메뉴 막대의 앱 메뉴(menubar)가 보일 때까지 기다렸는데, 앱이 없으면 앱 메뉴가 없어 대기 한도(149초, 327초)까지 기다린 뒤 요소 수로만 통과했다. 요소 수로 기다리게 고쳤다<br>- 메모리(RSS): buntd-agent 약 15MB(스냅샷 여러 번 뒤), at-spi-bus-launcher 약 18MB<br>- cargo fmt·clippy(경고 0)·test(에이전트 17개) 통과 |
| 에이전트 동작 경로 arm64 (10-29) | Mac 창(QEMU cocoa, HVF)과 시험 VM에서 직렬 콘솔과 QMP로 확인했다.<br>- 계산기: `launch`가 창과 버튼 34개의 트리를 낸다. 6 × 7 = 을 누르면 `=`의 diff에 기록 `6×7 = 42`와 `[value="42"]`가 나온다(멈추기까지 430~530ms). 다시 열면 새로 띄우지 않고 그 창을 가져온다<br>- 텍스트 편집기(DBusActivatable): `gio launch`로는 서비스 모드로만 떠 14초 뒤 끝났다. Activate를 직접 보내자 0.5~5초에 창이 뜬다. 계산기의 결과를 `fill`로 넣고 읽었다. 파일, 시스템 모니터도 같은 길로 뜬다<br>- 입력기를 한글 모드로 두고 `type 'hello world'`: 키로 칠 때는 "되ㅣㅐ재"였고, 글 입력으로 넣자 "hello world"다. "안녕하세요 buntOS 😀"도 그대로 들어간다. Firefox 주소 막대에 example.org를 쳐 자동 완성이 뜨고, 스냅샷이 `[value="example.org"]`를 읽는다<br>- 시스템 설정: 사이드바를 누르면 diff에 그 페이지가 나온다. 스위치, 막대, 글자 칸<br>- 접근 정책: 사용자 서비스로 띄운 시험용 AI(python 스크립트)는 거절되고 요청 기록에 남는다. 정책에 더하면 받는다. 계산기를 막으면 `apps`·`apps --installed`에서 빠지고 스냅샷·열기·막기 전에 받은 ref로 누르기가 모두 BLOCKED_APP이다. 직렬 콘솔의 `sudo -u buntos bunt …`는 관리자다<br>- 데몬 파일을 바꿔 넣으면 옛 데몬이 다음 연결 전에 끝나고 새 데몬이 받는다. 컴포지터는 다른 파일(inode)인 옛 데몬의 입력 요청을 거절했다<br>- 계산기 결과를 편집기에 넣는 시나리오 전체: 4.3초<br>- cargo clippy(경고 0)·test(에이전트 22개, 컴포지터 25개) 통과 |
| 통합 이미지 arm64 (10-29) | 세 갈래(계정과 로그인, AI 접근 정책과 인증 창, 에이전트 동작 경로)를 합친 이미지 5.8G(사용 5.4G), 빌드 2분 22초(캐시 있음).<br>- e2e 1번·5번 통과: 콘솔 로그인 19초, 셸 22초, 상태 `running`, 로그인 화면 27초, 로그인(1번째 시도) 34초, sudo가 암호를 묻는다, 그래픽 셸(에이전트) 41초에 요소 20개, e2e 5번 6개 통과 63초<br>- 처음 돌린 e2e 5번은 6개 중 5개였다. 컴포지터가 바쁜 VM에서 창 목록에 500ms 안에 답하지 못해 `--app org.gnome.TextEditor`가 앱을 찾지 못했다(답 제한을 2초로 늘렸다). GTK4가 GrabFocus를 지원하지 않아 `focus`가 실패했다(글 칸은 눌러 포커스를 준다). 세션을 다시 띄운 VM에서 첫 창이 늦은 계산기를 다시 열자 둘이 떴다(그 앱의 창을 기다리고, 막 연 앱은 다시 띄우지 않는다)<br>- 인증 창(pkexec)이 떠 있는 동안: `apps`와 `snapshot --app bunt-shell`에 인증 창이 없다. `snapshot`, `press`, `type`은 "an authentication dialog is open"으로 거절되고, `launch`는 컴포지터가 창 활성화를 거절한다. 인증 창은 그대로 남아 사람이 Esc로 닫았다<br>- 인증 창의 pkexec 설명에 `{$program}`이 그대로 보였다. polkitd(128)가 pkexec의 세부를 넘기지 않기 때문이다(`polkit.caller-pid`, `polkit.subject-pid`만 온다). polkit 설명에서 프로그램을 읽게 고친 뒤 "/usr/bin/true 프로그램을 관리자 권한으로 실행하려고 합니다."다<br>- 셸의 레이어 창이 접근성 이름을 갖는다(`bunt-menubar`, `bunt-dock`) |
| 키보드 단축키 arm64 (10-29) | 이미지 5.8G(사용 5.5G), 빌드 2분 17초(캐시 있음, curl 추가).<br>- e2e 1·5·6번 통과: 콘솔 로그인 15초, 셸 18초, 로그인 화면 24초, 로그인 31초, 그래픽 셸 41초. e2e 5번 6개, e2e 6번 11개 통과, 전원 꺼짐까지 77~91초.<br>- 시험 VM에서 새 컴포지터를 갈아 끼우고 QMP로 확인했다: ⌘A·⌘C·⌘→·⌘V("one twoone two"), ⌘Z, ⌥⌫, ⌘⇥, ⌘M과 ⌘⇥로 돌아오기, ⌘스페이스(모든 앱), 단축키 API 53개.<br>- 스크린샷은 게스트에서 꺼내 눈으로 봤다. ⇧⌘3은 1280×800 PNG(365KB)로 위아래가 바르고 커서가 없다. ⇧⌘4로 끈 영역은 600×370이다. 끄는 동안 반투명 회색 사각형에 흰 테두리가 보인다. 처음에는 위아래가 뒤집혀 나왔다(smithay의 `GlesMapping::flipped()`가 늘 true다).<br>- 터미널(foot): 편집기에서 ⌘C로 복사한 명령을 ⌥↩로 연 foot에 ⌘V로 붙여 실행했다(Ctrl+Shift+V). ⌘W로 foot을 닫으면 편집기가 포커스를 받는다.<br>- 사용자가 Mac 창에서 입력 소스 전환을 ⌃⇧로 바꾸려 했는데 아무 반응이 없었다(화면 기록). 시트와 컴포지터가 조합 키만으로 된 단축키를 몰라 안내 없이 버렸다. 입력 소스 전환에 ⌃⇧·⌥⇧(눌렀다 뗄 때)와 ⇧스페이스를 받게 하고, 다른 단축키는 까닭을 안내하게 고쳤다. 시험 VM에서 ⌃⇧로 영문↔한글이 바뀌고 ⌃⇧A로는 바뀌지 않는 것을 확인했다.<br>- 시스템 설정의 키보드 단축키 시트: 밝게·어둡게, 시트를 연 채 화면 모드 바꾸기(처음에는 시트 바탕이 그대로였다. 모든 시트가 부모 창을 따르게 고쳤다), 125%. 기록 칸에서 ⌘⇥를 누르면 앱이 바뀌지 않고 "'다음 앱으로 전환'에서 이미 쓰는 단축키입니다."가 뜬다(단축키 억제). ⌥C는 `comp-state.toml`에 저장되고, 기본값으로 복원하면 지워진다 |
| AI 접근 요청 창 arm64 (10-29) | 시험 VM에 새 셸과 시스템 설정을 갈아 끼우고, 허용하지 않은 가짜 AI(`systemd-run --user`로 띄운 `python3 fake-ai.py`, 루트 조상이 없어 관리자가 아니다)가 `bunt agent apps`를 부르게 해 QMP로 확인했다.<br>- 거절되면 1초 안에 경고창이 뜨고 요청 기록에 `asked`가 남는다. [시스템 설정 열기]를 누르면 AI 에이전트 페이지의 추가 시트가 그 AI를 고른 채로 열리고, [추가]를 누르면 인증 창이 암호를 묻는다. 시스템 설정이 이미 떠 있어도 그 창이 받는다.<br>- 처음에는 막 띄운 설정 창 위의 시트가 화면 왼쪽 위에 떴다. 컴포지터는 부모 창의 첫 버퍼가 온 뒤에야 부모 자리를 알기 때문이다. 창이 첫 화면을 그린 뒤에 시트를 열게 고쳤다. 한글 설명이 낱말 가운데에서 줄을 바꾸던 것도 Pango `allow_breaks`로 고쳤다.<br>- 시험 VM의 데몬은 이번 변경 전 이미지의 것이라 기록을 다시 쓸 때 `asked`를 지워, 물은 AI를 한 번 더 물었다. 새 데몬은 `asked`를 지킨다(단위 시험) |
| 에이전트 사용 후기 반영 arm64 (10-29) | 따로 빌드한 이미지로 e2e 1·5·6번 통과(103초). fill 다시 읽기, 글 칸이 아닌 곳의 type 거절(`--keys`), `press --ref --keep-focus`로 Tab 나아가기(계산기 C → ↑n → ↓n), `--settle`과 `settled`, 창 스크린샷과 창 기준 좌표 누르기(계산기 `=`, Firefox 캔버스), 허용한 AI가 있을 때 세션을 열며 접근성 켜기를 확인했다 |
| VM 디스크와 login 키링 arm64 (10-29) | 16G 시험 디스크에서 처음 켤 때 루트가 5.4G에서 15.4G로 늘었다(systemd-repart와 systemd-growfs-root 합쳐 0.1초 남짓). 로그인하면 login 키링이 열려 `secret-tool store/lookup`이 창 없이 된다. 새 이미지로 e2e를 두 번 돌려 둘 다 통과했다. 콘솔 로그인이 26~27초로 전(15초)보다 늦었는데, 그때 Mac이 바빴고(load average 10, 스왑 7.2GB) 모든 부팅 단계가 고르게 두 배쯤 느려 이 바뀜 때문은 아닌 것으로 본다(증명하지 못했다) |
| 합친 이미지 arm64 (10-29) | 키보드 단축키, AI 허용 목록 패턴, 에이전트 사용 후기 반영, VM 디스크와 키링, AI 접근 요청 창을 합쳐 빌드했다. 이미지 5.9G(사용 5.5G), 빌드 161초(캐시 있음). 작업 공간 전체 clippy 경고 0, 시험 215개 통과.<br>- e2e 1·5·6번 통과: 로그인 화면 19초, 로그인 22초, 그래픽 셸 25초(누를 수 있는 요소 20개), 세션을 열 때 접근성이 켜졌다. e2e 5번 13개(50초), e2e 6번 13개, 전원 꺼짐까지 96초.<br>- 처음 돌린 e2e는 5번의 Firefox 검사 3개가 실패했다. 주소 막대가 글 입력을 켜기 전에 type이 돌아 "no text field has the keyboard focus"로 거절됐고, 주소가 들어가지 않아 캔버스 검사까지 이어서 실패했다. 같은 이미지로 다시 돌리자 모두 통과했다. Firefox가 늦게 준비되는 흔들림으로 본다 |
| 세션 데스크톱 이름 arm64 (10-29) | 새 이미지에서 Claude 데스크톱 앱이 "시스템 키링을 사용할 수 없습니다"라고 알렸다. 앱 로그는 `[safeStorage] isEncryptionAvailable=false … (backend=basic_text)`였고, 로그인 토큰과 환경 변수를 남기지 못했다. 키링은 정상이었다(PAM에 pam_gnome_keyring, login.keyring 생성). 원인은 `XDG_CURRENT_DESKTOP=buntOS`를 Electron이 몰라 평문 저장소를 고른 것이다.<br>- `buntOS:GNOME`으로 바꿨다. 처음에는 greeter의 세션 환경만 고쳐 그대로였다. 세션 스크립트(`/usr/libexec/buntos/session`)가 다시 덮어썼기 때문이라 그것도 고쳤다.<br>- 시험 VM에서 컴포지터, 셸, 사용자 서비스 관리자, 에이전트가 연 계산기가 모두 `buntOS:GNOME`을 받는다. 포털은 그대로 xdg-desktop-portal-gtk이고 Settings·Secret·FileChooser가 답한다.<br>- Debian Chromium 156으로 비교했다. 그것은 `buntOS`일 때도 Secret 포털로 login 키링에 열쇠를 저장해 비교가 되지 않았다(Electron에는 이 길이 없다). Claude 앱으로 바뀐 것을 확인하는 일은 남았다.<br>- 빌드 중 Mac 여유 공간이 3.6GB까지 떨어져 빌드 VM 디스크가 I/O 오류를 냈다. 빌드 캐시의 debug 산출물(12G)을 지우고 `fstrim`하자 빌드 VM 디스크 파일이 31G에서 15G로 줄었다(루트가 discard로 마운트돼 있다) |
| 에이전트 2차 후기 반영 arm64 (10-29) | VM의 Claude Code가 남긴 후기 7가지와 스크롤을 시험 VM에 새 컴포지터·데몬·CLI를 갈아 끼워 확인한 뒤 이미지를 빌드했다.<br>- 휠: 처음 만든 `scroll down 5`는 Firefox에서 아무 일도 하지 않았다. WAYLAND_DEBUG로 보니 Firefox는 `enter` 뒤에 `axis_source`·`axis_discrete 5`·`axis 75`를 받고도 페이지가 그대로였고, 같은 자리에서 다시 돌리면(`motion` 뒤) `y=639`로 내려갔다. 컴포지터가 새 표면에 들어간 휠을 100ms 미루자 첫 휠에 내려간다. 파일 앱의 `/usr/bin` 표에서 `scroll down 10 --ref`의 diff에 바뀐 행이 나온다<br>- 화면 밖 단추: `-i` 스냅샷에 `[offscreen]` ref로 나오고, 누르면 ScrollTo로 보이게 한 뒤 접근성 동작 `press`로 누른다. 새로 띄운 Firefox에서는 처음에 휠 4번과 포인터로 눌렀다. 진단 로그를 넣어 보니 처음 GetExtents가 (-1, -1, -1, -1), 처음 NActions가 0이었고 100ms 뒤에는 제대로 답했다. 그 상태의 ScrollTo는 참을 돌려주고도 SHOWING을 고치지 않았다. 자리를 먼저 묻고 다시 묻게 하자 새 Firefox에서도 바로 된다<br>- 웹 페이지 글: 문단이 `text`·`link`·`text` 차례로 나오고 U+FFFC가 없다. 2.5초 늦게 답하는 페이지에서 Return이 2.9초를 기다려 그 페이지(`slow-loaded`)를 읽는다<br>- 시스템 정보: 파일시스템 탭에서 숨은 자원 페이지의 글(`CPU`)이 0줄이다. 목록(Cache.GetItems)에서 그 페이지는 자식 수 0을 알리는데 그 아래 객체가 모두 번호 0, VISIBLE·SHOWING으로 남아 있었다. 프로세스 목록(GtkTreeView)은 목록에 행이 없다<br>- 계산기 닫기는 `then the app quit (window "계산기" closed)`, `click <ref> --app 계산기`는 받고 다른 앱이면 `drop --app`이라고 알린다. foot은 `(the app does not support accessibility)`다<br>- 이미지 5.9G(사용 5.5G). e2e 1·5·6번 통과: 셸 23초, 로그인 화면 24초, 로그인 27초, 그래픽 셸 30초(요소 20개). e2e 5번 23개(79초), e2e 6번 13개, 전원 꺼짐까지 125초 |
| 알림 배너·Dock 그림자와 알림 센터 깜빡임 arm64 (10-29) | 사용자가 쓰던 VM의 디스크를 복제해 로그를 읽고, 사용자가 찍은 화면 기록 두 개를 프레임별로 나눠 본 뒤 시험 VM에서 재현했다.<br>- 배너: Claude 앱 알림(19:19:46)은 bunt-shell까지 왔고 배너 창도 생겼다(Claude 앱은 자기 창이 앞에 있으면 보내지 않는다). 시험 VM에서 같은 알림을 보내자 배너가 화면 너비 전체(1280×141)의 띠로 뜨고 내용은 왼쪽 위에 붙었다. 크기를 바꿀 수 있는 레이어 창이라 처음 받은 크기를 그대로 썼다. 크기를 내용에 맞추자 348px 카드가 메뉴 막대 아래 8px, 화면 오른쪽에서 10px에 뜨고, 알림 센터의 카드와 같은 x 922~1269에 놓인다<br>- 그림자: 배너는 카드가 창을 채워 그림자가 잘려 없었고, Dock은 창 여백 16px이 그림자(0 10px 28px)보다 좁아 옆에 세로 경계가 보였다. 여백을 넓히고 입력 영역을 판·카드로 줄였다(WAYLAND_DEBUG: Dock 698×106 창의 입력 영역 (28, 18, 642×72), 배너 368×104 창의 (10, 8, 348×84))<br>- 깜빡임: 알림 센터의 머리 줄과 카드 사이의 투명한 곳이 두 화면 버퍼에서 달랐고, 한쪽은 뒤 배경에 흐림의 채도 1.4를 한 번 더 건 색이었다. 화면이 연달아 그려질 때(목록 스크롤) 두 버퍼가 번갈아 보여 깜빡였다. 빈 알림 센터를 연 뒤 알림 8개를 받고 다시 열어 목록을 휠로 내리면 재현됐다(방해 금지로 배너를 꺼도 같다). 흐림에 로그를 넣어 보니 머리 줄은 팝업의 첫 두 프레임에만 그려졌고, 첫 프레임에서 흐림이 가림(팝업 텍스처)을 smithay보다 먼저 읽었다. smithay 0.7은 새 shm 텍스처에 축소 필터를 정하지 않고 그릴 때마다 정하므로, 그 프레임의 텍스처는 밉맵 없는 불완전한 텍스처라 알파가 1로 읽혀 흐림이 팝업 전체를 덮었다. 가림에 LINEAR를 정하자 같은 순서에서 스크롤하며 찍은 16장이 모두 같았다<br>- `qmp.py`에 휠(`wheel:down,N`)을 더했다<br>- 이미지 5.9G(사용 5.5G), 빌드 159초. e2e 1·5·6번 통과: 셸 19초, 로그인 24초, 그래픽 셸 27초, e2e 5번 23개, e2e 6번 13개, 전원 꺼짐까지 109초 |
| 입력기 알림과 긴 알림 카드 arm64 (10-29) | 시험 VM에 새 셸과 컴포지터를 갈아 끼우고 QMP로 확인했다.<br>- 입력기: 로그인하면 fcitx5가 "Wayland 진단: Input Method Panel GNOME Shell 확장을 설치하는 것을 권장합니다"를 띄웠다. fcitx5의 자가 진단은 데스크톱이 GNOME이면 UI가 kimpanel이 아닐 때 늘 이 알림을 낸다(Wayland 입력기가 돌아가도). 세션 이름을 `buntOS:GNOME`으로 바꾼 뒤부터다. 컴포지터가 fcitx5만 `XDG_CURRENT_DESKTOP=buntOS`로 띄우자 알림이 없고(fcitx5 환경에서 확인), e2e 5번의 한글 모드와 e2e 6번의 입력 소스 전환이 그대로 통과한다<br>- 감추기: 컴포지터가 fcitx5를 예전 명령(`buntOS:GNOME`)으로 다시 띄우게 해(`/etc/buntos/comp.toml`) 견줬다. 기본 설정 `/etc/xdg/fcitx5/conf/notifications.conf`가 없으면 진단 알림을 보내고, 있으면 보내지 않는다(dbus-monitor로 Notify를 셌다). fcitx5는 끝날 때 사용자 설정 파일을 쓰고 그 파일이 기본 설정보다 앞서므로, 견줄 때는 fcitx5를 SIGKILL로 끝내고 사용자 파일을 지웠다<br>- 긴 알림: 제목이 한 줄, 본문이 두 줄에서 잘리고 펼침 단추가 보인다. 펼치면 본문 5줄과 동작 단추가 나온다. 짧은 알림에는 펼침 단추가 없다. 처음에는 긴 글의 배너가 옆으로 늘었다. 줄바꿈하는 라벨의 자연 너비가 한 줄로 쓴 너비라 내용에 크기를 맞추는 배너 창이 따라 늘었다. 라벨의 자연 너비를 묶어 고쳤다. 또 줄 수를 두지 않는 값(-1)은 Pango가 말줄임하는 글을 한 줄로 잘라, 펼침 판단과 펼친 모습에 쓸 수 없었다<br>- 배너: 포인터를 올려 둔 채 7.6초가 지나도 남고, 포인터를 치우면 6.5초 안에 사라진다<br>- 이미지 5.9G(사용 5.5G), 빌드 155초. e2e 1·5·6번 통과: 셸 19초, 로그인 23초, 그래픽 셸 26초, e2e 5번 23개(한글 모드 포함), e2e 6번 13개(입력 소스 전환 포함), 전원 꺼짐까지 117초 |
| 창 최소화·앱 전환 창·스크린샷 화면 arm64 (10-30) | 시험 VM에 새 컴포지터와 셸(그리고 셸 설정 스키마, 아이콘, 앱 CSS)을 갈아 끼우고 에이전트와 QMP로 확인했다.<br>- 창 단추: 텍스트 편집기·계산기의 단추가 왼쪽에 닫기·최소화·확대로 나온다(접근성 이름 "닫기", "최소화", "최대화"). 앞에 있지 않은 창은 회색 원이다. 처음에는 단추가 창 끝에서 6px였고 뒤 창의 회색이 바탕과 거의 같아(`alpha(currentColor, 0.18)`) 안쪽 여백과 회색 값을 고쳤다<br>- 최소화 단추: 에이전트로 누르면 창이 내려가고 Dock 오른쪽 끝, 구분선 뒤에 창 그림과 앱 아이콘이 나온다. 그 칸을 누르면 창이 돌아오고 칸이 빠진다. 처음에는 창 그림이 제 크기(256px)로 그려져 Dock이 커졌다(gtk::Picture의 자연 크기). 아이콘 크기의 정사각형에 그리는 gtk::Image로 바꿨다. 그림을 a{sv}로 보내면 바이트마다 variant가 되어 답이 2.6초 걸려 a(ussuuay)로 바꿨다<br>- ⌘⇥: QMP로 ⌘를 따로 누른 채 ⇥를 누르면 화면 가운데에 앱 아이콘 줄이 뜨고 고른 앱의 칸이 칠해진다. ⌘를 떼면 그 앱으로 간다. 두 앱에서 ⇥를 두 번 누르면 제자리로 돈다. 처음에는 창이 뜨지 않았다: 셸이 전환 창 객체를 붙들지 않아 신호 처리기의 약한 참조가 비었다. 또 QMP의 send-key는 따로 누른 ⌘를 놓쳤다(7장)<br>- 스크린샷: ⇧⌘5 도구 막대가 Dock 위에 뜨고, 전체 화면 캡처 → 캡처를 누르면 막대가 사라진 뒤 찍혀(막대가 그림에 없다) 사진 폴더에 저장되고 오른쪽 아래에 미리 보기가 뜬다. ⇧⌘4 뒤 스페이스로 가리킨 창이 옅은 파랑으로 덮이고, 누르면 그 창만 700×520 RGBA PNG(색 형식 6)로 저장된다. 옵션 메뉴의 저장 위치·타이머·플로팅 썸네일 보기가 셸 설정을 따르고, 마지막으로 고른 방식으로 다시 열린다<br>- e2e: 처음 두 번은 e2e 5번의 Firefox 확인 9개가 실패해 e2e 6번이 돌지 못했다. 같은 이미지가 CPU 둘인 시험 VM에서는 23개가 다 통과해, 시험 VM을 CPU 둘로 바꿨다(7장). 다른 세션의 e2e와 함께 돌리자 mkosi가 같은 VM 이름(image)을 거절해, 이미지마다 이름을 달리 했다(7장)<br>- 이미지 5.9G(사용 5.5G), 빌드 196초. e2e 1·5·6번 통과(CPU 둘, 다른 세션의 시험 VM과 함께): 로그인 48초, 셸 50초, 그래픽 셸 58초, e2e 5번 23개(86초), e2e 6번 19개(140초, 새로 6개: 최소화 단추와 Dock의 최소화 칸, 되돌리기, 앱 전환 창, ⌘를 뗄 때의 전환, 스크린샷 미리 보기, ⇧⌘5 도구 막대), 전원 꺼짐까지 142초 |
| 합친 이미지 arm64 (10-30) | 창 최소화와 단축키 화면, 클라우드 세션 셋(bunt-comp 기본기, 세션, 시스템 서비스 에이전트)을 합쳐 커밋마다 clippy를 돌리고(39개), 작업 공간 시험과 이미지 빌드(3분 6초, 5.9G)를 했다.<br>- e2e 1·5·6·7번 통과: 로그인 18초, 로그인 화면 22초, 그래픽 셸 28초, e2e 5번 23개(91초), 6번 19개(145초), 7번 7개(165초), 전원 꺼짐 167초<br>- CPU 렌더러: 컴포지터 기록 `renderer: llvmpipe (LLVM 21.1.8)`, `apps: drawn with the CPU (GSK_RENDERER=cairo)`, linux-dmabuf 줄 없음. 셸, systemd 사용자 환경, 에이전트로 연 계산기·시스템 설정이 `GSK_RENDERER=cairo`. 시스템 설정 RSS 104~110MB, 셸 89MB, 컴포지터 206MB<br>- 앱 창 흐림: `glass-window --region=sidebar --toggle=4`가 `capabilities: blur`, `drew 480x320 (wl_shm)`을 적고 linux-dmabuf를 보지 못한다. 흐림을 켜면 사이드바 뒤의 계산기 단추가 흐려지고 끄면 또렷하다. 예제는 빌드 VM의 HTTP로 넣었다(직렬 콘솔로 0.7MB를 넣는 `vmctl.sh push`는 시간이 넘었다)<br>- 세션: 로그아웃 확인 창(60초 세기), 강제 종료 창(계산기), Esc로 닫기. 컴포지터 API `Lock`과 `loginctl lock-session`으로 잠기고 LockedHint가 yes가 된다. 잠긴 채 `pkill -KILL bunt-lock`이면 1초 안에 다시 뜨고, `loginctl unlock-session`으로는 풀리지 않는다. 잠금 화면(날짜, 큰 시각, 모노그램, 별칭, 암호 칸)에 QMP로 친 암호로 풀린다<br>- Wi-Fi 암호 창(4장 Wi-Fi 시험): NetworkManager에 `org.buntos.Shell` 에이전트가 등록된다. 메뉴 막대의 새 네트워크는 키보드를 받은 창으로 묻는다(제목이 낱말 단위로 줄 바뀜, 7자면 [연결] 꺼짐). 틀린 암호는 "암호가 올바르지 않습니다."로 다시 묻고, 맞는 암호로 연결되어 연결 하나가 남는다. 핫스팟 암호를 바꾸면 키보드를 가져가지 않는 창으로 다시 묻고(친 글은 계산기로 간다), 창을 눌러 친 새 암호가 저장된다. 창이 떠 있는 동안 `bunt agent apps`·`snapshot`에 창이 없고 `press`는 거절된다. 저널에 암호가 없다. 핫스팟을 첫 장치(wlan0)에 띄우면 메뉴 막대가 그 장치로 연결하려다 핫스팟을 끊고 실패한 새 연결을 지운다<br>- 시스템 설정: 사운드 페이지(경고음 "새 메시지", 경고음 음량, 출력 장치 "Virtio 1.0 sound (QEMU) Stereo" 내장, 출력 음량, 균형), 알림 페이지(미리 보기 "잠금 해제 시", 알림을 보낸 텍스트 편집기가 "배너, 사운드"로 오른다), 사이드바의 빨간 종·분홍 스피커 배지 |
| 검토 반영·버스 고침 arm64 (10-30 오후) | 시스템 서비스 에이전트의 검토 반영 셋과 버스가 없을 때 셸이 죽던 고침을 합쳐 커밋마다 clippy를 돌리고 작업 공간 시험(셸 71개)과 이미지 빌드(3분 11초)를 했다.<br>- Mac이 바쁠 때(부하 4~5, 스왑 8GB) e2e 5번의 Firefox 확인 여섯이 세 번 잇달아 실패했다. 시험 VM에서 보니 Firefox 주 프로세스가 CPU를 100% 쓰며 첫 창까지 20초 넘게, 첫 file:// 페이지까지 58초가 걸렸다. 셸과 컴포지터는 쉬고 있었다(20초에 0.15초, 0.35초). `tests/e2e/agent-apps.sh`가 창 제목과 빨간 칸, 누른 결과를 기다리게 고쳤다. main(`58bc6e2`)의 이미지와 견주려고 main 체크아웃에서 빌드했지만, 그 트리의 소스가 공유 cargo 대상 디렉터리의 산출물보다 오래되어 cargo가 아무것도 다시 빌드하지 않고 앞 빌드의 바이너리를 넣었다(빌드 기록에 `Compiling bunt-…` 줄이 없었다). 그래서 그 비교는 쓸 수 없다<br>- 고친 뒤 e2e 1·5·6·7번 통과: 로그인 15초, 그래픽 셸 25초, e2e 5번 23개(155초), 6번 19개(208초), 7번 7개(228초), 전원 꺼짐 229초<br>- Wi-Fi 암호 창(4장 Wi-Fi 시험 6번): 세션의 터미널(foot)에서 `nmcli dev wifi connect buntOS-Lab`을 하면 창이 뜨되 키보드를 가져가지 않는다(친 "12"는 터미널로 간다). [취소]는 nmcli에 "Secrets were required, but not provided"(종료 4)로 돌아가고, nmcli가 만든 연결은 남는다. 메뉴 막대에서 고른 네트워크는 창이 키보드를 받아 누르지 않고 친 암호로 연결된다. 저널에 암호가 없다 |
| 한글 입력 e2e arm64 (10-30 오후) | 다른 AI가 만든 e2e 4번과 고침 다섯을 main에 옮겨 커밋마다 clippy를 돌리고, 작업 공간 시험(컴포지터 95개)과 이미지 빌드를 했다. 빌드 기록에서 자체 컴포넌트가 다시 컴파일된 것을 확인했다.<br>- e2e 1·5·6·4·7번을 한 부팅에서 통과: 로그인 19초, 그래픽 셸 19초, e2e 5번 23개(219초), 6번 19개(259초), 4번 25개(352초), 7번 7개(367초), 전원 꺼짐 369초. Mac이 바빠(스왑 8GB) e2e 5번이 실패해도 이어 가는 드라이버 사본으로 돌렸는데 5번도 모두 통과해, 드라이버 그대로 돌린 것과 같은 결과다<br>- 앞선 시도에서 찾은 것: 게스트에 스크립트를 넣다 콘솔 입력이 빠져 e2e 5번이 시작하지 못했다(두 번, `push`를 고쳤다). Firefox가 첫 창을 1분 넘게 띄우지 못했다(두 번, 기다리게 고쳤다)<br>- e2e 4번의 "글 입력 칸 밖에서 입력기가 키보드를 놓는다"가 실패했다. 합치기 전의 이미지(04시 빌드)에서도 같아 시험의 전제가 틀렸다. GTK4 앱(시스템 설정, 시스템 모니터)은 글 입력 칸 밖에서도 입력기를 켜 두고, 시스템 모니터의 목록에서 한글로 치면 검색 칸에 "안"이 들어간다. 그 자리는 포커스가 사이드바 항목으로 옮겨 갔는지 본다 |
| 잠자기·시스템 모니터 arm64 (10-30 밤) | 사람이 Mac 창에서 써 보고 알린 두 가지(잠자기 뒤 깨어나지 못함, 시스템 모니터에 GPU 없음)를 고쳐 이미지를 빌드했다(빌드 기록에 `Compiling bunt-shell`).<br>- 시험 VM: `systemd-detect-virt`는 kvm, `/sys/power/mem_sleep`은 `[s2idle]`뿐이다. `buntos-vm-sleep.service`가 `/run/systemd/sleep.conf.d/50-buntos-vm.conf`(`AllowSuspend=no` 등)를 놓고, logind의 CanSuspend는 root와 buntos 모두 `no`, `systemctl suspend`는 "Sleep verb 'suspend' is disabled by config"로 거절된다<br>- 로고 메뉴: 잠자기만 흐리고 나머지 항목은 그대로다(QMP 화면). 셸이 D-Bus로 내보내는 `org.gtk.Actions`의 `Describe sleep`이 false, `lock-screen`이 true다. 열린 메뉴의 접근성 트리에는 항목이 이름 없이 `menuitem`으로 나오고 넷째(잠자기)에 `[disabled]`가 붙는다<br>- Resources: Dock(속도계 아이콘, 이름 "Resources")과 에이전트(`launch org.gnome.Resources`)로 연다. 사이드바는 Apps, Processes, Processor, Memory, GPU, 17 GB Drive, Ethernet Connection이고 영어다. GPU 페이지는 사용량·영상 부호기·복호기 0%, 영상 메모리 0 B, 클럭·온도·전력 N/A, PCI 슬롯 0000:00:05.0, 드라이버 virtio-pci다. 마지막에 본 페이지로 뜬다<br>- 셸 fmt·clippy(경고 0)·시험 72개, reuse lint 통과<br>- e2e 1·5·6·4·7번 통과: 로그인 15초, 그래픽 셸 20초, e2e 5번 24개(52초), 6번 19개(91초), 4번 25개(149초), 7번 8개(164초), 전원 꺼짐 166초. 첫 시도는 Mac이 바빠(부하 3.3, 스왑 7GB) e2e 5번의 Firefox 확인 8개가 잇달아 실패했다(새 시스템 모니터 확인 둘은 통과). 5번이 실패해도 이어 가는 드라이버 사본으로 다시 돌려 모두 통과해, 드라이버 그대로 돌린 것과 같은 결과다 |
| GPU 렌더 루프 arm64 (10-31) | 렌더 루프를 바뀐 것이 있을 때만 그리게 바꾸고, 앱 dmabuf 기다리기, 컴포지터 EGL 우선순위, 창 그림 영역만 되읽기, 폭 1픽셀 배경화면을 넣어 이미지를 빌드했다(빌드 기록에 `Compiling bunt-comp`). 컴포지터 시험 95개, fmt·clippy(경고 0) 통과.<br>- 가만히 둔 화면(로그인 40초 뒤 30초씩 두 번, 직렬 콘솔에서 bunt-comp 주 스레드의 `voluntary_ctxt_switches`와 `/proc/<pid>/stat`의 CPU 틱): 고치기 전 이미지 1702·1693번(1초에 56번), 틱 63·68(100Hz). 고친 이미지 0·8번, 틱 1·2<br>- QMP 화면: 로고 메뉴를 누르면 열리고 Esc로 닫힌다. 계산기 창이 뜨고, 최대화 단추를 누르고 1.5초 뒤 Dock이 끝까지 숨고 메뉴 막대가 검다<br>- virgl(`VMCTL_GPU=virgl VMCTL_RENDERNODE=/dev/null`): `renderer: virgl (LLVMPIPE (LLVM 19.1.7, 128 bits))`, `linux-dmabuf: main device renderD128, 48 formats`, `apps: GTK picks its GPU renderer`. 계산기는 `GSK_RENDERER` 없이 `renderD128`을 연다. 6×7 = 42, 창 그림(360×616)에서 찾은 '=' 자리를 눌러 42+8 = 50. 쉬는 화면 30초에 0번<br>- e2e 1·5·6·4·7번을 처음 시도에 통과: 로그인 15초, 그래픽 셸 21초, e2e 5번 24개(69초), 6번 19개(108초), 4번 25개(167초), 7번 8개(182초), 전원 꺼짐 183초 |
| 알려진 문제 고침 arm64 (10-31) | 클라우드 세션이 만든 다음 작업 1번의 고침 여섯을 main 위에 옮기고, 인수인계 메모의 VM 확인 항목을 돌리며 찾은 넷을 고쳤다. 커밋마다 clippy(경고 0), 끝에서 fmt와 작업 공간 시험(컴포지터 97개, 셸 79개, 에이전트 43개) 통과. 이미지 빌드 기록에 `Compiling bunt-shell`·`bunt-agent`·`bunt-comp`.<br>- 셸 단추: 로고를 누르면 포인터로 (17, 14)를 누르고 메뉴 항목 여덟이 이름을 갖는다(잠자기는 `[disabled]`). "이 시스템에 관하여"를 이름으로 골라 시스템 설정 정보가 뜬다. 시계는 알림 센터를 열고, Dock의 텍스트 편집기는 앱을 띄운다. 모든 앱 패널 검색 칸에 `type --ref`로 친 글이 들어가고 Esc가 글을 지운 뒤 패널을 닫는다. 앱 이름 메뉴("텍스트 편집기 정보", "텍스트 편집기 종료")와 AccountsService로 별칭을 바꾼 뒤의 로그아웃 항목("시험별칭 로그아웃…")도 이름이 있다<br>- 거절: Dock에 `press --keep-focus`("does not have the keyboard focus"), 자동으로 숨은 메뉴 막대("it is hidden"), 모든 앱 패널이 덮은 점, 관리자 인증 창(`pkexec`)이나 Wi-Fi 암호 창이 떠 있을 때("an authentication dialog is open", 스냅샷에도 없다), `loginctl lock-sessions` 뒤의 잠긴 화면<br>- Wi-Fi(hwsim 3개, 핫스팟을 wlan0에 권한 없이): 메뉴에 buntOS-Lab이 기타 네트워크로 나오고, 눌러 뜬 WPA2 암호 창에 QMP로 치자 wlan1이 연결되고 wlan0의 핫스팟은 그대로다. 유선이 주 연결이어도 buntOS-Lab이 맨 위에 한 번 나오고 아이콘은 유선, 제어 센터의 Wi-Fi 칸은 "buntOS-Lab"이다. `nmcli dev disconnect wlan1` 뒤에는 맨 위가 비고 알고 있는 네트워크로 간다(메모는 기타 네트워크를 바랐지만, 연결하며 저장한 infrastructure 프로필이 있어 알고 있는 네트워크가 맞다. 핫스팟 프로필 lab-ap(mode ap)은 빠진다). wlan1·wlan2를 관리하지 않으면 메뉴가 wlan0을 쓰되 핫스팟 이름을 연결된 네트워크로 보이지 않는다("찾은 네트워크 없음")<br>- 앱 항목(`nm-applet --indicator`를 apt로 넣었다): 메뉴를 열고 Wi-Fi를 껐다 켜도 `g_variant_get_variant`가 0줄이다. 고치기 전에는 에이전트가 단추를 누르면 메뉴 항목 하나(`sni.item-13`)를 실행했고, 고친 뒤에는 포인터로 눌러 메뉴를 열고 항목 일곱이 이름을 갖는다<br>- 버스: `systemctl restart polkit NetworkManager bluetooth` 뒤 셸 pid가 같고, NetworkManager에 셸의 비밀 에이전트가 다시 등록되고, 다시 띄운 polkit에서 인증 창이 뜬다. nm-applet을 끝내면 항목이 사라진다. `systemctl kill -s KILL dbus.service` 뒤 셸은 SIGTERM으로 끝나고(컴포지터 기록 "the shell ended (signal: 15 (SIGTERM))") panic 줄은 0이다<br>- 찾아 고친 것: 셸 표면과 키보드 포커스의 pid(socketpair의 SO_PEERCRED), 모든 앱 패널과 알림 배너의 레이어 이름, Esc로 닫은 메뉴 뒤 누르기가 전 자리로 가는 것(고치기 전 이미지에서 새 e2e 5번 확인이 실패했다), 앱 항목 단추의 접근성 동작(7장)<br>- 옛 이미지 저널에 셸의 GTK 경고 `gdk_wayland_toplevel_set_a11y_properties: assertion 'GDK_IS_WAYLAND_TOPLEVEL (toplevel)' failed`가 한 번 있었다(원인 미확인, 새 이미지에서는 없었다)<br>- e2e 1·5·6·4·7번 통과: 로그인 14초, 그래픽 셸 20초, e2e 5번 26개(105초), 6번 19개(144초), 4번 25개(202초), 7번 8개(218초), 전원 꺼짐 220초. 첫 시도는 Mac이 바빠(부하 3.5, 스왑 7GB) Firefox 주소 막대가 글 입력을 켜기 전에 `type`이 거절돼 e2e 5번의 Firefox 확인 8개가 잇달아 실패했다. 5번이 실패해도 이어 가는 드라이버 사본으로 다시 돌려 모두 통과해, 드라이버 그대로 돌린 것과 같은 결과다 |
| 단축키 화면의 남은 것 arm64 (10-31) | 클라우드 세션이 만든 다음 작업 3번의 고침 열하나를 main 위에 옮기고(텍스트 충돌은 렌더 루프 자리 하나), 렌더 루프와 합친 정리, 이미지 꾸러미(GStreamer, Showtime), 영어 이름, 시험 VM에서 찾은 ⇧⌘5 Return 고침을 더했다. 커밋마다 clippy(경고 0), 끝에서 fmt와 작업 공간 시험(컴포지터 132개, 셸 82개, 설정 59개) 통과. 화면 기록의 부호기 시험 10개는 빌드 VM에 GStreamer 1.26을 넣어 건너뛰지 않고 돌았다. 이미지 빌드 기록에 `Compiling bunt-comp`·`bunt-shell`·`bunt-settings`, 꾸러미 GStreamer 1.28.8과 Showtime 50.0.<br>- 선택 영역(⇧⌘5): 막대가 아래 가운데에 뜨고 선택 부분 캡처가 골라져, 화면 가운데 640×400 영역 둘레가 어둡고 흰 테두리와 손잡이 여덟이 보인다. 안쪽을 끌면 옮기고, 모서리를 끌면 늘고, 바깥을 끌면 새로 고른다. Return이 그 영역을 찍는다(201×151, QMP 좌표의 반올림으로 1픽셀 더해진다. 7장). 다음 ⇧⌘5는 마지막 영역에 띄운다. Esc는 막대와 영역을 함께 닫고 그 뒤 앱이 누르기와 키를 받는다. 옵션 메뉴는 영역 위에 뜨고 첫 Esc는 메뉴만 닫는다. 빨리 두 번 눌러 닫고 다시 열어도 영역이 뜨고, 막대를 연 채 잠갔다 풀면 막대가 닫혀 있다. 125%에서 띠와 테두리 사이에 틈이 없고 찍힌 PNG가 영역×1.25(251×189)다. ⇧⌘4는 그대로 끌었다 떼면 찍는다<br>- 최소화 효과: ⌘M의 지니 효과가 0.2초쯤에 창 아래쪽을 Dock 오른쪽 끝 칸으로 휘고, 끝나면 칸에 창 그림이 있다. 칸을 누르면 칸에서 나와 펴지고 바로 친 글(abc)이 창에 들어간다. ⌘M 직후(0.15초) 칸을 누르면 그 자리에서 돌아선다. ⌘H는 효과 없이 사라진다. 시스템 설정의 최소화된 윈도우 애니메이션(투명도와 자동으로 Dock 가리기 사이)에서 크기 조절 효과를 고르면 `minimize-effect`가 'scale'이고 창이 휘지 않고 준다. 최대화한 창(Dock이 숨음)은 Dock이 나온 뒤 칸으로 들어간다. 셰이더 실패 기록은 없다. ⌘M 한 번(효과와 화면 저장 1.5초)에 컴포지터 CPU 0.26초<br>- 화면 기록: 전체 화면 기록을 고르면 실행 단추가 "기록"이 되고, 누르면 메뉴 막대 오른쪽 맨 왼쪽에 기록 중단 단추가 뜬다(저널 `recording 1280x800 … of Virtual-1 to /home/buntos/Pictures/화면 기록 ….webm`). 멈추면 마지막 프레임의 미리 보기가 뜨고, 누르면 Showtime이 D-Bus로 떠서 재생한다(영상에 창과 커서가 있다). 파일은 VP8/WebM 1280×800이고 끝까지 풀리며, 65초 기록이 1968프레임이다. 선택 부분 기록은 고친 영역 크기(320×230)다. 기록 중 ⌃⌘Q로 잠그면 부호기가 끝나고 파일이 마무리되며 풀면 중단 단추가 없다. 썸네일을 끄면 "화면 기록을 저장했습니다: …" 알림이 뜬다. 기록 중 배율을 125%로 바꿔도 이어지고 마지막 프레임이 125% 화면이다. `gst-launch-1.0`을 치우면 "화면을 기록할 수 없습니다. 화면 기록 도구(GStreamer)가 설치되어 있지 않습니다." 알림이 뜨고 빈 파일이 없다. 부호기의 환경은 `GST_PLUGIN_SYSTEM_PATH_1_0`, `GST_REGISTRY_1_0`, `HOME`, `XDG_RUNTIME_DIR`뿐이고, fd는 0(프레임 파이프), 1(/dev/null), 2(저널), 3(기록 파일)과 자신의 socketpair 셋, eventfd 둘이다(DRM 장치와 컴포지터 소켓이 없다). 거의 그대로인 1280×800을 기록하는 동안 3초에 컴포지터 0.34초, 부호기 0.93초의 CPU를 썼다(vCPU 2개, llvmpipe)<br>- 확인하지 못한 것: 영역 위의 포인터 모양(커서 평면은 QMP 화면 저장에 없다), 화면 둘, 하드웨어 GPU에서의 효과와 기록<br>- e2e 1·5·6·4·7번을 처음 시도에 통과: 로그인 15초, 그래픽 셸 21초, e2e 5번 26개(54초), 6번 19개(92초), 4번 25개(148초), 7번 8개(163초), 전원 꺼짐 164초. Return 고침 전 이미지도 같았다(165초) |
| x86-64 그래픽 세션 CI (10-30) | `ubuntu-26.04` 호스티드 러너(2코어, KVM)에서 자체 컴포넌트를 넣은 x86_64-generic 이미지를 빌드하고 e2e 1·5·6·4·7번을 한 부팅에서 통과했다(클라우드 세션의 실행 2·3).<br>- e2e(실행 2): 콘솔 로그인 9초, 셸 11초, 상태 `running`, 로그인 화면 11초, 로그인 14초(1번째), 그래픽 셸 14초(요소 20개), e2e 5번 24개(50초), 6번 19개(88초), 4번 25개(143초), 7번 8개(158초), 전원 꺼짐 159초<br>- 컴포지터: `using GPU /dev/dri/card0`, `renderer: llvmpipe (LLVM 21.1.8, 256 bits)`, `apps: drawn with the CPU (GSK_RENDERER=cairo)`. arm64와 같은 CPU 경로다<br>- 작업 시간: 캐시가 있을 때 약 15분(실행 3: 14분 31초), 첫 실행(캐시 없음)은 약 30분<br>- 첫 실행은 이미지 빌드의 `systemctl preset-all`에서 실패했다(7장). 그 뒤로 빌드 VM과 같은 Debian 13 도구 트리로 만든다 |
| Agent 3단계·x86-64 CI 합침 arm64 (10-31) | 클라우드 세션 두 갈래(다음 작업 4번 9개, 2번 24개)를 main 위에 옮기고(텍스트 충돌은 에이전트 데몬의 시험 모듈 끝 하나), 워크플로를 손으로만 돌리게 하고, e2e 5번에 REPL 확인 둘을 더했다. 커밋마다 clippy(경고 0), 끝에서 fmt와 작업 공간 시험(에이전트 100개, 컴포지터 132개, 셸 82개) 통과.<br>- 4번 로컬 확인: 새 환경 변수를 주지 않은 arm64 빌드와 e2e가 전과 같다. `E2E_LOG_DIR`을 주면 aarch64에서도 `console.log`(786줄)와 `journal.log`(3357줄, virtio 직렬 포트 `org.buntos.e2e.journal`)가 남는다<br>- REPL: 계산기 6 × 7을 호출 한 번으로 42(2.3초, 앱 열기 포함), 다음 호출의 `calc.wait({ text: "42" })`가 4ms에 두 요소를 찾는다<br>- wait: `=` 단추 8ms, 없는 글 `4242`는 500ms 뒤 시간 초과(종료 1), 트리 없는 foot에서 `--gone`은 2초 뒤 시간 초과와 "has no accessibility tree"(사라졌다고 하지 않는다)<br>- 워커: `Seccomp: 2`, `NoNewPrivs: 1`, fd 0·1·2(파이프), 파일 크기·코어·프로세스 상한 0, 주소 공간 1073741824, RSS 4.8MB, 저널에 `REPL worker:` 줄 없음<br>- MCP: 허용 목록에 더한 시험 클라이언트로 `protocol 2025-06-18 buntos`, `tools ['repl', 'skills']`, `text: 42`, 창 그림 줄과 `image: image/png True 360 616`, `isError False`, skills 목록. 더하기 전에는 거절하고 AI 접근 요청 창("“mcp-check.py”이(가) 이 컴퓨터를 제어하려고 합니다.")이 뜬다(7장)<br>- 막은 앱: 떠 있는 계산기를 막으면 스냅샷이 `BlockedAppError`(종료 1), 앱 목록에서 빠지고 풀면 돌아온다. MCP도 `BlockedAppError`, `isError True`<br>- 허용하지 않은 AI(`systemd-run --user`로 띄운 MCP): JS를 돌리기 전에 거절하고 워커가 늘지 않는다(2→2). 이미 물었으면 "The user was already asked"다. 허용 목록에 더하면 `text: 2`<br>- 잠금: `ScreenLockedError: the screen is locked; only the user can unlock it with their password`(종료 1)<br>- 긴 웹 페이지(3000줄): 스냅샷이 2000줄에서 잘린 채 getByRole로 끝의 End 단추를 누르고 "Done" 창을 확인한다(85초, `--timeout 120000`. 기본 30초는 넘었다, 7장)<br>- 앱 종료: 계산기를 ⌃Q로 끝내면 그 ref를 기다리던 것이 36ms에 "is gone"으로 끝난다. 텍스트 편집기를 막아 둔 채로도 같다(46ms)<br>- 확인하지 못한 것: Claude 데스크톱 앱에 MCP 서버로 더해 사람이 쓰는 것<br>- e2e 1·5·6·4·7번: 합친 이미지에서 처음 시도에 통과(로그인 15초, 그래픽 셸 22초, e2e 5번 26개(73초), 6번 19개(111초), 4번 25개(172초), 7번 8개(187초), 전원 꺼짐 190초). REPL 확인을 더한 뒤에는 처음에 e2e 5번 끝에 두어 6번의 ⌘⇥ 셋이 실패했고(7장), 계산기를 닫기 전으로 옮기자 앞 확인이 눌러 둔 "1" 때문에 112를 읽어 C로 비우게 했다. 그 사이 한 번은 Mac이 바빠(부하 2.9, 스왑 6.7GB) Firefox 확인 8개가 잇달아 실패했다. 마지막 판은 처음 시도에 통과: 로그인 14초, 그래픽 셸 19초, e2e 5번 28개(55초), 6번 19개(93초), 4번 25개(148초), 7번 8개(163초), 전원 꺼짐 164초 |
| 확인하지 않은 것 | 사용자가 쓰는 Mac 창 VM에서의 새 배너와 깜빡임 수정(시험 VM에서만 봤다). GTK3 앱(Firefox)의 신호등과 foot·Chromium 계열이 그리는 창 단추. 실제 GPU에서의 창 그림 뜨기와 앱 전환 창이 뜨는 데 걸리는 시간(시험 VM이 바빠 처음 뜰 때 1초 넘게 걸렸다). 타이머(5초, 10초)와 데스크탑·문서·클립보드 저장 위치. 미리 보기를 끌어서 다른 앱에 놓기. 밝게만 봤다(어둡게의 앱 전환 창, 도구 막대, 회색 신호등). Chromium 계열의 접근성 트리. Chromium·Electron 앱에서의 화면 밖 요소와 휠 스크롤(시험은 Firefox와 GTK4 앱으로 했다). Mac 창에 보이는 커서의 모양과 자리(QMP 화면 캡처에는 커서 평면이 나오지 않는다). 실제 USB·Bluetooth 마우스의 이름·연결 방식과 이동 속도·자연스러운 스크롤·보조 클릭(QEMU에는 가상 태블릿뿐이고 절대 좌표 장치에는 가속이 없다). `tools/devvm/buntos-dev.yaml`의 컨테이너 런타임 끄기 설정(검증한 VM은 그 설정을 넣기 전에 만들었다). Bluetooth 연결·페어링, 배터리 표시, 화면 밝기, 실제 Wi-Fi 칩(QEMU에 장치가 없다). 실제 앱이 보낸 알림(시험 알림만 썼다)과 nm-applet 밖의 실제 앱 항목(10-31에 nm-applet으로 봤다). 분수 배율의 실제 HiDPI 화면(QEMU 화면은 약 100ppi라 기본 배율은 늘 100%다). Mac 창(view-mac.sh)에서 사람이 조작한 배율 변경. 클립보드로 낸 스크린샷(⌃⇧⌘3·⌃⇧⌘4)을 붙여넣는 앱(이미지를 받는 앱이 이미지에 없다). Mac 창의 `--full-grab`(QEMU에 손쉬운 사용 권한이 필요해 사람이 확인한다)과 실제 Mac·PC 키보드의 ⌘. Firefox에서의 ⌘ 단축키와 글 입력 판단(시험은 텍스트 편집기, 시스템 설정, foot으로 했다). 여러 화면과 회전한 화면의 스크린샷. 합친 이미지(10-30)에서 보지 않은 것: GPU 경로(virgl `VMCTL_GPU=virgl`, 하드웨어 GPU), 흐림 위에 겹친 불투명한 창과 창을 옮길 때의 컴포지터 CPU, 화면 둘, Bluetooth 페어링(QEMU에 장치가 없다. Bumble 가상 컨트롤러로 볼 수 있다), 잠자기 전후의 잠금, 5번 틀린 뒤 기다리기, 저장하지 않은 앱이 있을 때의 로그아웃 취소, 사운드의 균형과 입력 레벨, 앱별 알림 시트의 설정 |

## 7. 알려진 문제

| 문제 | 원인 | 대처 |
|---|---|---|
| 첫 빌드가 `/var/cache/mkosi` 권한 오류로 실패 | mkosi는 일반 사용자로 실행해도 이 경로에 패키지 캐시를 둔다 | `setup.sh`가 사용자 소유로 만든다 |
| "systemd-boot binary was not found" 빌드 오류 | Debian은 systemd-boot의 EFI 파일을 `systemd-boot-efi` 패키지로 나눈다 | 레시피 패키지 목록에 넣었다 |
| 자동 로그인 직후 로그인이 반복되며 셸이 뜨지 않음 | Debian에서 `login`은 Priority: required지만 Essential이 아니어서 mkosi가 설치하지 않는다. `apt`, `passwd`도 같다 | 레시피 패키지 목록에 넣었다 |
| 부팅이 시간대 입력 화면에서 멈춤 | systemd-firstboot의 대화형 설정 | 레시피에 로캘·시간대·키맵·호스트 이름을 정하고 `systemd.firstboot=no`로 끈다. 계정은 처음 켤 때 로그인 화면이 만든다(2026-10-29) |
| `/dev/tpm0`, `/dev/tpmrm0`를 기다리다 90초씩 지연 (arm64) | mkosi가 붙이는 에뮬레이션 TPM을 게스트 커널이 장치로 잡지 못했다(원인 미확인) | 레시피에서 `TPM=no`로 끈다. 원인은 따로 확인한다 |
| `--console=read-only`, `interactive`를 쓸 수 없음 | Debian 13 호스트에 `systemd-pty-forward`가 없다 | 기본(native) 콘솔을 쓴다 |
| 홈 디렉터리가 root 소유라 셸 설정을 읽지 못함 | 이미지를 비특권으로 빌드하면 파일 소유자를 바꿀 수 없다 | 2026-10-29부터 계정은 부팅한 시스템에서 useradd가 만들므로 홈 폴더 소유자가 맞다 (그전에는 tmpfiles로 만들었다) |
| 시계가 UTC로 나옴 | `tzdata`도 Priority: required지만 Essential이 아니어서 설치되지 않았다 | 레시피 패키지 목록에 넣었다 |
| 화면 장치가 없는 VM에서 `getty@tty1` 실패 | 그래픽 세션이 바로 끝나며 자동 로그인이 반복됐다 | 2026-10-29부터 tty1에는 텍스트 로그인 대신 greetd가 있다. 화면 장치가 없으면 로그인 화면의 컴포지터가 끝나고 greetd가 다시 띄운다 |
| GTK 앱이 CPU로 그린다 (GPU가 없는 VM) | 컴포지터가 CPU 렌더러(llvmpipe)로 그린다. GTK는 GPU가 없어도 같은 소프트웨어 래스터라이저로 GL·Vulkan을 써 메모리를 몇 배 쓴다(아래 직렬 콘솔 행) | 컴포지터가 자기 렌더러를 보고 정한다(`comp/src/gpu.rs`). CPU 렌더러면 띄우는 프로그램과 D-Bus 활성화·systemd 사용자 환경에 `GSK_RENDERER=cairo`를 주고 linux-dmabuf를 열지 않는다. GPU(virgl 포함)면 linux-dmabuf v5를 열고 GTK가 GL이나 Vulkan으로 그려 dmabuf로 보낸다. 컴포지터 기록의 `renderer:`, `apps:`, `linux-dmabuf:` 줄로 본다. 세션이 정한 `GSK_RENDERER`는 그대로 둔다 |
| 배율을 높이면 Firefox 본문이 한 번 더 확대돼 화면 밖으로 잘림 | Firefox는 본문을 하위 표면에 그리고 그 크기를 wp_viewporter로 알린다. 컴포지터에 viewporter가 없어 2배로 그린 본문을 1배 버퍼로 받아 다시 2배로 키웠다. 또 1280×800에서 200%를 허용해 논리 화면(640×400)이 Dock과 앱의 최소 크기보다 작았다 | wp_viewporter, wp_fractional_scale_v1, wl_compositor v6를 넣었다. 논리 화면이 1024×640보다 작아지는 배율은 두지 않는다. 배율이 바뀌면 창을 화면 안에 맞춘다([디자인 스펙](../design/DESIGN-SPEC.md) 3.1절) |
| 창에 최소화 단추가 없음 | 컴포지터가 창 관리 기능(xdg_toplevel wm_capabilities)에 최소화를 알리지 않아 GTK가 단추를 숨겼고, 이미지의 창 단추 설정도 `appmenu:maximize,close`였다 | 2026-10-30부터 최소화를 알리고 앱의 최소화 요청을 받는다. 창 단추는 macOS처럼 왼쪽에 `close,minimize,maximize:`다. 최소화한 창은 Dock의 최소화 칸에 놓인다 |
| QMP로 ⌘를 누른 채 ⇥를 거듭 누를 수 없음(앱 전환 창이 뜨지 않음) | QMP의 send-key(`keys:`)는 키를 함께 눌렀다 뗀다. input-send-event로 누른 ⌘ 다음에 send-key를 섞으면 컴포지터가 ⌘를 받지 못했다 | `qmp.py`의 `down:키`·`up:키`(input-send-event)로 ⌘와 ⇥를 모두 따로 누르고 뗀다 |
| e2e 5번의 Firefox 확인 9개가 연달아 실패("no app matches firefox-esr"), 다시 돌려도 같음 | mkosi `vm`은 기본으로 CPU 하나로 띄운다. Mac이 바쁠 때(다른 세션의 시험 VM, 스왑 7GB) 처음 켠 Firefox가 1분 안에 창을 띄우지 못해, 에이전트의 `launch`(20초씩 세 번)가 창을 받지 못했다. 같은 이미지를 CPU 둘인 `vmctl.sh` VM에서 돌리면 23개가 다 통과했다 | 시험 VM은 CPU 둘로 띄운다(`image/mkosi.conf`의 `[Runtime] CPUs=2`, 2026-10-30) |
| 다른 세션이 다른 이미지(`BUNTOS_IMAGE_DIR`)로 e2e를 돌리는 동안 e2e가 바로 끝남("디스크에 임시 덮어쓰기를 씌우지 못했다", 콘솔에는 "Another virtual machine named image is already running") | mkosi `vm`은 VM 이름을 이미지 이름(image)으로 정하고, 같은 이름의 VM은 하나만 띄운다 | `tools/image/build.sh`가 `vm`에 이미지 디렉터리마다 다른 이름(`buntos-<디렉터리>`)을 준다(2026-10-30) |
| 셸의 새 창(앱 전환 창, 도구 막대)이 빌드 VM의 시험 VM에서 처음 뜰 때 1초 넘게 걸림 | Mac이 바쁠 때(스왑 7GB) 시험 VM이 느리다. 처음 띄우는 레이어 창은 GTK가 CSS와 아이콘을 읽어 그린다 | e2e는 셸 창이 뜰 때까지 20초까지 기다린다(`tests/e2e/shortcuts.sh`의 `shell_window`) |
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
| 저장된 Wi-Fi 연결의 암호가 바뀌면 다시 묻지 못함 | NetworkManager 비밀 에이전트가 없다 | 셸이 비밀 에이전트가 되어 Wi-Fi 암호 창으로 다시 묻는다(`shell/src/secret_agent.rs`). 메뉴 막대는 새 네트워크도 연결을 만들어 켜고 암호를 이 창에 맡긴다. 메뉴 막대와 시스템 설정은 창에서 취소하거나 연결하지 못한 새 연결을 지운다. 2026-10-30에 mac80211_hwsim 핫스팟으로 확인했다(4장 Wi-Fi 시험, 6장) |
| PIN이나 확인 번호가 필요한 Bluetooth 기기(키보드 등)를 짝지을 수 없음 | BlueZ 페어링 에이전트가 없다 | 셸이 세션의 기본 페어링 에이전트(KeyboardDisplay)가 되어 Bluetooth 페어링 창으로 묻는다(`shell/src/pairing_agent.rs`). 시스템 설정의 Bluetooth 페이지와 메뉴 막대의 근처 기기가 이 에이전트로 짝짓는다. QEMU에 Bluetooth 장치가 없어 VM에서 아직 확인하지 않았다 |
| 빌드 VM에서 bunt-comp의 dmabuf 가림 시험 셋이 실패 | 빌드 VM(Debian 13)의 Mesa 25.0.7 llvmpipe는 선형 dmabuf 형식을 모두 외부 텍스처로만 알리고(render formats에 없다) NV12는 알리기만 하고 가져오지 못한다. 시험은 보통 텍스처를 전제로 했다(Mesa 25.2에서는 통과) | 2026-10-30부터 전제가 맞지 않으면 "skipped"를 찍고 지나간다. 이미지(forky)의 Mesa는 26.1이다 |
| `cargo test --workspace`에서만 dmabuf 평면 시험이 실패 | 작업 공간을 함께 빌드하면 다른 크레이트가 wayland-backend의 `client_system`을 켜 시험 클라이언트가 libwayland로 돈다. libwayland는 이미 지운 객체의 프로토콜 오류를 인터페이스 이름 없이 `@0`으로 알린다 | 2026-10-30부터 시험이 오류 코드만 본다 |
| e2e가 "e2e 5번이 끝나지 않았다"로 끝나고 콘솔에 `-bash: pps.sh: command not found`가 되풀이됨 | e2e의 `push`는 게스트를 기다리지 않고 base64 줄(1000자)을 잇달아 보낸다. 게스트가 바쁘면 콘솔 입력이 빠져 한 줄이 다음 명령과 섞였다(2026-10-30, Mac 스왑 8GB) | 2026-10-30부터 `push`가 줄마다 게스트의 답을 기다리고, 다 넣은 뒤 sha256이 다르면 다시 넣는다 |
| 바쁜 Mac에서 e2e 5번의 Firefox 확인이 잇달아 실패("no app matches firefox-esr", 페이지 글 없음) | Mac 스왑이 8GB일 때 시험 VM의 Firefox가 첫 창까지 1분, 첫 file:// 페이지까지 1분 가까이 걸렸다 | 2026-10-30부터 Firefox가 늦게 뜨면 다시 띄우지 않고 창을 기다리고, 페이지와 누른 결과를 90초까지 기다린다. e2e 전체 제한 시간은 1200초 |
| GTK4 앱에서 글 입력 칸 밖에 포커스가 있어도 입력기(fcitx5 DebugInfo `focus:1`)가 켜져 있음 | GTK4가 창의 글 입력을 켜 둔다 | 바른 동작이다. 시스템 모니터의 목록에서 한글로 치면 검색 칸이 열려 한글이 들어간다. e2e 4번은 입력기가 아니라 포커스로 본다 |
| 큰 파일을 `vmctl.sh push`로 넣으면 시간이 넘고 직렬 콘솔이 `>`(이어 쓰기)에 멈춤 | 직렬 콘솔로 base64 줄을 하나씩 보낸다. 0.7MB 실행 파일은 120초 안에 다 가지 않았다 | 콘솔은 `printf '\003\n' > ~/buntos-build/vm-<ID>/console.in`으로 풀린다. 큰 파일은 빌드 VM에서 `python3 -m http.server --bind 127.0.0.1`로 내고 게스트에서 `curl http://10.0.2.2:<포트>/…`로 받는다 |
| `gdk_pixbuf::Pixbuf::from_file`이 그림을 못 읽음 | Debian forky의 gdk-pixbuf는 그림 형식 로더 없이 glycin에 맡긴다 | 그림은 `gdk::Texture::from_file`(GTK)로 읽는다 |
| fcitx5의 키 반복이 바뀐 값을 바로 따르지 않음 | fcitx5는 키보드를 잡을 때 받은 반복 값으로 반복을 직접 흉내 낸다 | 이미 포커스된 입력 칸은 다음 포커스부터 새 값을 쓴다 |
| 필수 펌웨어를 넣은 뒤 부팅이 30초 넘게 늦어짐 | mkosi는 UKI에 모든 커널 모듈과 그 모듈이 쓰는 펌웨어를 담은 initrd를 붙인다. 펌웨어 패키지를 넣자 initrd가 220MB가 됐다 | 커널 모듈 initrd에는 루트를 여는 모듈만 넣는다(`image/mkosi.conf`의 `KernelModulesInitrdInclude`). 나머지는 루트로 바꾼 뒤 읽는다. UKI 268MB→87MB, 로그인 57초→19초 |
| 메뉴 막대 메뉴를 열면 포인터가 메뉴 밖에 있어도 맨 아래 항목이 가리킨 것처럼 칠해짐 | 셸이 팝오버가 뜬 뒤(show)에 내용을 채워 크기가 바뀌고 다시 배치됐고, GTK가 메뉴가 뜰 때 포인터가 메뉴 위에 있다고 보고 hover를 줬다. 컴포지터는 팝업에 포인터 이벤트를 보내지 않았다(WAYLAND_DEBUG로 확인) | 메뉴 단추의 create-popup에서 띄우기 전에 채우고, 가리킨 줄은 포인터가 메뉴 안에서 실제로 움직인 뒤에만 칠한다(`shell/src/extras.rs`의 pointer-moved) |
| 셸에서 메뉴 막대·Dock을 숨기면 막대가 다시 그려지지 않음 | 셸이 창을 최대화한 직후 막대를 숨기면(CSS 클래스와 입력 영역) gtk4-layer-shell 표면이 커밋을 멈추고, 포인터가 막대 자리에 들어와야 다시 그렸다. GTK 쪽 원인은 찾지 못했다 | macOS의 WindowServer처럼 컴포지터가 막대를 숨기고 꺼낸다(`comp/src/panels.rs`). 셸은 막대 표면을 그대로 둔다 |
| 빌드 VM 안의 시험 VM에서 애니메이션 시간을 잴 수 없음 | 화면 없이 띄운 QEMU가 vblank(플립 완료)를 고르지 않게, 몇 초씩 늦게 준다 | 애니메이션과 타이밍은 Mac 창(QEMU cocoa, HVF)에서 확인한다 |
| 시스템 설정에서 누르기가 먹지 않음(가리키면 칠해지기는 한다). Esc를 누르면 풀린다 | 시트(모달 창, 예: 입력 소스 추가)를 띄운 채 부모 창을 누르면 컴포지터가 부모를 시트 위로 올려 시트가 뒤에 숨었다. GTK는 모달 창이 떠 있는 동안 부모의 누르기를 버리고, Esc는 숨은 시트에 가서 시트를 닫았다(QMP로 재현) | 부모가 있는 창(대화 상자)은 부모와 함께 앞으로 와서 부모 위에 남는다. 모달 대화 상자(xdg-dialog-v1)가 있으면 그 대화 상자가 활성화되고 키보드를 받는다(`comp/src/state.rs`의 `focus_window`) |
| 포인터를 움직이면 컴포지터가 CPU 한 코어 넘게 씀(시스템 모니터를 띄운 채 130%) | 커서 이미지를 Abgr8888로 만들어서, smithay가 커서를 커서 평면(크기 64×64, Argb8888)에 올리지 못하고 프레임마다 화면에 합성했다. virtio-gpu의 커서 큐 인터럽트가 0이었다 | 커서 버퍼를 xcursor 파일 바이트 그대로 Argb8888로 만든다(`comp/src/cursor.rs`). 빨강과 파랑이 바뀌어 보이던 문제도 함께 고쳐졌다(기본 화살표는 무채색이라 보이지 않았다) |
| 보이지 않는 창이 CPU를 씀(다른 창 뒤의 시스템 모니터에 컴포지터 39%, 대부분 llvmpipe) | 모든 표면에 프레임마다 프레임 알림(frame callback)을 보내서, 가려진 창의 애니메이션도 화면 주사율(QEMU 창은 Mac 화면을 따라 120~144Hz)로 그려졌다. Dock 아래에 걸친 창은 멀리 떨어진 곳을 그려도 커밋할 때마다 Dock 흐림을 다시 했다 | 이번 프레임에 보인 표면에만 바로 알리고, 다 가려진 표면과 숨은 막대는 1초에 한 번 알린다(`comp/src/udev.rs`). 흐림은 아래 요소의 손상이 흐릴 범위에 닿을 때만 다시 한다(`comp/src/blur.rs`) |
| 직렬 콘솔에서 띄운 GTK 앱이 메모리를 많이 씀(시스템 설정 475MB) | 세션 밖에서 띄우면 컴포지터가 정한 `GSK_RENDERER=cairo`를 받지 못해 GTK가 GL(llvmpipe)로 그린다 | 콘솔에서 앱을 띄울 때 `GSK_RENDERER=cairo`를 함께 주거나, 사용자 서비스(`systemd-run --user`)로 띄워 세션의 환경을 받는다. 같은 조건에서 시스템 설정은 페이지를 처음 열 때만 늘고(102→164MB) 다시 돌아도 늘지 않았다 |
| Dock 크기를 키운 뒤 줄이면 아이콘만 작아지고 Dock 판은 큰 채로 남고 아이콘이 왼쪽 위로 몰림 | gtk4-layer-shell은 표면 크기를 전체 화면 창의 크기처럼 GTK에 알리고, GTK는 그 크기를 계속 쓴다. 크기를 바꿀 수 있는 창은 내용이 최소 크기보다 클 때만 늘리므로 줄지 않았다. 96px에서 32px로 바꾸면 창이 1154×152로 남았다(맞는 크기 514×88) | Dock 창을 크기를 바꿀 수 없는 창으로 둔다(`shell/src/dock.rs`의 `set_resizable(false)`). GTK가 창 크기를 내용 크기로 맞춰, 96→32→48에서 1154×152 → 514×88 → 674×104가 됐다 |
| 시스템 설정의 슬라이더 길이가 행마다 다름(크기가 투명도보다 김) | 슬라이더의 hexpand가 행까지 퍼져, 제목이 남긴 너비를 제목과 나눠 가졌다. 제목이 짧을수록 슬라이더가 길었다: 크기 390px, 투명도 384px, 키 반복 속도 370px, 반복 지연 시간 364px. 양 끝 이름도 트랙보다 12px씩 바깥에 있었다 | 슬라이더 행의 조작 너비를 260px로 고정하고 hexpand를 끈다(`settings/src/widgets.rs`의 `slider_row`). 네 트랙 모두 236px로 같은 자리에 있고, 양 끝 이름은 트랙 끝 아래에 온다 |
| 하드웨어 커서 평면을 쓰지 않아 포인터를 움직일 때마다 화면을 다시 합성함 | 가상 GPU(virtio-gpu)의 커널은 커서 핫스폿을 알리겠다고 한(DRM_CLIENT_CAP_CURSOR_PLANE_HOTSPOT) 클라이언트에게만 커서 평면을 보인다. smithay 0.7은 이 기능을 모른다(기록: `planes primary 1, cursor 0`). DrmDevice를 만든 뒤에 켜면 smithay의 원자적 모드 표면이 그 평면을 몰라 출력 설정이 `UnknownPlane`으로 실패하고 컴포지터가 꺼졌다 | DrmDevice보다 먼저 원자적 모드와 핫스폿 기능을 켠다(`comp/src/udev.rs`). 커서 버퍼는 Argb8888로 화면 픽셀 크기와 같게 만든다. 이제 커서가 커서 평면(plane-1)에 오른다(`/sys/kernel/debug/dri/0/state`). QMP 화면 캡처(screendump)에는 커서가 나오지 않는다 |
| GTK 앱이 조금만 바뀌어도 컴포지터가 창 전체를 다시 그림(시스템 모니터 하나에 42~48%) | 세션은 `GSK_RENDERER=cairo`이고, GTK의 cairo 렌더러는 그릴 때마다 버퍼 전체를 손상으로 알린다(시스템 모니터는 초당 10번 `damage_buffer(0, 0, 850, 718)`, WAYLAND_DEBUG로 확인). 창 그림자 여백이 Dock 밑에 닿아 Dock 흐림도 매번 다시 했다. perf로 보면 컴포지터 시간의 84%가 llvmpipe의 그리기였다 | 버퍼 거의 전체를 손상으로 알리는 SHM 커밋은 앞 커밋의 사본과 64×32 타일로 견주어 바뀐 타일만 손상으로 남긴다(`comp/src/shmdiff.rs`). 시스템 모니터를 띄운 채 42~48% → 7%, 창 넷을 띄운 채 50% → 9% |
| 여러 창 위에서 큰 창을 빠르게 옮기면 CPU가 크게 오름 | GPU가 없는 VM에서 컴포지터가 옮기는 창의 앞뒤 자리를 llvmpipe로 60Hz에 다시 합성한다(컴포지터 주 스레드는 3~4%). Mac의 Homebrew QEMU에는 GPU 가속(virtio-gpu-gl, virglrenderer)이 없다 | 손상 줄이기 뒤 700×520 창을 창 셋 위에서 원을 그리며 옮길 때 62%(전 72%). 소프트웨어 렌더러에서는 앞에 있지 않은 창의 프레임 알림을 30Hz로 줄인다. 더 줄이려면 QEMU의 GPU 가속이 필요하다 |
| Mac 창(`view-mac.sh`)에서 GPU 경로(linux-dmabuf, GTK의 GL)를 볼 수 없음 | QEMU의 cocoa 화면에는 GL이 없고(QEMU 10.0), Homebrew QEMU에는 virglrenderer가 없어 3D 가상 GPU(virtio-gpu-gl)를 붙일 수 없다 | 빌드 VM에서 `VMCTL_GPU=virgl`로 띄워 QMP로 본다(4장). 게스트의 GL은 빌드 VM이 llvmpipe로 그리므로 성능은 견주지 않고 경로만 본다 |
| 창 제목 막대를 메뉴 막대 밑으로 옮기면 다시 잡을 수 없음 | 컴포지터가 창을 어디로든 옮기게 했다 | macOS처럼 창의 위쪽 끝은 메뉴 막대 아래 끝을 넘지 못한다(옮기기와 위쪽 가장자리 크기 조절). Dock 쪽은 넘어가도 된다 |
| 메뉴 막대 메뉴가 Esc로 닫히지 않음 | 글자 칸에 포커스가 있으면 입력기(fcitx5)가 키보드를 잡고 있는데, 컴포지터가 이것을 다른 그랩으로 보고 메뉴의 키보드 그랩을 거절했다. 레이어 포커스 정리가 열린 팝업의 포커스를 창으로 되돌리기도 했다 | 입력기 그랩은 팝업 그랩을 막지 않고, 열린 팝업의 포커스는 건드리지 않는다(`comp/src/state.rs`) |
| 메뉴 막대와 제어 센터에서 Bluetooth·Wi-Fi를 켜고 끄거나 전원 모드를 골라도 바뀌지 않음 | 셸의 `dbus::set_property`가 값을 v로 두 번 감싸 보냈다. 서비스가 "Expected type 'b' but got 'v'"로 거절했다(GDBus 시험 버스로 재현). QEMU에 Wi-Fi·Bluetooth 장치가 없어 드러나지 않았다 | 값을 한 번만 감싼다(`shell/src/dbus.rs`, 2026-10-29). 전원 모드는 VM에서 다시 확인한다 |
| 시스템 버스나 세션 버스가 없는 곳(dbus-daemon이 없는 컨테이너, 이미지 밖)에서 셸이 뜨자마자 죽음(`gio-0.22.10/src/dbus.rs:44`의 panic, `UnexpectedNone`) | GLib의 `g_bus_watch_name()`은 버스에 붙지 못하면 사라짐 처리기를 연결 없이(NULL) 부르는데, gio-rs의 이름 감시가 그 연결을 unwrap한다(gtk-rs-core main도 같다). 지켜보던 연결이 닫힐 때(버스 데몬이 끝나거나 다시 시작할 때)도 GLib은 NULL을 주므로 `bus_watch_name_on_connection`만으로는 막지 못한다. 시험용 버스 데몬을 SIGKILL로 끝내 재현했다 | Bluetooth와 입력 소스는 버스 연결을 먼저 얻고(`gio::bus_get_future`), 얻지 못하면 기록만 남기고 항목을 끈다. 이름은 `shell/src/dbus.rs`의 `watch_name`으로 지켜보며, 연결이 닫히면 GLib보다 먼저 감시를 멈춘다. 버스가 끊기면 셸은 GLib의 exit-on-close로 SIGTERM을 받아 끝나고 컴포지터는 다시 띄우지 않는다. 셸의 이름 감시는 모두 `shell/src/dbus.rs`의 `watch_name`·`watch_name_owner`를 쓴다(2026-10-31) |
| 한글을 조합하는 중에 다른 창을 누르면 조합하던 글자가 사라짐 | GTK는 text-input-v3의 leave를 받으면 조합 글자를 버리고, fcitx5는 input-method-v2에서 확정을 앱 쪽에 맡긴다(ClientUnfocusCommit). 아무도 확정하지 않았다 | 컴포지터가 입력기의 조합 글자를 기억해 두었다가 키보드 포커스를 다른 표면으로 옮기기 직전에 확정한다(`comp/src/ime.rs`, 2026-10-30) |
| GTK4 앱의 한 창 안에서 조합 중에 다른 칸을 누르면 조합하던 글자가 사라짐 | GTK4는 칸이 포커스를 잃으면 조합 글자를 버리고(gtkimcontextwayland.c의 disable) 다음 칸에서 글 입력을 다시 켠다. 컴포지터가 확정을 보내도 GTK는 이미 다음 칸으로 옮겨 그 칸에 들어간다. Firefox(GTK3)는 칸을 떠날 때 스스로 확정한다 | 조합을 끝낸 뒤(스페이스, Enter, Tab, 방향키, 입력 소스 전환) 옮긴다. 다른 창을 누를 때는 확정된다. GTK 쪽 고침이 필요하다 |
| 터미널(foot)에서 조합 중 Enter를 누르면 가끔 마지막 글자가 줄바꿈 뒤로 감 | 입력기가 commit을 보낼 때 그사이 컴포지터가 보낸 done을 아직 받지 못했으면 serial이 어긋나고, smithay는 그때 앱에 done(0)을 보냈다. foot은 serial이 다른 done을 버리고 확정 글자를 다음 done까지 미룬다(빠르게 치면 줄마다 done(0)이 보였다) | 입력기의 commit은 늘 그 글 입력의 serial로 done을 보낸다(`comp/src/ime.rs`, 2026-10-30). input-method-v2 명세대로 serial이 달라도 바뀐 것을 적용한다 |
| 왼쪽 Shift만 눌렀다 떼면 입력 소스가 영문으로 바뀜 | fcitx5의 AltTriggerKeys(첫 입력 소스와 지금 입력 소스 오가기) 기본값이 Shift_L이다 | 기본 설정(`/etc/xdg/fcitx5/config`)에서 빈 목록으로 끈다(2026-10-30) |
| Mac 창에서 로고 메뉴의 잠자기를 누르면 검은 화면에서 멈추고 키보드·마우스가 듣지 않음 | 잠자기 직전에 컴포지터가 화면을 가린 뒤 VM이 잠들었는데 깨울 장치가 없다. QEMU virt(aarch64)에는 S3가 없어 커널이 s2idle로만 잠들고(`/sys/power/mem_sleep`이 `[s2idle]`), virtio 입력 장치는 깨우는 장치가 아니다. 멈춘 VM은 CPU를 5초에 70ms로 일정하게 써, 잠금 화면이 다시 뜨는 흔적도 없었다 | 2026-10-30부터 가상 기기에서는 이미지가 잠자기를 끈다(`buntos-vm-sleep.service`가 `/run/systemd/sleep.conf.d`에 `AllowSuspend=no`를 놓는다). logind의 CanSuspend가 no가 되어 셸이 로고 메뉴의 잠자기를 흐리게 두고, `systemctl suspend`도 "disabled by config"로 거절된다. 이미 멈춘 VM은 창을 닫아 끈다 |
| 시스템 모니터에서 GPU 상태를 볼 수 없음 | GNOME 시스템 모니터는 CPU, 메모리, 디스크, 네트워크만 보인다 | 2026-10-30부터 기본 앱이 GNOME Resources(resources 51)다. GPU 페이지에 사용량·영상 메모리·클럭·온도·전력과 PCI 슬롯·드라이버가, 앱 목록에 앱별 GPU 사용량이 보인다. 사용량은 드라이버가 알리는 것만 읽는다(amdgpu, i915, xe, v3d, NVIDIA). VM의 virtio-gpu는 3D가 없어 늘 0%다(화면은 CPU가 그려 프로세서 사용량에 잡힌다). Mali(panthor)는 원본 코드로 보면 사용량을 읽지 않는다 |
| 시스템 모니터(Resources)가 영어로 보임 | Debian의 resources 51.0에 한국어 번역(ko)이 없다 | GNOME 번역으로 한국어를 올려 보내야 한다 |
| 빌드 VM에서 `VMCTL_GPU=virgl`이 "빌드 VM에 렌더 노드가 없다"로 멈춤 | Lima 빌드 VM의 커널(`6.12 … cloud-arm64`)에 vgem 모듈이 없어 렌더 노드(/dev/dri/renderD*)를 만들 수 없다 | `VMCTL_RENDERNODE=/dev/null`로 띄운다(Mesa의 소프트웨어 GBM). 게스트의 GL·linux-dmabuf 경로는 돈다(2026-10-31). 이때 QMP 화면 저장은 "no surface"로 실패하므로 에이전트의 창 그림(`bunt agent screenshot`)으로 본다 |
| 로고 메뉴·앱 메뉴의 항목이 접근성 트리에 이름 없이 `menuitem`으로만 나옴 | GTK 4.24.0까지 GtkPopoverMenu의 항목(GtkModelButton)이 이름을 가져오는 레이블의 역할이 presentation이라 GTK의 이름 계산이 그 글을 버린다. GTK main(4.25 개발판)에서 고쳐졌다 | 셸이 메뉴를 열 때 항목의 labelled-by를 떼고 레이블 글을 이름으로 단다(`shell/src/menu_names.rs`). 고친 GTK가 들어오면 지운다 |
| bunt-shell이 앱 항목 메뉴를 다시 읽을 때마다 `g_variant_get_variant` 경고 | glib-rs가 `av`를 `Vec<Variant>`로 읽을 때 이미 상자를 벗기는데 한 번 더 벗겼다 | 고쳤다(2026-10-31). nm-applet의 메뉴를 열고 바꿔도 경고가 없다 |
| 에이전트가 셸 단추를 포인터로 누르면 "the compositor has no window for this app" | 셸·입력기·잠금 화면은 컴포지터가 만든 socketpair를 `WAYLAND_SOCKET`으로 넘겨받아 붙는데, socketpair의 SO_PEERCRED는 쌍을 만든 프로세스(컴포지터)다. 그래서 컴포지터가 셸의 표면과 키보드 포커스를 컴포지터의 pid로 알렸다 | 컴포지터가 띄운 자식의 pid를 클라이언트 상태에 적고 그것을 먼저 쓴다(2026-10-31) |
| 에이전트가 메뉴를 Esc로 닫은 뒤 다른 메뉴 막대 단추를 누르면 방금 닫은 메뉴가 다시 열림 | 키보드나 앱이 닫은 메뉴의 포인터 잡기(smithay 0.7의 `PopupPointerGrab`)는 다음 움직임을 받아 풀리면서 그 움직임을 버리고 포인터를 전 자리에 둔다(`unset_grab`의 restore_focus). 한 번 옮기고 바로 누르는 에이전트는 전 자리를 누른다 | 컴포지터의 `pointer_moved`가 움직인 뒤에도 포인터가 그 자리에 없으면 한 번 더 보낸다(2026-10-31). e2e 5번이 본다 |
| 에이전트가 앱 항목(트레이) 단추를 누르면 메뉴가 열리지 않고 메뉴 항목 하나가 실행됨(`ran its only accessible action "sni.item-13"`) | 셸이 dbusmenu 동작 묶음을 단추에 달아 GTK가 그 동작을 모두 단추의 접근성 동작으로 내보냈고, 데몬은 누르기 이름의 동작이 없으면 첫 동작을 실행했다 | 셸은 묶음을 메뉴(PopoverMenu)에 달고, 데몬은 누르기 이름이 아닌 동작을 그것 하나뿐일 때만 실행한다. 여럿이면 포인터로 누른다(2026-10-31) |
| ⇧⌘5 도구 막대에서 Return을 누르면 찍지 않고 막대가 닫힘 | 막대가 뜨면 닫기 단추가 키보드 포커스를 가지는데, 막대의 키 처리기가 버블 단계라 그 단추가 Return을 먼저 받았다 | 막대의 키를 캡처 단계에서 받아 Return은 캡처, Esc는 닫기다. 옵션 메뉴가 열려 있으면 메뉴에 둔다(2026-10-31) |
| 에이전트가 드롭다운의 열린 목록에서 항목을 눌러도 값이 바뀌지 않음("selected it in its list") | 데몬은 누르기 이름의 동작이 없는 목록 항목을 접근성 Selection으로 고르는데, GtkDropDown의 팝업 목록은 고르기만으로는 값을 바꾸지 않는다 | 아직 고치지 않았다(설계안 11장 다음 작업 1번). `click <ref> --pointer`나 QMP 누르기로 고른다 |
| QMP로 ⇧⌘4를 (200,200)에서 (400,300)까지 끌면 201×101이 찍힘 | QMP의 절대 좌표가 소수 논리 좌표가 되어, 영역을 바깥쪽으로(floor·ceil) 잡는 계산에서 1픽셀이 더해진다 | 시험의 기대값을 ±1로 본다. 사람의 드래그도 같은 계산이다 |
| CI(Ubuntu 26.04 러너)에서 이미지 빌드가 "Applying presets"(`systemctl --root=/buildroot preset-all`)에서 실패 | mkosi는 호스트의 systemctl을 부른다. systemd 259(259.5, 259.9)는 레시피가 마스크한 `getty@tty1`과 기본 프리셋의 `enable getty@.service`(`DefaultInstance=tty1`)를 만나면 1로 끝난다(이 둘만 둔 루트로 재현). systemd 255와 빌드 VM의 257은 알리고 넘어간다. 꾸러미가 마스크한 `sudo.service`·`alsa-utils.service`도 오류로 찍히지만 그것만으로는 실패하지 않는다 | CI는 빌드 VM과 같은 Debian 13의 도구 트리(`tools/image/tools-tree.sh`)로 만든다(2026-10-30). 빌드 VM이나 도구 트리의 systemd를 259 이상으로 올리기 전에 레시피의 마스크를 프리셋 뒤(`mkosi.finalize`)로 옮기거나 드롭인 조건으로 바꾼다(설계안 11장 다음 작업 4번) |
| root 콘솔에서 `sudo -u buntos python3 mcp-check.py`로 띄운 MCP 시험이 "mcp-check.py is not allowed to control this computer"로 거절되고 AI 접근 요청 창이 뜸 | 데몬은 소켓에 붙은 `bunt`에서 위로 올라가며 AI 클라이언트를 찾고, root가 `bunt`를 직접(또는 sudo 같은 감싼 프로그램으로) 부를 때만 관리자로 본다. 사용자로 띄운 스크립트는 사람이 허용할 AI다(설계대로) | 시험에서는 `bunt-access-helper add-client '이름' "$(readlink -f /usr/bin/python3)" /tmp/mcp-check.py`로 더하고 끝나면 `remove-client`. AI 접근 요청 창은 키보드를 가져가지 않으므로 QMP Esc가 닿지 않는다. [거부]를 누른다. 창이 떠 있는 동안은 에이전트의 창 활성화도 거절된다 |
| `bunt agent repl`이 시험 VM의 Firefox에서 "the code ran past its time limit (30000 ms)" | 호출의 기본 시간 상한은 30초인데, 바쁜 Mac의 시험 VM에서 Firefox가 file:// 페이지를 여는 데 1분 가까이 걸린다 | `bunt agent repl --timeout 120000`으로 늘린다(최대 120초) |
| e2e 5번 끝에 확인을 더하자 e2e 6번의 ⌘⇥ 확인 셋이 실패(시스템 설정 대신 계산기로 감) | e2e 6번의 ⌘⇥는 e2e 5번이 남긴 앱 차례(마지막이 시스템 설정, 계산기와 Firefox는 닫힘)를 쓴다. 끝에 넣은 REPL 확인이 계산기를 다시 열어 가장 최근 앱이 되었다 | e2e 5번에 앱을 여는 확인을 더할 때는 그 앱이 원래 떠 있는 자리(계산기는 닫기 전)에 두거나 끝나기 전에 닫는다(2026-10-31) |
