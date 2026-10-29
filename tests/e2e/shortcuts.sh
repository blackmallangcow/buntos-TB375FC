#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# e2e 6번 게스트 쪽 도우미: 키보드 단축키 (ARCHITECTURE 9.2, 5.3). 키는 호스트가 QMP로 가상 키보드에
# 넣고(boot-to-shell.sh), 이 스크립트는 준비와 확인만 한다. 그래서 키가 실제 입력과 같은 길(libinput →
# 컴포지터 단축키 → 앱)을 거친다. 직렬 콘솔의 root로 실행하며 마지막 줄이 답이다.
# 사용법: shortcuts.sh prepare | value | focus | screenshots | dbus | terminal-prepare | terminal |
#         chord-prepare | input-method | chord-reset
set -u

user=buntos
A() { sudo -u "$user" XDG_RUNTIME_DIR=/run/user/1000 bunt agent "$@"; }
im() { sudo -u "$user" DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus fcitx5-remote "$@"; }
compositor() {
	sudo -u "$user" DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus \
		busctl --user call org.buntos.Compositor /org/buntos/Compositor org.buntos.Compositor1 "$@" 2>&1
}
document() { A snapshot --app org.gnome.TextEditor -i | grep -m1 textbox | grep -o 'ref=e[0-9]*' | cut -d= -f2; }

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
*)
	echo "사용법: $0 prepare | value | focus | screenshots | dbus | terminal-prepare | terminal | chord-prepare | input-method | chord-reset" >&2
	exit 2
	;;
esac
