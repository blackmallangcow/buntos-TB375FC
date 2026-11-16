// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 프로그램의 앱 이름을 .desktop 파일(XDG Desktop Entry)에서 찾는다. 데몬 안에서도 쓰므로 GIO 없이
//! 파일을 직접 읽는다

use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

use crate::identify::{is_interpreter, script_argument};

/// 하위 디렉터리를 얼마나 깊이 찾을지 (applications/kde4/foo.desktop은 ID가 kde4-foo다)
const MAX_DEPTH: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopApp {
    /// .desktop을 뺀 데스크톱 ID
    pub id: String,
    /// 지금 언어의 이름
    pub name: String,
}

/// 이 실행 파일(과 스크립트)을 실행하는 앱. Exec의 프로그램을 PATH에서 찾고 링크를 풀어 맞춘다.
/// 스크립트를 바로 실행하는 Exec(`Exec=claude`가 링크를 풀면 cli.js)도 맞는다. 앱 메뉴에 보이지 않는 항목
/// (NoDisplay, Hidden)은 앱의 이름이 아니다: Claude Code가 까는 URL 처리 항목(`Name=Claude Code URL Handler`,
/// `NoDisplay=true`)이 Claude Code의 이름이 되었다(이슈 #1 P2-1). 맞는 항목이 없으면 부르는 쪽이 실행 파일
/// 이름을 쓴다(identify)
pub fn desktop_app(exe: &Path, script: Option<&Path>) -> Option<DesktopApp> {
    find_app(desktop_files(), &languages(), exe, script)
}

/// 저장된 AI 이름 대신 화면에 보일 이름. bdaf997 전에는 앱 메뉴에 보이지 않는 항목의 이름이 AI 이름이 되어, 그때
/// 허용하거나 요청을 남긴 AI는 그 이름이 정책 파일과 요청 기록에 남았다(이슈 #1 리뷰: `name = "Claude Code URL
/// Handler"`). 정책 파일은 root의 것이라 고쳐 쓰지 않고 보일 때 바꾼다. 저장된 이름이 이 프로그램을 실행하는 숨은
/// 항목의 이름이면 지금의 identify처럼 정한다: 앱 메뉴에 보이는 항목의 이름, 없으면 그 숨은 항목이 부르는 프로그램의
/// 이름(claude). 그 밖에는 저장된 이름 그대로다
pub fn shown_name(name: &str, exe: &Path, script: Option<&Path>) -> String {
    shown_name_in(desktop_files(), &languages(), name, exe, script)
}

fn shown_name_in(
    files: Vec<(String, PathBuf)>,
    languages: &[String],
    name: &str,
    exe: &Path,
    script: Option<&Path>,
) -> String {
    let entries: Vec<Entry> = files
        .into_iter()
        .filter_map(|(_, path)| Entry::parse(&fs::read_to_string(path).ok()?, languages))
        .filter(|entry| {
            (entry.is_application || entry.is_hidden_application) && runs(&entry.exec, exe, script)
        })
        .collect();
    let Some(stale) = entries
        .iter()
        .find(|entry| entry.is_hidden_application && entry.names.iter().any(|known| known == name))
    else {
        return name.to_owned();
    };
    entries
        .iter()
        .find(|entry| entry.is_application)
        .map(|entry| entry.name.clone())
        .or_else(|| exec_name(&stale.exec))
        .unwrap_or_else(|| name.to_owned())
}

/// Exec가 부르는 프로그램의 이름(`"/usr/bin/claude" --handle-uri %u`면 claude). 인터프리터면 스크립트의 이름이다
fn exec_name(exec: &[String]) -> Option<String> {
    let (program, rest) = exec_program(exec)?;
    let path = if is_interpreter(Path::new(program)) {
        Path::new(*rest.iter().find(|argument| !argument.starts_with('-'))?)
    } else {
        Path::new(program)
    };
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

/// `files`(데스크톱 ID, 경로) 가운데 이 실행 파일을 실행하는 첫 앱 항목
fn find_app(
    files: Vec<(String, PathBuf)>,
    languages: &[String],
    exe: &Path,
    script: Option<&Path>,
) -> Option<DesktopApp> {
    files.into_iter().find_map(|(id, path)| {
        let text = fs::read_to_string(path).ok()?;
        let entry = Entry::parse(&text, languages)?;
        (entry.is_application && runs(&entry.exec, exe, script)).then_some(DesktopApp {
            id,
            name: entry.name,
        })
    })
}

/// (데스크톱 ID, 경로). 앞의 디렉터리가 같은 ID를 가린다 (XDG_DATA_HOME, XDG_DATA_DIRS 순서)
fn desktop_files() -> Vec<(String, PathBuf)> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for dir in data_dirs() {
        collect(&dir.join("applications"), "", 0, &mut seen, &mut files);
    }
    files
}

