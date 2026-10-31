#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# mkimages.sh를 시험한다. 빌드 VM에서 돈다.
# 1. 입력을 만든다(주지 않으면): 가짜 arm64 Image(LZ4 legacy 덩이가 둘 나오는 10MiB), 작은 DTB, 작은 newc cpio
# 2. mkimages.sh로 이미지 넷을 만들고(--boot-ramdisk로 한 벌 더), tools/hw/bootimg.py로 다시 푼다
# 3. 푼 커널·램디스크·DTB가 넣은 것과 바이트가 같은지, DT 영역이 dt_table.py의 결과와 같은지, 벤더 램디스크에 표지가
#    있는지, dtbo가 빈 오버레이인지 본다. AOSP unpack_bootimg가 있으면 그것으로도 풀어 견준다
# 4. check_images.py로 머리 필드를 원래 이미지와 견준다. 원래 이미지가 있으면 그 DTB와 오버레이를 dt_table.py로 다시
#    감싸 원래 DT 영역과 바이트가 같은지도 본다
# 사용법: test.sh [--kernel Image] [--dtb DTB] [--initramfs 파일] [--stock 디렉터리] [--keep]
#   --stock  원래 이미지(boot, init_boot, vendor_boot, dtbo .img) 디렉터리. 기본은 저장소의 .handoff/tb375fc/stock
#            (레노버 파일이라 저장소에 넣지 않는다). 없으면 check_images.py의 REFERENCE와 견준다
#   --keep   작업 디렉터리를 지우지 않는다
set -euo pipefail
umask 022

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../../.." && pwd)
kernel= dtb= initramfs= keep=
stock="$repo/.handoff/tb375fc/stock"
while [[ $# -gt 0 ]]; do
	case $1 in
	--kernel) kernel=$2; shift 2 ;;
	--dtb) dtb=$2; shift 2 ;;
	--initramfs) initramfs=$2; shift 2 ;;
	--stock) stock=$2; shift 2 ;;
	--keep) keep=1; shift ;;
	*) echo "모르는 인자: $1" >&2; exit 2 ;;
	esac
done

work=$(mktemp -d)
if [[ -n $keep ]]; then
	echo "작업 디렉터리: $work"
else
	trap 'rm -rf "$work"' EXIT
fi
pass=0 fail=0
check() {
	local what=$1
	shift
	if "$@" >/dev/null 2>&1; then
		echo "  통과: $what"
		pass=$((pass + 1))
	else
		echo "  실패: $what"
		fail=$((fail + 1))
	fi
}

# 견주기 도우미. 파이프 끝의 grep -q가 앞 명령을 SIGPIPE로 끝내면 pipefail에 걸리므로 출력을 먼저 받아 둔다
is_lz4() { [[ $(head -c 4 "$1" | od -An -tx1 | tr -d ' \n') == 02214c18 ]]; }
lz4_same() { is_lz4 "$1" && lz4 -dc "$1" | cmp -s - "$2"; }
cpio_has() {
	local list
	list=$(lz4 -dc "$1" | cpio -it --quiet)
	grep -qx "$2" <<<"$list"
}
marker_has_dtb() {
	local text
	text=$(lz4 -dc "$1" | cpio -i --quiet --to-stdout etc/buntos/vendor_boot)
	[[ $text == *"sha256 $(sha256sum "$2" | cut -c1-12)"* ]]
}
empty_overlay() { [[ $(dtc -q -I dtb -O dts "$1" | grep -c '{') == 1 ]]; }
prefix_same() { head -c "$(stat -c %s "$1")" "$2" | cmp -s - "$1"; }
aosp_vendor_args() {
	local want args
	args=$(unpack_bootimg --boot_img "$1" --out "$work/aosp/args" --format=mkbootimg -0 | tr '\0' '\n')
	for want in "--pagesize 0x00001000" "--base 0x00000000" "--kernel_offset 0x40000000" \
		"--ramdisk_offset 0x66f00000" "--tags_offset 0x47c80000" "--dtb_offset 0x0000000047c80000" \
		"--vendor_cmdline bootopt=64S3,32N2,64N2"; do
		[[ $(grep -A 1 -x -- "${want%% *}" <<<"$args" | sed -n 2p) == "${want#* }" ]] || return 1
	done
}

