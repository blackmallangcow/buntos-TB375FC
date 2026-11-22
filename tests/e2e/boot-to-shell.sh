#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# e2e 1번: 부팅 → 로그인 → 셸 (ARCHITECTURE 9.2. ADR-009의 Phase 0 드라이버: 시리얼 콘솔과 QMP)
# 빌드한 이미지를 시험 계정과 함께 띄운다(systemd 자격 증명 buntos.account를 SMBIOS로 넘긴다).
# 직렬 콘솔의 root 자동 로그인 뒤 셸에서 명령이 실행되는지 보고, 로그인 화면(bunt-greeter)에 QMP로
# 암호를 쳐 그 계정으로 들어간다. sudo가 암호를 묻는지 보고, 그래픽 셸(메뉴 막대, Dock)이 떴는지는
# 에이전트의 읽기 경로(bunt agent snapshot, ADR-009)로 본다. 이어서 e2e 8번(데스크탑 항목, tests/e2e/desktop.sh),
# e2e 5번(에이전트가 앱 A의 값을 읽어 앱 B에 넣는다, tests/e2e/agent-apps.sh), e2e 6번(키보드 단축키,
# tests/e2e/shortcuts.sh), e2e 4번(한글 입력, tests/e2e/hangul.sh), e2e 7번(화면 잠금과 VM의 잠자기,
# tests/e2e/session-lock.sh), e2e 10번(클립보드, tests/e2e/clipboard.sh), e2e 9번(패키지 매니저, tests/e2e/packages.sh),
# e2e 11번(화면 키보드와 단축 줄, tests/e2e/touch-keyboard.sh)을 돌린 뒤 끈다. 자체 컴포넌트가 없는 이미지(그래픽 세션이
# 없는 x86-64)에서는 8·5·6·4·7·10·9·11번을 건너뛴다.
# 사용법: tests/e2e/boot-to-shell.sh <보드>   (먼저 tools/image/build.sh <보드>로 빌드한다)
# 환경 변수: E2E_TIMEOUT 전체 제한 시간(초, 기본 1200. 바쁜 Mac에서는 8·5·6·4·7·10·9·11번에 10분 넘게 걸린다)
#            BUNTOS_IMAGE_DIR 띄울 이미지의 디렉터리 (tools/image/build.sh·vmctl.sh와 같다. 기본
#            $BUNTOS_BUILD_DIR/<보드>, BUNTOS_BUILD_DIR의 기본은 ~/buntos-build)
#            E2E_LOG_DIR 기록을 남길 디렉터리. 주면 끝날 때(실패해도) 직렬 콘솔 기록(console.log)과 게스트의 저널
#            (journal.log, 부팅부터)을 남기고, 실패하면 그때의 화면(screen.png)도 남긴다. CI가 아티팩트로 올린다
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
board=${1:?"보드 이름이 필요하다 (bsp/ 아래 디렉터리 이름)"}
deadline=$((SECONDS + ${E2E_TIMEOUT:-1200}))
# 다른 사람이 기본 이미지로 VM을 띄워 둔 동안 따로 빌드한 이미지를 시험할 수 있게 build.sh와 같은 디렉터리를 쓴다
image_dir="${BUNTOS_IMAGE_DIR:-${BUNTOS_BUILD_DIR:-$HOME/buntos-build}/$board}"
if [[ ! -f "$image_dir/buntos.raw" ]]; then
	echo "실패($board): $image_dir/buntos.raw가 없다. 먼저 tools/image/build.sh $board로 빌드한다" >&2
	exit 1
fi
echo "이미지: $image_dir/buntos.raw"
# 시험 계정 (계정 이름:별칭:암호). 다른 시험과 개발 VM 도구(tools/image/vmctl.sh)도 같은 계정을 쓴다.
# 암호는 QMP로 치므로 영문 소문자와 숫자만 쓴다
user=buntos
password=buntos
account="$user:buntOS:$password"

work=$(mktemp -d)
log="$work/console.log"
mkfifo "$work/console.in"
cleanup() {
	exec 3>&- 2>/dev/null || true
	if [[ -n "${vm_pid:-}" ]] && kill -0 "$vm_pid" 2>/dev/null; then
		kill "$vm_pid" 2>/dev/null || true
	fi
	# E2E_LOG_DIR: 지우기 전에 콘솔 기록과 게스트 저널을 남긴다
	if [[ -n "${E2E_LOG_DIR:-}" ]]; then
		console >"$E2E_LOG_DIR/console.log" 2>/dev/null || true
		cp "$work/journal.log" "$E2E_LOG_DIR/journal.log" 2>/dev/null || true
	fi
	rm -rf "$work"
}
trap cleanup EXIT

console() {
	sed -E 's/\x1b\[[0-9;?]*[A-Za-z]//g; s/\r//g' "$log"
}
# grep -q는 찾자마자 끝나 콘솔 기록이 파이프 버퍼보다 크면 sed가 SIGPIPE로 끝나고, pipefail 때문에 찾았는데도
# 못 찾은 것이 된다. 그래서 끝까지 읽어 센다
wait_for() {
	until (($(console | grep -a -c -E "$1") > 0)); do
		if ! kill -0 "$vm_pid" 2>/dev/null || ((SECONDS >= deadline)); then
			return 1
		fi
		sleep 0.2
	done
}
fail() {
	echo "실패($board): $1" >&2
	echo "--- 콘솔 마지막 40줄" >&2
	console | tail -40 >&2
	# E2E_LOG_DIR: 실패한 때의 화면을 남긴다 (QEMU가 파일을 쓴다)
	if [[ -n "${E2E_LOG_DIR:-}" ]] && kill -0 "${vm_pid:-0}" 2>/dev/null; then
		timeout 30 python3 -I "$repo/tools/image/qmp.py" "$work/qmp.sock" "screen:$E2E_LOG_DIR/screen.png" || true
	fi
	exit 1
}
# 저장소의 스크립트($1)를 게스트의 $2로 넣는다. 콘솔은 4KB 가까운 줄을 자르므로 base64를 1000자씩 나눠 보낸다.
# 게스트가 바쁘면 잇달아 보낸 입력이 빠져 한 줄이 다음 명령과 섞였다(2026-10-30). 그래서 줄마다 게스트의 답을
# 기다리고, 다 넣은 뒤 sha256을 견줘 다르면 두 번까지 다시 넣는다. 답은 산술 확장으로 만들어, 콘솔에 되비친
# 명령 줄이 답으로 읽히지 않게 한다
pushes=0
push() {
	local want n chunk
	want=$(sha256sum "$repo/$1" | cut -c1-16)
	for _ in 1 2 3; do
		pushes=$((pushes + 1))
		n=0
		echo "rm -f $2.b64" >&3
		while IFS= read -r chunk; do
			n=$((n + 1))
			echo "echo $chunk >>$2.b64; echo PUSH${pushes}_\$((0 + $n))" >&3
			wait_for "^PUSH${pushes}_$n\$" || fail "$1을 게스트에 넣지 못했다"
		done < <(base64 -w 1000 "$repo/$1")
		echo "base64 -d $2.b64 >$2; echo PUSHED${pushes}_\$(sha256sum <$2 | cut -c1-16)" >&3
		wait_for "^PUSHED${pushes}_[0-9a-f]{16}\$" || fail "$1을 게스트에 넣지 못했다"
		if (($(console | grep -a -c -E "^PUSHED${pushes}_$want\$") > 0)); then
			return
		fi
		echo "알림: $1의 sha256이 달라 다시 넣는다" >&2
	done
	fail "$1을 게스트에 넣지 못했다 (sha256이 다르다)"
}

# 테스트가 빌드한 이미지를 바꾸지 않게, 멈춘 상태로 띄워 디스크에 임시 덮어쓰기를 씌운 뒤 실행한다.
# mkosi는 이미지를 -blockdev로 붙여 QEMU의 -snapshot이 효과가 없고, --ephemeral은 이미지 전체를 복사한다
# 그래픽 세션이 뜨도록 가상 GPU를 붙이고(화면은 내보내지 않는다), 로그인 화면에 암호를 치도록 가상 키보드를,
# 데스크탑을 누르고 드래그하도록(e2e 8번) 가상 태블릿을 붙인다. 키보드와 태블릿은 PCIe 루트 포트에 꽂아 e2e 11번이 떼고
# 그 자리에 가상 터치 화면을 붙일 수 있게 한다(태블릿의 키보드 덮개를 뗀 것과 같다. 입력 모드가 터치가 된다)
# E2E_LOG_DIR이 있으면 게스트 저널을 받을 virtio 직렬 포트를 더한다(셸에 들어간 뒤 journalctl -f를 이 포트로 보낸다)
journal_port=()
if [[ -n "${E2E_LOG_DIR:-}" ]]; then
	mkdir -p "$E2E_LOG_DIR"
	journal_port=(-device "virtio-serial-pci,id=e2e-serial" -chardev "file,id=e2e-journal,path=$work/journal.log"
		-device "virtserialport,bus=e2e-serial.0,chardev=e2e-journal,name=org.buntos.e2e.journal")
fi
BUNTOS_IMAGE_DIR="$image_dir" "$repo/tools/image/build.sh" "$board" vm -- -S -qmp "unix:$work/qmp.sock,server=on,wait=off" \
	-device virtio-gpu-pci \
	-device pcie-root-port,id=e2e-keyboard-port,chassis=11,slot=11 \
	-device virtio-keyboard-pci,id=e2e-keyboard,bus=e2e-keyboard-port \
	-device pcie-root-port,id=e2e-tablet-port,chassis=12,slot=12 \
	-device virtio-tablet-pci,id=e2e-tablet,bus=e2e-tablet-port \
	-device pcie-root-port,id=e2e-touch-port,chassis=13,slot=13 "${journal_port[@]}" \
	-smbios "type=11,value=io.systemd.credential:buntos.account=$account" \
	<"$work/console.in" >"$log" 2>&1 &
vm_pid=$!
exec 3>"$work/console.in"
python3 -I "$repo/tools/image/qmp.py" "$work/qmp.sock" "overlay:$work/disk.qcow2" cont ||
	fail "디스크에 임시 덮어쓰기를 씌우지 못했다"

wait_for "automatic login" || fail "콘솔 자동 로그인까지 가지 못했다"
echo "로그인: ${SECONDS}초"
sleep 2

