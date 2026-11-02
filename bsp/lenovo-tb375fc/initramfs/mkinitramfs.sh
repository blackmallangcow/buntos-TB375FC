#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# TB375FC 첫 부팅용 initramfs를 만든다. 빌드 VM(Debian arm64)에서 돈다.
# busybox-static, 같은 디렉터리의 init·profile, modules.list의 커널 모듈을 newc cpio로 묶어(mkcpio.py) LZ4 legacy로
# 누른다. 원래 init_boot의 램디스크와 같은 꼴이고, ../boot/mkimages.sh가 init_boot.img에 넣는다.
# 사용법: mkinitramfs.sh [--out 파일] [--modules 디렉터리] [--extra-modules 파일] [--busybox 파일]
#   --out      출력 (기본 ~/buntos-build/tb375fc/images/initramfs.cpio.lz4)
#   --modules  커널 모듈을 설치한 디렉터리(make modules_install INSTALL_MOD_PATH=<여기>). 그 아래 lib/modules/<판>이
#              하나 있어야 한다. modules.list의 모듈과 그 의존 모듈만 넣는다. 주지 않으면 모듈 없이 만든다(모두 내장일 때)
#   --extra-modules  modules.list 뒤에 더할 모듈 목록(같은 꼴). 시험(test.sh)에만 필요한 모듈을 modules.list에 섞지 않으려고 쓴다
#   --busybox  정적 링크한 busybox (기본 /bin/busybox, Debian busybox-static). 이 컴퓨터에서 돌아야 한다(애플릿 목록)
set -euo pipefail
umask 022

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../../.." && pwd)
out="$HOME/buntos-build/tb375fc/images/initramfs.cpio.lz4"
modroot=
extra_modules=/dev/null
busybox=/bin/busybox

die() {
	echo "mkinitramfs: $*" >&2
	exit 1
}

while [[ $# -gt 0 ]]; do
	case $1 in
	--out) out=${2:?"--out에 파일이 필요하다"}; shift 2 ;;
	--modules) modroot=${2:?"--modules에 디렉터리가 필요하다"}; shift 2 ;;
	--extra-modules) extra_modules=${2:?"--extra-modules에 파일이 필요하다"}; shift 2 ;;
	--busybox) busybox=${2:?"--busybox에 파일이 필요하다"}; shift 2 ;;
	*) die "모르는 인자: $1 (사용법은 이 파일의 머리말)" ;;
	esac
done

for tool in lz4 python3 cpio; do
	command -v "$tool" >/dev/null || die "$tool이 없다"
done
[[ -x $busybox ]] || die "$busybox가 없다 (busybox-static)"
if command -v file >/dev/null && ! file -L "$busybox" | grep -q 'statically linked'; then
	die "$busybox가 정적 링크가 아니다 (initramfs에는 라이브러리가 없다)"
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
root="$work/root"
mkdir -p "$root"/{bin,sbin,usr/bin,usr/sbin,dev,proc,sys,run,tmp,root,mnt,var,etc/buntos}
chmod 1777 "$root/tmp"
install -m 0755 "$busybox" "$root/bin/busybox"
# Debian의 busybox는 자기 이름(bin/busybox)도 애플릿으로 낸다. 그 링크가 실행 파일을 덮지 않게 건너뛴다
"$busybox" --list-full | while read -r applet; do
	[[ $applet == linuxrc || ${applet##*/} == busybox ]] && continue
	mkdir -p "$root/$(dirname "$applet")"
	ln -sf /bin/busybox "$root/$applet"
done
[[ -L $root/sbin/init && -L $root/bin/sh && ! -L $root/bin/busybox ]] || die "$busybox에 init이나 sh 애플릿이 없다"
install -m 0755 "$here/init" "$root/init"
install -m 0644 "$here/profile" "$root/etc/profile"
# 가젯 스크립트는 진짜 루트와 같은 파일이다
install -D -m 0755 "$here/../mkosi.extra/usr/libexec/buntos/usb-gadget" "$root/usr/libexec/buntos/usb-gadget"
echo 'root:x:0:0:root:/root:/bin/sh' >"$root/etc/passwd"
echo 'root:x:0:' >"$root/etc/group"
: >"$root/etc/buntos/modules"

# 모듈 하나를 넣는다. busybox modprobe가 누른 모듈을 다 읽지 못하므로 풀어서 넣는다
copy_module() {
	local src=$1 dst=$2 rel=$3
	mkdir -p "$(dirname "$dst/$rel")"
	case $rel in
	*.ko) install -m 0644 "$src/$rel" "$dst/$rel" ;;
	*.ko.xz) xz -dc "$src/$rel" >"$dst/${rel%.xz}" ;;
	*.ko.zst) zstd -dcq "$src/$rel" >"$dst/${rel%.zst}" ;;
	*.ko.gz) gzip -dc "$src/$rel" >"$dst/${rel%.gz}" ;;
	*) die "모르는 모듈 꼴: $rel" ;;
	esac
}

