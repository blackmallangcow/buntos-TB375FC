// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 정책 파일 `/etc/buntos/agent-access.toml` (root:root 0644). 형식:
//!
//! ```toml
//! version = 1
//! # AI가 접근할 수 없는 앱: .desktop을 뺀 데스크톱 ID (= Wayland app_id)
//! blocked-apps = ["org.gnome.Nautilus"]
//!
//! # 이 컴퓨터를 제어할 수 있는 AI. exe와 script가 모두 맞아야 같은 클라이언트다
//! [[client]]
//! name = "claude"
//! exe = "/usr/bin/node"
//! script = "/home/buntos/.local/lib/node_modules/@anthropic-ai/claude-code/cli.js"
//! enabled = true
//!
//! # 판 번호·빌드 해시 자리를 *로 바꾼 패턴이 있으면 업데이트한 프로그램도 맞는다
//! [[client]]
//! name = "claude"
//! exe = "/home/buntos/.config/Claude/claude-code/2.1.308/a93ded4b1b47/claude"
//! exe-pattern = "/home/buntos/.config/Claude/claude-code/*/*/claude"
//! ```
//!
//! 경로는 심볼릭 링크를 푼 절대 경로다([`crate::identify`]가 내는 경로와 같다). 패턴(`exe-pattern`,
//! `script-pattern`)은 [`crate::pattern`]을 본다. 특권 도우미가 사용자 홈 아래의 경로에만 만든다. 없으면
//! 경로가 정확히 같아야 한다. 읽을 때 틀린 항목은 버린다. 모르는 판(version)이거나 문법이 틀리면 빈
//! 정책(관리자가 아니면 모두 거절)이다. 패턴을 모르는 옛 판은 패턴을 무시하고 정확히 비교한다.

use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{Client, files, pattern};

pub const POLICY_PATH: &str = "/etc/buntos/agent-access.toml";
/// 이 크레이트가 아는 파일 형식의 판
const VERSION: u32 = 1;
const MAX_NAME: usize = 64;
const MAX_PATH: usize = 4096;
const MAX_APP_ID: usize = 255;

const HEADER: &str = "\
# buntOS AI 에이전트 접근 정책. 시스템 설정 > AI 에이전트에서 바꾼다 (bunt-access-helper가 쓴다).
# [[client]]: 이 컴퓨터를 제어할 수 있는 AI. exe는 실행 파일, script는 인터프리터가 실행한 스크립트다.
#   exe-pattern, script-pattern은 판 번호(2.1.308)나 빌드 해시(a93ded4b1b47) 자리를 *로 바꾼 경로다.
#   업데이트로 그 자리만 바뀐 프로그램도 같은 AI로 본다. 없으면 경로가 정확히 같아야 한다.
# blocked-apps: AI가 접근할 수 없는 앱의 데스크톱 ID (.desktop을 뺀 이름, Wayland app_id와 같다).
";

