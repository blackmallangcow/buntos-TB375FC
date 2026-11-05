#!/bin/sh
# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# 꾸러미를 올리는 스크립트가 세션과 네트워크가 쓰는 서비스를 다시 시작하지 못하게 한다. 쓰는 중에 화면이
# 꺼지거나 Wi-Fi가 끊겨 apt까지 함께 죽는 일을 막는다. 새 판은 다음 부팅부터 돈다 (apt-guard post가 알린다).
#
# deb-systemd-invoke와 invoke-rc.d가 부른다: policy-rc.d <서비스> <동작>. 0은 해도 된다, 101은 하지 말라는 뜻이다.
[ "${1:-}" = --quiet ] && shift

case "${1:-}" in
greetd* | display-manager* | NetworkManager* | network-manager | wpa_supplicant* | wpasupplicant | \
	dbus* | systemd-logind* | polkit* | upower* | accounts-daemon* | accountsservice) ;;
*) exit 0 ;;
esac

case "${2:-}" in
restart | try-restart | stop | force-stop | force-reload) ;;
*) exit 0 ;;
esac

mkdir -p /run/buntos-apt-guard 2>/dev/null && echo "$1" >>/run/buntos-apt-guard/not-restarted 2>/dev/null
exit 101
