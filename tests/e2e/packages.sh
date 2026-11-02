#!/bin/bash
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# e2e 9번 게스트 쪽 도우미: 패키지 매니저 (ARCHITECTURE 9.2, 5.5). 받은 설치 파일을 기본 앱으로 열어(파일 앱에서 두 번
# 누른 것과 같은 길) 설치하고, 실행하고, 목록 창에서 지운다. 관리자 암호는 호스트가 QMP로 친다(boot-to-shell.sh). 사람만
# 답하는 인증 창을 에이전트가 건드리지 못하기 때문이다. 인터넷이 없어도 되도록 의존성 없는 작은 .deb와 AppImage를 게스트에서
# 만든다. 직렬 콘솔의 root로 실행하며 마지막 줄이 답이다.
# 사용법: packages.sh prepare | default <deb|appimage> | open <deb|appimage> | ready <상태> | click <이름> | sheet <이름>
#         | asked <polkit 동작> | ran <deb|appimage> | deb-installed | appimage-installed | list | list-open | base-hidden
set -u

user=buntos
home=/home/$user
downloads=$home/Downloads
deb_name=bunt-e2e-hello
deb_file=$downloads/bunt-e2e-hello_1.0-1_all.deb
appimage_file=$downloads/BuntE2EApp-1.0-$(uname -m).AppImage
app=org.buntos.PackageManager

A() { sudo -u "$user" XDG_RUNTIME_DIR=/run/user/1000 bunt agent "$@"; }
# 그래픽 세션의 앱처럼 띄운다. 사용자 관리자의 범위(scope)에 넣어야 polkit이 지금 쓰는 자리의 프로세스로 본다
session() {
	sudo -u "$user" env LANG=ko_KR.UTF-8 XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1 \
		DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus \
		systemd-run --user --quiet --scope "$@"
}
file_of() {
	case "$1" in
	deb) echo "$deb_file" ;;
	appimage) echo "$appimage_file" ;;
	esac
}
ref() { grep -a -m1 -- "$1" | grep -o 'ref=e[0-9]*' | cut -d= -f2; }

case "${1:-}" in
prepare)
	sudo -u "$user" mkdir -p "$downloads"
	work=$(mktemp -d)
	# 의존성 없는 .deb: 실행하면 표시 파일을 만든다
	root=$work/deb
	mkdir -p "$root/DEBIAN" "$root/usr/bin" "$root/usr/share/applications"
	cat >"$root/DEBIAN/control" <<CONTROL
Package: $deb_name
Version: 1.0-1
Architecture: all
Maintainer: buntOS e2e <e2e@buntos.invalid>
Section: misc
Priority: optional
Description: Package manager e2e test
 A small package with one menu entry.
CONTROL
	printf '#!/bin/sh\ntouch /tmp/bunt-e2e-deb-ran\n' >"$root/usr/bin/$deb_name"
	chmod 0755 "$root/usr/bin/$deb_name"
	cat >"$root/usr/share/applications/$deb_name.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=E2E Hello
Exec=$deb_name
Icon=utilities-terminal
Terminal=false
Categories=Utility;
DESKTOP
	dpkg-deb --build --root-owner-group "$root" "$deb_file" >/dev/null
	chown "$user:" "$deb_file"
	# AppImage: 런타임 자리에 아무 ELF, 그 뒤에 squashfs (이 ELF는 파일 끝에서 섹션 표가 끝난다)
	dir=$work/appdir
	mkdir -p "$dir"
	printf '#!/bin/sh\ntouch /tmp/bunt-e2e-appimage-ran\n' >"$dir/AppRun"
	chmod 0755 "$dir/AppRun"
	cat >"$dir/bunte2eapp.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=E2E AppImage
Exec=bunte2eapp %U
Icon=bunte2eapp
Terminal=false
Categories=Utility;
X-AppImage-Version=1.0
DESKTOP
	# 1×1 PNG
	base64 -d >"$dir/bunte2eapp.png" <<<'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M/wHwAEBgIApD5fRAAAAABJRU5ErkJggg=='
	ln -s bunte2eapp.png "$dir/.DirIcon"
	mksquashfs "$dir" "$work/fs" -quiet -noappend -no-progress -comp gzip >/dev/null
	cat /usr/bin/true "$work/fs" >"$appimage_file"
	chown "$user:" "$appimage_file"
	rm -rf "$work"
	find "$downloads" -type f | wc -l
	;;
