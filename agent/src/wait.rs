// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 기다리기 (`wait`): 요소가 나타나거나 사라질 때, 창이 뜨거나 닫힐 때, 요소의 값이 바뀔 때까지 시간 상한을
//! 두고 범위를 다시 읽는다. Playwright의 `locator.waitFor`, `waitForEvent('popup')`,
//! `expect(locator).toHaveValue`를 하나로 묶었다. 시간 상한을 0으로 주면 한 번만 보는 찾기가 된다(REPL의
//! getByRole·getByText가 ref를 얻을 때).
//!
//! 다시 읽을 때마다 데몬에 따로 요청한다([`Desk`]): 기다리는 동안 데몬을 붙잡지 않고, 요청마다 정책을 다시
//! 보므로 그 사이에 화면이 잠기거나 사용자가 AI를 끄면 바로 거절된다. 다시 읽는 스냅샷은 `peek`이라 다음
//! 스냅샷의 diff를 흐리지 않는다.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{
    desk::{Desk, Failure},
    outline::{self, Line, Matcher},
    protocol::{self, Found, WaitParams, Waited},
};

/// 시간 상한의 기본값과 최댓값
pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;
pub const MAX_TIMEOUT_MS: u64 = 120_000;
/// 다시 읽는 간격. 스냅샷 한 번은 수 ms다(ARCHITECTURE 8.3)
const POLL: Duration = Duration::from_millis(100);
/// 답에 싣는 맞는 요소 수 (REPL의 all(), strict 검사와 안내에 쓴다. all()이 쓰도록 10에서 늘렸다)
const MATCHES_KEPT: usize = 50;
/// 시간이 다 됐을 때 알리는 창 제목 수
const WINDOWS_SHOWN: usize = 5;

/// 기다릴 것
#[derive(Debug, Clone, PartialEq)]
enum Condition {
    /// role·name·text에 맞는 요소
    Element(Matcher),
    /// ref 요소 자체
    Itself,
    /// 제목에 이 글이 든 창 (빈 글이면 새로 뜬 창)
    Window(String),
    /// ref 요소의 값이 이것이 된다
    Value(String),
    /// ref 요소의 값이 처음 본 것과 달라진다
    Changed,
}

/// 범위를 한 번 읽은 것
enum Seen {
    Lines(Vec<Line>),
    Windows(Vec<String>),
    /// 읽지 못했다 (앱이 아직 없거나 끝났다). 사라지기를 기다렸으면 사라진 것이다
    Missing(String),
}

/// 처음 본 것: 새로 뜬 창은 처음 본 창 목록과, 바뀐 값은 처음 본 값과 견준다
enum Baseline {
    Windows(Vec<String>),
    Value(Option<String>),
}

/// 한 번 본 결과: 기다리던 것이 일어났으면 그 답
enum Outcome {
    Found {
        found: Found,
        matches: Vec<Found>,
        count: usize,
    },
    Gone,
    Window(String),
}

fn invalid(message: impl Into<String>) -> Failure {
    (protocol::INVALID_PARAMS, message.into())
}

fn condition(params: &WaitParams) -> Result<Condition, Failure> {
    let matcher = Matcher {
        role: params.role.clone(),
        name: params.name.clone(),
        text: params.text.clone(),
        exact: params.exact,
    };
    let valued = params.value.is_some() || params.changed;
    if let Some(window) = &params.window {
        if !matcher.is_empty() || valued || params.scope.is_some() {
            return Err(invalid(
                "window goes alone (with app): wait for a window or for an element, not both",
            ));
        }
        if window.is_empty() && params.gone {
            return Err(invalid("gone needs the title of the window to close"));
        }
        return Ok(Condition::Window(window.clone()));
    }
    if valued {
        if params.scope.is_none() {
            return Err(invalid(
                "value and changed need a ref: they watch one element",
            ));
        }
        if !matcher.is_empty() || params.gone || (params.value.is_some() && params.changed) {
            return Err(invalid(
                "value and changed go alone with a ref (not with role, name, text, gone or each other)",
            ));
        }
        return Ok(match &params.value {
            Some(value) => Condition::Value(value.clone()),
            None => Condition::Changed,
        });
    }
    if params.nth.is_some() && matcher.is_empty() {
        return Err(invalid("nth goes with role, name or text"));
    }
    if !matcher.is_empty() {
        return Ok(Condition::Element(matcher));
    }
    if params.scope.is_some() {
        return Ok(Condition::Itself);
    }
    Err(invalid(
        "wait needs something to wait for: text, role and name, window, a ref, or value or changed \
         with a ref",
    ))
}

