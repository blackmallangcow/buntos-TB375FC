# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# 스크린샷(⇧⌘3, ⇧⌘4) 파일 이름. macOS처럼 "스크린샷 2026-10-29 오전 9.52.13"
screenshot-file = 스크린샷 { $date } { $period } { $time }
screenshot-am = 오전
screenshot-pm = 오후
# 스크린샷 알림
screenshot-title = 스크린샷
screenshot-saved = 사진 폴더에 저장했습니다: { $name }
screenshot-copied = 클립보드에 복사했습니다.
screenshot-failed = 스크린샷을 찍지 못했습니다.
# 로그아웃할 때 정한 시간 안에 닫히지 않은 윈도우가 있어 그만뒀을 때의 알림. $app: 그 윈도우의 제목
logout-cancelled-title = 로그아웃 취소됨
logout-cancelled-body = ‘{ $app }’이(가) 닫히지 않아 로그아웃을 중단했습니다. 앱을 종료한 다음 다시 로그아웃하십시오.
