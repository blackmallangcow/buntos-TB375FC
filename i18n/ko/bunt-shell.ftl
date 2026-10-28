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
launcher-search = 응용 프로그램 검색
launcher-empty = 일치하는 앱이 없습니다
launcher-category-utilities = 유틸리티
launcher-category-developer = 개발자 도구
launcher-category-productivity = 생산성
launcher-category-creativity = 창의력
launcher-category-internet = 인터넷
launcher-category-entertainment = 엔터테인먼트
launcher-category-games = 게임
launcher-category-education = 교육 및 참고
launcher-category-other = 기타

## 메뉴 막대 항목 (Wi-Fi, 배터리, 사운드, Bluetooth, 입력 소스)

network-title = 네트워크
network-settings = 네트워크 설정…
wifi-title = Wi-Fi
wifi-known = 알고 있는 네트워크
wifi-other = 기타 네트워크
wifi-none = 찾은 네트워크 없음
wifi-settings = Wi-Fi 설정…
power-title = 전원
power-battery = 배터리
power-battery-percent = 배터리 { $percent }%
power-adapter = 전원 어댑터 사용 중
power-source-adapter = 전원: 전원 어댑터
power-source-battery = 전원: 배터리
power-fully-charged = 완전히 충전됨
power-until-full = 완전히 충전될 때까지 { $time }
power-remaining = 남은 시간 { $time }
power-duration = { $hours ->
    [0] { $minutes }분
   *[other] { $hours }시간 { $minutes }분
}
power-mode = 전원 모드
power-mode-low = 저전력 모드
power-mode-automatic = 자동
power-mode-high = 고성능
power-settings = 배터리 설정…
sound-title = 사운드
sound-output = 출력
sound-volume = 음량
sound-mute = 음소거
bluetooth-title = Bluetooth
bluetooth-devices = 기기
bluetooth-no-devices = 연결한 기기 없음
bluetooth-connected = 연결됨
bluetooth-settings = Bluetooth 설정…
input-source-title = 입력 소스
input-source-abc = ABC
input-source-hangul = 2벌식
input-source-shortcut = Ctrl+Space 또는 한/영 키로 전환
input-source-settings = 키보드 설정…

## 제어 센터

cc-title = 제어 센터
cc-on = 켬
cc-off = 끔
cc-wired = 이더넷
cc-unavailable = 사용할 수 없음
cc-focus = 방해 금지
cc-dark-mode = 다크 모드
cc-low-power = 저전력 모드
cc-display = 디스플레이
cc-brightness = 밝기
cc-edit = 메뉴 막대 설정…

## 관리자 인증 창 (polkit 인증 에이전트). 제목은 인증을 요청한 앱 이름이고, 앱을 모르면 auth-title이다
auth-title = 인증 필요
auth-hint = 허용하려면 암호를 입력하십시오.
auth-hint-admin = 허용하려면 관리자 이름과 암호를 입력하십시오.
auth-user-name = 사용자 이름
auth-password = 암호
auth-cancel = 취소
auth-confirm = 확인
# 동작마다의 설명 (auth-action-<polkit 동작 ID의 .을 -로>). polkit이 보낸 설명 대신 쓴다. 없으면 polkit의 설명을
# 쓴다. 제목이 요청한 앱 이름이라 주어 없이 쓴다. $app은 앱 이름(모르면 auth-someone), $program은 pkexec가 실행할 프로그램
auth-someone = 프로그램
auth-action-org-buntos-agent-access-change = 이 컴퓨터를 제어할 수 있는 AI를 바꾸려고 합니다.
auth-action-org-freedesktop-timedate1-set-timezone = 시간대를 바꾸려고 합니다.
auth-action-org-freedesktop-timedate1-set-time = 시스템 시간을 바꾸려고 합니다.
auth-action-org-freedesktop-timedate1-set-ntp = 날짜와 시간을 자동으로 맞출지 바꾸려고 합니다.
auth-action-org-freedesktop-timedate1-set-local-rtc = 하드웨어 시계의 시간대를 바꾸려고 합니다.
auth-action-org-freedesktop-hostname1-set-static-hostname = 기기 이름을 바꾸려고 합니다.
auth-action-org-freedesktop-hostname1-set-hostname = 기기 이름을 바꾸려고 합니다.
auth-action-org-freedesktop-hostname1-set-machine-info = 기기 정보를 바꾸려고 합니다.
auth-action-org-freedesktop-policykit-exec = { $program } 프로그램을 관리자 권한으로 실행하려고 합니다.
