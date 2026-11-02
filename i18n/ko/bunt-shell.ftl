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
notifications-expand = 더 보기
notifications-collapse = 접기
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
sound-mute = 소리 끔
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

## AI 접근 요청 창 (agent_access.rs). 허용하지 않은 AI가 이 컴퓨터를 제어하려 할 때 묻는다. macOS가 손쉬운 사용
## 권한을 묻는 경고를 따른다. $name은 AI 프로그램 이름
agent-access-title = “{ $name }”이(가) 이 컴퓨터를 제어하려고 합니다.
agent-access-message = 허용하면 이 AI가 앱의 화면 내용을 읽고, 버튼을 누르고, 글을 입력할 수 있습니다. 시스템 설정의 AI 에이전트에서 허용할 수 있습니다.
agent-access-deny = 거부
agent-access-open = 시스템 설정 열기

# ⇧⌘5 스크린샷 도구 막대와 찍은 뒤의 미리 보기 (macOS 스크린샷)
screenshot-close = 닫기
screenshot-mode-screen = 전체 화면 캡처
screenshot-mode-window = 선택한 윈도우 캡처
screenshot-mode-area = 선택 부분 캡처
screenshot-mode-record-screen = 전체 화면 기록
screenshot-mode-record-area = 선택 부분 기록
screenshot-options = 옵션
screenshot-capture = 캡처
screenshot-record = 기록
screenshot-save-to = 저장 위치
screenshot-location-desktop = 데스크탑
screenshot-location-documents = 문서
screenshot-location-pictures = 사진
screenshot-location-clipboard = 클립보드
screenshot-timer = 타이머
screenshot-timer-none = 없음
screenshot-timer-5 = 5초
screenshot-timer-10 = 10초
screenshot-show-thumbnail = 플로팅 썸네일 보기
screenshot-preview = 스크린샷 미리 보기
recording-preview = 화면 기록 미리 보기
# 화면 기록 중 메뉴 막대의 단추
recording-stop = 기록 중단

## 로고 메뉴의 세션 항목 (macOS의 Apple 메뉴)
menu-force-quit = 강제 종료…
menu-sleep = 잠자기
menu-lock-screen = 화면 잠금
# $name: 사용자의 별칭 (없으면 계정 이름)
menu-log-out = { $name } 로그아웃…
## 로그아웃 확인 창 (⇧⌘Q). 아무것도 하지 않으면 스스로 로그아웃한다
logout-title = 모든 응용 프로그램을 종료하고 지금 로그아웃하겠습니까?
# $seconds: 스스로 로그아웃하기까지 남은 초
logout-countdown = 아무것도 하지 않으면 { $seconds }초 후에 자동으로 로그아웃됩니다.
logout-confirm = 로그아웃
## 응용 프로그램 강제 종료 창 (⌥⌘⎋)
force-quit-title = 응용 프로그램 강제 종료
force-quit-message = 응용 프로그램이 응답하지 않으면 이름을 선택하고 ‘강제 종료’를 클릭하십시오.
force-quit-hint = ⌥⌘⎋를 눌러 이 윈도우를 열 수 있습니다.
force-quit-button = 강제 종료
force-quit-close = 닫기
force-quit-not-responding = (응답 없음)
force-quit-empty = 실행 중인 응용 프로그램이 없습니다.
force-quit-unavailable = 컴포지터에서 응용 프로그램 목록을 받지 못했습니다.
# $app: 앱 이름
force-quit-confirm-title = ‘{ $app }’을(를) 강제 종료하겠습니까?
force-quit-confirm-message = 저장하지 않은 변경 사항은 손실됩니다.
# $error: 까닭
force-quit-failed = 강제 종료하지 못했습니다: { $error }

## Wi-Fi 암호 창 (secret_agent.rs, NetworkManager 비밀 에이전트). macOS의 Wi-Fi 암호 창을 따른다.
## $name은 네트워크 이름(Wi-Fi는 SSID, 아니면 연결 이름), $security는 wpa2, wpa3, wep, leap
secret-wifi-title = Wi-Fi 네트워크 “{ $name }”에 { $security ->
        [wpa3] WPA3
        [wep] WEP
        [leap] LEAP
       *[wpa2] WPA2
    } 암호가 필요합니다.