/// 허용 목록의 AI
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AllowedClient {
    pub name: String,
    pub exe: PathBuf,
    /// 판 번호·빌드 해시 자리를 `*`로 바꾼 exe ([`crate::pattern`]). 업데이트한 실행 파일도 맞는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exe_pattern: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<PathBuf>,
    /// 판 번호·빌드 해시 자리를 `*`로 바꾼 script
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_pattern: Option<PathBuf>,
    /// 끄면 목록에 남지만 허용하지 않는다 (macOS 목록의 스위치)
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

impl AllowedClient {
    /// 정확히 같은 항목인지. 도우미의 빼기·켜기·끄기는 이것으로 항목을 고른다
    fn is(&self, exe: &Path, script: Option<&Path>) -> bool {
        self.exe == exe && self.script.as_deref() == script
    }

    /// 이 실행 파일과 스크립트가 이 항목에 맞는지: 정확히 같거나 패턴이 맞는다
    fn matches(&self, exe: &Path, script: Option<&Path>) -> bool {
        let fits = |exact: &Path, pattern: Option<&Path>, path: &Path| {
            exact == path || pattern.is_some_and(|pattern| pattern::matches(pattern, path))
        };
        fits(&self.exe, self.exe_pattern.as_deref(), exe)
            && match (self.script.as_deref(), script) {
                (None, None) => true,
                (Some(allowed), Some(script)) => {
                    fits(allowed, self.script_pattern.as_deref(), script)
                }
                _ => false,
            }
    }

    /// 같은 AI인지: 한쪽이 다른 쪽의 실행 파일과 스크립트에 맞는다 (패턴으로 맞는 다른 판 포함)
    fn same_client(&self, other: &AllowedClient) -> bool {
        self.matches(&other.exe, other.script.as_deref())
            || other.matches(&self.exe, self.script.as_deref())
    }

    fn is_valid(&self) -> bool {
        valid_name(&self.name)
            && valid_path(&self.exe)
            && self.script.as_deref().is_none_or(valid_path)
            && self
                .exe_pattern
                .as_deref()
                .is_none_or(|pattern| pattern::valid_pattern(pattern, &self.exe))
            && match (self.script.as_deref(), self.script_pattern.as_deref()) {
                (_, None) => true,
                (Some(script), Some(pattern)) => pattern::valid_pattern(pattern, script),
                (None, Some(_)) => false,
            }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    /// 더한 순서
    pub clients: Vec<AllowedClient>,
    pub blocked_apps: Vec<String>,
}

/// 파일 그대로의 모양. 단순 값이 표보다 먼저 와야 TOML로 쓸 수 있다
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct PolicyFile {
    #[serde(default = "current_version")]
    version: u32,
    #[serde(default)]
    blocked_apps: Vec<String>,
    #[serde(default, rename = "client", skip_serializing_if = "Vec::is_empty")]
    clients: Vec<AllowedClient>,
}

fn current_version() -> u32 {
    VERSION
}

#[derive(Debug)]
pub enum PolicyError {
    Syntax(toml::de::Error),
    /// 이 크레이트보다 새 판의 파일
    Version(u32),
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(err) => write!(f, "{err}"),
            Self::Version(version) => write!(f, "unsupported policy version {version}"),
        }
    }
}

impl std::error::Error for PolicyError {}

impl Policy {
    /// 시스템 정책. 파일이 없거나 틀리면 빈 정책이다
    pub fn load() -> Policy {
        Self::load_from(Path::new(POLICY_PATH))
    }