/// 기다리는 시간의 상한: 없으면 DEFAULT_TIMEOUT_MS, 많아야 MAX_TIMEOUT_MS
fn limit(params: &WaitParams) -> Duration {
    Duration::from_millis(
        params
            .timeout_ms
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(MAX_TIMEOUT_MS),
    )
}

/// 조건이 맞을 때까지 기다린다. 시간이 다 되면 TIMEOUT으로, 무엇을 기다렸고 지금 어떤지 알린다
pub fn run(desk: &dyn Desk, params: &WaitParams) -> Result<Waited, Failure> {
    let condition = condition(params)?;
    let limit = limit(params);
    let started = Instant::now();
    let mut baseline: Option<Baseline> = None;
    loop {
        let seen = observe(desk, params, &condition)?;
        if let Some(outcome) = check(&condition, params, &seen, &mut baseline) {
            return Ok(answer(
                &condition,
                params,
                outcome,
                started.elapsed().as_millis() as u64,
            ));
        }
        let elapsed = started.elapsed();
        if elapsed >= limit {
            return Err((
                protocol::TIMEOUT,
                timed_out(&condition, params, &seen, limit),
            ));
        }
        std::thread::sleep(POLL.min(limit - elapsed));
    }
}

/// 범위를 한 번 읽는다. 앱이 없거나 끝나 읽지 못하는 것(AGENT_ERROR)은 기다릴 것이 아직(또는 더) 없는 것이다.
/// 정책의 거절(허용하지 않은 AI, 막은 앱, 화면 잠김)과 잘못된 인자는 바로 돌려준다
fn observe(desk: &dyn Desk, params: &WaitParams, condition: &Condition) -> Result<Seen, Failure> {
    let reply = if matches!(condition, Condition::Window(_)) && params.app.is_none() {
        desk.call("apps", Value::Null)
            .map(|reply| Seen::Windows(all_windows(&reply)))
    } else {
        let mut snapshot = json!({ "peek": true });
        for (key, value) in [
            ("session", &params.session),
            ("app", &params.app),
            ("ref", &params.scope),
        ] {
            if let Some(value) = value {
                snapshot[key] = json!(value);
            }
        }
        desk.call("snapshot", snapshot).map(|reply| {
            let lines = outline::parse(reply["tree"].as_str().unwrap_or_default());
            match condition {
                Condition::Window(_) => Seen::Windows(
                    lines
                        .iter()
                        .filter(|line| line.depth == 0)
                        .map(|line| line.name.clone())
                        .collect(),
                ),
                _ => Seen::Lines(lines),
            }
        })
    };
    match reply {
        Ok(seen) => Ok(seen),
        Err((protocol::AGENT_ERROR, message)) => Ok(Seen::Missing(message)),
        Err(failure) => Err(failure),
    }
}

/// `apps`의 답에서 모든 창의 제목 (트리가 있는 앱의 창과 트리가 없는 창)
fn all_windows(reply: &Value) -> Vec<String> {
    let titles = |list: &Value| -> Vec<String> {
        list.as_array()
            .into_iter()
            .flatten()
            .filter_map(|window| window["title"].as_str().map(str::to_string))
            .collect()
    };
    let mut windows: Vec<String> = reply["apps"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|app| titles(&app["windows"]))
        .collect();
    windows.extend(titles(&reply["untracked"]));
    windows
}

