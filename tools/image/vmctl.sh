#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 빌드한 이미지를 화면, 직렬 콘솔, QMP와 함께 띄워 두고 다른 명령에서 조작한다.
# 화면을 보며 기능을 확인할 때 쓴다. 빌드 VM 안에서 실행한다. 직렬 콘솔은 root로 자동 로그인한다.
# 사용법 (VMCTL_ID로 이름을 주면 여러 대를 따로 띄운다. 기본 dbg):
#   tools/image/vmctl.sh start        VM이 끝날 때까지 돌아오지 않으므로 백그라운드로 실행한다
#   tools/image/vmctl.sh wait-login   직렬 콘솔의 자동 로그인을 기다린 뒤, 로그인 화면에 시험 계정의 암호를
#                                     쳐 그래픽 세션에 들어간다 (VMCTL_GREETER_LOGIN=0이면 들어가지 않는다)
#   tools/image/vmctl.sh login        로그인 화면에 시험 계정의 암호를 쳐 그래픽 세션에 들어간다
#   tools/image/vmctl.sh sh '명령'     직렬 콘솔에서 root로 실행하고 출력을 보인다 (SH_TIMEOUT초, 기본 120)
#   tools/image/vmctl.sh push 파일 경로  빌드 VM의 파일을 게스트의 경로로 넣는다 (실행 권한을 주고 sha256으로 견준다)
#   tools/image/vmctl.sh qmp 동작...   tools/image/qmp.py 동작 (click:X,Y, type:글자, keys:A+B, screen:경로,
#                                     VMCTL_TOUCH=1이면 touch:X,Y와 tdrag:X1,Y1,X2,Y2 ...)
#   tools/image/vmctl.sh stop
# 시험 계정은 부팅 때 systemd 자격 증명 buntos.account로 만든다. 기본은 buntos(uid 1000, 암호 buntos,
# 관리자)이고, VMCTL_ACCOUNT="계정 이름:별칭:암호"로 바꾼다. VMCTL_ACCOUNT=를 비워 주면 계정 없이
# 띄워 처음 켤 때의 계정 만들기 화면이 나온다. 암호는 QMP로 치므로 영문 소문자와 숫자만 쓴다.
# 빌드 VM 메모리를 넘지 않게 한 번에 VMCTL_SLOTS대(기본 2)까지만 띄우고, 자리가 없으면 start가 기다린다.
# 이미지 빌드(tools/image/build.sh)가 도는 동안에는 띄우지 않고 기다린다. 띄운 뒤에는 빌드가
# 이미지 파일을 새로 만들어도 이미 연 파일을 계속 쓰므로 영향이 없다.
# VMCTL_QEMU_ARGS로 QEMU 인자를 더할 수 있다 (예: -vnc unix:/경로/vnc.sock).
# 디스크(임시 덮어쓰기)는 VMCTL_DISK_SIZE(기본 16G)이고, buntOS가 부팅 때 루트를 디스크 끝까지 늘린다.
# 덮어쓰기는 쓴 만큼만 자리를 차지한다. 빈 값을 주면 이미지 크기 그대로 띄운다.
# BUNTOS_IMAGE_DIR을 주면 그 디렉터리의 이미지를 띄운다 (tools/image/build.sh와 같다).
# VMCTL_GPU=virgl이면 가상 GPU를 3D(virtio-gpu-gl, virglrenderer)로 붙여 게스트가 GPU 경로(linux-dmabuf,
# GTK의 GL)로 그린다. 게스트의 GL은 이 빌드 VM이 그린다. 화면은 egl-headless로 받아 QMP 화면 저장이 된다.
# 빌드 VM에 렌더 노드(/dev/dri/renderD*, VMCTL_RENDERNODE로 고른다)와 QEMU의 OpenGL 모듈
# (qemu-system-modules-opengl, tools/devvm/setup.sh)이 있어야 한다. GPU가 없는 빌드 VM(Lima)은 vgem 커널
# 모듈로 렌더 노드를 만들고, 그때 빌드 VM은 llvmpipe(CPU)로 그린다. 기본값(빈 값)은 3D 없는 virtio-gpu다.
# VMCTL_INPUT_HOTPLUG=1이면 키보드와 태블릿을 PCIe 루트 포트에 id keyboard, tablet으로 꽂고 빈 포트(spare-port)를
# 하나 더 두어, QMP device_del로 떼고 device_add로 다시 붙일 수 있다(입력 모드 시험). 기본값은 루트 버스(pcie.0)에
# 바로 꽂아 뗄 수 없다("Bus 'pcie.0' does not support hotplugging")
# 보드는 BOARD(기본 qemu-aarch64)로 고른다. BOARD=x86_64-generic이면 q35 기계와 OVMF로 띄우고, 빌드 VM과
# 아키텍처가 다르면(Apple Silicon의 빌드 VM) KVM 없이 소프트웨어 에뮬레이션(TCG)으로 띄워 느리다.
# VMCTL_TOUCH=1이면 가상 터치 화면(virtio-multitouch, QEMU 8.1 이상)도 붙인다. qmp 동작 touch:·tdrag:이 손가락으로
# 누르고 끈다. 가상 키보드와 태블릿은 그대로 있으므로 입력 모드는 auto에서 pointer다.
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
board=${BOARD:-qemu-aarch64}
build_root="${BUNTOS_BUILD_DIR:-$HOME/buntos-build}"
work="$build_root/vm-${VMCTL_ID:-dbg}"

