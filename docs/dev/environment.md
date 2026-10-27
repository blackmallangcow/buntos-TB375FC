# 개발 환경

buntOS 이미지는 Linux에서 mkosi로 빌드한다. Apple Silicon Mac에서는 Lima로 Debian 13 VM을 띄워 빌드 호스트로 쓰고, aarch64 테스트 이미지는 그 VM 안에서 중첩 가상화(KVM)로 띄운다(ADR-010).

## 요구 사항

- Apple Silicon M3 이상, macOS 15 이상 (VM 안에서 KVM을 쓰기 위한 중첩 가상화 조건)
- Homebrew
- 디스크 여유 공간: VM 디스크는 최대 60GiB이며 실제로 쓴 만큼만 커진다. 도구 설치와 시험 빌드 한 번까지 Mac 쪽에서 약 8GB를 썼다

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

VM에는 저장소 폴더만 쓰기 가능하게 연결되고, VM 안에서도 경로가 같다. 홈 디렉터리의 다른 폴더는 연결하지 않는다.

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

### 시험 빌드

환경 확인용 최소 이미지다. 저장소 레시피가 아니므로 저장소 밖(VM 홈 디렉터리)에서 만든다.

```bash
mkdir -p ~/mkosi-smoke && cd ~/mkosi-smoke
```

`mkosi.conf`:

```ini
[Distribution]
Distribution=debian
Release=forky
Architecture=arm64

[Output]
Format=disk
ImageId=smoke

[Content]
Bootable=yes
Bootloader=systemd-boot
Packages=
        linux-image-arm64
        systemd
        systemd-boot
        systemd-boot-efi
        systemd-sysv
        udev
        dbus
Autologin=yes
KernelCommandLine=systemd.firstboot=no
```

```bash
mkosi -f build
```

```bash
mkosi --kvm=yes --tpm=no qemu
```

자동 로그인된 root 셸이 나오면 성공이다. QEMU를 끝내려면 `Ctrl-A` 다음 `X`를 누른다.

## 4. 일상 사용

| 할 일 | 명령 (Mac) |
|---|---|
| VM 셸 열기 | `limactl shell buntos-dev` |
| 끄기 / 켜기 | `limactl stop buntos-dev` / `limactl start buntos-dev` |
| 지우기 | `limactl delete buntos-dev` |

## 5. 검증 기록 (2026-10-27)

| 항목 | 결과 |
|---|---|
| 호스트 | Apple M4, 16GB, macOS 27.0.1, Lima 2.2.1 |
| VM | Debian 13, 커널 6.12(cloud-arm64), 6코어·8GiB. 커널이 EL2에서 시작하고 KVM nVHE 모드 초기화, `/dev/kvm` 확인 |
| 빌드 도구 | mkosi 27.1, QEMU 10.0.13 |
| 시험 빌드 | Debian forky arm64(커널 7.2.12, systemd 262), systemd-boot + UKI 디스크 이미지 1.1G. 패키지 캐시가 있는 상태에서 33초 |
| 시험 부팅 | QEMU `-accel kvm`, 실행부터 root 자동 로그인까지 13초 |
| 확인하지 않은 것 | x86-64 이미지 교차 빌드와 부팅 (binfmt 등록만 확인), `tools/devvm/buntos-dev.yaml`의 컨테이너 런타임 끄기 설정(검증한 VM은 그 설정을 넣기 전에 만들었다) |

## 6. 알려진 문제

이미지 레시피(Phase 0)를 만들 때 반영해야 한다.

| 문제 | 원인 | 지금의 대처 |
|---|---|---|
| 첫 빌드가 `/var/cache/mkosi` 권한 오류로 실패 | mkosi는 일반 사용자로 실행해도 이 경로에 패키지 캐시를 둔다 | `setup.sh`가 사용자 소유로 만든다 |
| "systemd-boot binary was not found" 빌드 오류 | Debian은 systemd-boot의 EFI 파일을 `systemd-boot-efi` 패키지로 나눈다 | 이미지 패키지 목록에 `systemd-boot-efi`를 넣는다 |
| 부팅이 시간대 입력 화면에서 멈춤 | systemd-firstboot의 대화형 설정 | 시험 이미지는 `systemd.firstboot=no`로 끈다. 실제 이미지는 최초 설정을 따로 설계해야 한다 |
| `/dev/tpm0`, `/dev/tpmrm0`를 기다리다 90초씩 지연 | mkosi가 붙이는 에뮬레이션 TPM을 게스트 커널이 장치로 잡지 못했다(원인 미확인) | `--tpm=no`로 끈다. 레시피 작업 때 원인을 확인한다 |
| `--console=read-only`, `interactive`를 쓸 수 없음 | Debian 13 호스트에 `systemd-pty-forward`가 없다 | 기본(native) 콘솔을 쓴다 |