default)
	# 파일 앱이 두 번 눌렀을 때 고르는 앱: 파일의 형식과 그 형식의 기본 앱
	mime=$(session gio info --attributes=standard::content-type "$(file_of "$2")" |
		sed -n 's/^ *standard::content-type: //p')
	session gio mime "$mime" | sed -n '1s/.*: //p'
	;;
open)
	# 파일 앱에서 두 번 누른 것과 같은 길 (GIO가 형식의 기본 앱을 찾아 연다)
	session gio open "$(file_of "$2")" >/dev/null 2>&1
	echo opened
	;;
ready)
	# 패키지 매니저의 창에 그 상태가 나올 때까지 4분 기다린다 (처음에는 apt 목록을 받는다). 호스트의 guest는 인자를
	# 공백으로 다시 나누므로 글을 넘기지 않고 이름으로 고른다
	case "$2" in
	install-enabled) pattern='button "설치" \[ref=e[0-9]+\]$' ;;
	deb-title) pattern='E2E Hello 설치' ;;
	appimage-title) pattern='E2E AppImage 설치' ;;
	success) pattern='설치가 성공적으로 완료되었습니다' ;;
	list-deb) pattern='listitem "E2E Hello"' ;;
	list-appimage) pattern='listitem "E2E AppImage"' ;;
	sheet) pattern='제거하겠습니까' ;;
	empty) pattern='설치한 앱이 없습니다' ;;
	*)
		echo "unknown state: $2"
		exit 0
		;;
	esac
	for _ in $(seq 240); do
		if A snapshot --app "$app" 2>/dev/null | grep -a -q -E -- "$pattern"; then
			echo yes
			exit 0
		fi
		sleep 1
	done
	A snapshot --app "$app" 2>&1 | tail -4 | tr '\n' ' ' | cut -c1-300
	echo
	;;
click)
	# 앞에 있는 창의 단추를 이름으로 누른다. 인증 창이 뜨면 에이전트의 다음 읽기가 늦어지므로 끝나기를 기다리지 않는다
	target=$(A snapshot --app "$app" -i | ref "button \"$2\"")
	if [[ -z "$target" ]]; then
		echo "no button: $2"
		exit 0
	fi
	A click "$target" >/dev/null 2>&1 &
	echo clicked
	;;
sheet)
	# 확인 시트(모달 창)의 단추를 이름으로 누른다
	target=$(A snapshot --app "$app" -i | awk '/\[modal\]/{on=1} on' | ref "button \"$2\"")
	if [[ -z "$target" ]]; then
		echo "no sheet button: $2"
		exit 0
	fi
	A click "$target" >/dev/null 2>&1 &
	echo clicked
	;;
asked)
	# PackageKit이 관리자 인증을 물을 때까지 기다린다. 기다리는 동안 인증 창이 뜬다
	for _ in $(seq 40); do
		if journalctl -b -q --no-pager -u packagekit -n 1 | grep -a -q "trying to obtain org.freedesktop.packagekit.$2 auth"; then
			echo yes
			exit 0
		fi
		sleep 0.5
	done
	echo no
	;;
ran)
	for _ in $(seq 30); do
		if [[ -e "/tmp/bunt-e2e-$2-ran" ]]; then
			echo yes
			exit 0
		fi
		sleep 1
	done
	echo no
	;;
deb-installed)
	if dpkg-query --show --showformat='${db:Status-Status}\n' "$deb_name" 2>/dev/null | grep -q '^installed$'; then
		echo yes
	else
		echo no
	fi
	;;
appimage-installed)
	if [[ -x "$home/Applications/E2E AppImage/AppRun" && -e "$home/.local/share/applications/bunte2eapp.desktop" ]]; then
		echo yes
	else
		echo no
	fi
	;;
list)
	# 목록 창에 보이는 앱 이름 (|로 잇는다)
	A snapshot --app "$app" | grep -a 'listitem "' | sed 's/^ *- listitem "\([^"]*\)".*/\1/' | paste -sd '|'
	;;
list-open)
	A launch "$app" >/dev/null 2>&1
	echo opened
	;;
base-hidden)
	# 이미지에 든 패키지는 제거 목록의 기준에 들어 있다
	grep -c -x nautilus /usr/share/buntos/base-packages
	;;
esac
