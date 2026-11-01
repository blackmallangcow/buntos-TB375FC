#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# TB375FC(MediaTek MT6897)용 커널을 빌드한다. 빌드 VM(Linux arm64) 안에서 실행한다.
# 사용법: kernel/tb375fc/build.sh [prepare|config|dtbs|kernel|all|image-config|image]
#   prepare  Linux $version 원본을 받아(kernel.org의 sha256으로 확인) 작업 트리를 만들고, patches/series의
#            패치와 dts/의 장치 트리를 넣는다. 작업 트리는 git으로 원본 상태를 기억해 두고 매번 되돌린 뒤
#            얹는다. 바뀌지 않은 파일은 시각이 그대로라 다시 컴파일하지 않는다
#   config   arm64 defconfig에 config/tb375fc.config를 합친다. 조각의 값이 그대로 들어가지 않은 기호를 알린다
#   dtbs     장치 트리만 빌드한다
#   kernel   Image, 장치 트리, 모듈을 빌드하고 모듈을 modules/에 설치한다
#   all      prepare, config, kernel (기본)
#   image-config  이미지용 커널의 작업 트리를 prepare와 같은 방법으로 만들고, debian-config.sh로 이미지와 같은 시점의
#            Debian forky 커널 설정에 조각을 합친다. 조각의 값이 하나라도 빠지면 멈춘다
#   image    image-config 뒤 커널을 컴파일하고 make bindeb-pkg로 .deb를 만든다(docs/tb375fc/DESIGN.md 4절). 공용 작업
#            트리와 out/ 대신 자기 작업 트리(linux-$version-image)와 출력(out-image/)을 쓴다. 빌드 디스크를 아끼려고
#            끝나면 out-image/를 지우므로, 다음 image는 처음부터 컴파일한다(빌드 VM 6코어에서 약 55분)
# 산출물은 저장소 밖 $BUNTOS_BUILD_DIR/tb375fc(기본 ~/buntos-build/tb375fc)에 둔다.
#   out/arch/arm64/boot/Image                                   커널 (부트 이미지 도구가 LZ4로 줄인다)
#   out/arch/arm64/boot/dts/mediatek/mt6897-lenovo-tb375fc.dtb  장치 트리 (tb373fu도 같은 곳)
#   modules/lib/modules/<판>/                                   모듈
#   deb/linux-image-<판>_<꾸러미 판>_arm64.deb, linux-headers-…  이미지용 커널 (bsp/lenovo-tb375fc/mkosi.conf가 넣는다)
#   deb/extra/                                                  디버그 기호(-dbg), linux-libc-dev, .buildinfo, .changes
# 부트 이미지(boot, vendor_boot, init_boot, dtbo)는 bsp/lenovo-tb375fc/boot/가 이 산출물로 만든다.
# 이미지용 커널의 디버그 정보는 Debian 설정 그대로 켠다(DWARF, BTF, BTF_MODULES). 빌드 디스크를 아끼려고 디버그 섹션은
# zstd로 줄이고, DEBUG_INFO_COMPRESS=none이면 Debian처럼 줄이지 않는다. 설치하는 커널과 모듈은 strip하므로 어느 쪽이든
# 같고, 다른 것은 빌드 디렉터리와 -dbg 꾸러미다.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../.." && pwd)
version=7.2.12
url="https://cdn.kernel.org/pub/linux/kernel/v${version%%.*}.x"
build_root="${BUNTOS_BUILD_DIR:-$HOME/buntos-build}/tb375fc"
src="$build_root/src"
tree="$build_root/linux-$version-tb375fc"
out="$build_root/out"
debs="$build_root/deb"
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
		# 다른 사본으로 만든 dtb가 out/에 남으면 시각 때문에 다시 빌드되지 않는다. 보드 dtb는 늘 새로 만든다(몇 초)
		rm -f "$out"/arch/arm64/boot/dts/mediatek/mt6897-lenovo-*.dtb
	else
		echo "알림: dts/mediatek에 TB375FC 장치 트리가 아직 없어 빌드 목록에서 뺀다" >&2
		git -C "$tree" checkout -q base -- arch/arm64/boot/dts/mediatek/Makefile
	fi
}

