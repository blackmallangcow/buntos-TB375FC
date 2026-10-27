#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# buntOS 이미지를 빌드하거나 띄운다. 빌드 VM(Linux) 안에서 실행한다.
# 사용법: tools/image/build.sh <보드> [mkosi 인자...]
#   tools/image/build.sh qemu-aarch64        자체 컴포넌트와 이미지 빌드 (mkosi -f build)
#   tools/image/build.sh qemu-aarch64 --ephemeral=yes vm   빌드한 이미지를 임시 복사본으로 부팅
# 산출물은 저장소 밖 $BUNTOS_BUILD_DIR/<보드>에 둔다 (기본 ~/buntos-build).
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
board=${1:?"보드 이름이 필요하다 (bsp/ 아래 디렉터리 이름)"}
shift
bsp="$repo/bsp/$board/mkosi.conf"
if [[ ! -f "$bsp" ]]; then
	echo "bsp/$board/mkosi.conf가 없다" >&2
	exit 1
fi

build_root="${BUNTOS_BUILD_DIR:-$HOME/buntos-build}"
out="$build_root/$board"
mkdir -p "$out"
mkosi=$(command -v mkosi || echo "$HOME/.local/bin/mkosi")
extra=()

# 자체 컴포넌트(bunt-comp, bunt-shell, bunt-settings)를 빌드해 이미지에 넣을 트리로 모은다.
# 지금은 빌드 VM과 아키텍처가 같은 보드만 지원한다. 다른 아키텍처는 CI에서 빌드한다.
stage_components() {
	local arch host
	arch=$(sed -n 's/^Architecture=//p' "$bsp")
	host=$(uname -m)
	if [[ ! ("$arch" == arm64 && "$host" == aarch64) && ! ("$arch" == x86-64 && "$host" == x86_64) ]]; then
		echo "알림: $host 빌드 VM에서는 $arch용 자체 컴포넌트를 빌드하지 않는다. 그래픽 세션 없이 이미지를 만든다" >&2
		rm -rf "$out/stage"
		return
	fi
	local cargo target
	cargo=$(command -v cargo || echo "$HOME/.cargo/bin/cargo")
	target="$build_root/cargo-target"
	(cd "$repo" && CARGO_TARGET_DIR="$target" "$cargo" build --release --locked --workspace)
	rm -rf "$out/stage"
	install -D -m 0755 "$target/release/bunt-comp" "$out/stage/usr/bin/bunt-comp"
	install -D -m 0755 "$target/release/bunt-shell" "$out/stage/usr/bin/bunt-shell"
	install -D -m 0755 "$target/release/bunt-settings" "$out/stage/usr/bin/bunt-settings"
	install -D -m 0644 "$repo/settings/data/org.buntos.Settings.desktop" \
		"$out/stage/usr/share/applications/org.buntos.Settings.desktop"
	local icon
	for icon in "$repo"/shell/data/icons/*.svg "$repo/settings/data/org.buntos.Settings.svg"; do
		install -D -m 0644 "$icon" "$out/stage/usr/share/icons/hicolor/scalable/apps/$(basename "$icon")"
	done
}

if [[ $# -eq 0 ]]; then
	stage_components
	if [[ -d "$out/stage" ]]; then
		extra+=(--extra-tree="$out/stage")
	fi
	set -- -f build
fi

exec "$mkosi" -C "$repo/image" --include="$repo/bsp/$board" --output-directory="$out" "${extra[@]}" "$@"