# 입력 줄의 에코와 구분되도록 확인 값은 셸이 계산하게 한다
echo 'echo "E2E_SHELL_$((6*7))"; systemctl is-system-running' >&3
wait_for "^E2E_SHELL_42$" || fail "셸에서 명령 실행 결과를 받지 못했다"
echo "셸: ${SECONDS}초"
wait_for "^(running|degraded|starting|maintenance)$" || true
system_state=$(console | grep -a -E "^(running|degraded|starting|maintenance)$" | tail -1)
echo "시스템 상태: $system_state"
if [[ "$system_state" == degraded ]]; then
	# 실패한 유닛을 보여 준다. 입력 줄의 에코와 구분되도록 표시는 셸이 계산하게 한다
	echo 'systemctl --failed --no-legend --plain | sed "s/^/E2E_$((1 + 1))F /"; echo "E2E_DONE_$((2 * 3))"' >&3
	wait_for "^E2E_DONE_6$" || true
	console | grep -a "^E2E_2F " | sed 's/^E2E_2F /실패한 유닛: /'
fi
# 스왑 파일(설계안 8.1절): 디스크에 남는 공간이 있으면 처음 켤 때 만들어 켜고, 모자라면 만들지 않는다. 어느 쪽이든 두 유닛은
# 실패하지 않아야 한다. 표시는 셸이 계산하게 한다(입력 줄의 에코와 구분된다)
echo 'set -- $(systemctl is-failed buntos-swapfile.service swap-swapfile.swap); echo "E2E_SWAP_$((0 + 1)) $1 $2 $(swapon --show=NAME,SIZE --noheadings | tr -s " " | head -n 1)"' >&3
wait_for "^E2E_SWAP_1 " || fail "스왑 파일 유닛의 상태를 읽지 못했다"
swap_line=$(console | grep -a "^E2E_SWAP_1 " | tail -1)
[[ "$swap_line" != *failed* ]] || fail "스왑 파일 유닛이 실패했다: ${swap_line#E2E_SWAP_1 }"
echo "스왑(만드는 서비스, 스왑 유닛, 켜진 스왑): ${swap_line#E2E_SWAP_1 }"
if [[ -n "${E2E_LOG_DIR:-}" ]]; then
	# 게스트 저널을 부팅부터 끝까지 저널 포트로 보낸다. 시험이 어디서 멈춰도 그때까지의 저널이 남는다. 시스템
	# 상태를 읽은 뒤에 보낸다(바로 보내면 이 명령의 메아리가 systemctl의 답 줄에 끼어 상태 줄을 찾지 못하고
	# 제한 시간까지 기다렸다)
	echo 'setsid -f sh -c "exec journalctl -b -f -o short-monotonic --no-pager >/dev/virtio-ports/org.buntos.e2e.journal 2>&1"' >&3
fi

# 로그인 화면: 계정은 부팅 때 자격 증명으로 만들어졌다(buntos-account.service). 로그인 화면이 뜨면 QMP로
# 암호를 치고 Enter를 누른다. 키가 로그인 화면보다 먼저 가거나 VM이 바빠 몇 개가 빠지면 들어가지 못하므로,
# 세션이 뜨지 않으면 Esc(친 암호를 지운다)를 누르고 다시 친다.
# 자체 컴포넌트를 넣지 않은 이미지(다른 아키텍처로 빌드한 이미지)에는 로그인 화면이 없어 건너뛴다
echo "if command -v bunt-greeter >/dev/null; then echo \"E2E_ACCOUNT_\$(id -u $user 2>/dev/null || echo none)\"; for i in \$(seq 120); do journalctl -b -q -t bunt-greeter --no-pager 2>/dev/null | grep -q 'bunt-greeter: ready (login)' && break; sleep 1; done; echo \"E2E_GREETER_\$(journalctl -b -q -t bunt-greeter --no-pager 2>/dev/null | grep -c 'bunt-greeter: ready (login)')\"; else echo E2E_GREETER_NONE; fi" >&3
wait_for "^E2E_GREETER_([0-9]+|NONE)$" || fail "로그인 화면을 기다리지 못했다"
greeter=$(console | grep -a -E "^E2E_GREETER_([0-9]+|NONE)$" | tail -1 | sed 's/^E2E_GREETER_//')
if [[ "$greeter" == NONE ]]; then
	echo "로그인 화면: 이 이미지에는 로그인 화면이 없어 건너뛴다"
else
	uid=$(console | grep -a -E "^E2E_ACCOUNT_([0-9]+|none)$" | tail -1 | sed 's/^E2E_ACCOUNT_//')
	[[ "$uid" == 1000 ]] || fail "자격 증명으로 만든 시험 계정이 없다 (uid $uid)"
	((greeter > 0)) || fail "로그인 화면이 뜨지 않았다"
	echo "로그인 화면: ${SECONDS}초"
	# 접근성: 사용자가 허용한 AI가 있으면 세션을 열 때 켠다(buntd-agent --prepare-session). Chromium·Electron
	# 앱은 뜰 때만 보기 때문이다. 시험용 AI 하나를 정책에 넣고 들어간다(e2e의 에이전트 요청은 root라 늘 허용된다)
	policy=$(printf 'version = 1\n\n[[client]]\nname = "e2e"\nexe = "/usr/bin/true"\n' | base64 -w 0)
	echo "mkdir -p /etc/buntos && echo $policy | base64 -d >/etc/buntos/agent-access.toml" >&3
	for attempt in 1 2 3; do
		python3 -I "$repo/tools/image/qmp.py" "$work/qmp.sock" keys:esc sleep:0.5 "type:$password" sleep:0.5 keys:ret ||
			fail "QMP로 암호를 치지 못했다"
		echo "for i in \$(seq 40); do pgrep -u $user -x bunt-shell >/dev/null && break; sleep 1; done; echo \"E2E_LOGIN${attempt}_\$(pgrep -c -u $user -x bunt-shell)\"" >&3
		wait_for "^E2E_LOGIN${attempt}_[0-9]+$" || fail "로그인 결과를 받지 못했다"
		if console | grep -a -q -E "^E2E_LOGIN${attempt}_[1-9][0-9]*$"; then
			break
		fi
		((attempt < 3)) || fail "로그인 화면에서 암호로 들어가지 못했다"
	done
	echo "로그인(로그인 화면, ${attempt}번째): ${SECONDS}초"

	# 화면 넘겨받기(comp/src/handover.rs): 사용자 세션의 컴포지터가 로그인 화면의 CRTC와 모드를 끄지 않고 이어받는다
	echo "echo \"E2E_HANDOVER_\$(journalctl -b -q -t bunt-comp | grep -c 'keeping the mode of the previous compositor')\"" >&3
	wait_for "^E2E_HANDOVER_[0-9]+$" || fail "화면 넘겨받기를 확인하지 못했다"
	console | grep -a -q -E "^E2E_HANDOVER_[1-9][0-9]*$" || fail "로그인할 때 컴포지터가 모드를 다시 설정했다"
	echo "화면 넘겨받기: 로그인할 때 모드를 다시 설정하지 않았다"

	# 보안: 개발용 암호 없는 sudo가 없어야 한다. sudo -n은 암호가 필요하면 바로 실패한다
	echo "echo \"E2E_SUDO_\$(sudo -u $user sudo -n true 2>&1 | grep -c 'password is required')\"" >&3
	wait_for "^E2E_SUDO_[0-9]+$" || fail "sudo를 확인하지 못했다"
	console | grep -a -q -E "^E2E_SUDO_1$" || fail "sudo가 암호를 묻지 않는다"
	echo "sudo: 암호를 묻는다"

	# 에이전트에 처음 묻기 전에 본다(데몬도 첫 요청 때 켠다). 접근성 버스는 로그인 중에 몇 초 늦게 뜬다
	a11y="sudo -u $user DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus busctl --user get-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled"
	echo "for i in \$(seq 30); do $a11y 2>/dev/null | grep -q true && break; sleep 1; done; echo \"E2E_A11Y_\$($a11y 2>/dev/null | tr -d ' ')\"" >&3
	wait_for "^E2E_A11Y_" || fail "접근성 상태를 읽지 못했다"
	console | grep -a -q -E "^E2E_A11Y_btrue$" || fail "허용한 AI가 있는데 세션을 열 때 접근성이 켜지지 않았다"
	echo "접근성: 세션을 열 때 켰다"
fi

# 그래픽 셸: 에이전트가 접근성 트리에서 메뉴 막대 항목과 Dock 단추를 읽는다. 앱이 없으면 메뉴 막대에
# 앱 메뉴가 없으므로 누를 수 있는 요소의 수로 본다. 자체 컴포넌트를 넣지 않은 이미지(다른 아키텍처로
# 빌드한 이미지)에는 에이전트가 없어 건너뛴다
agent="sudo -u $user XDG_RUNTIME_DIR=/run/user/1000 bunt agent snapshot --app bunt-shell -i"
echo "if command -v bunt >/dev/null; then for i in \$(seq 300); do $agent >/tmp/e2e-shell 2>&1 && [ \$(grep -c 'ref=e' /tmp/e2e-shell) -ge 10 ] && break; sleep 1; done; echo \"E2E_AGENT_\$(grep -c 'ref=e' /tmp/e2e-shell)\"; else echo E2E_AGENT_NONE; fi" >&3
wait_for "^E2E_AGENT_([0-9]+|NONE)$" || fail "에이전트로 그래픽 셸을 확인하지 못했다"
agent_refs=$(console | grep -a -E "^E2E_AGENT_([0-9]+|NONE)$" | tail -1 | sed 's/^E2E_AGENT_//')
if [[ "$agent_refs" == NONE ]]; then
	echo "그래픽 셸: 이 이미지에는 에이전트가 없어 건너뛴다"
elif ((agent_refs >= 10)); then
	echo "그래픽 셸(에이전트): ${SECONDS}초, 누를 수 있는 요소 ${agent_refs}개"
else
	echo 'cat /tmp/e2e-shell' >&3
	sleep 2
	fail "에이전트가 읽은 셸에 메뉴 막대 항목과 Dock이 없다 (요소 ${agent_refs}개)"
fi

