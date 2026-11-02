#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# ../mkosi.extra/usr/lib/udev/rules.d/90-buntos-tb375fc-hide-ufs.rules를 시험한다. 빌드 VM에서 돈다. 기기 없이 볼 수 있는 것만 본다.
# 1. 규칙 파일 (몇 초, 늘 돈다)
#    - udevadm verify
#    - 모든 줄이 블록 장치의 부모(UFS 호스트)를 가리고 UDISKS_IGNORE·SYSTEM·AUTO 말고는 아무것도 정하지 않는지. /dev의 링크나
#      모드, RUN을 건드리면 진짜 루트에서 nvdata를 붙이는 길과 슬롯을 굽는 길이 깨질 수 있다
#    - 규칙이 기대는 두 이름이 소스에 그대로 있는지: 장치 트리의 ufshci 노드(mt6897.dtsi)와 커널의 드라이버 이름 ufshcd-mtk.
#      이름이 바뀌면 규칙이 조용히 빗나가 안드로이드 파티션이 다시 보인다
# 2. QEMU virt(KVM) (--root를 주면 돈다)
#    UFS 호스트(LU 둘)에 안드로이드를 흉내 낸 GPT와 buntOS 루트를 두고 USB 메모리를 함께 달아, initramfs로 루트에 넘어간다.
#    진짜 커널과 이미지의 udev가 규칙을 돌린 결과를 본다:
#    - UFS의 모든 디스크와 파티션(루트 포함)에 UDISKS_*가 붙고, USB 메모리에는 붙지 않는다. 규칙을 가리면 UFS에서도 사라진다
#    - 루트로 넘어가고, /dev/disk/by-partlabel 링크가 그대로이고, nvdata를 ro,noload로 붙이고, 슬롯 파티션을 dd로 굽는 일이
#      된다. 구운 파티션 말고는 바이트가 그대로다
#    - 이미지에 udisks2가 있으면 Block.HintIgnore를 읽고, gvfs(GLib의 볼륨 모니터)가 UFS의 볼륨을 보이지 않는지, udisksctl로
#      직접 붙이는 일은 막히지 않는지(숨김일 뿐이다) 본다
#    QEMU의 UFS 호스트는 PCI(드라이버 ufshcd)라서 기기의 플랫폼 장치(ufshcd-mtk, 112b0000.ufshci)와 이름이 다르다.
#    드라이버 이름 글롭(ufshcd*) 쪽 줄만 이 시험에서 걸리고, 노드 이름(*.ufshci) 쪽 줄은 걸리지 않는다. 그 줄은 기기에서 본다
# 사용법: test.sh [--root raw 파일] [--deb 파일] [--kernel-tree 디렉터리] [--work 디렉터리] [--rules 파일] [--guest-extra 파일] [--keep]
#   --root         이미지의 루트 파티션 파일(mkosi의 buntos.root-arm64.raw). 보드 프로필(bsp/lenovo-tb375fc)을 --include로 넣어 만든
#                  것이어야 규칙이 들어 있다. systemd, udev, kmod, mount가 있으면 되고, 작은 시험 이미지
#                  (../initramfs/test-image/mkosi.conf)면 충분하다. udisks2, gvfs, dbus가 더 있으면 그 시험도 한다
#   --deb          linux-image 꾸러미 (기본 ~/buntos-build/tb375fc/deb/linux-image-7*_arm64.deb)
#   --kernel-tree  커널 소스 (기본 ~/buntos-build/tb375fc/linux-7.2.12). 있으면 드라이버 이름을 본다
#   --work         작업 디렉터리 (기본 ~/buntos-build/tb375fc/udev-verify/test). 끝나면 기록(log/)만 남기고 지운다
#   --rules        시험할 규칙 파일 (기본은 저장소의 것). 1단계가 나쁜 규칙을 실제로 거르는지 볼 때 쓴다
#   --guest-extra  게스트에서 확인 뒤에 더 돌릴 스크립트. 줄마다 EXTRA:를 붙여 기록(log/)에 남기고 판정은 하지 않는다. 기기 없이
#                  따로 알아볼 것(polkit 규칙 후보 등)을 같은 디스크와 같은 부팅에서 볼 때 쓴다
#   --keep         작업 디렉터리를 지우지 않는다
# shellcheck disable=SC2016,SC2053 # 게스트 스크립트의 글은 기기 안의 셸이 풀고, [[ == ]]의 오른쪽은 규칙에서 읽은 글롭이다
set -euo pipefail
umask 022

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../../.." && pwd)
rules="$here/../mkosi.extra/usr/lib/udev/rules.d/90-buntos-tb375fc-hide-ufs.rules"
dtsi="$repo/kernel/tb375fc/dts/mediatek/mt6897.dtsi"
deb='' rootraw='' keep='' extra=''
ktree="$HOME/buntos-build/tb375fc/linux-7.2.12"
work="$HOME/buntos-build/tb375fc/udev-verify/test"
while [[ $# -gt 0 ]]; do
	case $1 in
	--root) rootraw=$2; shift 2 ;;
	--deb) deb=$2; shift 2 ;;
	--kernel-tree) ktree=$2; shift 2 ;;
	--work) work=$2; shift 2 ;;
	--rules) rules=$2; shift 2 ;;
	--guest-extra) extra=$2; shift 2 ;;
	--keep) keep=1; shift ;;
	*) echo "모르는 인자: $1" >&2; exit 2 ;;
	esac
