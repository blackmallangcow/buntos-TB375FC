#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# cargo를 이미지와 같은 Debian(image/mkosi.conf의 Release=와 Snapshot=)의 컨테이너에서 실행한다.
# 자체 컴포넌트는 이미지의 라이브러리(GTK4, gtk4-layer-shell, libinput, PAM 등)에 링크하므로 빌드 환경의 개발
# 꾸러미가 이미지와 같은 판이어야 한다. 이 스크립트는 같은 스냅숏에서 개발 꾸러미를 깐 컨테이너 이미지를 만들고
# (처음 한 번, tools/image/debian-snapshot.sh), rust-toolchain.toml의 툴체인으로 cargo를 돌린다. 빌드 VM(Debian 13,
# tools/devvm/setup.sh)이 아닌 곳, 곧 CI의 Ubuntu 러너(.github/workflows/x86-64.yml)가 tools/image/build.sh의
# BUNTOS_CARGO로 쓴다.
# 사용법: tools/image/cargo-debian.sh <cargo 인자...>   (예: build --release --locked --workspace)
# 저장소 루트에서 돌고, 산출물은 CARGO_TARGET_DIR(없으면 저장소의 target)에 이 컴퓨터의 사용자 소유로 생긴다.
# 컨테이너는 docker로 띄우고, 호스트와 같은 아키텍처의 바이너리를 만든다.
# 환경 변수: BUNTOS_CARGO_CACHE  컨테이너가 쓰는 캐시(내려받은 apt 꾸러미, cargo 레지스트리, rustup 툴체인).
#                                기본 $BUNTOS_BUILD_DIR/cargo-debian (BUNTOS_BUILD_DIR의 기본은 ~/buntos-build)
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
# shellcheck source=tools/image/debian-snapshot.sh
. "$repo/tools/image/debian-snapshot.sh"
release=$(sed -n 's/^Release=//p' "$repo/image/mkosi.conf")
channel=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$repo/rust-toolchain.toml")
if [[ -z "$release" || -z "$channel" ]]; then
	echo "image/mkosi.conf의 Release=나 rust-toolchain.toml의 channel을 읽지 못했다" >&2
	exit 1
fi

# 컨테이너의 바탕 이미지. 판(코드네임)은 image/mkosi.conf의 Release=와 같아야 한다(debian-snapshot.sh가 확인한다).
# 꾸러미는 apt가 Snapshot=의 판으로 맞추므로 바탕이 스냅숏보다 오래돼도 되고, 날짜가 가까울수록 맞출 꾸러미가
# 적다. Docker의 Debian 이미지는 snapshot.debian.org의 그날 0시(UTC)로 만든다. 다이제스트로 고정한다
base=docker.io/library/debian:forky-20261027@sha256:49c219ade93d7c11635c0d57e735ed82339a503a4376cd6957046c2bd4e5905b

# 자체 컴포넌트를 빌드하는 개발 꾸러미 (tools/devvm/setup.sh와 같다)
packages=(
	ca-certificates build-essential pkg-config rustup
	libudev-dev libinput-dev libgbm-dev libseat-dev libxkbcommon-dev libegl-dev libgles-dev libdrm-dev
	libwayland-dev libpixman-1-dev libgtk-4-dev libgtk4-layer-shell-dev
	libpam0g-dev
)

if ! command -v docker >/dev/null; then
	echo "docker가 없다. 이 스크립트는 컨테이너에서 빌드한다" >&2
	exit 1
fi
# 컨테이너에 같은 경로로 붙이므로 절대 경로로 바꾼다 (상대 경로는 docker가 이름 있는 볼륨으로 읽는다)
target=$(realpath -m "${CARGO_TARGET_DIR:-$repo/target}")
cache=$(realpath -m "${BUNTOS_CARGO_CACHE:-${BUNTOS_BUILD_DIR:-$HOME/buntos-build}/cargo-debian}")
mkdir -p "$target" "$cache/cargo" "$cache/rustup"

# 개발 꾸러미를 깐 컨테이너 이미지 (처음 한 번 만든다). 이미지의 판(Release=)의 스냅숏만 쓴다
image=$(debian_snapshot_image "$base" "$release" "$cache/apt/archives" "debian $release" "${packages[@]}")
# shellcheck disable=SC2016 # dpkg-query의 형식이다
echo "빌드 환경: Debian $release, $(docker run --rm "$image" dpkg-query -W -f='libgtk-4-dev ${Version}' libgtk-4-dev)"

# cargo는 이 컴퓨터의 사용자로 돌려 산출물과 캐시의 소유자를 맞춘다
# shellcheck disable=SC2016 # 컨테이너 안의 bash가 펼친다
docker run --rm --init --user "$(id -u):$(id -g)" \
	--volume "$repo:$repo" --volume "$target:$target" --volume "$cache:$cache" \
	--workdir "$repo" \
	--env HOME=/tmp --env CHANNEL="$channel" \
	--env CARGO_TARGET_DIR="$target" --env CARGO_HOME="$cache/cargo" --env RUSTUP_HOME="$cache/rustup" \
	"$image" bash -c 'rustup toolchain install "$CHANNEL" --profile minimal && exec rustup run "$CHANNEL" cargo "$@"' bash "$@"
