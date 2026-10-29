# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

app-title = 로그인

# 로그인 화면 (macOS 로그인 창)
login-password-placeholder = 암호 입력
login-submit = 로그인
login-no-accounts = 로그인할 수 있는 계정이 없습니다.
login-password-for = { $name }의 암호
login-wrong-password = 암호가 올바르지 않습니다.
login-failed = 로그인하지 못했습니다: { $error }
# 시각 (GLib 날짜 형식). 큰 시각에는 오전·오후를 붙이지 않는다 (macOS 잠금 화면)
clock-date-format = %B %-d일 %A
clock-time-format-12h = %-I:%M
clock-time-format-24h = %H:%M
power-restart = 다시 시작
power-off = 시스템 종료

# 처음 켤 때의 계정 만들기 (macOS 설정 지원)
setup-title = 컴퓨터 계정 생성
setup-description =
    이 컴퓨터에 로그인하고 설정을 바꿀 때 쓸 이름과 암호를 정하십시오.
    이 계정은 이 컴퓨터의 관리자가 됩니다.
setup-full-name = 별칭
setup-account-name = 계정 이름
setup-account-name-hint = 홈 폴더의 이름이 되며 나중에 바꿀 수 없습니다.
setup-password = 암호
setup-required = 필수
setup-password-hint = { $min }자 이상. 로그인 화면과 관리자 인증(sudo 등)에 씁니다.
setup-verify = 확인
setup-verify-placeholder = 암호 확인
setup-continue = 계속
setup-creating = 계정을 만드는 중…
setup-signing-in = 로그인하는 중…
setup-preview = 미리 보기에서는 계정을 만들지 않습니다.
setup-error-full-name = 별칭을 입력하십시오. 쌍점(:)과 쉼표(,)는 쓸 수 없습니다.
setup-error-account-name = 계정 이름은 영문 소문자로 시작하고 영문 소문자, 숫자, -, _만 쓸 수 있습니다 (32자까지).
setup-error-account-taken = 이미 쓰이는 이름입니다. 다른 계정 이름을 입력하십시오.
setup-error-password-short = 암호는 { $min }자 이상이어야 합니다.
setup-error-password-invalid = 암호에는 줄바꿈을 넣을 수 없고, 너무 길어도 안 됩니다.
setup-error-password-mismatch = 암호가 일치하지 않습니다. 다시 입력하십시오.
setup-error-failed = 계정을 만들지 못했습니다: { $error }
