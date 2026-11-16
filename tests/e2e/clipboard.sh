#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# e2e 10번 게스트 쪽 도우미: 터미널의 클립보드와 클립보드 기록 (ARCHITECTURE 5.2, 9.2). 키는 호스트가 QMP로 넣고
# (boot-to-shell.sh), 이 스크립트는 준비와 확인만 한다. 직렬 콘솔의 root로 실행하며 마지막 줄이 답이다.
# 클립보드 기록은 사람만 답하는 창이라 에이전트로 읽지 못한다. 그래서 항목을 키로 골라 wl-paste로 무엇이 들어왔는지 본다.
# 사용법: clipboard.sh globals | sandbox-globals | roundtrip | prepare | value | paste | copy 글 | copy-secret 글 |
#         types | png | focus | wait-focus 이름 | setting | set-setting true/false | set-period 값 | clear-from-settings |
#         agent-refused 글 | a11y-scan 글 | no-trace 글 | locked | paste-locked
set -u

user=buntos
runtime=/run/user/1000
probe=/tmp/e2e-wayland-probe.py
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
A() { agent_retry sudo -u "$user" XDG_RUNTIME_DIR="$runtime" bunt agent "$@"; }
session() { sudo -u "$user" DBUS_SESSION_BUS_ADDRESS="unix:path=$runtime/bus" "$@"; }
compositor() { session busctl --user call org.buntos.Compositor /org/buntos/Compositor org.buntos.Compositor1 "$@" 2>&1; }
display() { basename "$(ls "$runtime"/wayland-* | grep -v '\.lock$' | head -1)"; }
# 세션의 Wayland에 붙는 프로그램을 시험 계정으로 돌린다
W() { sudo -u "$user" env XDG_RUNTIME_DIR="$runtime" WAYLAND_DISPLAY="$(display)" "$@"; }
focus() { compositor GetFocus | sed -E 's/^ussb [0-9]+ "([^"]*)".*/\1/'; }
document() { A snapshot --app org.gnome.TextEditor -i | grep -m1 textbox | grep -o 'ref=e[0-9]*' | cut -d= -f2; }

case "${1:-}" in
globals | sandbox-globals)
	W python3 -I "$probe" "$1"
	;;
roundtrip)
	# wl-copy와 wl-paste는 창을 띄우지 않아 포커스가 그대로다
	before=$(focus)
	printf '%s' "왕복 e2e 클립보드" | W wl-copy >/dev/null 2>&1
	text=$(W timeout 5 wl-paste -n)
	[[ "$(focus)" == "$before" ]] && same=same || same=moved
	echo "[$text] $same"
	;;
prepare)
	# 텍스트 편집기에 글을 채우고 문서에 포커스를 준다
	A launch org.gnome.TextEditor >/dev/null
	for _ in $(seq 30); do
		doc=$(document)
		[[ -n "$doc" ]] && break
		sleep 1
	done
	A fill "$doc" "from editor" >/dev/null && A focus "$doc" >/dev/null && echo ready
	;;
value)
	text=$(A snapshot --app org.gnome.TextEditor -i | grep -m1 textbox | grep -o 'value="[^"]*"' |
		sed 's/^value="\(.*\)"$/\1/')
	echo "[$text]"
	;;
paste)
	echo "[$(W timeout 5 wl-paste -n 2>&1)]"
	;;
copy)
	# wl-copy는 뒤에 남아 내용을 내준다. 이 스크립트의 출력을 붙들지 않게 닫는다. 기록이 읽을 틈을 준다
	printf '%s' "$2" | W wl-copy >/dev/null 2>&1
	sleep 1
	echo copied
	;;
copy-secret)
	W python3 -I "$probe" copy-secret "$2" | tail -1
	sleep 1
	;;
types)
	# 지금 클립보드의 형식
	W timeout 5 wl-paste --list-types | tr '\n' ' '
	echo
	;;
png)
	# 지금 클립보드의 그림이 PNG인지 파일 머리로 본다
	W timeout 10 wl-paste -t image/png | head -c 4 | tail -c 3
	echo
	;;
focus)
	focus
	;;
wait-focus)
	# 그 앱이나 레이어가 키보드를 받을 때까지 기다린다. 바쁜 VM에서는 패널을 처음 여는 데 몇 초 걸린다
	for _ in $(seq 150); do
		[[ "$(focus)" == "$2" ]] && break
		sleep 0.2
	done
	focus
	;;
setting)
	session gsettings get org.buntos.shell clipboard-history
	;;
set-setting)
	session gsettings set org.buntos.shell clipboard-history "$2"
	sleep 1
	session gsettings get org.buntos.shell clipboard-history
	;;
set-period)
	session gsettings set org.buntos.shell clipboard-history-period "$2"
	sleep 1
	session gsettings get org.buntos.shell clipboard-history-period
	;;