# 보드의 아키텍처에 따른 QEMU와 UEFI 펌웨어(tools/devvm/setup.sh의 qemu-efi-aarch64, ovmf). 전에는 aarch64로
# 정해 두었다. x86-64는 보안 부팅 없는 OVMF라 SMM이 필요 없다
case "$(sed -n 's/^Architecture=//p' "$repo/bsp/$board/mkosi.conf" 2>/dev/null)" in
arm64)
	qemu=qemu-system-aarch64 machine=virt host_arch=aarch64
	firmware_code=/usr/share/AAVMF/AAVMF_CODE.fd firmware_vars=/usr/share/AAVMF/AAVMF_VARS.fd
	;;
x86-64)
	qemu=qemu-system-x86_64 machine=q35 host_arch=x86_64
	firmware_code=/usr/share/OVMF/OVMF_CODE_4M.fd firmware_vars=/usr/share/OVMF/OVMF_VARS_4M.fd
	;;
*)
	echo "bsp/$board/mkosi.conf의 Architecture=를 모른다" >&2
	exit 2
	;;
esac
# 빌드 VM과 아키텍처가 같고 KVM이 있으면 KVM, 아니면 TCG로 돌린다
if [[ "$(uname -m)" == "$host_arch" && -e /dev/kvm ]]; then
	accel=(-accel kvm -cpu host)
else
	accel=(-accel tcg -cpu max)
fi

clean() { sed -E 's/\x1b\[[0-9;?]*[A-Za-z]//g; s/\r//g' "$work/console.log"; }

# 직렬 콘솔에서 root로 실행하고 출력을 보인다 (SH_TIMEOUT초, 기본 120)
console_run() {
	local marker before
	marker="DBG${RANDOM}${RANDOM}"
	before=$(clean | wc -l)
	printf '%s; echo "%s_$((40+2))"\n' "$*" "$marker" >"$work/console.in"
	for _ in $(seq 1 "${SH_TIMEOUT:-120}"); do
		if clean | grep -a -q "^${marker}_42$"; then
			# 출력이 없으면 grep이 1로 끝나지만 실패가 아니다
			clean | tail -n +"$((before + 1))" | sed -n "/^${marker}_42$/q;p" | grep -a -v "${marker}_\$((40+2))" || true
			return 0
		fi
		sleep 1
	done
	echo "timeout" >&2
	clean | tail -n +"$((before + 1))" | tail -20 >&2
	return 1
}