    pub fn load_from(path: &Path) -> Policy {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| Self::parse(&text).ok())
            .unwrap_or_default()
    }

    /// 틀린 항목과 겹친 항목(실행 파일과 스크립트가 정확히 같은 항목)은 버린다
    pub fn parse(text: &str) -> Result<Policy, PolicyError> {
        let file: PolicyFile = toml::from_str(text).map_err(PolicyError::Syntax)?;
        if file.version > VERSION {
            return Err(PolicyError::Version(file.version));
        }
        let mut policy = Policy::default();
        for client in file.clients {
            let seen = policy
                .clients
                .iter()
                .any(|seen| seen.is(&client.exe, client.script.as_deref()));
            if client.is_valid() && !seen {
                policy.clients.push(client);
            }
        }
        for app in file.blocked_apps {
            let app = normalize_app_id(&app);
            if valid_app_id(app) && !policy.blocks(app) {
                policy.blocked_apps.push(app.to_owned());
            }
        }
        Ok(policy)
    }

    pub fn to_toml(&self) -> String {
        let file = PolicyFile {
            version: VERSION,
            blocked_apps: self.blocked_apps.clone(),
            clients: self.clients.clone(),
        };
        let body = toml::to_string(&file).expect("the policy serializes to TOML");
        format!("{HEADER}\n{body}")
    }

    /// 원자적으로 쓴다 (0644). 특권 도우미가 root로 부른다
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        files::write_atomic(path, self.to_toml().as_bytes(), 0o644)
    }

    /// 관리자는 늘 허용한다. 나머지는 실행 파일·스크립트에 맞는 항목([`Policy::find`])이 켜져 있어야 한다
    pub fn allows(&self, client: &Client) -> bool {
        if client.admin {
            return true;
        }
        !client.is_unknown()
            && self
                .find(&client.exe, client.script.as_deref())
                .is_some_and(|allowed| allowed.enabled)
    }

    /// 데스크톱 ID(= Wayland app_id)가 막은 앱인지. 끝의 .desktop은 무시한다
    pub fn blocks(&self, app_id: &str) -> bool {
        let app_id = normalize_app_id(app_id);
        self.blocked_apps.iter().any(|blocked| blocked == app_id)
    }

    /// 이 실행 파일과 스크립트에 적용할 항목: 정확히 같은 항목, 없으면 패턴이 맞는 첫 항목. 관리자가
    /// 정책 파일에 한 판의 정확한 항목을 따로 두면 그 판만 끄거나 켤 수 있다
    pub fn find(&self, exe: &Path, script: Option<&Path>) -> Option<&AllowedClient> {
        self.clients
            .iter()
            .find(|client| client.is(exe, script))
            .or_else(|| {
                self.clients
                    .iter()
                    .find(|client| client.matches(exe, script))
            })
    }

    /// 더하고 켠다. 같은 AI의 항목(정확히 같거나, 한쪽의 패턴이 다른 쪽과 맞는 다른 판)이 있으면 첫
    /// 항목 자리에 새 항목을 두고 나머지는 지운다. 업데이트한 AI를 다시 더하면 한 줄로 합쳐진다
    pub fn add_client(&mut self, client: AllowedClient) {
        let client = AllowedClient {
            enabled: true,
            ..client
        };
        let first = self.clients.iter().position(|old| old.same_client(&client));
        self.clients.retain(|old| !old.same_client(&client));
        // 첫 항목 앞의 항목은 남으므로 자리가 그대로다
        let index = first.unwrap_or(self.clients.len());
        self.clients.insert(index, client);
    }

    /// 지웠으면 참
    pub fn remove_client(&mut self, exe: &Path, script: Option<&Path>) -> bool {
        let before = self.clients.len();
        self.clients.retain(|client| !client.is(exe, script));
        self.clients.len() != before
    }

    /// 그런 항목이 있으면 참
    pub fn set_enabled(&mut self, exe: &Path, script: Option<&Path>, enabled: bool) -> bool {
        match self
            .clients
            .iter_mut()
            .find(|client| client.is(exe, script))
        {
            Some(client) => {
                client.enabled = enabled;
                true
            }
            None => false,
        }
    }

    /// 새로 막았으면 참
    pub fn block_app(&mut self, app_id: &str) -> bool {
        let app_id = normalize_app_id(app_id);
        if self.blocks(app_id) {
            return false;
        }
        self.blocked_apps.push(app_id.to_owned());
        true
    }

    /// 막혀 있었으면 참
    pub fn unblock_app(&mut self, app_id: &str) -> bool {
        let app_id = normalize_app_id(app_id);
        let before = self.blocked_apps.len();
        self.blocked_apps.retain(|blocked| blocked != app_id);
        self.blocked_apps.len() != before
    }
}

/// 보이는 이름: 앞뒤 공백이 없는 1~64자, 제어 문자 없음
pub fn valid_name(name: &str) -> bool {
    let count = name.chars().count();
    name.trim() == name && (1..=MAX_NAME).contains(&count) && !name.chars().any(char::is_control)
}

/// 정리된 절대 경로: UTF-8이고 `.`, `..`, 빈 부분(`//`, 끝의 `/`)과 제어 문자가 없다
pub fn valid_path(path: &Path) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    let Some(relative) = text.strip_prefix('/') else {
        return false;
    };
    text.len() <= MAX_PATH
        && !text.chars().any(char::is_control)
        && relative
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// 데스크톱 ID: 영문자, 숫자와 `.` `_` `-` `+`만 쓰고 `.`이나 `-`로 시작하지 않는다
pub fn valid_app_id(app_id: &str) -> bool {
    !app_id.is_empty()
        && app_id.len() <= MAX_APP_ID
        && !app_id.starts_with(['.', '-'])
        && app_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
}

