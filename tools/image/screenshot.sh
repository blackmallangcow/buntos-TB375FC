#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 빌드한 이미지를 가상 화면 장치와 함께 띄우고, 기다린 뒤 QMP 동작을 실행한다.
# 빌드 VM 안에서 실행한다.
# 사용법: tools/image/screenshot.sh <보드> [기다릴 초, 기본 60] [qmp.py 동작...]
#   동작을 주지 않으면 화면만 저장한다: $BUNTOS_BUILD_DIR/<보드>/screen.png
#   예: tools/image/screenshot.sh qemu-aarch64 45 click:640,755 sleep:5 screen:/tmp/after.png
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
board=${1:?"보드 이름이 필요하다 (bsp/ 아래 디렉터리 이름)"}
wait_s=${2:-60}
shift $(($# >= 2 ? 2 : $#))
out="${BUNTOS_BUILD_DIR:-$HOME/buntos-build}/$board"
if [[ $# -eq 0 ]]; then
	rm -f "$out/screen.png"
	set -- "screen:$out/screen.png"
fi

work=$(mktemp -d)
cleanup() {
	if [[ -n "${vm_pid:-}" ]] && kill -0 "$vm_pid" 2>/dev/null; then
		kill "$vm_pid" 2>/dev/null || true
	fi
	rm -rf "$work"
}
trap cleanup EXIT
qmp() {
	python3 -I "$repo/tools/image/qmp.py" "$work/qmp.sock" "$@"
}

# 빌드한 이미지를 바꾸지 않게, 멈춘 상태로 띄워 디스크에 임시 덮어쓰기를 씌운 뒤 실행한다
# (tests/e2e/boot-to-shell.sh 참고)
"$repo/tools/image/build.sh" "$board" vm -- -S \
	-device virtio-gpu-pci -device virtio-keyboard-pci -device virtio-tablet-pci \
	-qmp "unix:$work/qmp.sock,server=on,wait=off" </dev/null >"$work/console.log" 2>&1 &
vm_pid=$!
qmp "overlay:$work/disk.qcow2" cont

sleep "$wait_s"
if ! kill -0 "$vm_pid" 2>/dev/null; then
	echo "VM이 먼저 끝났다. 콘솔 마지막 20줄:" >&2
	tail -20 "$work/console.log" >&2
	exit 1
fi
qmp "$@"