fn collect(
    dir: &Path,
    prefix: &str,
    depth: usize,
    seen: &mut HashSet<String>,
    files: &mut Vec<(String, PathBuf)>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let path = entry.path();
        if path.is_dir() {
            if depth < MAX_DEPTH {
                collect(&path, &format!("{prefix}{name}-"), depth + 1, seen, files);
            }
        } else if let Some(stem) = name.strip_suffix(".desktop") {
            let id = format!("{prefix}{stem}");
            if seen.insert(id.clone()) {
                files.push((id, path));
            }
        }
    }
}

fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match env::var_os("XDG_DATA_HOME").filter(|dir| !dir.is_empty()) {
        Some(dir) => dirs.push(PathBuf::from(dir)),
        None => dirs.extend(env::var_os("HOME").map(|home| Path::new(&home).join(".local/share"))),
    }
    let system = env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    dirs.extend(
        system
            .split(':')
            .filter(|dir| dir.starts_with('/'))
            .map(PathBuf::from),
    );
    dirs
}

/// 이름을 찾을 언어: ko_KR.UTF-8이면 ["ko_KR", "ko"]
fn languages() -> Vec<String> {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| env::var(name).ok())
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    let locale = locale
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .to_owned();
    let mut languages = Vec::new();
    if !locale.is_empty() && locale != "C" && locale != "POSIX" {
        if let Some((language, _)) = locale.split_once('_') {
            languages.push(locale.clone());
            languages.push(language.to_owned());
        } else {
            languages.push(locale);
        }
    }
    languages
}

/// [Desktop Entry] 묶음에서 쓰는 값
struct Entry {
    /// 앱 메뉴에 보이는 앱: Type=Application이고 Hidden·NoDisplay가 아니다 (agent/src/desktop.rs와 같은 판단)
    is_application: bool,
    /// 앱 메뉴에 보이지 않는 앱: Type=Application이고 Hidden이나 NoDisplay다
    is_hidden_application: bool,
    /// 지금 언어의 이름
    name: String,
    /// 모든 언어의 이름 (Name과 Name[…])
    names: Vec<String>,
    exec: Vec<String>,
}

impl Entry {
    fn parse(text: &str, languages: &[String]) -> Option<Entry> {
        let mut in_entry = false;
        let mut kind = None;
        let mut hidden = false;
        let mut no_display = false;
        let mut exec = None;
        let mut name = None;
        let mut names = Vec::new();
        // (languages 안의 순위, 이름)
        let mut localized: Option<(usize, String)> = None;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
                continue;
            }
            if !in_entry || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "Type" => kind = Some(value.to_owned()),
                "Hidden" => hidden = value == "true",
                "NoDisplay" => no_display = value == "true",
                "Exec" => exec = Some(exec_arguments(value)),
                "Name" => {
                    name = Some(unescape_string(value));
                    names.push(unescape_string(value));
                }
                _ => {
                    let Some(language) = key
                        .strip_prefix("Name[")
                        .and_then(|rest| rest.strip_suffix(']'))
                    else {
                        continue;
                    };
                    names.push(unescape_string(value));
                    let language = language.split('@').next().unwrap_or_default();
                    if let Some(rank) = languages.iter().position(|l| l == language)
                        && localized.as_ref().is_none_or(|(best, _)| rank < *best)
                    {
                        localized = Some((rank, unescape_string(value)));
                    }
                }
            }
        }
        let name = localized.map(|(_, name)| name).or(name)?;
        let application = kind.as_deref() == Some("Application");
        Some(Entry {
            is_application: application && !hidden && !no_display,
            is_hidden_application: application && (hidden || no_display),
            name,
            names,
            exec: exec?,
        })
    }
}

/// Exec의 프로그램과 나머지 인자. `env 변수=값 프로그램 …`은 env를 건너뛴다
fn exec_program(exec: &[String]) -> Option<(&str, Vec<&str>)> {
    let mut arguments = exec.iter().map(String::as_str);
    let mut program = arguments.next();
    if program.is_some_and(|program| program == "env" || program.ends_with("/env")) {
        program = arguments.find(|argument| !argument.contains('=') && !argument.starts_with('-'));
    }
    Some((program?, arguments.collect()))
}

/// Exec가 이 프로그램을 실행하는지
fn runs(exec: &[String], exe: &Path, script: Option<&Path>) -> bool {
    let Some((program, rest)) = exec_program(exec) else {
        return false;
    };
    let Some(program) = find_program(program) else {
        return false;
    };
    if script.is_some_and(|script| program == script) {
        return true;
    }
    if program != exe {
        return false;
    }
    let exec_script = if is_interpreter(&program) {
        script_argument(&program, &rest, None).map(|(_, canonical)| canonical)
    } else {
        None
    };
    exec_script.as_deref() == script
}

