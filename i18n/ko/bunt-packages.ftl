# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

## 패키지 매니저 (bunt-packages)
app-name = 패키지 매니저

## 형식과 설치되는 곳
kind-deb = Debian 패키지
kind-appimage = AppImage
kind-flatpak = Flatpak 앱
kind-flatpak-repo = Flatpak 저장소
location-system = 시스템 전체 (관리자 암호가 필요합니다)
location-user = 이 사용자의 응용 프로그램 폴더 (~/Applications)
location-flatpak = Flatpak (앱마다 샌드박스에서 실행됩니다)
location-repo = 소프트웨어 저장소 목록

## 설치 창: 왼쪽 단계는 macOS 설치 프로그램과 같다
installer-step-introduction = 소개
installer-step-install = 설치
installer-step-summary = 요약
installer-title-reading = 설치 파일을 읽는 중
installer-title-install = { $name } 설치
installer-title-installed = { $name }이(가) 이미 설치되어 있습니다
installer-title-cannot = 설치할 수 없습니다
installer-title-installing = 설치하는 중
installer-title-removing = 제거하는 중
installer-title-summary = 요약
installer-version = 버전 { $version }
installer-version-newer = 버전 { $version } (이 파일보다 새로움)
installer-fact-size = 설치 크기
installer-fact-maintainer = 만든 곳
installer-fact-homepage = 홈페이지
installer-fact-source = 출처
installer-fact-file = 파일
installer-fact-location = 설치 위치
installer-fact-installed = 설치된 버전
installer-fact-also-install = 함께 설치
installer-fact-also-update = 함께 업데이트
installer-fact-also-remove = 함께 제거
installer-count-names = { $count }개: { $names }
installer-trust-deb = 이 파일은 buntOS 저장소에서 온 것이 아닙니다. 믿을 수 있는 곳에서 받은 파일만 설치하십시오. 설치할 때 패키지의 스크립트가 관리자 권한으로 실행됩니다.
installer-trust-appimage = 믿을 수 있는 곳에서 받은 파일만 설치하십시오. 이 앱은 샌드박스 없이 이 사용자의 권한으로 실행됩니다.
installer-trust-flatpak = 믿을 수 있는 곳에서 받은 파일만 설치하십시오. 앱은 샌드박스에서 실행됩니다.
installer-trust-repo = 이 저장소의 앱을 앱 스토어에서 찾아 설치할 수 있게 됩니다. 믿을 수 있는 저장소만 더하십시오.
installer-cancel = 취소
installer-install = 설치
installer-reinstall = 다시 설치
installer-update = 업데이트
installer-downgrade = 이 버전으로 바꾸기
installer-add-repo = 저장소 추가
installer-remove = 제거…
installer-close = 닫기
installer-open = 열기
installer-status-reading = 설치 파일을 읽는 중…
installer-status-refresh = 패키지 목록을 받는 중…
installer-status-refresh-percent = 패키지 목록을 받는 중… { $percent }%
installer-status-dependencies = 필요한 패키지를 확인하는 중…
installer-status-preparing = 설치를 준비하는 중…
installer-status-auth = 관리자 인증을 기다리는 중…
installer-status-lock = 다른 설치가 끝나기를 기다리는 중…
installer-status-download = 필요한 패키지를 내려받는 중…
installer-status-check = 패키지를 확인하는 중…
installer-status-installing = 설치하는 중…
installer-status-removing = 제거하는 중…
installer-status-cleanup = 정리하는 중…
installer-success = 설치가 성공적으로 완료되었습니다.
installer-success-details = 소프트웨어가 설치되었습니다.
installer-repo-added = 저장소를 추가했습니다.
installer-repo-added-details = 앱 스토어에서 이 저장소의 앱을 찾을 수 있습니다.
installer-repo-exists = 이 저장소는 이미 추가되어 있습니다.
installer-removed = 제거했습니다.
installer-failed = 설치하지 못했습니다.
installer-failed-details = 오류: { $error }
installer-move-to-trash = 설치 파일을 휴지통으로 이동
installer-moved-to-trash = 휴지통으로 이동했습니다
installer-not-authorized = 관리자 인증을 받지 못해 설치하지 않았습니다. 다시 시도하려면 단추를 누르십시오.
installer-not-local = 이 컴퓨터에 있는 파일만 설치할 수 있습니다. 파일을 내려받은 뒤 다시 여십시오.
installer-appimage-format = 이 AppImage는 buntOS가 풀 수 없는 형식(1형이나 DwarFS)입니다. 만든 곳에서 새 버전이나 .deb, Flatpak 버전을 찾아보십시오.
installer-rpm = RPM 패키지는 Fedora 계열의 형식이라 buntOS에 설치할 수 없습니다. 만든 곳에서 .deb, Flatpak, AppImage 버전을 찾아보십시오.
installer-snap = Snap 패키지는 buntOS에 설치할 수 없습니다. 만든 곳에서 .deb, Flatpak, AppImage 버전을 찾아보십시오.
installer-unknown = 패키지 매니저가 아는 설치 파일이 아닙니다. .deb, AppImage, .flatpak, .flatpakref, .flatpakrepo 파일을 열 수 있습니다.
installer-read-failed = 설치 파일을 읽지 못했습니다. 파일이 손상되었을 수 있습니다. ({ $error })
installer-wrong-arch = 이 파일은 { $arch }용이고 이 컴퓨터는 { $host }입니다. 만든 곳에서 { $host }용 파일을 받으십시오.

