#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 빌드 VM에서 만든 arm64 이미지를 Mac으로 가져와 창으로 띄운다 (Apple Silicon, HVF 가속).
# Mac 터미널의 저장소 루트에서 실행한다. QEMU가 필요하다: brew install qemu
# 사용법: tools/image/view-mac.sh [--no-copy] [--setup]
#   --no-copy  이미지를 다시 가져오지 않고 지난번에 가져온 것을 띄운다 (빌드 VM이 꺼져 있어도 된다)
#   --setup    시험 계정 없이 띄워 처음 켤 때의 계정 만들기 화면을 본다
# 이미지는 읽기 전용으로 띄우므로(snapshot) 안에서 바꾼 내용은 끄면 사라진다.
# 시험 계정 buntos(암호 buntos, 관리자)를 부팅 때 systemd 자격 증명 buntos.account로 만들어 주므로
# 로그인 화면에 buntos를 치고 들어간다. BUNTOS_ACCOUNT="계정 이름:전체 이름:암호"로 바꿀 수 있다.
set -euo pipefail

board=qemu-aarch64
vm=${BUNTOS_DEV_VM:-buntos-dev}
cache="$HOME/Library/Caches/buntos"
image="$cache/$board.raw"
firmware="$(brew --prefix qemu)/share/qemu/edk2-aarch64-code.fd"
copy=yes
account=${BUNTOS_ACCOUNT-buntos:buntOS:buntos}
for arg in "$@"; do
	case "$arg" in
	--no-copy) copy=no ;;
	--setup) account= ;;
	*)
		sed -n '7,13p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2
		exit 2
		;;
	esac
done
# macOS의 bash 3.2는 set -u에서 빈 배열을 펼치지 못하므로 인자를 문자열 두 개로 둔다
credential_flag=
credential_value=
if [[ -n "$account" ]]; then
	credential_flag=-smbios
	credential_value="type=11,value=io.systemd.credential:buntos.account=$account"
fi

if [[ "$copy" == yes ]]; then
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
# 처음 창(640x480)을 화면 비율에만 맞춰 640x400이 된다. bunt-comp는 선호 해상도가 바뀌면 따라가지만
# (VNC로 확인), cocoa 창 크기 바꾸기로는 아직 확인하지 않았다
exec qemu-system-aarch64 \
	-name buntOS \
	-machine virt -accel hvf -cpu host -smp 4 -m 4G \
	-drive "if=pflash,format=raw,readonly=on,file=$firmware" \
	-drive "if=virtio,format=raw,file=$image,snapshot=on" \
	-device virtio-gpu-pci -device virtio-keyboard-pci -device virtio-tablet-pci \
	-audiodev coreaudio,id=snd0 -device virtio-sound-pci,audiodev=snd0,streams=1 \
	-nic user,model=virtio-net-pci \
	${credential_flag:+"$credential_flag" "$credential_value"} \
	-display cocoa