# e2e 8·6·4·7·9번은 키와 포인터를 QMP로 넣고, 결과는 게스트 쪽 도우미가 읽는다. begin <번호> <도우미>로
# 시작한다. guest는 도우미를 돌려 마지막 줄을 answer에 받는다. 답 줄은 입력 줄의 에코와 구분되도록 번호와
# 차례 표시를 붙인다. 차례가 이어지도록 명령 치환($(...), 하위 셸) 없이 부른다. 답을 받은 뒤에야 다음 키를 넣는다
scenario=
helper=
step=0
answer=
passed=0
failed=0
begin() {
	scenario=$1
	helper=$2
	passed=0
	failed=0
}
guest() {
	step=$((step + 1))
	echo "bash $helper $* 2>&1 | tail -1 | sed 's/^/E2E${scenario}_$step=/'" >&3
	wait_for "^E2E${scenario}_$step=" || fail "e2e ${scenario}번: $*의 답을 받지 못했다"
	answer=$(console | grep -a "^E2E${scenario}_$step=" | tail -1 | sed "s/^E2E${scenario}_$step=//")
}
keys() {
	python3 -I "$repo/tools/image/qmp.py" "$work/qmp.sock" "$@" || fail "e2e ${scenario}번: QMP로 키를 넣지 못했다"
}
check() {
	if [[ "$2" == "$3" ]]; then
		echo "e2e ${scenario}번 통과: $1"
		passed=$((passed + 1))
	else
		echo "e2e ${scenario}번 실패: $1 (받은 값 '$2', 바란 값 '$3')"
		failed=$((failed + 1))
	fi
}

# e2e 8번: 데스크탑 항목(ARCHITECTURE 5.2). 앱 창이 데스크탑을 가리기 전에 돈다(e2e 5번 앞). 결과는 게스트 쪽
# 도우미(tests/e2e/desktop.sh)가 에이전트와 데스크탑 폴더로 읽는다. 항목은 화면 오른쪽 끝 열(가운데 x 1218)에
# 위에서부터 놓인다. 칸은 메뉴 막대(28px) 아래 8px에서 시작해 104px마다이고, 아이콘 가운데는 y 76, 180, 284, 388이다
if [[ "$agent_refs" != NONE ]]; then
	push tests/e2e/desktop.sh /tmp/e2e-desktop.sh
	begin 8 /tmp/e2e-desktop.sh
	guest prepare
	check "데스크탑 폴더의 항목을 이름 차례(수는 크기 차례)로 보인다" "$answer" "메모.txt|문서 1.txt|문서 2.txt|문서 10.txt|프로젝트"
	keys click:1218,76 sleep:0.5
	guest selected
	check "항목을 누르면 고른다" "$answer" "메모.txt"
	guest focus
	check "창이 없는 곳을 누르면 데스크탑이 키보드를 받는다" "$answer" bunt-desktop
	# 빈 곳(옆 열)에서 시작해 첫 두 행에 걸치게 드래그한다
	keys drag:1100,60,1250,200 sleep:0.5
	guest selected
	check "빈 곳에서 드래그하면 사각형에 걸친 항목을 고른다" "$answer" "메모.txt|문서 1.txt"
	keys keys:meta_l+a sleep:0.5
	guest selected
	check "⌘A로 모두 선택한다" "$answer" "메모.txt|문서 1.txt|문서 2.txt|문서 10.txt|프로젝트"
	keys keys:esc sleep:0.5
	guest selected
	check "Esc로 선택을 푼다" "$answer" none
	# 단축 메뉴의 새로운 폴더(메뉴의 첫 항목). 메뉴가 닫힌 뒤 키가 이름 변경 칸에 가야 한다
	keys rclick:900,300 sleep:0.8 click:960,318 sleep:1 type:box sleep:0.3 keys:ret sleep:1
	guest files
	check "단축 메뉴로 새로운 폴더를 만들고 이름을 바꾼다" "$answer" "box|메모.txt|문서 1.txt|문서 10.txt|문서 2.txt|프로젝트"
	keys click:1218,388 sleep:0.5 keys:meta_l+backspace sleep:1
	guest trash
	check "⌘⌫로 항목을 휴지통으로 옮긴다" "$answer" "문서 10.txt"
	guest show-items false
	check "항목 보기를 끄면 데스크탑 항목 창을 숨긴다" "$answer" hidden
	guest show-items true
	check "항목 보기를 켜면 다시 보인다" "$answer" shown
	((failed == 0)) || fail "e2e 8번(데스크탑 항목): ${failed}개 실패"
	echo "e2e 8번(데스크탑 항목): ${SECONDS}초, ${passed}개 통과"
fi

# e2e 5번: 에이전트가 앱 A(계산기)의 값을 읽어 앱 B(텍스트 편집기)에 넣고, 창 스크린샷의 픽셀을 누르며,
# fill이 실패를 성공으로 알리지 않는지 본다(tests/e2e/agent-apps.sh).
# 스크립트를 직렬 콘솔로 넣어 root로 돌린다. 결과 줄은 입력 줄의 에코와 구분되도록 앞에 표시를 붙인다
if [[ "$agent_refs" != NONE ]]; then
	push tests/e2e/agent-apps.sh /tmp/e2e-agent-apps.sh
	echo 'bash /tmp/e2e-agent-apps.sh 2>&1 | sed "s/^/E2E5: /"' >&3
	wait_for "^E2E5: E2E5_RESULT [0-9]+ [0-9]+$" || fail "e2e 5번(에이전트 앱 간 제어)이 끝나지 않았다"
	console | grep -a "^E2E5: E2E5 " | sed 's/^E2E5: E2E5 /e2e 5번 /'
	read -r passed failed < <(console | grep -a -E "^E2E5: E2E5_RESULT" | tail -1 | sed 's/^E2E5: E2E5_RESULT //')
	((failed == 0 && passed > 0)) || fail "e2e 5번(에이전트 앱 간 제어): ${failed}개 실패"
	echo "e2e 5번(에이전트 앱 간 제어): ${SECONDS}초, ${passed}개 통과"
fi

# e2e 5번 이어서: 사람과 AI가 함께 쓴다(설계 agent-focus-design 3장, 데몬과 컴포지터의 대상 확인 입력). AI가 텍스트 편집기에
# 조각을 잇달아 치는 동안 사람이 QMP로 시스템 설정의 검색 칸을 누르고 친다. AI 글은 사람의 칸에, 사람 글은 AI 문서에 가지
# 않고, AI는 포커스를 되빼앗지 않으며, 사람이 누른 뒤 ref 없는 type은 FocusMovedError다. fill(접근성)은 그대로 되고, 접근성
# 버스에 Value의 GetAll(이슈 P0)이 나가지 않는다. 사람이 누를 자리는 AI가 그 칸을 포인터로 누른 답의 화면 좌표다. 끝에 설정이
# 키보드 포커스를 가져 e2e 6번의 앱 차례(설정이 마지막)를 지킨다
if [[ "$agent_refs" != NONE ]]; then
	begin 5 /tmp/e2e-agent-apps.sh
	guest together-watch
	watching=$answer
	guest together-prepare
	point=$answer
	[[ "$point" =~ ^[0-9]+,[0-9]+$ ]] || fail "e2e 5번(함께 쓰기): 설정 검색 칸의 자리를 읽지 못했다 ($point)"
	guest together-start
	[[ "$answer" == started ]] || fail "e2e 5번(함께 쓰기): AI가 치기 시작하지 못했다 ($answer)"
	# AI가 치기 시작한 뒤 사람이 설정 검색 칸을 누르고 두 번 나눠 친다
	keys sleep:1.5 "click:$point" sleep:0.5 type:human sleep:1 type:human sleep:0.5
	guest together-result
	read -r chunks order human_in_editor ai_in_settings human_text focus before <<<"$answer"
	check "사람이 누르기 전에는 ref 없는 type이 AI의 창(키보드 포커스)에 간다" "$before" ok
	check "사람이 치는 동안에도 AI 글이 모두 차례대로 편집기에 들어간다" "$chunks $order" "36 1"
	check "사람 글이 AI 문서에 가지 않는다" "$human_in_editor" 0
	check "AI 글이 사람의 칸에 가지 않는다" "$ai_in_settings" 0
	check "사람 글은 설정 검색 칸에 들어간다" "$human_text" humanhuman
	check "AI가 사람의 포커스를 되빼앗지 않는다" "$focus" org.buntos.Settings
	guest together-refless
	check "사람이 누른 뒤 ref 없는 type은 FocusMovedError다" "$answer" FocusMovedError
	guest together-fill
	check "사람이 쓰는 중에도 fill은 AI의 칸만 바꾼다" "$answer" "AI fill|org.buntos.Settings|humanhuman"
	if [[ "$watching" == watching ]]; then
		guest together-getall
		read -r getall gets <<<"$answer"
		check "접근성 버스에 Value의 GetAll이 나가지 않는다" "$getall" 0
		((gets > 0)) || echo "e2e 5번 알림: Value를 가진 요소를 읽지 않아 GetAll 확인은 뜻이 적다"
	else
		echo "e2e 5번 알림: 게스트에 dbus-monitor가 없어 Value의 GetAll을 보지 않았다"
	fi
	if ((failed > 0)); then
		guest together-log
		echo "e2e 5번 알림: $answer"
	fi
	guest together-cleanup
	((failed == 0)) || fail "e2e 5번(사람과 AI가 함께 쓴다): ${failed}개 실패"
	echo "e2e 5번(사람과 AI가 함께 쓴다): ${SECONDS}초, ${passed}개 통과"
fi

