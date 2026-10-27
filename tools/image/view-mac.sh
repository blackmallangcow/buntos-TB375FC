#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 빌드 VM에서 만든 arm64 이미지를 Mac으로 가져와 창으로 띄운다 (Apple Silicon, HVF 가속).
# Mac 터미널의 저장소 루트에서 실행한다. QEMU가 필요하다: brew install qemu
# 사용법: tools/image/view-mac.sh [--no-copy]
#   --no-copy  이미지를 다시 가져오지 않고 지난번에 가져온 것을 띄운다 (빌드 VM이 꺼져 있어도 된다)
# 이미지는 읽기 전용으로 띄우므로(snapshot) 안에서 바꾼 내용은 끄면 사라진다.
set -euo pipefail

board=qemu-aarch64
vm=${BUNTOS_DEV_VM:-buntos-dev}
cache="$HOME/Library/Caches/buntos"
image="$cache/$board.raw"
firmware="$(brew --prefix qemu)/share/qemu/edk2-aarch64-code.fd"

if [[ "${1:-}" != --no-copy ]]; then
	if [[ "$(limactl list --format '{{.Status}}' "$vm" 2>/dev/null)" != Running ]]; then
		echo "빌드 VM($vm)이 켜져 있지 않다. limactl start $vm 으로 켜거나, --no-copy로 지난번에 가져온 이미지를 띄운다" >&2
		exit 1
	fi
	mkdir -p "$cache"
	vm_home=$(limactl shell "$vm" -- sh -c 'echo $HOME')
	echo "VM에서 이미지를 가져온다: $vm:$vm_home/buntos-build/$board/buntos.raw"
	limactl copy "$vm:$vm_home/buntos-build/$board/buntos.raw" "$image"
elif [[ ! -f "$image" ]]; then
	echo "가져온 이미지가 없다: $image (--no-copy 없이 실행한다)" >&2
	exit 1
fi

# zoom-to-fit은 쓰지 않는다. cocoa는 창 크기를 게스트에 해상도로 알려 주는데, zoom-to-fit을 켜면
# 처음 창(640x480)을 화면 비율에만 맞춰 640x400이 된다. bunt-comp는 아직 해상도 변경을 따라가지 않는다
exec qemu-system-aarch64 \
	-name buntOS \
	-machine virt -accel hvf -cpu host -smp 4 -m 4G \
	-drive "if=pflash,format=raw,readonly=on,file=$firmware" \
	-drive "if=virtio,format=raw,file=$image,snapshot=on" \
	-device virtio-gpu-pci -device virtio-keyboard-pci -device virtio-tablet-pci \
	-nic user,model=virtio-net-pci \
	-display cocoa
