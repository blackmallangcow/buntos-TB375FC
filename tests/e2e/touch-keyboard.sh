#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# e2e 11번 게스트 쪽 도우미: 화면 키보드와 단축 줄 (ARCHITECTURE 5.1, 5.2, 9.2). 손가락은 호스트가 QMP로 가상 터치 화면에
# 넣고(boot-to-shell.sh), 이 스크립트는 준비와 확인만 한다. 키보드는 손가락 → 컴포지터 → 셸의 화면 키보드 → bunt_osk_v1 →
# 컴포지터의 길을 거치고, 결과는 앱의 글(에이전트)과 터미널의 cat이 받은 바이트로 읽는다. 직렬 콘솔의 root로 실행하며 마지막
# 줄이 답이다(답이 없으면 호스트가 기다리므로 늘 한 줄을 낸다).
# 사용법: touch-keyboard.sh prepare | mode pointer|touch | value | window-height | select-all | password-prepare |
#         password-cleanup | lock | locked | foot-prepare | foot | cleanup | copy-prepare
set -u

user=buntos
foot_file=/tmp/e2e-touch-foot
session=(XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus)
A() { sudo -u "$user" "${session[@]}" bunt agent "$@"; }
im() { sudo -u "$user" "${session[@]}" fcitx5-remote "$@"; }
compositor() {
	sudo -u "$user" "${session[@]}" \
		busctl --user call org.buntos.Compositor /org/buntos/Compositor org.buntos.Compositor1 "$@" 2>&1
}
spawn() {
	(cd /tmp && sudo -u "$user" env "${session[@]}" WAYLAND_DISPLAY=wayland-1 LANG=ko_KR.UTF-8 \
		GSK_RENDERER=cairo setsid "$@" >/dev/null 2>&1 &)
}
editor_doc() { A snapshot --app org.gnome.TextEditor -i | grep -m1 textbox | grep -o 'ref=e[0-9]*' | cut -d= -f2; }
editor_value() {
	A snapshot --app org.gnome.TextEditor -i | grep -m1 textbox | grep -m1 -o 'value="[^"]*"' |
		sed 's/^value="\(.*\)"$/\1/'
}
focus_app() { compositor GetFocus | sed -E 's/^ussb [0-9]+ "([^"]*)".*/\1/'; }
input_mode() { compositor GetInputMode | sed -E 's/^s "(.*)"$/\1/'; }
# 트리 글에서 그 줄의 ref
ref() { grep -m1 -- "$1" | grep -o 'ref=e[0-9]*' | cut -d= -f2; }
settings_ref() { A snapshot --app org.buntos.Settings -i | ref "$1"; }
locked() { compositor GetLocked | awk '{print $2}'; }

case "${1:-}" in
prepare)
	# 입력 소스를 영문에 두고 텍스트 편집기의 문서를 비운다. e2e 4번이 한글로 두었을 수 있다
	im -c
	A launch org.gnome.TextEditor >/dev/null
	doc=$(editor_doc)
	A fill "$doc" "" >/dev/null && A focus "$doc" >/dev/null && echo ready
	;;
mode)
	# 입력 모드가 그 값이 될 때까지 기다린다. 키보드를 떼고 붙이는 데 게스트가 몇 초 걸린다
	for _ in $(seq 60); do
		[[ "$(input_mode)" == "$2" ]] && break
		sleep 1
	done
	input_mode
	;;
value)
	# 텍스트 편집기의 글
	echo "[$(editor_value)]"
	;;
window-height)
	# 텍스트 편집기 창의 높이: 창 그림(PNG)의 머리에 적힌 값. 전체 화면이라 창의 아래 끝과 같다
	A screenshot --app org.gnome.TextEditor --out /tmp/e2e-touch-window.png >/dev/null 2>&1
	od -An -tu1 -j20 -N4 /tmp/e2e-touch-window.png | awk '{print $1 * 16777216 + $2 * 65536 + $3 * 256 + $4}'
	rm -f /tmp/e2e-touch-window.png
	;;
select-all)
	# 문서의 글을 모두 고른다. 손가락으로 고르는 것은 GTK의 일이고(길게 눌러 끌기) 누른 자리에 민감해 시험은 키로 한다
	A press ctrl+a --ref "$(editor_doc)" --keep-focus >/dev/null 2>&1
	echo selected
	;;
password-prepare)
	# 시스템 설정을 화면에 채우고 암호 변경 시트를 연다. 칸은 호스트가 손가락으로 누른다: 창이 화면에 차 있어 자리가 늘 같다. 포인터로
	# 누르면 GTK가 글 입력을 다시 켜지 않아 키보드가 뜨지 않는다. 답은 포커스를 가진 암호 칸의 수다
	A launch org.buntos.Settings >/dev/null
	A click "$(settings_ref 'button "최대화"')" >/dev/null
	sleep 1
	A click "$(settings_ref 'listitem "사용자 및 그룹"')" >/dev/null
	sleep 1
	A click "$(settings_ref 'button "암호 변경…"')" >/dev/null
	sleep 2
	A snapshot --app org.buntos.Settings -i | grep -c 'textbox "이전 암호".*focused.*secret'
	;;
password-cleanup)
	# 시트를 닫고 텍스트 편집기로 돌아온다
	A click "$(settings_ref 'button "취소"')" >/dev/null
	sleep 0.5
	A launch org.gnome.TextEditor >/dev/null
	focus_app
	;;
foot-prepare)
	# 터미널(foot)을 화면에 채워 띄우고 cat이 받은 바이트를 파일에 쓰게 한다. 글자 자리가 늘 같다
	rm -f "$foot_file"
	spawn foot --fullscreen --working-directory=/tmp sh -c "cat >$foot_file"
	for _ in $(seq 50); do
		[[ "$(focus_app)" == foot ]] && break
		sleep 0.2
	done
	sleep 0.5
	focus_app
	;;
foot)
	# cat이 받은 바이트. 줄 단위로 넘어오므로 Return을 친 뒤에 읽는다
	sleep 0.5
	od -An -c -v "$foot_file" 2>/dev/null | tr -s ' ' | tr -d '\n'
	echo
	;;
lock)
	# 화면을 잠그고 잠금 화면이 뜰 때까지 기다린다
	loginctl lock-sessions
	for _ in $(seq 30); do
		[[ "$(locked)" == true ]] && break
		sleep 0.5
	done
	sleep 3
	locked
	;;
locked)
	# 잠겼는지 (잠금을 푸는 데 게스트가 잠깐 걸린다)
	for _ in $(seq 20); do
		[[ "$(locked)" == false ]] && break
		sleep 0.5
	done
	locked
	;;
copy-prepare)
	# 텍스트 편집기에 복사할 글을 채운다. 호스트가 ⌘A ⌘C로 복사한다
	A launch org.gnome.TextEditor >/dev/null
	doc=$(editor_doc)
	A fill "$doc" secretclip >/dev/null && A focus "$doc" >/dev/null && echo ready
	;;
cleanup)
	pkill -x foot
	rm -f "$foot_file"
	echo done
	;;
*)
	echo "unknown: $*"
	;;
esac
