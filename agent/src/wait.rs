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
//!
//! 앱(`app`)이나 ref 범위의 기다리기는 처음 본 그 앱의 pid에 묶인다: 앱이 다시 뜨거나 끝나면 새 프로세스에 조용히
//! 붙지 않고 TARGET_GONE으로 끝난다(이슈 #1 P2-2).
//!
//! 창을 기다리면 창이 뜬 뒤 그 문서가 다 불러오기(BUSY가 풀리기)까지 시간 상한 안에서 더 기다린다. `loaded`는 범위의
//! 보이는 문서가 다 불러오기를 기다린다(이슈 #1 P2-3: `wait --window`가 본문을 받기 전에 끝났다).

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
/// 맞는 요소 목록의 한 줄에 싣는 아래 글(text)의 글자 수. 그릇(region, document)은 아래의 글을 모두 담아, 여럿이
/// 맞으면 답이 트리의 수십 배가 되었다 (두 번째 검토: 1.3MiB 트리에서 56MiB 답, REPL을 잃었다). 고른 요소(found)는 다
/// 싣는다
const MATCH_TEXT: usize = 200;
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
    /// 범위의 보이는 문서가 다 불러왔다 (BUSY가 풀렸다)
    Loaded,
}

/// 범위를 한 번 읽은 것
enum Seen {
    /// 트리의 요소 줄. `cut`이면 트리가 길어 끝을 잘라 안 보이는 요소가 더 있을 수 있다. `pid`는 그 앱의 pid
    /// (스냅샷 머리줄)
    Lines {
        lines: Vec<Line>,
        cut: bool,
        pid: Option<u32>,
        /// 범위의 보이는 문서가 불러오는 중이다 (스냅샷 답의 busy)
        busy: bool,
    },
    /// 창 제목. `complete`가 아니면 빠진 창이 있을 수 있다 (잘린 트리에서 읽었다). `pid`는 앱 범위일 때 그 앱의
    /// pid (모든 앱의 창이면 None)
    Windows {
        titles: Vec<String>,
        complete: bool,
        pid: Option<u32>,
        /// 그 앱의 보이는 문서가 불러오는 중이다. 모든 앱의 창이면 모른다(None)
        busy: Option<bool>,
    },
    /// 앱이 없다 (아직 뜨지 않았거나 끝났다): 그 앱의 요소와 창도 없다
    Absent(String),
    /// 보지 못했다 (키보드 포커스를 가진 창이 없다, 트리가 없는 창, 접근성 버스의 오류): 있는지 없는지 모른다.
    /// 검토: 이것을 사라진 것으로 보아, 포커스가 터미널로 가기만 해도 기다리던 요소가 사라졌다고 했다
    Unknown(String),
}

/// 처음 본 것: 새로 뜬 창은 처음 본 창 목록과, 바뀐 값은 처음 본 값과 견준다. 불러오기는 불러오는 중이던 적이
/// 있는지 적는다
enum Baseline {
    Windows(Vec<String>),
    Value(Option<String>),
    Busy(bool),
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
    /// 다 불러왔다. 참이면 기다리는 동안 불러오는 중이었다
    Loaded(bool),
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
        if params.loaded && params.gone {
            return Err(invalid("loaded does not go with gone"));
        }
        return Ok(Condition::Window(window.clone()));
    }
    if params.loaded {
        if !matcher.is_empty() || valued || params.gone || params.nth.is_some() {
            return Err(invalid(
                "loaded goes alone (with app or ref) or with window: it waits for the page to load",
            ));
        }
        return Ok(Condition::Loaded);
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
        "wait needs something to wait for: text, role and name, window, loaded, a ref, or value or \
         changed with a ref",
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
    let mut bound: Option<u32> = None;
    // 모든 앱의 창에서 찾은 창은 그 창의 앱을 범위로 삼아 다 불러오기를 기다린다
    let mut params = params.clone();
    // 떴지만 그 문서를 아직 다 불러오지 못한 창: (제목, 마지막으로 불러오는 중이었는지). 앱을 읽지 못해 모를 수도 있다
    let mut opened: Option<(String, bool)> = None;
    let ms = || started.elapsed().as_millis() as u64;
    loop {
        let seen = observe(desk, &params, &condition)?;
        if let Some(failure) = bind(&mut bound, &seen, &condition, &params) {
            crate::atspi::dump_calls(None, &failure.1);
            return Err(failure);
        }
        match check(&condition, &params, &seen, &mut baseline) {
            // 창이 떴다: 그 문서가 다 불러오기를 시간 상한까지 기다린다 (이슈 #1 P2-3)
            Some(Outcome::Window(title)) if !params.gone => match seen {
                Seen::Windows {
                    busy: Some(true), ..
                } => opened = Some((title, true)),
                Seen::Windows { busy: None, .. } => match owner(desk, &title)? {
                    Some(bus) => {
                        params.app = Some(bus);
                        opened = Some((title, false));
                        continue;
                    }
                    // 트리가 없는 창: 불러오는지 모른다
                    None => return Ok(answer(&condition, &params, Outcome::Window(title), ms())),
                },
                _ => return Ok(answer(&condition, &params, Outcome::Window(title), ms())),
            },
            Some(outcome) => return Ok(answer(&condition, &params, outcome, ms())),
            // 창 목록에 다시 없다: 닫혔다
            None if matches!(seen, Seen::Windows { .. }) => opened = None,
            None => {}
        }
        let elapsed = started.elapsed();
        if elapsed >= limit {
            return match opened {
                // 창은 떴다: --loaded가 아니면 기다린 것은 일어났다
                Some((title, busy)) if !params.loaded => {
                    let mut waited = answer(&condition, &params, Outcome::Window(title), ms());
                    waited.done.push_str(if busy {
                        "; it is still loading"
                    } else {
                        "; could not tell whether its page finished loading"
                    });
                    Ok(waited)
                }
                Some((title, _)) => Err((
                    protocol::TIMEOUT,
                    format!(
                        "timed out after {} ms waiting for the window {title:?} to finish loading; \
                         it is still loading",
                        limit.as_millis()
                    ),
                )),
                None => Err((
                    protocol::TIMEOUT,
                    timed_out(&condition, &params, &seen, limit),
                )),
            };
        }
        std::thread::sleep(POLL.min(limit - elapsed));
    }
}