/// 끝의 `.desktop`을 뺀다
pub fn normalize_app_id(app_id: &str) -> &str {
    app_id.strip_suffix(".desktop").unwrap_or(app_id)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    /// 시험마다 따로 쓰는 빈 디렉터리
    pub(crate) fn temp_dir(name: &str) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "bunt-access-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn client(exe: &str, script: Option<&str>) -> Client {
        Client {
            name: "test".into(),
            exe: exe.into(),
            script: script.map(PathBuf::from),
            admin: false,
        }
    }

    /// 패턴이 없는 항목
    fn allowed(name: &str, exe: &str, script: Option<&str>) -> AllowedClient {
        AllowedClient {
            name: name.into(),
            exe: exe.into(),
            exe_pattern: None,
            script: script.map(PathBuf::from),
            script_pattern: None,
            enabled: true,
        }
    }

    /// Claude 데스크톱 앱 Code 탭의 클로드 코드: 판 번호와 sha256 앞 12자 디렉터리
    const CODE_TAB: &str = "/home/buntos/.config/Claude/claude-code/2.1.308/a93ded4b1b47/claude";
    const CODE_TAB_NEXT: &str =
        "/home/buntos/.config/Claude/claude-code/2.1.310/0f3c9a1b2d4e/claude";
    const CODE_TAB_PATTERN: &str = "/home/buntos/.config/Claude/claude-code/*/*/claude";

    /// 도우미가 홈 아래의 업데이트하는 AI에 패턴을 붙여 쓴 정책
    const VERSIONED: &str = r#"
version = 1

[[client]]
name = "claude"
exe = "/home/buntos/.config/Claude/claude-code/2.1.308/a93ded4b1b47/claude"
exe-pattern = "/home/buntos/.config/Claude/claude-code/*/*/claude"

[[client]]
name = "claude"
exe = "/home/buntos/.local/share/claude/versions/2.1.308"
exe-pattern = "/home/buntos/.local/share/claude/versions/*"

[[client]]
name = "claude"
exe = "/home/buntos/.nvm/versions/node/v22.11.0/bin/node"
exe-pattern = "/home/buntos/.nvm/versions/node/*/bin/node"
script = "/home/buntos/.nvm/versions/node/v22.11.0/lib/node_modules/@anthropic-ai/claude-code/cli.js"
script-pattern = "/home/buntos/.nvm/versions/node/*/lib/node_modules/@anthropic-ai/claude-code/cli.js"
"#;

    const SAMPLE: &str = r#"
version = 1
blocked-apps = ["org.gnome.Nautilus", "firefox-esr.desktop"]

[[client]]
name = "claude"
exe = "/usr/bin/node"
script = "/home/buntos/.local/lib/node_modules/@anthropic-ai/claude-code/cli.js"

