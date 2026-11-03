#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# TB375FC의 LK가 읽는 부트 이미지 넷을 만든다. 빌드 VM(AOSP mkbootimg)에서 돈다.
# 꼴은 원래 펌웨어(글로벌판 ZUI 17.0.04.266)를 따른다(docs/tb375fc/DESIGN.md 2.2절, docs/hw/lenovo-tb375fc.md 2절).
#   boot.img         헤더 v4, 커널만(LZ4 legacy). 명령줄은 --cmdline (원래는 비어 있다)
#   init_boot.img    헤더 v4, 램디스크만(initramfs, LZ4 legacy cpio)
#   vendor_boot.img  헤더 v4, 쪽 4096, 원래의 적재 주소, 명령줄 bootopt=64S3,32N2,64N2, DTB 영역은 DT 표(dt_table.py),
#                    벤더 램디스크 표 2개(type 1 이름 없음, type 2 recovery). 둘 다 표지 파일 하나만 든 램디스크다.
#                    init이 표지를 읽어 LK가 우리 vendor_boot(곧 우리 장치 트리)를 읽었는지 알린다
#   dtbo.img         DT 표, 빈 오버레이 하나 (벤더 dtbo는 벤더 노드 경로를 찾는 fixup이 우리 트리에서 실패할 수 있다)
# 원래 이미지와 달리 AVB 꼬리와 파티션 크기 채움, dtbo 뒤의 MediaTek 서명 블록(cert1·cert2)이 없다.
# 기기의 LK는 언락해도 DT 표가 있는 dtbo에 서명을 요구해 이 dtbo.img를 거부한다(2026-11-03). 파티션 앞을 0으로 채우면 건너뛴다.
#
# 사용법: mkimages.sh --kernel <Image> --dtb <DTB> --initramfs <cpio 또는 cpio.lz4> [선택]...
#   --kernel <Image>        압축하지 않은 arm64 Image (arch/arm64/boot/Image)
#   --dtb <DTB>             보드 장치 트리 (mt6897-lenovo-tb375fc.dtb)
#   --initramfs <파일>      newc cpio. 누르지 않았으면 LZ4 legacy로 누른다 (../initramfs/mkinitramfs.sh가 만든다)
#   --cmdline <명령줄>      boot.img의 커널 명령줄 (기본은 아래 cmdline)
#   --vendor-cmdline <줄>   vendor_boot의 명령줄 (기본 bootopt=64S3,32N2,64N2. LK가 읽는 값이라 바꾸지 않는다)
#   --boot-ramdisk          initramfs를 boot.img에도 넣는다. LK가 그 램디스크를 쓰는지 볼 때 쓴다(확인되지 않았다).
#                           이 기기의 LK에는 fastboot boot가 없어 이미지는 슬롯에 굽는다(2026-11-03)
#   --out <디렉터리>        출력 (기본 ~/buntos-build/tb375fc/images). 이미지 넷과 images.txt(해시, 입력, 명령줄)
set -euo pipefail
umask 022

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
cpio_tool="$here/../initramfs/mkcpio.py"
out="$HOME/buntos-build/tb375fc/images"
kernel='' dtb='' initramfs='' boot_ramdisk=''
# *_ignore_unused: 소비자가 아직 없는 클럭·전원 도메인·레귤레이터를 늦은 정리에서 끄지 않는다(LK가 켜 둔 화면 등)
cmdline="console=ttyS0,921600n8 console=tty0 earlycon clk_ignore_unused pd_ignore_unused regulator_ignore_unused"
vendor_cmdline="bootopt=64S3,32N2,64N2"

# 원래 vendor_boot의 머리 값 (unpack_bootimg로 읽었다)
pagesize=0x1000
kernel_addr=0x40000000
ramdisk_addr=0x66f00000
tags_addr=0x47c80000
dtb_addr=0x47c80000
# 파티션 크기 (원래 이미지는 이 크기로 채워져 있다. 기기의 fastboot getvar로도 같다)
declare -A part_size=([boot]=$((64 << 20)) [init_boot]=$((8 << 20)) [vendor_boot]=$((64 << 20)) [dtbo]=$((8 << 20)))

die() {
	echo "mkimages: $*" >&2
	exit 1
}

while [[ $# -gt 0 ]]; do
	case $1 in
	--kernel) kernel=${2:?"--kernel에 파일이 필요하다"}; shift 2 ;;
	--dtb) dtb=${2:?"--dtb에 파일이 필요하다"}; shift 2 ;;
	--initramfs) initramfs=${2:?"--initramfs에 파일이 필요하다"}; shift 2 ;;
	--cmdline) cmdline=${2?"--cmdline에 값이 필요하다"}; shift 2 ;;
	--vendor-cmdline) vendor_cmdline=${2?"--vendor-cmdline에 값이 필요하다"}; shift 2 ;;
	--boot-ramdisk) boot_ramdisk=1; shift ;;
	--out) out=${2:?"--out에 디렉터리가 필요하다"}; shift 2 ;;
	*) die "모르는 인자: $1 (사용법은 이 파일의 머리말)" ;;
	esac
done
[[ -n $kernel && -n $dtb && -n $initramfs ]] || die "--kernel, --dtb, --initramfs가 모두 있어야 한다"
for f in "$kernel" "$dtb" "$initramfs"; do
	[[ -f $f ]] || die "$f가 없다"
done
for tool in mkbootimg lz4 dtc python3 cpio sha256sum; do
	command -v "$tool" >/dev/null || die "$tool이 없다"
done

