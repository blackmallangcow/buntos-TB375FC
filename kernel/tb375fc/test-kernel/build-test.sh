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
# 퓨즈 사본(lk-private.dtsi)은 저장소에 넣지 않고 주 체크아웃의 .handoff에 둔다. worktree에서 빌드해도 거기서 찾는다.
# 없으면 멈춘다: dts의 __has_include가 조용히 넘어가면 efuse와 LVTS가 뜨지 않아 CPU와 GPU의 열 보호가 없는 커널이 된다.
common=$(cd "$(git -c safe.directory='*' -C "$repo" rev-parse --path-format=absolute --git-common-dir)/.." && pwd)
private=$common/.handoff/tb375fc-test-kernel
if [[ ! -f "$private/lk-private.dtsi" ]]; then
	echo "멈춤: $private/lk-private.dtsi가 없다. gen_dts.py로 기기에서 만든다" >&2
	exit 1
fi
dest=${1:-$common/.handoff/tb375fc-test-kernel/images}
B=${BUNTOS_BUILD_DIR:-$HOME/buntos-build}/tb375fc; T=$B/linux-7.2.12-tb375fc; O=$B/out; K=$O/arch/arm64/boot; W=$B/test-kernel
mkdir -p "$W" "$dest"
restore() { kernel/tb375fc/build.sh prepare >/dev/null 2>&1 || true; }
trap restore EXIT
kernel/tb375fc/build.sh prepare 2>&1 | tail -n 1 | cut -c1-100
cpp -nostdinc -I "$private" -I "$T/arch/arm64/boot/dts/mediatek" -I "$T/arch/arm64/boot/dts" -I "$T/include" -I "$T/scripts/dtc/include-prefixes" \
	-undef -D__DTS__ -x assembler-with-cpp "$here/mt6897-lenovo-tb375fc-test.dts" 2>/dev/null >"$W/test.pre.dts"
# TEST_ROOTFLAGS를 주면 명령줄에 rootflags=로 넣는다. 같은 파티션의 다른 서브볼륨으로 켤 때 쓴다
[ -z "${TEST_ROOTFLAGS:-}" ] || sed -i "s/root=PARTLABEL=buntos /root=PARTLABEL=buntos rootflags=$TEST_ROOTFLAGS /" "$W/test.pre.dts"
dtc -q -I dts -O dtb -i "$T/arch/arm64/boot/dts/mediatek" -o "$W/test.dtb" "$W/test.pre.dts"
fdtget -t s "$W/test.dtb" /chosen bootargs
if ! fdtget -p "$W/test.dtb" /chosen | grep -qx 'atag,devinfo'; then
	echo "멈춤: 시험 장치 트리에 퓨즈 사본이 없다" >&2
	exit 1
fi
python3 -I "$here/builtin-dtb.py" "$T" "$W/test.dtb"
"$T/scripts/config" --file "$O/.config" --set-str INITRAMFS_SOURCE "$B/images/initramfs.cpio.lz4" -e RD_LZ4
touch "$T/arch/arm64/kernel/tb375fc-dtb.S"
make -C "$T" O="$O" ARCH=arm64 LOCALVERSION= olddefconfig >/dev/null
make -C "$T" O="$O" ARCH=arm64 LOCALVERSION= -j"$(nproc)" Image 2>&1 | grep -E 'error|warning: .*(setup|tb375fc)|OBJCOPY' | tail -n 4
bsp/lenovo-tb375fc/boot/mkimages.sh --kernel "$K/Image" --dtb "$W/test.dtb" --initramfs "$B/images/initramfs.cpio.lz4" --out "$W/images" | tail -n 1
cp "$W/images/boot.img" "$W/test.dtb" "$dest/"
sha256sum "$dest/boot.img"
