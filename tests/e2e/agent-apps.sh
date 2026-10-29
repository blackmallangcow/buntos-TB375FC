#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# e2e 5번 게스트 쪽: 에이전트가 앱 A의 값을 읽어 앱 B에 넣는다 (ARCHITECTURE 9.2, 6.4). 모든 조작과 확인을
# `bunt agent`로만 한다. 계산기에서 6 × 7을 계산해 결과를 읽고, 텍스트 편집기에 채운 뒤 입력기를 한글
# 모드로 두고 글을 더 친다(글 입력으로 넣으므로 바뀌지 않아야 한다). ref가 없는 화면은 창 스크린샷에서 찾은
# 픽셀을 누른다(계산기의 '=' 단추, Firefox 캔버스). Firefox 주소 막대에서는 fill이 실패를 성공으로 알리지
# 않는지, 글 칸이 아닌 곳에 type하면 키로 바꿔 치지 않는지 본다. 시스템 설정 사이드바도 눌러 본다.
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
address=$(ref combobox <<<"$firefox")
if filled=$(A fill "$address" "$url" 2>&1); then
	echo "E2E5 알림: Firefox 주소 막대가 fill의 글을 지켰다"
else
	check "fill이 글을 지키지 못하면 type --ref를 권하는 오류다" \
		"$(grep -c "did not keep the text.*type \"$url\" --ref $address" <<<"$filled")" 1
	A press ctrl+a --ref "$address" >/dev/null
	A type "$url" --ref "$address" >/dev/null
fi
check "주소 막대에 주소가 들어간다" \
	"$(A snapshot --app firefox-esr -i | grep -m1 "ref=$address\]" | value)" "$url"
A press Return --settle 2000 >/dev/null
check "글 칸이 아닌 곳에는 type이 치지 않고 알린다" "$(A type abc 2>&1 | grep -c 'nothing was typed')" 1
shot=$(A screenshot --app firefox-esr)
clicked=$(A click --at "$(png red "$(tail -1 <<<"$shot")")" --app firefox-esr)
check "스크린샷에서 찾은 캔버스의 빨간 칸을 누른다" "$(grep -c '^+ *- text: "hit"$' <<<"$clicked")" 1
pkill -f /usr/lib/firefox-esr/firefox-esr
sleep 1

# 시스템 설정: 사이드바 항목을 누르면 답의 diff에 그 페이지의 제목이 나온다. 떠 있던 창이 이미 디스플레이
# 페이지일 수 있어 먼저 Wi-Fi로 옮긴다
settings=$(open_app org.buntos.Settings)
A click "$(ref 'listitem "Wi-Fi"' <<<"$settings")" >/dev/null
page=$(ref 'listitem "\(디스플레이\|Displays\)"' <<<"$settings")
shown=$(A click "$page" | grep -c -E '^\+ *- text: "(디스플레이|Displays)"$')
check "설정 사이드바를 누르면 디스플레이 페이지가 나온다" "$((shown > 0))" 1

echo "E2E5_RESULT $passed $failed"
