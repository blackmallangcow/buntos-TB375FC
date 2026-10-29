#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# buntOS 개발용 빌드 VM(Debian 13) 안에서 빌드 도구를 설치한다. 여러 번 실행해도 된다.
# 사용법: docs/dev/environment.md
set -euo pipefail

MKOSI_VERSION=v27.1

packages=(
	git ca-certificates pipx python3-venv
	# mkosi가 이미지를 만들 때 쓰는 도구
	bubblewrap uidmap systemd-container systemd-repart systemd-ukify systemd-boot systemd-boot-efi
	debian-archive-keyring dosfstools mtools e2fsprogs btrfs-progs erofs-utils squashfs-tools cpio zstd xz-utils
	# 이미지 부팅(QEMU, UEFI 펌웨어)과 x86-64 교차 빌드(binfmt)
	qemu-system-arm qemu-system-x86 qemu-utils qemu-efi-aarch64 ovmf qemu-user-binfmt swtpm swtpm-tools virtiofsd
	# 3D 가상 GPU(virtio-gpu-gl, virglrenderer)와 화면 없는 GL 화면(egl-headless). tools/image/vmctl.sh의 VMCTL_GPU=virgl
	qemu-system-modules-opengl
	# 자체 컴포넌트(Rust) 빌드. 툴체인은 rust-toolchain.toml에 고정한 버전을 rustup으로 받는다
	build-essential pkg-config rustup
	libudev-dev libinput-dev libgbm-dev libseat-dev libxkbcommon-dev libegl-dev libgles-dev libdrm-dev
	libwayland-dev libpixman-1-dev libgtk-4-dev libgtk4-layer-shell-dev
	# 잠금 화면(bunt-lock)의 암호 확인
	libpam0g-dev
)

sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "${packages[@]}"

# 테스트 이미지를 KVM으로 띄우기 위해 필요하다. 새 셸 세션부터 적용된다
sudo usermod -aG kvm "$USER"

# mkosi는 일반 사용자로 실행해도 패키지 캐시를 /var/cache/mkosi에 둔다
sudo install -d -m 0755 -o "$USER" -g "$(id -gn)" /var/cache/mkosi

if [[ "$("$HOME/.local/bin/mkosi" --version 2>/dev/null || true)" != "mkosi ${MKOSI_VERSION#v}" ]]; then
	pipx install --force "git+https://github.com/systemd/mkosi.git@${MKOSI_VERSION}"
fi
pipx ensurepath

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
toolchain=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$repo/rust-toolchain.toml")
rustup toolchain install "$toolchain" --profile minimal --component clippy --component rustfmt
