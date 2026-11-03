#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 장치 트리와 initramfs를 안에 넣은 시험 커널을 빌드 VM에서 만든다. 먼저 kernel/tb375fc/build.sh all과
# mkinitramfs.sh를 돌려 둔다. 공용 작업 트리를 잠깐 고치고 끝에 build.sh prepare로 되돌린다. out/은 시험용으로
# 남으니, 보통 커널이 필요하면 build.sh all을 다시 돌린다.
# 사용법: build-test.sh [출력 디렉터리]   (기본 저장소의 .handoff/tb375fc-test-kernel/images)
set -e
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../../.." && pwd)
cd "$repo"
dest=${1:-$repo/.handoff/tb375fc-test-kernel/images}
B=${BUNTOS_BUILD_DIR:-$HOME/buntos-build}/tb375fc; T=$B/linux-7.2.12-tb375fc; O=$B/out; K=$O/arch/arm64/boot; W=$B/test-kernel
mkdir -p "$W" "$dest"
restore() { kernel/tb375fc/build.sh prepare >/dev/null 2>&1 || true; }
trap restore EXIT
kernel/tb375fc/build.sh prepare 2>&1 | tail -n 1 | cut -c1-100
cpp -nostdinc -I "$T/arch/arm64/boot/dts/mediatek" -I "$T/arch/arm64/boot/dts" -I "$T/include" -I "$T/scripts/dtc/include-prefixes" \
	-undef -D__DTS__ -x assembler-with-cpp "$here/mt6897-lenovo-tb375fc-test.dts" 2>/dev/null >"$W/test.pre.dts"
dtc -q -I dts -O dtb -i "$T/arch/arm64/boot/dts/mediatek" -o "$W/test.dtb" "$W/test.pre.dts"
fdtget -t s "$W/test.dtb" /chosen bootargs
python3 -I "$here/builtin-dtb.py" "$T" "$W/test.dtb"
"$T/scripts/config" --file "$O/.config" --set-str INITRAMFS_SOURCE "$B/images/initramfs.cpio.lz4" -e RD_LZ4
touch "$T/arch/arm64/kernel/tb375fc-dtb.S"
make -C "$T" O="$O" ARCH=arm64 LOCALVERSION= olddefconfig >/dev/null
make -C "$T" O="$O" ARCH=arm64 LOCALVERSION= -j"$(nproc)" Image 2>&1 | grep -E 'error|warning: .*(setup|tb375fc)|OBJCOPY' | tail -n 4
bsp/lenovo-tb375fc/boot/mkimages.sh --kernel "$K/Image" --dtb "$W/test.dtb" --initramfs "$B/images/initramfs.cpio.lz4" --out "$W/images" | tail -n 1
cp "$W/images/boot.img" "$W/test.dtb" "$dest/"
sha256sum "$dest/boot.img"
