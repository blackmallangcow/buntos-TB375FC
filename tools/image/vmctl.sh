#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 빌드한 이미지를 화면, 직렬 콘솔, QMP와 함께 띄워 두고 다른 명령에서 조작한다.
# 화면을 보며 기능을 확인할 때 쓴다. 빌드 VM 안에서 실행한다. 직렬 콘솔은 root로 자동 로그인한다.
# 사용법 (VMCTL_ID로 이름을 주면 여러 대를 따로 띄운다. 기본 dbg):
#   tools/image/vmctl.sh start        VM이 끝날 때까지 돌아오지 않으므로 백그라운드로 실행한다
#   tools/image/vmctl.sh wait-login   직렬 콘솔의 자동 로그인을 기다린다
#   tools/image/vmctl.sh sh '명령'     직렬 콘솔에서 root로 실행하고 출력을 보인다 (SH_TIMEOUT초, 기본 120)
#   tools/image/vmctl.sh qmp 동작...   tools/image/qmp.py 동작 (click:X,Y, type:글자, keys:A+B, screen:경로 ...)
#   tools/image/vmctl.sh stop
# 빌드 VM 메모리를 넘지 않게 한 번에 VMCTL_SLOTS대(기본 2)까지만 띄우고, 자리가 없으면 start가 기다린다.
# 이미지 빌드(tools/image/build.sh)가 도는 동안에는 띄우지 않고 기다린다. 띄운 뒤에는 빌드가
# 이미지 파일을 새로 만들어도 이미 연 파일을 계속 쓰므로 영향이 없다.
# VMCTL_QEMU_ARGS로 QEMU 인자를 더할 수 있다 (예: -vnc unix:/경로/vnc.sock).
# BUNTOS_IMAGE_DIR을 주면 그 디렉터리의 이미지를 띄운다 (tools/image/build.sh와 같다).
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
board=${BOARD:-qemu-aarch64}
build_root="${BUNTOS_BUILD_DIR:-$HOME/buntos-build}"
work="$build_root/vm-${VMCTL_ID:-dbg}"

clean() { sed -E 's/\x1b\[[0-9;?]*[A-Za-z]//g; s/\r//g' "$work/console.log"; }

case "${1:-}" in
start)
	rm -rf "$work"
	mkdir -p "$work"
	mkfifo "$work/console.in"
	# 자리 하나가 잠금 파일 하나다. 이 프로세스가 VM이 끝날 때까지 쥐고 있는다
	while :; do
		for slot in $(seq 1 "${VMCTL_SLOTS:-2}"); do
			exec 8>"$build_root/.vm-slot-$slot.lock"
			if flock -n 8; then
				break 2
			fi
		done
		sleep 5
	done
	# 빌드 중이면 끝날 때까지 기다렸다가, 빌드한 이미지 위에 임시 덮어쓰기(qcow2)를 만든다.
	# mkosi vm은 VM이 도는 동안 이미지를 잠가 빌드를 막으므로 QEMU를 직접 띄운다(인자는 mkosi
	# 27의 vm과 같다). 빌드가 이미지를 새로 만들어도 QEMU는 이미 연 파일을 계속 쓴다
	image_dir="${BUNTOS_IMAGE_DIR:-$build_root/$board}"
	exec 7>"$image_dir.lock"
	flock -s 7
	image="$image_dir/buntos.raw"
	qemu-img create -q -f qcow2 -F raw -b "$image" "$work/disk.qcow2"
	exec 7>&-
	cp /usr/share/AAVMF/AAVMF_VARS.fd "$work/vars.fd"
	cmdline="systemd.wants=network.target rw loglevel=4 console=hvc0 TERM=dumb"
	cmdline+=" systemd.tty.term.hvc0=dumb systemd.tty.columns.hvc0=80 systemd.tty.rows.hvc0=24"
	# shellcheck disable=SC2086 # VMCTL_QEMU_ARGS는 일부러 낱말로 나눈다
	qemu-system-aarch64 -machine virt -accel kvm -cpu host -smp "${VMCTL_CPUS:-2}" -m 2048M \
		-nodefaults -nographic -no-user-config \
		-object rng-random,filename=/dev/urandom,id=rng0 -device virtio-rng-pci,rng=rng0 \
		-nic user,model=virtio-net-pci \
		-chardev stdio,mux=on,id=console,signal=off \
		-device virtio-serial-pci -device virtconsole,chardev=console -mon console \
		-drive if=pflash,format=raw,readonly=on,file=/usr/share/AAVMF/AAVMF_CODE.fd \
		-drive if=pflash,format=raw,file="$work/vars.fd" \
		-drive if=none,id=disk,format=qcow2,discard=unmap,file="$work/disk.qcow2" \
		-device virtio-blk-pci,drive=disk,bootindex=1 \
		-smbios "type=11,value=io.systemd.stub.kernel-cmdline-extra=$cmdline" \
		-device virtio-gpu-pci -device virtio-keyboard-pci -device virtio-tablet-pci \
		-audiodev none,id=snd0 -device virtio-sound-pci,audiodev=snd0 \
		-qmp "unix:$work/qmp.sock,server=on,wait=off" ${VMCTL_QEMU_ARGS:-} \
		<"$work/console.in" >"$work/console.log" 2>&1 8>&- &
	vm_pid=$!
	echo "$vm_pid" >"$work/vm.pid"
	exec 3>"$work/console.in"
	wait "$vm_pid" || true
	echo "VM exited"
	;;
wait-login)
	for _ in $(seq 1 240); do
		clean | grep -a -q "automatic login" && { echo "logged in"; exit 0; }
		sleep 0.5
	done
	echo "no login" >&2
	exit 1
	;;
sh)
	shift
	marker="DBG${RANDOM}${RANDOM}"
	before=$(clean | wc -l)
	printf '%s; echo "%s_$((40+2))"\n' "$*" "$marker" >"$work/console.in"
	for _ in $(seq 1 "${SH_TIMEOUT:-120}"); do
		if clean | grep -a -q "^${marker}_42$"; then
			clean | tail -n +"$((before + 1))" | sed -n "/^${marker}_42$/q;p" | grep -a -v "${marker}_\$((40+2))"
			exit 0
		fi
		sleep 1
	done
	echo "timeout" >&2
	clean | tail -n +"$((before + 1))" | tail -20 >&2
	exit 1
	;;
qmp)
	shift
	python3 -I "$repo/tools/image/qmp.py" "$work/qmp.sock" "$@"
	;;
stop)
	kill "$(cat "$work/vm.pid")" 2>/dev/null || true
	;;
*)
	sed -n '5,18p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2
	exit 2
	;;
esac
