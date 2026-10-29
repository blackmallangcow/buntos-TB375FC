#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 빌드 VM에서 만든 arm64 이미지를 Mac으로 가져와 창으로 띄운다 (Apple Silicon, HVF 가속).
# Mac 터미널의 저장소 루트에서 실행한다. QEMU가 필요하다: brew install qemu
# 사용법: tools/image/view-mac.sh [--no-copy] [--keep] [--size=크기] [--setup] [--full-grab]
#   --no-copy    이미지를 다시 가져오지 않고 지난번에 가져온 것을 띄운다 (빌드 VM이 꺼져 있어도 된다)
#   --keep       VM 안에서 바꾼 내용(설치한 앱, 로그인 정보 등)을 이미지에 남긴다. 주지 않으면 이미지를
#                읽기 전용으로 띄우므로(snapshot) 끄면 사라진다
#   --size=크기  디스크 크기 (GiB, 기본 16G). 이미지 파일을 이 크기까지 성기게 늘리면(쓴 만큼만 Mac 디스크를
#                차지한다) buntOS가 부팅 때 루트를 디스크 끝까지 늘린다. 이미지를 줄이지는 않는다
#   --setup      시험 계정 없이 띄워 처음 켤 때의 계정 만들기 화면을 본다
#   --full-grab  macOS가 먼저 가져가는 단축키(⌘⇥, ⌘스페이스, ⌃스페이스, ⇧⌘3 등)도 VM에 보낸다.
#                시스템 설정 > 개인정보 보호 및 보안 > 손쉬운 사용에서 이 스크립트를 실행하는 앱(터미널)을
#                허용해야 한다. 권한이 없으면 QEMU가 "Could not create event tap" 경고를 내고 macOS가 받는다
# 가져온 뒤 VM이 이미지에 쓴 적이 있으면(--keep 등) 바꾼 내용을 지키려고 새 빌드를 가져오지 않고 멈춘다.
# 그대로 띄우려면 --no-copy를 더하고, 바꾼 내용을 버리려면 이미지를 지운 뒤 다시 실행한다.
# 시험 계정 buntos(암호 buntos, 관리자)를 부팅 때 systemd 자격 증명 buntos.account로 만들어 주므로
# 로그인 화면에 buntos를 치고 들어간다. BUNTOS_ACCOUNT="계정 이름:별칭:암호"로 바꿀 수 있다.
# ⌘ 단축키는 창이 입력을 잡았을 때(포인터가 창 안에 있을 때)만 VM으로 간다. ⌃⌥G로 입력을 놓는다.
set -euo pipefail

board=qemu-aarch64
vm=${BUNTOS_DEV_VM:-buntos-dev}
cache="$HOME/Library/Caches/buntos"
image="$cache/$board.raw"
# 이 스크립트가 이미지를 마지막으로 쓴(가져오고 늘린) 뒤의 수정 시각과 크기. 지금 값이 다르면 그 뒤에
# VM이 이미지에 쓴 것이다
stamp="$image.stamp"
firmware="$(brew --prefix qemu)/share/qemu/edk2-aarch64-code.fd"
copy=yes
keep=no
size=16G
account=${BUNTOS_ACCOUNT-buntos:buntOS:buntos}
display=cocoa
for arg in "$@"; do
	case "$arg" in
	--no-copy) copy=no ;;
	--keep) keep=yes ;;
	--size=*) size=${arg#--size=} ;;
	--setup) account= ;;
	--full-grab) display=cocoa,full-grab=on ;;
	*)
		sed -n '7,21p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2
		exit 2
		;;
	esac
done
if [[ ! "$size" =~ ^[1-9][0-9]*G$ ]]; then
	echo "--size는 16G처럼 GiB로 준다" >&2
	exit 2
fi
# macOS의 bash 3.2는 set -u에서 빈 배열을 펼치지 못하므로 인자를 문자열 두 개로 둔다
credential_flag=
credential_value=
if [[ -n "$account" ]]; then
	credential_flag=-smbios
	credential_value="type=11,value=io.systemd.credential:buntos.account=$account"
fi

