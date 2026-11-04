#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 이 기기의 GPU(Mali-G615)를 아는 Mesa를 꾸러미로 만든다. Debian의 Mesa(이미지의 스냅숏은 26.1)는 G615를 모르고,
# 26.3부터 돈다. 이미지와 같은 Debian 스냅숏(image/mkosi.conf의 Snapshot=)의 컨테이너에서 빌드해 이미지의
# 라이브러리에 링크한다. 꾸러미는 /opt/mesa에 넣고 ld.so.conf로 Debian의 Mesa보다 먼저 찾게 한다. Debian이
# G615를 아는 판을 내면 이 폴더와 mkosi.conf의 buntos-mesa를 지운다.
# 사용법: bsp/lenovo-tb375fc/mesa/build.sh   (빌드 VM에서. 처음 약 20분, 디스크 약 5GB)
# 출력: ~/buntos-build/tb375fc/mesa-deb/buntos-mesa_<판>_arm64.deb (mkosi.conf의 PackageDirectories)
# kernel/tb375fc/build.sh image가 지우는 deb/와 다른 폴더다.
set -euo pipefail

version=26.3.0-rc3
sha256=3600c01f11439628d0a85de5ed9cb76f297d13e9783cdeecb91aa685055c9942
package_version=26.3.0~rc3-1

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../../.." && pwd)
build_root=${BUNTOS_BUILD_DIR:-$HOME/buntos-build}/tb375fc
work=$build_root/mesa
out=$build_root/mesa-deb
mkdir -p "$work" "$out"

# 컨테이너는 docker나 rootless nerdctl로 띄운다. rootless에서는 컨테이너의 root가 이 사용자다
if ! command -v docker >/dev/null; then
	docker() { nerdctl "$@"; }
fi
release=$(sed -n 's/^Release=//p' "$repo/image/mkosi.conf")
base=$(sed -n 's/^base=//p' "$repo/tools/image/cargo-debian.sh")
llvm=21

packages=(
	ca-certificates build-essential dpkg-dev pkg-config meson ninja-build
	python3-mako python3-yaml python3-packaging python3-ply python3-pycparser bison flex
	glslang-tools spirv-tools spirv-tools-dev libdrm-dev libwayland-dev libwayland-egl-backend-dev wayland-protocols
	libexpat1-dev zlib1g-dev libzstd-dev libelf-dev libsensors-dev libudev-dev libdisplay-info-dev libvulkan-dev libglvnd-dev
	"llvm-$llvm-dev" "libclang-$llvm-dev" "libclang-cpp$llvm-dev" "clang-$llvm" "libpolly-$llvm-dev"
	"libclc-$llvm-dev" "libllvmspirvlib-$llvm-dev" "llvm-spirv-$llvm" libedit-dev libz3-dev libxml2-dev
)
# 개발 꾸러미를 깐 컨테이너 이미지 (처음 한 번 만든다). 판은 이미지의 스냅숏(Snapshot=)으로 맞춘다.
# snapshot.debian.org는 붐비면 503을 주므로, 판이 그대로인 꾸러미는 deb.debian.org에서 받는다. 내려받은 것은
# 캐시에 남으니 다시 돌리면 이어서 받는다
snapshot=$(sed -n 's/^Snapshot=//p' "$repo/image/mkosi.conf")
image="buntos-mesa-build:$(printf '%s\n' "$base" "$release" "$snapshot" "${packages[*]}" | sha256sum | cut -c1-16)"
if ! docker image inspect "$image" >/dev/null 2>&1; then
	mkdir -p "$work/apt/archives"
	name=buntos-mesa-build-$$
	# shellcheck disable=SC2016 # 컨테이너 안의 bash가 펼친다
	docker run --name "$name" --volume "$work/apt/archives:/var/cache/apt/archives" \
		--env RELEASE="$release" --env SNAPSHOT="$snapshot" --env PACKAGES="${packages[*]}" \
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
Types: deb
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
apt-get -q -y --no-install-recommends install $PACKAGES
' || { docker rm "$name" >/dev/null 2>&1 || true; exit 1; }
	docker commit "$name" "$image" >/dev/null
	docker rm "$name" >/dev/null
fi

