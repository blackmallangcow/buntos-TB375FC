#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# e2e 6번 게스트 쪽 도우미: 키보드 단축키 (ARCHITECTURE 9.2, 5.3). 키는 호스트가 QMP로 가상 키보드에
# 넣고(boot-to-shell.sh), 이 스크립트는 준비와 확인만 한다. 그래서 키가 실제 입력과 같은 길(libinput →
# 컴포지터 단축키 → 앱)을 거친다. 직렬 콘솔의 root로 실행하며 마지막 줄이 답이다.
# 사용법: shortcuts.sh prepare | value | focus | screenshots | dbus | terminal-prepare | terminal |
#         chord-prepare | input-method | chord-reset | minimize-button | restore-dock | switcher |
#         preview | toolbar | volume-prepare | volume
set -u

user=buntos
A() { sudo -u "$user" XDG_RUNTIME_DIR=/run/user/1000 bunt agent "$@"; }
im() { sudo -u "$user" DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus fcitx5-remote "$@"; }
compositor() {
	sudo -u "$user" DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus \
		busctl --user call org.buntos.Compositor /org/buntos/Compositor org.buntos.Compositor1 "$@" 2>&1
}
audio() { sudo -u "$user" XDG_RUNTIME_DIR=/run/user/1000 "$@"; }
document() { A snapshot --app org.gnome.TextEditor -i | grep -m1 textbox | grep -o 'ref=e[0-9]*' | cut -d= -f2; }
# 셸의 Dock에서 그 이름의 단추 (최소화한 창은 창 제목이 이름이다)
dock_button() { A snapshot --app bunt-shell -i | grep -aF "button \"$1\"" | grep -o 'ref=e[0-9]*' | head -1 | cut -d= -f2; }
# 최소화 단추로 내린 텍스트 편집기 창의 제목
minimized_title=/tmp/e2e-minimized-title
# 셸의 그 이름의 창이 뜰 때까지 20초 기다려 shown이나 hidden을 답한다 (숨은 창은 에이전트에 보이지 않는다)
shell_window() {
	for _ in $(seq 40); do
		A snapshot --app bunt-shell -i | grep -aq "window \"$1\"" && break
		sleep 0.5
	done
	A snapshot --app bunt-shell -i | grep -aq "window \"$1\"" && echo shown || echo hidden
}

case "${1:-}" in
prepare)
	# 텍스트 편집기에 글을 채우고 문서에 포커스를 준다
	A launch org.gnome.TextEditor >/dev/null
	doc=$(document)
	A fill "$doc" "one two" >/dev/null && A focus "$doc" >/dev/null && echo ready
	;;
terminal-prepare)
	# 텍스트 편집기에 터미널에서 붙여 실행할 명령을 채운다
	rm -f /tmp/e2e-foot
	A launch org.gnome.TextEditor >/dev/null
	doc=$(document)
	A fill "$doc" "echo pasted >/tmp/e2e-foot" >/dev/null && A focus "$doc" >/dev/null && echo ready
	;;
terminal)
	# 터미널에서 실행한 명령이 만든 파일의 글과 떠 있는 터미널(foot) 수
	echo "$(cat /tmp/e2e-foot 2>/dev/null) $(pgrep -c -x foot)"
	;;
chord-prepare)
	# 입력 소스 전환을 조합 키만(⌃⇧, Windows의 입력 언어 전환)으로 바꾸고 입력기를 영문에 둔다
	compositor SetShortcut ssb input.switch-source "<Control><Shift>" true >/dev/null
	im -c
	sleep 0.5
	im -n
	;;
input-method)
	# 지금 입력기 (keyboard-us, hangul)
	im -n
	;;
chord-reset)
	compositor ResetShortcuts s input >/dev/null
	im -c
	echo done
	;;
value)
	# 문서의 글. 끝의 공백도 보이도록 대괄호로 감싼다
	text=$(A snapshot --app org.gnome.TextEditor -i | grep -m1 textbox | grep -o 'value="[^"]*"' |
		sed 's/^value="\(.*\)"$/\1/')
	echo "[$text]"
	;;
