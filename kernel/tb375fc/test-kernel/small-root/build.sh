#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 빌드 VM에서 작은 Debian 루트를 만든다. 결과는 buntos-small.root-arm64.raw이고 기기의 buntos 파티션에 굽는다.
# root의 SSH 공개 키는 이미지 안에 넣는다. 개발용 길이라 키는 저장소 밖에 둔다.
# 사용법: build.sh <SSH 공개 키 파일> [출력 디렉터리]
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
key=${1:?SSH 공개 키 파일이 필요하다}
B=${BUNTOS_BUILD_DIR:-$HOME/buntos-build}/tb375fc
out=${2:-$B/rootfs-small}
mkdir -p "$out" "$B/mkosi-cache-small"
tree=$(mktemp -d)
trap 'rm -rf "$tree"' EXIT
install -D -m 600 "$key" "$tree/root/.ssh/authorized_keys"
chmod 700 "$tree/root/.ssh"
mkosi=$(command -v mkosi || echo "$HOME/.local/bin/mkosi")
"$mkosi" --directory="$here" --output-dir="$out" --cache-dir="$B/mkosi-cache-small" \
	--extra-tree="$B/modules/lib/modules:/usr/lib/modules" --extra-tree="$tree" -f build
ls -la "$out"/*.root-arm64.raw
