#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# buntOS 이미지를 빌드하거나 띄운다. 빌드 VM(Linux) 안에서 실행한다.
# 사용법: tools/image/build.sh <보드> [mkosi 인자...]
#   tools/image/build.sh qemu-aarch64        자체 컴포넌트와 이미지 빌드 (mkosi -f build)
#   tools/image/build.sh qemu-aarch64 --ephemeral=yes vm   빌드한 이미지를 임시 복사본으로 부팅
# 산출물은 저장소 밖 $BUNTOS_BUILD_DIR/<보드>에 둔다 (기본 ~/buntos-build).
# BUNTOS_IMAGE_DIR을 주면 이미지만 그 디렉터리에 만든다 (다른 사람이 띄운 VM이 기본 이미지를
# 쓰는 동안 따로 빌드해 시험할 때). tools/image/vmctl.sh도 같은 변수를 읽는다.
# 이 파일은 실행 중에도 bash가 이어 읽으므로, 빌드가 도는 동안 고칠 때는 새 파일로 바꿔 넣는다.
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
out="${BUNTOS_IMAGE_DIR:-$build_root/$board}"
mkdir -p "$out"
mkosi=$(command -v mkosi || echo "$HOME/.local/bin/mkosi")
extra=()
# mkosi vm의 VM 이름. 이미지(BUNTOS_IMAGE_DIR)마다 달라 다른 이미지로 띄운 VM과 함께 돌 수 있다.
# mkosi의 기본 이름(image)은 빌드 VM 안에서 하나만 돈다("Another virtual machine named image")
if [[ $# -gt 0 ]]; then
	extra+=(--machine="buntos-$(basename "$out")")
fi

# 자체 컴포넌트(bunt-comp, bunt-shell, bunt-settings, buntd-agent와 bunt, bunt-greeter와 bunt-lock)를 빌드해
# 이미지에 넣을 트리로 모은다.
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
	install -D -m 0755 "$target/release/buntd-agent" "$out/stage/usr/bin/buntd-agent"
	install -D -m 0755 "$target/release/bunt" "$out/stage/usr/bin/bunt"
	local unit
	for unit in buntd-agent.service buntd-agent.socket; do
		install -D -m 0644 "$repo/agent/data/$unit" "$out/stage/usr/lib/systemd/user/$unit"
	done
	install -D -m 0755 "$target/release/bunt-greeter" "$out/stage/usr/bin/bunt-greeter"
	# 잠금 화면 (컴포지터가 화면을 잠그면 띄운다. PAM 서비스는 image/mkosi.extra/etc/pam.d/bunt-lock)
	install -D -m 0755 "$target/release/bunt-lock" "$out/stage/usr/bin/bunt-lock"
	install -D -m 0755 "$target/release/buntd-setup" "$out/stage/usr/libexec/buntos/buntd-setup"
	install -D -m 0755 "$target/release/bunt-access-helper" \
		"$out/stage/usr/libexec/buntos/bunt-access-helper"
	# 크레이트의 data/root는 이미지 루트에 그대로 놓는다(안의 경로가 이미지 안의 경로다):
	# 로그인 화면의 greetd·D-Bus 설정, 접근 정책의 polkit 동작 등
	local crate
	for crate in greeter access; do
		if [[ -d "$repo/$crate/data/root" ]]; then
			cp -a "$repo/$crate/data/root/." "$out/stage/"
		fi
	done
	install -D -m 0644 "$repo/settings/data/org.buntos.Settings.desktop" \
		"$out/stage/usr/share/applications/org.buntos.Settings.desktop"
	install -D -m 0644 "$repo/shell/data/org.buntos.shell.gschema.xml" \
		"$out/stage/usr/share/glib-2.0/schemas/org.buntos.shell.gschema.xml"
	# 앱 창에 입히는 모양(창 단추 신호등). 세션 스크립트가 사용자 CSS에서 읽게 한다
	install -D -m 0644 "$repo/theme/gtk4-apps.css" "$out/stage/usr/share/buntos/gtk-4.0/buntos.css"
	install -D -m 0644 "$repo/theme/gtk3-apps.css" "$out/stage/usr/share/buntos/gtk-3.0/buntos.css"
	# 기본 배경화면 (tools/wallpapers/generate.py). 만드는 데 몇 분 걸리므로 생성기가 바뀌었을 때만
	# 다시 만든다
	local wallpapers="$build_root/wallpapers" stamp
	stamp=$(sha256sum "$repo/tools/wallpapers/generate.py" | cut -d' ' -f1)
	if [[ "$(cat "$wallpapers/.stamp" 2>/dev/null)" != "$stamp" ]]; then
		rm -rf "$wallpapers"
		python3 -I "$repo/tools/wallpapers/generate.py" "$wallpapers"
		echo "$stamp" >"$wallpapers/.stamp"
	fi
	install -d "$out/stage/usr/share/backgrounds/buntos"
	install -m 0644 "$wallpapers"/*.png "$out/stage/usr/share/backgrounds/buntos/"
	local icon
	for icon in "$repo"/shell/data/icons/*.svg "$repo/settings/data/org.buntos.Settings.svg"; do
		install -D -m 0644 "$icon" "$out/stage/usr/share/icons/hicolor/scalable/apps/$(basename "$icon")"
	done
}

if [[ $# -eq 0 ]]; then
	# 같은 이미지는 한 번에 하나만 빌드한다. tools/image/vmctl.sh는 이 잠금을 공유로 잡아
	# 빌드 중에는 그 이미지로 VM을 띄우지 않는다
	exec 9>"$out.lock"
	if ! flock -n 9; then
		echo "다른 빌드가 끝나기를 기다린다 ($out.lock)" >&2
		flock 9
	fi
	# mkosi vm(tests/e2e, screenshot.sh)은 도는 동안 이미지를 잠가 빌드를 막는다. 2분까지 기다리고,
	# 그래도 돌면 알리고 끝낸다
	local_vm() { pgrep -f -- "--output-directory=$out vm" >/dev/null; }
	for _ in $(seq 1 12); do
		local_vm || break
		echo "이 이미지로 띄운 VM(mkosi vm)이 끝나기를 기다린다" >&2
		sleep 10
	done
	if local_vm; then
		echo "이 이미지로 띄운 VM(mkosi vm)이 아직 돈다. VM을 멈추거나 BUNTOS_IMAGE_DIR로 다른 디렉터리에 빌드한다" >&2
		exit 1
	fi
	stage_components
	if [[ -d "$out/stage" ]]; then
		extra+=(--extra-tree="$out/stage")
	fi
	set -- -f build
fi

exec "$mkosi" -C "$repo/image" --include="$repo/bsp/$board" --output-directory="$out" "${extra[@]}" "$@"
