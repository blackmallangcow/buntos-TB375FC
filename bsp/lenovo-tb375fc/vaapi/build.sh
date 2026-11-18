#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 비디오 디코더(mt6897-vdec)를 VA-API로 내는 libva 드라이버를 꾸러미로 만든다(mt6897_va.c, MIT).
# libva는 드라이버를 DRM 드라이버의 이름으로 찾는다. 이 기기의 GPU 드라이버는 panthor라서, 같은 파일을
# panthor_drv_video.so라는 이름으로도 둔다. 그러면 환경 변수 없이 잡힌다. 디코더 드라이버가 올라와 있지 않으면
# 초기화에서 물러나므로, 그때 응용 프로그램은 소프트웨어로 푼다.
# Firefox는 시스템 libavcodec의 VA-API로 이 드라이버를 쓴다(arm64의 Firefox에는 내장 VA-API가 없다).
# 그 libavcodec은 mkosi.conf가 넣는다.
# 이미지와 같은 Debian 스냅숏(image/mkosi.conf의 Snapshot=)의 컨테이너에서 빌드한다.
# 사용법: bsp/lenovo-tb375fc/vaapi/build.sh   (빌드 VM에서)
# 출력: ~/buntos-build/tb375fc/vaapi-deb/buntos-tb375fc-vaapi_<판>_arm64.deb (mkosi.conf의 PackageDirectories)
set -euo pipefail

package_version=0.1+buntos1

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../../.." && pwd)
build_root=${BUNTOS_BUILD_DIR:-$HOME/buntos-build}/tb375fc
work=$build_root/vaapi
out=$build_root/vaapi-deb
mkdir -p "$work" "$out"

# 컨테이너는 docker나 rootless nerdctl로 띄운다. rootless에서는 컨테이너의 root가 이 사용자다
if ! command -v docker >/dev/null; then
	docker() { nerdctl "$@"; }
fi
release=$(sed -n 's/^Release=//p' "$repo/image/mkosi.conf")
base=$(sed -n 's/^base=//p' "$repo/tools/image/cargo-debian.sh")
snapshot=$(sed -n 's/^Snapshot=//p' "$repo/image/mkosi.conf")

# 빌드 의존성을 깐 컨테이너 이미지 (처음 한 번 만든다). 판은 이미지의 스냅숏으로 맞춘다
image="buntos-vaapi-build:$(printf '%s\n' "$base" "$release" "$snapshot" "$package_version" | sha256sum | cut -c1-16)"
if ! docker image inspect "$image" >/dev/null 2>&1; then
	mkdir -p "$work/apt/archives"
	name=buntos-vaapi-build-$$
	# shellcheck disable=SC2016 # 컨테이너 안의 bash가 펼친다
	docker run --name "$name" --volume "$work/apt/archives:/var/cache/apt/archives" \
		--env RELEASE="$release" --env SNAPSHOT="$snapshot" \
		"$base" bash -euo pipefail -c '
rm -f /etc/apt/sources.list /etc/apt/sources.list.d/* /etc/apt/apt.conf.d/docker-clean
keyring=$(ls /usr/share/keyrings/debian-archive-keyring.pgp /usr/share/keyrings/debian-archive-keyring.gpg 2>/dev/null | head -n 1)
cat >/etc/apt/apt.conf.d/90buntos <<CONF
Binary::apt::APT::Keep-Downloaded-Packages "true";
APT::Sandbox::User "root";
Acquire::Retries "8";
Acquire::Queue-Mode "access";
CONF
for uri in http://deb.debian.org/debian "http://snapshot.debian.org/archive/debian/$SNAPSHOT/"; do
	cat >>/etc/apt/sources.list.d/buntos.sources <<SOURCES
Types: deb deb-src
URIs: $uri
Suites: $RELEASE
Components: main
Signed-By: $keyring
Check-Valid-Until: no

SOURCES
done
cat >/etc/apt/preferences.d/buntos-snapshot <<PIN
Package: *
Pin: origin snapshot.debian.org
Pin-Priority: 1001
PIN
export DEBIAN_FRONTEND=noninteractive
apt-get -q update
apt-get -q -y --allow-downgrades dist-upgrade
apt-get -q -y --no-install-recommends install ca-certificates build-essential dpkg-dev pkg-config libva-dev linux-libc-dev
' || { docker rm "$name" >/dev/null 2>&1 || true; exit 1; }
	docker commit "$name" "$image" >/dev/null
	docker rm "$name" >/dev/null
fi

cp "$here/mt6897_va.c" "$here/Makefile" "$repo/LICENSES/MIT.txt" "$work/"

# shellcheck disable=SC2016 # 컨테이너 안의 bash가 펼친다
docker run --rm --volume "$work:/work" --volume "$out:/out" --workdir /work \
	--env PACKAGE_VERSION="$package_version" \
	"$image" bash -euo pipefail -c '
rm -rf pkg mt6897_vdec_drv_video.so
make mt6897_vdec_drv_video.so
pkg=/work/pkg
dri=$pkg/usr/lib/aarch64-linux-gnu/dri
install -D -m 0644 mt6897_vdec_drv_video.so "$dri/mt6897_vdec_drv_video.so"
ln -s mt6897_vdec_drv_video.so "$dri/panthor_drv_video.so"
mkdir -p "$pkg/usr/share/doc/buntos-tb375fc-vaapi" "$pkg/DEBIAN"
{ printf "Files: *\nCopyright: 2026 buntOS contributors\nLicense: MIT\n\n"; cat MIT.txt; } >"$pkg/usr/share/doc/buntos-tb375fc-vaapi/copyright"
# 의존성은 바이너리에서 읽는다
mkdir -p debian
printf "Source: buntos-tb375fc-vaapi\n\nPackage: buntos-tb375fc-vaapi\nArchitecture: arm64\n" >debian/control
depends=$(dpkg-shlibdeps -O --ignore-missing-info "$dri/mt6897_vdec_drv_video.so" | sed -n "s/^shlibs:Depends=//p")
rm -rf debian
size=$(du -sk "$pkg" | cut -f1)
cat >"$pkg/DEBIAN/control" <<CONTROL
Package: buntos-tb375fc-vaapi
Version: $PACKAGE_VERSION
Architecture: arm64
Maintainer: buntOS contributors
Section: video
Priority: optional
Installed-Size: $size
Depends: libva2, $depends
Description: VA-API driver for the MT6897 video decoder
 A libva driver on top of the stateful V4L2 decoder mt6897-vdec (VP9
 profile 0). Also installed under the name of the GPU driver, panthor, so
 that libva finds it without configuration.
CONTROL
rm -f /out/buntos-tb375fc-vaapi_*.deb
dpkg-deb --root-owner-group --build "$pkg" "/out/buntos-tb375fc-vaapi_${PACKAGE_VERSION}_arm64.deb" >/dev/null
# 꾸러미가 이미지의 Debian에 깔리고 라이브러리를 다 찾는지 본다
apt-get -q -y --no-install-recommends install "/out/buntos-tb375fc-vaapi_${PACKAGE_VERSION}_arm64.deb" >/dev/null
if ldd /usr/lib/aarch64-linux-gnu/dri/panthor_drv_video.so | grep "not found"; then
	exit 1
fi
rm -rf /work/pkg
dpkg-deb --field "/out/buntos-tb375fc-vaapi_${PACKAGE_VERSION}_arm64.deb" Package Version Depends
dpkg-deb --contents "/out/buntos-tb375fc-vaapi_${PACKAGE_VERSION}_arm64.deb" | grep drv_video
'
ls -l "$out"