focus)
	# 키보드 포커스를 가진 앱 ID나 레이어 이름
	compositor GetFocus | sed -E 's/^ussb [0-9]+ "([^"]*)".*/\1/'
	;;
screenshots)
	# 사진 폴더의 PNG 수 (파일 머리로 센다)
	count=0
	for file in /home/$user/Pictures/*.png; do
		[[ -f "$file" ]] && [[ "$(head -c 4 "$file" | tail -c 3)" == PNG ]] && count=$((count + 1))
	done
	echo "$count"
	;;
dbus)
	# 시스템 설정이 쓰는 단축키 API: 목록, 겹치는 키 거절, 바꾸기, 기본값으로 되돌리기
	count=$(compositor GetShortcuts | awk '{print $2}')
	conflict=$(compositor SetShortcut ssb app.copy "<Super>v" true | grep -c 'already used by app.paste')
	compositor SetShortcut ssb app.copy "<Alt>c" true >/dev/null
	changed=$(compositor GetShortcuts | grep -c '"accelerator" s "<Alt>c"')
	compositor ResetShortcuts s "" >/dev/null
	reset=$(compositor GetShortcuts | grep -c '"accelerator" s "<Alt>c"')
	echo "$((count > 40)) $conflict $changed $reset"
	;;
minimize-button)
	# 텍스트 편집기 창의 최소화 단추(앱이 xdg_toplevel.set_minimized를 보낸다)를 누르고, 그 창이 Dock의
	# 최소화 칸에 창 제목으로 놓일 때까지 기다린다. 그 단추 수를 답한다
	A apps | grep -a -A3 '^gnome-text-editor' | grep -a -m1 'window "' |
		sed 's/^ *- window "\(.*\)"\( \[.*\)\?$/\1/' >"$minimized_title"
	title=$(cat "$minimized_title")
	button=$(A snapshot --app org.gnome.TextEditor -i | grep -a -m1 'button "최소화"' | grep -o 'ref=e[0-9]*' | cut -d= -f2)
	A click "$button" --app org.gnome.TextEditor >/dev/null
	for _ in $(seq 40); do
		[[ -n "$(dock_button "$title")" ]] && break
		sleep 0.5
	done
	echo "docked $(A snapshot --app bunt-shell -i | grep -acF "button \"$title\"")"
	;;
restore-dock)
	# Dock의 최소화 칸에서 그 창을 누르고, 키보드 포커스를 가진 앱 ID를 답한다
	A click "$(dock_button "$(cat "$minimized_title")")" --app bunt-shell >/dev/null
	sleep 1
	compositor GetFocus | sed -E 's/^ussb [0-9]+ "([^"]*)".*/\1/'
	;;
switcher)
	# ⌘를 누른 채 ⇥를 누른 동안 셸의 앱 전환 창이 뜨는지 (뜨기 전 잠깐 기다리고, 느린 VM은 처음 그리기가 늦다)
	shell_window bunt-switcher
	;;
preview)
	# 저장한 스크린샷의 미리 보기(플로팅 썸네일)가 뜨는지
	shell_window bunt-screenshot-preview
	;;
toolbar)
	# ⇧⌘5 스크린샷 도구 막대가 뜨는지
	shell_window bunt-screenshot
	;;
volume-prepare)
	# 소리 장치가 없는 VM에 출력 장치를 둔다. 셸은 음량을 3초마다 읽는다
	audio pw-cli create-node adapter '{ factory.name=support.null-audio-sink node.name=e2e-sink media.class=Audio/Sink object.linger=true audio.position=[FL FR] }' >/dev/null
	sleep 1
	audio wpctl set-volume @DEFAULT_AUDIO_SINK@ 0.5
	sleep 4
	audio wpctl get-volume @DEFAULT_AUDIO_SINK@
	;;
volume)
	# 음량과 음량 키의 표시
	echo "$(audio wpctl get-volume @DEFAULT_AUDIO_SINK@) $(shell_window bunt-volume-hud)"
	;;
*)
	echo "사용법: $0 prepare | value | focus | screenshots | dbus | terminal-prepare | terminal | chord-prepare | input-method | chord-reset | minimize-button | restore-dock | switcher | preview | toolbar | volume-prepare | volume" >&2
	exit 2
	;;
esac
