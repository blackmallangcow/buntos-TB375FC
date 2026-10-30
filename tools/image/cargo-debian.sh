#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# cargo를 이미지와 같은 Debian(image/mkosi.conf의 Release=와 Snapshot=)의 컨테이너에서 실행한다.
# 자체 컴포넌트는 이미지의 라이브러리(GTK4, gtk4-layer-shell, libinput, PAM 등)에 링크하므로 빌드 환경의 개발
# 꾸러미가 이미지와 같은 판이어야 한다. 이 스크립트는 같은 스냅숏에서 개발 꾸러미를 깐 컨테이너 이미지를 만들고
# (처음 한 번), rust-toolchain.toml의 툴체인으로 cargo를 돌린다. 빌드 VM(Debian 13, tools/devvm/setup.sh)이 아닌
# 곳, 곧 CI의 Ubuntu 러너(.github/workflows/x86-64.yml)가 tools/image/build.sh의 BUNTOS_CARGO로 쓴다.
# 사용법: tools/image/cargo-debian.sh <cargo 인자...>   (예: build --release --locked --workspace)
# 저장소 루트에서 돌고, 산출물은 CARGO_TARGET_DIR(없으면 저장소의 target)에 이 컴퓨터의 사용자 소유로 생긴다.
# 컨테이너는 docker로 띄우고, 호스트와 같은 아키텍처의 바이너리를 만든다.
# 환경 변수: BUNTOS_CARGO_CACHE  컨테이너가 쓰는 캐시(내려받은 apt 꾸러미, cargo 레지스트리, rustup 툴체인).
#                                기본 $BUNTOS_BUILD_DIR/cargo-debian (BUNTOS_BUILD_DIR의 기본은 ~/buntos-build)
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
release=$(sed -n 's/^Release=//p' "$repo/image/mkosi.conf")
snapshot=$(sed -n 's/^Snapshot=//p' "$repo/image/mkosi.conf")
channel=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$repo/rust-toolchain.toml")
if [[ -z "$release" || ! "$snapshot" =~ ^[0-9]{8}T[0-9]{6}Z$ || -z "$channel" ]]; then
	echo "image/mkosi.conf의 Release=·Snapshot=이나 rust-toolchain.toml의 channel을 읽지 못했다" >&2
	exit 1
fi

# 컨테이너의 바탕 이미지. 판(코드네임)은 image/mkosi.conf의 Release=와 같아야 한다(컨테이너 안에서 확인한다).
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
target=${CARGO_TARGET_DIR:-$repo/target}
cache=${BUNTOS_CARGO_CACHE:-${BUNTOS_BUILD_DIR:-$HOME/buntos-build}/cargo-debian}
mkdir -p "$target" "$cache/apt/archives" "$cache/cargo" "$cache/rustup"

# 개발 꾸러미를 깐 컨테이너 이미지. 바탕, 스냅숏, 꾸러미 목록이 같으면 다시 만들지 않는다
image="buntos-cargo-debian:$(printf '%s\n' "$base" "$snapshot" "${packages[@]}" | sha256sum | cut -c1-16)"
if ! docker image inspect "$image" >/dev/null 2>&1; then
	# shellcheck disable=SC2016 # 컨테이너 안의 bash가 펼친다
	prepare='
set -euo pipefail
. /etc/os-release
if [[ "${VERSION_CODENAME:-}" != "$RELEASE" ]]; then
	echo "컨테이너 바탕 이미지의 판(${VERSION_CODENAME:-?})이 이미지의 Release=($RELEASE)와 다르다 (tools/image/cargo-debian.sh의 base)" >&2
	exit 1
fi
rm -f /etc/apt/sources.list /etc/apt/sources.list.d/*
# 열쇠 꾸러미의 파일 이름은 .pgp로 옮겨 가는 중이다
keyring=$(ls /usr/share/keyrings/debian-archive-keyring.pgp /usr/share/keyrings/debian-archive-keyring.gpg 2>/dev/null | head -n 1)
# 내려받은 꾸러미를 캐시(/var/cache/apt/archives에 붙인 디렉터리)에 남긴다. Docker의 Debian 이미지는 설치 뒤 지운다
rm -f /etc/apt/apt.conf.d/docker-clean
cat >/etc/apt/apt.conf.d/90buntos <<EOF
Binary::apt::APT::Keep-Downloaded-Packages "true";
APT::Sandbox::User "root";
Acquire::Retries "5";
EOF
# 서명은 apt가 InRelease로 확인하므로 http로 받는다 (바탕 이미지에는 인증서 꾸러미가 없다)
cat >/etc/apt/sources.list.d/buntos-snapshot.sources <<EOF
Types: deb
URIs: http://snapshot.debian.org/archive/debian/$SNAPSHOT/
Suites: $RELEASE
Components: main
Signed-By: $keyring
Check-Valid-Until: no
EOF
# 바탕 이미지에 이미 든 꾸러미도 스냅숏의 판으로 맞춘다 (더 새것이면 내린다)
cat >/etc/apt/preferences.d/buntos-snapshot <<EOF
Package: *
Pin: origin snapshot.debian.org
Pin-Priority: 1001
EOF
export DEBIAN_FRONTEND=noninteractive
apt-get -q update
apt-get -q -y --allow-downgrades dist-upgrade
apt-get -q -y --no-install-recommends install $PACKAGES
chown -R "$HOST_UID:$HOST_GID" /var/cache/apt/archives
'
	name="buntos-cargo-debian-$$"
	docker run --name "$name" --init \
		--volume "$cache/apt/archives:/var/cache/apt/archives" \
		--env RELEASE="$release" --env SNAPSHOT="$snapshot" --env PACKAGES="${packages[*]}" \
		--env HOST_UID="$(id -u)" --env HOST_GID="$(id -g)" \
		"$base" bash -c "$prepare" || {
		docker rm "$name" >/dev/null 2>&1 || true
		exit 1
	}
	docker commit "$name" "$image" >/dev/null
	docker rm "$name" >/dev/null
fi
echo "빌드 환경: Debian $release (스냅숏 $snapshot), $(docker run --rm "$image" dpkg-query -W -f='libgtk-4-dev ${Version}' libgtk-4-dev)"

# cargo는 이 컴퓨터의 사용자로 돌려 산출물과 캐시의 소유자를 맞춘다
# shellcheck disable=SC2016 # 컨테이너 안의 bash가 펼친다
docker run --rm --init --user "$(id -u):$(id -g)" \
	--volume "$repo:$repo" --volume "$target:$target" --volume "$cache:$cache" \
	--workdir "$repo" \
	--env HOME=/tmp --env CHANNEL="$channel" \
	--env CARGO_TARGET_DIR="$target" --env CARGO_HOME="$cache/cargo" --env RUSTUP_HOME="$cache/rustup" \
	"$image" bash -c 'rustup toolchain install "$CHANNEL" --profile minimal && exec rustup run "$CHANNEL" cargo "$@"' bash "$@"
