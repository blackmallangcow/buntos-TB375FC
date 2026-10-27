#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# e2e 1번: 부팅 → 로그인 → 셸 (ARCHITECTURE 9.2. ADR-009의 Phase 0 드라이버: 시리얼 콘솔)
# 빌드한 이미지를 띄워 콘솔 자동 로그인 뒤 셸에서 명령이 실행되는지 보고, 끈다.
# 사용법: tests/e2e/boot-to-shell.sh <보드>   (먼저 tools/image/build.sh <보드>로 빌드한다)
# 환경 변수: E2E_TIMEOUT 전체 제한 시간(초, 기본 600)
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
board=${1:?"보드 이름이 필요하다 (bsp/ 아래 디렉터리 이름)"}
deadline=$((SECONDS + ${E2E_TIMEOUT:-600}))

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
"$repo/tools/image/build.sh" "$board" vm -- -S -qmp "unix:$work/qmp.sock,server=on,wait=off" \
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

echo 'systemctl poweroff' >&3
while kill -0 "$vm_pid" 2>/dev/null; do
	((SECONDS < deadline)) || fail "전원이 꺼지지 않았다"
	sleep 1
done
echo "통과($board): 전원 꺼짐까지 ${SECONDS}초"