done

pass=0 fail=0
ok() {
	echo "  통과: $1"
	pass=$((pass + 1))
}
bad() {
	echo "  실패: $1"
	fail=$((fail + 1))
}

# 1. 규칙 파일 ------------------------------------------------------------------------------------------------------
echo "규칙 파일"
[[ -f $rules ]] || { echo "규칙 파일이 없다: $rules" >&2; exit 1; }
if ! command -v udevadm >/dev/null; then
	bad "udevadm이 없다 (udev)"
elif out=$(udevadm verify "$rules" 2>&1); then
	ok "udevadm verify (systemd $(udevadm --version))"
else
	bad "udevadm verify: $out"
fi

# 줄마다 토큰을 허용 목록과 견준다. 장치를 UFS 호스트의 자식으로만 좁히고 UDISKS_*만 정해야 한다
mapfile -t rule_lines < <(grep -Ev '^[[:space:]]*(#|$)' "$rules")
nrules=0
for line in "${rule_lines[@]}"; do
	nrules=$((nrules + 1))
	if [[ $line == *\\ ]]; then
		bad "줄 잇기(\\)는 이 검사가 읽지 못한다: $line"
		continue
	fi
	IFS=',' read -ra toks <<<"$line"
	has_block='' has_parent='' has_ignore='' wrong=''
	for t in "${toks[@]}"; do
		t=${t#"${t%%[![:space:]]*}"}
		t=${t%"${t##*[![:space:]]}"}
		case $t in
		'SUBSYSTEM=="block"') has_block=1 ;;
		'SUBSYSTEMS=="platform"') ;;
		DRIVERS=='"'?*'"' | KERNELS=='"'?*'"') has_parent=1 ;;
		'ENV{UDISKS_IGNORE}="1"') has_ignore=1 ;;
		'ENV{UDISKS_SYSTEM}="1"' | 'ENV{UDISKS_AUTO}="0"') ;;
		*) wrong="$wrong [$t]" ;;
		esac
	done
	if [[ -n $wrong ]]; then
		bad "허용하지 않는 토큰:$wrong"
	elif [[ -z $has_block || -z $has_parent || -z $has_ignore ]]; then
		bad "SUBSYSTEM==\"block\", 부모 짝(DRIVERS나 KERNELS), UDISKS_IGNORE가 모두 있어야 한다: $line"
	else
		ok "규칙 ${nrules}은 블록 장치의 부모만 가리고 UDISKS_*만 정한다"
	fi
done
((nrules > 0)) || bad "규칙 줄이 없다"

# 규칙이 기대는 이름이 소스에 있다. 규칙의 글롭(ufshcd*, *.ufshci)이 소스의 실제 이름에 맞는지 글롭으로 견준다
drv_pat=$(printf '%s\n' "${rule_lines[@]}" | sed -n '/DRIVERS==/{s/^.*DRIVERS=="\([^"]*\)".*$/\1/p;q}')
node_pat=$(printf '%s\n' "${rule_lines[@]}" | sed -n '/KERNELS==/{s/^.*KERNELS=="\([^"]*\)".*$/\1/p;q}')
addr=$(sed -n '/^[[:space:]]*\(ufshci: \)\{0,1\}ufshci@[0-9a-f]* {/{s/^.*ufshci@\([0-9a-f]*\) {.*$/\1/p;q}' "$dtsi")
if [[ -z $drv_pat || -z $node_pat ]]; then
	bad "규칙에서 DRIVERS나 KERNELS 글롭을 읽지 못했다"
elif [[ -z $addr ]]; then
	bad "mt6897.dtsi에 ufshci 노드가 없다 (규칙의 노드 이름 짝이 빗나간다)"
elif [[ ${addr}.ufshci == $node_pat ]]; then
	ok "장치 트리의 ufshci@$addr(플랫폼 장치 ${addr}.ufshci)가 규칙의 '$node_pat'에 맞는다"
