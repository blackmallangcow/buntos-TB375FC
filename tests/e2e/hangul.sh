#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# e2e 4번 게스트 쪽 도우미: 한글 입력 (ARCHITECTURE 9.2, 5.2, 7장). 키는 호스트가 QMP로 가상 키보드에
# 넣고(boot-to-shell.sh), 이 스크립트는 준비와 확인만 한다. 그래서 키가 실제 입력과 같은 길(libinput →
# 컴포지터 → fcitx5(input-method-v2, fcitx5-hangul) → 앱(text-input-v3))을 거쳐 조합된다. e2e 5번의
# type은 글 입력으로 확정해 조합을 거치지 않는다. 확인은 에이전트(앱의 글)와 입력기 상태(fcitx5)로 한다.
# 직렬 콘솔의 root로 실행하며 마지막 줄이 답이다(답이 없으면 호스트가 기다리므로 늘 한 줄을 낸다).
# 사용법: hangul.sh prepare | state | focus-list | clear hangul|english | value | focus-other |
#         other-value | foot-prepare | foot | firefox-prepare | firefox-wait first|second |
#         firefox-values | firefox-focus first|second | cleanup
set -u

user=buntos
foot_file=/tmp/e2e-hangul-foot
page=/tmp/e2e-hangul.html
profile=/tmp/e2e-hangul-firefox
session=(XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus)
A() { sudo -u "$user" "${session[@]}" bunt agent "$@"; }
im() { sudo -u "$user" "${session[@]}" fcitx5-remote "$@"; }
compositor() {
	sudo -u "$user" "${session[@]}" \
		busctl --user call org.buntos.Compositor /org/buntos/Compositor org.buntos.Compositor1 "$@" 2>&1
}
# 앱을 세션 사용자로 띄운다. 로그인 세션 밖에서 띄우므로 앱이 쓰는 환경을 함께 준다
spawn() {
	(cd /tmp && sudo -u "$user" env "${session[@]}" WAYLAND_DISPLAY=wayland-1 LANG=ko_KR.UTF-8 \
		GSK_RENDERER=cairo setsid "$@" >/dev/null 2>&1 &)
}
# 트리 글에서 그 줄의 ref와 글자 칸의 값
ref() { grep -m1 -- "$1" | grep -o 'ref=e[0-9]*' | cut -d= -f2; }
value() { grep -m1 -o 'value="[^"]*"' | sed 's/^value="\(.*\)"$/\1/'; }
# 키보드 포커스를 가진 앱 ID나 레이어 이름
focus_app() { compositor GetFocus | sed -E 's/^ussb [0-9]+ "([^"]*)".*/\1/'; }
# 입력기가 글 입력 칸과 이어져 키보드를 잡았으면 1, 아니면 0. fcitx5의 Wayland 입력 문맥이 포커스를
# 가졌는지로 본다. 앱이 글 입력을 켜기 전에 친 키는 입력기를 거치지 않고 앱에 바로 가므로, 키를 넣기
# 전에 이것을 기다린다
im_focus() {
	sudo -u "$user" "${session[@]}" busctl --user call org.fcitx.Fcitx5 /controller \
		org.fcitx.Fcitx.Controller1 DebugInfo 2>/dev/null | grep -o 'wayland_v2[^\\]*focus:[01]' |
		grep -o '[01]$' | head -1
}
wait_im_focus() {
	for _ in $(seq 50); do
		[[ "$(im_focus)" == "$1" ]] && return 0
		sleep 0.1
	done
	return 1
}
# 메뉴 막대의 입력 소스 표시 (A, 한)
badge() {
	A snapshot --app bunt-shell | grep -A1 -E 'button "(입력 소스|Input Source)"' |
		grep -o 'text: "[^"]*"' | sed 's/^text: "\(.*\)"$/\1/'
}
editor_doc() { A snapshot --app org.gnome.TextEditor -i | ref textbox; }
editor_value() { A snapshot --app org.gnome.TextEditor -i | grep -m1 textbox | value; }
# 앱을 열어 그 창의 트리를 낸다. 막 뜬 앱은 포털을 기다리느라 늦을 수 있어 트리가 나올 때까지 다시 연다
open_app() {
	local out
	for _ in 1 2 3; do
		out=$(A launch "$1") && grep -q 'ref=e' <<<"$out" && break
	done
	printf '%s\n' "$out"
}
firefox_field() { A snapshot --app firefox-esr -i | ref "textbox \"$1\""; }
firefox_values() {
	local tree
	tree=$(A snapshot --app firefox-esr -i)
	echo "[$(grep -m1 'textbox "first"' <<<"$tree" | value)][$(grep -m1 'textbox "second"' <<<"$tree" | value)]"
}
# 편집기를 앞에 두고 문서를 비운 뒤 포커스를 준다
clear_editor() {
	local doc
	open_app org.gnome.TextEditor >/dev/null
	doc=$(editor_doc)
	[[ -n "$doc" ]] && A fill "$doc" "" >/dev/null && A focus "$doc" >/dev/null
}

