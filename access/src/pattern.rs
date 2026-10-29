// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 업데이트해도 맞는 경로 패턴. 스스로 업데이트하는 AI는 판마다 새 디렉터리나 파일에 실행 파일을 둔다.
//! - Claude 데스크톱 앱의 Code 탭: `~/.config/Claude/claude-code/2.1.308/a93ded4b1b47/claude`
//!   (판 번호와 실행 파일 sha256의 앞 12자. 옛 판도 남는다)
//! - 네이티브로 설치한 클로드 코드 CLI: `~/.local/share/claude/versions/2.1.308` (판 번호가 파일 이름이다)
//!
//! 패턴은 정확한 경로에서 판 번호나 빌드 해시처럼 보이는 부분을 `*`로 바꾼 것이다
//! (`~/.config/Claude/claude-code/*/*/claude`). `*`는 판 번호나 빌드 해시처럼 보이는 한 부분과만 맞는다.
//! 그래서 다른 디렉터리에 있거나 이름이 다른 프로그램과는 맞지 않는다.
//!
//! 특권 도우미는 사용자 홈 아래의 경로에만 패턴을 만든다. 홈부터 실행 파일이 든 디렉터리까지가 모두 그
//! 사용자나 root의 것이고 그룹·남이 쓸 수 없어야 한다. 그러면 패턴과 맞는 경로를 새로 만들 수 있는 것은
//! 그 사용자와 root뿐이다. 그 사용자는 정확한 경로의 실행 파일도 이미 바꿀 수 있으므로(ARCHITECTURE
//! 6.5의 한계) 패턴이 위협 모델을 넓히지 않는다. /usr 같은 시스템 경로는 꾸러미 관리자가 같은 경로에 새
//! 판을 두므로 정확히 비교한다.

use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use crate::policy::valid_path;

/// 판 번호나 빌드 해시 하나 자리
const ANY_VERSION: &str = "*";
/// 판 번호의 꼬리(1.0.0-beta.1, 22.11.0_1)를 여는 문자
const TAIL: [char; 4] = ['-', '+', '~', '_'];
/// 빌드 해시로 볼 가장 짧은 길이 (git의 짧은 해시)
const MIN_HASH: usize = 7;

/// 사용자(uid)의 홈 아래에 있는 경로(링크를 푼 절대 경로)의 패턴. 홈 아래에서 판 번호나 빌드 해시처럼
/// 보이는 부분을 `*`로 바꾼다. 그런 부분이 없거나 홈 밖이면 None이다. 홈이 그 사용자의 것이 아니거나,
/// 홈부터 경로가 든 디렉터리까지 가운데 그 사용자나 root의 것이 아니거나 그룹·남이 쓸 수 있는
/// 디렉터리가 있어도 None이다 (다른 사용자가 맞는 경로를 만들 수 있다)
pub fn version_pattern(path: &Path, home: &Path, uid: u32) -> Option<PathBuf> {
    let home = fs::canonicalize(home).ok()?;
    let pattern = wildcard_versions(path, &home)?;
    let private = path
        .ancestors()
        .skip(1)
        .take_while(|dir| dir.starts_with(&home))
        .all(|dir| {
            fs::symlink_metadata(dir).is_ok_and(|meta| {
                meta.is_dir()
                    && (meta.uid() == uid || (meta.uid() == 0 && dir != home))
                    && meta.mode() & 0o022 == 0
            })
        });
    private.then_some(pattern)
}

/// 홈 아래 경로에서 판 번호나 빌드 해시처럼 보이는 부분을 `*`로 바꾼다. 바꿀 부분이 없거나 홈 밖이면 None
fn wildcard_versions(path: &Path, home: &Path) -> Option<PathBuf> {
    // 홈이 /인 계정이면 시스템 경로까지 홈 아래가 된다
    home.parent()?;
    let below = path.strip_prefix(home).ok()?.to_str()?;
    // 이름에 *가 든 경로는 패턴으로 적을 수 없다
    if below.is_empty() || !valid_path(path) || path.to_str()?.contains('*') {
        return None;
    }
    let mut any = false;
    let parts: Vec<&str> = below
        .split('/')
        .map(|part| {
            if is_version_like(part) {
                any = true;
                ANY_VERSION
            } else {
                part
            }
        })
        .collect();
    any.then(|| home.join(parts.join("/")))
}