[[client]]
name = "aside"
exe = "/home/buntos/.local/bin/aside"
enabled = false
"#;

    #[test]
    fn reads_clients_and_blocked_apps() {
        let policy = Policy::parse(SAMPLE).unwrap();
        assert_eq!(policy.clients.len(), 2);
        assert!(policy.clients[0].enabled, "enabled defaults to true");
        assert!(!policy.clients[1].enabled);
        assert_eq!(policy.blocked_apps, ["org.gnome.Nautilus", "firefox-esr"]);
    }

    #[test]
    fn allows_only_enabled_clients_with_the_same_exe_and_script() {
        let policy = Policy::parse(SAMPLE).unwrap();
        let script = "/home/buntos/.local/lib/node_modules/@anthropic-ai/claude-code/cli.js";
        assert!(policy.allows(&client("/usr/bin/node", Some(script))));
        // 같은 인터프리터의 다른 스크립트, 스크립트 없는 인터프리터
        assert!(!policy.allows(&client("/usr/bin/node", Some("/tmp/other.js"))));
        assert!(!policy.allows(&client("/usr/bin/node", None)));
        // 꺼 둔 항목
        assert!(!policy.allows(&client("/home/buntos/.local/bin/aside", None)));
        assert!(!policy.allows(&client("/usr/bin/foot", None)));
    }

    #[test]
    fn entries_without_patterns_still_need_the_exact_path() {
        // 패턴이 생기기 전에 더한 항목은 그대로 읽고 정확히 비교한다
        let policy = Policy::parse(&format!(
            "[[client]]\nname = \"claude\"\nexe = \"{CODE_TAB}\"\n"
        ))
        .unwrap();
        assert_eq!(policy.clients[0].exe_pattern, None);
        assert!(policy.allows(&client(CODE_TAB, None)));
        assert!(!policy.allows(&client(CODE_TAB_NEXT, None)));
    }

    #[test]
    fn patterns_allow_updated_versions_and_nothing_else() {
        let policy = Policy::parse(VERSIONED).unwrap();
        assert_eq!(policy.clients.len(), 3);
        let nvm = "/home/buntos/.nvm/versions/node";
        let cli = "lib/node_modules/@anthropic-ai/claude-code/cli.js";
        // 판 번호·해시 디렉터리나 판 번호 파일만 바뀐 경로
        for (exe, script) in [
            (CODE_TAB, None),
            (CODE_TAB_NEXT, None),
            ("/home/buntos/.local/share/claude/versions/2.1.310", None),
            (
                &format!("{nvm}/v24.1.0/bin/node"),
                Some(format!("{nvm}/v24.1.0/{cli}")),
            ),
        ] {
            assert!(policy.allows(&client(exe, script.as_deref())), "{exe}");
        }
        // 다른 프로그램, 다른 폴더, 다른 사용자, 판 번호가 아닌 디렉터리, 다른 스크립트
        for (exe, script) in [
            (
                "/home/buntos/.config/Claude/claude-code/2.1.310/0f3c9a1b2d4e/agent",
                None,
            ),
            (
                "/home/buntos/.config/Claude/claude-code/evil/0f3c9a1b2d4e/claude",
                None,
            ),
            ("/home/buntos/Downloads/claude", None),
            (
                "/home/eve/.config/Claude/claude-code/2.1.310/0f3c9a1b2d4e/claude",
                None,
            ),
            ("/home/buntos/.local/share/claude/versions/evil", None),
            (
                "/home/buntos/.local/share/claude/versions/2.1.310.tmp.4242.1",
                None,
            ),
            (&format!("{nvm}/v24.1.0/bin/node"), None),
            (
                &format!("{nvm}/v24.1.0/bin/node"),
                Some("/tmp/other.js".to_owned()),
            ),
            (
                &format!("{nvm}/v24.1.0/bin/node"),
                Some(format!("{nvm}/v24.1.0/lib/node_modules/other/cli.js")),
            ),
            ("/usr/bin/node", Some(format!("{nvm}/v24.1.0/{cli}"))),
        ] {
            assert!(!policy.allows(&client(exe, script.as_deref())), "{exe}");
        }
    }

    #[test]
    fn an_exact_entry_comes_before_a_pattern() {
        // 관리자가 한 판만 꺼 두었다 (패턴 항목 뒤에 있어도 먼저다)
        let policy = Policy::parse(&format!(
            "{VERSIONED}\n[[client]]\nname = \"bad build\"\nexe = \"{CODE_TAB_NEXT}\"\nenabled = false\n"
        ))
        .unwrap();
        assert_eq!(policy.clients.len(), 4);
        assert!(policy.allows(&client(CODE_TAB, None)));
        assert!(!policy.allows(&client(CODE_TAB_NEXT, None)));
        let found = policy.find(Path::new(CODE_TAB_NEXT), None).unwrap();
        assert_eq!(found.name, "bad build");
    }

    #[test]
    fn drops_entries_whose_pattern_does_not_widen_the_path() {
        let entry = |exe: &str, exe_pattern: &str, script: &str, script_pattern: &str| {
            let mut entry = format!("[[client]]\nname = \"x\"\nexe = \"{exe}\"\n");
            for (key, value) in [
                ("exe-pattern", exe_pattern),
                ("script", script),
                ("script-pattern", script_pattern),
            ] {
                if !value.is_empty() {
                    entry.push_str(&format!("{key} = \"{value}\"\n"));
                }
            }
            entry
        };
        let native = "/home/buntos/.local/share/claude/versions/2.1.308";
        let text = [
            // *가 없다, 판 번호가 아닌 자리의 *, 부분만 *, 깊이가 다르다, 상대 경로
            entry(CODE_TAB, CODE_TAB, "", ""),
            entry(
                CODE_TAB,
                "/home/*/.config/Claude/claude-code/*/*/claude",
                "",
                "",
            ),
            entry(
                CODE_TAB,
                "/home/buntos/.config/Claude/claude-code/2.*/*/claude",
                "",
                "",
            ),
            entry(
                CODE_TAB,
                "/home/buntos/.config/Claude/claude-code/*/claude",
                "",
                "",
            ),
            entry(
                CODE_TAB,
                "home/buntos/.config/Claude/claude-code/*/*/claude",
                "",
                "",
            ),
            // 다른 경로의 패턴, 스크립트 없는 스크립트 패턴
            entry(
                CODE_TAB,
                "/home/buntos/.local/share/claude/versions/*",
                "",
                "",
            ),
            entry(
                native,
                "",
                "",
                "/home/buntos/.local/share/claude/versions/*",
            ),
            // 맞는 패턴
            entry(
                native,
                "/home/buntos/.local/share/claude/versions/*",
                "",
                "",
            ),
        ]
        .concat();
        let policy = Policy::parse(&text).unwrap();
        assert_eq!(policy.clients.len(), 1, "{:?}", policy.clients);
        assert_eq!(policy.clients[0].exe, Path::new(native));
    }

    #[test]
    fn admin_is_always_allowed_and_unknown_never() {
        let empty = Policy::default();
        let mut admin = client("/usr/bin/login", None);
        admin.admin = true;
        assert!(empty.allows(&admin));
        let mut unknown = client("", None);
        assert!(!empty.allows(&unknown));
        unknown.admin = true;
        assert!(empty.allows(&unknown));
    }

    #[test]
    fn blocks_desktop_ids_with_or_without_suffix() {
        let policy = Policy::parse(SAMPLE).unwrap();
        assert!(policy.blocks("org.gnome.Nautilus"));
        assert!(policy.blocks("org.gnome.Nautilus.desktop"));
        assert!(policy.blocks("firefox-esr"));
        assert!(!policy.blocks("org.gnome"));
        assert!(!policy.blocks("org.buntos.Settings"));
    }

    #[test]
    fn broken_or_newer_files_are_an_empty_policy() {
        assert!(matches!(
            Policy::parse("version = 2"),
            Err(PolicyError::Version(2))
        ));
        assert!(Policy::parse("[[client]\nname =").is_err());
        let dir = temp_dir("broken");
        let path = dir.join("agent-access.toml");
        fs::write(&path, "blocked-apps = 3").unwrap();
        assert_eq!(Policy::load_from(&path), Policy::default());
        assert_eq!(
            Policy::load_from(&dir.join("missing.toml")),
            Policy::default()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn drops_invalid_and_duplicate_entries() {
        let policy = Policy::parse(
            r#"
blocked-apps = ["ok.App", "../etc/passwd", "-x", "ok.App"]
[[client]]
name = "relative"
exe = "bin/node"
[[client]]
name = "dots"
exe = "/usr/bin/../bin/node"
[[client]]
name = "bad\nname"
exe = "/usr/bin/node"
[[client]]
name = "first"
exe = "/usr/bin/foot"
[[client]]
name = "duplicate"
exe = "/usr/bin/foot"
"#,
        )
        .unwrap();
        assert_eq!(policy.blocked_apps, ["ok.App"]);
        assert_eq!(policy.clients.len(), 1);
        assert_eq!(policy.clients[0].name, "first");
    }

    #[test]
    fn changes_round_trip_through_the_file() {
        let dir = temp_dir("round-trip");
        let path = dir.join("buntos").join("agent-access.toml");
        let mut policy = Policy::default();
        policy.add_client(allowed("claude", "/usr/bin/node", Some("/opt/cli.js")));
        policy.add_client(allowed("foot", "/usr/bin/foot", None));
        policy.add_client(AllowedClient {
            exe_pattern: Some(CODE_TAB_PATTERN.into()),
            ..allowed("claude", CODE_TAB, None)
        });
        assert!(policy.set_enabled(Path::new("/usr/bin/foot"), None, false));
        assert!(!policy.set_enabled(Path::new("/usr/bin/missing"), None, false));
        assert!(policy.block_app("org.gnome.Nautilus.desktop"));
        assert!(!policy.block_app("org.gnome.Nautilus"));
        assert!(policy.block_app("org.buntos.Settings"));
        assert!(policy.unblock_app("org.buntos.Settings"));
        assert!(!policy.unblock_app("org.buntos.Settings"));
        policy.save_to(&path).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# buntOS"), "{text}");
        assert!(
            text.contains(&format!("exe-pattern = \"{CODE_TAB_PATTERN}\"")),
            "{text}"
        );
        // 패턴이 없는 항목은 옛 판과 같은 모양이다
        assert_eq!(text.matches("-pattern = ").count(), 1, "{text}");
        // 빈 목록은 쓰지 않는다
        assert!(!Policy::default().to_toml().contains("client = ["));
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
        let read = Policy::load_from(&path);
        assert_eq!(read, policy);
        assert_eq!(read.blocked_apps, ["org.gnome.Nautilus"]);
        assert!(!read.clients[1].enabled);

        // 다시 더하면 켜지고 이름이 바뀐다
        let mut read = read;
        read.add_client(allowed("Foot", "/usr/bin/foot", None));
        assert_eq!(read.clients.len(), 3);
        assert!(read.clients[1].enabled);
        assert_eq!(read.clients[1].name, "Foot");
        assert!(read.remove_client(Path::new("/usr/bin/node"), Some(Path::new("/opt/cli.js"))));
        assert!(!read.remove_client(Path::new("/usr/bin/node"), None));
        // 빼기·켜기·끄기는 정확한 경로로 항목을 고른다 (패턴에만 맞는 다른 판은 아니다)
        assert!(!read.remove_client(Path::new(CODE_TAB_NEXT), None));
        assert!(!read.set_enabled(Path::new(CODE_TAB_NEXT), None, false));
        assert_eq!(read.clients.len(), 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn adding_an_updated_version_merges_it_with_the_old_entries() {
        let mut policy = Policy::default();
        policy.add_client(allowed("foot", "/usr/bin/foot", None));
        // 패턴이 생기기 전에 더해 둔 판, 꺼 둔 판
        policy.add_client(allowed("claude", CODE_TAB, None));
        let older = "/home/buntos/.config/Claude/claude-code/2.1.304/6bbba6adc37f/claude";
        policy.add_client(allowed("claude", older, None));
        assert!(policy.set_enabled(Path::new(older), None, false));
        policy.add_client(allowed("aside", "/home/buntos/.local/bin/aside", None));
        assert_eq!(policy.clients.len(), 4);

        // 업데이트한 판을 패턴과 함께 더하면 첫 옛 항목 자리에 한 줄로 합쳐지고 켜진다
        policy.add_client(AllowedClient {
            exe_pattern: Some(CODE_TAB_PATTERN.into()),
            ..allowed("claude", CODE_TAB_NEXT, None)
        });
        let names: Vec<&str> = policy.clients.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["foot", "claude", "aside"]);
        let claude = &policy.clients[1];
        assert_eq!(claude.exe, Path::new(CODE_TAB_NEXT));
        assert_eq!(
            claude.exe_pattern.as_deref(),
            Some(Path::new(CODE_TAB_PATTERN))
        );
        assert!(claude.enabled);
        assert!(policy.allows(&client(older, None)));

        // 옛 판을 다시 더해도(다른 프로그램… 등) 한 줄이다. 같은 디렉터리의 다른 프로그램은 따로다
        policy.add_client(AllowedClient {
            exe_pattern: Some(CODE_TAB_PATTERN.into()),
            ..allowed("Claude", older, None)
        });
        assert_eq!(policy.clients.len(), 3);
        assert_eq!(policy.clients[1].name, "Claude");
        let agent = "/home/buntos/.config/Claude/claude-code/2.1.310/0f3c9a1b2d4e/agent";
        policy.add_client(allowed("agent", agent, None));
        assert_eq!(policy.clients.len(), 4);
    }

    #[test]
    fn validates_names_paths_and_app_ids() {
        assert!(valid_name("Claude Code"));
        assert!(valid_name("클로드"));
        assert!(!valid_name(""));
        assert!(!valid_name(" padded"));
        assert!(!valid_name("tab\there"));
        assert!(!valid_name(&"x".repeat(65)));

        assert!(valid_path(Path::new("/usr/bin/node")));
        assert!(!valid_path(Path::new("/")));
        assert!(!valid_path(Path::new("usr/bin/node")));
        assert!(!valid_path(Path::new("/usr//bin/node")));
        assert!(!valid_path(Path::new("/usr/bin/")));
        assert!(!valid_path(Path::new("/usr/./bin")));
        assert!(!valid_path(Path::new("/usr/../etc")));
        assert!(!valid_path(Path::new("/tmp/new\nline")));

        assert!(valid_app_id("org.gnome.Nautilus"));
        assert!(valid_app_id("firefox-esr"));
        assert!(valid_app_id("libreoffice-writer"));
        assert!(!valid_app_id(""));
        assert!(!valid_app_id(".hidden"));
        assert!(!valid_app_id("-flag"));
        assert!(!valid_app_id("a/b"));
        assert!(!valid_app_id("a b"));
        assert_eq!(normalize_app_id("foo.desktop"), "foo");
    }
}
