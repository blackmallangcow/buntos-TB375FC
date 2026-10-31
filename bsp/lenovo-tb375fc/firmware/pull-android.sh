#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 남겨 둔 안드로이드 슬롯이 켜져 있을 때 adb로 읽을 수 있는 펌웨어와 판 정보를 extract.sh의 원천 꼴로 받는다.
# root 없이 되는 것만 한다: /vendor/firmware 같은 읽을 수 있는 디렉터리, getprop, 그 목록. nvdata는 root가 있어야 해서
# 여기서 받지 않는다(파티션 백업이나 buntOS initramfs에서 얻는다). 기기에 아무것도 쓰지 않는다.
# 뼈대다. SELinux가 adb 셸의 읽기를 막는 디렉터리가 있을 수 있으니 기기에서 확인한다.
#
# 사용법: pull-android.sh <원천 디렉터리>   (호스트에서. adb가 기기 하나에 붙어 있어야 한다)
set -euo pipefail

die() {
	echo "pull-android: $*" >&2
	exit 1
}

dst=${1:?"원천 디렉터리가 필요하다 (사용법은 이 파일의 머리말)"}
command -v adb >/dev/null || die "adb가 없다"
[[ $(adb get-state 2>/dev/null) == device ]] || die "adb에 붙은 기기가 없다 (개발자 옵션의 USB 디버깅)"
mkdir -p "$dst/info"
adb shell getprop >"$dst/info/getprop.txt"
echo "기기: $(grep -m 1 'ro.build.fingerprint' "$dst/info/getprop.txt" || echo '판 모름')"
for d in /vendor/firmware /vendor/etc/firmware /odm/firmware; do
	if ! adb shell "test -d $d"; then
		echo "없음: $d"
		continue
	fi
	adb shell "ls -lR $d" >"$dst/info/$(echo "${d#/}" | tr / _).txt" || true
	mkdir -p "$dst$(dirname "$d")"
	adb pull "$d" "$dst$(dirname "$d")/" || echo "일부를 받지 못했다: $d (권한)"
done
echo "원천: $dst (다음: extract.sh $dst <출력>)"