/// 모든 앱의 창 목록(`apps`)에서 이 제목의 창을 가진 앱의 접근성 버스 이름. 트리가 없는 창이거나 목록을 읽지
/// 못했으면 None
fn owner(desk: &dyn Desk, title: &str) -> Result<Option<String>, Failure> {
    let reply = match desk.call("apps", Value::Null) {
        Ok(reply) => reply,
        Err((protocol::AGENT_ERROR, _)) => return Ok(None),
        Err(failure) => return Err(failure),
    };
    Ok(reply["apps"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|app| {
            app["windows"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|window| window["title"].as_str() == Some(title))
        })
        .and_then(|app| app["bus"].as_str())
        .map(str::to_string))
}

/// 범위를 한 번 읽는다. 정책의 거절(허용하지 않은 AI, 막은 앱, 화면 잠김)과 잘못된 인자는 바로 돌려준다. 읽지 못한
/// 것(AGENT_ERROR)은 그 까닭으로 가른다(`unseen`)
fn observe(desk: &dyn Desk, params: &WaitParams, condition: &Condition) -> Result<Seen, Failure> {
    let seen = match condition {
        Condition::Window(_) => windows(desk, params),
        _ => lines(desk, params, condition),
    };
    match seen {
        Err((protocol::AGENT_ERROR, message)) => unseen(message),
        // 데몬이 ref의 앱이 끝났거나 다시 떴다고 알렸다: 사라지기를 기다렸으면 사라진 것이고, 아니면 그대로 끝낸다
        Err((protocol::TARGET_GONE, message)) if params.gone => Ok(Seen::Absent(message)),
        seen => seen,
    }
}

/// 기다리기를 처음 본 대상 앱의 pid에 묶는다 (이슈 #1 P2-2: Firefox가 끝나고 다시 뜨자 `wait --window … --app
/// firefox-esr`이 새 프로세스를 지켜보다 시간이 다 되었다). 앱(`app`)이나 ref 범위만 묶는다: 활성 창 범위는 다른
/// 앱으로 옮겨 갈 수 있다. pid가 바뀌면 앱이 다시 뜬 것이고, 묶은 뒤에 앱이 없으면 끝난 것이다(ref의 앱은 처음부터
/// 없어도). 사라지기(gone)를 기다리면 앱이 끝난 것은 기다리던 것이 사라진 것이라 실패가 아니다
fn bind(
    bound: &mut Option<u32>,
    seen: &Seen,
    condition: &Condition,
    params: &WaitParams,
) -> Option<Failure> {
    if params.app.is_none() && params.scope.is_none() {
        return None;
    }
    let what = || describe(condition, params);
    match seen {
        Seen::Lines { pid, .. } | Seen::Windows { pid, .. } => {
            let now = (*pid)?;
            match *bound.get_or_insert(now) {
                first if first != now => Some((
                    protocol::TARGET_GONE,
                    format!(
                        "stopped waiting for {}: the app restarted (pid {first} → {now}); take a \
                         new snapshot",
                        what()
                    ),
                )),
                _ => None,
            }
        }
        Seen::Absent(_) if params.gone => None,
        Seen::Absent(message) => match (*bound, &params.scope) {
            (Some(first), _) => Some((
                protocol::TARGET_GONE,
                format!(
                    "stopped waiting for {}: {} (pid {first}); take a new snapshot",
                    what(),
                    protocol::APP_QUIT
                ),
            )),
            // ref의 앱은 다시 나타나지 않는다
            (None, Some(_)) => Some((
                protocol::TARGET_GONE,
                format!("stopped waiting for {}: {message}", what()),
            )),
            // 아직 뜨지 않은 앱: 뜨기를 기다린다
            (None, None) => None,
        },
        Seen::Unknown(_) => None,
    }
}

/// 데몬이 범위를 읽지 못한 까닭: 앱이 없다(ref의 앱이 끝났다), 앱 이름이 여럿에 맞는다(기다려도 하나로 정해지지
/// 않아 바로 알린다), 그 밖은 보지 못한 것이다. 데몬의 알림은 buntd-agent의 find_app과 snapshot이 쓴다 (find_app의
/// 시험이 이 글을 본다)
fn unseen(message: String) -> Result<Seen, Failure> {
    if message.starts_with(protocol::NO_APP_MATCHES) || message.contains(protocol::APP_QUIT) {
        Ok(Seen::Absent(message))
    } else if message.contains(protocol::BE_MORE_SPECIFIC) {
        Err((protocol::AGENT_ERROR, message))
    } else {
        Ok(Seen::Unknown(message))
    }
}

