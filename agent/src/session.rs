// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 에이전트 세션. 같은 세션의 스냅샷끼리 같은 요소에 같은 ref를 주고, 범위마다 지난 트리를
//! 기억해 다음 스냅샷의 diff를 만든다. 데몬이 떠 있는 동안 남는다.

use std::collections::HashMap;

#[derive(Default)]
pub struct Session {
    /// (접근성 버스 이름, 객체 경로) → ref 번호
    numbers: HashMap<(String, String), usize>,
    /// ref 번호 - 1 → (버스, 경로)
    targets: Vec<(String, String)>,
    /// 범위 열쇠 → 지난번 트리
    last: HashMap<String, String>,
}

impl Session {
    /// 객체의 ref. 처음 보는 객체에는 다음 번호를 준다
    pub fn ref_for(&mut self, bus: &str, path: &str) -> String {
        let key = (bus.to_string(), path.to_string());
        let number = match self.numbers.get(&key) {
            Some(number) => *number,
            None => {
                self.targets.push(key.clone());
                self.numbers.insert(key, self.targets.len());
                self.targets.len()
            }
        };
        format!("e{number}")
    }

    /// ref가 가리키는 (버스, 경로)
    pub fn target(&self, reference: &str) -> Option<(&str, &str)> {
        let number: usize = reference.strip_prefix('e')?.parse().ok()?;
        let (bus, path) = self.targets.get(number.checked_sub(1)?)?;
        Some((bus, path))
    }

    /// 이번 트리를 기억하고 지난번 트리를 돌려준다
    pub fn remember(&mut self, scope: String, tree: &str) -> Option<String> {
        self.last.insert(scope, tree.to_string())
    }
}

#[derive(Default)]
pub struct Sessions {
    sessions: HashMap<String, Session>,
}

impl Sessions {
    pub fn get(&mut self, name: &str) -> &mut Session {
        self.sessions.entry(name.to_string()).or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_are_stable_within_a_session() {
        let mut session = Session::default();
        assert_eq!(session.ref_for(":1.3", "/a"), "e1");
        assert_eq!(session.ref_for(":1.3", "/b"), "e2");
        assert_eq!(session.ref_for(":1.3", "/a"), "e1");
        assert_eq!(session.ref_for(":1.9", "/a"), "e3");
        assert_eq!(session.target("e2"), Some((":1.3", "/b")));
        assert_eq!(session.target("e0"), None);
        assert_eq!(session.target("x2"), None);
    }

    #[test]
    fn remembers_the_last_tree_per_scope() {
        let mut session = Session::default();
        assert_eq!(session.remember("w".into(), "one"), None);
        assert_eq!(session.remember("w".into(), "two").as_deref(), Some("one"));
        assert_eq!(session.remember("other".into(), "x"), None);
    }
}
