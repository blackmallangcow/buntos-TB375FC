# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later
#
# tty1에 로그인한 buntos 사용자는 그래픽 세션으로 들어간다.
# 화면 장치가 없으면 세션을 열지 않고, 세션이 끝나면 tty1의 셸로 돌아온다
# (로그인 셸까지 끝내면 getty가 재시작을 반복하다 실패 상태가 된다).

if [ "$(id -un)" = buntos ] && [ "$(tty)" = /dev/tty1 ] && [ -z "$WAYLAND_DISPLAY" ] \
	&& command -v bunt-comp >/dev/null 2>&1 && ls /dev/dri/card* >/dev/null 2>&1; then
	/usr/libexec/buntos/session
fi
