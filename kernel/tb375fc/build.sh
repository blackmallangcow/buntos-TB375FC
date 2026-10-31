#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# TB375FC(MediaTek MT6897)용 커널을 빌드한다. 빌드 VM(Linux arm64) 안에서 실행한다.
# 사용법: kernel/tb375fc/build.sh [prepare|config|dtbs|kernel|all]
#   prepare  Linux $version 원본을 받아(kernel.org의 sha256으로 확인) 작업 트리를 만들고, patches/series의
#            패치와 dts/의 장치 트리를 넣는다. 작업 트리는 git으로 원본 상태를 기억해 두고 매번 되돌린 뒤
#            얹는다. 바뀌지 않은 파일은 시각이 그대로라 다시 컴파일하지 않는다
#   config   arm64 defconfig에 config/tb375fc.config를 합친다. 조각의 값이 그대로 들어가지 않은 기호를 알린다
#   dtbs     장치 트리만 빌드한다
#   kernel   Image, 장치 트리, 모듈을 빌드하고 모듈을 modules/에 설치한다
#   all      prepare, config, kernel (기본)
# 산출물은 저장소 밖 $BUNTOS_BUILD_DIR/tb375fc(기본 ~/buntos-build/tb375fc)에 둔다.
#   out/arch/arm64/boot/Image                                   커널 (부트 이미지 도구가 LZ4로 줄인다)
#   out/arch/arm64/boot/dts/mediatek/mt6897-lenovo-tb375fc.dtb  장치 트리 (tb373fu도 같은 곳)
#   modules/lib/modules/<판>/                                   모듈
# 부트 이미지(boot, vendor_boot, init_boot, dtbo)는 bsp/lenovo-tb375fc/boot/가 이 산출물로 만든다.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
version=7.2.12
url="https://cdn.kernel.org/pub/linux/kernel/v${version%%.*}.x"
build_root="${BUNTOS_BUILD_DIR:-$HOME/buntos-build}/tb375fc"
src="$build_root/src"
tree="$build_root/linux-$version-tb375fc"
out="$build_root/out"
jobs=${JOBS:-$(nproc)}
# LOCALVERSION을 빈 값으로 준다. 작업 트리가 git인데 v<판> 태그가 없어서, 주지 않으면 커널이 판 끝에 +를 붙인다
make_k() { make -C "$tree" O="$out" ARCH=arm64 LOCALVERSION= -j"$jobs" "$@"; }

fetch() {
	mkdir -p "$src"
	local tarball="$src/linux-$version.tar.xz"
	if [[ ! -f "$tarball" ]]; then
		curl -fsSL -o "$tarball.part" "$url/linux-$version.tar.xz"
		mv "$tarball.part" "$tarball"
	fi
	curl -fsSL -o "$src/sha256sums.asc" "$url/sha256sums.asc"
	local want have
	want=$(awk -v f="linux-$version.tar.xz" '$2 == f { print $1 }' "$src/sha256sums.asc")
	have=$(sha256sum "$tarball" | cut -d' ' -f1)
	if [[ -z "$want" || "$want" != "$have" ]]; then
		echo "linux-$version.tar.xz의 sha256이 맞지 않는다 (기대 ${want:-없음}, 받은 것 $have)" >&2
		exit 1
	fi
}

prepare() {
	fetch
	if [[ ! -d "$tree/.git" ]]; then
		rm -rf "$tree"
		mkdir -p "$tree"
		tar -xf "$src/linux-$version.tar.xz" -C "$tree" --strip-components=1
		git -C "$tree" init -q
		git -C "$tree" add -A
		git -C "$tree" -c user.name=buntos -c user.email=buntos@localhost commit -q -m "linux $version"
		git -C "$tree" tag base
	fi
	# 원본 상태로 되돌린다. 바뀐 파일만 다시 쓰므로 나머지는 시각이 그대로다
	git -C "$tree" reset -q --hard base
	git -C "$tree" clean -q -fdx
	local patch
	while read -r patch; do
		[[ -z "$patch" || "$patch" == \#* ]] && continue
		echo "패치: $patch"
		patch -d "$tree" -p1 --forward --quiet <"$here/patches/$patch"
	done <"$here/patches/series"
	# 장치 트리는 파일로 관리해 그대로 넣는다(-p: 바뀌지 않았으면 다시 빌드하지 않는다). 아직 없으면 0001이
	# 더한 빌드 목록 줄을 되돌린다. 그대로 두면 dtbs가 없는 소스를 찾다 실패한다
	if compgen -G "$here/dts/mediatek/mt6897-lenovo-*.dts" >/dev/null; then
		cp -p "$here"/dts/mediatek/*.dts* "$tree/arch/arm64/boot/dts/mediatek/"
	else
		echo "알림: dts/mediatek에 TB375FC 장치 트리가 아직 없어 빌드 목록에서 뺀다" >&2
		git -C "$tree" checkout -q base -- arch/arm64/boot/dts/mediatek/Makefile
	fi
}

config() {
	mkdir -p "$out"
	make_k defconfig >/dev/null
	"$tree/scripts/kconfig/merge_config.sh" -m -O "$out" "$out/.config" "$here/config/tb375fc.config" >/dev/null
	make_k olddefconfig >/dev/null
	# 조각에 적은 값이 그대로 들어갔는지 본다(의존 기호가 꺼져 있으면 빠진다)
	local line sym missing=0
	while read -r line; do
		[[ "$line" =~ ^(CONFIG_[A-Z0-9_]+)=(.*)$ ]] || continue
		sym=${BASH_REMATCH[1]}
		if ! grep -qx -- "$line" "$out/.config"; then
			echo "알림: $line 이 들어가지 않았다 (지금: $(grep -E "^(# )?$sym[= ]" "$out/.config" || echo 없음))" >&2
			missing=$((missing + 1))
		fi
	done <"$here/config/tb375fc.config"
	echo "설정: $out/.config (조각에서 빠진 값 $missing개)"
}

dtbs() {
	make_k dtbs
	ls -l "$out"/arch/arm64/boot/dts/mediatek/mt6897-lenovo-*.dtb
}

kernel() {
	make_k Image dtbs modules
	rm -rf "$build_root/modules"
	make_k INSTALL_MOD_PATH="$build_root/modules" INSTALL_MOD_STRIP=1 modules_install >/dev/null
	ls -l "$out/arch/arm64/boot/Image"
	ls -l "$out"/arch/arm64/boot/dts/mediatek/mt6897-lenovo-*.dtb 2>/dev/null || echo "알림: TB375FC 장치 트리가 아직 없다 (dts/mediatek)" >&2
	echo "커널: $(cat "$out/include/config/kernel.release")"
}

case "${1:-all}" in
prepare) prepare ;;
config) config ;;
dtbs) dtbs ;;
kernel) kernel ;;
all)
	prepare
	config
	kernel
	;;
*)
	echo "사용법: $0 [prepare|config|dtbs|kernel|all]" >&2
	exit 2
	;;
esac
