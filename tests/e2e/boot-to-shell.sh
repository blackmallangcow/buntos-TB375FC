#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# e2e 1번: 부팅 → 로그인 → 셸 (ARCHITECTURE 9.2. ADR-009의 Phase 0 드라이버: 시리얼 콘솔과 QMP)
# 빌드한 이미지를 시험 계정과 함께 띄운다(systemd 자격 증명 buntos.account를 SMBIOS로 넘긴다).
# 직렬 콘솔의 root 자동 로그인 뒤 셸에서 명령이 실행되는지 보고, 로그인 화면(bunt-greeter)에 QMP로
# 암호를 쳐 그 계정으로 들어간다. sudo가 암호를 묻는지 보고, 그래픽 셸(메뉴 막대, Dock)이 떴는지는
# 에이전트의 읽기 경로(bunt agent snapshot, ADR-009)로 본 뒤 끈다.
# 사용법: tests/e2e/boot-to-shell.sh <보드>   (먼저 tools/image/build.sh <보드>로 빌드한다)
# 환경 변수: E2E_TIMEOUT 전체 제한 시간(초, 기본 600)
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
board=${1:?"보드 이름이 필요하다 (bsp/ 아래 디렉터리 이름)"}
deadline=$((SECONDS + ${E2E_TIMEOUT:-600}))
# 시험 계정 (계정 이름:전체 이름:암호). 다른 시험과 개발 VM 도구(tools/image/vmctl.sh)도 같은 계정을 쓴다.
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
wait_for() {
	until console | grep -a -q -E "$1"; do
		if ! kill -0 "$vm_pid" 2>/dev/null || ((SECONDS >= deadline)); then
			return 1
		fi
		sleep 1
	done
}
fail() {
	echo "실패($board): $1" >&2
	echo "--- 콘솔 마지막 40줄" >&2
	console | tail -40 >&2
	exit 1
}

# 테스트가 빌드한 이미지를 바꾸지 않게, 멈춘 상태로 띄워 디스크에 임시 덮어쓰기를 씌운 뒤 실행한다.
# mkosi는 이미지를 -blockdev로 붙여 QEMU의 -snapshot이 효과가 없고, --ephemeral은 이미지 전체를 복사한다
# 그래픽 세션이 뜨도록 가상 GPU를 붙이고(화면은 내보내지 않는다), 로그인 화면에 암호를 치도록 가상 키보드를 붙인다
"$repo/tools/image/build.sh" "$board" vm -- -S -qmp "unix:$work/qmp.sock,server=on,wait=off" \
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

echo 'systemctl poweroff' >&3
while kill -0 "$vm_pid" 2>/dev/null; do
	((SECONDS < deadline)) || fail "전원이 꺼지지 않았다"
	sleep 1
done
echo "통과($board): 전원 꺼짐까지 ${SECONDS}초"
