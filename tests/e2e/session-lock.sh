#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# e2e 7번 게스트 쪽 도우미: 화면 잠금 (ARCHITECTURE 6.5, 9.2). 키(⌃⌘Q, 암호)는 호스트가 QMP로 가상 키보드에
# 넣고(boot-to-shell.sh), 이 스크립트는 상태만 읽는다. 잠금을 푸는 길은 잠금 화면에 사람이 친 암호뿐이므로
# 시험도 그 길(QMP 키 → libinput → 컴포지터 → bunt-lock → PAM)로만 푼다. 직렬 콘솔의 root로 실행하며 마지막
# 줄이 답이다.
# 사용법: session-lock.sh locked | ready | agent | journal | sleep
set -u

user=buntos
# 사람이 방금 입력했으면(QMP 키·누르기) 컴포지터가 에이전트의 입력을 미루고 "try again in N ms"로 알린다(사람 우선,
# ARCHITECTURE 6.3절). AI가 하듯 그만큼 기다렸다 다시 한다(다섯 번까지). 표준 출력과 오류는 그대로 낸다
agent_retry() {
	local err rc ms try
	err=$(mktemp)
	for try in 1 2 3 4 5 6; do
		"$@" 2>"$err"
		rc=$?
		ms=$(grep -a -o 'try again in [0-9]* ms' "$err" | grep -o '[0-9]*' | head -1)
		if [[ $rc -eq 0 || -z "$ms" || $try -eq 6 ]]; then
			cat "$err" >&2
			rm -f "$err"
			return "$rc"
		fi
		sleep "$(awk -v ms="$ms" 'BEGIN { printf "%.2f", (ms + 100) / 1000 }')"
	done
}
A() { agent_retry sudo -u "$user" XDG_RUNTIME_DIR=/run/user/1000 bunt agent "$@"; }
session_bus() { sudo -u "$user" DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "$@"; }
compositor() {
	session_bus busctl --user call org.buntos.Compositor /org/buntos/Compositor org.buntos.Compositor1 "$@" 2>&1
}
# 컴포지터가 띄운 잠금 화면의 출력은 세션의 bunt-comp 저널에 남는다 (/usr/libexec/buntos/session)
lock_log() { journalctl -b -q -t bunt-comp --no-pager 2>/dev/null | grep -c -- "bunt-lock: $1"; }
locked() { compositor GetLocked | awk '{print $2}'; }

case "${1:-}" in
locked)
	# 화면이 잠겼는지 (true, false)
	locked
	;;
ready)
	# 잠금 화면이 암호 칸을 띄울 때까지 기다린 뒤 잠김 여부와 띄운 수 (예: "true 1")
	for _ in $(seq 30); do
		(($(lock_log ready) > 0)) && break
		sleep 1
	done
	echo "$(locked) $(lock_log ready)"
	;;
agent)
	# 에이전트의 화면 읽기(snapshot)와 앱 목록 가운데 잠김을 이유로 거절된 수 (잠긴 동안 2, 풀린 뒤 0)
	refused=0
	A snapshot --app bunt-shell -i 2>&1 | grep -q 'the screen is locked' && refused=$((refused + 1))
	A apps 2>&1 | grep -q 'the screen is locked' && refused=$((refused + 1))
	echo "$refused"
	;;
journal)
	# 잠금 화면이 남긴 틀린 암호와 잠금 해제의 수 (예: "1 1")
	echo "$(lock_log 'wrong password') $(lock_log unlocked)"
	;;
sleep)
	# logind가 잠자기를 받는지(CanSuspend)와 셸이 로고 메뉴의 잠자기 동작을 켰는지. 셸은 동작을 D-Bus
	# (org.gtk.Actions)로 내보낸다. 가상 기기에서는 잠자기를 끄므로(buntos-vm-sleep.service) "no false"다
	can=$(busctl call org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager \
		CanSuspend 2>&1 | awk '{gsub(/"/, "", $2); print $2}')
	shell=$(session_bus busctl --user list 2>/dev/null | awk '$3 == "bunt-shell" {print $1; exit}')
	enabled=$(session_bus busctl --user call "$shell" /org/buntos/Shell org.gtk.Actions Describe s sleep \
		2>&1 | awk '{print $2}')
	echo "$can $enabled"
	;;
*)
	echo "사용법: $0 locked | ready | agent | journal | sleep" >&2
	exit 2
	;;
esac
