#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 슬롯 _b에 순정 dtbo, 순정 vendor_boot, 시험 boot를 굽는다. 슬롯 _a(안드로이드)는 건드리지 않는다.
# 사람이 맥에서 실행한다. 슬롯은 바꾸지 않는다. 실패하면 바로 멈춘다. vendor_boot는 64MiB라 약 4분 걸린다.
# 사용법: flash-b-test.sh <16.0.20.091 묶음의 image 디렉터리> <시험 boot.img>
set -u
stock=${1:?"순정 이미지 디렉터리가 필요하다"}
boot=${2:?"시험 boot.img가 필요하다"}
FB=${FASTBOOT:-$HOME/Library/Android/sdk/platform-tools/fastboot}
get() { "$FB" getvar "$1" 2>&1 | sed -n "s/^$1: //p"; }
[ "$(get current-slot)" = a ] || { echo "멈춤: 지금 슬롯이 a가 아니다"; exit 1; }
[ "$(get unlocked)" = yes ] || { echo "멈춤: 언락 상태가 아니다"; exit 1; }
n=0
for pair in "dtbo $stock/dtbo.img" "boot $boot" "vendor_boot $stock/vendor_boot.img"; do
	p=${pair%% *}
	f=${pair#* }
	[ -s "$f" ] || { echo "멈춤: 파일이 없다: $f"; exit 1; }
	out=$("$FB" --slot=b flash "$p" "$f" 2>&1)
	rc=$?
	echo "$out" | grep -E "Sending|Writing|FAILED|error"
	if [ "$rc" -ne 0 ] || ! echo "$out" | grep -q "Writing '${p}_b'.*OKAY"; then
		echo "멈춤: ${p}_b 굽기 실패"
		exit 1
	fi
	n=$((n + 1))
done
echo "완료: _b에 ${n}개를 구웠다. 슬롯은 아직 a다."
