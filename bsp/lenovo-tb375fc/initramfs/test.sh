#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# init의 루트 전환을 QEMU virt(KVM)에서 시험한다. 빌드 VM에서 돈다. 기기 없이 볼 수 있는 것만 본다(README).
# 1. 이미지용 커널 꾸러미를 풀고, 모듈을 넣은 initramfs를 만든다. virtio_blk(디스크)와 dummy_hcd(가짜 UDC)는 시험에만 쓰므로
#    --extra-modules로 더한다
# 2. UFS처럼 쪽 4096바이트인 GPT 디스크를 만든다. 안드로이드를 흉내 낸 boot_a·nvdata·userdata, btrfs 루트 buntos(작은 busybox
#    루트), init이 없는 btrfs noinit, 파일 시스템이 아닌 garbage, /run이 없는 btrfs norun이 있다
# 3. 명령줄을 바꿔 가며 띄운다. 넘어가면 루트의 init이 PID 1로 사실을 찍고 끈다. 못 넘어가면 패닉 없이 셸이 떠서 까닭을 읽는다.
#    끝날 때마다 GPT와 안드로이드 파티션의 해시가 그대로인지 본다
# 4. --root를 주면 그 루트 파티션 파일(mkosi의 buntos.root-arm64.raw, test-image/mkosi.conf)로 systemd까지 간다. /run/buntos
#    기록, 디스크 안전, 파일 시스템 늘리기, 가짜 UDC 위의 USB 가젯(호스트 쪽 ttyACM 셸, DHCP, SSH)을 본다
# 사용법: test.sh [--deb 파일] [--root raw 파일] [--work 디렉터리] [--only 경우 이름...] [--keep]
#   --deb   linux-image 꾸러미 (기본 ~/buntos-build/tb375fc/deb/linux-image-*_arm64.deb)
#   --work  작업 디렉터리 (기본 ~/buntos-build/tb375fc/initramfs-verify/test). 끝나면 기록(log/)만 남기고 지운다
#   --only  이 이름의 경우만 돈다 (여러 번 줄 수 있다)
#   --keep  작업 디렉터리를 지우지 않는다
# shellcheck disable=SC2016 # send의 글은 기기 안의 셸이 푼다
set -euo pipefail
umask 022

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
deb='' rootraw='' keep='' only=()
work="$HOME/buntos-build/tb375fc/initramfs-verify/test"
while [[ $# -gt 0 ]]; do
	case $1 in
	--deb) deb=$2; shift 2 ;;
	--root) rootraw=$2; shift 2 ;;
	--work) work=$2; shift 2 ;;
	--only) only+=("$2"); shift 2 ;;
	--keep) keep=1; shift ;;
	*) echo "모르는 인자: $1" >&2; exit 2 ;;
	esac
done
if [[ -z $deb ]]; then
	debs=("$HOME"/buntos-build/tb375fc/deb/linux-image-7*_arm64.deb)
	deb=${debs[0]}
fi
for tool in qemu-system-aarch64 mkfs.btrfs mkfs.ext4 sfdisk dpkg-deb lz4 sha256sum; do
	command -v "$tool" >/dev/null || { echo "$tool이 없다" >&2; exit 1; }
done

rm -rf "$work"
mkdir -p "$work/log"
cleanup() {
	[[ -n ${qemu:-} ]] && kill -9 "$qemu" 2>/dev/null
	if [[ -z $keep ]]; then
		find "$work" -mindepth 1 -maxdepth 1 ! -name log -exec rm -rf {} +
	fi
}
trap cleanup EXIT

pass=0 fail=0
ok() {
	echo "  통과: $1"
	pass=$((pass + 1))
}
bad() {
	echo "  실패: $1"
	fail=$((fail + 1))
}
# 기록에 무늬가 있는가 / 없는가
has() { if grep -aqE -- "$2" "$log"; then ok "$1"; else bad "$1 (무늬: $2)"; fi; }
hasnt() { if grep -aqE -- "$2" "$log"; then bad "$1 (나온 줄: $(grep -aE -m 1 -- "$2" "$log"))"; else ok "$1"; fi; }

echo "커널과 initramfs"
dpkg-deb -x "$deb" "$work/pkg"
rel=$(ls "$work/pkg/lib/modules")
image="$work/pkg/boot/vmlinuz-$rel"
printf 'virtio_blk\ndummy_hcd\n' >"$work/extra.list"
"$here/mkinitramfs.sh" --modules "$work/pkg" --extra-modules "$work/extra.list" --out "$work/initramfs.cpio.lz4" | sed 's/^/  /'
initrd="$work/initramfs.cpio.lz4"

# 작은 busybox 루트. init은 PID 1일 때만 사실을 찍고 끈다. check_init이 정적 링크한 인터프리터의 스크립트를 실제로 돌리므로
# PID 1이 아니면 곧 끝낸다
mkroot() {
	local t=$1 a
	mkdir -p "$t"/{bin,sbin,proc,sys,dev,run,etc}
	install -m 0755 /bin/busybox "$t/bin/busybox"
	for a in $(/bin/busybox --list); do
		[[ $a == busybox ]] || ln -sf busybox "$t/bin/$a"
	done
	cat >"$t/sbin/init" <<-'EOF'
		#!/bin/sh
		[ $$ = 1 ] || exit 0
		echo "TINY pid=$$ args=[$*] ENV=[$ENV] HOME=[$HOME]"
		sed 's/^/TINY mount: /' /proc/self/mountinfo
		ls / | tr '\n' ' ' | sed 's/^/TINY ls: /'; echo
		sed 's/^/TINY log: /' /run/buntos/boot.log
		ps | sed 's/^/TINY ps: /'
		ls -l /run/systemd/system /run/systemd/system-generators /run/repart.d | sed 's/^/TINY guard: /'
		echo TINY-END
		poweroff -f
	EOF
	chmod 755 "$t/sbin/init"
}
mkroot "$work/tiny"