else
	bad "플랫폼 장치 ${addr}.ufshci가 규칙의 '$node_pat'에 맞지 않는다"
fi
mtk_c="$ktree/drivers/ufs/host/ufs-mediatek.c"
if [[ -f $mtk_c ]]; then
	drv=$(sed -n '/^[[:space:]]*\.name[[:space:]]*=[[:space:]]*"ufshcd/{s/^.*"\(ufshcd[^"]*\)".*$/\1/p;q}' "$mtk_c")
	if [[ -z $drv ]]; then
		bad "$mtk_c에서 플랫폼 드라이버 이름을 찾지 못했다"
	elif [[ $drv == $drv_pat ]]; then
		ok "MediaTek UFS 드라이버 '$drv'가 규칙의 '$drv_pat'에 맞는다"
	else
		bad "MediaTek UFS 드라이버 '$drv'가 규칙의 '$drv_pat'에 맞지 않는다"
	fi
else
	echo "  건너뜀: 커널 소스가 없어 드라이버 이름을 보지 않는다 ($mtk_c, --kernel-tree)"
fi

if [[ -z $rootraw ]]; then
	echo "결과: 통과 $pass, 실패 $fail (QEMU 시험은 --root를 줄 때 돈다)"
	((fail == 0))
	exit
fi

# 2. QEMU ----------------------------------------------------------------------------------------------------------
for tool in qemu-system-aarch64 mkfs.ext4 sfdisk dpkg-deb lz4 sha256sum truncate fallocate; do
	command -v "$tool" >/dev/null || { echo "$tool이 없다" >&2; exit 1; }
done
[[ -f $rootraw ]] || { echo "루트 파티션 파일이 없다: $rootraw" >&2; exit 1; }
[[ -z $extra || -f $extra ]] || { echo "--guest-extra 파일이 없다: $extra" >&2; exit 1; }
if [[ -z $deb ]]; then
	debs=("$HOME"/buntos-build/tb375fc/deb/linux-image-7*_arm64.deb)
	deb=${debs[0]}
fi

rm -rf "$work"
mkdir -p "$work/log"
cleanup() {
	[[ -n ${qemu:-} ]] && kill -9 "$qemu" 2>/dev/null
	if [[ -z $keep ]]; then
		find "$work" -mindepth 1 -maxdepth 1 ! -name log -exec rm -rf {} +
	fi
}
trap cleanup EXIT

# 기록에 무늬가 있는가 / 없는가
has() { if grep -aqE -- "$2" "$log"; then ok "$1"; else bad "$1 (무늬: $2)"; fi; }
hasnt() { if grep -aqE -- "$2" "$log"; then bad "$1 (나온 줄: $(grep -aE -m 1 -- "$2" "$log"))"; else ok "$1"; fi; }

echo "커널과 initramfs"
dpkg-deb -x "$deb" "$work/pkg"
rel=$(ls "$work/pkg/lib/modules")
image="$work/pkg/boot/vmlinuz-$rel"
# virtio_blk는 게스트에 시험 파일을 건네는 디스크, ufshcd_pci는 QEMU의 UFS 호스트다
printf 'virtio_blk\nufshcd_pci\n' >"$work/extra.list"
"$here/../initramfs/mkinitramfs.sh" --modules "$work/pkg" --extra-modules "$work/extra.list" --out "$work/initramfs.cpio.lz4" \
	>"$work/mkinitramfs.out" 2>&1 || {
	sed 's/^/  /' "$work/mkinitramfs.out"
	exit 1
}
sed 's/^/  /' "$work/mkinitramfs.out"
initrd="$work/initramfs.cpio.lz4"

# 쪽 4096바이트 GPT 디스크 (UFS의 LU와 같다). 파티션 시작과 크기는 4096바이트 단위
part_start() { sfdisk --sector-size 4096 -d "$1" | awk -v n="$2" '$1 ~ "[^0-9]"n"$" {gsub(/[^0-9]/,"",$4); print $4}'; }
part_size() { sfdisk --sector-size 4096 -d "$1" | awk -v n="$2" '$1 ~ "[^0-9]"n"$" {gsub(/[^0-9]/,"",$6); print $6}'; }
# 파일을 파티션에 쓴다. 먼저 그 자리를 비워 sparse를 지킨다
put() {
	local start
	start=$(part_start "$1" "$2")
	fallocate -p -o $((start * 4096)) -l "$(stat -c %s "$3")" "$1"
	dd if="$3" of="$1" bs=4096 seek="$start" conv=notrunc,sparse status=none
}
# 파티션의 해시
part_hash() { dd if="$1" bs=4096 skip="$(part_start "$1" "$2")" count="$(part_size "$1" "$2")" status=none | sha256sum | cut -d' ' -f1; }