# e2e 6번: 키보드 단축키. 키는 실제 입력과 같은 길(libinput → 컴포지터 단축키 → 앱)을 거치고, 결과는 게스트 쪽
# 도우미(tests/e2e/shortcuts.sh)가 에이전트와 컴포지터 API로 읽는다. e2e 5번이 연 계산기, 텍스트 편집기,
# 시스템 설정을 그대로 쓴다
if [[ "$agent_refs" != NONE ]]; then
	push tests/e2e/shortcuts.sh /tmp/e2e-shortcuts.sh
	begin 6 /tmp/e2e-shortcuts.sh
	guest prepare
	[[ "$answer" == ready ]] || fail "e2e 6번: 텍스트 편집기를 준비하지 못했다 ($answer)"
	keys keys:meta_l+a sleep:0.3 keys:meta_l+c sleep:0.3 keys:meta_l+right sleep:0.3 keys:meta_l+v sleep:1
	guest value
	check "⌘A ⌘C ⌘→ ⌘V로 글을 복사해 붙인다" "$answer" "[one twoone two]"
	keys keys:meta_l+z sleep:1
	guest value
	check "⌘Z로 붙인 것을 되돌린다" "$answer" "[one two]"
	keys keys:alt+backspace sleep:1
	guest value
	check "⌥⌫로 앞 단어를 지운다" "$answer" "[one ]"
	keys keys:meta_l+tab sleep:1
	guest focus
	check "⌘⇥로 앞서 쓰던 앱으로 간다" "$answer" org.buntos.Settings
	keys keys:meta_l+m sleep:1
	guest focus
	check "⌘M으로 최소화하면 다음 창이 포커스를 받는다" "$answer" org.gnome.TextEditor
	keys keys:meta_l+tab sleep:1
	guest focus
	check "⌘⇥로 최소화한 앱이 돌아온다" "$answer" org.buntos.Settings
	# 창의 최소화 단추로 내린 창은 Dock 오른쪽 끝의 최소화 칸에 놓이고, 누르면 돌아온다
	guest minimize-button
	check "창의 최소화 단추로 내린 창이 Dock의 최소화 칸에 놓인다" "$answer" "docked 1"
	guest restore-dock
	check "Dock의 최소화 칸을 누르면 그 창이 돌아온다" "$answer" org.gnome.TextEditor
	# ⌘를 누른 채 ⇥를 누르면 앱 전환 창이 뜨고, ⌘를 떼면 고른 앱(바로 앞에 쓴 앱)으로 간다. 앞의 keys는
	# 키를 함께 눌렀다 떼므로 여기서는 ⌘를 따로 누르고 뗀다
	keys down:meta_l sleep:0.1 down:tab sleep:0.05 up:tab
	guest switcher
	check "⌘⇥를 누르고 있으면 앱 전환 창이 뜬다" "$answer" shown
	keys up:meta_l sleep:1
	guest focus
	check "⌘를 떼면 앱 전환 창에서 고른 앱으로 간다" "$answer" org.buntos.Settings
	keys keys:meta_l+spc sleep:1.5
	guest focus
	check "⌘스페이스로 모든 앱을 연다" "$answer" bunt-launcher
	keys keys:esc sleep:1
	guest screenshots
	before=$answer
	[[ "$before" =~ ^[0-9]+$ ]] || fail "e2e 6번: 스크린샷 수를 읽지 못했다 ($before)"
	# 미리 보기는 5초 뒤 사라지므로 저장한 파일을 세기 전에 본다
	keys keys:shift+meta_l+3 sleep:1
	guest preview
	check "저장한 스크린샷이 화면 오른쪽 아래에 미리 보기로 뜬다" "$answer" shown
	guest screenshots
	check "⇧⌘3으로 화면을 사진 폴더에 저장한다" "$answer" "$((before + 1))"
	keys keys:shift+meta_l+5
	guest toolbar
	check "⇧⌘5로 스크린샷 도구 막대를 연다" "$answer" shown
	keys keys:esc sleep:1
	guest dbus
	check "단축키 API (목록, 겹치는 키 거절, 바꾸기, 기본값)" "$answer" "1 1 1 0"
	# 음량 키: 출력 장치를 하나 두고 누른다. 한 칸은 16분의 1이다
	guest volume-prepare
	check "e2e 음량 키의 출력 장치를 둔다" "$answer" "Volume: 0.50"
	keys keys:volumeup sleep:0.5
	guest volume
	check "음량 키로 음량이 한 칸 오르고 메뉴 막대 아래에 표시가 뜬다" "$answer" "Volume: 0.56 shown"
	# 터미널: 편집기에서 ⌘C로 복사한 명령을 ⌥↩로 연 foot에 ⌘V(Ctrl+Shift+V로 간다)로 붙여 실행하고 ⌘W로 닫는다
	guest terminal-prepare
	[[ "$answer" == ready ]] || fail "e2e 6번: 터미널 시험을 준비하지 못했다 ($answer)"
	keys keys:meta_l+a sleep:0.3 keys:meta_l+c sleep:0.5 keys:alt+ret sleep:5 keys:meta_l+v sleep:1 keys:ret sleep:1.5
	guest terminal
	check "터미널에서 ⌘V로 붙여 실행한다" "$answer" "pasted 1"
	# 터미널의 Ctrl+V와 Ctrl+C (고친 foot과 그 설정, image/foot): Ctrl+V는 붙여넣기다. Ctrl+C는 고른 글이 있으면
	# 복사하고 고른 표시를 남겨 누르고 있어도(입력기가 되풀이를 새 누름으로 보낸다) 인터럽트하지 않으며, 고른 글이
	# 없으면 인터럽트다. 글은 ⌘F(foot의 검색)에 찾을 말을 Ctrl+V로 붙이고 ↩로 마쳐 고른다. foot은 클립보드가 바뀌어도
	# 고른 표시를 지우지 않으므로, 고른 뒤 클립보드를 다른 글로 바꾼다. 고른 표시는 Esc(foot이 보내는 키)로 지운다
	guest terminal-clip command
	keys keys:ctrl+v sleep:1 keys:ret sleep:1.5
	guest terminal
	check "터미널에서 Ctrl+V로 붙여 실행한다" "$answer" "ctrl-v 1"
	guest terminal-clip sleep
	keys keys:ctrl+v sleep:1 keys:ret sleep:1
	guest terminal-clip ctrl-v
	keys keys:meta_l+f sleep:0.5 keys:ctrl+v sleep:0.5 keys:ret sleep:0.5
	guest terminal-clip other
	keys down:ctrl sleep:0.05 down:c sleep:1.2 up:c sleep:0.05 up:ctrl sleep:0.5
	guest terminal-copy
	check "터미널에서 고른 글을 Ctrl+C로 복사하고, 누르고 있어도 인터럽트하지 않는다" "$answer" "ctrl-v 1"
	keys keys:esc sleep:0.5 keys:ctrl+c sleep:1
	guest terminal-copy
	check "터미널에서 고른 글이 없으면 Ctrl+C가 인터럽트다" "$answer" "ctrl-v 0"
	keys keys:meta_l+w sleep:2
	guest terminal
	check "터미널에서 ⌘W로 창을 닫는다" "$answer" "ctrl-v 0"
	# 조합 키만으로 된 입력 소스 전환(Windows의 Ctrl+Shift): 다른 키 없이 눌렀다 떼면 바뀌고, 사이에
	# 다른 키가 끼면(⌃⇧A) 그대로다
	guest chord-prepare
	[[ "$answer" == keyboard-us ]] || fail "e2e 6번: 입력 소스 전환을 ⌃⇧로 바꾸지 못했다 ($answer)"
	keys keys:ctrl+shift sleep:1
	guest input-method
	check "⌃⇧를 눌렀다 떼면 입력 소스가 바뀐다" "$answer" hangul
	keys keys:ctrl+shift+a sleep:1
	guest input-method
	check "⌃⇧ 사이에 다른 키를 누르면 바뀌지 않는다" "$answer" hangul
	guest chord-reset
	# Caps Lock 키(comp/src/caps_lock.rs): 짧게 누르면 입력 소스를 바꾸고, 0.4초보다 길게 누르면 Caps Lock을
	# 켜며(키보드의 불), 켜져 있을 때 누르면 끈다. 불은 게스트의 /sys/class/leds로 읽는다
	keys down:caps_lock sleep:0.08 up:caps_lock sleep:1
	guest caps-lock
	check "Caps Lock을 짧게 누르면 입력 소스가 바뀌고 불은 꺼져 있다" "$answer" "hangul 0"
	keys down:caps_lock sleep:1 up:caps_lock sleep:1
	guest caps-lock
	check "Caps Lock을 길게 누르면 불이 켜지고 입력 소스는 그대로다" "$answer" "hangul 1"
	keys down:caps_lock sleep:0.08 up:caps_lock sleep:1
	guest caps-lock
	check "Caps Lock이 켜져 있을 때 누르면 불이 꺼지고 입력 소스는 그대로다" "$answer" "hangul 0"
	keys down:caps_lock sleep:0.08 up:caps_lock sleep:1
	guest caps-lock
	check "Caps Lock을 다시 짧게 누르면 영문으로 돌아온다" "$answer" "keyboard-us 0"
	((failed == 0)) || fail "e2e 6번(키보드 단축키): ${failed}개 실패"
	echo "e2e 6번(키보드 단축키): ${SECONDS}초, ${passed}개 통과"
fi