# init이 없는 루트와, 실행되지 못하는 init들
# 새 루트에 /bin/sh가 있어야 시험이 된다. chroot(execvp)가 ENOEXEC이면 그것으로 다시 돌리기 때문이다
n="$work/noinit"
mkdir -p "$n"/{bin,proc,sys,dev,run,bad/dir,lib}
install -m 0755 /bin/busybox "$n/bin/busybox"
ln -s busybox "$n/bin/sh"
ln -s /nonexistent "$n/bad/dangling"
install -m 0644 /dev/null "$n/bad/noexec"
printf 'no shebang here\n' >"$n/bad/text"
chmod 755 "$n/bad/text"
install -m 0755 /dev/null "$n/bad/empty"
printf '#!/nonexistent/sh\n' >"$n/bad/badshebang"
chmod 755 "$n/bad/badshebang"
# x86-64 ELF 머리
printf '\177ELF\002\001\001\000\000\000\000\000\000\000\000\000\002\000\076\000\001\000\000\000' >"$n/bad/x86"
head -c 64 /dev/zero >>"$n/bad/x86"
chmod 755 "$n/bad/x86"
# 동적 링크한 빌드 VM의 true. 로더를 이 루트에 넣고 libc는 넣지 않는다. 인터프리터 경로를 고친 사본은 로더가 없는 꼴이다
loader=$(readelf -l /bin/true | sed -n 's/.*interpreter: \(.*\)]$/\1/p')
install -D -m 0755 "$(readlink -f "$loader")" "$n$loader"
install -m 0755 /bin/true "$n/bad/nolib"
LC_ALL=C sed "s|$loader|${loader/ld-/xx-}|" /bin/true >"$n/bad/nointerp"
chmod 755 "$n/bad/nointerp"
# 시간 제한: 정적 링크한 busybox 스크립트는 check_init에서 실제로 돈다
cat >"$work/tiny/sbin/hang" <<-'EOF'
	#!/bin/sh
	[ $$ = 1 ] || exec sleep 1000
	echo "HANG-ROOT pid=$$"
	poweroff -f
EOF
chmod 755 "$work/tiny/sbin/hang"
# 읽기 전용이면 /run을 만들 수 없는 루트: 옮기다가 실패하고 되돌린다
mkroot "$work/norun"
rmdir "$work/norun/run"