config() {
	mkdir -p "$out"
	make_k defconfig >/dev/null
	# merge_config.sh는 임시 파일을 현재 디렉터리에 만든다. 저장소가 아닌 출력 디렉터리에서 돌린다
	(cd "$out" && "$tree/scripts/kconfig/merge_config.sh" -m -O "$out" "$out/.config" "$here/config/tb375fc.config" >/dev/null)
	make_k olddefconfig >/dev/null
	# 조각에 적은 값이 그대로 들어갔는지 본다(의존 기호가 꺼져 있으면 빠진다)
	local line sym missing=0
	while read -r line; do
		[[ "$line" =~ ^(CONFIG_[A-Z0-9_]+)=(.*)$ ]] || continue
		sym=${BASH_REMATCH[1]}
		if ! grep -qx -- "$line" "$out/.config"; then
			echo "알림: $line 이 들어가지 않았다 (지금: $(grep -E "^(# )?${sym}[= ]" "$out/.config" || echo 없음))" >&2
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

# 이미지용 설정. 패치 대기열이 얹힌 자기 작업 트리로 Debian 설정에 조각을 합치므로, 대기열이 더하는 기호까지 모두
# 들어가야 한다(STRICT)
image_config() {
	prepare
	KERNEL_TREE="$tree" KERNEL_OUT="$out" STRICT=1 "$here/debian-config.sh"
	case "${DEBUG_INFO_COMPRESS:-zstd}" in
	zstd)
		"$tree/scripts/config" --file "$out/.config" -d DEBUG_INFO_COMPRESSED_NONE -e DEBUG_INFO_COMPRESSED_ZSTD
		make_k olddefconfig >/dev/null
		;;
	none) ;;
	*)
		echo "DEBUG_INFO_COMPRESS는 zstd나 none이다 (받은 것 $DEBUG_INFO_COMPRESS)" >&2
		exit 2
		;;
	esac
	# 툴체인(pahole, -gz=zstd)이 받지 않으면 olddefconfig가 조용히 끈다
	local sym
	for sym in DEBUG_INFO DEBUG_INFO_BTF DEBUG_INFO_BTF_MODULES DEBUG_INFO_COMPRESSED_"${DEBUG_INFO_COMPRESS:-zstd}"; do
		if ! grep -qx "CONFIG_${sym^^}=y" "$out/.config"; then
			echo "CONFIG_${sym^^}이 켜지지 않았다. 빌드 VM의 툴체인을 본다" >&2
			exit 1
		fi
	done
	echo "디버그 정보: DWARF, BTF, BTF_MODULES, 섹션 압축 ${DEBUG_INFO_COMPRESS:-zstd}"
}