# 로그인 화면에 시험 계정의 암호를 쳐 그래픽 세션에 들어간다. 키가 로그인 화면보다 먼저 가거나 VM이 바빠
# 몇 개가 빠지면 들어가지 못하므로, 세션이 뜨지 않으면 Esc(친 암호를 지운다)를 누르고 다시 친다
greeter_login() {
	local account user password
	account=$(cat "$work/account" 2>/dev/null || true)
	if [[ -z "$account" ]]; then
		echo "시험 계정 없이 띄웠다 (VMCTL_ACCOUNT)" >&2
		return 1
	fi
	user=${account%%:*}
	password=${account#*:*:}
	SH_TIMEOUT=150 console_run 'for i in $(seq 120); do journalctl -b -q -t bunt-greeter --no-pager 2>/dev/null | grep -q "bunt-greeter: ready (login)" && break; sleep 1; done' >/dev/null
	for _ in 1 2 3; do
		python3 -I "$repo/tools/image/qmp.py" "$work/qmp.sock" keys:esc sleep:0.5 "type:$password" sleep:0.5 keys:ret
		# 입력 줄의 에코와 구분되도록 확인 값은 셸이 계산하게 한다
		if SH_TIMEOUT=60 console_run "for i in \$(seq 40); do pgrep -u $user -x bunt-shell >/dev/null && break; sleep 1; done; pgrep -u $user -x bunt-shell >/dev/null && echo SESSION_\$((1 + 1))_UP" | grep -q -x SESSION_2_UP; then
			echo "logged in as $user"
			return 0
		fi
	done
	echo "로그인 화면에서 들어가지 못했다" >&2
	return 1
}

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
	disk_size=${VMCTL_DISK_SIZE-16G}
	qemu-img create -q -f qcow2 -F raw -b "$image" "$work/disk.qcow2" ${disk_size:+"$disk_size"}
	exec 7>&-
	cp "$firmware_vars" "$work/vars.fd"
	account=${VMCTL_ACCOUNT-buntos:buntOS:buntos}
	printf '%s' "$account" >"$work/account"
	credential=()
	if [[ -n "$account" ]]; then
		credential=(-smbios "type=11,value=io.systemd.credential:buntos.account=$account")
	fi
	cmdline="systemd.wants=network.target rw loglevel=4 console=hvc0 TERM=dumb"
	cmdline+=" systemd.tty.term.hvc0=dumb systemd.tty.columns.hvc0=80 systemd.tty.rows.hvc0=24"
	# 가상 GPU. virgl은 게스트의 GL을 이 빌드 VM이 그리고, 화면을 egl-headless로 받는다(-nographic 대신)
	case "${VMCTL_GPU:-}" in
	"")
		gpu=(-device virtio-gpu-pci -nographic)
		;;
	virgl)
		if ! "$qemu" -device help 2>/dev/null | grep -q '"virtio-gpu-gl-pci"'; then
			echo "QEMU에 virtio-gpu-gl-pci가 없다. qemu-system-modules-opengl을 설치한다(tools/devvm/setup.sh)" >&2
			exit 1
		fi
		rendernode=${VMCTL_RENDERNODE:-$(find /dev/dri -name 'renderD*' 2>/dev/null | sort | head -n 1)}
		if [[ -z "$rendernode" ]]; then
			echo "빌드 VM에 렌더 노드(/dev/dri/renderD*)가 없다. GPU가 없으면 vgem 커널 모듈로 만든다(sudo modprobe vgem)." >&2
			echo "vgem이 없는 커널이면 VMCTL_RENDERNODE=/dev/null로 Mesa의 소프트웨어 GBM을 써 본다(QEMU의 EGL 초기화까지만 확인했다)" >&2
			exit 1
		fi
		if [[ ! -r "$rendernode" || ! -w "$rendernode" ]]; then
			echo "$rendernode 을 열 수 없다. render 그룹에 넣고 다시 로그인한다: sudo usermod -aG render $USER" >&2
			exit 1
		fi
		gpu=(-device virtio-gpu-gl-pci -display "egl-headless,rendernode=$rendernode")
		;;
	*)
		echo "VMCTL_GPU는 비우거나 virgl이다" >&2
		exit 2
		;;
	esac
	# 가상 터치 화면. 태블릿보다 앞에 붙인다: QEMU는 단추 이벤트를 단추를 받는 장치 가운데 게스트가 마지막에 켠 것에
	# 보내므로, 태블릿이 나중에 켜져야 click:의 누르기가 태블릿으로 간다. qmp.py도 누르기 전에 태블릿을 앞에 둔다
	touch=()
	case "${VMCTL_TOUCH:-}" in
	"" | 0) ;;
	1)
		if ! "$qemu" -device help 2>/dev/null | grep -q '"virtio-multitouch-pci"'; then
			echo "QEMU에 virtio-multitouch-pci가 없다(QEMU 8.1 이상)" >&2
			exit 1
		fi
		touch=(-device virtio-multitouch-pci)
		;;
	*)
		echo "VMCTL_TOUCH는 비우거나 1이다" >&2
		exit 2
		;;
	esac
	# 키보드, 터치 화면, 태블릿 차례로 붙인다(태블릿이 마지막이어야 한다, 위). 떼고 붙이는 시험(VMCTL_INPUT_HOTPLUG=1)
	# 에서는 키보드와 태블릿을 루트 포트에 id를 붙여 꽂는다(머리말). 터치 화면은 떼지 않으므로 루트 버스에 둔다
	input=(-device virtio-keyboard-pci "${touch[@]}" -device virtio-tablet-pci)
	if [[ "${VMCTL_INPUT_HOTPLUG:-}" == 1 ]]; then
		input=(
			-device "pcie-root-port,id=keyboard-port,chassis=1,slot=1"
			-device "virtio-keyboard-pci,id=keyboard,bus=keyboard-port"
			"${touch[@]}"
			-device "pcie-root-port,id=tablet-port,chassis=2,slot=2"
			-device "virtio-tablet-pci,id=tablet,bus=tablet-port"
			-device "pcie-root-port,id=spare-port,chassis=3,slot=3"
		)
	fi
	# shellcheck disable=SC2086 # VMCTL_QEMU_ARGS는 일부러 낱말로 나눈다
	"$qemu" -machine "$machine" "${accel[@]}" -smp "${VMCTL_CPUS:-2}" -m 2048M \
		-nodefaults -no-user-config \
		-object rng-random,filename=/dev/urandom,id=rng0 -device virtio-rng-pci,rng=rng0 \
		-nic user,model=virtio-net-pci \
		-chardev stdio,mux=on,id=console,signal=off \
		-device virtio-serial-pci -device virtconsole,chardev=console -mon console \
		-drive if=pflash,format=raw,readonly=on,file="$firmware_code" \
		-drive if=pflash,format=raw,file="$work/vars.fd" \
		-drive if=none,id=disk,format=qcow2,discard=unmap,file="$work/disk.qcow2" \
		-device virtio-blk-pci,drive=disk,bootindex=1 \
		-smbios "type=11,value=io.systemd.stub.kernel-cmdline-extra=$cmdline" "${credential[@]}" \
		"${gpu[@]}" "${input[@]}" \
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
	logged_in=
	for _ in $(seq 1 240); do
		clean | grep -a -q "automatic login" && { logged_in=1; break; }
		sleep 0.5
	done
	if [[ -z "$logged_in" ]]; then
		echo "no login" >&2
		exit 1
	fi
	echo "logged in"
	if [[ -s "$work/account" && "${VMCTL_GREETER_LOGIN:-1}" != 0 ]]; then
		sleep 2
		greeter_login
	fi
	;;