echo "입력"
if [[ -z $kernel ]]; then
	kernel="$work/Image"
	python3 -I - "$kernel" <<-'EOF'
		import random, struct, sys
		rng = random.Random(375)
		body = rng.randbytes(2 << 20) + b"buntOS TB375FC fake kernel\n" * ((8 << 20) // 27)
		# arm64 Image 머리: code0, code1, text_offset, image_size(BSS 1MiB를 더한다), flags(LE, 4K, 어디든), magic
		size = 64 + len(body)
		head = struct.pack("<IIQQQQQQ4sI", 0x91005A4D, 0, 0, size + (1 << 20), 0xA, 0, 0, 0, b"ARM\x64", 0)
		open(sys.argv[1], "wb").write(head + body)
	EOF
fi
if [[ -z $dtb ]]; then
	dtb="$work/test.dtb"
	printf '/dts-v1/;\n/ {\n\tmodel = "buntOS test";\n\tcompatible = "buntos,test";\n};\n' |
		dtc -q -I dts -O dtb -o "$dtb" -
fi
if [[ -z $initramfs ]]; then
	initramfs="$work/initramfs.cpio"
	mkdir -p "$work/rd/etc" "$work/rd/dev"
	printf '#!/bin/sh\nexec /bin/sh\n' >"$work/rd/init"
	chmod 755 "$work/rd/init"
	echo test >"$work/rd/etc/hello"
	python3 -I "$here/../initramfs/mkcpio.py" "$initramfs" "$work/rd" dev/console:600:c:5:1
fi
echo "  kernel $kernel ($(stat -c %s "$kernel")바이트), dtb $dtb, initramfs $initramfs"

echo "만들기"
"$here/mkimages.sh" --kernel "$kernel" --dtb "$dtb" --initramfs "$initramfs" --out "$work/images" >"$work/mkimages.log"
"$here/mkimages.sh" --kernel "$kernel" --dtb "$dtb" --initramfs "$initramfs" --boot-ramdisk \
	--out "$work/images2" >>"$work/mkimages.log"
echo "  $(ls "$work/images" | tr '\n' ' ')"

echo "다시 풀기 (tools/hw/bootimg.py)"
u="$work/unpacked"
python3 -I "$repo/tools/hw/bootimg.py" "$work/images"/{boot,vendor_boot,init_boot,dtbo}.img "$u" >"$work/unpack.log"
python3 -I "$repo/tools/hw/bootimg.py" "$work/images2"/{boot,vendor_boot,init_boot,dtbo}.img "$u.2" >/dev/null
sed 's/^/  /' "$work/unpack.log"

echo "바이트 견주기"
check "boot.img의 커널이 LZ4 legacy이고, 풀면 넣은 Image와 같다" lz4_same "$u/boot.kernel" "$kernel"
check "boot.img에 램디스크가 없다" test ! -e "$u/boot.ramdisk"
if is_lz4 "$initramfs"; then
	check "init_boot.img의 램디스크가 넣은 initramfs와 같다" cmp -s "$u/init_boot.ramdisk" "$initramfs"
else
	check "init_boot.img의 램디스크가 LZ4 legacy이고, 풀면 넣은 cpio와 같다" lz4_same "$u/init_boot.ramdisk" "$initramfs"
fi
check "vendor_boot.img의 DT 표 항목이 넣은 DTB와 같다" cmp -s "$u/vendor_boot-0.dtb" "$dtb"
python3 -I "$here/dt_table.py" create "$work/expect.dt" "$dtb"
check "vendor_boot.img의 DTB 영역이 dt_table.py create의 결과와 같다" cmp -s "$u/vendor_boot.dtb" "$work/expect.dt"
printf '/dts-v1/;\n/plugin/;\n\n/ {\n};\n' | dtc -q -I dts -O dtb -o "$work/empty.dtbo" -
check "dtbo.img의 오버레이가 빈 오버레이와 같다" cmp -s "$u/dtbo-0.dtb" "$work/empty.dtbo"
check "빈 오버레이에 노드가 없다" empty_overlay "$u/dtbo-0.dtb"
check "벤더 램디스크 0(type 1)에 표지와 DTB 해시가 있다" marker_has_dtb "$u/vendor_ramdisk0.cpio.compressed" "$dtb"
check "벤더 램디스크 1(recovery)에 표지가 있다" cpio_has "$u/vendor_ramdisk1.cpio.compressed" etc/buntos/vendor_boot_recovery
check "--boot-ramdisk: boot.img의 램디스크가 init_boot.img의 것과 같다" cmp -s "$u.2/boot.ramdisk" "$u.2/init_boot.ramdisk"
check "--boot-ramdisk: 커널은 그대로다" cmp -s "$u.2/boot.kernel" "$u/boot.kernel"
check "같은 입력이면 같은 이미지가 나온다" cmp -s "$work/images/vendor_boot.img" "$work/images2/vendor_boot.img"

if command -v unpack_bootimg >/dev/null; then
	echo "AOSP unpack_bootimg로 견주기"
	for img in boot init_boot vendor_boot; do
		unpack_bootimg --boot_img "$work/images/$img.img" --out "$work/aosp/$img" >/dev/null
	done
	check "boot: 커널" cmp -s "$work/aosp/boot/kernel" "$u/boot.kernel"
	check "init_boot: 램디스크" cmp -s "$work/aosp/init_boot/ramdisk" "$u/init_boot.ramdisk"
	check "vendor_boot: DTB 영역" cmp -s "$work/aosp/vendor_boot/dtb" "$u/vendor_boot.dtb"
	check "vendor_boot: 벤더 램디스크 0" cmp -s "$work/aosp/vendor_boot/vendor_ramdisk00" "$u/vendor_ramdisk0.cpio.compressed"
	check "vendor_boot: 벤더 램디스크 1" cmp -s "$work/aosp/vendor_boot/vendor_ramdisk01" "$u/vendor_ramdisk1.cpio.compressed"
	check "vendor_boot: mkbootimg 인자로 다시 읽은 쪽 크기, 적재 주소, 명령줄" aosp_vendor_args "$work/images/vendor_boot.img"
fi

echo "머리 필드 견주기 (check_images.py)"
ok=
if [[ -f $stock/vendor_boot.img ]]; then
	python3 -I "$here/check_images.py" "$work/images" "$stock" >"$work/check.log" && ok=1
	python3 -I "$repo/tools/hw/bootimg.py" "$stock"/{boot,vendor_boot,init_boot,dtbo}.img "$work/stock" >/dev/null
	python3 -I "$here/dt_table.py" create "$work/stock/re-vendor.dt" "$work/stock/vendor_boot-0.dtb"
	python3 -I "$here/dt_table.py" create "$work/stock/re-dtbo.dt" "$work/stock/dtbo-0.dtb"
	check "원래 DTB를 dt_table.py로 감싸면 원래 vendor_boot의 DT 영역과 같다" \
		cmp -s "$work/stock/re-vendor.dt" "$work/stock/vendor_boot.dtb"
	check "원래 오버레이를 감싸면 원래 dtbo.img의 DT 표(서명 블록 앞)와 같다" \
		prefix_same "$work/stock/re-dtbo.dt" "$stock/dtbo.img"
else
	echo "  원래 이미지가 없어($stock) REFERENCE와 견준다"
	python3 -I "$here/check_images.py" "$work/images" >"$work/check.log" && ok=1
fi
grep -v -E ' (같음|참고)$' "$work/check.log" | sed 's/^/  /'
check "머리 필드에 뜻밖의 차이가 없다" test -n "$ok"

echo "결과: 통과 $pass, 실패 $fail"
((fail == 0))