/// 다시 읽는 스냅샷의 인자. peek이라 다음 스냅샷의 diff를 흐리지 않는다
fn snapshot(params: &WaitParams, interactive: bool) -> Value {
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
    if interactive {
        snapshot["interactive"] = json!(true);
    }
    snapshot
}

/// 범위의 요소 줄. 트리가 길어 잘렸으면 상호작용 요소만 읽은 트리(훨씬 짧다)도 읽어, 찾는 요소가 더 많이 맞는 쪽을
/// 쓴다: 긴 웹 페이지의 2000줄 뒤에 있는 단추, 링크, 글 칸을 찾고(검토: snapshot({ interactive: true })에는 보이는
/// 것을 getByRole이 찾지 못했다), 여럿 가운데 고르기(nth, last)와 세기, strict 검사가 앞쪽만 보지 않는다(두 번째 검토)
fn lines(desk: &dyn Desk, params: &WaitParams, condition: &Condition) -> Result<Seen, Failure> {
    let reply = desk.call("snapshot", snapshot(params, false))?;
    let tree = reply["tree"].as_str().unwrap_or_default();
    let lines = outline::parse(tree);
    let cut = outline::truncated(tree);
    let pid = outline::app_pid(tree);
    let busy = reply["busy"].as_bool().unwrap_or(false);
    if let Condition::Element(matcher) = condition
        && cut
        && !params.gone
    {
        let count = |lines: &[Line]| lines.iter().filter(|line| matcher.matches(line)).count();
        let reply = desk.call("snapshot", snapshot(params, true))?;
        let short = reply["tree"].as_str().unwrap_or_default();
        let found = outline::parse(short);
        if count(&found) > count(&lines) {
            return Ok(Seen::Lines {
                lines: found,
                cut: outline::truncated(short),
                pid,
                busy,
            });
        }
    }
    Ok(Seen::Lines {
        lines,
        cut,
        pid,
        busy,
    })
}

/// 창 제목. 앱을 주었으면 그 앱의 보이는 창: 스냅샷의 맨 위 줄이다. 트리가 2000줄에서 잘렸으면 뒤의 창이 빠질 수
/// 있어 앱 목록(`apps`)에서 그 앱(머리줄의 pid)의 창을 더한다 (검토: 잘린 트리 뒤의 창을 닫힌 것으로 보았다). 앱
/// 목록만 쓰지 않는 것은 바쁜 앱(창 목록을 읽다 시간이 다 된 앱)이 창 없는 앱으로 보이고 숨은 창(닫힌 Qt 대화 상자)도
/// 들기 때문이다 (두 번째 검토)
fn windows(desk: &dyn Desk, params: &WaitParams) -> Result<Seen, Failure> {
    if params.app.is_none() {
        let reply = desk.call("apps", Value::Null)?;
        return Ok(Seen::Windows {
            titles: all_windows(&reply, None),
            complete: true,
            pid: None,
            busy: None,
        });
    }
    let reply = desk.call("snapshot", snapshot(params, false))?;
    let tree = reply["tree"].as_str().unwrap_or_default();
    let mut titles: Vec<String> = outline::parse(tree)
        .into_iter()
        .filter(|line| line.depth == 0)
        .map(|line| line.name)
        .collect();
    let mut complete = !outline::truncated(tree);
    let pid = outline::app_pid(tree);
    let busy = Some(reply["busy"].as_bool().unwrap_or(false));
    if !complete && let Some(pid) = pid {
        let listed = all_windows(&desk.call("apps", Value::Null)?, Some(pid));
        // 보이는 창이 있는 앱인데 목록에 창이 없으면 목록을 읽지 못한 것이다(바쁜 앱): 다 보았다고 하지 않는다
        complete = !listed.is_empty();
        for title in listed {
            if !titles.contains(&title) {
                titles.push(title);
            }
        }
    }
    Ok(Seen::Windows {
        titles,
        complete,
        pid,
        busy,
    })
}

