// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 접근 요청 기록: 허용되지 않은 AI가 에이전트 데몬에 붙으면 남긴다. 권한과는 상관없고 두 곳이 읽는다.
//! - 셸의 AI 접근 요청 창: 방금 붙은 AI를 한 번 묻는다([`request_to_ask`], [`mark_asked`]). macOS가
//!   손쉬운 사용 권한이 없는 앱에 "이 컴퓨터를 제어하려고 합니다"라고 묻는 것과 같다.
//! - 시스템 설정 > AI 에이전트의 [+]: 더할 AI로 제안한다.
//!
//! 사용자가 쓸 수 있는 `$XDG_STATE_HOME/buntos/agent-requests.toml`(기본 `~/.local/state`, 0600)에
//! 둔다. 데몬과 셸이 함께 고쳐 쓰므로 옆의 잠금 파일을 잡고 쓴다.
//!
//! ```toml
//! [[request]]
//! name = "claude"
//! exe = "/usr/bin/node"
//! script = "/home/buntos/.local/lib/node_modules/@anthropic-ai/claude-code/cli.js"
//! time = 1761706800  # 마지막 요청 (유닉스 시각, 초)
//! asked = 1761706801  # 사용자에게 물은 때. 없으면 아직 묻지 않았다
//! ```

use std::{
    cmp::Reverse,
    env, fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    Client, files,
    policy::{Policy, valid_name, valid_path},
};

/// `$XDG_STATE_HOME` 아래의 경로
pub const REQUESTS_FILE: &str = "buntos/agent-requests.toml";
/// 남기는 개수. 넘으면 오래된 것부터 지운다
const MAX_REQUESTS: usize = 20;
/// 같은 클라이언트가 이 안에 다시 붙으면 파일을 다시 쓰지 않는다 (초)
const QUIET_SECONDS: u64 = 60;
/// 이보다 오래된 요청은 묻지 않는다 (초). AI가 지금 붙으려 할 때만 묻고, 셸이 다시 떠도 지난 요청을
/// 묻지 않는다
const ASK_WITHIN: u64 = 10 * 60;

const HEADER: &str = "\
# AI 에이전트 접근 요청: 허용되지 않은 AI가 에이전트 데몬(buntd-agent)에 붙은 기록이다.
# 셸이 방금 붙은 AI를 한 번 묻고, 시스템 설정 > AI 에이전트의 [+]가 더할 AI로 보여 준다. 지워도 된다.
";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub name: String,
    pub exe: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<PathBuf>,
    /// 마지막으로 요청한 유닉스 시각 (초)
    pub time: u64,
    /// 셸이 사용자에게 허용할지 물은 유닉스 시각 (초). 같은 프로그램은 한 번만 묻는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked: Option<u64>,
}

#[derive(Default, Serialize, Deserialize)]
struct RequestsFile {
    #[serde(default, rename = "request")]
    requests: Vec<Request>,
}

/// `$XDG_STATE_HOME/buntos/agent-requests.toml`. 홈을 모르면 None
pub fn requests_path() -> Option<PathBuf> {
    let state = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| Path::new(&home).join(".local/state")))?;
    Some(state.join(REQUESTS_FILE))
}

/// 허용되지 않은 클라이언트를 남긴다. 실행 파일과 스크립트가 같으면 한 줄로 합쳐 시각과 이름만
/// 바꾼다. 관리자와 알아보지 못한 클라이언트는 남기지 않는다. 쓰지 못해도 조용히 넘어간다
pub fn record_request(client: &Client) {
    if let Some(path) = requests_path() {
        let _ = record_request_in(&path, client, now());
    }
}

/// 새것부터
pub fn requests() -> Vec<Request> {
    requests_path().map(|path| read(&path)).unwrap_or_default()
}