modver=none
if [[ -n $modroot ]]; then
	command -v depmod >/dev/null || die "depmod가 없다 (kmod)"
	vers=("$modroot"/lib/modules/*)
	[[ ${#vers[@]} -eq 1 && -f ${vers[0]}/modules.dep ]] || die "$modroot/lib/modules 아래에 modules.dep이 있는 판이 하나여야 한다"
	src=${vers[0]}
	modver=${src##*/}
	dst="$root/lib/modules/$modver"
	mkdir -p "$dst"
	# 모듈 이름(-와 _를 같게 본다) → modules.dep의 경로와 의존 모듈
	declare -A mpath mdeps
	while IFS=: read -r rel deps; do
		name=${rel##*/}
		name=${name%%.ko*}
		mpath[${name//-/_}]=$rel
		mdeps[${name//-/_}]=$deps
	done <"$src/modules.dep"
	added=() builtin=() missing=()
	while read -r m _; do
		[[ -z $m || $m == \#* ]] && continue
		m=${m//-/_}
		if [[ -n ${mpath[$m]:-} ]]; then
			for rel in ${mpath[$m]} ${mdeps[$m]}; do
				copy_module "$src" "$dst" "$rel"
			done
			echo "$m" >>"$root/etc/buntos/modules"
			added+=("$m")
		# grep -q는 첫 줄에서 끝나 tr이 SIGPIPE를 맞으면 pipefail이 내장 모듈을 "못 찾음"으로 가른다. 끝까지 읽게 한다
		elif [[ -f $src/modules.builtin ]] && tr '-' '_' <"$src/modules.builtin" | grep "/$m\.ko$" >/dev/null; then
			builtin+=("$m")
		else
			missing+=("$m")
		fi
	done < <(cat "$here/modules.list" "$extra_modules")
	for f in modules.order modules.builtin modules.builtin.modinfo; do
		[[ -f $src/$f ]] && install -m 0644 "$src/$f" "$dst/$f"
	done
	depmod -b "$root" "$modver"
	echo "모듈 $modver: 넣음 ${added[*]:-없음} / 내장 ${builtin[*]:-없음} / 못 찾음 ${missing[*]:-없음}"
fi

rev=$(git -C "$repo" rev-parse --short HEAD 2>/dev/null || echo unknown)
if [[ $rev != unknown && -n $(git -C "$repo" status --porcelain -- bsp/lenovo-tb375fc 2>/dev/null) ]]; then
	rev+=-dirty
fi
# head -n 1이면 pipefail에서 busybox가 SIGPIPE를 맞아 가끔(400번에 33번) 스크립트가 141로 끝난다. awk는 끝까지 읽는다
bbver=$("$busybox" 2>&1 | awk 'NR == 1 {print $2}')
echo "busybox $bbver, modules $modver, git $rev" >"$root/etc/buntos/initramfs"

# 커널은 init을 띄우기 전에 /dev/console을 연다. initramfs에는 devtmpfs가 저절로 붙지 않으므로 노드를 넣는다
python3 -I "$here/mkcpio.py" "$work/initramfs.cpio" "$root" dev/console:600:c:5:1 dev/null:666:c:1:3
mkdir -p "$(dirname "$out")"
lz4 -l -9 -f -q "$work/initramfs.cpio" "$out"

# 스스로 확인: 풀리고, init과 루트 전환에 쓰는 애플릿과 /dev/console이 있고, busybox가 링크가 아닌 파일이다
lz4 -dc "$out" | cpio -itv --quiet >"$work/list"
for want in init sbin/init sbin/switch_root sbin/run-init usr/sbin/chroot usr/bin/timeout dev/console etc/profile \
	usr/libexec/buntos/usb-gadget; do
	grep -q " $want\( ->.*\)\?$" "$work/list" || die "만든 initramfs에 $want가 없다"
done
grep -q '^-.* bin/busybox$' "$work/list" || die "만든 initramfs의 bin/busybox가 실행 파일이 아니다"
echo "initramfs: $out ($(stat -c %s "$out")바이트, cpio $(stat -c %s "$work/initramfs.cpio")바이트," \
	"항목 $(wc -l <"$work/list")개, busybox $bbver, 모듈 $modver, git $rev)"
