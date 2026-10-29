// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 소리 장치의 음량과 경고음. 셸(메뉴 막대의 사운드, 제어 센터, 알림 소리)과 시스템 설정(사운드 페이지)이
//! 같이 쓴다.
//!
//! PipeWire의 세션 관리자(WirePlumber)에게 명령줄 도구로 묻고 바꾼다. 이 크레이트가 인자를 만들고 출력을 읽고,
//! 실행은 GLib의 하위 프로세스로 한다([`run`]).
//! - wpctl: 장치 목록(status), 음량(get-volume, set-volume), 음소거, 기본 장치
//! - pw-dump, pw-cli: 채널마다의 음량(균형). wpctl은 모든 채널을 같은 값으로 바꾸므로 균형을 둔 장치는
//!   WirePlumber의 mixer-api와 같은 길(카드의 경로 Route나 노드의 Props)로 채널 음량을 바꾼다([`mixer`])
//! - pw-play, pw-record: 경고음 재생, 입력 레벨
//! - XDG 사운드 테마: 소리 이름(message-new-instant 등)의 파일([`theme`])

pub mod mixer;
pub mod run;
pub mod theme;
pub mod wpctl;
