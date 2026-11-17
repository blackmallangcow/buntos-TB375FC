# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

## 키보드 단축키 시트 (settings/src/pages/keyboard/shortcuts.rs). macOS 26의 키보드 단축키 시트를 따른다

# 키보드 페이지의 단축키 묶음
keyboard-shortcuts-button = 키보드 단축키…
# 입력 소스 바꾸기 행의 값. $shortcut: 컴포지터에 정한 단축키 (예: ⌃스페이스)
keyboard-switch-keys-with = { $shortcut }, 한/영 키
keyboard-switch-keys-hangul = 한/영 키

# 시트
shortcuts-title = 키보드 단축키
shortcuts-restore = 기본값으로 복원
shortcuts-none = 없음
# 기록 칸에서 키를 기다릴 때
shortcuts-recording = 단축키 입력
# $name: 그 키를 쓰는 다른 단축키의 이름
shortcuts-in-use = ‘{ $name }’에서 이미 쓰는 단축키입니다.
shortcuts-needs-modifier = ⌃, ⌥, ⌘ 가운데 하나를 함께 누르십시오. 기능 키(F1 등)는 혼자 써도 됩니다.
shortcuts-no-shift = 텍스트 편집 단축키에는 ⇧를 쓸 수 없습니다. ⇧를 함께 누르면 선택 영역을 넓힙니다.
# 조합 키만 눌렀다 뗐을 때 (입력 소스 전환만 조합 키만으로 된다)
shortcuts-needs-key = 조합 키만으로는 이 단축키를 정할 수 없습니다. 다른 키도 함께 누르십시오.
shortcuts-error = 단축키를 바꾸지 못했습니다: { $error }
shortcuts-unavailable-title = 키보드 단축키를 사용할 수 없음
shortcuts-unavailable-description = 컴포지터(bunt-comp)가 키보드 단축키 API를 제공하지 않습니다.
# 단축키 기호 안의 키 이름
shortcuts-key-space = 스페이스
shortcuts-key-hangul = 한/영
# 분류 목록 아래: 기호가 PC 키보드의 어느 키인지
shortcuts-legend-control = Ctrl 키
shortcuts-legend-option = Alt 키
shortcuts-legend-shift = Shift 키
shortcuts-legend-command = Super(Windows) 키

# 분류 (컴포지터의 분류 ID)
shortcut-category-launcher = 모든 앱 및 Dock
shortcut-category-windows = 앱 및 윈도우
shortcut-category-input = 입력 소스
shortcut-category-screenshots = 스크린샷
shortcut-category-text = 텍스트 편집
shortcut-category-app = 앱 단축키
# 분류 목록 아래 설명
shortcuts-note-input = ⌃⇧처럼 조합 키만 눌렀다 떼거나 ⇧스페이스로도 정할 수 있습니다. 한/영 키로도 입력 소스를 바꾸며, 한/영 키는 바꿀 수 없습니다. Caps Lock 키로 전환을 켜면 짧게 누를 때 입력 소스를 바꾸고, 길게 누르면 Caps Lock이 켜집니다.
shortcuts-note-text = 글 입력 칸에 입력하는 동안 동작합니다. ⇧를 함께 누르면 선택 영역을 넓힙니다.
shortcuts-note-app = 앞에 있는 앱에 그 앱의 Ctrl 단축키로 보냅니다. 터미널에는 복사하기, 붙여넣기 등을 터미널의 단축키(Ctrl+Shift+C…)로 보냅니다.

# 단축키. 메시지 ID는 컴포지터의 단축키 ID에서 .을 -로 바꾼 것이다 (app.copy → shortcut-app-copy)
shortcut-launcher-show = 모든 앱 보기
shortcut-launcher-dock-autohide = Dock 가리기 켜기/끄기
shortcut-windows-next-app = 다음 앱으로 전환
shortcut-windows-previous-app = 이전 앱으로 전환
shortcut-windows-next-window = 다음 윈도우 활성화
shortcut-windows-previous-window = 이전 윈도우 활성화
shortcut-windows-quit-app = 앱 종료
shortcut-windows-minimize = 윈도우 최소화
shortcut-windows-hide-app = 앱 가리기
shortcut-windows-hide-others = 기타 가리기
shortcut-windows-fullscreen = 전체 화면 전환
shortcut-windows-close = 윈도우 닫기
shortcut-windows-terminal = 터미널 열기
shortcut-input-switch-source = 입력 소스 전환
shortcut-input-caps-lock = Caps Lock 키로 입력 소스 전환
shortcut-screenshot-screen = 화면을 파일로 저장
shortcut-screenshot-screen-clipboard = 화면 사진을 클립보드에 복사
shortcut-screenshot-area = 선택한 영역을 파일로 저장
shortcut-screenshot-area-clipboard = 선택한 영역 사진을 클립보드에 복사
shortcut-screenshot-toolbar = 스크린샷 및 기록 옵션
shortcut-text-line-start = 줄의 시작으로 이동
shortcut-text-line-end = 줄의 끝으로 이동
shortcut-text-document-start = 문서의 시작으로 이동
shortcut-text-document-end = 문서의 끝으로 이동
shortcut-text-word-left = 이전 단어로 이동
shortcut-text-word-right = 다음 단어로 이동
shortcut-text-paragraph-up = 이전 단락으로 이동
shortcut-text-paragraph-down = 다음 단락으로 이동
shortcut-text-delete-word = 왼쪽 단어 삭제
shortcut-text-delete-word-forward = 오른쪽 단어 삭제
shortcut-text-delete-line = 줄의 시작까지 삭제
shortcut-app-cut = 오려두기
shortcut-app-copy = 복사하기
shortcut-app-paste = 붙여넣기
shortcut-app-undo = 실행 취소
shortcut-app-redo = 실행 복귀
shortcut-app-select-all = 전체 선택
shortcut-app-find = 찾기
shortcut-app-find-next = 다음 찾기
shortcut-app-find-previous = 이전 찾기
shortcut-app-new = 새로 만들기
shortcut-app-new-tab = 새로운 탭
shortcut-app-open = 열기
shortcut-app-save = 저장
shortcut-app-save-as = 별도 저장
shortcut-app-print = 프린트
shortcut-app-close = 탭 또는 윈도우 닫기
shortcut-app-settings = 앱 설정
shortcut-app-back = 뒤로
shortcut-app-forward = 앞으로
shortcut-app-reload = 새로 고침
shortcut-app-zoom-in = 확대
shortcut-app-zoom-out = 축소
shortcut-app-zoom-reset = 실제 크기
shortcut-app-other-command-keys = 그 밖의 ⌘ 단축키를 앱의 Ctrl 단축키로 보내기
# 화면 잠금, 로그아웃, 강제 종료 (macOS의 Apple 메뉴 단축키)
shortcut-category-session = 화면 잠금 및 로그아웃
shortcut-windows-force-quit = 응용 프로그램 강제 종료
shortcut-session-lock-screen = 화면 잠금
shortcut-session-log-out = 로그아웃
shortcut-session-log-out-now = 확인 없이 로그아웃
# AI 제어 (컴포지터의 agent.pause. 메뉴 막대 AI 표시의 제어 창과 같은 말)
shortcut-category-agent = AI 에이전트
shortcut-agent-pause = AI 제어 멈추기 또는 다시 허용