# 이미지용 커널 .deb. 꾸러미 판은 <커널 판>-<n>+g<커밋>이다. n은 이 커널에 들어가는 것(kernel/tb375fc, Debian 시점을
# 정하는 image/mkosi.conf)을 바꾼 커밋 수라 커밋마다 는다. 커밋하지 않은 고침이 있으면 ~dirty를 붙여, 같은 커밋을
# 깨끗하게 빌드한 판보다 낮게 둔다
image() {
	local inputs=(kernel/tb375fc image/mkosi.conf) pkgver
	pkgver="$version-$(git -C "$repo" rev-list --count HEAD -- "${inputs[@]}")+g$(git -C "$repo" rev-parse --short=12 HEAD)"
	if [[ -n "$(git -C "$repo" status --porcelain -- "${inputs[@]}")" ]]; then
		pkgver+="~dirty"
	fi
	# 판 문자열(/proc/version)과 꾸러미의 관리자에 빌드 VM의 사용자·호스트 이름 대신 buntos를 쓴다
	export KBUILD_BUILD_USER=buntos KBUILD_BUILD_HOST=buntos DEBFULLNAME=buntOS DEBEMAIL=buntos@localhost
	# 꾸러미 이름은 linux-image-<판>, linux-headers-<판>, linux-image-<판>-dbg, linux-libc-dev이고 원천 이름은 linux-tb375fc다
	export KDEB_PKGVERSION="$pkgver" KDEB_SOURCENAME=linux-tb375fc
	KDEB_CHANGELOG_DIST=$(sed -n 's/^Release=//p' "$repo/image/mkosi.conf")
	export KDEB_CHANGELOG_DIST
	# 커널은 압축하지 않은 Image(EFI 스텁이 든 PE)를 /boot/vmlinuz-<판>으로 싣는다(arm64 기본은 Image.gz). Debian forky의
	# arm64 커널도 그렇고, mkosi의 UKI와 LK 부트 이미지(boot/mkimages.sh가 LZ4로 줄인다)가 그대로 쓴다.
	# 컴파일은 먼저 끝낸다. KBUILD_BUILD_VERSION을 debian/rules가 꾸러미 판에서 넘기는 값과 같게 해, bindeb-pkg가
	# 판 문자열 때문에 vmlinux와 모듈을 다시 링크하지 않게 한다
	local kopts=(KBUILD_IMAGE=arch/arm64/boot/Image KBUILD_BUILD_VERSION="${pkgver##*-}")
	make_k "${kopts[@]}" all
	# 링크가 남긴 임시 vmlinux(.tmp_vmlinux1·2)는 디버그 정보가 다 든 사본이라 크다. make의 대상이 아니라 지워도 다시
	# 빌드하지 않고, 다시 링크할 때 새로 만든다
	rm -f "$out"/.tmp_vmlinux*
	local krel f
	krel=$(cat "$out/include/config/kernel.release")
	# 꾸러미는 둘로 나눠 만든다. -dbg를 함께 만들면 그 펼친 트리(약 4.4GB, 모듈의 디버그 재배치는 압축되지 않는다),
	# dpkg-deb의 임시 파일과 .deb(각 약 1.6GB)가 빌드 출력(약 17GB)에 더해진다.
	# 1) bindeb-pkg로 image·headers·libc-dev. dpkg-buildpackage는 결과를 출력 디렉터리의 부모($build_root)에 쓴다. 꾸러미
	#    안의 모듈은 strip해 서명하고 xz로 줄인다(INSTALL_MOD_STRIP=1)
	DEB_BUILD_PROFILES=pkg.linux-tb375fc.nokerneldbg make_k "${kopts[@]}" bindeb-pkg
	# mkosi는 deb/의 꾸러미로 저장소를 만든다(하위 디렉터리는 보지 않는다). 이미지에 넣지 않는 것은 extra/에 둔다
	rm -rf "$debs"
	mkdir -p "$debs/extra"
	for f in "$build_root"/*_"$pkgver"_*; do
		case "$f" in
		*/linux-libc-dev_*.deb | *.buildinfo | *.changes) mv "$f" "$debs/extra/" ;;
		*) mv "$f" "$debs/" ;;
		esac
	done
	# 2) 객체 파일과 의존 기록(.o, .*.cmd, 약 11GB)을 지우고, 같은 debian/rules의 -dbg 대상만 컴파일 단계(build-arch)
	#    없이 부른다. builddeb의 -dbg는 .ko, vmlinux, modules.order만 읽는다
	find "$out" \( -name '*.o' -o -name '.*.cmd' \) -delete
	(cd "$out" && make -f debian/rules -o build-arch binary-image-dbg)
	mv "$build_root"/linux-image-"$krel"-dbg_"$pkgver"_*.deb "$debs/extra/"
	# 3) 출력 디렉터리를 지운다. 객체 파일이 없어 이어 빌드할 수 없다. 이미지 빌드는 가끔 하고 이어 빌드는 개발
	#    트리(out/)가 맡으므로, 다음 image는 처음부터 다시 컴파일한다
	rm -rf "$out"
	ls -l "$debs" "$debs/extra"
	echo "커널: $krel, 꾸러미 판 $pkgver"
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
image-config | image)
	tree="$build_root/linux-$version-image"
	out="$build_root/out-image"
	image_config
	if [[ $1 == image ]]; then
		image
	fi
	;;
*)
	echo "사용법: $0 [prepare|config|dtbs|kernel|all|image-config|image]" >&2
	exit 2
	;;
esac
