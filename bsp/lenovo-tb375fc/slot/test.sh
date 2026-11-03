#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# slot-keep을 가짜 misc 파일로 시험한다. crc32는 파이썬 zlib으로 따로 계산해 맞댄다. GNU coreutils가 있어야 한다.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
tool=$here/../mkosi.extra/usr/libexec/buntos/slot-keep
w=$(mktemp -d)
trap 'rm -rf "$w"' EXIT
pass=0 fail=0

# make <_a 첫 바이트> <_b 첫 바이트> [crc를 깨면 1] [매직을 깨면 1]: 앞뒤를 채운 가짜 misc를 만든다
make() {
	python3 -I - "$w/misc" "$@" <<'PY'
import sys, zlib
path, a, b, *rest = sys.argv[1:]
badcrc = rest[:1] == ['1']
badmagic = rest[1:2] == ['1']
s = bytearray(b'_a\0\0' + (b'XXXX' if badmagic else b'BCAB') + bytes([1, 2, 0, 0, int(a, 16), 0, int(b, 16), 0]) + bytes(12))
s += (zlib.crc32(bytes(s)) ^ (1 if badcrc else 0)).to_bytes(4, 'little')
open(path, 'wb').write(bytes(range(256)) * 8 + bytes(s) + bytes(range(256)) * 8)
PY
}

# check <이름> <기대 _b 바이트, 또는 same> <기대 종료 코드>
check() {
	local name=$1 want=$2 wantrc=$3 rc=0
	cp "$w/misc" "$w/before"
	"$tool" "$w/misc" >/dev/null 2>&1 || rc=$?
	if python3 -I - "$w/before" "$w/misc" "$want" "$rc" "$wantrc" <<'PY'; then
import sys, zlib
before, after = open(sys.argv[1], 'rb').read(), open(sys.argv[2], 'rb').read()
want, rc, wantrc = sys.argv[3], int(sys.argv[4]), int(sys.argv[5])
ok = rc == wantrc and len(before) == len(after)
ok = ok and before[:0x800] == after[:0x800] and before[0x820:] == after[0x820:]
s = after[0x800:0x820]
if want == 'same':
    ok = ok and before == after
else:
    ok = ok and s[14] == int(want, 16) and before[0x800:0x80e] == after[0x800:0x80e] and before[0x80f:0x81c] == after[0x80f:0x81c]
    ok = ok and int.from_bytes(s[28:], 'little') == zlib.crc32(s[:28])
sys.exit(0 if ok else 1)
PY
		pass=$((pass + 1))
	else
		fail=$((fail + 1))
		echo "실패: $name"
	fi
}

make 96 67; check "재시도 6을 7로" 77 0
make 96 07; check "재시도 0을 7로" 77 0
make 96 e7; check "successful_boot를 지우지 않는다" f7 0
make 96 77; check "이미 7이면 쓰지 않는다" same 0
make 97 66; check "_a가 활성이면 쓰지 않는다" same 0
make 96 96; check "우선순위가 같으면 쓰지 않는다" same 0
make 96 67 1; check "crc32가 틀리면 쓰지 않는다" same 1
make 96 67 0 1; check "매직이 틀리면 쓰지 않는다" same 1
echo "통과 $pass, 실패 $fail"
((fail == 0))