case "${1:-}" in
prepare)
	# 다른 창 시험에 쓸 시스템 설정을 먼저 띄우고(e2e 5·6번이 띄운 창을 그대로 쓴다) 텍스트 편집기를 앞에
	# 둔다. 입력기는 영문
	open_app org.buntos.Settings >/dev/null
	result=not-ready
	clear_editor && im -c && wait_im_focus 1 && result=ready
	echo "$result"
	;;
state)
	# 입력기의 지금 입력 소스와 메뉴 막대의 표시. 셸은 400ms마다 따라가므로 같아질 때까지 3초 기다린다
	for _ in $(seq 15); do
		current=$(im -n)
		shown=$(badge)
		case "$current:$shown" in
		hangul:한 | keyboard-us:A) break ;;
		esac
		sleep 0.2
	done
	echo "$current $shown"
	;;
focus-list)
	# 키보드 포커스를 글 입력 칸이 아닌 곳(시스템 설정의 사이드바 항목)에 둔다. 입력기는 키보드를 놓는다.
	# 포커스를 가진 앱과 입력기가 키보드를 잡았는지를 낸다
	# 검색 칸에 글이 있으면 사이드바가 걸러지므로 먼저 비운다
	settings=$(open_app org.buntos.Settings)
	A fill "$(ref 'textbox "\(검색\|Search\)"' <<<"$settings")" "" >/dev/null 2>&1
	A click "$(A snapshot --app org.buntos.Settings -i | ref 'listitem "Wi-Fi"')" --pointer >/dev/null
	wait_im_focus 0
	echo "$(focus_app) $(im_focus)"
	;;
clear)
	# 편집기 문서를 비워 포커스를 주고 입력기를 그 입력 소스로 둔다 (준비이므로 키가 아니라 fcitx5-remote로)
	result=not-ready
	if clear_editor; then
		if [[ "${2:-}" == hangul ]]; then im -o; else im -c; fi
		wait_im_focus 1 && result=ready
	fi
	echo "$result"
	;;
value)
	# 편집기 문서의 글. 끝의 공백도 보이도록 대괄호로 감싼다. GTK는 조합 중인 글자를 문서에 넣지 않는다
	echo "[$(editor_value)]"
	;;
focus-other)
	# 조합하는 중에 다른 창(시스템 설정)의 검색 칸을 누른다. 에이전트는 그 창을 앞으로 가져와 포인터로
	# 칸을 누른다(사람이 다른 창을 누를 때와 같은 컴포지터 경로). 편집기의 글, 검색 칸의 글, 포커스를 낸다
	A focus "$(A snapshot --app org.buntos.Settings -i | ref 'textbox "\(검색\|Search\)"')" >/dev/null
	wait_im_focus 1
	sleep 0.5
	echo "[$(editor_value)] [$(A snapshot --app org.buntos.Settings -i | grep -m1 textbox | value)] $(focus_app)"
	;;