clear-from-settings)
	# 시스템 설정의 클립보드 기록 지우기와 같다: 키를 늘린다
	now=$(session gsettings get org.buntos.shell clipboard-history-clear | awk '{print $NF}')
	session gsettings set org.buntos.shell clipboard-history-clear "$((now + 1))"
	sleep 1
	echo cleared
	;;
agent-refused)
	# 기록이 떠 있는 동안 에이전트: 키를 넣으면 거절, 셸을 읽어도 그 창과 글이 없다. "거절된 수 창이나 글이 보인 수" (바람: 1 0)
	refused=0
	A press Down 2>&1 | grep -q 'only the user can answer' && refused=1
	shell=$(A snapshot --app bunt-shell 2>&1)
	shown=$(grep -a -c -e 'bunt-clipboard' -e "$2" <<<"$shell")
	echo "$refused $shown"
	;;
a11y-scan)
	# 같은 사용자의 다른 프로그램처럼 접근성 버스에 바로 붙어 셸을 읽는다. 에이전트의 목록과 달리 사람만 답하는 창도 빼지 않는다.
	# 앱 뿌리에서 자식을 따라 내려가고, 그 뒤 버스에 오른 모든 경로도 읽는다. 답은
	# "그 글이 나온 수 기록의 줄이 보이는지" (바람: 0 1). 줄의 이름(클립보드 항목 1)은 대조군이다: 패널까지 닿았는데 글만 없다는 뜻이다
	address=$(session busctl --user call org.a11y.Bus /org/a11y/bus org.a11y.Bus GetAddress | sed -E 's/^s "(.*)"$/\1/')
	# 찾을 글은 파일로 넘긴다. 인자로 넘기면 sudo가 저널에 적어 no-trace에 걸린다
	printf '%s' "$2" >/tmp/e2e-needle
	chmod 644 /tmp/e2e-needle
	sudo -u "$user" XDG_RUNTIME_DIR="$runtime" python3 -I - "$address" <<'PY'
import json, subprocess, sys

address = sys.argv[1]
needle = open("/tmp/e2e-needle", encoding="utf-8").read()
BUSCTL = ["busctl", f"--address={address}", "--no-pager"]
ACCESSIBLE = "org.a11y.atspi.Accessible"


def busctl(*args):
    done = subprocess.run([*BUSCTL, *args], capture_output=True, text=True)
    return done.stdout if done.returncode == 0 else ""


def data(*args):
    # JSON으로 받는다: 보통 출력은 한글을 8진수로 적는다
    out = busctl("--json=short", *args)
    return json.loads(out)["data"] if out else None


shell = next(
    line.split()[0]
    for line in busctl("list").splitlines()
    if len(line.split()) > 2 and line.split()[2] == "bunt-shell"
)
found = rows = 0
seen = set()
stack = ["/org/a11y/atspi/accessible/root"]


def read(path):
    global found, rows
    texts = [
        data("get-property", shell, path, ACCESSIBLE, "Name"),
        data("get-property", shell, path, ACCESSIBLE, "Description"),
        data("call", shell, path, "org.a11y.atspi.Text", "GetText", "ii", "0", "-1"),
    ]
    text = json.dumps(texts, ensure_ascii=False)
    found += needle in text
    rows += "클립보드 항목" in text or "Clipboard item" in text


while stack:
    path = stack.pop()
    if path in seen:
        continue
    seen.add(path)
    read(path)
    children = data("call", shell, path, ACCESSIBLE, "GetChildren") or [[]]
    stack.extend(child[1] for child in children[0] if child[0] == shell)
for path in busctl("tree", "--list", shell).split():
    if path not in seen:
        seen.add(path)
        read(path)
print(found, int(rows > 0))
PY
	;;
no-trace)
	# 기록의 글이 파일과 저널에 남지 않았는지. "파일 수 저널 줄 수" (바람: 0 0)
	files=$(grep -rlaF "$2" "/home/$user" /tmp /var/tmp /etc/dconf 2>/dev/null | grep -c -v '^/tmp/e2e-')
	lines=$(journalctl -b -q --no-pager 2>/dev/null | grep -c -F "$2")
	echo "$files $lines"
	;;
locked)
	compositor GetLocked | awk '{print $2}'
	;;
paste-locked)
	# 잠긴 동안 새 연결은 클립보드 프로토콜을 보지 못한다. wl-paste는 창을 띄워 포커스를 기다리다 제한 시간에 걸린다
	if text=$(W timeout 4 wl-paste -n 2>/dev/null) && [[ -n "$text" ]]; then
		echo "read [$text]"
	else
		echo refused
	fi
	;;
*)
	echo "사용법: $0 globals | sandbox-globals | roundtrip | prepare | value | paste | copy | ..." >&2
	exit 2
	;;
esac