# e2e 4번: 한글 입력. 키는 QMP로 넣어 libinput → 컴포지터 → fcitx5(input-method-v2, fcitx5-hangul) → 앱
# (text-input-v3)의 실제 길로 조합하게 하고, 결과는 게스트 쪽 도우미(tests/e2e/hangul.sh)가 에이전트(앱의 글)와
# 입력기 상태로 읽는다. 기대값은 macOS 한국어 두벌식과 fcitx5-hangul의 기본 동작이다. e2e 5·6번이 연 텍스트
# 편집기와 시스템 설정을 그대로 쓰고, 터미널(foot)과 Firefox는 새로 띄운다
if [[ "$agent_refs" != NONE ]]; then
	push tests/e2e/hangul.sh /tmp/e2e-hangul.sh
	begin 4 /tmp/e2e-hangul.sh
	guest prepare
	[[ "$answer" == ready ]] || fail "e2e 4번: 텍스트 편집기를 준비하지 못했다 ($answer)"
	# 입력 소스 전환: ⌃스페이스와 한/영 키(QEMU lang1 → evdev KEY_HANGEUL → keysym Hangul)는 컴포지터가 받아
	# fcitx5를 바꾸고, 메뉴 막대의 입력 소스 표시가 따라간다
	keys keys:ctrl+spc sleep:0.5
	guest state
	check "⌃스페이스로 한글이 되고 메뉴 막대가 '한'이다" "$answer" "hangul 한"
	keys keys:ctrl+spc sleep:0.5
	guest state
	check "⌃스페이스로 영문이 되고 메뉴 막대가 'A'다" "$answer" "keyboard-us A"
	keys keys:lang1 sleep:0.5
	guest state
	check "한/영 키로 한글이 된다" "$answer" "hangul 한"
	keys keys:lang1 sleep:0.5
	guest state
	check "한/영 키로 영문이 된다" "$answer" "keyboard-us A"
	# 글 입력 칸 밖(시스템 설정의 사이드바)에서도 한/영 키는 컴포지터가 받아 바꾼다. GTK4 앱은 글 입력 칸 밖에서도
	# 입력기를 켜 두므로(치면 검색을 여는 앱에 한글로 친다) 입력기의 키보드 잡기가 아니라 포커스로 본다
	guest focus-list
	check "글 입력 칸 밖(시스템 설정 사이드바 항목)으로 포커스를 옮긴다" "$answer" "org.buntos.Settings listitem"
	keys keys:lang1 sleep:0.5
	guest state
	check "글 입력 칸 밖에서도 한/영 키로 바뀐다" "$answer" "hangul 한"

	# 텍스트 편집기에서 두벌식으로 조합한다. 칸마다 문서를 비우고 한글로 둔 뒤 QMP 동작($3…)을 넣는다
	compose() {
		local name=$1 want=$2
		shift 2
		guest clear hangul
		[[ "$answer" == ready ]] || fail "e2e 4번: 텍스트 편집기를 비우지 못했다 ($answer)"
		keys "$@" sleep:0.5
		guest value
		check "$name" "$answer" "$want"
	}
	compose "dkssudgktpdy와 마침표: 안녕하세요. (조합 중 문장 부호는 조합을 끝낸다)" "[안녕하세요.]" \
		type:dkssudgktpdy keys:dot
	compose "쌍자음 ⇧R(ㄲ)·⇧T(ㅆ)이 첫소리와 받침에 들어간다: 깎았다" "[깎았다 ]" \
		keys:shift+r type:k keys:shift+r type:dk keys:shift+t type:ek keys:spc
	compose "겹모음 ㅘ(ㅗ+ㅏ)·ㅢ(ㅡ+ㅣ): 과자 의사" "[과자 의사 ]" type:rhkwk keys:spc type:dmltk keys:spc
	compose "받침 넘김: 각 다음에 ㅏ를 치면 가가" "[가가 ]" type:rkrk keys:spc
	compose "조합 중 Backspace는 자모 하나를 지운다: 한 → 하" "[하 ]" type:gks keys:backspace keys:spc
	compose "확정한 글자는 Backspace가 글자째 지운다: 안녕 → 안" "[안]" \
		type:dkssud keys:spc keys:backspace keys:backspace
	compose "조합 중 쉼표와 물음표(⇧/): 안녕, 한글?" "[안녕, 한글?]" \
		type:dkssud keys:comma keys:spc type:gksrmf keys:shift+slash
	compose "왼쪽 Shift만 눌렀다 떼도 조합이 이어진다: 한 다음 ㅏ는 하나" "[하나 ]" \
		type:gks keys:shift type:k keys:spc
	guest state
	check "왼쪽 Shift만 눌렀다 떼도 입력 소스가 그대로다" "$answer" "hangul 한"

	# 조합 중에 입력 소스를 바꾸거나 포커스를 옮겨도 조합하던 글자는 사라지거나 두 번 들어가지 않는다
	compose "조합 중 ⌃스페이스로 영문으로 바꾸면 조합하던 글자가 확정된다: 한abc" "[한abc]" \
		type:gks keys:ctrl+spc sleep:0.3 type:abc
	compose "조합 중 한/영 키로 바꿔도 확정된다: 안녕hi" "[안녕hi]" type:dkssud keys:lang1 sleep:0.3 type:hi
	guest clear hangul
	[[ "$answer" == ready ]] || fail "e2e 4번: 텍스트 편집기를 비우지 못했다 ($answer)"
	keys type:gks sleep:0.3
	guest focus-other
	check "조합 중 다른 창의 칸을 누르면 조합하던 글자가 원래 칸에 확정된다" "$answer" "[한] [] org.buntos.Settings"
	keys type:dkssud keys:spc sleep:0.5
	guest other-value
	check "옮겨 간 칸에서는 새로 조합한다 (앞 글자가 따라오지 않는다)" "$answer" "[안녕 ]"

	# 한글, 영어(대문자 포함), 숫자를 섞어 친다. 숫자는 한글 모드에서도 그대로 들어간다
	guest clear english
	[[ "$answer" == ready ]] || fail "e2e 4번: 텍스트 편집기를 비우지 못했다 ($answer)"
	keys type:bunt keys:shift+o keys:shift+s keys:spc type:2026 keys:ctrl+spc sleep:0.3 type:sus keys:spc \
		type:gksrmf keys:spc keys:ctrl+spc sleep:0.3 type:ok sleep:0.5
	guest value
	check "한글·영어·숫자를 섞어 친다: buntOS 2026년 한글 ok" "$answer" "[buntOS 2026년 한글 ok]"

	# 터미널(foot): Phase 1 종료 조건의 터미널 창. 사람이 편집기에 조합하는 중에(방금 쳤다) 다른 프로그램이 띄운 새 창은
	# 키보드를 가져가지 않고 뒤에 뜨며 조합도 그대로다(R3, 사람 우선. 창이 늦게 떠 치는 중이 지났으면 보지 않는다). 사람이
	# 손을 멈춘 뒤(yield-ms 1.5초) 띄우면 새 창이 포커스를 가져가므로 조합하던 글자가 편집기에 확정되어야 한다. foot에서
	# 친 한글은 cat이 받은 바이트(UTF-8)로 본다. 조합 중 Enter는 조합을 끝내고 줄을 바꾸며, Ctrl+D로 cat이 끝나면 foot도
	# 닫힌다
	guest clear hangul
	[[ "$answer" == ready ]] || fail "e2e 4번: 텍스트 편집기를 비우지 못했다 ($answer)"
	keys type:gks sleep:0.3
	guest foot-behind
	read -r behind_ms behind <<<"$answer"
	if [[ "$behind" != "org.gnome.TextEditor []" && "$behind_ms" =~ ^[0-9]+$ ]] && ((behind_ms > 1000)); then
		echo "e2e 4번 알림: 터미널 창이 늦게 떠(${behind_ms}ms) 사람이 치는 중이 지났을 수 있어 뒤에 뜨는지 보지 않았다 ($behind)"
	else
		check "사람이 조합하는 중 다른 프로그램이 띄운 새 창(터미널)은 키보드를 가져가지 않고 뒤에 뜬다" "$behind" \
			"org.gnome.TextEditor []"
	fi
	keys sleep:1.6
	guest foot-prepare
	check "조합 중 새 창(터미널)이 뜨면 조합하던 글자가 원래 칸에 확정되고 터미널이 포커스를 받는다" \
		"$answer" "[한] ready"
	[[ "$answer" == *ready ]] || fail "e2e 4번: 터미널을 띄우지 못했다 ($answer)"
	keys type:dkssudgktpdy keys:ret sleep:0.3 keys:ctrl+d sleep:0.5
	guest foot
	check "터미널(foot)에 친 '안녕하세요'와 Enter가 그 차례의 UTF-8 바이트로 프로그램에 간다" "$answer" \
		"ec9588eb8595ed9598ec84b8ec9a940a 0"

	# Firefox(GTK3, text-input-v3)의 글 칸: 조합 중 Tab으로 다음 칸에 가면 조합하던 글자는 앞 칸에 남고, 같은
	# 창의 다른 칸을 눌러도 남는다. 방금 터미널에 친 키에서 yield-ms(1.5초)가 지난 뒤에 띄운다: 그 안에 뜬 새 창은
	# 사람이 치는 중이라 뒤에 뜨고 키보드를 받지 않는다(R3). Firefox가 0.7초 만에 뜨면 걸렸다
	keys sleep:1.6
	guest firefox-prepare
	check "Firefox에서 칸 둘인 페이지를 열고 첫 칸에 포커스를 준다" "$answer" ready
	if [[ "$answer" == ready ]]; then
		keys type:dkssud keys:tab
		guest firefox-wait second
		keys type:gksrmf keys:spc sleep:1
		guest firefox-values
		check "Firefox: 조합 중 Tab으로 다음 칸에 가면 조합하던 글자가 앞 칸에 남고 다음 칸에서 새로 조합한다" \
			"$answer" "[안녕][한글 ]"
		keys type:gks sleep:0.5
		guest firefox-focus first
		check "Firefox: 조합 중 같은 창의 다른 칸에 포커스를 옮겨도 조합하던 글자가 남는다" "$answer" "[안녕][한글 한]"
	fi
	guest cleanup
	((failed == 0)) || fail "e2e 4번(한글 입력): ${failed}개 실패"
	echo "e2e 4번(한글 입력): ${SECONDS}초, ${passed}개 통과"
fi

# e2e 7번: 화면 잠금(ARCHITECTURE 6.5). ⌃⌘Q로 잠그면 잠금 화면이 뜨고, 잠긴 동안 에이전트의 화면 읽기와 앱
# 목록을 거절하며, 틀린 암호로는 풀리지 않고 사람이 잠금 화면에 친 암호(QMP 키)로만 풀리는지 본다. 상태는
# 게스트 쪽 도우미(tests/e2e/session-lock.sh)가 컴포지터 API(GetLocked)와 저널로 읽는다. 잠든 VM은 깨울 수
# 없으므로 VM에서는 잠자기를 받지 않고 로고 메뉴의 잠자기가 흐린지도 본다(5.4절)
if [[ "$agent_refs" != NONE ]]; then
	push tests/e2e/session-lock.sh /tmp/e2e-session-lock.sh
	begin 7 /tmp/e2e-session-lock.sh
	guest sleep
	check "VM에서는 logind가 잠자기를 받지 않고 로고 메뉴의 잠자기가 흐리다" "$answer" "no false"
	guest locked
	check "처음에는 잠기지 않았다" "$answer" false
	keys keys:ctrl+meta_l+q sleep:2
	guest ready
	check "⌃⌘Q로 잠그면 잠금 화면이 뜬다" "$answer" "true 1"
	guest agent
	check "잠긴 동안 에이전트의 화면 읽기와 앱 목록을 거절한다" "$answer" 2
	keys keys:esc sleep:0.5 type:wrong1 sleep:0.5 keys:ret sleep:4
	guest locked
	check "틀린 암호로는 풀리지 않는다" "$answer" true
	# 키가 잠금 화면보다 먼저 가거나 VM이 바빠 몇 개가 빠지면 풀리지 않으므로 Esc(친 암호를 지운다)를 누르고
	# 세 번까지 다시 친다 (로그인 화면과 같다)
	for attempt in 1 2 3; do
		keys keys:esc sleep:0.5 "type:$password" sleep:0.5 keys:ret sleep:4
		guest locked
		if [[ "$answer" == false ]]; then
			break
		fi
	done
	check "잠금 화면에 친 암호로 풀린다" "$answer" false
	guest journal
	# 틀린 암호는 한 번 넘게 남을 수 있다(다시 친 횟수만큼)
	check "잠금 화면이 틀린 암호와 잠금 해제를 남긴다" \
		"$(awk '{print ($1 >= 1) " " $2}' <<<"$answer")" "1 1"
	guest agent
	check "풀린 뒤에는 에이전트가 다시 읽는다" "$answer" 0
	((failed == 0)) || fail "e2e 7번(화면 잠금): ${failed}개 실패"
	echo "e2e 7번(화면 잠금): ${SECONDS}초, ${passed}개 통과"