other-value)
	# 시스템 설정 검색 칸의 글. 읽은 뒤 비워 둔다
	tree=$(A snapshot --app org.buntos.Settings -i)
	echo "[$(grep -m1 textbox <<<"$tree" | value)]"
	A fill "$(ref textbox <<<"$tree")" "" >/dev/null 2>&1
	;;
foot-prepare)
	# 터미널(foot)을 띄워 cat이 받은 바이트를 파일에 쓰게 한다. 새 창이 키보드 포커스를 가져가므로, 조합하던
	# 글자가 있으면 편집기에 확정되어야 한다. 편집기의 글과 준비 결과를 낸다
	rm -f "$foot_file"
	spawn foot --working-directory=/tmp sh -c "cat >$foot_file"
	for _ in $(seq 50); do
		[[ "$(focus_app)" == foot ]] && break
		sleep 0.2
	done
	result=not-ready
	[[ "$(focus_app)" == foot ]] && wait_im_focus 1 && result=ready
	echo "[$(editor_value)] $result"
	;;
foot)
	# cat이 받은 바이트(16진수)와 떠 있는 foot 수. Ctrl+D로 cat이 끝나면 foot도 닫힌다
	for _ in $(seq 15); do
		pgrep -x foot >/dev/null || break
		sleep 0.2
	done
	echo "$(od -An -tx1 -v "$foot_file" 2>/dev/null | tr -d ' \n') $(pgrep -c -x foot)"
	;;
firefox-prepare)
	# 칸 둘인 페이지를 새 프로필의 Firefox로 열고 첫 칸에 포커스를 준다. e2e 5번이 쓴 기본 프로필과 따로 두어
	# 앞 시험의 남은 상태(끝낸 창, 잠금)와 상관없게 한다. 처음 켤 때 여는 안내 페이지는 끈다
	cat >"$page" <<'HTML'
<!doctype html>
<meta charset="utf-8">
<title>e2e hangul</title>
<p><input aria-label="first"></p>
<p><input aria-label="second"></p>
HTML
	chmod 644 "$page"
	rm -rf "$profile"
	install -d -o "$user" "$profile"
	cat >"$profile/user.js" <<'JS'
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.aboutwelcome.enabled", false);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("datareporting.policy.firstRunURL", "");
user_pref("toolkit.telemetry.reportingpolicy.firstRun", false);
JS
	chown "$user" "$profile/user.js"
	spawn firefox-esr --no-remote --profile "$profile" "file://$page"
	# 소프트웨어 렌더링 VM에서는 Firefox가 뜨는 데 1분 넘게 걸리기도 한다
	first=
	for _ in $(seq 60); do
		first=$(firefox_field first 2>/dev/null)
		[[ -n "$first" ]] && break
		sleep 2
	done
	result=no-window
	if [[ -n "$first" ]]; then
		result=not-ready
		A focus "$first" >/dev/null && im -o && wait_im_focus 1 && result=ready
	fi
	echo "$result"
	;;
firefox-wait)
	# 그 칸이 포커스를 받고 입력기가 그 칸과 다시 이어지기를 기다린다 (Firefox는 칸을 옮길 때 글 입력을
	# 껐다 켠다)
	for _ in $(seq 25); do
		A snapshot --app firefox-esr -i | grep -q "textbox \"$2\".*\[focused\]" && break
		sleep 0.2
	done
	sleep 1
	result=not-ready
	wait_im_focus 1 && result=ready
	echo "$result"
	;;
firefox-values)
	# 두 칸의 글. Firefox는 조합 중인 글자도 칸의 글에 넣는다
	firefox_values
	;;
firefox-focus)
	# 조합하는 중에 같은 창의 다른 칸에 포커스를 준다
	A focus "$(firefox_field "$2")" >/dev/null
	sleep 1
	firefox_values
	;;
cleanup)
	pkill -f -- "--profile $profile"
	pkill -x foot
	im -c
	sleep 1
	rm -rf "$profile" "$page" "$foot_file"
	echo "done"
	;;
*)
	sed -n '9,11p' "$0" | sed 's/^# \{0,1\}//' >&2
	echo usage
	exit 2
	;;
esac
