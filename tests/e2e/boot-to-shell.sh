#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# e2e 1번: 부팅 → 로그인 → 셸 (ARCHITECTURE 9.2. ADR-009의 Phase 0 드라이버: 시리얼 콘솔과 QMP)
# 빌드한 이미지를 시험 계정과 함께 띄운다(systemd 자격 증명 buntos.account를 SMBIOS로 넘긴다).
# 직렬 콘솔의 root 자동 로그인 뒤 셸에서 명령이 실행되는지 보고, 로그인 화면(bunt-greeter)에 QMP로
# 암호를 쳐 그 계정으로 들어간다. sudo가 암호를 묻는지 보고, 그래픽 셸(메뉴 막대, Dock)이 떴는지는
# 에이전트의 읽기 경로(bunt agent snapshot, ADR-009)로 본다. 이어서 e2e 5번(에이전트가 앱 A의 값을 읽어 앱 B에
# 넣는다, tests/e2e/agent-apps.sh), e2e 6번(키보드 단축키, tests/e2e/shortcuts.sh), e2e 4번(한글 입력,
# tests/e2e/hangul.sh), e2e 7번(화면 잠금, tests/e2e/session-lock.sh)을 돌린 뒤 끈다. 자체 컴포넌트가 없는
# 이미지(그래픽 세션이 없는 x86-64)에서는 5·6·4·7번을 건너뛴다.
# 사용법: tests/e2e/boot-to-shell.sh <보드>   (먼저 tools/image/build.sh <보드>로 빌드한다)
# 환경 변수: E2E_TIMEOUT 전체 제한 시간(초, 기본 900)
#            BUNTOS_IMAGE_DIR 띄울 이미지의 디렉터리 (tools/image/build.sh·vmctl.sh와 같다. 기본
#            $BUNTOS_BUILD_DIR/<보드>, BUNTOS_BUILD_DIR의 기본은 ~/buntos-build)
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
board=${1:?"보드 이름이 필요하다 (bsp/ 아래 디렉터리 이름)"}
deadline=$((SECONDS + ${E2E_TIMEOUT:-900}))
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
# 그래픽 세션이 뜨도록 가상 GPU를 붙이고(화면은 내보내지 않는다), 로그인 화면에 암호를 치도록 가상 키보드를 붙인다
BUNTOS_IMAGE_DIR="$image_dir" "$repo/tools/image/build.sh" "$board" vm -- -S -qmp "unix:$work/qmp.sock,server=on,wait=off" \
	-device virtio-gpu-pci -device virtio-keyboard-pci \
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

# e2e 6·4·7번은 키를 QMP로 가상 키보드에 넣고, 결과는 게스트 쪽 도우미가 읽는다. begin <번호> <도우미>로
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
	# 터미널: 편집기에서 ⌘C로 복사한 명령을 ⌥↩로 연 foot에 ⌘V(Ctrl+Shift+V로 간다)로 붙여 실행하고 ⌘W로 닫는다
	guest terminal-prepare
	[[ "$answer" == ready ]] || fail "e2e 6번: 터미널 시험을 준비하지 못했다 ($answer)"
	keys keys:meta_l+a sleep:0.3 keys:meta_l+c sleep:0.5 keys:alt+ret sleep:5 keys:meta_l+v sleep:1 keys:ret sleep:1.5
	guest terminal
	check "터미널에서 ⌘V로 붙여 실행한다" "$answer" "pasted 1"
	keys keys:meta_l+w sleep:2
	guest terminal
	check "터미널에서 ⌘W로 창을 닫는다" "$answer" "pasted 0"
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
	((failed == 0)) || fail "e2e 6번(키보드 단축키): ${failed}개 실패"
	echo "e2e 6번(키보드 단축키): ${SECONDS}초, ${passed}개 통과"
fi

# e2e 4번: 한글 입력. 키는 QMP로 넣어 libinput → 컴포지터 → fcitx5(input-method-v2, fcitx5-hangul) → 앱
# (text-input-v3)의 실제 길로 조합하게 하고, 결과는 게스트 쪽 도우미(tests/e2e/hangul.sh)가 에이전트(앱의 글)와
# 입력기 상태로 읽는다. 기대값은 macOS 한국어 2벌식과 fcitx5-hangul의 기본 동작이다. e2e 5·6번이 연 텍스트
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
	# 입력기가 키보드를 잡지 않았을 때(글 입력 칸 밖)도 바뀌면 컴포지터가 받은 한/영 키다
	guest focus-list
	check "글 입력 칸 밖으로 포커스를 옮기면 입력기가 키보드를 놓는다" "$answer" "org.buntos.Settings 0"
	keys keys:lang1 sleep:0.5
	guest state
	check "글 입력 칸 밖에서도 한/영 키로 바뀐다" "$answer" "hangul 한"

	# 텍스트 편집기에서 2벌식으로 조합한다. 칸마다 문서를 비우고 한글로 둔 뒤 QMP 동작($3…)을 넣는다
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

	# 터미널(foot): Phase 1 종료 조건의 터미널 창. 조합하는 중에 띄우면 새 창이 포커스를 가져가므로 조합하던
	# 글자가 편집기에 확정되어야 한다. foot에서 친 한글은 cat이 받은 바이트(UTF-8)로 본다. 조합 중 Enter는
	# 조합을 끝내고 줄을 바꾸며, Ctrl+D로 cat이 끝나면 foot도 닫힌다
	guest clear hangul
	[[ "$answer" == ready ]] || fail "e2e 4번: 텍스트 편집기를 비우지 못했다 ($answer)"
	keys type:gks sleep:0.3
	guest foot-prepare
	check "조합 중 새 창(터미널)이 뜨면 조합하던 글자가 원래 칸에 확정되고 터미널이 포커스를 받는다" \
		"$answer" "[한] ready"
	[[ "$answer" == *ready ]] || fail "e2e 4번: 터미널을 띄우지 못했다 ($answer)"
	keys type:dkssudgktpdy keys:ret sleep:0.3 keys:ctrl+d sleep:0.5
	guest foot
	check "터미널(foot)에 친 '안녕하세요'와 Enter가 그 차례의 UTF-8 바이트로 프로그램에 간다" "$answer" \
		"ec9588eb8595ed9598ec84b8ec9a940a 0"

	# Firefox(GTK3, text-input-v3)의 글 칸: 조합 중 Tab으로 다음 칸에 가면 조합하던 글자는 앞 칸에 남고, 같은
	# 창의 다른 칸을 눌러도 남는다
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
# 게스트 쪽 도우미(tests/e2e/session-lock.sh)가 컴포지터 API(GetLocked)와 저널로 읽는다
if [[ "$agent_refs" != NONE ]]; then
	push tests/e2e/session-lock.sh /tmp/e2e-session-lock.sh
	begin 7 /tmp/e2e-session-lock.sh
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

echo 'systemctl poweroff' >&3
while kill -0 "$vm_pid" 2>/dev/null; do
	((SECONDS < deadline)) || fail "전원이 꺼지지 않았다"
	sleep 1
done
echo "통과($board): 전원 꺼짐까지 ${SECONDS}초"