# 쪽 4096바이트 GPT 디스크
sfdisk_script() {
	cat <<-EOF
		label: gpt
		sector-size: 4096
		first-lba: 256
		name=boot_a, size=8MiB, type=ebd0a0a2-b9e5-4433-87c0-68b6b72699c7
		name=nvdata, size=32MiB, type=0fc63daf-8483-4772-8e79-3d69d8477de4
		name=userdata, size=16MiB, type=ebd0a0a2-b9e5-4433-87c0-68b6b72699c7
		name=$1, size=$2, type=b921b045-1df0-41c3-af44-4c6f280d3fae, attrs="GUID:59"
	EOF
	shift 2
	while [[ $# -gt 0 ]]; do
		echo "name=$1, size=$2, type=0fc63daf-8483-4772-8e79-3d69d8477de4"
		shift 2
	done
}
# 파티션 시작과 크기(4096바이트 단위). $1 디스크, $2 번호
part_start() { sfdisk --sector-size 4096 -d "$1" | awk -v n="$2" '$1 ~ n" *$" || $1 ~ "[^0-9]"n"$" {gsub(/[^0-9]/,"",$4); print $4}'; }
part_size() { sfdisk --sector-size 4096 -d "$1" | awk -v n="$2" '$1 ~ "[^0-9]"n"$" {gsub(/[^0-9]/,"",$6); print $6}'; }
# 파일을 파티션에 쓴다. dd의 sparse는 0인 블록을 건너뛰므로 먼저 그 자리를 비운다(앞에 쓴 내용이 남으면 파일 시스템이 깨진다)
put() {
	local start
	start=$(part_start "$1" "$2")
	fallocate -p -o $((start * 4096)) -l "$(stat -c %s "$3")" "$1"
	dd if="$3" of="$1" bs=4096 seek="$start" conv=notrunc,sparse status=none
}
# 안드로이드를 흉내 낸 파티션을 채운다
fill_android() {
	local d=$1 v="$work/nvtree"
	head -c $((8 << 20)) /dev/urandom >"$work/boot_a.bin"
	put "$d" 1 "$work/boot_a.bin"
	mkdir -p "$v/APCFG/APRDEB"
	head -c 2048 /dev/urandom >"$v/APCFG/APRDEB/WIFI"
	mkfs.ext4 -q -F -b 4096 -d "$v" "$work/nvdata.ext4" 32M
	put "$d" 2 "$work/nvdata.ext4"
	head -c $((16 << 20)) /dev/urandom >"$work/userdata.bin"
	put "$d" 3 "$work/userdata.bin"
}
# GPT 앞뒤와 안드로이드 파티션의 해시. 파티션 번호를 더 주면 그것도 본다
disk_hash() {
	local d=$1 total p
	shift
	total=$(($(stat -c %s "$d") / 4096))
	{
		dd if="$d" bs=4096 count=6 status=none
		dd if="$d" bs=4096 skip=$((total - 6)) status=none
		for p in 1 2 3 "$@"; do
			dd if="$d" bs=4096 skip="$(part_start "$d" "$p")" count="$(part_size "$d" "$p")" status=none
		done
	} | sha256sum | cut -d' ' -f1
}

echo "디스크"
disk="$work/disk.img"
truncate -s 900M "$disk"
sfdisk_script buntos 200MiB noinit 200MiB garbage 16MiB norun 200MiB | sfdisk -q --sector-size 4096 "$disk"
fill_android "$disk"
mkbtrfs() {
	truncate -s 200M "$2"
	mkfs.btrfs -q -f -m single -r "$1" "$2"
}
mkbtrfs "$work/tiny" "$work/tiny.btrfs"
put "$disk" 4 "$work/tiny.btrfs"
mkbtrfs "$n" "$work/noinit.btrfs"
put "$disk" 5 "$work/noinit.btrfs"
head -c $((16 << 20)) /dev/urandom >"$work/garbage.bin"
put "$disk" 6 "$work/garbage.bin"
mkbtrfs "$work/norun" "$work/norun.btrfs"
put "$disk" 7 "$work/norun.btrfs"
disk_sum=$(disk_hash "$disk")
# 해시가 정말 GPT와 안드로이드 파티션을 보는지 먼저 본다. 한 바이트를 바꿔 보고 되돌린다
hash_sees() {
	local d=$1 off=$2 old rc=0
	old=$(od -An -tx1 -j "$off" -N 1 "$d" | tr -d ' ')
	if [[ $old == ff ]]; then printf '\xfe'; else printf '\xff'; fi | dd of="$d" bs=1 seek="$off" conv=notrunc status=none
	[[ $(disk_hash "$d") != "$disk_sum" ]] || rc=1
	# shellcheck disable=SC2059 # 값은 위에서 읽은 16진수 두 자리다
	printf "\\x$old" | dd of="$d" bs=1 seek="$off" conv=notrunc status=none
	return $rc
}
if hash_sees "$disk" $(($(stat -c %s "$disk") - 4096)) && hash_sees "$disk" 4096 &&
	hash_sees "$disk" $(($(part_start "$disk" 3) * 4096 + 5)) && [[ $(disk_hash "$disk") == "$disk_sum" ]]; then
	ok "해시가 GPT 앞뒤와 안드로이드 파티션의 변화를 알아본다"
else
	bad "해시가 GPT 앞뒤와 안드로이드 파티션의 변화를 알아보지 못한다"
fi
fs_uuid=$(blkid -p -o value -s UUID "$work/tiny.btrfs" 2>/dev/null || btrfs inspect-internal dump-super "$work/tiny.btrfs" | awk '/^fsid/{print $2}')
part_uuid=$(sfdisk --sector-size 4096 -d "$disk" | awk -F'uuid=' 'NR>0 && /name="buntos"/{split($2,a,","); print a[1]}')
echo "  $disk, GPT와 안드로이드 파티션 $disk_sum"

# 띄우기 ------------------------------------------------------------------------------------------------------------
qemu='' log='' guest_drive=()
want() {
	local c
	[[ ${#only[@]} -eq 0 ]] && return 0
	for c in "${only[@]}"; do
		[[ $c == "$1" ]] && return 0
	done
	return 1
}
start() {
	local name=$1 d=$2 append=$3 mem=${4:-1024} fifo="$work/in"
	log="$work/log/$name.log"
	rm -f "$fifo"
	mkfifo "$fifo"
	qemu-system-aarch64 -M virt -cpu host -accel kvm -smp 2 -m "$mem" -display none -serial stdio -monitor none -no-reboot \
		-kernel "$image" -initrd "$initrd" -append "console=ttyAMA0 $append" \
		-drive if=none,id=d0,file="$d",format=raw \
		-device virtio-blk-pci,drive=d0,logical_block_size=4096,physical_block_size=4096 "${guest_drive[@]}" \
		<"$fifo" >"$log" 2>&1 &
	qemu=$!
	exec 8>"$fifo"
}
send() { printf '%s\n' "$*" >&8; }
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
stop() {
	for _ in $(seq 1 100); do
		kill -0 "$qemu" 2>/dev/null || break
		sleep 0.2
	done
	kill -9 "$qemu" 2>/dev/null || true
	wait "$qemu" 2>/dev/null || true
	qemu=''
	exec 8>&-
	rm -f "$work/in"
}

# 넘어가서 작은 루트의 init이 돌아야 한다
case_switch() {
	local name=$1 append=$2 dev=${3:-/dev/vda4}
	want "$name" || return 0
	echo "경우 $name: $append"
	start "$name" "$disk" "$append"
	if wait_for 'TINY-END' 60; then ok "넘어가서 루트의 init이 끝까지 돌았다"; else bad "루트의 init이 끝나지 않았다"; fi
	stop
	has "init이 PID 1이다" 'TINY pid=1 '
	has "ENV가 넘어가지 않았다" 'ENV=\[\] HOME=\[/\]'
	has "루트가 $dev의 btrfs다" "TINY mount: .* / / (rw|ro),.*btrfs $dev"
	has "boot.log가 /run/buntos에 남았다" 'TINY log: root: switching to'
	has "/run이 tmpfs로 옮겨졌다" 'TINY mount: .* / /run .* tmpfs tmpfs'
	has "/dev/pts가 옮겨졌다" 'TINY mount: .* /dev/pts .*devpts'
	has "configfs가 옮겨졌다" 'TINY mount: .* /sys/kernel/config '
	has "debugfs가 옮겨졌다" 'TINY mount: .* /sys/kernel/debug '
	has "pstore가 옮겨졌다" 'TINY mount: .* /sys/fs/pstore '
	hasnt "initramfs의 파일이 지워졌다(/init 없음)" 'TINY ls: [a-z ]*\binit\b'
	hasnt "/sysroot가 남지 않았다" 'TINY mount: .*sysroot'
	hasnt "initramfs의 다른 프로세스가 남지 않았다(udhcpd, late)" 'TINY ps: .*(udhcpd|--late)'
	has "systemd-repart를 가렸다" 'TINY guard: .*systemd-repart.service -> /dev/null'
	has "gpt-auto 생성기를 가렸다" 'TINY guard: .*systemd-gpt-auto-generator -> /dev/null'
	hasnt "패닉이 없다" 'Kernel panic|Attempted to kill init'
	if [[ $(disk_hash "$disk") == "$disk_sum" ]]; then ok "GPT와 안드로이드 파티션이 그대로다"; else bad "GPT나 안드로이드 파티션이 바뀌었다"; fi
}

# 넘어가지 못하고 셸이 떠야 한다. $3의 무늬가 boot.log에 있어야 한다
case_fail() {
	local name=$1 append=$2 reason=$3 t=${4:-90}
	want "$name" || return 0
	echo "경우 $name: $append"
	start "$name" "$disk" "$append buntos.wait=3"
	if wait_for 'shells on:' "$t" && wait_for 'tb375fc:/# ' 15; then ok "패닉 없이 셸이 떴다"; else bad "셸이 뜨지 않았다"; fi
	# 애플릿 이름이 명령 줄의 메아리와 섞이지 않게 표지를 계산해서 찍는다
	send "sed 's/^/LOG: /' /run/buntos/boot.log; mount | sed 's/^/MNT: /'; ps | sed 's/^/PS: /'; echo DONE\$((40 + 2))"
	wait_for 'DONE42' 15 || bad "셸이 답하지 않았다"
	send 'poweroff -f'
	stop
	has "까닭이 boot.log에 남았다" "LOG: $reason"
	has "까닭이 커널 기록에 남았다" "bunt-init: $reason"
	has "머무른다고 알렸다" 'LOG: root: staying in the initramfs'
	hasnt "/sysroot를 뗐다" 'MNT: .*/sysroot'
	has "/proc·/sys·/dev·/run이 제자리다" 'MNT: tmpfs on /run '
	has "실패 길의 --late가 돌았다" 'LOG: usb gadget: dummy_udc.0'
	has "실패 길에서 nvdata를 읽기 전용으로 붙였다" 'LOG: nvdata: /dev/vda2 mounted read-only'
	hasnt "패닉이 없다" 'Kernel panic|Attempted to kill init'
	if [[ $(disk_hash "$disk") == "$disk_sum" ]]; then ok "GPT와 안드로이드 파티션이 그대로다"; else bad "GPT나 안드로이드 파티션이 바뀌었다"; fi
}

echo "넘어가는 경우"
case_switch partlabel "root=PARTLABEL=buntos rootfstype=btrfs rw"
case_switch auto-fstype "root=PARTLABEL=buntos rw"
case_switch partuuid "root=PARTUUID=$part_uuid rootfstype=btrfs rw"
case_switch partuuid-upper "root=PARTUUID=${part_uuid^^} rootfstype=btrfs rw"
case_switch uuid "root=UUID=$fs_uuid rootfstype=btrfs rw"
case_switch path "root=/dev/vda4 rootfstype=btrfs rw"
case_switch lk-after "root=PARTLABEL=buntos rootfstype=btrfs rw root=/dev/ram"
case_switch lk-before "root=/dev/ram root=PARTLABEL=buntos rootfstype=btrfs rw"
case_switch lk-tabs "$(printf 'root=PARTLABEL=buntos rootfstype=btrfs rw root=/dev/ram\t\t\t  ro init=/init')"
case_switch lk-dm "root=PARTLABEL=buntos root=/dev/dm-0 rootfstype=btrfs rw"

if want rootflags; then
	echo "경우 rootflags: rootflags를 mount에 넘긴다"
	start rootflags "$disk" "root=PARTLABEL=buntos rootfstype=btrfs rootflags=noatime,compress=zstd rw"
	wait_for 'TINY-END' 60 || bad "루트의 init이 끝나지 않았다"
	stop
	has "옵션이 붙었다" 'TINY mount: .* / / rw,noatime .*compress=zstd'
fi
if want rw-ro; then
	echo "경우 rw-ro: ro만 있으면 읽기 전용이다"
	start rw-ro "$disk" "root=PARTLABEL=buntos rootfstype=btrfs ro"
	wait_for 'TINY-END' 60 || bad "루트의 init이 끝나지 않았다"
	stop
	has "루트가 읽기 전용이다" 'TINY mount: .* / / ro,'
fi
if want hang; then
	echo "경우 hang: 확인 때 끝나지 않는 init은 시간 제한 뒤 그대로 넘어간다"
	start hang "$disk" "root=PARTLABEL=buntos rootfstype=btrfs rw init=/sbin/hang"
	if wait_for 'HANG-ROOT pid=1' 60; then ok "시간 제한 뒤 넘어갔다"; else bad "넘어가지 않았다"; fi
	stop
	has "시간 제한을 알렸다" 'bunt-init: root: init /sbin/hang: the check timed out'
	hasnt "패닉이 없다" 'Kernel panic|Attempted to kill init'
fi

echo "넘어가지 못하는 경우"
case_fail norun-ro "root=PARTLABEL=norun rootfstype=btrfs ro" 'root: could not move /run to the new root'
case_fail no-partition "root=PARTLABEL=nonexistent rootfstype=btrfs rw" 'root: PARTLABEL=nonexistent not found within 3s'
case_fail no-partuuid "root=PARTUUID=00000000-0000-0000-0000-000000000000 rw" 'root: PARTUUID=0+-0+-0+-0+-0+ not found within 3s'
case_fail no-uuid "root=UUID=00000000-0000-0000-0000-000000000000 rw" 'root: UUID=0+-0+-0+-0+-0+ not found within 3s'
case_fail no-path "root=/dev/nonexistent rw" 'root: /dev/nonexistent not found within 3s'
case_fail bad-fs "root=PARTLABEL=garbage rootfstype=btrfs rw" 'root: could not mount /dev/vda6'
case_fail bad-fs-auto "root=PARTLABEL=garbage rw" 'root: could not mount /dev/vda6'
case_fail bad-fstype "root=PARTLABEL=buntos rootfstype=nosuchfs rw" 'root: could not mount /dev/vda4'
case_fail no-init "root=PARTLABEL=noinit rootfstype=btrfs rw" "root: init /sbin/init: .*can't execute"
case_fail init-dangling "root=PARTLABEL=noinit rootfstype=btrfs rw init=/bad/dangling" "root: init /bad/dangling: .*can't execute"
case_fail init-noexec "root=PARTLABEL=noinit rootfstype=btrfs rw init=/bad/noexec" "root: init /bad/noexec: .*can't execute"
case_fail init-dir "root=PARTLABEL=noinit rootfstype=btrfs rw init=/bad/dir" 'root: init /bad/dir is not an arm64 program'
case_fail init-x86 "root=PARTLABEL=noinit rootfstype=btrfs rw init=/bad/x86" 'root: init /bad/x86 is not an arm64 program'
case_fail init-text "root=PARTLABEL=noinit rootfstype=btrfs rw init=/bad/text" 'root: init /bad/text is not an arm64 program'
case_fail init-empty "root=PARTLABEL=noinit rootfstype=btrfs rw init=/bad/empty" 'root: init /bad/empty is not an arm64 program'
case_fail init-shebang "root=PARTLABEL=noinit rootfstype=btrfs rw init=/bad/badshebang" 'root: init /bad/badshebang cannot run'
case_fail init-nointerp "root=PARTLABEL=noinit rootfstype=btrfs rw init=/bad/nointerp" 'root: init /bad/nointerp cannot run'
case_fail init-nolib "root=PARTLABEL=noinit rootfstype=btrfs rw init=/bad/nolib" 'root: init /bad/nolib cannot run: .*not found'
case_fail only-ram "root=/dev/ram rw" 'root: no usable root='
# norun-ro가 먼저 돈 뒤에 돈다. 쓸 수 있으면 /run을 만들고 넘어간다
case_switch norun-rw "root=PARTLABEL=norun rootfstype=btrfs rw" /dev/vda7

if want no-root; then
	echo "경우 no-root: root=가 없으면 전과 같다"
	start no-root "$disk" "buntos.wait=3"
	if wait_for 'shells on:' 60 && wait_for 'tb375fc:/# ' 15; then ok "셸이 떴다"; else bad "셸이 뜨지 않았다"; fi
	send "sed 's/^/LOG: /' /run/buntos/boot.log; echo DONE\$((40 + 2))"
	wait_for 'DONE42' 15 || bad "셸이 답하지 않았다"
	send 'poweroff -f'
	stop
	hasnt "루트 전환 기록이 없다" 'LOG: root: |bunt-init: root: '
	has "--late가 돌았다" 'LOG: usb gadget: dummy_udc.0'
	has "nvdata를 읽기 전용으로 붙였다" 'LOG: nvdata: /dev/vda2 mounted read-only'
	has "Wi-Fi 보정값을 찾았다" 'LOG: wifi calibration: /mnt/vendor/nvdata/APCFG/APRDEB/WIFI \(2048 bytes\)'
	if [[ $(disk_hash "$disk") == "$disk_sum" ]]; then ok "GPT와 안드로이드 파티션이 그대로다"; else bad "GPT나 안드로이드 파티션이 바뀌었다"; fi
fi

if want no-root-ecm; then
	echo "경우 no-root-ecm: initramfs의 ECM, telnetd, ttyGS0 셸"
	start no-root-ecm "$disk" "buntos.wait=3 buntos.usbnet=ecm buntos.telnet=1"
	if wait_for 'shells on:' 60 && wait_for 'tb375fc:/# ' 15 && wait_for 'telnetd: 172' 15; then ok "셸과 telnetd가 떴다"; else bad "셸이나 telnetd가 뜨지 않았다"; fi
	sleep 2
	send "sed 's/^/LOG: /' /run/buntos/boot.log; grep ttyGS0 /etc/inittab | sed 's/^/TAB: /'; ls -l /proc/[0-9]*/fd/0 2>/dev/null | grep -c ttyGS0 | sed 's/^/GSHELLS: /'; echo DONE\$((40 + 2))"
	wait_for 'DONE42' 15 || bad "셸이 답하지 않았다"
	send 'poweroff -f'
	stop
	has "ECM으로 띄웠다" 'LOG: usb gadget: dummy_udc.0, acm \(shell on ttyGS0\) \+ ecm'
	has "telnetd를 띄웠다" 'LOG: telnetd: 172\.16\.42\.1:23'
	has "inittab에 ttyGS0 셸을 더했다" 'TAB: ttyGS0::respawn:/bin/sh'
	has "ttyGS0에 셸이 돈다" 'GSHELLS: [1-9]'
fi

# systemd까지 가는 경우 ---------------------------------------------------------------------------------------------
# 진짜 이미지의 루트 파티션 파일을 디스크에 쓴다. 파티션은 파일 시스템보다 1GiB 크고 뒤에 빈 자리가 있어, systemd-repart가 돌면
# GPT를 고친다. 스왑 형식의 파티션도 있어 gpt-auto 생성기가 돌면 붙인다. 기기에서 볼 수 없는 호스트 쪽은 같은 커널의 dummy_hcd로
# 대신한다: 호스트 쪽 네트워크를 netns에 넣어 DHCP와 SSH를 가젯 너머로 한다
if [[ -n $rootraw ]]; then
	echo "진짜 이미지의 루트"
	disk2="$work/disk2.img"
	raw_mib=$(($(stat -c %s "$rootraw") / 1048576 + 1))
	truncate -s $((raw_mib + 1024 + 1024 + 128))M "$disk2"
	sfdisk_script buntos $((raw_mib + 1024))MiB | sed 's/^name=buntos/name=fakeswap, size=8MiB, type=0657fd6d-a4ab-43c4-84e5-0933c84b4f4f\n&/' |
		sfdisk -q --sector-size 4096 "$disk2"
	fill_android "$disk2"
	head -c $((8 << 20)) /dev/zero >"$work/swap.bin"
	mkswap -q -p 4096 "$work/swap.bin"
	put "$disk2" 4 "$work/swap.bin"
	put "$disk2" 5 "$rootraw"
	disk2_sum=$(disk_hash "$disk2" 4)
	ssh-keygen -q -t ed25519 -N '' -C test -f "$work/key"
	cred=$(base64 -w0 "$work/key.pub")
	# 시험 도구는 둘째 디스크(tar)로 넘긴다. 첫째 디스크의 파티션 번호를 흔들지 않는다
	mkdir -p "$work/guest"
	install -m 0600 "$work/key" "$work/guest/key"
	cat >"$work/guest/run.sh" <<-'EOF'
		#!/bin/bash
		# 기기 안에서 돈다. 사실을 찍기만 한다. 판단은 test.sh가 한다. 읽기 전용 루트에서도 돌도록 /run에만 쓴다
		T=/run/guest
		echo "STATE: $(timeout "${STATE_WAIT:-150}" systemctl is-system-running --wait)"
		systemctl list-jobs --no-legend | sed 's/^/JOB: /'
		systemctl --failed --no-legend | sed 's/^/FAILED: /'
		sed 's/^/LOG: /' /run/buntos/boot.log
		findmnt -rno TARGET,SOURCE,FSTYPE,OPTIONS | sed 's/^/MNT: /'
		echo "PID1: $(cat /proc/1/comm) $(systemctl --version | head -n 1)"
		for u in systemd-repart.service systemd-repart.socket systemd-repart@x.service; do
			echo "LOAD $u: $(systemctl show -p LoadState --value "$u")"
		done
		for u in ssh.service ssh.socket buntos-usb-gadget.service buntos-usb-shell.service buntos-usb-sshd.service systemd-growfs-root.service; do
			echo "ACTIVE $u: $(systemctl is-active "$u") $(systemctl is-enabled "$u" 2>&1)"
		done
		ls -l /etc/systemd/system-generators/ /run/systemd/system-generators/ | sed 's/^/GENMASK: /'
		ls /run/systemd/generator /run/systemd/generator.late 2>/dev/null | sed 's/^/GENOUT: /'
		grep -v '^Filename' /proc/swaps | sed 's/^/SWAP: /'
		echo "FS: $(stat -f -c '%S %b' /) PART: $(blockdev --getsize64 /dev/vda5)"
		btrfs filesystem show / | sed 's/^/BTRFS: /'
		journalctl -b -u systemd-growfs-root.service --no-pager | tail -n 3 | sed 's/^/GROWFS: /'
		# 가려 두었으므로 할 일이 없어야 한다
		systemd-repart --dry-run=no 2>&1 | sed 's/^/REPART: /'
		echo "REPART-RC: ${PIPESTATUS[0]}"
		# 대조: 가리지 않았다면 파티션 표를 고친다 (시험 운전이라 쓰지 않는다)
		systemd-repart --dry-run=yes --definitions=/usr/lib/repart.d 2>&1 | sed 's/^/CONTROL: /'
		# 대조: EFI가 아니어도 생성기는 GPT를 읽어 유닛을 만든다
		mkdir -p $T/g1 $T/g2 $T/g3
		/usr/lib/systemd/system-generators/systemd-gpt-auto-generator $T/g1 $T/g2 $T/g3
		find $T/g1 $T/g2 $T/g3 -type f,l | sed 's/^/GPTAUTO: /'
		ls -l /sys/firmware/efi 2>&1 | head -n 1 | sed 's/^/EFI: /'
		# 가젯
		g=/sys/kernel/config/usb_gadget/buntos
		echo "UDC: $(cat "$g/UDC" 2>&1) PRODUCT: $(cat "$g/strings/0x409/product" 2>&1)"
		ss -ltnH | sed 's/^/LISTEN: /'
		if [ -d "$g" ]; then
			ifn=$(cat "$g/functions/ncm.0/ifname")
			ip -br addr show "$ifn" | sed 's/^/GADGET: /'
			udevadm info "/sys/class/net/$ifn" | grep -E 'DEVTYPE|NM_UNMANAGED' | sed 's/^/UDEV: /'
			nmcli -t -f DEVICE,STATE device 2>&1 | sed 's/^/NM: /'
			# 호스트 쪽 (같은 커널)
			modprobe cdc_acm
			modprobe cdc_ncm
			sleep 3
			hostif=$(ip -o link | awk -F': ' '/02:62:75:6e:74:02/ {print $2}' | cut -d@ -f1)
			echo "HOSTIF: $hostif"
			ip netns add host
			ip link set "$hostif" netns host
			ip netns exec host ip link set lo up
			ip netns exec host ip link set "$hostif" up
			cat >$T/dhcp.sh <<-'DH'
				#!/bin/sh
				if [ "$1" = bound ]; then
					echo "DHCP: $ip/$mask"
					ip addr add "$ip/$mask" dev "$interface"
				fi
			DH
			chmod 755 $T/dhcp.sh
			ip netns exec host busybox udhcpc -i "$hostif" -n -q -f -t 10 -T 2 -s $T/dhcp.sh 2>&1 | grep -E 'DHCP|lease of' | sed 's/^/HOST: /'
			chmod 600 $T/key
			ip netns exec host timeout 30 ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o BatchMode=yes -i $T/key root@172.16.42.1 \
				'echo "SSH-OK $(id -un) $(cat /proc/1/comm) $(hostname)"' 2>&1 | grep -v '^Warning' | sed 's/^/HOST: /'
			# 직렬 포트의 셸. cdc-acm은 포트를 닫으면 termios를 되돌리므로 열어 둔 채로 raw로 둔다. 그러지 않으면 호스트 쪽 줄 규율이
			# 셸의 글을 되돌려 보내 입력이 깨진다 (screen과 minicom은 스스로 raw로 둔다)
			exec 9<>/dev/ttyACM0
			stty -F /dev/ttyACM0 raw -echo 115200
			printf '\n' >&9
			sleep 1
			timeout 6 cat <&9 >$T/acm.out &
			printf 'echo "ACM-OK $(id -un) $$ $((40 + 2))"\n' >&9
			sleep 3
			printf 'exit\n' >&9
			wait
			exec 9>&-
			tr -d '\r' <$T/acm.out | sed 's/^/ACM: /'
			echo
			sleep 2
			echo "ACM-AFTER: $(systemctl is-active buntos-usb-shell.service)"
			# 케이블을 뽑았다 꽂는다. 호스트가 없는 동안 셸이 바쁘게 다시 뜨면 안 되고, 다시 꽂으면 주소가 그대로여야 한다
			udc=$(cat "$g/UDC")
			n0=$(systemctl show -p NRestarts --value buntos-usb-shell.service)
			echo disconnect >"/sys/class/udc/$udc/soft_connect"
			sleep 6
			n1=$(systemctl show -p NRestarts --value buntos-usb-shell.service)
			echo connect >"/sys/class/udc/$udc/soft_connect"
			sleep 3
			echo "UNPLUG: restarts=$((n1 - n0)) shell=$(systemctl is-active buntos-usb-shell.service) $(ip -br addr show "$ifn" | awk '{print $3}')"
			# 서비스를 다시 시작해도 있는 가젯을 다시 쓴다
			systemctl restart buntos-usb-gadget.service buntos-usb-shell.service buntos-usb-sshd.service
			sleep 1
			echo "RESTART: $(systemctl is-active buntos-usb-gadget.service buntos-usb-shell.service buntos-usb-sshd.service | tr '\n' ' ')UDC=$(cat "$g/UDC") DHCPD=$(pgrep -fc udhcpd)"
		fi
	EOF
	(cd "$work/guest" && tar -cf "$work/guest.tar" key run.sh)
	truncate -s 1M "$work/guest.tar"
	# shellcheck disable=SC2054 # 쉼표는 QEMU 인자의 일부다
	guest_drive=(-drive if=none,id=d1,file="$work/guest.tar",format=raw,readonly=on -device virtio-blk-pci,drive=d1
		-device virtio-serial-pci -chardev null,id=c1 -device virtconsole,chardev=c1)
	# 시험에서만 쓰는 값: ttyAMA0의 systemd debug shell로 들어가고, 콘솔 글이 답에 섞이지 않게 줄인다. /dev/console은 hvc0로 나눈다.
	# debug shell이 /dev/console을 쥐면 그것을 기다리는 systemd-firstboot가 끝나지 않는다
	debug_args="loglevel=3 systemd.show_status=0 systemd.debug_shell=ttyAMA0 systemd.mask=serial-getty@ttyAMA0.service"
	test_args="console=hvc0 $debug_args"
	real_args="root=PARTLABEL=buntos rootfstype=btrfs rw systemd.firstboot=no buntos.wait=10"

	# 띄우고 기기 안의 run.sh를 돌려 답을 OUT: 줄로 받는다. $3은 run.sh의 STATE_WAIT
	boot_guest() {
		local name=$1 append=$2 state_wait=${3:-150}
		start "$name" "$disk2" "$append" 2048
		if wait_for 'root@[a-z]*:|bash-[0-9.]*#' 120; then ok "셸까지 왔다"; else bad "셸까지 오지 못했다"; return 1; fi
		sleep 1
		send "mkdir -p /run/guest && tar -xf /dev/vdb -C /run/guest && STATE_WAIT=$state_wait bash /run/guest/run.sh >/run/guest/out 2>&1; sed 's/^/OUT: /' /run/guest/out; echo DONE\$((40 + 2))"
		if wait_for 'DONE42' 300; then ok "확인을 끝냈다"; else bad "확인이 끝나지 않았다"; return 1; fi
	}
	disk2_same() {
		hasnt "패닉이 없다" 'Kernel panic|Attempted to kill init'
		if [[ $(disk_hash "$disk2" 4) == "$disk2_sum" ]]; then
			ok "GPT와 안드로이드 파티션, 스왑 파티션이 그대로다"
		else
			bad "GPT나 안드로이드 파티션이 바뀌었다"
		fi
	}
	poweroff_guest() {
		send 'systemctl poweroff'
		if wait_for 'reboot: Power down' 120; then ok "깨끗이 꺼졌다"; else bad "꺼지지 않았다"; fi
		stop
		disk2_same
	}
	force_off_guest() {
		send 'sync; poweroff -f'
		stop
		disk2_same
	}
	usb_reachable() {
		has "가젯 서비스가 돈다" 'OUT: ACTIVE buntos-usb-gadget.service: active'
		has "ttyGS0 셸 서비스가 돈다" 'OUT: ACTIVE buntos-usb-shell.service: active'
		has "SSH 서비스가 돈다" 'OUT: ACTIVE buntos-usb-sshd.service: active'
		has "호스트가 DHCP로 주소를 받았다" 'OUT: HOST: DHCP: 172\.16\.42\.2/24'
		has "호스트에서 SSH로 들어갔다" 'OUT: HOST: SSH-OK root systemd '
		has "호스트에서 ttyACM의 셸이 답했다" 'ACM-OK root [0-9]+ 42'
		has "셸이 끝나면 다시 뜬다" 'OUT: ACM-AFTER: active'
		has "케이블을 뽑아도 셸이 바쁘게 다시 뜨지 않고, 다시 꽂으면 주소가 그대로다" 'OUT: UNPLUG: restarts=[0-2] shell=active 172\.16\.42\.1/24'
		has "서비스를 다시 시작해도 된다" 'OUT: RESTART: active active active UDC=dummy_udc.0 DHCPD=1'
	}

	if want systemd; then
		echo "경우 systemd: 처음 켜기, 가젯, SSH"
		boot_guest systemd "$real_args buntos.usbdebug=1 systemd.set_credential_binary=ssh.authorized_keys.root:$cred $test_args"
		has "PID 1이 systemd다" 'OUT: PID1: systemd systemd '
		has "부팅이 끝났다" 'OUT: STATE: running'
		hasnt "실패한 유닛이 없다" 'OUT: FAILED: '
		has "/run/buntos의 기록이 남았다" 'OUT: LOG: root: switching to /dev/vda5'
		has "루트가 쓰기 가능한 btrfs다" 'OUT: MNT: / /dev/vda5 btrfs rw'
		has "pstore가 붙어 있다" 'OUT: MNT: /sys/fs/pstore '
		has "configfs가 붙어 있다" 'OUT: MNT: /sys/kernel/config '
		has "systemd-repart.service를 가렸다" 'OUT: LOAD systemd-repart.service: masked'
		has "systemd-repart.socket을 가렸다" 'OUT: LOAD systemd-repart.socket: masked'
		has "systemd-repart@.service를 가렸다" 'OUT: LOAD systemd-repart@x.service: masked'
		has "손으로 돌린 systemd-repart도 할 일이 없다" 'OUT: REPART: No changes\.'
		has "손으로 돌린 systemd-repart가 성공했다" 'OUT: REPART-RC: 0'
		has "대조: 가리지 않으면 systemd-repart가 파티션을 늘린다" 'OUT: CONTROL: root-arm64 .*(→|->)'
		has "대조: EFI가 없어도 gpt-auto가 스왑 유닛을 만든다" 'OUT: GPTAUTO: .*\.swap'
		has "대조: EFI가 없어도 gpt-auto가 속성 59를 따른다" 'OUT: GPTAUTO: .*systemd-growfs-root'
		has "EFI 부팅이 아니다" 'OUT: EFI: ls: .*No such file'
		hasnt "gpt-auto 생성기가 돌지 않았다" 'OUT: GENOUT: .*(\.swap|gpt-auto)'
		hasnt "스왑을 켜지 않았다" 'OUT: SWAP: '
		has "/etc의 gpt-auto 가림" 'OUT: GENMASK: .*systemd-gpt-auto-generator -> /dev/null'
		has "ssh.service가 꺼져 있다" 'OUT: ACTIVE ssh.service: inactive disabled'
		has "ssh.socket이 꺼져 있다" 'OUT: ACTIVE ssh.socket: inactive disabled'
		has "가젯 서비스가 켜져 있다" 'OUT: ACTIVE buntos-usb-gadget.service: active enabled'
		has "파일 시스템을 늘리는 서비스가 돌았다" 'OUT: GROWFS: .*Finished systemd-growfs-root.service'
		has "가젯이 dummy_udc에 붙었다" 'OUT: UDC: dummy_udc.0 PRODUCT: TB375FC'
		has "sshd가 가젯 주소에서만 듣는다" 'OUT: LISTEN: .*172\.16\.42\.1:22'
		hasnt "다른 주소의 22번은 듣지 않는다" 'OUT: LISTEN: .*(0\.0\.0\.0|\*|\[::\]):22 '
		has "기기 주소가 붙었다" 'OUT: GADGET: .*172\.16\.42\.1/24'
		has "NetworkManager가 가젯을 맡지 않는다" 'OUT: UDEV: E: NM_UNMANAGED=1'
		has "NetworkManager가 가젯을 unmanaged로 본다" 'OUT: NM: usb0:unmanaged'
		usb_reachable
		poweroff_guest
		# 파일 시스템이 파티션만큼 늘어났는지는 기록의 값으로 본다
		read -r fs_bsize fs_blocks part_size_b < <(sed -n 's/.*OUT: FS: \([0-9]*\) \([0-9]*\) PART: \([0-9]*\).*/\1 \2 \3/p' "$log" | head -n 1)
		if ((fs_bsize * fs_blocks > part_size_b - (64 << 20))); then
			ok "파일 시스템이 파티션 끝까지 늘었다 ($((fs_bsize * fs_blocks)) / $part_size_b 바이트)"
		else
			bad "파일 시스템이 늘지 않았다 ($((fs_bsize * fs_blocks)) / $part_size_b 바이트)"
		fi
	fi

	if want systemd-again; then
		echo "경우 systemd-again: 다시 켜기"
		boot_guest systemd-again "$real_args buntos.usbdebug=1 $test_args"
		has "부팅이 끝났다" 'OUT: STATE: running'
		hasnt "실패한 유닛이 없다" 'OUT: FAILED: '
		has "명령줄 없이도 넣어 둔 키로 들어간다" 'OUT: HOST: SSH-OK root systemd '
		poweroff_guest
	fi

	if want systemd-nodebug; then
		echo "경우 systemd-nodebug: buntos.usbdebug=1이 없으면 USB 길이 돌지 않는다"
		boot_guest systemd-nodebug "$real_args $test_args"
		has "부팅이 끝났다" 'OUT: STATE: running'
		has "가젯 서비스가 돌지 않았다" 'OUT: ACTIVE buntos-usb-gadget.service: inactive enabled'
		has "셸 서비스가 돌지 않았다" 'OUT: ACTIVE buntos-usb-shell.service: inactive enabled'
		has "SSH 서비스가 돌지 않았다" 'OUT: ACTIVE buntos-usb-sshd.service: inactive enabled'
		has "UDC에 가젯이 없다" 'OUT: UDC: .*No such file'
		hasnt "22번을 듣지 않는다" 'OUT: LISTEN: .*:22 '
		poweroff_guest
	fi

	if want systemd-ro; then
		echo "경우 systemd-ro: 읽기 전용 루트 (호스트 키는 앞 경우에서 만들었다)"
		boot_guest systemd-ro "root=PARTLABEL=buntos rootfstype=btrfs ro systemd.firstboot=no buntos.wait=10 buntos.usbdebug=1 $test_args" 60
		has "루트가 읽기 전용이다" 'OUT: MNT: / /dev/vda5 btrfs ro'
		usb_reachable
		poweroff_guest
	fi

	if want systemd-stuck; then
		echo "경우 systemd-stuck: sysinit이 멈춰도 USB로 들어간다"
		# systemd.extra-unit 자격 증명으로 끝나지 않는 유닛을 sysinit 앞에 넣는다 (systemd-debug-generator)
		stuck=$(printf '[Unit]\nDefaultDependencies=no\nBefore=sysinit.target\n[Service]\nType=oneshot\nExecStart=/usr/bin/sleep infinity\nTimeoutStartSec=infinity\n' | base64 -w0)
		boot_guest systemd-stuck "$real_args buntos.usbdebug=1 systemd.set_credential_binary=systemd.extra-unit.buntos-stuck.service:$stuck systemd.wants=buntos-stuck.service systemd.set_credential_binary=ssh.authorized_keys.root:$cred $test_args" 10
		has "시험 유닛이 멈춰 있다" 'OUT: JOB: .*buntos-stuck.service +start running'
		has "sysinit이 끝나지 않았다" 'OUT: JOB: .*sysinit.target +start waiting'
		usb_reachable
		force_off_guest
	fi

	if want systemd-emergency; then
		# sysinit이 돌지 않아 자격 증명의 키가 들어가지 않는다. 앞의 systemd 경우가 넣어 둔 키를 쓴다
		echo "경우 systemd-emergency: emergency.target에서도 USB로 들어간다"
		boot_guest systemd-emergency "$real_args buntos.usbdebug=1 systemd.unit=emergency.target $test_args" 3
		has "emergency.target이다" 'OUT: STATE: maintenance'
		usb_reachable
		force_off_guest
	fi

	if want generic-root; then
		# 보드 프로필의 가림이 없는 루트(다른 보드의 이미지를 잘못 구운 때)에서도 initramfs의 가림이 듣는지 본다
		if command -v sudo >/dev/null && sudo -n true 2>/dev/null; then
			echo "경우 generic-root: 보드의 가림을 뺀 루트에서도 GPT가 그대로다"
			mkdir -p "$work/mnt"
			cp --sparse=always "$rootraw" "$work/generic.raw"
			sudo mount -o loop "$work/generic.raw" "$work/mnt"
			sudo rm -rf "$work/mnt/etc/repart.d" "$work/mnt/etc/systemd/system-generators"
			sudo rm -f "$work/mnt/etc/systemd/system/systemd-repart.service" "$work/mnt/etc/systemd/system/systemd-repart.socket" \
				"$work/mnt/etc/systemd/system/systemd-repart@.service"
			sudo umount "$work/mnt"
			put "$disk2" 5 "$work/generic.raw"
			rm -f "$work/generic.raw"
			disk2_sum=$(disk_hash "$disk2" 4)
			boot_guest generic-root "$real_args buntos.usbdebug=1 $test_args"
			has "systemd-repart.service를 가렸다(/run)" 'OUT: LOAD systemd-repart.service: masked'
			hasnt "gpt-auto 생성기가 돌지 않았다" 'OUT: GENOUT: .*(\.swap|gpt-auto)'
			hasnt "스왑을 켜지 않았다" 'OUT: SWAP: '
			poweroff_guest
		else
			echo "경우 generic-root: sudo가 없어 건너뛴다"
		fi
	fi
fi

echo "결과: 통과 $pass, 실패 $fail"
((fail == 0))