/// 경로가 패턴과 맞는지: 부분 수가 같고, `*` 자리는 판 번호나 빌드 해시처럼 보이며 나머지는 같다
pub(crate) fn matches(pattern: &Path, path: &Path) -> bool {
    let (Some(pattern), Some(path)) = (pattern.to_str(), path.to_str()) else {
        return false;
    };
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    pattern.len() == path.len()
        && pattern
            .iter()
            .zip(&path)
            .all(|(expected, part)| match *expected {
                ANY_VERSION => is_version_like(part),
                expected => expected == *part,
            })
}

/// 정책 파일의 패턴이 그 정확한 경로를 넓힌 것인지: `*`는 한 부분 전체로만 쓰고 하나 이상 있으며,
/// 정확한 경로와 맞는다 (`*` 자리의 정확한 부분도 판 번호나 빌드 해시처럼 보인다)
pub(crate) fn valid_pattern(pattern: &Path, exact: &Path) -> bool {
    let Some(text) = pattern.to_str() else {
        return false;
    };
    text.split('/').any(|part| part == ANY_VERSION)
        && text
            .split('/')
            .all(|part| part == ANY_VERSION || !part.contains('*'))
        && valid_path(pattern)
        && matches(pattern, exact)
}

/// 경로의 한 부분이 판 번호나 빌드 해시처럼 보이는지.
/// - 판 번호: 점으로 이은 숫자 묶음 둘 이상. 앞에 v, 뒤에 -·+·~·_로 여는 꼬리가 붙을 수 있다
///   (2.1.308, v22.11.0, 1.0.0-beta.1, 22.11.0_1)
/// - 빌드 해시: 7자 이상의 소문자 16진수 (a93ded4b1b47). 숫자만 든 해시도 있다: Code 탭의 sha256 앞
///   12자는 280번에 한 번꼴로 숫자뿐이다
///
/// 넓히면 이미 저장한 패턴도 더 많은 경로와 맞고, 좁히면 저장한 패턴이 틀린 항목이 될 수 있다
pub(crate) fn is_version_like(part: &str) -> bool {
    is_version_number(part) || is_build_hash(part)
}

fn is_version_number(part: &str) -> bool {
    let part = part.strip_prefix(['v', 'V']).unwrap_or(part);
    let (number, tail) = part.split_at(part.find(TAIL).unwrap_or(part.len()));
    number.split('.').count() >= 2
        && number
            .split('.')
            .all(|group| !group.is_empty() && group.bytes().all(|byte| byte.is_ascii_digit()))
        && (tail.is_empty()
            || (tail[1..].starts_with(|c: char| c.is_ascii_alphanumeric())
                && tail
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || TAIL.contains(&c))))
}