tarball=$work/mesa-$version.tar.xz
if [[ ! -f $tarball ]]; then
	curl -fsSL -o "$tarball.part" "https://archive.mesa3d.org/mesa-$version.tar.xz"
	mv "$tarball.part" "$tarball"
fi
echo "$sha256  $tarball" | sha256sum -c -

# shellcheck disable=SC2016 # 컨테이너 안의 bash가 펼친다
docker run --rm --volume "$work:/work" --volume "$out:/out" --workdir /work \
	--env VERSION="$version" --env PACKAGE_VERSION="$package_version" \
	"$image" bash -euo pipefail -c '
rm -rf "mesa-$VERSION" pkg
tar --no-same-owner -xf "mesa-$VERSION.tar.xz"
cd "mesa-$VERSION"
meson setup build --prefix=/opt/mesa --libdir=lib --buildtype=release \
	-Dgallium-drivers=panfrost,llvmpipe -Dvulkan-drivers=panfrost -Dplatforms=wayland -Dglx=disabled \
	-Dgbm=enabled -Degl=enabled -Dgles2=enabled -Dglvnd=enabled -Dllvm=enabled -Dmesa-clc=auto -Dvideo-codecs= \
	-Dvalgrind=disabled -Dlibunwind=disabled
ninja -C build -j"$(nproc)"
pkg=/work/pkg
meson install -C build --destdir "$pkg" --strip >/dev/null
# 빌드할 때만 쓰는 것
rm -rf "$pkg/opt/mesa/include" "$pkg/opt/mesa/lib/pkgconfig"
mkdir -p "$pkg/etc/ld.so.conf.d" "$pkg/DEBIAN" "$pkg/usr/share/doc/buntos-mesa"
# 앱이 어떻게 뜨든(D-Bus 활성화, 에이전트) 같은 Mesa를 쓴다
echo /opt/mesa/lib >"$pkg/etc/ld.so.conf.d/00-opt-mesa.conf"
cp docs/license.rst "$pkg/usr/share/doc/buntos-mesa/copyright"
printf "#!/bin/sh\nset -e\nldconfig\n" >"$pkg/DEBIAN/postinst"
cp "$pkg/DEBIAN/postinst" "$pkg/DEBIAN/postrm"
chmod 755 "$pkg/DEBIAN/postinst" "$pkg/DEBIAN/postrm"
# 의존성은 라이브러리에서 읽는다
cd /work
mkdir -p debian
printf "Source: buntos-mesa\n\nPackage: buntos-mesa\nArchitecture: arm64\n" >debian/control
mapfile -t libraries < <(find "$pkg/opt/mesa/lib" -type f -name "*.so*")
depends=$(dpkg-shlibdeps -O --ignore-missing-info -l"$pkg/opt/mesa/lib" "${libraries[@]}" | sed -n "s/^shlibs:Depends=//p")
rm -rf debian
size=$(du -sk "$pkg" | cut -f1)
cat >"$pkg/DEBIAN/control" <<CONTROL
Package: buntos-mesa
Version: $PACKAGE_VERSION
Architecture: arm64
Maintainer: buntOS contributors
Section: libs
Priority: optional
Installed-Size: $size
Depends: $depends
Description: Mesa $VERSION for the Mali-G615
 Built from the upstream release for the Lenovo TB375FC, in /opt/mesa.
CONTROL
rm -f /out/buntos-mesa_*.deb
dpkg-deb --root-owner-group --build "$pkg" "/out/buntos-mesa_${PACKAGE_VERSION}_arm64.deb" >/dev/null
# 꾸러미가 이미지의 Debian에 깔리고 Mesa를 먼저 찾는지 본다
apt-get -q -y --no-install-recommends install "/out/buntos-mesa_${PACKAGE_VERSION}_arm64.deb" >/dev/null
ldconfig -p | grep -E "libgbm.so.1 |libEGL_mesa.so.0 " | sed "s/.*=> //"
find /opt/mesa/lib /opt/mesa/share -type f | sort
rm -rf "/work/mesa-$VERSION" "$pkg"
dpkg-deb --field "/out/buntos-mesa_${PACKAGE_VERSION}_arm64.deb" Depends
'
ls -l "$out"