/// 사용자에게 허용할지 물을 요청: 방금(ASK_WITHIN 안에) 붙었고, 아직 묻지 않았고, 정책 목록에 없는
/// (켠 것도 끈 것도 아닌) 가장 새 요청이다. 지워진 프로그램은 묻지 않는다
pub fn request_to_ask(policy: &Policy) -> Option<Request> {
    let now = now();
    requests().into_iter().find(|request| {
        should_ask(request, policy, now)
            && request.exe.is_file()
            && request.script.as_deref().is_none_or(Path::is_file)
    })
}

/// 셸이 이 프로그램을 이미 물었는지. 데몬이 거절할 때 AI에게 사용자가 물음을 받았는지 알린다
pub fn was_asked(exe: &Path, script: Option<&Path>) -> bool {
    requests()
        .iter()
        .any(|request| same(request, exe, script) && request.asked.is_some())
}

/// 물었다고 남겨 같은 프로그램을 다시 묻지 않는다. 쓰지 못해도 조용히 넘어간다
pub fn mark_asked(exe: &Path, script: Option<&Path>) {
    if let Some(path) = requests_path() {
        let _ = mark_asked_in(&path, exe, script, now());
    }
}

fn should_ask(request: &Request, policy: &Policy, now: u64) -> bool {
    request.asked.is_none()
        && now.saturating_sub(request.time) <= ASK_WITHIN
        && policy
            .find(&request.exe, request.script.as_deref())
            .is_none()
}

fn mark_asked_in(path: &Path, exe: &Path, script: Option<&Path>, time: u64) -> io::Result<()> {
    let _lock = files::lock(path)?;
    let mut requests = read(path);
    let Some(request) = requests
        .iter_mut()
        .find(|request| same(request, exe, script))
    else {
        return Ok(());
    };
    request.asked = Some(time);
    write(path, requests)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn read(path: &Path) -> Vec<Request> {
    let Some(file) = fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str::<RequestsFile>(&text).ok())
    else {
        return Vec::new();
    };
    let mut requests: Vec<Request> = Vec::new();
    for request in file.requests {
        let valid = valid_name(&request.name)
            && valid_path(&request.exe)
            && request.script.as_deref().is_none_or(valid_path);
        if valid
            && !requests
                .iter()
                .any(|seen| same(seen, &request.exe, request.script.as_deref()))
        {
            requests.push(request);
        }
    }
    requests.sort_by_key(|request| Reverse(request.time));
    requests
}

fn same(request: &Request, exe: &Path, script: Option<&Path>) -> bool {
    request.exe == exe && request.script.as_deref() == script
}

fn record_request_in(path: &Path, client: &Client, time: u64) -> io::Result<()> {
    if client.admin || client.is_unknown() || !valid_name(&client.name) {
        return Ok(());
    }
    // 데몬의 여러 스레드와 셸(물었다는 표시)이 함께 쓴다
    let _lock = files::lock(path)?;
    let mut requests = read(path);
    let script = client.script.as_deref();
    match requests
        .iter_mut()
        .find(|request| same(request, &client.exe, script))
    {
        Some(request) => {
            if request.name == client.name && time.saturating_sub(request.time) < QUIET_SECONDS {
                return Ok(());
            }
            request.name = client.name.clone();
            request.time = time;
        }
        None => requests.push(Request {
            name: client.name.clone(),
            exe: client.exe.clone(),
            script: client.script.clone(),
            time,
            asked: None,
        }),
    }
    write(path, requests)
}