secret-wifi-enterprise-title = Wi-Fi 네트워크 “{ $name }”에 WPA2 엔터프라이즈 자격 증명이 필요합니다.
secret-wired-title = 네트워크 “{ $name }”에 연결하려면 암호가 필요합니다.
secret-private-key-title = 네트워크 “{ $name }”에 연결하려면 인증서 개인 키의 암호가 필요합니다.
# NetworkManager가 틀린 암호를 다시 물을 때(REQUEST_NEW)
secret-retry = 암호가 올바르지 않습니다.
secret-user-name = 사용자 이름
secret-password = 암호
secret-private-key-password = 개인 키 암호
secret-show-password = 암호 보기
secret-cancel = 취소
secret-join = 연결
# 관리자 인증 창의 설명(auth-action-…). Wi-Fi 암호를 시스템 연결에 저장할 때 NetworkManager가 묻는다
auth-action-org-freedesktop-NetworkManager-settings-modify-system = 모든 사용자의 네트워크 설정을 바꾸려고 합니다.
# 관리자 인증 창의 설명(auth-action-…). 패키지 매니저와 앱 스토어가 PackageKit으로 설치하고 지울 때 묻는다
auth-action-org-freedesktop-packagekit-package-install = 새로운 소프트웨어를 설치하려고 합니다.
auth-action-org-freedesktop-packagekit-package-install-untrusted = 서명되지 않은 새로운 소프트웨어를 설치하려고 합니다.
auth-action-org-freedesktop-packagekit-package-reinstall = 소프트웨어를 다시 설치하려고 합니다.
auth-action-org-freedesktop-packagekit-package-downgrade = 소프트웨어를 이전 판으로 바꾸려고 합니다.
auth-action-org-freedesktop-packagekit-package-remove = 소프트웨어를 제거하려고 합니다.
auth-action-org-freedesktop-packagekit-system-update = 소프트웨어를 업데이트하려고 합니다.
auth-action-org-freedesktop-packagekit-system-sources-configure = 소프트웨어 저장소 설정을 바꾸려고 합니다.
auth-action-org-freedesktop-Flatpak-configure-remote = 소프트웨어 저장소 설정을 바꾸려고 합니다.
auth-action-org-freedesktop-Flatpak-app-install = 새로운 앱을 설치하려고 합니다.
auth-action-org-freedesktop-Flatpak-runtime-install = 앱에 필요한 런타임을 설치하려고 합니다.
auth-action-org-freedesktop-Flatpak-app-uninstall = 앱을 제거하려고 합니다.
auth-action-org-freedesktop-Flatpak-runtime-uninstall = 런타임을 제거하려고 합니다.

## Bluetooth 페어링 창 (pairing_agent.rs, BlueZ 페어링 에이전트). macOS의 페어링 요청을 따른다. $name은 기기 이름
pairing-title = Bluetooth 페어링 요청
pairing-confirm = “{ $name }”에 같은 코드가 보이는지 확인하십시오.
pairing-display = “{ $name }”에서 다음 코드를 입력한 다음 Return 또는 Enter 키를 누르십시오.
pairing-pin = “{ $name }”의 PIN 코드를 입력하십시오. 기기 설명서에 적혀 있습니다(0000이나 1234가 흔합니다).
pairing-passkey = “{ $name }”에 보이는 6자리 코드를 입력하십시오.
pairing-authorize = “{ $name }”이(가) 이 컴퓨터와 페어링하려고 합니다.
pairing-pin-placeholder = PIN 코드
pairing-passkey-placeholder = 코드
pairing-cancel = 취소
pairing-connect = 연결
pairing-deny = 거부
pairing-allow = 허용
# 믿는 기기가 아닌 기기가 서비스에 연결하려 할 때. $service는 서비스 이름(아래 pairing-service-…, 모르면 UUID)
pairing-service-title = Bluetooth 연결 요청
pairing-service = “{ $name }”이(가) 이 컴퓨터의 { $service } 서비스에 연결하려고 합니다.
pairing-service-audio = 오디오
pairing-service-handsfree = 핸즈프리
pairing-service-input = 키보드·마우스
pairing-service-files = 파일 전송
pairing-service-network = 네트워크
pairing-service-serial = 직렬 포트
# 메뉴 막대 Bluetooth 메뉴의 근처 기기 (메뉴가 열린 동안 찾고, 누르면 짝짓는다)
bluetooth-nearby = 근처 기기

## 사운드 메뉴와 알림 (시스템 설정의 사운드·알림 페이지)
sound-settings = 사운드 설정…
# 미리 보기 보기가 "안 함"일 때 알림 카드의 제목 자리에 보인다 (macOS와 같다)
notifications-preview-hidden = 알림

# 데스크탑 항목 (macOS 한국어판의 Finder 데스크탑 용어)
desktop-title = 데스크탑
desktop-open = 열기
desktop-rename = 이름 변경
desktop-move-to-trash = 휴지통으로 이동
desktop-new-folder = 새로운 폴더
desktop-clean-up-by = 정리 기준
desktop-sort-name = 이름
desktop-change-wallpaper = 배경화면 변경…
desktop-untitled-folder = 무제 폴더