fi

# e2e 10번: 터미널의 클립보드와 클립보드 기록(ARCHITECTURE 5.2). wl-copy·wl-paste는 포커스 없이 돌고, 샌드박스 앱과 잠긴 동안의
# 새 연결에는 프로토콜이 보이지 않는다. 클립보드 기록은 사람만 답하는 창이라 에이전트로 읽지 못한다. 그래서 키(QMP)로 항목을
# 골라 wl-paste로 무엇이 들어왔는지 보고, 에이전트와 접근성 버스에서 글이 보이지 않는지 본다. 화면은 E2E_LOG_DIR에 남긴다
if [[ "$agent_refs" != NONE ]]; then
	push tests/e2e/clipboard.sh /tmp/e2e-clipboard.sh
	push tests/e2e/wayland-probe.py /tmp/e2e-wayland-probe.py
	echo 'chmod 755 /tmp/e2e-clipboard.sh /tmp/e2e-wayland-probe.py' >&3
	begin 10 /tmp/e2e-clipboard.sh
	shot() {
		if [[ -n "${E2E_LOG_DIR:-}" ]]; then
			keys "screen:$E2E_LOG_DIR/$1.png"
		fi
	}
	# 모든 앱을 열고 클립보드 기록으로 넘어간다. 바쁜 VM에서는 패널을 여는 데 몇 초 걸리므로 키보드를 받기를 기다린다
	open_history() {
		keys keys:meta_l+spc
		guest wait-focus bunt-launcher
		[[ "$answer" == bunt-launcher ]] || fail "e2e 10번: 모든 앱이 뜨지 않았다 ($answer)"
		keys sleep:0.3 keys:meta_l+4
		guest wait-focus bunt-clipboard
		[[ "$answer" == bunt-clipboard ]] || fail "e2e 10번: 클립보드 기록이 뜨지 않았다 ($answer)"
		keys sleep:0.5
	}
	# 패널이 닫혀 앞의 앱이 키보드를 돌려받기를 기다린다
	closed() {
		guest wait-focus org.gnome.TextEditor
	}
	# 잠금 화면에 암호를 쳐 푼다 (e2e 7번과 같다)
	unlock() {
		for attempt in 1 2 3; do
			keys keys:esc sleep:0.5 "type:$password" sleep:0.5 keys:ret sleep:4
			guest locked
			if [[ "$answer" == false ]]; then
				break
			fi
		done
	}

	guest globals
	check "일반 클라이언트에는 클립보드 프로토콜(ext, zwlr)과 보안 컨텍스트가 보인다" "$answer" "ext=1 zwlr=1 security=1"
	guest sandbox-globals
	check "샌드박스(보안 컨텍스트) 클라이언트에는 보이지 않는다" "$answer" "ext=0 zwlr=0 security=0"
	guest roundtrip
	check "wl-copy와 wl-paste가 포커스를 옮기지 않고 왕복한다 (UTF-8)" "$answer" "[왕복 e2e 클립보드] same"

	# 앱과의 복사와 붙여넣기: 같은 클립보드다
	guest prepare
	[[ "$answer" == ready ]] || fail "e2e 10번: 텍스트 편집기를 준비하지 못했다 ($answer)"
	keys keys:meta_l+a sleep:0.3 keys:meta_l+c sleep:0.5
	guest paste
	check "앱에서 ⌘C로 복사한 글을 wl-paste가 읽는다" "$answer" "[from editor]"
	guest copy fromcli
	keys keys:meta_l+a sleep:0.3 keys:meta_l+v sleep:1
	guest value
	check "wl-copy로 넣은 글을 앱에서 ⌘V로 붙인다" "$answer" "[fromcli]"

	# 처음 쓸 때 허용한다 (macOS와 같다)
	guest setting
	check "클립보드 기록은 처음에 꺼져 있다" "$answer" false
	open_history
	check "⌘스페이스 뒤 ⌘4로 클립보드 기록으로 넘어간다" "$answer" bunt-clipboard
	shot clipboard-allow
	keys keys:ret sleep:1
	guest setting
	check "허용을 묻는 화면에서 Enter로 켠다" "$answer" true
	shot clipboard-allowed
	keys keys:esc
	closed
	check "Esc로 닫으면 앞의 앱이 키보드를 돌려받는다" "$answer" org.gnome.TextEditor

	# 복사가 기록에 남고, 고르면 클립보드에 들어온다. 허용하기 전에 복사한 fromcli가 첫 항목이다
	guest copy alphatoken
	guest copy betatoken
	guest copy-secret hunter2secret
	guest paste
	check "비밀번호 관리자의 복사도 클립보드로는 쓴다" "$answer" "[hunter2secret]"
	open_history
	shot clipboard-list
	guest agent-refused betatoken
	check "기록이 떠 있는 동안 에이전트의 키를 거절하고 셸을 읽어도 창과 글이 없다" "$answer" "1 0"
	guest a11y-scan betatoken
	check "같은 사용자의 프로그램이 접근성 버스에서 기록의 글을 읽지 못한다" "$answer" "0 1"
	keys keys:ret
	closed
	check "고르면 패널이 닫힌다" "$answer" org.gnome.TextEditor
	guest paste
	check "Enter로 첫 항목을 고른다. 비밀번호 관리자의 복사는 기록에 없다" "$answer" "[betatoken]"
	open_history
	keys keys:down sleep:0.3 keys:ret
	closed
	guest paste
	check "↓로 다음 항목을 고른다" "$answer" "[alphatoken]"
	keys keys:meta_l+a sleep:0.3 keys:meta_l+v sleep:1
	guest value
	check "고른 항목을 앞의 앱에 ⌘V로 붙인다" "$answer" "[alphatoken]"
	open_history
	keys keys:down sleep:0.3 keys:meta_l+c
	closed
	guest paste
	check "⌘C로 고른 항목을 복사한다" "$answer" "[betatoken]"
	open_history
	keys type:fromc sleep:1 keys:ret
	closed
	guest paste
	check "검색한 항목을 고른다" "$answer" "[fromcli]"
	open_history
	keys type:nosuchthing sleep:1 keys:ret sleep:0.5
	guest focus
	check "찾은 것이 없으면 Enter는 아무것도 하지 않는다" "$answer" bunt-clipboard
	keys keys:esc sleep:0.5
	guest focus
	check "검색어가 있으면 Esc는 검색어만 지운다" "$answer" bunt-clipboard
	keys keys:esc
	closed

	# 그림: 스크린샷을 클립보드로 찍으면 컴포지터가 내놓은 PNG가 기록에 남는다
	keys keys:ctrl+shift+meta_l+3 sleep:2
	guest types
	check "⌃⇧⌘3의 스크린샷을 wl-paste가 PNG로 본다" "$answer" "image/png "
	open_history
	shot clipboard-image
	keys keys:down sleep:0.3 keys:ret
	closed
	guest paste
	check "그림 다음 항목을 고른다" "$answer" "[fromcli]"
	open_history
	keys keys:down sleep:0.3 keys:ret
	closed
	guest png
	check "기록의 그림을 고르면 PNG로 내놓는다" "$answer" PNG

	# 지우기: 더 보기(검색 칸에서 ⇥ 세 번)의 기록 지우기
	guest copy keptinclipboard
	open_history
	keys keys:tab sleep:0.2 keys:tab sleep:0.2 keys:tab sleep:0.2 keys:ret sleep:0.8
	shot clipboard-more
	keys keys:ret sleep:1
	shot clipboard-cleared
	guest paste
	check "기록을 지워도 지금 클립보드는 그대로다" "$answer" "[keptinclipboard]"
	keys keys:esc
	closed
	guest copy aftercleartoken
	open_history
	keys keys:down sleep:0.3 keys:ret
	closed
	guest paste
	check "지운 뒤에는 새 복사만 있다" "$answer" "[aftercleartoken]"

	# 잠금: 패널이 닫히고, 새 연결이 클립보드를 읽지 못하고, 풀린 뒤에도 패널이 저절로 뜨지 않는다
	open_history
	check "잠그기 전에 기록을 연다" "$answer" bunt-clipboard
	keys keys:ctrl+meta_l+q sleep:3
	guest locked
	check "기록을 연 채로 잠근다" "$answer" true
	shot clipboard-locked
	guest paste-locked
	check "잠긴 동안 새 wl-paste는 클립보드를 읽지 못한다" "$answer" refused
	guest globals
	check "잠긴 동안 새 연결에는 클립보드 프로토콜이 보이지 않는다" "$answer" "ext=0 zwlr=0 security=1"
	unlock
	check "잠금을 푼다" "$answer" false
	closed
	check "풀린 뒤 기록이 떠 있지 않고 앞의 앱이 키보드를 받는다" "$answer" org.gnome.TextEditor
	shot clipboard-unlocked
	guest paste
	check "풀린 뒤 wl-paste가 다시 읽는다" "$answer" "[aftercleartoken]"
	guest globals
	check "풀린 뒤 새 연결에 클립보드 프로토콜이 다시 보인다" "$answer" "ext=1 zwlr=1 security=1"

	# 시스템 설정: 지우기 단추, 끄기
	guest copy settingstoken
	guest clear-from-settings
	open_history
	keys keys:ret sleep:1
	guest focus
	check "시스템 설정의 지우기로 비면 Enter가 아무것도 하지 않는다" "$answer" bunt-clipboard
	keys keys:esc
	closed
	guest set-setting false
	check "시스템 설정의 스위치로 끈다" "$answer" false
	guest copy offtoken
	open_history
	shot clipboard-off
	keys keys:ret sleep:1
	guest setting
	check "끄면 다시 허용을 묻는다" "$answer" true
	guest copy backontoken
	keys keys:down sleep:0.3 keys:ret
	closed
	guest paste
	check "다시 켜면 켠 때의 클립보드부터 남는다" "$answer" "[offtoken]"
	guest set-setting false

	guest no-trace betatoken
	check "기록의 글이 파일과 저널에 남지 않는다" "$answer" "0 0"
	((failed == 0)) || fail "e2e 10번(클립보드): ${failed}개 실패"
	echo "e2e 10번(클립보드): ${SECONDS}초, ${passed}개 통과"
