// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! AI가 읽는 사용 안내 (Markdown 스킬, `agent/skills/`). 데몬의 `skills` 메서드가 목록과 글을 주고, MCP의 skills
//! 도구와 `bunt agent skills`가 그것을 읽는다. `bunt agent repl --help`는 repl 안내를 바로 보인다. 4단계의 앱
//! 스킬(창, 설정, 사운드, 네트워크)도 여기에 더한다 (ARCHITECTURE 6.1).
//!
//! 안내 글은 `bunt agent --help`처럼 모델이 읽는 글이라 영어로 쓴다.

use serde_json::Value;

use crate::{
    desk::Failure,
    protocol::{self, Skill, SkillsParams},
};

/// 안내 하나
pub struct Guide {
    pub name: &'static str,
    /// 목록에 보이는 한 줄
    pub description: &'static str,
    /// 파일 그대로 (SPDX 머리 주석 포함)
    source: &'static str,
}

impl Guide {
    /// 모델에게 줄 글: 파일 앞의 SPDX 주석(<!-- … -->)을 뺀다
    pub fn content(&self) -> &'static str {
        let text = self.source;
        match text
            .strip_prefix("<!--")
            .and_then(|rest| rest.find("-->").map(|end| &rest[end + 3..]))
        {
            Some(rest) => rest.trim_start_matches('\n'),
            None => text,
        }
    }
}

pub const GUIDES: [Guide; 2] = [
    Guide {
        name: "repl",
        description: "JavaScript REPL API (Playwright-like) for reading and controlling the apps \
                      of this desktop: desktop, app.snapshot(), locators, actions, wait, errors",
        source: include_str!("../skills/repl.md"),
    },
    Guide {
        name: "desktop",
        description: "How AI control works on buntOS: what the user decides, how to read and act, \
                      app IDs",
        source: include_str!("../skills/desktop.md"),
    },
];

pub fn find(name: &str) -> Option<&'static Guide> {
    GUIDES.iter().find(|guide| guide.name == name)
}

/// `skills` 메서드: 이름이 없으면 목록, 있으면 그 글
pub fn answer(params: Value) -> Result<Value, Failure> {
    let params: SkillsParams = serde_json::from_value(params)
        .map_err(|err| (protocol::INVALID_PARAMS, err.to_string()))?;
    let skill = |guide: &Guide, content: bool| Skill {
        name: guide.name.into(),
        description: guide.description.into(),
        content: content.then(|| guide.content().into()),
    };
    let reply = match params.name.as_deref() {
        None => serde_json::json!({
            "skills": GUIDES.iter().map(|guide| skill(guide, false)).collect::<Vec<_>>()
        }),
        Some(name) => {
            let guide = find(name).ok_or_else(|| {
                let names: Vec<&str> = GUIDES.iter().map(|guide| guide.name).collect();
                (
                    protocol::INVALID_PARAMS,
                    format!("no skill named {name:?}; there are {}", names.join(", ")),
                )
            })?;
            serde_json::to_value(skill(guide, true))
                .map_err(|err| (protocol::AGENT_ERROR, err.to_string()))?
        }
    };
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn guides_are_listed_and_read_without_their_license_header() {
        let list = answer(json!({})).unwrap();
        let names: Vec<&str> = list["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|skill| skill["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["repl", "desktop"]);
        assert!(list["skills"][0].get("content").is_none());
        for guide in &GUIDES {
            let read = answer(json!({ "name": guide.name })).unwrap();
            let content = read["content"].as_str().unwrap();
            assert!(content.starts_with("# "), "{}: {content:.40}", guide.name);
            assert!(!content.contains("SPDX"), "{}", guide.name);
            // REUSE-IgnoreStart (이 줄의 글을 이 파일의 라이선스 표시로 읽지 않게)
            assert!(
                guide
                    .source
                    .contains("SPDX-License-Identifier: GPL-3.0-or-later")
            );
            // REUSE-IgnoreEnd
        }
        let missing = answer(json!({ "name": "nope" })).unwrap_err();
        assert_eq!(missing.0, protocol::INVALID_PARAMS);
        assert!(missing.1.ends_with("there are repl, desktop"));
        assert_eq!(answer(json!("x")).unwrap_err().0, protocol::INVALID_PARAMS);
    }

    #[test]
    fn the_repl_guide_matches_the_api() {
        // 안내가 말하는 이름이 prelude에 있다
        let guide = find("repl").unwrap().content();
        let prelude = include_str!("repl/prelude.js");
        for name in [
            "snapshot",
            "locator",
            "getByRole",
            "getByText",
            "fill",
            "type",
            "press",
            "scrollIntoView",
            "check",
            "inputValue",
            "textContent",
            "waitForWindow",
            "waitFor",
            "launch",
            "installed",
            "screenshot",
            "NotAllowedError",
            "BlockedAppError",
            "ScreenLockedError",
            "TimeoutError",
        ] {
            assert!(guide.contains(name), "the guide misses {name}");
            assert!(prelude.contains(name), "the API misses {name}");
        }
    }
}