/// 새것부터 MAX_REQUESTS개만 쓴다. 잠금을 잡고 부른다
fn write(path: &Path, mut requests: Vec<Request>) -> io::Result<()> {
    requests.sort_by_key(|request| Reverse(request.time));
    requests.truncate(MAX_REQUESTS);
    let body = toml::to_string(&RequestsFile { requests }).expect("requests serialize to TOML");
    files::write_atomic(path, format!("{HEADER}\n{body}").as_bytes(), 0o600)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::policy::tests::temp_dir;

    fn client(name: &str, exe: &str, script: Option<&str>) -> Client {
        Client {
            name: name.into(),
            exe: exe.into(),
            script: script.map(PathBuf::from),
            admin: false,
        }
    }

    #[test]
    fn merges_by_exe_and_script_and_keeps_the_latest_time() {
        let dir = temp_dir("requests");
        let path = dir.join("buntos/agent-requests.toml");
        let claude = client("claude", "/usr/bin/node", Some("/opt/claude/cli.js"));
        let other = client("other", "/usr/bin/node", Some("/opt/other.js"));
        record_request_in(&path, &claude, 1000).unwrap();
        record_request_in(&path, &other, 1010).unwrap();
        // 조용한 시간 안의 같은 요청은 다시 쓰지 않는다
        record_request_in(&path, &claude, 1030).unwrap();
        assert_eq!(read(&path)[1].time, 1000);
        record_request_in(&path, &claude, 2000).unwrap();
        let requests = read(&path);
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].name, "claude");
        assert_eq!(requests[0].time, 2000);
        assert_eq!(requests[1].name, "other");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn keeps_at_most_the_newest_requests() {
        let dir = temp_dir("requests-cap");
        let path = dir.join("agent-requests.toml");
        for index in 0..(MAX_REQUESTS as u64 + 5) {
            let program = client("p", &format!("/opt/p{index}"), None);
            record_request_in(&path, &program, 100 + index).unwrap();
        }
        let requests = read(&path);
        assert_eq!(requests.len(), MAX_REQUESTS);
        assert_eq!(
            requests[0].exe,
            Path::new(&format!("/opt/p{}", MAX_REQUESTS + 4))
        );
        assert_eq!(requests.last().unwrap().exe, Path::new("/opt/p5"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn asks_once_for_recent_requests_outside_the_policy() {
        let dir = temp_dir("requests-ask");
        let path = dir.join("agent-requests.toml");
        let claude = client("claude", "/opt/claude/2.1.308/claude", None);
        let allowed = client("aside", "/opt/aside", None);
        let disabled = client("other", "/opt/other", None);
        for program in [&claude, &allowed, &disabled] {
            record_request_in(&path, program, 1000).unwrap();
        }
        let policy = Policy::parse(
            "version = 1\n\
             [[client]]\nname = \"aside\"\nexe = \"/opt/aside\"\n\
             [[client]]\nname = \"other\"\nexe = \"/opt/other\"\nenabled = false\n",
        )
        .unwrap();
        let named = |name: &str| {
            read(&path)
                .into_iter()
                .find(|request| request.name == name)
                .unwrap()
        };
        // 방금 붙은 AI만 묻는다. 목록에 있는 AI(켠 것, 끈 것)와 오래된 요청은 묻지 않는다
        assert!(should_ask(&named("claude"), &policy, 1060));
        assert!(!should_ask(&named("aside"), &policy, 1060));
        assert!(!should_ask(&named("other"), &policy, 1060));
        assert!(!should_ask(&named("claude"), &policy, 1001 + ASK_WITHIN));
        // 물은 뒤로는 묻지 않고, 데몬이 다시 남겨도 물었다는 표시가 남는다
        mark_asked_in(&path, &claude.exe, None, 1001).unwrap();
        record_request_in(&path, &claude, 2000).unwrap();
        let again = named("claude");
        assert_eq!((again.time, again.asked), (2000, Some(1001)));
        assert!(!should_ask(&again, &policy, 2000));
        // 기록에 없는 프로그램은 그대로 둔다
        mark_asked_in(&path, Path::new("/opt/none"), None, 3000).unwrap();
        assert_eq!(read(&path).len(), 3);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn skips_admin_and_unknown_clients() {
        let dir = temp_dir("requests-skip");
        let path = dir.join("agent-requests.toml");
        let mut admin = client("root", "/usr/bin/login", None);
        admin.admin = true;
        record_request_in(&path, &admin, 1).unwrap();
        record_request_in(&path, &client("", "", None), 1).unwrap();
        assert!(!path.exists());
        // 깨진 파일은 빈 목록이다
        fs::write(&path, "[[request]]\nname = 3").unwrap();
        assert!(read(&path).is_empty());
        record_request_in(&path, &client("x", "/opt/x", None), 5).unwrap();
        assert_eq!(read(&path).len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}