# GNU coreutils의 stat이 PATH에 먼저 있어도 macOS의 stat을 쓴다
untouched() { [[ -f "$stamp" && "$(/usr/bin/stat -f '%m %z' "$image")" == "$(cat "$stamp")" ]]; }

if [[ "$copy" == yes ]]; then
	if [[ -f "$image" ]] && ! untouched; then
		echo "$image 에 VM 안에서 바꾼 내용이 남아 있을 수 있어(--keep 등) 새 빌드로 덮어쓰지 않는다." >&2
		echo "그대로 띄우려면 --no-copy를 더한다. 바꾼 내용을 버리고 새 빌드를 띄우려면 지운 뒤 다시 실행한다:" >&2
		echo "  rm '$image'" >&2
		exit 1
	fi
	if [[ "$(limactl list --format '{{.Status}}' "$vm" 2>/dev/null)" != Running ]]; then
		echo "빌드 VM($vm)이 켜져 있지 않다. limactl start $vm 으로 켜거나, --no-copy로 지난번에 가져온 이미지를 띄운다" >&2
		exit 1
	fi
	mkdir -p "$cache"
	vm_home=$(limactl shell "$vm" -- sh -c 'echo $HOME')
	echo "VM에서 이미지를 가져온다: $vm:$vm_home/buntos-build/$board/buntos.raw"
	rm -f "$stamp"
	# 가져오다 실패하거나 멈추면 반쯤 쓴 이미지를 지운다
	trap 'rm -f "$image"' EXIT
	limactl copy "$vm:$vm_home/buntos-build/$board/buntos.raw" "$image"
	trap - EXIT
elif [[ ! -f "$image" ]]; then
	echo "가져온 이미지가 없다: $image (--no-copy 없이 실행한다)" >&2
	exit 1
fi

# 디스크를 --size까지 늘린다. 파일 끝만 옮기므로(성긴 파일) VM이 쓰기 전에는 자리를 차지하지 않는다.
# buntOS가 부팅 때 루트 파티션과 파일 시스템을 디스크 끝까지 늘린다
# (image/mkosi.extra/usr/lib/repart.d/10-root.conf)
if [[ "$copy" == yes ]] || untouched; then
	pristine=yes
else
	pristine=no
fi
if (($(/usr/bin/stat -f %z "$image") < ${size%G} * 1024 * 1024 * 1024)); then
	qemu-img resize -q -f raw "$image" "$size"
fi
# --keep으로 띄우면 VM이 이미지에 쓰므로 다음에 새 빌드를 가져오지 않게 표시를 지운다
if [[ "$keep" == yes ]]; then
	rm -f "$stamp"
elif [[ "$pristine" == yes ]]; then
	/usr/bin/stat -f '%m %z' "$image" >"$stamp"
fi
# 버린 블록(discard)은 Mac 파일에서도 비운다. snapshot이면 쓴 것은 임시 파일에 가고 끄면 지워진다
drive="if=virtio,format=raw,file=$image,discard=unmap"
if [[ "$keep" == no ]]; then
	drive+=",snapshot=on"
fi

# zoom-to-fit은 쓰지 않는다. cocoa는 창 크기를 게스트에 해상도로 알려 주는데, zoom-to-fit을 켜면
# 처음 창(640x480)을 화면 비율에만 맞춰 640x400이 된다. bunt-comp는 선호 해상도가 바뀌면 따라가지만
# (VNC로 확인), cocoa 창 크기 바꾸기로는 아직 확인하지 않았다
exec qemu-system-aarch64 \
	-name buntOS \
	-machine virt -accel hvf -cpu host -smp 4 -m 4G \
	-drive "if=pflash,format=raw,readonly=on,file=$firmware" \
	-drive "$drive" \
	-device virtio-gpu-pci -device virtio-keyboard-pci -device virtio-tablet-pci \
	-audiodev coreaudio,id=snd0 -device virtio-sound-pci,audiodev=snd0,streams=1 \
	-nic user,model=virtio-net-pci \
	${credential_flag:+"$credential_flag" "$credential_value"} \
	-display "$display"
