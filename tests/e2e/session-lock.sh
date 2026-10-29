#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# e2e 7번 게스트 쪽 도우미: 화면 잠금 (ARCHITECTURE 6.5, 9.2). 키(⌃⌘Q, 암호)는 호스트가 QMP로 가상 키보드에
# 넣고(boot-to-shell.sh), 이 스크립트는 상태만 읽는다. 잠금을 푸는 길은 잠금 화면에 사람이 친 암호뿐이므로
# 시험도 그 길(QMP 키 → libinput → 컴포지터 → bunt-lock → PAM)로만 푼다. 직렬 콘솔의 root로 실행하며 마지막
# 줄이 답이다.
# 사용법: session-lock.sh locked | ready | agent | journal
set -u

user=buntos
A() { sudo -u "$user" XDG_RUNTIME_DIR=/run/user/1000 bunt agent "$@"; }
compositor() {
	sudo -u "$user" DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus \
		busctl --user call org.buntos.Compositor /org/buntos/Compositor org.buntos.Compositor1 "$@" 2>&1
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
*)
	echo "사용법: $0 locked | ready | agent | journal" >&2
	exit 2
	;;
esac
