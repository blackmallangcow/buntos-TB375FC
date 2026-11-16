// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 에이전트 세션. 같은 세션의 스냅샷끼리 같은 요소에 같은 ref를 주고, 범위마다 지난 트리를
//! 기억해 다음 스냅샷의 diff를 만든다. 데몬이 떠 있는 동안 남는다.
//!
//! 사람과 AI가 한 컴퓨터를 함께 쓰므로 세션은 AI가 마지막으로 다룬 창(`Aim`)도 기억한다. ref 없는 키·글·휠은
//! 그 창이 아직 키보드 포커스를 가졌을 때만 보내고, 범위 없는 스냅샷은 사람의 포커스가 아니라 그 창을 읽는다.
//! ref는 그 앱의 프로세스(pid)에도 묶는다: 앱이 끝나거나 다시 뜨면 옛 ref가 새 앱의 요소를 가리키지 않는다.

use std::collections::HashMap;

/// AI가 마지막으로 다룬 창
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Aim {
    /// 창을 가진 프로세스
    pub pid: u32,
    /// 컴포지터 창의 제목 (셸의 레이어 표면이면 레이어 이름). 앱이 바꿀 수 있어(Firefox는 페이지 제목을 쓴다)
    /// 접근성 창이 있으면 그것으로도 찾는다
    pub title: String,
    /// 앱 이름 (알림에 쓴다)
    pub app: String,
    /// 데스크톱 ID(컴포지터 창의 앱 ID). 모르면 빈 글
    pub app_id: String,
    /// 접근성 창 (버스 이름, 경로). 트리가 없는 창이면 None
    pub window: Option<(String, String)>,
    /// 셸의 레이어 표면이다 (메뉴 막대의 열린 메뉴, 모든 앱 패널 등)
    pub layer: bool,
}

#[derive(Default)]
pub struct Session {
    /// (접근성 버스 이름, 객체 경로) → ref 번호
    numbers: HashMap<(String, String), usize>,
    /// ref 번호 - 1 → (버스, 경로)
    targets: Vec<(String, String)>,
    /// 범위 열쇠 → 지난번 트리
    last: HashMap<String, String>,
    /// 접근성 버스 이름 → ref를 줄 때 그 이름을 가졌던 프로세스
    pids: HashMap<String, u32>,
    /// AI가 마지막으로 다룬 창
    aim: Option<Aim>,
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

    /// 접근성 버스 이름의 프로세스를 적는다. 처음 적은 것이 남는다: 버스 이름은 연결 하나의 것이라, 같은 이름의
    /// pid가 달라졌다면 그 이름이 가리키던 앱은 이미 없다
    pub fn bind(&mut self, bus: &str, pid: u32) {
        self.pids.entry(bus.to_string()).or_insert(pid);
    }

    /// ref를 준 때 그 버스 이름을 가졌던 프로세스
    pub fn pid_of(&self, bus: &str) -> Option<u32> {
        self.pids.get(bus).copied()
    }

    /// AI가 마지막으로 다룬 창
    pub fn aim(&self) -> Option<&Aim> {
        self.aim.as_ref()
    }

    pub fn set_aim(&mut self, aim: Aim) {
        self.aim = Some(aim);
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

    #[test]
    fn refs_stay_with_the_process_that_had_the_bus_name() {
        let mut session = Session::default();
        assert_eq!(session.pid_of(":1.3"), None);
        session.bind(":1.3", 812);
        // 다시 읽어도 처음 적은 프로세스가 남는다: 다른 pid라면 그 이름의 앱은 이미 끝났다
        session.bind(":1.3", 900);
        assert_eq!(session.pid_of(":1.3"), Some(812));
        assert_eq!(session.pid_of(":1.4"), None);
    }

    #[test]
    fn the_last_window_the_ai_worked_on_is_kept_per_session() {
        let mut sessions = Sessions::default();
        assert!(sessions.get("default").aim().is_none());
        let editor = Aim {
            pid: 40,
            title: "Untitled".into(),
            app: "Text Editor".into(),
            app_id: "org.gnome.TextEditor".into(),
            window: Some((":1.40".into(), "/w/1".into())),
            layer: false,
        };
        sessions.get("default").set_aim(editor.clone());
        assert_eq!(sessions.get("default").aim(), Some(&editor));
        // 다른 세션(다른 REPL)은 따로다
        assert!(sessions.get("other").aim().is_none());
        let settings = Aim {
            pid: 50,
            title: "Settings".into(),
            ..editor
        };
        sessions.get("default").set_aim(settings.clone());
        assert_eq!(sessions.get("default").aim(), Some(&settings));
    }
}