fn check(
    condition: &Condition,
    params: &WaitParams,
    seen: &Seen,
    baseline: &mut Option<Baseline>,
) -> Option<Outcome> {
    match (condition, seen) {
        (Condition::Window(title), Seen::Windows(windows)) => {
            if title.is_empty() {
                // 새로 뜬 창: 처음 본 목록에 없던 것
                let Baseline::Windows(known) =
                    baseline.get_or_insert_with(|| Baseline::Windows(windows.clone()))
                else {
                    return None;
                };
                return windows
                    .iter()
                    .find(|window| !known.contains(window))
                    .map(|window| Outcome::Window(window.clone()));
            }
            let open = windows
                .iter()
                .rev()
                .find(|window| outline::fits(title, window, false, params.exact));
            match (open, params.gone) {
                (Some(window), false) => Some(Outcome::Window(window.clone())),
                (None, true) => Some(Outcome::Gone),
                _ => None,
            }
        }
        (Condition::Window(title), Seen::Missing(_)) => {
            (params.gone && !title.is_empty()).then_some(Outcome::Gone)
        }
        (_, Seen::Missing(_)) => params.gone.then_some(Outcome::Gone),
        // observe는 창을 기다릴 때만 창 목록을, 아닐 때만 줄을 읽는다
        (_, Seen::Windows(_)) | (Condition::Window(_), Seen::Lines(_)) => None,
        (Condition::Element(matcher), Seen::Lines(lines)) => {
            // ref 아래에서 찾을 때 맨 위 줄은 그 ref 자신이다 (Playwright처럼 자손만 찾는다)
            let start = usize::from(params.scope.is_some()).min(lines.len());
            let hits: Vec<usize> = (start..lines.len())
                .filter(|&at| matcher.matches(&lines[at]))
                .collect();
            if params.gone {
                return hits.is_empty().then_some(Outcome::Gone);
            }
            let pick = match params.nth {
                None => hits.first(),
                Some(nth) if nth >= 0 => hits.get(nth as usize),
                Some(nth) => hits
                    .len()
                    .checked_sub(nth.unsigned_abs() as usize)
                    .and_then(|at| hits.get(at)),
            }?;
            Some(Outcome::Found {
                found: found(lines, *pick),
                matches: hits
                    .iter()
                    .take(MATCHES_KEPT)
                    .map(|&at| found(lines, at))
                    .collect(),
                count: hits.len(),
            })
        }
        (Condition::Itself, Seen::Lines(lines)) => match (lines.is_empty(), params.gone) {
            (true, true) => Some(Outcome::Gone),
            (false, false) => Some(single(lines)),
            _ => None,
        },
        (Condition::Value(value), Seen::Lines(lines)) => {
            let root = lines.first()?;
            outline::fits(value, root.value(), root.value_cut(), true).then(|| single(lines))
        }
        (Condition::Changed, Seen::Lines(lines)) => {
            // 처음에 없던 요소가 나타나도 바뀐 것이다
            let now = lines.first().map(|root| root.value().to_string());
            let Baseline::Value(first) =
                baseline.get_or_insert_with(|| Baseline::Value(now.clone()))
            else {
                return None;
            };
            (now.is_some() && *first != now).then(|| single(lines))
        }
    }
}

fn single(lines: &[Line]) -> Outcome {
    let found = found(lines, 0);
    Outcome::Found {
        matches: vec![found.clone()],
        found,
        count: 1,
    }
}

fn found(lines: &[Line], at: usize) -> Found {
    let line = &lines[at];
    Found {
        reference: outline::owner(lines, at).and_then(|owner| lines[owner].reference.clone()),
        line: line.text.clone(),
        role: line.role.clone(),
        name: line.name.clone(),
        // 값 속성만 준다: REPL의 inputValue()가 이름을 값으로 읽지 않게 (값이 없으면 이름을 주던 것을 바꿨다)
        value: line.prop("value").map(str::to_string),
        states: line.states.clone(),
        text: outline::text_content(lines, at),
    }
}

/// 사람이 읽을 범위: ` in Calculator`, ` below e12`, ` in the active window`
fn scope(params: &WaitParams) -> String {
    match (&params.scope, &params.app) {
        (Some(reference), _) => format!(" below {reference}"),
        (None, Some(app)) => format!(" in {app}"),
        (None, None) => " in the active window".into(),
    }
}

