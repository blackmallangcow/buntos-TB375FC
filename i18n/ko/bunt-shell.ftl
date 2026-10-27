# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

menubar-logo = buntOS
# GLib DateTime 형식 문자열. 12시간제와 24시간제는 시스템 설정에서 고른다
menubar-clock-format-12h = %-m월 %-d일 (%a) %p %-I:%M
menubar-clock-format-24h = %-m월 %-d일 (%a) %H:%M
menu-about = 이 시스템에 관하여
menu-settings = 시스템 설정…
menu-restart = 다시 시작…
menu-power-off = 시스템 종료…
restart-confirm = 지금 다시 시작할까요?
power-off-confirm = 지금 시스템을 종료할까요?
dialog-cancel = 취소
dialog-restart = 다시 시작
dialog-power-off = 시스템 종료

# 메뉴바에 보이는, 앞에 있는 앱의 메뉴. $app은 앱 이름이다
app-menu-about = { $app } 정보
app-menu-preferences = 설정…
app-menu-quit = { $app } 종료
app-menu-file = 파일
app-menu-new-window = 새 창
app-menu-close = 닫기
app-menu-window = 창
app-menu-zoom = 확대/축소
app-menu-fullscreen-enter = 전체 화면 시작
app-menu-fullscreen-leave = 전체 화면 종료
app-menu-help = 도움말
app-menu-help-app = { $app } 도움말

network-wired = 유선 네트워크에 연결됨
network-wireless = Wi-Fi '{ $ssid }'에 연결됨
network-other = 네트워크에 연결됨
network-limited = 네트워크에 연결됐지만 인터넷에 닿지 않음
network-connecting = 네트워크에 연결하는 중…
network-disconnected = 네트워크에 연결되지 않음

notifications-title = 알림 센터
notifications-clear = 모두 지우기
notifications-empty = 새로운 알림 없음
notifications-now = 지금
notifications-minutes-ago = { $minutes }분 전
notifications-hours-ago = { $hours }시간 전
# 하루가 지난 알림의 날짜. GLib DateTime 형식 문자열
notifications-date-format = %-m월 %-d일

dock-terminal = 터미널
dock-apps = 앱
launcher-search = 검색