# arm64 Image 머리: 0x38에 magic "ARM\x64", 0x10에 image_size(BSS까지 든 메모리 크기)
read -r magic image_size < <(python3 -I -c '
import struct, sys
d = open(sys.argv[1], "rb").read(64)
print(d[56:60].hex(), struct.unpack("<Q", d[16:24])[0])' "$kernel")
[[ $magic == 41524d64 ]] || die "$kernel이 압축하지 않은 arm64 Image가 아니다 (Image.gz, Image.lz4는 받지 않는다)"
((image_size > 0)) || image_size=$(stat -c %s "$kernel")
# LK는 커널을 kernel_addr에 풀고 DTB를 dtb_addr에 둔다. 커널이 DTB 자리까지 닿으면 안 된다
if ((kernel_addr + image_size > dtb_addr)); then
	die "커널 메모리 크기 $image_size바이트가 DTB 자리($dtb_addr)까지 닿는다 (최대 $((dtb_addr - kernel_addr))바이트)"
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

lz4 -l -9 -f -q "$kernel" "$work/kernel.lz4"
case $(head -c 6 "$initramfs" | od -An -tx1 | tr -d ' \n') in
3037303730*) lz4 -l -9 -f -q "$initramfs" "$work/initramfs.cpio.lz4" ;;
02214c18*) cp "$initramfs" "$work/initramfs.cpio.lz4" ;;
*) die "$initramfs가 newc cpio도 LZ4 legacy도 아니다" ;;
esac

# 벤더 램디스크 조각 둘. 원래 표처럼 type 1(이름 없음)과 type 2(recovery)를 두고, 표지 파일 하나씩만 담는다
dtb_sha=$(sha256sum "$dtb" | cut -c1-12)
for kind in platform recovery; do
	mkdir -p "$work/$kind/etc/buntos"
done
echo "buntOS, dtb $(basename "$dtb") sha256 $dtb_sha" >"$work/platform/etc/buntos/vendor_boot"
echo "buntOS recovery fragment (empty)" >"$work/recovery/etc/buntos/vendor_boot_recovery"
for kind in platform recovery; do
	python3 -I "$cpio_tool" "$work/$kind.cpio" "$work/$kind"
	lz4 -l -9 -f -q "$work/$kind.cpio" "$work/$kind.cpio.lz4"
done

python3 -I "$here/dt_table.py" create "$work/vendor_dtb.img" "$dtb"
printf '/dts-v1/;\n/plugin/;\n\n/ {\n};\n' | dtc -q -I dts -O dtb -o "$work/empty.dtbo" -
mkdir -p "$work/images"
python3 -I "$here/dt_table.py" create "$work/images/dtbo.img" "$work/empty.dtbo"

boot_args=(--header_version 4 --kernel "$work/kernel.lz4" --cmdline "$cmdline")
if [[ -n $boot_ramdisk ]]; then
	boot_args+=(--ramdisk "$work/initramfs.cpio.lz4")
fi
mkbootimg "${boot_args[@]}" -o "$work/images/boot.img"
mkbootimg --header_version 4 --ramdisk "$work/initramfs.cpio.lz4" -o "$work/images/init_boot.img"
mkbootimg --header_version 4 --pagesize "$pagesize" --base 0 \
	--kernel_offset "$kernel_addr" --ramdisk_offset "$ramdisk_addr" \
	--tags_offset "$tags_addr" --dtb_offset "$dtb_addr" \
	--vendor_cmdline "$vendor_cmdline" --dtb "$work/vendor_dtb.img" \
	--ramdisk_type platform --ramdisk_name '' --vendor_ramdisk_fragment "$work/platform.cpio.lz4" \
	--ramdisk_type recovery --ramdisk_name recovery --vendor_ramdisk_fragment "$work/recovery.cpio.lz4" \
	--vendor_boot "$work/images/vendor_boot.img"

for name in boot init_boot vendor_boot dtbo; do
	size=$(stat -c %s "$work/images/$name.img")
	((size <= part_size[$name])) || die "$name.img($size바이트)가 파티션(${part_size[$name]}바이트)보다 크다"
done

# 커널과 initramfs 모듈의 판이 맞는지 본다 (다르면 init이 모듈을 올리지 못한다)
release=$(LC_ALL=C grep -a -o -m 1 'Linux version [^ ]*' "$kernel" | head -n 1 | awk '{print $3}' || true)
modver=$(lz4 -dc "$work/initramfs.cpio.lz4" | cpio -it --quiet 2>/dev/null | sed -n 's|^lib/modules/\([^/]*\)$|\1|p' || true)
if [[ -n $modver && -n $release && $modver != "$release" ]]; then
	echo "mkimages: 경고: 커널은 $release인데 initramfs의 모듈은 $modver다" >&2
fi
info=$(lz4 -dc "$work/initramfs.cpio.lz4" | cpio -i --quiet --to-stdout etc/buntos/initramfs 2>/dev/null || true)

{
	echo "kernel: $(basename "$kernel") (${release:-판 모름}, 메모리 $image_size바이트)"
	echo "dtb: $(basename "$dtb") sha256 $dtb_sha"
	echo "initramfs: $(basename "$initramfs") (${info:-정보 없음})${boot_ramdisk:+, boot.img에도 넣음}"
	echo "cmdline: $cmdline"
	echo "vendor cmdline: $vendor_cmdline"
	(cd "$work/images" && sha256sum boot.img init_boot.img vendor_boot.img dtbo.img)
} >"$work/images/images.txt"
mkdir -p "$out"
mv -f "$work/images"/* "$out/"
cat "$out/images.txt"
echo "이미지: $out"