fn answer(condition: &Condition, params: &WaitParams, outcome: Outcome, ms: u64) -> Waited {
    let target = params.scope.as_deref().unwrap_or("it");
    match outcome {
        Outcome::Gone => {
            let what = match condition {
                Condition::Element(matcher) => format!("{}{}", matcher.describe(), scope(params)),
                Condition::Window(title) => format!("the window {title:?}"),
                _ => target.to_string(),
            };
            Waited {
                done: format!("{what} is gone"),
                waited_ms: ms,
                ..Waited::default()
            }
        }
        Outcome::Window(title) => Waited {
            done: if params.window.as_deref() == Some("") {
                format!("a new window {title:?} appeared")
            } else {
                format!("the window {title:?} is open")
            },
            waited_ms: ms,
            window: Some(title),
            ..Waited::default()
        },
        Outcome::Found {
            found,
            matches,
            count,
        } => {
            // 값을 기다렸으면 값(값이 없으면 이름)을 알린다
            let now = found.value.as_deref().unwrap_or(&found.name);
            let done = match condition {
                Condition::Element(matcher) if count > 1 => format!(
                    "{count} elements match {}{}",
                    matcher.describe(),
                    scope(params)
                ),
                Condition::Element(matcher) => format!("{} is there", matcher.describe()),
                Condition::Value(value) => format!("{target}'s value is {value:?}"),
                Condition::Changed => format!("{target} changed to {now:?}"),
                _ => format!("{target} is there"),
            };
            Waited {
                done,
                waited_ms: ms,
                found: Some(found),
                count,
                matches,
                window: None,
            }
        }
    }
}

