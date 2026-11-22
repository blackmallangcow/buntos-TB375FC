#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 터미널 foot을 고쳐 빌드한다. 터미널에서도 Ctrl+C·Ctrl+V로 복사하고 붙이게 하려면(DESIGN-SPEC 11장) Ctrl+C가 고른 글이
# 있을 때만 복사하고 없으면 그대로 인터럽트(^C)로 가야 한다. foot에는 그런 바인딩이 없어(복사 바인딩은 고른 글이 없어도
# 키를 먹는다) 0001 패치가 clipboard-copy-or-cancel 바인딩을 더한다. 업스트림에 열린 요청이 있다
# (https://codeberg.org/dnkl/foot/issues/1333). 업스트림에 들어오면 이 폴더와 tools/image/build.sh의 foot 부분을 지운다.
# 이미지와 같은 Debian 스냅숏(image/mkosi.conf의 Snapshot=)의 컨테이너에서 Debian 소스를 그대로 빌드한다(Debian의 빌드
# 옵션과 시험이 그대로 돈다). 바이너리만 내고, 이미지에는 tools/image/build.sh가 /usr/libexec/buntos/foot으로 넣는다.
# 사용법: image/foot/build.sh   (빌드 VM이나 CI. docker나 nerdctl이 있어야 한다)
# 출력: $BUNTOS_BUILD_DIR/foot/<아키텍처>/foot. 마지막 줄에 그 경로를 낸다. 소스 판, 패치, 이 파일이 그대로면 다시 빌드하지
#       않는다
set -euo pipefail

# 이미지 스냅숏의 foot 소스 판. 스냅숏을 옮기면 같이 맞춘다(패치가 붙지 않으면 빌드가 멈춘다)
debian_version=1.28.0-1

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../.." && pwd)
case "$(uname -m)" in
aarch64) arch=arm64 ;;
x86_64) arch=amd64 ;;
*)
	echo "foot을 빌드하지 않는 아키텍처다: $(uname -m)" >&2
	exit 1
	;;
esac
build_root=${BUNTOS_BUILD_DIR:-$HOME/buntos-build}/foot
work=$build_root/work-$arch
out=$build_root/$arch
mkdir -p "$work" "$out"

release=$(sed -n 's/^Release=//p' "$repo/image/mkosi.conf")
snapshot=$(sed -n 's/^Snapshot=//p' "$repo/image/mkosi.conf")
base=$(sed -n 's/^base=//p' "$repo/tools/image/cargo-debian.sh")
stamp=$({
	printf '%s\n' "$debian_version" "$release" "$snapshot" "$base"
	cat "$here/build.sh" "$here"/*.patch
} | sha256sum | cut -c1-16)
if [[ -x "$out/foot" && "$(cat "$out/.stamp" 2>/dev/null)" == "$stamp" ]]; then
	echo "$out/foot"
	exit 0
fi

# 컨테이너는 docker나 rootless nerdctl(빌드 VM)로 띄운다. rootless에서는 컨테이너의 root가 이 사용자이고, docker(CI)에서는
# 빌드를 이 사용자로 돌려 산출물의 소유자를 맞춘다
user=(--user "$(id -u):$(id -g)")
if ! command -v docker >/dev/null; then
	docker() { nerdctl "$@"; }
	user=()
fi

# 빌드 의존성과 Debian 소스를 넣은 컨테이너 이미지 (처음 한 번 만든다). 소스는 apt가 서명된 스냅숏의 Sources로 확인한다
image="buntos-foot-build:$(printf '%s\n' "$base" "$release" "$snapshot" "$debian_version" | sha256sum | cut -c1-16)"
if ! docker image inspect "$image" >/dev/null 2>&1; then
	mkdir -p "$build_root/apt/archives"
	name=buntos-foot-build-$$
	# shellcheck disable=SC2016 # 컨테이너 안의 bash가 펼친다
	docker run --name "$name" --volume "$build_root/apt/archives:/var/cache/apt/archives" \
		--env RELEASE="$release" --env SNAPSHOT="$snapshot" --env DEBIAN_VERSION="$debian_version" \
		"$base" bash -euo pipefail -c '
rm -f /etc/apt/sources.list /etc/apt/sources.list.d/* /etc/apt/apt.conf.d/docker-clean
keyring=$(ls /usr/share/keyrings/debian-archive-keyring.pgp /usr/share/keyrings/debian-archive-keyring.gpg 2>/dev/null | head -n 1)
cat >/etc/apt/apt.conf.d/90buntos <<CONF
Binary::apt::APT::Keep-Downloaded-Packages "true";
APT::Sandbox::User "root";
Acquire::Retries "8";
CONF
cat >/etc/apt/sources.list.d/buntos-snapshot.sources <<SOURCES
Types: deb deb-src
URIs: http://snapshot.debian.org/archive/debian/$SNAPSHOT/
Suites: $RELEASE
Components: main
Signed-By: $keyring
Check-Valid-Until: no
SOURCES
cat >/etc/apt/preferences.d/buntos-snapshot <<PIN
Package: *
Pin: origin snapshot.debian.org
Pin-Priority: 1001
PIN
export DEBIAN_FRONTEND=noninteractive
apt-get -q update
apt-get -q -y --allow-downgrades dist-upgrade
apt-get -q -y --no-install-recommends install ca-certificates build-essential dpkg-dev fakeroot
apt-get -q -y --no-install-recommends build-dep "foot=$DEBIAN_VERSION"
mkdir /src
cd /src
apt-get -q -y --only-source source "foot=$DEBIAN_VERSION"
chmod -R a+rX /src
' >&2 || {
		docker rm "$name" >/dev/null 2>&1 || true
		exit 1
	}
	docker commit "$name" "$image" >/dev/null
	docker rm "$name" >/dev/null
fi

rm -rf "${work:?}"/*
mkdir -p "$work/patches"
cp "$here"/*.patch "$work/patches/"

# shellcheck disable=SC2016 # 컨테이너 안의 bash가 펼친다
docker run --rm "${user[@]}" --volume "$work:/work" --env HOME=/tmp --env ARCH="$arch" \
	"$image" bash -euo pipefail -c '
cp -R /src/foot-*/ /work/src
cd /work/src
for patch in /work/patches/*.patch; do
	patch -p1 --no-backup-if-mismatch <"$patch"
done
DEB_BUILD_OPTIONS="parallel=$(nproc)" dpkg-buildpackage -b -uc -us -rfakeroot
dpkg-deb -x /work/foot_*_"$ARCH".deb /work/x
if ldd /work/x/usr/bin/foot | grep "not found"; then
	exit 1
fi
' >&2
install -m 0755 "$work/x/usr/bin/foot" "$out/foot"
echo "$stamp" >"$out/.stamp"
rm -rf "$work"
echo "$out/foot"
