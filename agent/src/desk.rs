// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 데몬의 메서드를 정책을 거쳐 부르는 길. 기다리기(wait.rs)와 REPL(repl/)은 호출 하나 안에서 데몬을 여러 번
//! 부른다. 그때마다 이 길로 가서, 데몬은 부를 때마다 정책을 다시 읽어 클라이언트를 받고(설정에서 끈 AI는 다음
//! 호출부터 거절된다), 화면이 잠겼는지 보고, 막은 앱과 사람만 답하는 창을 거른다. 기다리는 동안이나 JS가 도는
//! 동안에는 데몬을 붙잡지 않는다.

use serde_json::Value;

/// (JSON-RPC 오류 번호, 알림)
pub type Failure = (i64, String);

pub trait Desk {
    fn call(&self, method: &str, params: Value) -> Result<Value, Failure>;
}

impl<F> Desk for F
where
    F: Fn(&str, Value) -> Result<Value, Failure>,
{
    fn call(&self, method: &str, params: Value) -> Result<Value, Failure> {
        self(method, params)
    }
}