echo "디스크"
lnx=0fc63daf-8483-4772-8e79-3d69d8477de4
root_type=b921b045-1df0-41c3-af44-4c6f280d3fae
raw_mib=$(($(stat -c %s "$rootraw") / 1048576 + 1))
ufs0="$work/ufs0.img" ufs1="$work/ufs1.img" usb="$work/usb.img"
truncate -s $((raw_mib + 128))M "$ufs0"
# 안드로이드를 흉내 낸 파티션 여섯과 buntOS 루트. 이름은 기기의 것이 아니라 흉내다
sfdisk -q --sector-size 4096 "$ufs0" <<-EOF
	label: gpt
	sector-size: 4096
	first-lba: 256
	name=boot_a, size=4MiB, type=$lnx
	name=boot_b, size=4MiB, type=$lnx
	name=nvdata, size=16MiB, type=$lnx
	name=nvcfg, size=8MiB, type=$lnx
	name=persist, size=8MiB, type=$lnx
	name=userdata, size=16MiB, type=$lnx
	name=buntos, size=${raw_mib}MiB, type=$root_type
EOF
head -c $((4 << 20)) /dev/urandom >"$work/boot_a.bin"
head -c $((4 << 20)) /dev/urandom >"$work/boot_b.bin"
put "$ufs0" 1 "$work/boot_a.bin"
put "$ufs0" 2 "$work/boot_b.bin"
mkdir -p "$work/nvtree/APCFG/APRDEB" "$work/cfgtree" "$work/pertree" "$work/usertree"
head -c 2048 /dev/urandom >"$work/nvtree/APCFG/APRDEB/WIFI"
echo nvcfg >"$work/cfgtree/file"
echo persist >"$work/pertree/file"
echo userdata >"$work/usertree/file"
for p in "nvdata 16 nvtree 3" "nvcfg 8 cfgtree 4" "persist 8 pertree 5" "userdata 16 usertree 6"; do
	read -r name size tree n <<<"$p"
	mkfs.ext4 -q -F -b 4096 -L "$name" -d "$work/$tree" "$work/$name.ext4" "${size}M"
	put "$ufs0" "$n" "$work/$name.ext4"
done
put "$ufs0" 7 "$rootraw"
# 두 번째 LU: 부트 LU처럼 작은 디스크에 raw 파티션 하나
truncate -s 8M "$ufs1"
sfdisk -q --sector-size 4096 "$ufs1" <<-EOF
	label: gpt
	sector-size: 4096
	first-lba: 256
	name=preloader_a, size=1MiB, type=$lnx
EOF
head -c $((1 << 20)) /dev/urandom >"$work/pl.bin"
put "$ufs1" 1 "$work/pl.bin"
# USB 메모리: 512바이트 쪽 GPT에 ext4 하나
truncate -s 64M "$usb"
sfdisk -q "$usb" <<-EOF
	label: gpt
	first-lba: 2048
	name=usbdata, size=32MiB, type=$lnx
EOF
mkdir -p "$work/usbtree"
echo "from the usb stick" >"$work/usbtree/hello.txt"
mkfs.ext4 -q -F -L USBSTICK -d "$work/usbtree" "$work/usb.ext4" 32M
dd if="$work/usb.ext4" of="$usb" bs=512 seek=2048 conv=notrunc status=none
# 구운 파티션 말고는 바이트가 그대로여야 한다
declare -A before
for n in 1 2 3 4 5 6; do before[$n]=$(part_hash "$ufs0" "$n"); done
before_pl=$(part_hash "$ufs1" 1)
before_usb=$(sha256sum <"$usb" | cut -d' ' -f1)
wifi_sha=$(sha256sum <"$work/nvtree/APCFG/APRDEB/WIFI" | cut -d' ' -f1)
# 슬롯에 굽는 내용: boot_b 크기 그대로
head -c $((4 << 20)) /dev/urandom >"$work/slot.bin"
slot_sha=$(sha256sum <"$work/slot.bin" | cut -d' ' -f1)
echo "  UFS 디스크 ${raw_mib}MiB+, 부트 LU 8MiB, USB 64MiB"

# 게스트에서 도는 확인. 줄마다 표지를 붙여 호스트가 무늬로 읽는다
mkdir -p "$work/guest"
cat >"$work/guest/run.sh" <<'EOF'
#!/bin/bash
exec 2>&1
rule=/usr/lib/udev/rules.d/90-buntos-tb375fc-hide-ufs.rules

