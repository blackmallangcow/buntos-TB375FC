// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! buntOS 에이전트 제어 계층 (Agent Control Plane, ARCHITECTURE 6장). AI 에이전트가 화면 픽셀
//! 대신 앱의 접근성 트리로 데스크톱을 읽게 한다. Aside 브라우저의 snapshot()처럼 누를 수 있는
//! 요소에 ref를 달고, 같은 범위를 다시 읽으면 바뀐 줄(diff)도 돌려준다.
//!
//! - `buntd-agent`: 사용자 서비스. 접근성 버스를 읽고 세션(ref, 지난 트리)을 지닌다
//! - `bunt agent`: 데몬에 JSON-RPC로 묻는 CLI

pub mod atspi;
pub mod diff;
pub mod protocol;
pub mod session;
pub mod tree;
