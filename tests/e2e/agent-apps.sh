#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# e2e 5번 게스트 쪽: 에이전트가 앱 A의 값을 읽어 앱 B에 넣는다 (ARCHITECTURE 9.2, 6.4). 모든 조작과 확인을
# `bunt agent`로만 한다. 계산기에서 6 × 7을 계산해 결과를 읽고, 텍스트 편집기에 채운 뒤 입력기를 한글
# 모드로 두고 글을 더 친다(글 입력으로 넣으므로 바뀌지 않아야 한다). 시스템 설정 사이드바도 눌러 본다.
# 직렬 콘솔의 root로 실행한다(boot-to-shell.sh가 넣어 돌린다). 마지막 줄은 "E2E5_RESULT 통과 실패"다
set -u

user=buntos
A() { sudo -u "$user" XDG_RUNTIME_DIR=/run/user/1000 bunt agent "$@"; }
im() { sudo -u "$user" DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus fcitx5-remote "$@"; }
passed=0
failed=0
check() {
	if [[ "$2" == "$3" ]]; then
		echo "E2E5 통과: $1"
		passed=$((passed + 1))
	else
		echo "E2E5 실패: $1 (받은 값 '$2', 바란 값 '$3')"
		failed=$((failed + 1))
	fi
}
# 트리 글에서 그 줄의 ref
ref() { grep -m1 -- "$1" | grep -o 'ref=e[0-9]*' | cut -d= -f2; }
value() { grep -m1 -o 'value="[^"]*"' | sed 's/^value="\(.*\)"$/\1/'; }
# 앱을 열어 그 창의 트리를 낸다. 세션이 막 떴으면 앱이 포털을 기다리느라 20초 안에 창을 못 띄울 수 있어,
# 트리가 나올 때까지 다시 연다(떠 있는 앱은 그 창을 가져온다)
open_app() {
	local out
	for _ in 1 2 3; do
		out=$(A launch "$1") && grep -q 'ref=e' <<<"$out" && break
	done
	printf '%s\n' "$out"
}

# 다시 돌려도 같도록 이 시나리오가 쓰는 앱을 닫고 시작한다
pkill -f /usr/bin/gnome-calculator
pkill -f /usr/bin/gnome-text-editor
sleep 1

# 계산기: 이름이 아니라 데스크톱 ID로 열어 언어와 상관없게 한다
calculator=$(open_app org.gnome.Calculator)
check "계산기를 연다" "$(grep -c -m1 'button "="' <<<"$calculator")" 1
for key in 6 × 7 =; do
	A click "$(ref "button \"$key\"" <<<"$calculator")" >/dev/null
done
result=$(A snapshot --app org.gnome.Calculator -i | grep -m1 'textbox .*\[focused\]' | value)
check "6 × 7을 읽는다" "$result" 42
check "다시 열면 떠 있는 창을 쓴다" "$(A launch org.gnome.Calculator | head -1 | grep -c 'already open')" 1

# 텍스트 편집기: 읽은 값을 채우고, 입력기를 한글 모드로 둔 채 글을 더 친다
editor=$(open_app org.gnome.TextEditor)
document=$(ref textbox <<<"$editor")
A fill "$document" "계산 결과: $result" >/dev/null
A focus "$document" >/dev/null
im -o
check "입력기가 한글 모드다" "$(im)" 2
A type " (check 확인)" --ref "$document" >/dev/null
im -c
check "편집기에 결과가 들어간다" "$(A snapshot --app org.gnome.TextEditor -i | grep -m1 textbox | value)" \
	"계산 결과: 42 (check 확인)"

# 시스템 설정: 사이드바 항목을 누르면 답의 diff에 그 페이지의 제목이 나온다. 떠 있던 창이 이미 디스플레이
# 페이지일 수 있어 먼저 Wi-Fi로 옮긴다
settings=$(open_app org.buntos.Settings)
A click "$(ref 'listitem "Wi-Fi"' <<<"$settings")" >/dev/null
page=$(ref 'listitem "\(디스플레이\|Displays\)"' <<<"$settings")
shown=$(A click "$page" | grep -c -E '^\+ *- text: "(디스플레이|Displays)"$')
check "설정 사이드바를 누르면 디스플레이 페이지가 나온다" "$((shown > 0))" 1

echo "E2E5_RESULT $passed $failed"