## 실패한 까닭
failure-network = 필요한 패키지를 내려받지 못했습니다. 인터넷 연결을 확인하십시오.
failure-dependencies = 이 패키지에 필요한 다른 패키지를 찾지 못했거나 서로 맞지 않아 설치할 수 없습니다.
failure-lock = 다른 프로그램이 패키지를 설치하고 있습니다. 끝난 뒤 다시 시도하십시오.
failure-space = 디스크 공간이 모자랍니다.
failure-conflict = 이미 설치된 패키지와 충돌합니다.
failure-architecture = 이 컴퓨터의 아키텍처에 맞지 않는 패키지입니다.
failure-corrupt = 패키지 파일이 손상되었습니다. 다시 내려받으십시오.
failure-system-package = 시스템에 꼭 필요한 패키지라 제거할 수 없습니다.
failure-service = 패키지 서비스(PackageKit)를 찾지 못했습니다.

## 제거
remove-question = '{ $name }'을(를) 제거하겠습니까?
remove-details-deb = 앱과 앱 때문에 설치된 패키지를 지웁니다. 사용자의 문서와 설정은 남습니다.
remove-details-flatpak = 앱과 이제 쓰지 않는 런타임을 지웁니다. 앱의 자료(~/.var/app)는 남습니다.
remove-details-appimage = 응용 프로그램 폴더의 앱을 지웁니다. 사용자의 문서와 설정은 남습니다.
remove-also = 함께 제거되는 패키지: { $names }
remove-blocked = 시스템에 꼭 필요한 패키지가 함께 지워지므로 제거할 수 없습니다: { $names }
remove-cancel = 취소
remove-confirm = 제거

## 목록 창
manager-open-file = 설치 파일 열기…
manager-store = 앱 스토어
manager-search = 찾기
manager-file-filter = 설치 파일
manager-footnote = 직접 설치한 앱과 패키지만 보입니다. buntOS에 들어 있는 앱은 제거할 수 없습니다.
manager-empty-title = 설치한 앱이 없습니다
manager-empty-details = 받은 설치 파일(.deb, AppImage, Flatpak)을 두 번 클릭하거나 이 창으로 끌어다 놓으면 설치할 수 있습니다.
manager-no-match = 일치하는 앱이 없습니다
manager-row-open = 열기
manager-row-remove = 제거…
manager-row-removing = 제거하는 중…
manager-removed = '{ $name }'을(를) 제거했습니다.
