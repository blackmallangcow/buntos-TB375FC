#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# e2e 5번 게스트 쪽: 에이전트가 앱 A의 값을 읽어 앱 B에 넣는다 (ARCHITECTURE 9.2, 6.4). 모든 조작과 확인을
# `bunt agent`로만 한다. 계산기에서 6 × 7을 계산해 결과를 읽고, 텍스트 편집기에 채운 뒤 입력기를 한글
# 모드로 두고 글을 더 친다(글 입력으로 넣으므로 바뀌지 않아야 한다). ref가 없는 화면은 창 스크린샷에서 찾은
# 픽셀을 누른다(계산기의 '=' 단추, Firefox 캔버스). Firefox 주소 막대에서는 fill이 실패를 성공으로 알리지
# 않는지, 글 칸이 아닌 곳에 type하면 키로 바꿔 치지 않는지 본다. 긴 웹 페이지의 글과 화면 밖 단추, 휠 스크롤,
# 다른 페이지(시스템 모니터)를 빼는지, 앱을 끝내는 동작을 알리는지, 시스템 설정 사이드바도 본다. 끝으로
# 셸의 메뉴 막대 단추를 포인터로 누르고, 메뉴를 Escape로 닫은 뒤 다른 메뉴 단추가 눌리는지 본다. JS REPL로
# 계산기를 한 번에 다루고 그 변수가 다음 호출에 남는지도 본다.
# 직렬 콘솔의 root로 실행한다(boot-to-shell.sh가 넣어 돌린다). 마지막 줄은 "E2E5_RESULT 통과 실패"다.
# 인자로 together-* 단계 이름을 주면 그 단계만 돌리고 마지막 줄에 답한다: 사람과 AI가 함께 쓰는 시나리오다(boot-to-shell.sh가
# 단계 사이에 QMP로 사람의 누르기와 키를 넣는다)
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
# 창 스크린샷(RGB 8비트 PNG)을 읽는다. size: "너비 높이". red, accent: 빨간 영역, 아래쪽 절반의 강조 색(채도가
# 높은 색) 영역의 가운데 "x,y". 한 줄에 20픽셀 넘게 그 색인 줄만 세어 작은 아이콘과 글자는 빼다
png() {
	python3 -I - "$@" <<'PY'
import struct, sys, zlib

mode, path = sys.argv[1], sys.argv[2]
data = open(path, "rb").read()
if data[:8] != b"\x89PNG\r\n\x1a\n":
    sys.exit("not a PNG")
pos, packed = 8, b""
while pos < len(data):
    length, kind = struct.unpack(">I4s", data[pos:pos + 8])
    body = data[pos + 8:pos + 8 + length]
    if kind == b"IHDR":
        width, height, depth, color = struct.unpack(">IIBB", body[:10])
    elif kind == b"IDAT":
        packed += body
    pos += 12 + length
if mode == "size":
    print(width, height)
    sys.exit()
if (depth, color) != (8, 2):
    sys.exit("not 8-bit RGB")
stride = width * 3
raw = zlib.decompress(packed)
rows, prev = [], bytearray(stride)
for y in range(height):
    start = y * (stride + 1)
    kind, line = raw[start], bytearray(raw[start + 1:start + 1 + stride])
    for i in range(stride):
        left = line[i - 3] if i >= 3 else 0
        up = prev[i]
        corner = prev[i - 3] if i >= 3 else 0
        if kind == 1:
            line[i] = (line[i] + left) & 255
        elif kind == 2:
            line[i] = (line[i] + up) & 255
        elif kind == 3:
            line[i] = (line[i] + (left + up) // 2) & 255
        elif kind == 4:
            guess = left + up - corner
            near = min((abs(guess - left), 0, left), (abs(guess - up), 1, up),
                       (abs(guess - corner), 2, corner))[2]
            line[i] = (line[i] + near) & 255
    rows.append(line)
    prev = line
if mode == "red":
    wanted, top = (lambda r, g, b: r > 180 and g < 60 and b < 60), 0
else:
    wanted, top = (lambda r, g, b: max(r, g, b) - min(r, g, b) > 80), height // 2
xs, ys = [], []
for y in range(top, height):
    row = rows[y]
    hits = [x for x in range(width) if wanted(row[3 * x], row[3 * x + 1], row[3 * x + 2])]
    if len(hits) >= 20:
        xs += [hits[0], hits[-1]]
        ys.append(y)
if not ys:
    sys.exit("no such color")
print(f"{(min(xs) + max(xs)) // 2},{(min(ys) + max(ys)) // 2}")
PY
}

# Firefox 창 제목이 그 정규식에 맞을 때까지 2초마다 본다. Mac이 바쁘면 빌드 VM 안의 시험 VM에서 Firefox가 새로
# 연 페이지를 그리기까지 1분 넘게 걸리기도 한다. 90초가 지나면 그대로 두어 뒤의 확인이 실패로 알린다
wait_firefox() {
	for _ in $(seq 45); do
		A apps 2>/dev/null | grep -q -E "window \"$1" && return
		sleep 2
	done
}

# ---- 사람과 AI가 함께 쓴다 (설계 agent-focus-design 3장, 계약 A의 대상 확인 입력) ----
# AI는 텍스트 편집기 문서에 조각을 잇달아 치고(접근성 type --ref), 그사이 사람은 QMP로 시스템 설정의 검색 칸을 누르고
# 친다. 단계마다 상태는 /tmp/e2e-together.*에 둔다. 편집기 글은 트리의 값(200자에서 자른다)으로 읽으므로 조각은 그보다
# 짧게 둔다. 편집기를 키로 읽으면(⌘A ⌘C) 편집기가 포커스를 가져가므로 쓰지 않는다
together_chunks=36
compositor() {
	sudo -u "$user" DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus busctl --user call \
		org.buntos.Compositor /org/buntos/Compositor org.buntos.Compositor1 "$@"
}
# 키보드 포커스를 가진 앱 ID나 레이어 이름
focus_app() { compositor GetFocus | sed -E 's/^ussb [0-9]+ "([^"]*)".*/\1/'; }
together() {
	local state=/tmp/e2e-together.env search doc point editor settings
	# shellcheck source=/dev/null
	[[ -f $state ]] && . "$state"
	case $1 in
	together-watch)
		# 접근성 버스에서 Value 인터페이스의 GetAll과 Get을 지켜본다. GetAll은 at-spi2-core 결함으로 앱(Firefox)을 죽였다
		# (이슈 P0). dbus-monitor가 없으면 건너뛴다
		command -v dbus-monitor >/dev/null || {
			echo skip
			return
		}
		local address
		address=$(sudo -u "$user" DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus busctl --user call \
			org.a11y.Bus /org/a11y/bus org.a11y.Bus GetAddress | sed -E 's/^s "(.*)"$/\1/')
		rm -f /tmp/e2e-together-monitor.log
		setsid nohup sudo -u "$user" dbus-monitor --address "$address" \
			"type='method_call',member='GetAll',arg0='org.a11y.atspi.Value'" \
			"type='method_call',member='Get',arg0='org.a11y.atspi.Value'" \
			</dev/null >/tmp/e2e-together-monitor.log 2>&1 &
		sleep 1
		echo watching
		;;
	together-prepare)
		# 설정(사람의 창)의 검색 칸을 비우고, 사람이 누를 화면 자리를 AI가 그 칸을 포인터로 누른 답(화면 좌표)에서 읽는다.
		# 끝으로 AI가 편집기를 열어 앞으로 가져온다: 사람이 누르기 전에는 AI의 창이 키보드 포커스를 가진다
		settings=$(open_app org.buntos.Settings)
		search=$(ref textbox <<<"$settings")
		A press "ctrl+a BackSpace" --ref "$search" >/dev/null
		point=$(A click "$search" --pointer | grep -o 'clicked with the pointer at ([0-9]*, [0-9]*)' |
			grep -o '[0-9]*, [0-9]*' | tr -d ' ')
		editor=$(open_app org.gnome.TextEditor)
		doc=$(ref textbox <<<"$editor")
		A fill "$doc" "-" >/dev/null
		printf 'search=%s\ndoc=%s\n' "$search" "$doc" >"$state"
		echo "$point"
		;;
	together-start)
		# AI가 편집기 문서에 조각(a100 a101 …)을 잇달아 친다. 먼저 ref 없는 type이 아직 포커스를 가진 AI의 창에 가는지 본다
		cat >/tmp/e2e-together.js <<JS