/// `apps`의 답에서 창 제목 (트리가 있는 앱의 창과 트리가 없는 창). pid를 주면 그 프로세스의 창만
fn all_windows(reply: &Value, pid: Option<u32>) -> Vec<String> {
    let mine = |entry: &&Value| pid.is_none_or(|pid| entry["pid"].as_u64() == Some(u64::from(pid)));
    let title = |window: &Value| window["title"].as_str().map(str::to_string);
    let mut windows: Vec<String> = reply["apps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(mine)
        .flat_map(|app| {
            app["windows"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(title)
        })
        .collect();
    windows.extend(
        reply["untracked"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(mine)
            .filter_map(title),
    );
    windows
}

fn check(
    condition: &Condition,
    params: &WaitParams,
    seen: &Seen,
    baseline: &mut Option<Baseline>,
) -> Option<Outcome> {
    match (condition, seen) {
        (
            Condition::Window(title),
            Seen::Windows {
                titles: windows,
                complete,
                ..
            },
        ) => {
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
                .find(|window| outline::fits(title, window, 0, params.exact));
            match (open, params.gone) {
                (Some(window), false) => Some(Outcome::Window(window.clone())),
                (None, true) if *complete => Some(Outcome::Gone),
                _ => None,
            }
        }
        (Condition::Window(title), Seen::Absent(_)) => {
            // 아직 뜨지 않은 앱의 첫 창도 새 창이다
            if title.is_empty() {
                baseline.get_or_insert_with(|| Baseline::Windows(Vec::new()));
            }
            (params.gone && !title.is_empty()).then_some(Outcome::Gone)
        }
        (_, Seen::Absent(_)) => params.gone.then_some(Outcome::Gone),
        (_, Seen::Unknown(_)) => None,
        // observe는 창을 기다릴 때만 창 목록을, 아닐 때만 줄을 읽는다
        (_, Seen::Windows { .. }) | (Condition::Window(_), Seen::Lines { .. }) => None,
        (Condition::Element(matcher), Seen::Lines { lines, cut, .. }) => {
            // ref 아래에서 찾을 때 맨 위 줄은 그 ref 자신이다 (Playwright처럼 자손만 찾는다)
            let start = usize::from(params.scope.is_some()).min(lines.len());
            let hits: Vec<usize> = (start..lines.len())
                .filter(|&at| matcher.matches(&lines[at]))
                .collect();
            if params.gone {
                // 잘린 트리에 없다고 사라진 것은 아니다
                return (hits.is_empty() && !cut).then_some(Outcome::Gone);
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
                    .map(|&at| listed(found(lines, at)))
                    .collect(),
                count: hits.len(),
            })
        }
        (Condition::Itself, Seen::Lines { lines, .. }) => match (lines.is_empty(), params.gone) {
            (true, true) => Some(Outcome::Gone),
            (false, false) => Some(single(lines)),
            _ => None,
        },
        (Condition::Value(value), Seen::Lines { lines, .. }) => {
            let root = lines.first()?;
            outline::fits(value, root.value(), root.value_hidden(), true).then(|| single(lines))
        }
        (Condition::Loaded, Seen::Lines { busy, .. }) => {
            let Baseline::Busy(was_busy) = baseline.get_or_insert(Baseline::Busy(false)) else {
                return None;
            };
            *was_busy |= *busy;
            (!busy).then_some(Outcome::Loaded(*was_busy))
        }
        (Condition::Changed, Seen::Lines { lines, .. }) => {
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
        matches: vec![listed(found.clone())],
        found,
        count: 1,
    }
}

/// 목록에 싣는 요소: 아래 글을 MATCH_TEXT 글자로 자른다 (트리처럼 `…[+N chars]`)
fn listed(mut found: Found) -> Found {
    if let Some((cut, _)) = found.text.char_indices().nth(MATCH_TEXT) {
        let hidden = found.text[cut..].chars().count();
        found.text.truncate(cut);
        found.text.push_str(&format!("…[+{hidden} chars]"));
    }
    found
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
        Outcome::Loaded(was_busy) => Waited {
            done: if was_busy {
                format!("the page finished loading{}", scope(params))
            } else {
                format!("nothing is loading{}", scope(params))
            },
            waited_ms: ms,
            ..Waited::default()
        },
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

/// 기다린 것 (알림에 쓴다): `button "OK" in Calculator`, `a window titled "저장"`
fn describe(condition: &Condition, params: &WaitParams) -> String {
    let target = params.scope.as_deref().unwrap_or("it");
    match condition {
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
        Condition::Loaded => format!("the page to finish loading{}", scope(params)),
    }
}

/// 시간이 다 됐을 때의 알림: 무엇을 기다렸고 마지막으로 본 것이 어땠는지
fn timed_out(condition: &Condition, params: &WaitParams, seen: &Seen, limit: Duration) -> String {
    let target = params.scope.as_deref().unwrap_or("it");
    let what = describe(condition, params);
    // 잘린 트리에서는 다 보지 못한다: 범위를 좁히라고 알린다
    let narrower = "the tree is longer than the 2000 lines a snapshot shows; wait below a ref \
                    (locator(ref).getByText(…), --ref) to see the rest";
    let now = match (condition, seen) {
        (_, Seen::Absent(message) | Seen::Unknown(message)) => format!("; last error: {message}"),
        (
            Condition::Window(_),
            Seen::Windows {
                titles, complete, ..
            },
        ) => {
            let open: Vec<String> = titles
                .iter()
                .take(WINDOWS_SHOWN)
                .map(|title| format!("{title:?}"))
                .collect();
            let open = match open.len() {
                0 => "; no windows are open".into(),
                _ => format!("; open: {}", open.join(", ")),
            };
            match complete {
                true => open,
                false => format!("{open}; {narrower}"),
            }
        }
        (Condition::Element(matcher), Seen::Lines { lines, cut, .. }) => {
            match (lines.iter().find(|line| matcher.matches(line)), cut) {
                (Some(line), _) if params.gone => format!("; still there: {}", line.text),
                (Some(_), _) => String::new(),
                (None, false) => "; nothing matches it now".into(),
                (None, true) => format!("; nothing matches it in what it can see: {narrower}"),
            }
        }
        (Condition::Value(_) | Condition::Changed, Seen::Lines { lines, .. }) => {
            match lines.first() {
                Some(root) => format!("; it is {:?}", root.value()),
                None => format!("; {target} is not on screen"),
            }
        }
        (Condition::Itself, Seen::Lines { lines, .. }) => match lines.first() {
            Some(root) => format!("; still there: {}", root.text),
            None => format!("; {target} is not on screen"),
        },
        (Condition::Loaded, Seen::Lines { .. }) => "; it is still loading".into(),
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
    fn what_it_cannot_see_is_not_gone() {
        // 키보드 포커스가 트리 없는 창(터미널)으로 갔다: 기다리던 대화 상자가 사라졌다고 하지 않는다
        let unseen = || -> Result<Value, Failure> {
            Err((
                protocol::AGENT_ERROR,
                "no window has the keyboard focus; pass --app".into(),
            ))
        };
        let failure = run(
            &Script::new(vec![unseen()]),
            &params(|p| {
                p.role = Some("dialog".into());
                p.gone = true;
                p.timeout_ms = Some(150);
            }),
        )
        .unwrap_err();
        assert_eq!(failure.0, protocol::TIMEOUT);
        assert!(
            failure
                .1
                .ends_with("last error: no window has the keyboard focus; pass --app"),
            "{}",
            failure.1
        );
        // 다시 보이면 그때 본다
        let desk = Script::new(vec![unseen(), tree("- window \"w\" [ref=e1]\n")]);
        let gone = run(
            &desk,
            &params(|p| {
                p.role = Some("dialog".into());
                p.gone = true;
            }),
        )
        .unwrap();
        assert_eq!(gone.done, "dialog in the active window is gone");
        // 앱 이름이 여럿에 맞으면 기다려도 하나로 정해지지 않는다: 바로 알린다
        let desk = Script::new(vec![Err((
            protocol::AGENT_ERROR,
            "\"editor\" matches Text Editor, Text Editor; be more specific".into(),
        ))]);
        let failure = run(
            &desk,
            &params(|p| {
                p.app = Some("editor".into());
                p.text = Some("x".into());
                p.gone = true;
            }),
        )
        .unwrap_err();
        assert_eq!(failure.0, protocol::AGENT_ERROR);
        assert!(failure.1.ends_with("be more specific"));
        assert_eq!(desk.calls.borrow().len(), 1);
    }

    #[test]
    fn what_a_ref_held_is_gone_when_its_app_quits() {
        // 검토: 앱이 끝나 ref의 트리를 읽지 못하는 것을 보지 못한 것으로 보아, ref로 사라지기를 기다리면 끝나지 않았다
        let quit = || -> Result<Value, Failure> {
            Err((
                protocol::AGENT_ERROR,
                format!("e1 is gone: {}; take a new snapshot", protocol::APP_QUIT),
            ))
        };
        let gone = run(
            &Script::new(vec![tree("- window \"편집기\" [ref=e1]\n"), quit()]),
            &params(|p| {
                p.scope = Some("e1".into());
                p.gone = true;
            }),
        )
        .unwrap();
        assert_eq!(gone.done, "e1 is gone");
        let below = run(
            &Script::new(vec![quit()]),
            &params(|p| {
                p.scope = Some("e1".into());
                p.role = Some("button".into());
                p.gone = true;
            }),
        )
        .unwrap();
        assert_eq!(below.done, "button below e1 is gone");
    }

    #[test]
    fn a_cut_tree_neither_hides_nor_loses_elements() {
        let cut = |lines: &str| {
            tree(&format!(
                "{lines}# note: cut at 2000 lines; pass a ref to read one part\n"
            ))
        };
        // 사라지기: 잘린 트리에 없다고 사라졌다고 하지 않는다
        let failure = run(
            &Script::new(vec![cut("- document \"page\" [ref=e1]\n")]),
            &params(|p| {
                p.text = Some("Uploading".into());
                p.gone = true;
                p.timeout_ms = Some(150);
            }),
        )
        .unwrap_err();
        assert!(
            failure
                .1
                .contains("nothing matches it in what it can see: the tree is longer than"),
            "{}",
            failure.1
        );
        // 나타나기: 잘린 트리에 없으면 상호작용 요소만 읽은 트리에서 찾는다
        let desk = Script::new(vec![
            cut("- document \"page\" [ref=e1]\n"),
            tree(
                "# note: interactive elements only\n- document \"page\" [ref=e1]:\n  - button \"Submit\" [ref=e2400]\n",
            ),
        ]);
        let waited = run(
            &desk,
            &params(|p| {
                p.role = Some("button".into());
                p.name = Some("Submit".into());
            }),
        )
        .unwrap();
        assert_eq!(waited.found.unwrap().reference.as_deref(), Some("e2400"));
        assert_eq!(desk.calls.borrow()[1].1["interactive"], true);
        // 앞쪽에 하나가 보여도 뒤쪽까지 센다: last()는 뒤의 것이고, strict 검사는 둘을 본다 (두 번째 검토)
        let both = || {
            Script::new(vec![
                cut("- document \"page\" [ref=e1]:\n  - button \"Delete\" [ref=e2]\n"),
                tree(
                    "# note: interactive elements only\n- document \"page\" [ref=e1]:\n  - button \"Delete\" [ref=e2]\n  - button \"Delete\" [ref=e2400]\n",
                ),
            ])
        };
        let last = run(
            &both(),
            &params(|p| {
                p.role = Some("button".into());
                p.name = Some("Delete".into());
                p.nth = Some(-1);
            }),
        )
        .unwrap();
        assert_eq!(
            (last.count, last.found.unwrap().reference.as_deref()),
            (2, Some("e2400"))
        );
    }

    #[test]
    fn the_list_of_matches_stays_small() {
        // 겹친 그릇 60개가 저마다 아래 글을 모두 담는 트리: 목록의 글은 잘리고, 고른 요소의 글은 다 있다
        let mut lines = String::new();
        for depth in 0..60 {
            lines.push_str(&format!(
                "{}- region \"r{depth}\" [ref=e{depth}]:\n{}  - text: \"{}\"\n",
                "  ".repeat(depth),
                "  ".repeat(depth),
                "가".repeat(200)
            ));
        }
        let waited = run(
            &Script::new(vec![tree(&lines)]),
            &params(|p| {
                p.role = Some("region".into());
                p.nth = Some(0);
            }),
        )
        .unwrap();
        assert_eq!((waited.count, waited.matches.len()), (60, MATCHES_KEPT));
        assert!(
            waited
                .matches
                .iter()
                .all(|found| found.text.chars().count() < MATCH_TEXT + 20)
        );
        assert!(waited.matches[0].text.ends_with(" chars]"));
        assert!(waited.found.unwrap().text.chars().count() > 60 * 200);
        let size = serde_json::to_string(&waited.matches).unwrap().len();
        assert!(size < 100_000, "{size}");
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

    /// pid 7인 앱(편집기)의 창과 다른 앱 하나가 있는 `apps`의 답
    fn editor(titles: &[&str]) -> Result<Value, Failure> {
        let windows: Vec<Value> = titles
            .iter()
            .map(|title| json!({ "title": title, "active": false }))
            .collect();
        Ok(json!({
            "apps": [
                { "name": "editor", "pid": 7, "windows": windows },
                { "name": "other", "pid": 8, "windows": [{ "title": "다른 저장", "active": true }] },
            ],
            "untracked": [],
        }))
    }

    #[test]
    fn waits_for_windows_of_an_app_or_of_any_app() {
        // 앱 범위: 스냅샷의 맨 위 줄이 그 앱의 보이는 창이다
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
        // 트리가 잘렸으면 앱 목록에서 그 pid(머리줄)의 창을 더한다: 다른 앱의 창은 들지 않는다
        let cut = "- window \"편집기\" [ref=e1]\n# note: cut at 2000 lines; pass a ref to read one part\n";
        let desk = Script::new(vec![
            tree(cut),
            editor(&["편집기"]),
            tree(cut),
            editor(&["편집기", "다른 이름으로 저장"]),
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
        // 잘린 트리에 바쁜 앱(목록에 창이 없다): 닫혔다고 하지 않는다
        let failure = run(
            &Script::new(vec![tree(cut), editor(&[])]),
            &params(|p| {
                p.app = Some("editor".into());
                p.window = Some("저장".into());
                p.gone = true;
                p.timeout_ms = Some(0);
            }),
        )
        .unwrap_err();
        assert!(
            failure.1.contains("; open: \"편집기\"; the tree is longer"),
            "{}",
            failure.1
        );
        // 바빠서 트리를 읽지 못한 앱도 닫혔다고 하지 않는다
        let busy = || -> Result<Value, Failure> {
            Err((
                protocol::AGENT_ERROR,
                "cannot read :1.57: org.freedesktop.DBus.Error.Timeout".into(),
            ))
        };
        let failure = run(
            &Script::new(vec![busy()]),
            &params(|p| {
                p.app = Some("editor".into());
                p.window = Some("편집기".into());
                p.gone = true;
                p.timeout_ms = Some(0);
            }),
        )
        .unwrap_err();
        assert_eq!(failure.0, protocol::TIMEOUT);
        // 앱이 끝나면 그 창도 닫혔다
        let desk = Script::new(vec![
            tree("- window \"편집기\" [ref=e1]\n"),
            Err((
                protocol::AGENT_ERROR,
                "no app matches \"editor\"; see `bunt agent apps`".into(),
            )),
        ]);
        let closed = run(
            &desk,
            &params(|p| {
                p.app = Some("editor".into());
                p.window = Some("편집기".into());
                p.gone = true;
            }),
        )
        .unwrap();
        assert_eq!(closed.done, "the window \"편집기\" is gone");
        // 아직 뜨지 않은 앱의 첫 창도 새 창이다
        let desk = Script::new(vec![
            Err((
                protocol::AGENT_ERROR,
                "no app matches \"editor\"; see `bunt agent apps`".into(),
            )),
            tree("- window \"편집기\" [ref=e1]\n"),
        ]);
        let first = run(
            &desk,
            &params(|p| {
                p.app = Some("editor".into());
                p.window = Some(String::new());
            }),
        )
        .unwrap();
        assert_eq!(first.done, "a new window \"편집기\" appeared");
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

    /// pid가 `pid`인 앱의 스냅샷
    fn tree_of(pid: u32, lines: &str) -> Result<Value, Failure> {
        Ok(json!({
            "tree": format!("- app: \"Mozilla Firefox\" [pid={pid}]\n{lines}"),
            "diff": "",
            "first": false,
        }))
    }

    fn no_firefox() -> Result<Value, Failure> {
        Err((
            protocol::AGENT_ERROR,
            "no app matches \"firefox-esr\"; see `bunt agent apps`".into(),
        ))
    }

    #[test]
    fn a_wait_does_not_follow_a_restarted_app() {
        // 이슈 #1 P2-2: Firefox가 끝나고 다시 떴다. 새 프로세스의 창을 지켜보다 시간이 다 되지 않고 바로 알린다
        let firefox = |pid| tree_of(pid, "- window \"Mozilla Firefox\" [ref=e1]\n");
        let desk = Script::new(vec![firefox(552335), firefox(575616)]);
        let failure = run(
            &desk,
            &params(|p| {
                p.app = Some("firefox-esr".into());
                p.window = Some("mt6368".into());
            }),
        )
        .unwrap_err();
        assert_eq!(failure.0, protocol::TARGET_GONE);
        assert_eq!(
            failure.1,
            "stopped waiting for a window titled \"mt6368\": the app restarted (pid 552335 → \
             575616); take a new snapshot"
        );
        assert_eq!(desk.calls.borrow().len(), 2);
        // 끝나기만 해도 알린다
        let failure = run(
            &Script::new(vec![firefox(552335), no_firefox()]),
            &params(|p| {
                p.app = Some("firefox-esr".into());
                p.text = Some("lore.kernel.org".into());
            }),
        )
        .unwrap_err();
        assert_eq!(failure.0, protocol::TARGET_GONE);
        assert_eq!(
            failure.1,
            "stopped waiting for text \"lore.kernel.org\" in firefox-esr: its app quit (pid \
             552335); take a new snapshot"
        );
        // 아직 뜨지 않은 앱은 뜨기를 기다린다 (그 뒤에 묶는다)
        let desk = Script::new(vec![no_firefox(), firefox(575616)]);
        let waited = run(
            &desk,
            &params(|p| {
                p.app = Some("firefox-esr".into());
                p.window = Some("Firefox".into());
            }),
        )
        .unwrap();
        assert_eq!(waited.window.as_deref(), Some("Mozilla Firefox"));
    }

    #[test]
    fn a_target_that_quits_is_gone_when_waiting_for_it_to_go() {
        // 사라지기를 기다리면 앱이 끝난 것은 사라진 것이다 (다시 뜬 것은 알린다)
        let page = |pid| tree_of(pid, "- dialog \"저장할까요?\" [ref=e7]\n");
        let gone = run(
            &Script::new(vec![page(7), no_firefox()]),
            &params(|p| {
                p.app = Some("firefox-esr".into());
                p.role = Some("dialog".into());
                p.gone = true;
            }),
        )
        .unwrap();
        assert_eq!(gone.done, "dialog in firefox-esr is gone");
        let restarted = run(
            &Script::new(vec![page(7), page(8)]),
            &params(|p| {
                p.app = Some("firefox-esr".into());
                p.role = Some("dialog".into());
                p.gone = true;
            }),
        )
        .unwrap_err();
        assert_eq!(restarted.0, protocol::TARGET_GONE);
    }

    #[test]
    fn a_ref_of_an_app_that_quit_ends_the_wait_at_once() {
        let quit = || -> Result<Value, Failure> {
            Err((
                protocol::AGENT_ERROR,
                format!("e9 is gone: {}; take a new snapshot", protocol::APP_QUIT),
            ))
        };
        let desk = Script::new(vec![quit()]);
        let failure = run(
            &desk,
            &params(|p| {
                p.scope = Some("e9".into());
                p.value = Some("42".into());
            }),
        )
        .unwrap_err();
        assert_eq!(failure.0, protocol::TARGET_GONE);
        assert_eq!(
            failure.1,
            "stopped waiting for e9's value to be \"42\": e9 is gone: its app quit; take a new \
             snapshot"
        );
        assert_eq!(desk.calls.borrow().len(), 1);
        // 데몬이 TARGET_GONE으로 알려도 같다: 사라지기면 사라진 것, 아니면 그대로 끝낸다
        let target_gone = || -> Result<Value, Failure> {
            Err((
                protocol::TARGET_GONE,
                "e9 belongs to an app that restarted (pid 7 → 8); take a new snapshot".into(),
            ))
        };
        let gone = run(
            &Script::new(vec![target_gone()]),
            &params(|p| {
                p.scope = Some("e9".into());
                p.gone = true;
            }),
        )
        .unwrap();
        assert_eq!(gone.done, "e9 is gone");
        let desk = Script::new(vec![target_gone()]);
        let failure = run(&desk, &params(|p| p.scope = Some("e9".into()))).unwrap_err();
        assert_eq!(failure.0, protocol::TARGET_GONE);
        assert!(failure.1.starts_with("e9 belongs to an app that restarted"));
        assert_eq!(desk.calls.borrow().len(), 1);
    }

    /// 불러오는 중인지(busy)를 함께 주는 스냅샷
    fn page(lines: &str, busy: bool) -> Result<Value, Failure> {
        Ok(json!({
            "tree": format!("- app: \"Mozilla Firefox\" [pid=7]\n{lines}"),
            "diff": "",
            "first": false,
            "busy": busy,
        }))
    }

    const WIKIPEDIA: &str =
        "- window \"Linux - Wikipedia\" [ref=e1]:\n  - document \"Linux\" [ref=e2]\n";

    #[test]
    fn loaded_waits_until_the_page_stops_loading() {
        let desk = Script::new(vec![
            page(WIKIPEDIA, true),
            page(WIKIPEDIA, true),
            page(WIKIPEDIA, false),
        ]);
        let waited = run(
            &desk,
            &params(|p| {
                p.app = Some("firefox".into());
                p.loaded = true;
            }),
        )
        .unwrap();
        assert_eq!(waited.done, "the page finished loading in firefox");
        assert_eq!(desk.calls.borrow().len(), 3);
        let idle = run(
            &Script::new(vec![page(WIKIPEDIA, false)]),
            &params(|p| p.loaded = true),
        )
        .unwrap();
        assert_eq!(idle.done, "nothing is loading in the active window");
        let failure = run(
            &Script::new(vec![page(WIKIPEDIA, true)]),
            &params(|p| {
                p.loaded = true;
                p.timeout_ms = Some(150);
            }),
        )
        .unwrap_err();
        assert_eq!(failure.0, protocol::TIMEOUT);
        assert_eq!(
            failure.1,
            "timed out after 150 ms waiting for the page to finish loading in the active window; \
             it is still loading"
        );
    }

    #[test]
    fn a_window_wait_also_waits_for_its_page() {
        // 이슈 #1 P2-3: 창 제목이 바뀌자마자 끝나지 않고, 그 문서가 다 불러오기를 기다린다
        let desk = Script::new(vec![
            page("- window \"Mozilla Firefox\" [ref=e1]\n", true),
            page(WIKIPEDIA, true),
            page(WIKIPEDIA, false),
        ]);
        let waited = run(
            &desk,
            &params(|p| {
                p.app = Some("firefox".into());
                p.window = Some("Linux".into());
            }),
        )
        .unwrap();
        assert_eq!(waited.window.as_deref(), Some("Linux - Wikipedia"));
        assert_eq!(waited.done, "the window \"Linux - Wikipedia\" is open");
        assert_eq!(desk.calls.borrow().len(), 3);
        // 시간이 다 되도록 불러오면 창은 떴다고 알리고 아직 불러온다고 붙인다
        let still = run(
            &Script::new(vec![page(WIKIPEDIA, true)]),
            &params(|p| {
                p.app = Some("firefox".into());
                p.window = Some("Linux".into());
                p.timeout_ms = Some(150);
            }),
        )
        .unwrap();
        assert_eq!(
            still.done,
            "the window \"Linux - Wikipedia\" is open; it is still loading"
        );
        // --loaded와 함께면 실패다
        let failure = run(
            &Script::new(vec![page(WIKIPEDIA, true)]),
            &params(|p| {
                p.app = Some("firefox".into());
                p.window = Some("Linux".into());
                p.loaded = true;
                p.timeout_ms = Some(150);
            }),
        )
        .unwrap_err();
        assert_eq!(failure.0, protocol::TIMEOUT);
        assert!(
            failure
                .1
                .ends_with("to finish loading; it is still loading")
        );
    }

    #[test]
    fn a_window_of_any_app_waits_for_the_page_of_its_app() {
        let apps = |titles: &[&str]| -> Result<Value, Failure> {
            Ok(json!({
                "apps": [{
                    "name": "Firefox",
                    "bus": ":1.57",
                    "pid": 7,
                    "windows": titles.iter().map(|title| json!({ "title": title })).collect::<Vec<_>>(),
                }],
                "untracked": [{ "title": "터미널" }],
            }))
        };
        let desk = Script::new(vec![
            apps(&["Mozilla Firefox"]),
            apps(&["Linux - Wikipedia"]),
            // 그 창을 가진 앱
            apps(&["Linux - Wikipedia"]),
            page(WIKIPEDIA, true),
            page(WIKIPEDIA, false),
        ]);
        let waited = run(&desk, &params(|p| p.window = Some("Linux".into()))).unwrap();
        assert_eq!(waited.window.as_deref(), Some("Linux - Wikipedia"));
        let calls = desk.calls.borrow();
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[3].0, "snapshot");
        assert_eq!(calls[3].1["app"], ":1.57");
        // 그 앱을 읽지 못하면 다 불러왔는지 모르지만 창은 떴다
        let busy_app = || -> Result<Value, Failure> {
            Err((
                protocol::AGENT_ERROR,
                "cannot read :1.57: org.freedesktop.DBus.Error.Timeout".into(),
            ))
        };
        let waited = run(
            &Script::new(vec![
                apps(&["Linux - Wikipedia"]),
                apps(&["Linux - Wikipedia"]),
                busy_app(),
            ]),
            &params(|p| {
                p.window = Some("Linux".into());
                p.timeout_ms = Some(150);
            }),
        )
        .unwrap();
        assert_eq!(
            waited.done,
            "the window \"Linux - Wikipedia\" is open; could not tell whether its page finished \
             loading"
        );
        // 트리가 없는 창은 불러오는지 모른다: 기다리지 않는다
        let desk = Script::new(vec![apps(&[]), apps(&[])]);
        let waited = run(&desk, &params(|p| p.window = Some("터미널".into()))).unwrap();
        assert_eq!(waited.window.as_deref(), Some("터미널"));
        assert_eq!(desk.calls.borrow().len(), 2);
    }

    #[test]
    fn loaded_goes_alone_or_with_a_window() {
        for bad in [
            params(|p| {
                p.loaded = true;
                p.text = Some("x".into());
            }),
            params(|p| {
                p.loaded = true;
                p.gone = true;
            }),
            params(|p| {
                p.loaded = true;
                p.window = Some("w".into());
                p.gone = true;
            }),
            params(|p| {
                p.loaded = true;
                p.scope = Some("e1".into());
                p.changed = true;
            }),
        ] {
            assert_eq!(
                condition(&bad).unwrap_err().0,
                protocol::INVALID_PARAMS,
                "{bad:?}"
            );
        }
        assert_eq!(
            condition(&params(|p| p.loaded = true)).unwrap(),
            Condition::Loaded
        );
    }

    #[test]
    fn the_active_window_may_move_to_another_app() {
        // 활성 창 범위는 묶지 않는다: 포커스가 다른 앱으로 가도 기다리던 것을 찾는다
        let waited = run(
            &Script::new(vec![
                tree_of(7, "- window \"편집기\" [ref=e1]\n"),
                tree_of(
                    8,
                    "- window \"계산기\" [ref=e2]:\n  - button \"42\" [ref=e3]\n",
                ),
            ]),
            &params(|p| p.text = Some("42".into())),
        )
        .unwrap();
        assert_eq!(waited.found.unwrap().reference.as_deref(), Some("e3"));
    }
}