/// 시간이 다 됐을 때의 알림: 무엇을 기다렸고 마지막으로 본 것이 어땠는지
fn timed_out(condition: &Condition, params: &WaitParams, seen: &Seen, limit: Duration) -> String {
    let target = params.scope.as_deref().unwrap_or("it");
    let what = match condition {
        Condition::Element(matcher) if params.gone => {
            format!("{}{} to go away", matcher.describe(), scope(params))
        }
        Condition::Element(matcher) => {
            let nth = params
                .nth
                .map_or(String::new(), |nth| format!(" (match {nth})"));
            format!("{}{}{nth}", matcher.describe(), scope(params))
        }
        Condition::Itself if params.gone => format!("{target} to go away"),
        Condition::Itself => format!("{target} to show"),
        Condition::Window(title) if title.is_empty() => "a new window".into(),
        Condition::Window(title) if params.gone => format!("the window {title:?} to close"),
        Condition::Window(title) => format!("a window titled {title:?}"),
        Condition::Value(value) => format!("{target}'s value to be {value:?}"),
        Condition::Changed => format!("{target}'s value to change"),
    };
    let now = match (condition, seen) {
        (_, Seen::Missing(message)) => format!("; last error: {message}"),
        (Condition::Window(_), Seen::Windows(windows)) => {
            let open: Vec<String> = windows
                .iter()
                .take(WINDOWS_SHOWN)
                .map(|title| format!("{title:?}"))
                .collect();
            match open.len() {
                0 => "; no windows are open".into(),
                _ => format!("; open: {}", open.join(", ")),
            }
        }
        (Condition::Element(matcher), Seen::Lines(lines)) => {
            match lines.iter().find(|line| matcher.matches(line)) {
                Some(line) if params.gone => format!("; still there: {}", line.text),
                Some(_) => String::new(),
                None => "; nothing matches it now".into(),
            }
        }
        (Condition::Value(_) | Condition::Changed, Seen::Lines(lines)) => match lines.first() {
            Some(root) => format!("; it is {:?}", root.value()),
            None => format!("; {target} is not on screen"),
        },
        (Condition::Itself, Seen::Lines(lines)) => match lines.first() {
            Some(root) => format!("; still there: {}", root.text),
            None => format!("; {target} is not on screen"),
        },
        _ => String::new(),
    };
    format!(
        "timed out after {} ms waiting for {what}{now}",
        limit.as_millis()
    )
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::VecDeque};

    use super::*;

    /// 부를 때마다 차례로 답하는 가짜 데몬. 다 쓰면 마지막 답을 되풀이한다
    struct Script {
        replies: RefCell<VecDeque<Result<Value, Failure>>>,
        calls: RefCell<Vec<(String, Value)>>,
    }

    impl Script {
        fn new(replies: Vec<Result<Value, Failure>>) -> Script {
            Script {
                replies: RefCell::new(replies.into()),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl Desk for Script {
        fn call(&self, method: &str, params: Value) -> Result<Value, Failure> {
            self.calls.borrow_mut().push((method.to_string(), params));
            let mut replies = self.replies.borrow_mut();
            if replies.len() > 1 {
                replies.pop_front().unwrap()
            } else {
                replies.front().cloned().unwrap()
            }
        }
    }

    fn tree(lines: &str) -> Result<Value, Failure> {
        Ok(
            json!({ "tree": format!("- app: \"calc\" [pid=7]\n{lines}"), "diff": "", "first": false }),
        )
    }

    fn params(edit: impl FnOnce(&mut WaitParams)) -> WaitParams {
        let mut params = WaitParams {
            timeout_ms: Some(1_000),
            ..WaitParams::default()
        };
        edit(&mut params);
        params
    }

    #[test]
    fn waits_until_an_element_appears() {
        let desk = Script::new(vec![
            tree("- window \"계산기\" [ref=e1]:\n  - textbox [ref=e9] [value=\"6×7\"]\n"),
            tree(
                "- window \"계산기\" [ref=e1]:\n  - list [ref=e3]:\n    - listitem \"6×7\" [ref=e4]:\n      - text: \"42\"\n",
            ),
        ]);
        let waited = run(
            &desk,
            &params(|p| {
                p.app = Some("calc".into());
                p.text = Some("42".into());
            }),
        )
        .unwrap();
        let found = waited.found.unwrap();
        // 글 줄에는 ref가 없어 그 위의 목록 항목을 준다
        assert_eq!(found.reference.as_deref(), Some("e4"));
        assert_eq!(found.line, "- text: \"42\"");
        assert_eq!(waited.count, 1);
        assert!(waited.done.contains("is there"), "{}", waited.done);
        // 다시 읽은 스냅샷은 peek이라 다음 diff를 흐리지 않고, 범위를 그대로 넘긴다
        let calls = desk.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "snapshot");
        assert_eq!(calls[0].1, json!({ "peek": true, "app": "calc" }));
    }

    #[test]
    fn several_matches_are_counted_and_nth_picks_one() {
        let lines = "- window \"w\" [ref=e1]:\n  - button \"1\" [ref=e2]\n  - button \"2\" [ref=e3]\n  - button \"3\" [ref=e4]\n";
        let waited = run(
            &Script::new(vec![tree(lines)]),
            &params(|p| p.role = Some("button".into())),
        )
        .unwrap();
        assert_eq!(waited.count, 3);
        assert_eq!(waited.matches.len(), 3);
        assert!(waited.done.starts_with("3 elements match button"));
        let last = run(
            &Script::new(vec![tree(lines)]),
            &params(|p| {
                p.role = Some("button".into());
                p.nth = Some(-1);
            }),
        )
        .unwrap();
        assert_eq!(last.found.unwrap().reference.as_deref(), Some("e4"));
        // 없는 차례는 시간이 다 될 때까지 기다린다
        let missing = run(
            &Script::new(vec![tree(lines)]),
            &params(|p| {
                p.role = Some("button".into());
                p.nth = Some(3);
                p.timeout_ms = Some(0);
            }),
        )
        .unwrap_err();
        assert_eq!(missing.0, protocol::TIMEOUT);
        assert!(missing.1.contains("(match 3)"), "{}", missing.1);
    }

    #[test]
    fn below_a_ref_only_descendants_match() {
        let desk = Script::new(vec![tree(
            "- list \"목록\" [ref=e3]:\n  - listitem \"하나\" [ref=e4]\n",
        )]);
        let waited = run(
            &desk,
            &params(|p| {
                p.scope = Some("e3".into());
                p.text = Some("목록".into());
                p.timeout_ms = Some(0);
            }),
        );
        assert_eq!(waited.unwrap_err().0, protocol::TIMEOUT);
        assert_eq!(desk.calls.borrow()[0].1["ref"], "e3");
    }

    #[test]
    fn waits_until_an_element_is_gone() {
        let dialog = "- window \"편집기\" [ref=e1]:\n  - dialog \"저장할까요?\" [ref=e7]\n";
        let desk = Script::new(vec![
            tree(dialog),
            tree(dialog),
            tree("- window \"편집기\" [ref=e1]\n"),
        ]);
        let waited = run(
            &desk,
            &params(|p| {
                p.role = Some("dialog".into());
                p.gone = true;
            }),
        )
        .unwrap();
        assert!(waited.found.is_none());
        assert_eq!(waited.done, "dialog in the active window is gone");
        assert_eq!(desk.calls.borrow().len(), 3);
        // 시간이 다 되면 아직 있는 줄을 알린다
        let failure = run(
            &Script::new(vec![tree(dialog)]),
            &params(|p| {
                p.role = Some("dialog".into());
                p.gone = true;
                p.timeout_ms = Some(150);
            }),
        )
        .unwrap_err();
        assert_eq!(failure.0, protocol::TIMEOUT);
        assert!(
            failure.1.starts_with("timed out after 150 ms waiting for dialog in the active window to go away; still there: - dialog"),
            "{}",
            failure.1
        );
    }

    #[test]
    fn a_closed_app_counts_as_gone_but_not_as_there() {
        let closed = || -> Result<Value, Failure> {
            Err((protocol::AGENT_ERROR, "no app matches \"calc\"".into()))
        };
        let gone = run(
            &Script::new(vec![closed()]),
            &params(|p| {
                p.app = Some("calc".into());
                p.text = Some("42".into());
                p.gone = true;
            }),
        );
        assert!(gone.is_ok());
        let failure = run(
            &Script::new(vec![closed()]),
            &params(|p| {
                p.app = Some("calc".into());
                p.text = Some("42".into());
                p.timeout_ms = Some(0);
            }),
        )
        .unwrap_err();
        assert!(
            failure.1.ends_with("last error: no app matches \"calc\""),
            "{}",
            failure.1
        );
    }

    #[test]
    fn refusals_end_the_wait_at_once() {
        for code in [
            protocol::NOT_ALLOWED,
            protocol::BLOCKED_APP,
            protocol::SCREEN_LOCKED,
            protocol::INVALID_PARAMS,
        ] {
            let desk = Script::new(vec![Err((code, "no".into()))]);
            let failure = run(
                &desk,
                &params(|p| {
                    p.text = Some("x".into());
                    p.timeout_ms = Some(5_000);
                }),
            )
            .unwrap_err();
            assert_eq!(failure.0, code);
            assert_eq!(desk.calls.borrow().len(), 1, "no retry after {code}");
        }
    }

    #[test]
    fn waits_for_a_value_or_a_change() {
        let field = |value: &str| tree(&format!("- textbox [ref=e9] [value=\"{value}\"]\n"));
        let desk = Script::new(vec![field("6×7"), field("42")]);
        let waited = run(
            &desk,
            &params(|p| {
                p.scope = Some("e9".into());
                p.value = Some("42".into());
            }),
        )
        .unwrap();
        assert_eq!(waited.done, "e9's value is \"42\"");
        assert_eq!(waited.found.unwrap().value.as_deref(), Some("42"));

        let desk = Script::new(vec![field("6×7"), field("6×7"), field("42")]);
        let changed = run(
            &desk,
            &params(|p| {
                p.scope = Some("e9".into());
                p.changed = true;
            }),
        )
        .unwrap();
        assert_eq!(changed.done, "e9 changed to \"42\"");

        let failure = run(
            &Script::new(vec![field("6×7")]),
            &params(|p| {
                p.scope = Some("e9".into());
                p.value = Some("42".into());
                p.timeout_ms = Some(0);
            }),
        )
        .unwrap_err();
        assert!(failure.1.ends_with("; it is \"6×7\""), "{}", failure.1);
    }

    #[test]
    fn waits_for_windows_of_an_app_or_of_any_app() {
        // 앱 범위: 스냅샷의 맨 위 줄이 그 앱의 창이다
        let desk = Script::new(vec![
            tree("- window \"편집기\" [ref=e1]\n"),
            tree("- window \"편집기\" [ref=e1]\n- dialog \"다른 이름으로 저장\" [ref=e8]\n"),
        ]);
        let waited = run(
            &desk,
            &params(|p| {
                p.app = Some("editor".into());
                p.window = Some("저장".into());
            }),
        )
        .unwrap();
        assert_eq!(waited.window.as_deref(), Some("다른 이름으로 저장"));
        // 앱 없이: 모든 앱의 창 (트리가 없는 창 포함)에서 새로 뜬 창
        let apps = |titles: &[&str]| -> Result<Value, Failure> {
            Ok(json!({
                "apps": [{ "name": "calc", "windows": [{ "title": "계산기", "active": true }] }],
                "untracked": titles.iter().map(|title| json!({ "title": title })).collect::<Vec<_>>(),
            }))
        };
        let desk = Script::new(vec![apps(&[]), apps(&["Claude"])]);
        let waited = run(&desk, &params(|p| p.window = Some(String::new()))).unwrap();
        assert_eq!(waited.done, "a new window \"Claude\" appeared");
        assert_eq!(desk.calls.borrow()[0].0, "apps");
        // 닫히기
        let desk = Script::new(vec![apps(&["Claude"]), apps(&[])]);
        let waited = run(
            &desk,
            &params(|p| {
                p.window = Some("claude".into());
                p.gone = true;
            }),
        )
        .unwrap();
        assert_eq!(waited.done, "the window \"claude\" is gone");
        let failure = run(
            &Script::new(vec![apps(&[])]),
            &params(|p| {
                p.window = Some("설정".into());
                p.timeout_ms = Some(0);
            }),
        )
        .unwrap_err();
        assert!(failure.1.ends_with("; open: \"계산기\""), "{}", failure.1);
    }

    #[test]
    fn a_ref_alone_waits_for_that_element() {
        let waited = run(
            &Script::new(vec![tree("- button \"확인\" [ref=e5]\n")]),
            &params(|p| p.scope = Some("e5".into())),
        )
        .unwrap();
        assert_eq!(waited.found.unwrap().name, "확인");
        // 숨은 요소는 ref 범위의 트리에 줄이 없다
        let gone = run(
            &Script::new(vec![tree("")]),
            &params(|p| {
                p.scope = Some("e5".into());
                p.gone = true;
            }),
        )
        .unwrap();
        assert_eq!(gone.done, "e5 is gone");
    }

    #[test]
    fn conditions_are_checked_before_reading() {
        let desk = Script::new(vec![tree("")]);
        let bad = [
            params(|_| {}),
            params(|p| p.value = Some("1".into())),
            params(|p| p.changed = true),
            params(|p| {
                p.window = Some("a".into());
                p.text = Some("b".into());
            }),
            params(|p| {
                p.window = Some(String::new());
                p.gone = true;
            }),
            params(|p| {
                p.scope = Some("e1".into());
                p.value = Some("1".into());
                p.changed = true;
            }),
            params(|p| {
                p.scope = Some("e1".into());
                p.value = Some("1".into());
                p.gone = true;
            }),
            params(|p| p.nth = Some(1)),
        ];
        for params in bad {
            let failure = run(&desk, &params).unwrap_err();
            assert_eq!(failure.0, protocol::INVALID_PARAMS, "{params:?}");
        }
        assert!(desk.calls.borrow().is_empty());
    }

    #[test]
    fn the_timeout_has_a_default_and_a_cap() {
        let started = Instant::now();
        let failure = run(
            &Script::new(vec![tree("")]),
            &params(|p| {
                p.text = Some("never".into());
                p.timeout_ms = Some(250);
            }),
        )
        .unwrap_err();
        let took = started.elapsed();
        assert!(
            took >= Duration::from_millis(250) && took < Duration::from_millis(1_000),
            "{took:?}"
        );
        assert!(failure.1.contains("nothing matches it now"));
        let limit_of = |timeout_ms: Option<u64>| {
            limit(&WaitParams {
                timeout_ms,
                ..WaitParams::default()
            })
        };
        assert_eq!(limit_of(None), Duration::from_secs(10));
        assert_eq!(limit_of(Some(0)), Duration::ZERO);
        assert_eq!(limit_of(Some(u64::MAX)), Duration::from_secs(120));
    }
}
