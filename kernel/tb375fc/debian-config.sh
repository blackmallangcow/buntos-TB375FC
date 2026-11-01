#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 이미지용 커널 설정을 확인한다. buntOS 이미지의 Debian forky 커널 설정에 config/tb375fc.config를 합친다
# (docs/tb375fc/DESIGN.md 4절). 빌드 VM(Linux arm64) 안에서 실행하고, 커널은 빌드하지 않는다.
# 사용법: kernel/tb375fc/debian-config.sh
#   1. image/mkosi.conf가 고정한 snapshot.debian.org 시점에서 linux-image-arm64가 가리키는 커널의 설정을 받는다.
#      Release 서명(debian-archive-keyring), Packages.xz와 패키지의 sha256으로 확인한다. forky의
#      linux-image-<판>은 linux-base·linux-binary·linux-modules를 묶기만 하고, 설정은 linux-base-<판>의
#      /usr/lib/modules/<판>/config에 있다
#   2. 그 설정에 조각을 merge_config.sh로 합치고 olddefconfig를 돌린다
#   3. 조각의 값이 그대로 들어가지 않은 기호를 알린다(build.sh config와 같은 검사)
# 원천 트리는 원본 linux-$version(패치 없음)을 읽기만 한다. build.sh의 작업 트리와 out/은 건드리지 않는다.
# 패치 대기열이 더하는 기호까지 보려면 KERNEL_TREE에 build.sh prepare가 만든 작업 트리를 준다(그 트리로 빌드하지
# 않을 때). 산출물은 저장소 밖 $BUNTOS_BUILD_DIR/tb375fc(기본 ~/buntos-build/tb375fc)에 둔다.
#   debian-config/<snapshot>/config-<판>  Debian 설정 (받은 Release, Packages.xz, 패키지도 여기 둔다)
#   out-debian/.config                    조각을 합친 설정 (KERNEL_OUT으로 바꾼다)
# build.sh image-config는 KERNEL_TREE·KERNEL_OUT에 이미지용 작업 트리와 출력을 주고 STRICT=1로 부른다. STRICT=1이면
# 조각의 값이 빠지거나 원천 트리에 없는 기호가 있을 때 3으로 끝난다.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../.." && pwd)
version=7.2.12
build_root="${BUNTOS_BUILD_DIR:-$HOME/buntos-build}/tb375fc"
srctree=${KERNEL_TREE:-$build_root/linux-$version}
out=${KERNEL_OUT:-$build_root/out-debian}
keyring=/usr/share/keyrings/debian-archive-keyring.pgp
# 설정만 만드니 컴파일하는 것은 kconfig 도구뿐이다. 같은 VM의 커널 빌드를 방해하지 않게 2로 둔다
jobs=${JOBS:-2}
make_k() { make -C "$srctree" O="$out" ARCH=arm64 LOCALVERSION= -j"$jobs" "$@"; }

# 이미지와 같은 Debian 시점을 쓴다(ADR-004)
release=$(sed -n 's/^Release=//p' "$repo/image/mkosi.conf")
snapshot=$(sed -n 's/^Snapshot=//p' "$repo/image/mkosi.conf")
if [[ -z "$release" || ! "$snapshot" =~ ^[0-9]{8}T[0-9]{6}Z$ ]]; then
	echo "image/mkosi.conf의 Release=나 Snapshot=을 읽지 못했다" >&2
	exit 1
fi
snap="https://snapshot.debian.org/archive/debian/$snapshot"
dl="$build_root/debian-config/$snapshot"

# 시점이 고정돼 있어 한 번 받은 파일은 다시 받지 않는다. 확인은 매번 한다
get() {
	[[ -f "$dl/$2" ]] && return
	curl -fsSL --retry 3 -o "$dl/$2.part" "$snap/$1"
	mv "$dl/$2.part" "$dl/$2"
}

check_sha256() {
	local want=$2 have
	have=$(sha256sum "$dl/$1" | cut -d' ' -f1)
	if [[ -z "$want" || "$want" != "$have" ]]; then
		echo "$1의 sha256이 맞지 않는다 (기대 ${want:-없음}, 받은 것 $have)" >&2
		exit 1
	fi
}

# Packages.xz에서 패키지 하나의 문단을 꺼낸다
paragraph() { xz -dc "$dl/Packages.xz" | awk -v p="Package: $1" -v RS= 'index($0, p "\n") == 1'; }
# 패키지의 Depends에서 이름이 $2로 시작하는 첫 패키지
depends_on() {
	paragraph "$1" | sed -n 's/^Depends: //p' | tr ',' '\n' |
		awk -v p="$2" '!found && index($1, p) == 1 { print $1; found = 1 }'
}