fn is_build_hash(part: &str) -> bool {
    part.len() >= MIN_HASH
        && part
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::policy::tests::temp_dir;

    const CODE_TAB: &str = "/home/u/.config/Claude/claude-code/2.1.308/a93ded4b1b47/claude";
    const CODE_TAB_PATTERN: &str = "/home/u/.config/Claude/claude-code/*/*/claude";
    const NATIVE: &str = "/home/u/.local/share/claude/versions/2.1.308";
    const NATIVE_PATTERN: &str = "/home/u/.local/share/claude/versions/*";

    fn pattern(path: &str, home: &str) -> Option<String> {
        wildcard_versions(Path::new(path), Path::new(home)).map(|path| path.display().to_string())
    }

    fn matched(pattern: &str, path: &str) -> bool {
        matches(Path::new(pattern), Path::new(path))
    }

    #[test]
    fn knows_version_numbers_and_build_hashes() {
        for part in [
            "2.1.308",
            "1.0",
            "v22.11.0",
            "V1.2",
            "1.0.0-beta.1",
            "2.0.0+20251029",
            "22.11.0_1",
            "3.12.1~rc1",
            "a93ded4b1b47",
            "6bbba6adc37f",
            "0123abc",
            &"0f".repeat(32),
            // 숫자나 글자만 든 해시
            "305841562960",
            "deadbeef",
        ] {
            assert!(is_version_like(part), "{part}");
        }
        for part in [
            "",
            "claude",
            "claude-code",
            "versions",
            "2",
            "v2",
            "2025",
            ".1",
            "1.",
            "1..2",
            "1.2-",
            "1.2-?",
            "v",
            "vv1.2",
            // 클로드 코드가 받는 중인 임시 파일
            "2.1.308.tmp.4242.1",
            // 대문자, 짧은 해시
            "A93DED4B1B47",
            "abc123",
            "x86_64",
            "lib64",
            "python3.12",
            "node-v22.11.0-linux-x64",
            "*",
            "..",
        ] {
            assert!(!is_version_like(part), "{part}");
        }
    }

    #[test]
    fn replaces_version_parts_below_the_home() {
        assert_eq!(
            pattern(CODE_TAB, "/home/u").as_deref(),
            Some(CODE_TAB_PATTERN)
        );
        assert_eq!(pattern(NATIVE, "/home/u").as_deref(), Some(NATIVE_PATTERN));
        // nvm으로 설치한 node와 그 안의 스크립트
        assert_eq!(
            pattern("/home/u/.nvm/versions/node/v22.11.0/bin/node", "/home/u").as_deref(),
            Some("/home/u/.nvm/versions/node/*/bin/node")
        );
        // 판 번호가 없는 경로, 홈 밖, 홈 이름만 같은 다른 디렉터리, 홈 자신
        assert_eq!(pattern("/home/u/.local/bin/agent", "/home/u"), None);
        assert_eq!(pattern("/opt/claude/2.1.308/claude", "/home/u"), None);
        assert_eq!(pattern("/home/u2/1.0/agent", "/home/u"), None);
        assert_eq!(pattern("/home/u", "/home/u"), None);
        // 홈 디렉터리 이름은 판 번호처럼 보여도 그대로 둔다
        assert_eq!(
            pattern("/home/1.0/apps/2.0/agent", "/home/1.0").as_deref(),
            Some("/home/1.0/apps/*/agent")
        );
        // 홈이 /인 계정, 이름에 *가 든 경로, 정리되지 않은 경로
        assert_eq!(pattern("/opt/claude/2.1.308/claude", "/"), None);
        assert_eq!(pattern("/home/u/a*/2.0/agent", "/home/u"), None);
        assert_eq!(pattern("/home/u/x/../2.0/agent", "/home/u"), None);
    }

    #[test]
    fn patterns_match_only_other_versions_of_the_same_program() {
        // 업데이트한 Code 탭의 클로드 코드
        assert!(matched(CODE_TAB_PATTERN, CODE_TAB));
        assert!(matched(
            CODE_TAB_PATTERN,
            "/home/u/.config/Claude/claude-code/2.1.310/0f3c9a1b2d4e/claude"
        ));
        for other in [
            // 이름이 다른 프로그램
            "/home/u/.config/Claude/claude-code/2.1.310/0f3c9a1b2d4e/agent",
            // 판 번호나 해시가 아닌 디렉터리
            "/home/u/.config/Claude/claude-code/evil/0f3c9a1b2d4e/claude",
            "/home/u/.config/Claude/claude-code/2.1.310/tools/claude",
            "/home/u/.config/Claude/claude-code/../0f3c9a1b2d4e/claude",
            // 깊이가 다른 경로
            "/home/u/.config/Claude/claude-code/2.1.310/claude",
            "/home/u/.config/Claude/claude-code/2.1.310/0f3c9a1b2d4e/x/claude",
            // 다른 폴더, 다른 사용자의 홈
            "/home/u/Downloads/claude-code/2.1.310/0f3c9a1b2d4e/claude",
            "/home/v/.config/Claude/claude-code/2.1.310/0f3c9a1b2d4e/claude",
            // 이름이 *인 디렉터리
            CODE_TAB_PATTERN,
        ] {
            assert!(!matched(CODE_TAB_PATTERN, other), "{other}");
        }
        // 네이티브 CLI: 판 번호가 파일 이름이다
        assert!(matched(
            NATIVE_PATTERN,
            "/home/u/.local/share/claude/versions/2.1.310"
        ));
        for other in [
            "/home/u/.local/share/claude/versions/2.1.310.tmp.4242.1",
            "/home/u/.local/share/claude/versions/evil",
            "/home/u/.local/share/claude/versions/2.1.310/claude",
            "/home/u/.local/share/claude/2.1.310",
        ] {
            assert!(!matched(NATIVE_PATTERN, other), "{other}");
        }
    }

    #[test]
    fn stored_patterns_must_widen_their_exact_path() {
        let valid =
            |pattern: &str, exact: &str| valid_pattern(Path::new(pattern), Path::new(exact));
        assert!(valid(CODE_TAB_PATTERN, CODE_TAB));
        assert!(valid(
            "/home/u/.config/Claude/claude-code/2.1.308/*/claude",
            CODE_TAB
        ));
        assert!(valid(NATIVE_PATTERN, NATIVE));
        // *가 없다, 판 번호가 아닌 자리의 *, 부분만 *, 깊이가 다르다, 상대 경로
        assert!(!valid(CODE_TAB, CODE_TAB));
        assert!(!valid("/home/u/*/Claude/claude-code/*/*/claude", CODE_TAB));
        assert!(!valid(
            "/home/u/.config/Claude/claude-code/2.*/*/claude",
            CODE_TAB
        ));
        assert!(!valid(
            "/home/u/.config/Claude/claude-code/*/claude",
            CODE_TAB
        ));
        assert!(!valid("home/u/.local/share/claude/versions/*", NATIVE));
        assert!(!valid("/home/u/.local/share/claude//versions/*", NATIVE));
    }

    #[test]
    fn makes_patterns_only_in_a_private_home() {
        let home = temp_dir("pattern-home");
        let uid = fs::metadata(&home).unwrap().uid();
        let mode = |path: &Path, mode: u32| {
            fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
        };
        let versions = home.join(".local/share/claude/versions");
        fs::create_dir_all(&versions).unwrap();
        let mut dir = versions.clone();
        while dir.starts_with(&home) {
            mode(&dir, 0o755);
            dir.pop();
        }
        let exe = versions.join("2.1.308");
        fs::write(&exe, b"\x7fELF").unwrap();
        let exe = fs::canonicalize(&exe).unwrap();
        let expected = fs::canonicalize(&versions).unwrap().join("*");

        assert_eq!(version_pattern(&exe, &home, uid), Some(expected.clone()));
        // 다른 사용자의 홈
        assert_eq!(version_pattern(&exe, &home, uid + 1), None);
        // 그룹이나 남이 쓸 수 있는 디렉터리가 사이에 있으면 다른 사용자가 맞는 경로를 만들 수 있다
        let share = home.join(".local/share");
        mode(&share, 0o775);
        assert_eq!(version_pattern(&exe, &home, uid), None);
        mode(&share, 0o1777);
        assert_eq!(version_pattern(&exe, &home, uid), None);
        mode(&share, 0o700);
        mode(&home, 0o777);
        assert_eq!(version_pattern(&exe, &home, uid), None);
        mode(&home, 0o700);
        assert_eq!(version_pattern(&exe, &home, uid), Some(expected));
        // 판 번호가 없는 프로그램
        let agent = home.join("agent");
        fs::write(&agent, b"\x7fELF").unwrap();
        assert_eq!(
            version_pattern(&fs::canonicalize(&agent).unwrap(), &home, uid),
            None
        );
        fs::remove_dir_all(home).unwrap();
    }
}