const ed = desktop.app('org.gnome.TextEditor');
const doc = ed.locator('$doc');
const log = [];
try { await desktop.keyboard.type('k', { settle: 0 }); log.push('before ok'); }
catch (e) { log.push('before ' + e.name); }
let ok = 0;
for (let i = 100; i < 100 + $together_chunks; i++) {
  try { await doc.type('a' + i + ' ', { settle: 0 }); ok++; }
  catch (e) { log.push(i + ' ' + e.name + ': ' + String(e.message).slice(0, 100)); }
}
log.push('done ' + ok);
log.join('\n')
JS
		chmod 644 /tmp/e2e-together.js
		rm -f /tmp/e2e-together.out
		setsid nohup bash -c "sudo -u $user XDG_RUNTIME_DIR=/run/user/1000 bunt agent repl --timeout 120000 - \
			</tmp/e2e-together.js >/tmp/e2e-together.out 2>&1" </dev/null >/dev/null 2>&1 &
		echo started
		;;
	together-result)
		# AI가 다 칠 때까지 기다려 답한다: 편집기의 AI 조각 수, 차례대로인지(1/0), 편집기의 사람 글 수, 설정 칸의 AI 조각 수,
		# 설정 칸의 글, 지금 포커스, 사람이 누르기 전의 ref 없는 type
		for _ in $(seq 120); do
			grep -q '^done [0-9]*$' /tmp/e2e-together.out 2>/dev/null && break
			sleep 1
		done
		local text human
		text=$(A snapshot --app org.gnome.TextEditor -i | grep -m1 "ref=$doc\]" | value)
		human=$(A snapshot --app org.buntos.Settings -i | grep -m1 "ref=$search\]" | value)
		local chunks order
		chunks=$(grep -o 'a1[0-9][0-9]' <<<"$text" | wc -l)
		order=$(grep -o 'a1[0-9][0-9]' <<<"$text" | tr -d a | awk -v n="$together_chunks" \
			'{ if ($1 != 100 + NR - 1) bad = 1 } END { print (NR == n && !bad) ? 1 : 0 }')
		echo "$chunks $order $(grep -o human <<<"$text" | wc -l) $(grep -c 'a1[0-9][0-9]' <<<"$human")" \
			"${human:-none} $(focus_app) $(grep -m1 -o '^before [A-Za-z]*' /tmp/e2e-together.out | cut -d' ' -f2)"
		;;
	together-log)
		# 실패를 알아보려고: AI REPL의 답과 편집기 글 (한 줄로)
		echo "$(tr '\n' '|' </tmp/e2e-together.out | cut -c1-400) / 편집기: $(
			A snapshot --app org.gnome.TextEditor -i | grep -m1 "ref=$doc\]" | value
		)"
		;;
	together-refless)
		# 사람이 설정을 누른 뒤: ref 없는 type은 사람의 칸에 치지 않고 FocusMovedError다
		A repl "try { await desktop.keyboard.type('x'); 'typed' } catch (e) { e.name }" 2>&1 | tail -1
		;;
	together-fill)
		# 사람이 쓰는 중에도 fill(접근성)은 키보드 포커스 없이 AI의 칸만 바꾼다
		A fill "$doc" "AI fill" >/dev/null
		echo "$(A snapshot --app org.gnome.TextEditor -i | grep -m1 "ref=$doc\]" | value)|$(focus_app)|$(
			A snapshot --app org.buntos.Settings -i | grep -m1 "ref=$search\]" | value
		)"
		;;
	together-getall)
		# 지켜본 동안 Value의 GetAll 수와 Get 수 (Get이 0이면 Value를 가진 요소를 읽지 않아 GetAll 0이 뜻이 없다)
		pkill -f "dbus-monitor --address" 2>/dev/null
		sleep 0.5
		echo "$(grep -c 'member=GetAll$' /tmp/e2e-together-monitor.log)" \
			"$(grep -c 'member=Get$' /tmp/e2e-together-monitor.log)"
		;;
	together-cleanup)
		# 설정 검색 칸을 접근성으로 비운다(포커스는 설정에 남아 e2e 6번의 앱 차례를 지킨다)
		A press "ctrl+a BackSpace" --ref "$search" >/dev/null
		rm -f /tmp/e2e-together.* /tmp/e2e-together-monitor.log
		focus_app
		;;
	*)
		echo "unknown step $1"
		;;
	esac
}
if (($# > 0)); then
	together "$@"
	exit
fi

# 다시 돌려도 같도록 이 시나리오가 쓰는 앱을 닫고 시작한다
pkill -f /usr/bin/gnome-calculator
pkill -f /usr/bin/gnome-text-editor
pkill -f /usr/bin/resources
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

# 창 스크린샷과 그 픽셀의 포인터 누르기: ref가 없는 화면(캔버스, 직접 그리는 UI)을 다루는 길. 계산기에 42+8을
# 넣고, 스크린샷에서 강조 색 단추('=')를 찾아 그 자리를 누른다
A click "$(ref 'button "+"' <<<"$calculator")" >/dev/null
A click "$(ref 'button "8"' <<<"$calculator")" >/dev/null
shot=$(A screenshot --app org.gnome.Calculator)
image=$(tail -1 <<<"$shot")
check "스크린샷은 창 크기의 PNG다" "$(png size "$image")" \
	"$(grep -o '[0-9]*x[0-9]* pixels' <<<"$shot" | sed 's/ pixels//; s/x/ /')"
A click --at "$(png accent "$image")" --app org.gnome.Calculator >/dev/null
check "스크린샷에서 찾은 '=' 자리를 누르면 계산한다" \
	"$(A snapshot --app org.gnome.Calculator -i | grep -m1 'textbox .*\[focused\]' | value)" 50
check "창 밖의 점은 누르지 않는다" \
	"$(A click --at 99999,1 --app org.gnome.Calculator 2>&1 | grep -c 'outside the window')" 1

# ref에 --app을 함께 쓰면 그 앱의 ref인지 본다. 앱을 끝내는 동작(닫기)은 실패가 아니라 앱이 끝났다고 알린다
one=$(ref 'button "1"' <<<"$calculator")
check "ref와 같은 앱의 --app은 받는다" "$(A click "$one" --app org.gnome.Calculator >/dev/null && echo ok)" ok
check "ref와 다른 앱의 --app은 그렇다고 알린다" \
	"$(A click "$one" --app org.gnome.TextEditor 2>&1 | grep -c 'drop --app')" 1
# JS REPL(3단계): 떠 있는 계산기를 C로 비우고 6 × 7을 REPL 호출 한 번으로 계산해 읽고, 그 변수(calc)가 다음 호출에
# 남는다. e2e 6번이 쓰는 앱 차례가 바뀌지 않도록 계산기를 닫기 전에 한다
repl=$(A repl - 2>&1 <<'JS'
const calc = await desktop.launch('org.gnome.Calculator');
for (const key of ['C', '6', '×', '7', '=']) {
  await calc.getByRole('button', { name: key, exact: true }).click();
}
await calc.getByRole('textbox').first().inputValue()
JS
)
check "REPL 한 번으로 계산기의 6 × 7을 읽는다" "$(tail -1 <<<"$repl")" 42
check "REPL의 변수가 다음 호출에 남는다" \
	"$(A repl 'await calc.getByRole("textbox").first().inputValue()' 2>&1 | tail -1)" 42

check "닫기 단추를 누르면 앱이 끝났다고 알린다" \
	"$(A click "$(ref 'button "\(닫기\|Close\)"' <<<"$calculator")" | grep -c 'then the app quit')" 1

# 시스템 모니터(Resources): 사이드바로 넘긴 다른 페이지의 내용은 스냅샷에 나오지 않는다. GTK4는 숨긴
# GtkStack 페이지의 내용을 목록에 보이는 상태로 남긴다. 앱 페이지의 앱 목록 표가 GPU 페이지로 넘긴 뒤에는
# 없어야 한다(번역과 상관없게 표의 수로 본다). Resources는 마지막에 본 페이지로 뜨므로 앱 페이지를 먼저 누른다
monitor=$(open_app org.gnome.Resources)
A click "$(ref 'listitem "\(Apps\|앱\)"' <<<"$monitor")" >/dev/null
check "시스템 모니터의 사이드바에 GPU가 있고 앱 페이지에 앱 목록 표가 있다" \
	"$(grep -c 'listitem "GPU"' <<<"$monitor") $(A snapshot --app org.gnome.Resources | grep -c 'treegrid')" "1 1"
A click "$(ref 'listitem "GPU"' <<<"$monitor")" >/dev/null
check "다른 페이지(앱)의 내용은 스냅샷에 나오지 않는다" \
	"$(A snapshot --app org.gnome.Resources | grep -c 'treegrid')" 0
pkill -f /usr/bin/resources
sleep 1

# Firefox: 후기에서 주소 막대의 fill은 성공했다고 알렸지만 비어 있었다. 이제는 글이 남았거나, 남지 않았으면
# type --ref를 권하는 오류여야 한다. 그 주소로 연 캔버스를 스크린샷의 픽셀로 누르고(lichess 체스판처럼 트리에
# 없는 UI), 글 칸이 아닌 곳에 type하면 키로 바꿔 치지 않고 알리는지 본다. e2e 6번이 쓰는 창 차례가 바뀌지
# 않도록 시스템 설정보다 먼저 하고 Firefox는 닫는다
page=/tmp/e2e-agent-canvas.html
cat >"$page" <<'HTML'
<!doctype html>
<meta charset="utf-8">
<title>e2e canvas</title>
<canvas id="board" width="300" height="200"></canvas>
<p id="out">none</p>
<script>
const board = document.getElementById("board");
const pen = board.getContext("2d");
pen.fillStyle = "#e00000";
pen.fillRect(150, 50, 100, 100);
board.addEventListener("click", (event) => {
  const box = board.getBoundingClientRect();
  const x = event.clientX - box.left, y = event.clientY - box.top;
  document.getElementById("out").textContent =
    x >= 150 && x < 250 && y >= 50 && y < 150 ? "hit" : "miss";
});
</script>
HTML
chmod 644 "$page"
url="file://$page"
firefox=$(open_app firefox-esr)
# 바쁜 VM에서는 Firefox가 첫 창을 띄우는 데 1분을 넘기기도 하고, 창이 뜬 뒤에도 주소 막대는 접근성 트리에 늦게
# 나온다. 뜨는 중인 Firefox를 다시 띄우지 않고 주소 막대가 나올 때까지 트리를 다시 본다
for _ in $(seq 45); do
	grep -q combobox <<<"$firefox" && break
	sleep 2
	firefox=$(A snapshot --app firefox-esr -i 2>/dev/null)
done
address=$(ref combobox <<<"$firefox")
# fill이 다시 읽기에서 글을 지키지 못하면(이슈 P3-1) 데몬이 그 칸을 비우고 type의 길로 다시 넣고 둘 다 알린다
if filled=$(A fill "$address" "$url" 2>&1); then
	if grep -q 'cleared and typed instead' <<<"$filled"; then
		echo "E2E5 알림: Firefox 주소 막대가 fill의 글을 버려 데몬이 비우고 다시 쳤다"
	else
		echo "E2E5 알림: Firefox 주소 막대가 fill의 글을 지켰다"
	fi
else
	check "fill이 실패하면 다시 친 것까지 알리는 오류다" \
		"$(grep -c "did not keep the text.*typing it instead" <<<"$filled")" 1
	A press ctrl+a --ref "$address" >/dev/null
	A type "$url" --ref "$address" >/dev/null
fi
check "주소 막대에 주소가 들어간다" \
	"$(A snapshot --app firefox-esr -i | grep -m1 "ref=$address\]" | value)" "$url"
A press Return --settle 2000 >/dev/null
wait_firefox "e2e canvas"
check "글 칸이 아닌 곳에는 type이 치지 않고 알린다" "$(A type abc 2>&1 | grep -c 'nothing was typed')" 1
# 제목이 바뀐 뒤에도 캔버스는 조금 늦게 그려질 수 있어, 빨간 칸이 보일 때까지 다시 찍는다
point=
for _ in $(seq 15); do
	shot=$(A screenshot --app firefox-esr)
	point=$(png red "$(tail -1 <<<"$shot")" 2>/dev/null) && break
	sleep 2
done
clicked=$(A click --at "$point" --app firefox-esr)
# 바쁜 VM에서는 Firefox가 접근성 트리를 늦게 고쳐 누르기의 diff에 "hit"이 빠질 수 있다. 그때는 트리를 다시 본다
hit=$(grep -c '^+ *- text: "hit"$' <<<"$clicked")
for _ in $(seq 10); do
	((hit > 0)) && break
	sleep 2
	hit=$(A snapshot --app firefox-esr | grep -c -m1 'text: "hit"')
done
check "스크린샷에서 찾은 캔버스의 빨간 칸을 누른다" "$hit" 1

# 긴 웹 페이지: 문단의 글은 링크 사이에 차례대로 나오고(Firefox가 자식 자리에 둔 U+FFFC 없이), 화면 밖 단추는
# [offscreen]으로 나온다. 휠로 내리고, 화면 밖 단추를 누르면 먼저 보이게 스크롤해 누른다
long=/tmp/e2e-agent-long.html
cat >"$long" <<'HTML'
<!doctype html>
<meta charset="utf-8">
<title>top</title>
<p>첫 문단: 오늘 서울은 <a href="#a">흐림</a> 이고 밤부터 비가 옵니다.</p>
<div style="height: 4000px"></div>
<button onclick="document.title = 'bottom-clicked'">맨 아래 단추</button>
<script>
addEventListener("scroll", () => {
  if (!document.title.startsWith("bottom")) document.title = "y=" + Math.round(scrollY);
});
</script>
HTML
chmod 644 "$long"
A press ctrl+a --ref "$address" >/dev/null
A type "file://$long" --ref "$address" >/dev/null
A press Return --settle 2000 >/dev/null
wait_firefox top
# 바쁜 VM에서는 제목이 바뀐 뒤에도 Firefox가 문단의 글을 늦게 채운다. 링크 뒤의 글이 나올 때까지 트리를 다시 본다
for _ in $(seq 5); do
	text=$(A snapshot --app firefox-esr)
	grep -q 'text: "이고 밤부터 비가 옵니다."' <<<"$text" && break
	sleep 2
done
check "문단의 글이 링크 사이에 차례대로 나온다" \
	"$(grep -A2 -- '- text: "첫 문단: 오늘 서울은"' <<<"$text" | grep -c -e 'link "흐림"' -e 'text: "이고 밤부터 비가 옵니다."')" 2
check "글에 자식 자리 표시(U+FFFC)가 없다" "$(grep -c $'\xef\xbf\xbc' <<<"$text")" 0
bottom=$(A snapshot --app firefox-esr -i | grep -m1 '맨 아래 단추.*\[offscreen\]' | grep -o 'ref=e[0-9]*' | cut -d= -f2)
check "화면 밖 단추는 [offscreen]으로 나온다" "$((${#bottom} > 0))" 1
A scroll down 5 --app firefox-esr >/dev/null
wait_firefox "y=[1-9]"
check "휠로 페이지를 내린다" "$(A snapshot --app firefox-esr | grep -c -m1 'window "y=[1-9]')" 1
check "화면 밖 단추를 누르면 먼저 보이게 스크롤한다" "$(A click "$bottom" | grep -c 'scrolled it into view')" 1
wait_firefox bottom-clicked
check "그 단추가 눌린다" "$(A snapshot --app firefox-esr | grep -c -m1 'window "bottom-clicked')" 1
pkill -f /usr/lib/firefox-esr/firefox-esr
sleep 1

# 시스템 설정: 사이드바 항목을 누르면 답의 diff에 그 페이지의 제목이 나온다. 떠 있던 창이 이미 디스플레이
# 페이지일 수 있어 먼저 Wi-Fi로 옮긴다
settings=$(open_app org.buntos.Settings)
A click "$(ref 'listitem "Wi-Fi"' <<<"$settings")" >/dev/null
page=$(ref 'listitem "\(디스플레이\|Displays\)"' <<<"$settings")
shown=$(A click "$page" | grep -c -E '^\+ *- text: "(디스플레이|Displays)"$')
check "설정 사이드바를 누르면 디스플레이 페이지가 나온다" "$((shown > 0))" 1

# 셸: 메뉴 막대 단추는 접근성 동작이 없어 포인터로 누른다. 키보드로 닫힌 메뉴의 포인터 잡기는 다음 움직임
# 하나를 먹어, 전에는 Escape 뒤에 누른 제어 센터 대신 방금 닫은 네트워크 메뉴가 다시 열렸다
expanded() { A snapshot --app bunt-shell -i | grep -c -E "button \"($1)\".*\[expanded\]"; }
shell=$(A snapshot --app bunt-shell -i)
A click "$(ref 'button "\(네트워크\|Network\)"' <<<"$shell")" --app bunt-shell >/dev/null
check "메뉴 막대의 네트워크 단추를 누르면 그 메뉴가 열린다" "$(expanded '네트워크|Network')" 1
A press Escape --app bunt-shell >/dev/null
A click "$(ref 'button "\(제어 센터\|Control Center\)"' <<<"$shell")" --app bunt-shell >/dev/null
check "메뉴를 Escape로 닫은 뒤 제어 센터를 누르면 제어 센터가 열린다" "$(expanded '제어 센터|Control Center')" 1
A press Escape --app bunt-shell >/dev/null

echo "E2E5_RESULT $passed $failed"
