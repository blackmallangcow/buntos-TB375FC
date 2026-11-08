#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 고친 upowerd를 꾸러미로 만든다. PPS 충전기로 직충전하는 동안 USB 포트 전원의 online은 2다(Online Programmable,
# 커널의 Documentation/ABI/testing/sysfs-class-power). UPower 1.91.5는 이 값을 불리언 속성에 그대로 넣고, GLib은
# 값을 버리면서도 notify를 보낸다. UPower는 그때마다 충전기가 바뀐 것으로 보아 충전 속도를 10초 동안 0으로 두므로
# 직충전 내내 남은 시간이 나오지 않는다. 패치는 0이 아니면 연결된 것으로 읽는다.
# Debian의 upower는 그대로 두고 데몬만 /opt/upower에 넣어 systemd drop-in으로 띄운다. 꾸러미를 지우면 Debian의
# 데몬으로 돌아간다. UPower에 이 고침이 들어오면 이 폴더와 mkosi.conf의 buntos-upower를 지운다.
# 이미지와 같은 Debian 스냅숏(image/mkosi.conf의 Snapshot=)의 컨테이너에서 Debian 소스를 그대로 빌드한다.
# 사용법: bsp/lenovo-tb375fc/upower/build.sh   (빌드 VM에서)
# 출력: ~/buntos-build/tb375fc/upower-deb/buntos-upower_<판>_arm64.deb (mkosi.conf의 PackageDirectories)
set -euo pipefail

debian_version=1.91.5-1
package_version=$debian_version+buntos1

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../../.." && pwd)
build_root=${BUNTOS_BUILD_DIR:-$HOME/buntos-build}/tb375fc
work=$build_root/upower
out=$build_root/upower-deb
mkdir -p "$work" "$out"

# 컨테이너는 docker나 rootless nerdctl로 띄운다. rootless에서는 컨테이너의 root가 이 사용자다
if ! command -v docker >/dev/null; then
	docker() { nerdctl "$@"; }
fi
release=$(sed -n 's/^Release=//p' "$repo/image/mkosi.conf")
base=$(sed -n 's/^base=//p' "$repo/tools/image/cargo-debian.sh")
snapshot=$(sed -n 's/^Snapshot=//p' "$repo/image/mkosi.conf")

# 빌드 의존성을 깐 컨테이너 이미지 (처음 한 번 만든다). 판은 이미지의 스냅숏으로 맞춘다
image="buntos-upower-build:$(printf '%s\n' "$base" "$release" "$snapshot" "$debian_version" | sha256sum | cut -c1-16)"
if ! docker image inspect "$image" >/dev/null 2>&1; then
	mkdir -p "$work/apt/archives"
	name=buntos-upower-build-$$
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
apt-get -q -y --no-install-recommends install ca-certificates build-essential dpkg-dev fakeroot libupower-glib3
apt-get -q -y --no-install-recommends build-dep upower
' || { docker rm "$name" >/dev/null 2>&1 || true; exit 1; }
	docker commit "$name" "$image" >/dev/null
	docker rm "$name" >/dev/null
fi

cp "$here"/*.patch "$work/"

# shellcheck disable=SC2016 # 컨테이너 안의 bash가 펼친다
docker run --rm --volume "$work:/work" --volume "$out:/out" --workdir /work \
	--env DEBIAN_VERSION="$debian_version" --env PACKAGE_VERSION="$package_version" \
	"$image" bash -euo pipefail -c '
export DEBIAN_FRONTEND=noninteractive
rm -rf src pkg x
mkdir src x
cd src
apt-get -q -y --only-source source "upower=$DEBIAN_VERSION"
cd upower-*/
for patch in /work/*.patch; do
	patch -p1 <"$patch"
done
DEB_BUILD_OPTIONS="nocheck nodoc parallel=$(nproc)" DEB_BUILD_PROFILES="nocheck nodoc" dpkg-buildpackage -B -uc -us
dpkg-deb -x "../upower_${DEBIAN_VERSION}_arm64.deb" /work/x
pkg=/work/pkg
install -D -m 0755 /work/x/usr/libexec/upowerd "$pkg/opt/upower/libexec/upowerd"
mkdir -p "$pkg/usr/lib/systemd/system/upower.service.d" "$pkg/usr/share/doc/buntos-upower" "$pkg/DEBIAN"
cat >"$pkg/usr/lib/systemd/system/upower.service.d/50-buntos-upower.conf" <<UNIT
[Service]
ExecStart=
ExecStart=/opt/upower/libexec/upowerd
UNIT
cp debian/copyright "$pkg/usr/share/doc/buntos-upower/copyright"
printf "#!/bin/sh\nset -e\nif [ -d /run/systemd/system ]; then systemctl daemon-reload || true; fi\n" >"$pkg/DEBIAN/postinst"
cp "$pkg/DEBIAN/postinst" "$pkg/DEBIAN/postrm"
chmod 755 "$pkg/DEBIAN/postinst" "$pkg/DEBIAN/postrm"
# 의존성은 바이너리에서 읽는다
cd /work
mkdir -p debian
printf "Source: buntos-upower\n\nPackage: buntos-upower\nArchitecture: arm64\n" >debian/control
depends=$(dpkg-shlibdeps -O --ignore-missing-info "$pkg/opt/upower/libexec/upowerd" | sed -n "s/^shlibs:Depends=//p")
rm -rf debian
size=$(du -sk "$pkg" | cut -f1)
cat >"$pkg/DEBIAN/control" <<CONTROL
Package: buntos-upower
Version: $PACKAGE_VERSION
Architecture: arm64
Maintainer: buntOS contributors
Section: admin
Priority: optional
Installed-Size: $size
Depends: upower (>= $DEBIAN_VERSION), $depends
Description: UPower daemon that accepts a programmable USB PD supply
 upowerd $DEBIAN_VERSION with a fix for the online value 2 of a PPS supply, in
 /opt/upower, started through a systemd drop-in instead of the daemon of the
 upower package.
CONTROL
rm -f /out/buntos-upower_*.deb
dpkg-deb --root-owner-group --build "$pkg" "/out/buntos-upower_${PACKAGE_VERSION}_arm64.deb" >/dev/null
# 꾸러미가 이미지의 Debian에 깔리고 라이브러리를 다 찾는지 본다
apt-get -q -y --no-install-recommends install "/out/buntos-upower_${PACKAGE_VERSION}_arm64.deb" >/dev/null
if ldd /opt/upower/libexec/upowerd | grep "not found"; then
	exit 1
fi
rm -rf /work/src /work/x /work/pkg
dpkg-deb --field "/out/buntos-upower_${PACKAGE_VERSION}_arm64.deb" Package Version Depends
dpkg-deb --contents "/out/buntos-upower_${PACKAGE_VERSION}_arm64.deb" | grep -E "upowerd|\.conf"
'
ls -l "$out"