fetch() {
	local index=main/binary-arm64/Packages.xz image base para file
	mkdir -p "$dl"
	get "dists/$release/Release" Release
	get "dists/$release/Release.gpg" Release.gpg
	sqv --keyring "$keyring" --signature-file "$dl/Release.gpg" "$dl/Release" >/dev/null
	get "dists/$release/$index" Packages.xz
	check_sha256 Packages.xz "$(awk -v f="$index" '/^SHA256:/ { s = 1; next } /^[^ ]/ { s = 0 } s && $3 == f { print $1 }' "$dl/Release")"
	image=$(depends_on linux-image-arm64 linux-image-)
	base=$(depends_on "$image" linux-base-)
	krel=${image#linux-image-}
	if [[ -z "$image" || -z "$base" ]]; then
		echo "Packages.xz에서 linux-image-arm64의 커널 패키지를 찾지 못했다 ($release, $snapshot)" >&2
		exit 1
	fi
	if [[ ${krel%%[+-]*} != "$version" ]]; then
		echo "Debian 커널($krel)이 원천 트리의 판 $version과 다르다. 이 파일과 build.sh의 version을 함께 올린다" >&2
		exit 1
	fi
	para=$(paragraph "$base")
	file=$(awk '$1 == "Filename:" { print $2 }' <<<"$para")
	get "$file" "${file##*/}"
	check_sha256 "${file##*/}" "$(awk '$1 == "SHA256:" { print $2 }' <<<"$para")"
	dpkg-deb --fsys-tarfile "$dl/${file##*/}" | tar -xO "./usr/lib/modules/$krel/config" >"$dl/config-$krel.part"
	mv "$dl/config-$krel.part" "$dl/config-$krel"
	echo "Debian 설정: $base $(awk '$1 == "Version:" { print $2 }' <<<"$para")의 config-$krel (linux-image-arm64 → $image, $release $snapshot)"
}

config() {
	if [[ ! -f "$srctree/Makefile" ]]; then
		echo "원천 트리 $srctree가 없다. 원본은 build.sh prepare가 받은 것을 푼다: tar -xf $build_root/src/linux-$version.tar.xz -C $build_root" >&2
		exit 1
	fi
	mkdir -p "$out"
	# merge_config.sh는 임시 파일을 현재 디렉터리에 만든다. 원천 트리나 저장소가 아닌 출력 디렉터리에서 돌린다
	(cd "$out" && "$srctree/scripts/kconfig/merge_config.sh" -m -O "$out" "$dl/config-$krel" "$here/config/tb375fc.config" >/dev/null)
	make_k olddefconfig >/dev/null
	# 조각에 적은 값이 그대로 들어갔는지 본다(의존 기호가 모듈이거나 꺼져 있으면 m으로 내려가거나 빠진다).
	# 원천 트리의 Kconfig에 없는 기호는 패치 대기열이 더하는 것이라 따로 모아 알린다
	local line sym missing=0 absent=()
	while read -r line; do
		[[ "$line" =~ ^(CONFIG_[A-Z0-9_]+)=(.*)$ ]] || continue
		sym=${BASH_REMATCH[1]}
		grep -qx -- "$line" "$out/.config" && continue
		if ! grep -rqE --include='Kconfig*' "^[[:space:]]*(menu)?config ${sym#CONFIG_}[[:space:]]*$" "$srctree"; then
			absent+=("$sym")
			continue
		fi
		echo "알림: $line 이 들어가지 않았다 (지금: $(grep -E "^(# )?${sym}[= ]" "$out/.config" || echo 없음))" >&2
		missing=$((missing + 1))
	done <"$here/config/tb375fc.config"
	if [[ ${#absent[@]} -gt 0 ]]; then
		echo "알림: 원천 트리에 없는 기호라 보지 못했다(패치 대기열): ${absent[*]}" >&2
	fi
	echo "설정: $out/.config (Debian $krel 바탕, 조각에서 빠진 값 $missing개)"
	if [[ "${STRICT:-}" == 1 && $((missing + ${#absent[@]})) -gt 0 ]]; then
		echo "조각의 값이 모두 들어가지 않았다 (STRICT=1)" >&2
		exit 3
	fi
}

if [[ $# -gt 0 ]]; then
	echo "사용법: $0 (인자 없음)" >&2
	exit 2
fi
fetch
config