login)
	greeter_login
	;;
sh)
	shift
	console_run "$@"
	;;
push)
	# 콘솔은 4KB 가까운 줄을 자르므로 base64를 1000자씩 나눠 보내고, 64줄마다 게스트가 다 받기를 기다린다.
	# 다 넣은 뒤 sha256으로 견준다. 시험용 프로그램(예: comp/examples/glass-window)을 넣을 때 쓴다
	source_file=${2:?"넣을 파일이 필요하다"}
	target=${3:?"게스트의 경로가 필요하다"}
	console_run "rm -f '$target.b64'" >/dev/null
	lines=0
	while IFS= read -r chunk; do
		printf 'echo %s >>%s.b64\n' "$chunk" "'$target'" >"$work/console.in"
		lines=$((lines + 1))
		if ((lines % 64 == 0)); then
			console_run true >/dev/null
		fi
	done < <(base64 -w 1000 "$source_file")
	expected=$(sha256sum "$source_file" | cut -d' ' -f1)
	# 게스트에서 실패하면 해시가 없다. 그때도 끝내지 않고 아래에서 알린다
	actual=$(console_run "base64 -d '$target.b64' >'$target' && rm -f '$target.b64' && chmod 0755 '$target' && sha256sum '$target'" |
		grep -a -o -E '^[0-9a-f]{64}' | head -n 1 || true)
	if [[ "$actual" != "$expected" ]]; then
		echo "게스트의 $target 이 보낸 파일과 다르다" >&2
		exit 1
	fi
	echo "pushed $target ($lines lines)"
	;;
qmp)
	shift
	python3 -I "$repo/tools/image/qmp.py" "$work/qmp.sock" "$@"
	;;
stop)
	kill "$(cat "$work/vm.pid")" 2>/dev/null || true
	;;
*)
	# 머리말의 사용법을 보인다. 전에는 5~24줄로 정해 두어 머리말이 늘면 잘렸다. 이제 set 줄 앞까지 읽는다
	sed -n '5,/^set -euo pipefail/p' "${BASH_SOURCE[0]}" | sed '$d; s/^# \{0,1\}//' >&2
	exit 2
	;;
esac