/// 프로그램 이름을 PATH에서 찾아 링크를 푼 경로를 낸다. 경로면 그 경로를 푼다
pub(crate) fn find_program(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        return Path::new(program)
            .is_absolute()
            .then(|| fs::canonicalize(program).ok())
            .flatten();
    }
    search_path().into_iter().find_map(|dir| {
        let path = dir.join(program);
        path.is_file()
            .then(|| fs::canonicalize(&path).ok())
            .flatten()
    })
}

/// 데몬(사용자 서비스)과 설정 앱의 PATH가 다를 수 있으므로 흔한 디렉터리도 함께 찾는다
fn search_path() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = env::var_os("PATH")
        .map(|path| env::split_paths(&path).collect())
        .unwrap_or_default();
    if let Some(home) = env::var_os("HOME") {
        dirs.push(Path::new(&home).join(".local/bin"));
    }
    dirs.extend(["/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    let mut seen = HashSet::new();
    dirs.retain(|dir| dir.is_absolute() && seen.insert(dir.clone()));
    dirs
}

/// 값의 이스케이프 (\s \n \t \r \\). 다른 이스케이프는 Exec의 따옴표 규칙이 풀도록 그대로 둔다
fn unescape_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => {}
        }
    }
    out
}

/// Exec 값을 인자로 나눈다. 큰따옴표로 묶은 인자와 그 안의 이스케이프를 풀고 %f 같은 필드 코드는 뺀다
fn exec_arguments(value: &str) -> Vec<String> {
    let value = unescape_string(value);
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut has_argument = false;
    let mut quoted = false;
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                has_argument = true;
            }
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if has_argument {
                    arguments.push(std::mem::take(&mut current));
                    has_argument = false;
                }
            }
            c => {
                current.push(c);
                has_argument = true;
            }
        }
    }
    if has_argument {
        arguments.push(current);
    }
    arguments
        .into_iter()
        .filter(|argument| !(argument.len() == 2 && argument.starts_with('%') && argument != "%%"))
        .map(|argument| argument.replace("%%", "%"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_exec_lines() {
        assert_eq!(exec_arguments("foot"), ["foot"]);
        assert_eq!(
            exec_arguments(r#"env FOO=1 "/opt/My App/app" --open %U"#),
            ["env", "FOO=1", "/opt/My App/app", "--open"]
        );
        assert_eq!(
            exec_arguments(r#"sh -c "echo \"hi\" 100%%""#),
            ["sh", "-c", r#"echo "hi" 100%"#]
        );
        // 값의 \s는 따옴표 규칙보다 먼저 풀리므로 인자를 나눈다
        assert_eq!(exec_arguments(r"a\sb c"), ["a", "b", "c"]);
        assert_eq!(exec_arguments(r#""a\\\\b c" d"#), [r"a\b c", "d"]);
    }

    #[test]
    fn picks_the_localized_name() {
        let text = "[Desktop Entry]\nType=Application\nName=Files\nName[ko]=파일\nName[ko_KR]=파일 관리자\nExec=nautilus --new-window %U\n[Desktop Action new]\nName=New\nExec=other\n";
        let ko = Entry::parse(text, &["ko_KR".into(), "ko".into()]).unwrap();
        assert_eq!(ko.name, "파일 관리자");
        assert_eq!(ko.exec, ["nautilus", "--new-window"]);
        let ko = Entry::parse(text, &["ko".into()]).unwrap();
        assert_eq!(ko.name, "파일");
        let en = Entry::parse(text, &[]).unwrap();
        assert_eq!(en.name, "Files");
        assert!(en.is_application);
        let hidden = Entry::parse(
            "[Desktop Entry]\nType=Application\nName=X\nExec=x\nHidden=true",
            &[],
        );
        assert!(!hidden.unwrap().is_application);
        let no_display = |value: &str| {
            Entry::parse(
                &format!("[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay={value}"),
                &[],
            )
            .unwrap()
            .is_application
        };
        assert!(!no_display("true"));
        assert!(no_display("false"));
    }

    /// 이슈 #1 P2-1: Claude Code가 까는 URL 처리 항목은 클라이언트 이름이 아니다
    #[test]
    fn an_entry_hidden_from_menus_does_not_name_the_program() {
        let data = env::temp_dir().join(format!("bunt-access-desktop-{}", std::process::id()));
        let applications = data.join("applications");
        fs::create_dir_all(&applications).unwrap();
        fs::create_dir_all(data.join("bin")).unwrap();
        let program = data.join("bin/claude");
        fs::write(&program, "").unwrap();
        let program = fs::canonicalize(&program).unwrap();
        let files = || {
            let mut files = Vec::new();
            collect(&applications, "", 0, &mut HashSet::new(), &mut files);
            files
        };
        // ~/.local/share/applications/claude-code-url-handler.desktop (Exec만 이 시험의 프로그램으로 바꿨다)
        fs::write(
            applications.join("claude-code-url-handler.desktop"),
            format!(
                "[Desktop Entry]\nName=Claude Code URL Handler\nExec=\"{}\" --handle-uri %u\nType=Application\nNoDisplay=true\nMimeType=x-scheme-handler/claude-cli;\n",
                program.display()
            ),
        )
        .unwrap();
        let found = find_app(files(), &[], &program, None);
        // 맞는 항목이 없으면 identify가 실행 파일 이름(claude)을 쓴다
        assert_eq!(found, None);
        // 앱 메뉴에 보이는 항목이 있으면 그것이 이름이다 (URL 처리 항목이 이름 차례로 앞이어도)
        fs::write(
            applications.join("claude.desktop"),
            format!(
                "[Desktop Entry]\nName=Claude Code\nExec={}\nType=Application\n",
                program.display()
            ),
        )
        .unwrap();
        let found = find_app(files(), &[], &program, None);
        let _ = fs::remove_dir_all(&data);
        assert_eq!(
            found,
            Some(DesktopApp {
                id: "claude".into(),
                name: "Claude Code".into()
            })
        );
    }

    /// 이슈 #1 리뷰: bdaf997 전에 저장된 이름("Claude Code URL Handler")은 지금 규칙의 이름으로 보인다
    #[test]
    fn a_name_stored_from_a_hidden_entry_is_shown_by_the_current_rule() {
        let data = env::temp_dir().join(format!("bunt-access-shown-{}", std::process::id()));
        let applications = data.join("applications");
        fs::create_dir_all(&applications).unwrap();
        fs::create_dir_all(data.join("bin")).unwrap();
        let program = data.join("bin/claude");
        fs::write(&program, "").unwrap();
        let program = fs::canonicalize(&program).unwrap();
        let files = || {
            let mut files = Vec::new();
            collect(&applications, "", 0, &mut HashSet::new(), &mut files);
            files
        };
        fs::write(
            applications.join("claude-code-url-handler.desktop"),
            format!(
                "[Desktop Entry]\nName=Claude Code URL Handler\nName[de]=Claude Code URL-Handler\nExec=\"{}\" --handle-uri %u\nType=Application\nNoDisplay=true\n",
                program.display()
            ),
        )
        .unwrap();
        let shown = |name: &str| shown_name_in(files(), &[], name, &program, None);
        // 숨은 항목의 이름(어느 언어든)이면 그 항목이 부르는 프로그램의 이름이다
        assert_eq!(shown("Claude Code URL Handler"), "claude");
        assert_eq!(shown("Claude Code URL-Handler"), "claude");
        // 다른 이름은 그대로다
        assert_eq!(shown("claude"), "claude");
        assert_eq!(shown("내 AI"), "내 AI");
        // 다른 프로그램을 부르는 숨은 항목의 이름은 바꾸지 않는다
        let other = data.join("bin/other");
        fs::write(&other, "").unwrap();
        let other = fs::canonicalize(&other).unwrap();
        assert_eq!(
            shown_name_in(files(), &[], "Claude Code URL Handler", &other, None),
            "Claude Code URL Handler"
        );
        // 앱 메뉴에 보이는 항목이 있으면 그 이름이다 (identify와 같다)
        fs::write(
            applications.join("claude.desktop"),
            format!(
                "[Desktop Entry]\nName=Claude Code\nExec={}\nType=Application\n",
                program.display()
            ),
        )
        .unwrap();
        let visible = shown("Claude Code URL Handler");
        let _ = fs::remove_dir_all(&data);
        assert_eq!(visible, "Claude Code");
    }

    #[test]
    fn exec_names_its_program_or_script() {
        let exec = |line: &str| exec_name(&exec_arguments(line));
        assert_eq!(
            exec("\"/usr/bin/claude\" --handle-uri %u").as_deref(),
            Some("claude")
        );
        assert_eq!(exec("env A=1 claude %u").as_deref(), Some("claude"));
        assert_eq!(
            exec("node --no-warnings /opt/ai/cli.js %u").as_deref(),
            Some("cli.js")
        );
        assert_eq!(exec("").as_deref(), None);
    }

    #[test]
    fn matches_exec_against_the_resolved_program() {
        let sh = fs::canonicalize("/bin/sh").unwrap();
        assert!(runs(&["sh".into()], &sh, None));
        assert!(runs(
            &["env".into(), "A=1".into(), "/bin/sh".into()],
            &sh,
            None
        ));
        assert!(!runs(&["sh".into()], Path::new("/usr/bin/false"), None));
        assert!(!runs(&["no-such-program-here".into()], &sh, None));
    }
}