# 블록 장치가 달린 SCSI 호스트의 종류: UFS, USB, 그 밖
kind_of() {
	local p h
	p=$(readlink -f "/sys/class/block/$1")
	[[ $p =~ /(host[0-9]+)/ ]] || { echo OTHER; return; }
	h=${BASH_REMATCH[1]}
	case $(cat "/sys/class/scsi_host/$h/proc_name" 2>/dev/null) in
	ufshcd) echo UFS ;;
	usb-storage | uas) echo USB ;;
	*) echo OTHER ;;
	esac
}
prop() { sed -n "s/^$2=//p" <<<"$1"; }
# 모든 디스크·파티션의 UDISKS_* 값. 없으면 -
dump() {
	local n props
	for n in $(ls /sys/class/block); do
		case $n in loop* | ram* | zram* | nbd* | dm-*) continue ;; esac
		props=$(udevadm info -q property -n "/dev/$n" 2>/dev/null)
		printf '%s BLK %s %s %s ignore=%s system=%s auto=%s\n' "$1" "$n" "$(kind_of "$n")" "$(prop "$props" DEVTYPE)" \
			"$(prop "$props" UDISKS_IGNORE | grep . || echo -)" "$(prop "$props" UDISKS_SYSTEM | grep . || echo -)" \
			"$(prop "$props" UDISKS_AUTO | grep . || echo -)"
	done
}
settle() { udevadm control --reload && udevadm trigger --action=change --subsystem-match=block && udevadm settle; }
# gvfs(GLib의 볼륨 모니터, 파일 관리자가 보는 것)가 볼륨으로 내놓는 장치와, 그 가운데 UFS의 것
have_gio() { command -v dbus-run-session >/dev/null && command -v gio >/dev/null && command -v udisksctl >/dev/null; }
gio_list() { timeout 90 dbus-run-session -- gio mount -li 2>&1; }
# gio mount -li의 출력에서 Volume(n)이나 Drive(n) 아래의 unix-device 값. 드라이브는 볼륨이 없어도 남는다
gio_devs() {
	awk -v want="$2" '
		/^[ \t]*(Drive|Volume|Mount)\(/ { sub(/^[ \t]*/, ""); mode = substr($0, 1, index($0, "(") - 1); next }
		mode == want && /unix-device:/ { match($0, /\047[^\047]*\047/); print substr($0, RSTART + 1, RLENGTH - 2) }
	' <<<"$1" | tr '\n' ' '
}
ufs_in() {
	local n out=''
	for n in $(ls /sys/class/block); do
		[[ $n == sd* && $(kind_of "$n") == UFS && " $1 " == *" /dev/$n "* ]] && out="$out $n"
	done
	echo "${out:- none}"
}
# udisksctl info에서 속성 값
hint() { grep -E "^ *$2:" <<<"$1" | awk '{print $2}'; }

echo "T: start $(cut -d' ' -f1 /proc/uptime)"
echo "STATE: $(timeout 500 systemctl is-system-running --wait 2>&1)"
echo "T: booted $(cut -d' ' -f1 /proc/uptime)"
echo "FAILED: $(systemctl --failed --no-legend --plain | awk '{print $1}' | tr '\n' ' ')"
echo "UDEV: $(udevadm --version)"
echo "ROOT: $(findmnt -no SOURCE,FSTYPE,OPTIONS /)"
echo "ROOTLABEL: $(readlink -f /dev/disk/by-partlabel/buntos)"
echo "RULE: $([[ -f $rule ]] && echo installed || echo missing)"
echo "VERIFY: $(udevadm verify "$rule" 2>&1 | grep -E 'Success|Fail' | tr -s ' \n' ' ')"
# USB 메모리가 보일 때까지
for _ in $(seq 60); do
	for n in $(ls /sys/class/block); do
		[[ $n == sd* ]] && [[ $(kind_of "$n") == USB ]] && break 2
	done
	sleep 1
done
udevadm settle
echo "T: usb $(cut -d' ' -f1 /proc/uptime)"
dump ON

# 대조: 규칙을 가리면 UFS에서도 사라진다
mkdir -p /etc/udev/rules.d
ln -sf /dev/null /etc/udev/rules.d/90-buntos-tb375fc-hide-ufs.rules
settle
dump OFF
if have_gio; then
	g=$(gio_list)
	vols=$(gio_devs "$g" Volume)
	echo "GIO-OFF-VOLUMES: $vols"
	echo "GIO-OFF-UFS:$(ufs_in "$vols")"
fi
rm -f /etc/udev/rules.d/90-buntos-tb375fc-hide-ufs.rules
settle
dump ON2

# 다른 길: by-partlabel 링크, nvdata를 ro,noload로 붙이기, 슬롯 굽기, USB 메모리 붙이기
for l in boot_a boot_b nvdata nvcfg persist userdata buntos preloader_a usbdata; do
	echo "LINK: $l $(readlink -f "/dev/disk/by-partlabel/$l")"
done
mkdir -p /mnt/nv /mnt/usb
if blockdev --setro /dev/disk/by-partlabel/nvdata && mount -t ext4 -o ro,noload /dev/disk/by-partlabel/nvdata /mnt/nv; then
	echo "NVMOUNT: ok $(findmnt -no OPTIONS /mnt/nv | cut -d, -f1-2)"
	echo "WIFI: $(sha256sum </mnt/nv/APCFG/APRDEB/WIFI | cut -d' ' -f1)"
	umount /mnt/nv
else
	echo "NVMOUNT: fail"
fi
if dd if=/run/guest/slot.bin of=/dev/disk/by-partlabel/boot_b bs=4096 conv=fsync status=none; then
	echo "SLOT: $(dd if=/dev/disk/by-partlabel/boot_b bs=4096 status=none | sha256sum | cut -d' ' -f1)"
else
	echo "SLOT: fail"
fi
usbpart=$(readlink -f /dev/disk/by-label/USBSTICK)
if mount -o ro "$usbpart" /mnt/usb; then echo "USBMOUNT: ok $(cat /mnt/usb/hello.txt)"; umount /mnt/usb; else echo "USBMOUNT: fail"; fi
echo "USBPART: ${usbpart##*/}"

# udisks2와 gvfs가 있으면 (전체 이미지에는 있다): 숨김일 뿐 막지는 않는지
if command -v udisksctl >/dev/null; then
	echo "UDISKS: present"
	ufspart=$(readlink -f /dev/disk/by-partlabel/persist)
	for p in "$ufspart" "$usbpart"; do
		info=$(udisksctl info -b "$p" 2>&1)
		echo "HINTS: ${p##*/} ignore=$(hint "$info" HintIgnore) system=$(hint "$info" HintSystem) auto=$(hint "$info" HintAuto)"
	done
	out=$(udisksctl mount -b "$ufspart" --options ro 2>&1)
	echo "UDISKSCTL-UFS: rc=$? ${out:0:120}"
	findmnt -no TARGET "$ufspart" >/dev/null && udisksctl unmount -b "$ufspart" >/dev/null 2>&1
	if have_gio; then
		g=$(gio_list)
		vols=$(gio_devs "$g" Volume)
		echo "GIO-ON-VOLUMES: $vols"
		echo "GIO-ON-DRIVES: $(gio_devs "$g" Drive)"
		echo "GIO-ON-UFS:$(ufs_in "$vols")"
		sed 's/^/GIORAW: /' <<<"$g" | head -n 40
		echo "GIO-USB-SHOWN: $([[ " $vols " == *" $usbpart "* ]] && echo yes || echo no)"
	else
		echo "GIO: absent"
	fi
else
	echo "UDISKS: absent"
fi
echo "T: end $(cut -d' ' -f1 /proc/uptime)"
echo "FINISHED"
EOF
chmod 755 "$work/guest/run.sh"
cp "$work/slot.bin" "$work/guest/slot.bin"
files=(run.sh slot.bin)
if [[ -n $extra ]]; then
	cp "$extra" "$work/guest/extra.sh"
	files+=(extra.sh)
fi
(cd "$work/guest" && tar -cf "$work/guest.tar" "${files[@]}")
truncate -s 16M "$work/guest.tar"

# 띄우기 ------------------------------------------------------------------------------------------------------------
qemu='' log="$work/log/hide-ufs.log"
fifo="$work/in"
mkfifo "$fifo"
# 시험에서만 쓰는 값: ttyAMA0의 systemd debug shell로 들어가고 콘솔 글이 답에 섞이지 않게 줄인다 (initramfs/test.sh와 같다)
args="root=PARTLABEL=buntos rootfstype=btrfs rw systemd.firstboot=no buntos.wait=20 loglevel=3 systemd.show_status=0"
args="$args systemd.debug_shell=ttyAMA0 systemd.mask=serial-getty@ttyAMA0.service console=hvc0"
echo "띄우기"
qemu-system-aarch64 -M virt -cpu host -accel kvm -smp 2 -m 2048 -display none -serial stdio -monitor none -no-reboot \
	-kernel "$image" -initrd "$initrd" -append "console=ttyAMA0 $args" \
	-drive if=none,id=lu0,file="$ufs0",format=raw -drive if=none,id=lu1,file="$ufs1",format=raw \
	-device ufs,id=ufs0,serial=BUNTOSUFS -device ufs-lu,drive=lu0,bus=ufs0,lun=0 -device ufs-lu,drive=lu1,bus=ufs0,lun=1 \
	-device qemu-xhci,id=xhci -drive if=none,id=usb0,file="$usb",format=raw -device usb-storage,bus=xhci.0,drive=usb0 \
	-drive if=none,id=g0,file="$work/guest.tar",format=raw,readonly=on -device virtio-blk-pci,drive=g0 \
	-device virtio-serial-pci -chardev null,id=c1 -device virtconsole,chardev=c1 \
	<"$fifo" >"$log" 2>&1 &
qemu=$!
exec 8>"$fifo"
# QEMU가 먼저 죽어도 쓰기 때문에 스크립트가 SIGPIPE로 죽어 판정을 못 내지 않게 한다
trap '' PIPE
send() { printf '%s\n' "$*" >&8 2>/dev/null || true; }
# 기록에 무늬가 나오기를 기다린다. QEMU가 끝났거나 시간이 다 되면 실패한다
wait_for() {
	local pat=$1 t=${2:-60} end
	end=$((SECONDS + t))
	while ! grep -aqE -- "$pat" "$log"; do
		if ((SECONDS >= end)) || ! kill -0 "$qemu" 2>/dev/null; then
			return 1
		fi
		sleep 0.2
	done
}
if wait_for 'root@[a-z]*:|bash-[0-9.]*#' 300; then ok "UFS의 루트로 넘어가 셸까지 왔다"; else bad "셸까지 오지 못했다 ($log)"; fi
sleep 1
send "mkdir -p /run/guest && tar -xf /dev/vda -C /run/guest && bash /run/guest/run.sh >/run/guest/out 2>&1; sed 's/^/OUT: /' /run/guest/out; [ -f /run/guest/extra.sh ] && bash /run/guest/extra.sh >/run/guest/out2 2>&1; sed 's/^/EXTRA: /' /run/guest/out2 2>/dev/null; echo DONE\$((40 + 2))"
if wait_for 'DONE42' 900; then ok "확인을 끝냈다"; else bad "확인이 끝나지 않았다"; fi
send 'sync; poweroff -f'
for _ in $(seq 1 100); do
	kill -0 "$qemu" 2>/dev/null || break
	sleep 0.2
done
kill -9 "$qemu" 2>/dev/null || true
wait "$qemu" 2>/dev/null || true
qemu=''
exec 8>&-

# 확인 --------------------------------------------------------------------------------------------------------------
echo "기록"
hasnt "패닉이 없다" 'Kernel panic|Attempted to kill init'
has "부팅이 끝났다" 'OUT: STATE: running'
hasnt "실패한 유닛이 없다" 'OUT: FAILED: [a-z]'
has "루트가 UFS의 btrfs다 (QEMU의 UFS는 sd로 보인다)" 'OUT: ROOT: /dev/sd[a-z][0-9]+ btrfs rw'
has "규칙이 이미지에 들어 있다" 'OUT: RULE: installed'
has "이미지의 udev가 규칙 문법을 받아들인다" 'OUT: VERIFY: +Success: 1 Fail: 0'
echo "  (이미지의 udev: $(grep -am1 'OUT: UDEV:' "$log" | sed 's/.*UDEV: //' | tr -d '\r'))"
# 장치 쪽: 규칙이 켜진 때(ON, ON2)와 꺼진 때(OFF)
kinds() { grep -aEc "OUT: $1 BLK [a-z0-9]+ $2 " "$log" || true; }
for ph in ON ON2; do
	hasnt "$ph: UFS의 디스크·파티션 가운데 UDISKS_IGNORE가 1이 아닌 것이 없다" "OUT: $ph BLK [a-z0-9]+ UFS [a-z]+ ignore=[^1]"
	hasnt "$ph: UFS의 디스크·파티션 가운데 UDISKS_SYSTEM이 1이 아닌 것이 없다" "OUT: $ph BLK [a-z0-9]+ UFS [a-z]+ ignore=1 system=[^1]"
	hasnt "$ph: UFS의 디스크·파티션 가운데 UDISKS_AUTO가 0이 아닌 것이 없다" "OUT: $ph BLK [a-z0-9]+ UFS [a-z]+ ignore=1 system=1 auto=[^0]"
	hasnt "$ph: USB 메모리에는 UDISKS_IGNORE·SYSTEM이 붙지 않는다" "OUT: $ph BLK [a-z0-9]+ USB [a-z]+ (ignore=[^-]|ignore=- system=[^-])"
done
# 디스크 둘(LU 둘)과 파티션 일곱 + 하나, USB 디스크 하나와 파티션 하나
n_disk=$(grep -aEc "OUT: ON BLK [a-z0-9]+ UFS disk ignore=1 " "$log" || true)
n_part=$(grep -aEc "OUT: ON BLK [a-z0-9]+ UFS partition ignore=1 " "$log" || true)
n_usb=$(grep -aEc "OUT: ON BLK [a-z0-9]+ USB (disk|partition) ignore=- system=- " "$log" || true)
if ((n_disk == 2 && n_part == 8)); then ok "UFS 디스크 ${n_disk}개와 파티션 ${n_part}개가 모두 걸렸다 (LU 둘, 루트 포함)"; else bad "UFS 디스크 ${n_disk}개, 파티션 ${n_part}개 (기대: 2개, 8개)"; fi
if ((n_usb == 2)); then ok "USB 디스크와 파티션 ${n_usb}개는 걸리지 않았다"; else bad "걸리지 않은 USB 장치 ${n_usb}개 (기대: 2개)"; fi
if (($(kinds OFF UFS) > 0)); then
	hasnt "OFF: 규칙을 가리면 UFS에서 UDISKS_*가 사라진다" "OUT: OFF BLK [a-z0-9]+ UFS [a-z]+ ignore=1"
else
	bad "OFF: UFS 장치가 없다"
fi
# 다른 길
for l in boot_a boot_b nvdata nvcfg persist userdata buntos preloader_a; do
	has "by-partlabel/$l 링크가 그대로다" "OUT: LINK: $l /dev/sd[a-z][0-9]+"
done
has "by-partlabel/usbdata 링크가 그대로다" 'OUT: LINK: usbdata /dev/sd[a-z][0-9]+'
has "nvdata를 ro,noload로 붙인다" 'OUT: NVMOUNT: ok ro'
has "Wi-Fi 보정값을 읽는다" "OUT: WIFI: $wifi_sha"
has "슬롯 파티션에 dd로 구운 내용을 읽어 확인한다" "OUT: SLOT: $slot_sha"
has "USB 메모리를 붙인다" 'OUT: USBMOUNT: ok from the usb stick'
if grep -aq 'OUT: UDISKS: present' "$log"; then
	echo "udisks2와 gvfs"
	has "UFS 파티션: HintIgnore와 HintSystem이 참, HintAuto가 거짓이다" 'OUT: HINTS: sd[a-z][0-9]+ ignore=true system=true auto=false'
	has "USB 파티션: HintIgnore와 HintSystem이 거짓이다" 'OUT: HINTS: sd[a-z][0-9]+ ignore=false system=false '
	has "숨김일 뿐이다: udisksctl로 직접 붙이면 붙는다" 'OUT: UDISKSCTL-UFS: rc=0 Mounted /dev/sd'
	if grep -aq 'OUT: GIO: absent' "$log"; then
		echo "  건너뜀: 이미지에 gio나 dbus-run-session이 없다"
	else
		has "gvfs: USB 메모리의 볼륨은 보인다" 'OUT: GIO-USB-SHOWN: yes'
		has "gvfs: UFS의 볼륨은 하나도 보이지 않는다" 'OUT: GIO-ON-UFS: none'
		# 대조: 규칙이 없으면 UFS의 볼륨이 보인다. 이것이 없으면 위 시험은 아무것도 보지 못한 것일 수 있다
		has "대조: 규칙을 가리면 gvfs가 UFS 파티션을 볼륨으로 내놓는다" 'OUT: GIO-OFF-UFS: sd[a-z][0-9]'
	fi
else
	echo "  건너뜀: 이미지에 udisks2가 없다"
fi
# 디스크: 구운 boot_b 말고는 그대로
for n in 1 3 4 5 6; do
	if [[ $(part_hash "$ufs0" "$n") == "${before[$n]}" ]]; then ok "UFS 파티션 $n이 그대로다"; else bad "UFS 파티션 $n이 바뀌었다"; fi
done
if [[ $(part_hash "$ufs0" 2) == "$slot_sha" ]]; then ok "boot_b가 구운 내용이다"; else bad "boot_b가 구운 내용과 다르다"; fi
if [[ $(part_hash "$ufs1" 1) == "$before_pl" ]]; then ok "부트 LU의 파티션이 그대로다"; else bad "부트 LU의 파티션이 바뀌었다"; fi
if [[ -n $extra ]]; then
	echo "  건너뜀: USB 메모리가 그대로인지는 --guest-extra가 쓰기 가능으로 붙였을 수 있어 보지 않는다"
elif [[ $(sha256sum <"$usb" | cut -d' ' -f1) == "$before_usb" ]]; then
	ok "USB 메모리가 그대로다 (읽기 전용으로 붙였다)"
else
	bad "USB 메모리가 바뀌었다"
fi

if [[ -n $extra ]]; then
	echo "추가 확인(--guest-extra)"
	tr -d '\r' <"$log" | grep -a 'EXTRA: ' | sed 's/^.*EXTRA: /  /'
fi
echo "결과: 통과 $pass, 실패 $fail"
((fail == 0))