fi

# e2e 9번: 패키지 매니저(ARCHITECTURE 5.5). 받은 .deb와 AppImage를 기본 앱으로 열어 설치하고, 실행하고, 목록 창에서 지운다.
# 창은 에이전트로 읽고 누른다. .deb의 관리자 인증 창은 사람만 답하므로 암호를 QMP로 친다. 처음 열 때 apt 목록을 받으므로
# 오래 걸릴 수 있고, 인터넷이 없어도 의존성 없는 패키지는 설치된다
if [[ "$agent_refs" != NONE ]]; then
	push tests/e2e/packages.sh /tmp/e2e-packages.sh
	begin 9 /tmp/e2e-packages.sh
	guest prepare
	check "시험 설치 파일 둘을 만든다" "$answer" 2
	guest base-hidden
	check "이미지에 든 패키지를 제거 목록의 기준으로 적어 둔다" "$answer" 1
	guest guard
	check "apt 보호: buntos-runtime을 고정해 목록에서 빼고 문제가 없다" "$answer" "hold 1 ok"
	guest guard-refuses
	check "apt 보호: 라이브러리 제거를 거부하고 자동 제거가 지우는 것이 없다" "$answer" "refused 0"
	guest guard-before
	check "apt 보호: 우리 파일의 해시를 재 둔다" "$answer" ready
	guest default deb
	check ".deb의 기본 앱이 패키지 매니저다" "$answer" org.buntos.PackageManager.desktop
	guest default appimage
	check "AppImage의 기본 앱이 패키지 매니저다" "$answer" org.buntos.PackageManager.desktop

	guest open deb
	guest ready install-enabled
	check ".deb를 열면 읽고 의존성을 본 뒤 설치 단추가 켜진다" "$answer" yes
	guest ready deb-title
	check "창에 패키지 안 .desktop의 이름이 보인다" "$answer" yes
	guest click 설치
	guest asked package-install-untrusted
	check "설치를 누르면 관리자 인증을 묻는다" "$answer" yes
	# 인증 창이 뜨고 키보드를 받을 틈을 준다
	keys sleep:2 "type:$password" sleep:0.5 keys:ret
	guest ready success
	check "암호를 치면 설치가 끝난다" "$answer" yes
	guest deb-installed
	check "패키지가 설치되었다" "$answer" yes
	guest click 열기
	guest ran deb
	check "[열기]가 설치한 앱을 실행한다" "$answer" yes

	guest list-open
	guest ready list-deb
	check "목록 창에 설치한 앱이 보인다" "$answer" yes
	guest list
	check "목록에는 직접 설치한 것만 보인다" "$answer" "E2E Hello"
	guest click 제거…
	guest ready sheet
	check "제거를 누르면 확인 시트가 뜬다" "$answer" yes
	guest sheet 제거
	guest asked package-remove
	check "제거도 관리자 인증을 묻는다" "$answer" yes
	keys sleep:2 "type:$password" sleep:0.5 keys:ret
	guest ready empty
	check "암호를 치면 목록에서 사라진다" "$answer" yes
	guest deb-installed
	check "패키지가 지워졌다" "$answer" no
	guest guard-after
	check "apt 보호: 설치와 제거도 훅을 거치고 스냅숏 없는 루트를 막지 않으며 우리 파일이 그대로다" "$answer" "ran same none"

	# AppImage: 관리자 암호 없이 이 사용자의 폴더에 푼다
	guest open appimage
	guest ready install-enabled
	check "AppImage를 열면 설치 단추가 켜진다" "$answer" yes
	guest ready appimage-title
	check "창에 AppImage 안 .desktop의 이름이 보인다" "$answer" yes
	guest click 설치
	guest ready success
	check "AppImage는 암호 없이 설치된다" "$answer" yes
	guest appimage-installed
	check "응용 프로그램 폴더에 풀고 메뉴 항목을 만든다" "$answer" yes
	guest click 열기
	guest ran appimage
	check "[열기]가 푼 앱을 실행한다" "$answer" yes
	guest ready list-appimage
	check "떠 있는 목록 창이 새로 설치한 앱을 보인다" "$answer" yes
	guest click 제거…
	guest ready sheet
	guest sheet 제거
	guest ready empty
	check "AppImage를 목록에서 지운다" "$answer" yes
	guest appimage-installed
	check "폴더와 메뉴 항목이 지워졌다" "$answer" no
	keys keys:esc sleep:1
	((failed == 0)) || fail "e2e 9번(패키지 매니저): ${failed}개 실패"
	echo "e2e 9번(패키지 매니저): ${SECONDS}초, ${passed}개 통과"
fi

