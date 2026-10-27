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

x86-64는 보드 이름만 `x86_64-generic`으로 바꾸면 된다. Apple Silicon에서는 패키지 설치 스크립트를 qemu-user로 에뮬레이션해 빌드하고, 부팅도 KVM 없이 소프트웨어 에뮬레이션(TCG)으로 한다. CI의 x86-64 러너에서는 KVM을 쓴다([설계안](../design/ARCHITECTURE.md) 9.4).

### 그래픽 세션 보기

이미지 빌드는 자체 컴포넌트(bunt-comp, bunt-shell, bunt-settings)를 먼저 컴파일해 이미지에 넣는다. 화면 장치가 있으면 tty1에서 개발용 사용자 `buntos`로 자동 로그인해 그래픽 세션을 연다. 빌드 VM과 아키텍처가 다른 x86-64 이미지에는 아직 넣지 않는다. 필수 드라이버를 넣은 뒤 CI에서 빌드한다.

Dock에는 기본 앱인 파일(Nautilus), 터미널(foot), 텍스트 편집기, 계산기, 이미지 보기(Loupe), 문서 보기(Papers), 시스템 모니터와 시스템 설정(bunt-settings)이 있다. 터미널에서는 nano, vim, htop을 쓸 수 있다. 네트워크는 아직 설정하지 않았으므로 웹 브라우저는 넣지 않았다.

Mac에서 창으로 본다. Mac 터미널의 저장소 루트에서 실행하며, QEMU가 필요하다(`brew install qemu`):

```bash
tools/image/view-mac.sh
```

빌드 VM에서 이미지(약 4GB)를 `~/Library/Caches/buntos/`로 가져온 뒤 HVF 가속으로 띄운다. 이미지는 snapshot 모드로 띄우므로 안에서 바꾼 내용은 창을 닫으면 사라진다. 같은 이미지를 다시 볼 때는 `--no-copy`를 붙이면 가져오는 단계를 건너뛰며, 이때는 빌드 VM이 꺼져 있어도 된다. Mac 메모리가 16GB 이하이면 이미지를 가져온 뒤 빌드 VM을 끄고(`limactl stop buntos-dev`) `--no-copy`로 띄우는 편이 낫다. 빌드 VM이 8GiB, 보기용 QEMU가 최대 4GiB를 쓴다.

화면은 1280×800으로 고정이다. 창 크기를 바꾸거나 전체 화면으로 해도 해상도는 따라가지 않는다.

| 조작 | 방법 |
|---|---|
| 앱 실행 | Dock 아이콘을 누른다. 터미널은 Option+Return으로도 연다 |
| 시스템 설정, 다시 시작, 시스템 종료 | 메뉴바 왼쪽의 buntOS 메뉴 |
| 창 옮기기 | 제목줄을 끈다 |
| 창 크기 조절 | 창 가장자리를 끈다. 제목줄을 두 번 누르면 최대화와 복원 |
| 창 닫기 | 제목줄의 X 버튼, 또는 Option+F4 (Mac 노트북 키보드는 fn도 함께) |
| 세션 끝내기 | Ctrl+Option+Delete(⌫). tty1 셸로 돌아온다 |
| 끝내기 | QEMU 창을 닫는다 (종료 확인 창이 뜨면 Quit) |

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

2026-10-27에 1차 빌드와 그래픽 세션 v0를, 2026-10-28에 기본 앱과 시스템 설정을 확인했다.

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
| Mac에서 보기 | Mac의 QEMU 11.1.2(HVF)로 같은 이미지를 cocoa 창으로 띄워 1280×800 바탕화면과 메뉴바, Dock이 뜨는 것을 QMP 화면 캡처로 확인. 이미지 복사 10초. 이어서 사용자가 창에서 Mac 키보드와 마우스로 조작해 시계(한국 시간), 화면, 입력이 정상임을 확인 |
| 확인하지 않은 것 | `tools/devvm/buntos-dev.yaml`의 컨테이너 런타임 끄기 설정(검증한 VM은 그 설정을 넣기 전에 만들었다). 데스크톱 패키지를 넣은 지금 레시피로 x86-64 이미지를 빌드하고 e2e를 돌리는 것(x86-64 기록은 1차 빌드 때 것이며, 필수 드라이버를 넣은 뒤 확인한다). Mac 창에서 기본 앱과 시스템 설정을 직접 조작하는 것 |

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
| 창 최소화와 전체 화면, 한글 입력기가 없다 | bunt-comp v0 범위 밖 | 다음 단계. 창 버튼은 최대화와 닫기만 보인다 |
| GTK 앱이 시스템 설정(글꼴, 밝게·어둡게, 창 버튼)을 따르지 않음 | 지금의 GTK는 이 설정을 xdg-desktop-portal로 읽는데 포털이 없었다 | 포털과 GTK 구현(`xdg-desktop-portal-gtk`)을 넣고, 세션 환경을 bunt-shell보다 먼저 D-Bus에 알린다(`comp.toml`의 `prepare`) |
| 강조 색을 바꿔도 앱에 반영되지 않음 | GTK 포털은 `org.freedesktop.appearance`의 `color-scheme`만 주고 `accent-color`는 주지 않는다 | 시스템 설정 v0에서 강조 색을 뺐다. 강조 색을 전하는 포털 구현이 생기면 넣는다 |
| sudo가 매번 "unable to resolve host"를 경고 | 호스트 이름을 `/etc/hosts`에 적지 않았다 | `libnss-myhostname`을 넣었다. 시스템 설정에서 바꾼 이름도 찾는다 |
| 팝업 메뉴를 닫은 직후 QMP로 누른 곳이 엉뚱하게 눌림 | smithay는 팝업 그랩이 끝난 뒤 첫 포인터 이동을 그랩을 푸는 데 쓰고 버린다. QMP 클릭은 이동이 한 번뿐이었다 | `qmp.py`가 누르기 전에 포인터를 두 번 옮긴다. 실제 마우스는 이동이 이어져서 영향이 없다 |
| 테스트로 띄운 이미지가 바뀜 | mkosi `vm`은 이미지를 직접 쓴다. 이미지를 `-blockdev`로 붙여서 QEMU `-snapshot`도 효과가 없다. `--ephemeral`은 이미지 전체를 복사하는데, Mac 메모리가 빠듯할 때 첫 부팅이 1분 넘게 늦어졌다 | e2e와 화면 캡처는 VM을 멈춘 상태로 띄우고 QMP로 디스크에 임시 qcow2 덮어쓰기를 씌운 뒤 실행한다(`tools/image/qmp.py`의 `overlay`) |
| Mac 창에서 바탕화면이 640×400으로 뜸 | QEMU cocoa는 창 크기를 게스트에 해상도로 알려 준다. zoom-to-fit을 켜면 처음 창(640×480)을 화면 비율에만 맞춰 그 크기가 된다. bunt-comp는 아직 해상도 변경을 따라가지 않는다 | `view-mac.sh`는 zoom-to-fit 없이 띄워 1280×800으로 둔다. 해상도 변경 처리는 다음 단계 |