# e2e 11번: 화면 키보드와 단축 줄(ARCHITECTURE 5.1, 5.2). 키보드와 태블릿을 떼고 터치 화면을 붙여 터치 모드로 간 뒤, 손가락만으로
# 친 글을 단축 줄로 오려두고 복사해 붙여 넣는다. 결과는 게스트 쪽 도우미(tests/e2e/touch-keyboard.sh)가 앱의 글과 터미널이 받은
# 바이트로 읽는다. 화면 키보드는 에이전트에 보이지 않으므로(6.3절) 단추는 화면의 픽셀로 본다
if [[ "$agent_refs" != NONE ]]; then
	push tests/e2e/touch-keyboard.sh /tmp/e2e-touch-keyboard.sh
	begin 11 /tmp/e2e-touch-keyboard.sh
	# 1280×800에서 판은 472에서 시작한다(단축 줄 48px + 줄 68px × 4 + 위아래 여백 4px씩). 칸은 (1280 - 8) / 46 픽셀이다
	bar_y=500
	key_top=472
	cut_key=touch:45,$bar_y
	copy_key=touch:128,$bar_y
	paste_key=touch:211,$bar_y
	return_key=touch:1168,626
	# 글자 하나의 손가락 동작 (글자 쪽의 QWERTY. 글자 키는 4칸이고, 둘째 줄은 2칸, 셋째 줄은 5칸 들였다)
	osk_letter() {
		local row letters start y head
		for row in qwertyuiop:0:558 asdfghjkl:2:626 zxcvbnm:5:694; do
			IFS=: read -r letters start y <<<"$row"
			head=${letters%%"$1"*}
			if ((${#head} < ${#letters})); then
				echo "touch:$((4 + (2 * (start + 4 * ${#head}) + 4) * 1272 / 92)),$y"
				return
			fi
		done
	}
	osk_type() {
		local actions=() i
		for ((i = 0; i < ${#1}; i++)); do
			actions+=("$(osk_letter "${1:i:1}")" sleep:0.3)
		done
		keys "${actions[@]}"
	}
	# 단축 줄에 보이는 단추. 그림은 칸 가운데의 어두운 점으로 본다 (판은 밝다)
	bar_buttons() {
		local x name shown=()
		for x in 45:cut 128:copy 211:paste; do
			name=${x#*:}
			x=${x%:*}
			if (($(keys "dark:$work/bar.ppm,$((x - 16)),$((bar_y - 16)),$((x + 16)),$((bar_y + 16))") > 15)); then
				shown+=("$name")
			fi
		done
		echo "${shown[*]:-none}"
	}

	guest prepare
	[[ "$answer" == ready ]] || fail "e2e 11번: 텍스트 편집기를 준비하지 못했다 ($answer)"
	# 창을 화면에 채운다. 떠 있는 창은 키보드를 피해 옮기지 않는다 (디자인 스펙 15장)
	keys keys:ctrl+meta_l+f sleep:2
	keys plug:virtio-multitouch-pci,e2e-touch,e2e-touch-port unplug:e2e-keyboard unplug:e2e-tablet
	guest mode touch
	check "키보드와 태블릿을 떼고 터치 화면을 붙이면 터치 모드다" "$answer" touch

	# 문서를 누르면 키보드가 뜨고 창이 판 위에서 끝난다 (창 자리 줄이기와 단축 줄이 있는 판의 높이가 맞다)
	keys touch:640,200 sleep:2
	guest window-height
	check "키보드가 뜨면 창이 단축 줄 위에서 끝난다" "$answer" "$key_top"
	check "보통 칸의 단축 줄에 오려두기, 복사하기, 붙여넣기가 있다" "$(bar_buttons)" "cut copy paste"
	osk_type hello
	keys sleep:1
	guest value
	check "손가락으로 친 글이 문서에 들어간다" "$answer" "[hello]"

	# 글을 고르고 복사한 뒤, 줄의 끝을 눌러 글 커서를 옮기고 붙여 넣는다 (⌘C, ⌘V와 같은 키가 앱에 간다)
	guest select-all
	keys "$copy_key" sleep:1
	guest value
	check "복사하기는 글을 바꾸지 않는다" "$answer" "[hello]"
	keys touch:1000,27 sleep:1 "$paste_key" sleep:1
	guest value
	check "복사하기와 붙여넣기: 고른 글이 글 커서 자리에 들어간다" "$answer" "[hellohello]"
	guest select-all
	keys "$cut_key" sleep:1
	guest value
	check "오려두기: 고른 글이 지워진다" "$answer" "[]"
	keys "$paste_key" sleep:1
	guest value
	check "붙여넣기: 오려둔 글이 들어간다" "$answer" "[hellohello]"

	# 암호 칸에는 붙여넣기만 있다. 붙여넣기는 자리가 그대로다
	guest password-prepare
	check "시스템 설정의 암호 시트를 연다" "$answer" 1
	keys touch:677,392 sleep:2
	check "암호 칸의 단축 줄에는 붙여넣기만 있다" "$(bar_buttons)" paste
	guest password-cleanup
	check "암호 시트를 닫으면 텍스트 편집기로 돌아온다" "$answer" org.gnome.TextEditor
	keys sleep:1 touch:640,200 sleep:2
	check "보통 칸으로 돌아오면 세 단추가 다시 있다" "$(bar_buttons)" "cut copy paste"

	# 잠금 화면의 키보드에는 단축 줄이 없다. 단축 줄의 붙여넣기 자리를 눌러도 클립보드의 글이 암호 칸에 들어가지 않아, 그대로 친
	# 암호로 풀린다
	guest lock
	check "화면을 잠근다" "$answer" true
	keys touch:640,488 sleep:2 "$paste_key" sleep:0.5
	osk_type "$password"
	keys "$return_key" sleep:3
	guest locked
	check "잠금 화면에서 붙여넣기 자리를 눌러도 클립보드가 들어가지 않고 친 암호로 풀린다" "$answer" false

	# 터미널에는 오려두기가 없고, 복사하기와 붙여넣기는 터미널의 키(Ctrl+Shift+C·V)로 간다. Ctrl+V로 갔으면 cat은 글이 아니라 ^V를 받는다
	guest foot-prepare
	check "터미널을 띄운다" "$answer" foot
	keys touch:640,200 sleep:2
	check "터미널의 단축 줄에는 복사하기와 붙여넣기만 있다" "$(bar_buttons)" "copy paste"
	keys "$paste_key" sleep:1 "$return_key" sleep:1
	guest foot
	check "터미널에서 붙여넣기: 프로그램이 글을 받는다" "$answer" " h e l l o h e l l o \\n"
	# 둘째 줄의 낱말을 손가락으로 두 번 눌러 고르면 복사하기와 붙여넣기로 다시 들어간다
	osk_type xyz
	keys "$return_key" sleep:1 touch:8,32 touch:8,32 sleep:1 "$copy_key" sleep:1 "$paste_key" sleep:1 \
		"$return_key" sleep:1
	guest foot
	check "터미널에서 복사하기: 고른 낱말을 복사한다" "$answer" " h e l l o h e l l o \\n x y z \\n x y z \\n"
	guest cleanup
	keys sleep:2

	# 키보드와 태블릿을 다시 붙이면 포인터 모드로 돌아온다
	keys plug:virtio-keyboard-pci,e2e-keyboard,e2e-keyboard-port plug:virtio-tablet-pci,e2e-tablet,e2e-tablet-port \
		unplug:e2e-touch
	guest mode pointer
	check "키보드와 태블릿을 다시 붙이면 포인터 모드다" "$answer" pointer

	# 잠금 화면의 암호 칸은 물리 키보드의 ⌃V도 받지 않는다. 클립보드의 글이 들어갔으면 그 뒤에 친 암호가 틀린다
	guest copy-prepare
	[[ "$answer" == ready ]] || fail "e2e 11번: 복사할 글을 준비하지 못했다 ($answer)"
	keys keys:meta_l+a sleep:0.3 keys:meta_l+c sleep:0.5
	guest lock
	check "클립보드에 글을 두고 화면을 잠근다" "$answer" true
	keys keys:ctrl+v sleep:1 "type:$password" sleep:0.5 keys:ret sleep:3
	guest locked
	check "잠금 화면에서 ⌃V로 클립보드가 들어가지 않고 친 암호로 풀린다" "$answer" false
	((failed == 0)) || fail "e2e 11번(화면 키보드와 단축 줄): ${failed}개 실패"
	echo "e2e 11번(화면 키보드와 단축 줄): ${SECONDS}초, ${passed}개 통과"
fi

# 로그아웃: 로그인 화면이 다시 뜨고, 그 컴포지터도 사용자 세션의 CRTC와 모드를 끄지 않고 이어받는다 (comp/src/handover.rs)
if [[ "$greeter" != NONE ]]; then
	# e2e 4번이 저장하지 않은 글을 남긴 텍스트 편집기는 저장할지 물어 로그아웃을 멈춘다 (설계안 5.4절)
	echo "pkill -f /usr/bin/gnome-text-editor; for i in \$(seq 20); do pgrep -f /usr/bin/gnome-text-editor >/dev/null || break; sleep 0.5; done; echo \"E2E_EDITOR_\$((1 + 1))\"" >&3
	wait_for "^E2E_EDITOR_2$" || fail "텍스트 편집기를 닫지 못했다"
	python3 -I "$repo/tools/image/qmp.py" "$work/qmp.sock" keys:alt+shift+meta_l+q ||
		fail "QMP로 로그아웃 단축키를 넣지 못했다"
	echo "for i in \$(seq 90); do pgrep -u _greetd -x bunt-greeter >/dev/null && break; sleep 1; done; echo \"E2E_LOGOUT_\$(pgrep -c -u _greetd -x bunt-greeter)_\$(journalctl -b -q -t bunt-greeter | grep -c 'keeping the mode of the previous compositor')\"" >&3
	wait_for "^E2E_LOGOUT_[0-9]+_[0-9]+$" || fail "로그아웃 결과를 받지 못했다"
	console | grep -a -q -E "^E2E_LOGOUT_[1-9][0-9]*_[0-9]+$" || fail "로그아웃한 뒤 로그인 화면이 뜨지 않았다"
	console | grep -a -q -E "^E2E_LOGOUT_[0-9]+_[1-9][0-9]*$" || fail "로그아웃할 때 컴포지터가 모드를 다시 설정했다"
	echo "로그아웃: ${SECONDS}초, 모드를 다시 설정하지 않았다"

	# 곧바로 다시 로그인: 로그아웃 뒤 10초(UserStopDelaySec) 안에는 앞 세션의 사용자 관리자와 D-Bus가 남는다.
	# 그때 포털 구현(xdg-desktop-portal-gtk)을 누가 부르면 화면이 없어 뜨지 못하고, dbus-daemon은 그 활성화를
	# 120초 동안 쥔다. 새 세션이 그것을 풀지 못하면 셸(GTK)이 포털을 25초씩 세 번 기다린 뒤에야 뜬다
	# (comp/data/comp.toml의 prepare와 finish). 로그아웃할 때 포털을 먼저 내리므로 그런 활성화가 저절로 생기지
	# 않는다. 여기서는 일부러 만든 뒤 들어가, 셸이 포털을 기다리지 않는지 본다
	bus="sudo -u $user DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus"
	echo "$bus busctl --user --timeout=1 call org.freedesktop.impl.portal.desktop.gtk /org/freedesktop/portal/desktop org.freedesktop.DBus.Peer Ping >/dev/null 2>&1; echo \"E2E_HELD_\$(journalctl -b -q --no-pager _UID=1000 | grep -c 'cannot open display')_\$(journalctl -b -q --no-pager | grep -c 'Cannot get portal')\"" >&3
	wait_for "^E2E_HELD_[0-9]+_[0-9]+$" || fail "남은 D-Bus에 포털 구현을 불러 보지 못했다"
	[[ "$(console | grep -a -E "^E2E_HELD_[0-9]+_[0-9]+$" | tail -1)" =~ ^E2E_HELD_([0-9]+)_([0-9]+)$ ]]
	held=${BASH_REMATCH[1]}
	# 바쁜 컴퓨터에서는 앞의 시험에서 앱이 포털을 기다리다 시간이 초과됐을 수 있다. 여기서 늘어난 것만 본다
	waited=${BASH_REMATCH[2]}
	for attempt in 1 2 3; do
		python3 -I "$repo/tools/image/qmp.py" "$work/qmp.sock" keys:esc sleep:0.5 "type:$password" sleep:0.5 keys:ret ||
			fail "QMP로 암호를 치지 못했다"
		echo "for i in \$(seq 40); do pgrep -u $user -x bunt-shell >/dev/null && break; sleep 1; done; echo \"E2E_RELOGIN${attempt}_\$(pgrep -c -u $user -x bunt-shell)\"" >&3
		wait_for "^E2E_RELOGIN${attempt}_[0-9]+$" || fail "다시 로그인한 결과를 받지 못했다"
		if console | grep -a -q -E "^E2E_RELOGIN${attempt}_[1-9][0-9]*$"; then
			break
		fi
		((attempt < 3)) || fail "로그아웃한 뒤 암호로 다시 들어가지 못했다"
	done
	# 셸이 뜬 뒤(메뉴 막대와 Dock을 읽을 수 있다) 포털 시간 초과가 늘지 않아야 한다. 포털을 기다렸으면 셸이
	# 읽히기 전에 세 번 남는다
	echo "for i in \$(seq 300); do $agent >/tmp/e2e-shell 2>&1 && [ \$(grep -c 'ref=e' /tmp/e2e-shell) -ge 10 ] && break; sleep 1; done; echo \"E2E_RESHELL_\$(grep -c 'ref=e' /tmp/e2e-shell)_\$(journalctl -b -q --no-pager | grep -c 'Cannot get portal')_\$($bus systemctl --user is-active xdg-desktop-portal.service xdg-desktop-portal-gtk.service | grep -c -x active)\"" >&3
	wait_for "^E2E_RESHELL_[0-9]+_[0-9]+_[0-9]+$" || fail "다시 로그인한 뒤 셸을 확인하지 못했다"
	reshell=$(console | grep -a -E "^E2E_RESHELL_[0-9]+_[0-9]+_[0-9]+$" | tail -1)
	[[ "$reshell" =~ ^E2E_RESHELL_([0-9]+)_([0-9]+)_([0-9]+)$ ]]
	((BASH_REMATCH[1] >= 10)) || fail "다시 로그인한 뒤 셸에 메뉴 막대 항목과 Dock이 없다 ($reshell)"
	((BASH_REMATCH[2] == waited)) || fail "다시 로그인한 셸이 포털을 기다리다 시간이 초과됐다 ($reshell, 앞에는 ${waited}번)"
	((BASH_REMATCH[3] == 2)) || fail "다시 로그인한 뒤 포털과 그 구현이 떠 있지 않다 ($reshell)"
	echo "다시 로그인(${attempt}번째): ${SECONDS}초, 쥐고 있던 활성화 ${held}개, 포털 시간 초과 없음"
fi

echo 'systemctl poweroff' >&3
while kill -0 "$vm_pid" 2>/dev/null; do
	((SECONDS < deadline)) || fail "전원이 꺼지지 않았다"
	sleep 1
done
echo "통과($board): 전원 꺼짐까지 ${SECONDS}초"
