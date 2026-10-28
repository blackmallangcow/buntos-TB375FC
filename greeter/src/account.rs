// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 계정 규칙: 사람이 쓰는 정규 계정, 이름과 암호 검사, 전체 이름에서 계정 이름 제안,
//! 시험·개발용 자격 증명(buntos.account)

use std::{fmt, io};

pub const PASSWD: &str = "/etc/passwd";
pub const GROUP: &str = "/etc/group";

/// 정규 계정의 uid 범위. Debian login.defs의 UID_MIN..UID_MAX(60000)에서 nobody 근처를 뺀다
pub const UID_MIN: u32 = 1000;
pub const UID_MAX: u32 = 59999;
/// 계정 이름의 최대 길이 (홈 폴더 이름이 된다)
pub const ACCOUNT_NAME_MAX: usize = 32;
/// 암호의 최소 글자 수. 사용자가 나중에 암호를 바꿀 때 pam_unix가 요구하는 길이(minlen 6)와 같다
pub const PASSWORD_MIN: usize = 6;
const PASSWORD_MAX_BYTES: usize = 512;
const FULL_NAME_MAX: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub name: String,
    pub uid: u32,
    /// GECOS의 첫 칸(전체 이름). 없으면 빈 문자열
    pub real_name: String,
    pub home: String,
    pub shell: String,
}

impl User {
    /// 보일 이름: 전체 이름, 없으면 계정 이름
    pub fn display_name(&self) -> &str {
        if self.real_name.trim().is_empty() {
            &self.name
        } else {
            &self.real_name
        }
    }
}

/// /etc/passwd 형식을 읽는다. 형식이 맞지 않는 줄은 건너뛴다
pub fn parse_passwd(text: &str) -> Vec<User> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            let [name, _, uid, _, gecos, home, shell] = fields[..] else {
                return None;
            };
            Some(User {
                name: name.to_owned(),
                uid: uid.parse().ok()?,
                real_name: gecos.split(',').next().unwrap_or_default().to_owned(),
                home: home.to_owned(),
                shell: shell.to_owned(),
            })
        })
        .collect()
}

/// 로그인 셸인지. nologin, false는 로그인하지 않는 계정이다
pub fn is_login_shell(shell: &str) -> bool {
    let program = shell.rsplit('/').next().unwrap_or_default();
    !program.is_empty() && !matches!(program, "nologin" | "false")
}

/// 사람이 로그인하는 정규 계정: uid가 범위 안이고 로그인 셸이 있다
pub fn is_regular(user: &User) -> bool {
    (UID_MIN..=UID_MAX).contains(&user.uid) && is_login_shell(&user.shell)
}

/// 정규 계정들 (uid 순)
pub fn regular_users(passwd: &str) -> Vec<User> {
    let mut users: Vec<User> = parse_passwd(passwd)
        .into_iter()
        .filter(is_regular)
        .collect();
    users.sort_by_key(|user| user.uid);
    users
}

/// 이 시스템의 정규 계정들
pub fn read_regular_users() -> io::Result<Vec<User>> {
    Ok(regular_users(&std::fs::read_to_string(PASSWD)?))
}

/// 사용자나 그룹이 이미 그 이름을 쓰는지. useradd가 같은 이름의 그룹을 함께 만들므로 둘 다 본다
pub fn is_name_taken(name: &str, passwd: &str, group: &str) -> bool {
    let first_field = |line: &str| line.split(':').next() == Some(name);
    passwd.lines().any(first_field) || group.lines().any(first_field)
}

pub fn group_exists(name: &str, group: &str) -> bool {
    group
        .lines()
        .any(|line| line.split(':').next() == Some(name))
}

/// 입력이 규칙에 맞지 않는 까닭. D-Bus 오류 이름과 로그인 화면의 안내로 바뀐다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    FullName,
    AccountName,
    AccountNameTaken,
    PasswordTooShort,
    Password,
}

impl Invalid {
    pub const ALL: [Invalid; 5] = [
        Invalid::FullName,
        Invalid::AccountName,
        Invalid::AccountNameTaken,
        Invalid::PasswordTooShort,
        Invalid::Password,
    ];

    /// D-Bus 오류 이름의 끝 (org.buntos.Setup1.Error.<이것>)
    pub fn error_name(self) -> &'static str {
        match self {
            Self::FullName => "InvalidFullName",
            Self::AccountName => "InvalidAccountName",
            Self::AccountNameTaken => "AccountNameTaken",
            Self::PasswordTooShort => "PasswordTooShort",
            Self::Password => "InvalidPassword",
        }
    }

    pub fn from_error_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|invalid| invalid.error_name() == name)
    }
}

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::FullName => "the full name is empty, too long or contains ':' or ','",
            Self::AccountName => {
                "the account name must start with a-z and contain only a-z, 0-9, '-' and '_'"
            }
            Self::AccountNameTaken => "a user or group already has that name",
            Self::PasswordTooShort => "the password is too short",
            Self::Password => "the password contains a line break or is too long",
        })
    }
}

impl std::error::Error for Invalid {}

/// 전체 이름을 다듬어 돌려준다. GECOS 칸에 들어가므로 ':'와 ','(칸 구분자), 제어 문자를 받지 않는다
pub fn check_full_name(name: &str) -> Result<String, Invalid> {
    let name = name.trim();
    let bad = |c: char| c == ':' || c == ',' || c.is_control();
    if name.is_empty() || name.chars().count() > FULL_NAME_MAX || name.contains(bad) {
        return Err(Invalid::FullName);
    }
    Ok(name.to_owned())
}

/// 영문 소문자로 시작하고 영문 소문자, 숫자, '-', '_'만 쓴다 (Debian useradd의 기본 규칙보다 좁다)
pub fn check_account_name(name: &str) -> Result<(), Invalid> {
    let mut chars = name.chars();
    let starts_well = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    let rest_ok =
        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if starts_well && rest_ok && name.len() <= ACCOUNT_NAME_MAX {
        Ok(())
    } else {
        Err(Invalid::AccountName)
    }
}

/// chpasswd에 한 줄로 넘기므로 줄바꿈과 NUL을 받지 않는다
pub fn check_password(password: &str) -> Result<(), Invalid> {
    if password.chars().count() < PASSWORD_MIN {
        return Err(Invalid::PasswordTooShort);
    }
    if password.len() > PASSWORD_MAX_BYTES || password.contains(['\n', '\r', '\0']) {
        return Err(Invalid::Password);
    }
    Ok(())
}

/// 한글 음절의 초성·중성·종성 로마자 (국어의 로마자 표기법, 음절마다 따로 적는다)
const INITIALS: [&str; 19] = [
    "g", "kk", "n", "d", "tt", "r", "m", "b", "pp", "s", "ss", "", "j", "jj", "ch", "k", "t", "p",
    "h",
];
const MEDIALS: [&str; 21] = [
    "a", "ae", "ya", "yae", "eo", "e", "yeo", "ye", "o", "wa", "wae", "oe", "yo", "u", "wo", "we",
    "wi", "yu", "eu", "ui", "i",
];
const FINALS: [&str; 28] = [
    "", "k", "k", "k", "n", "n", "n", "t", "l", "k", "m", "l", "l", "l", "p", "l", "m", "p", "p",
    "t", "t", "ng", "t", "t", "k", "t", "p", "t",
];
/// 흔한 성씨는 표기법보다 널리 쓰는 철자를 쓴다 (김 gim → kim)
const SURNAMES: [(char, &str); 28] = [
    ('김', "kim"),
    ('이', "lee"),
    ('박', "park"),
    ('최', "choi"),
    ('정', "jung"),
    ('강', "kang"),
    ('조', "cho"),
    ('윤', "yoon"),
    ('임', "lim"),
    ('오', "oh"),
    ('신', "shin"),
    ('권', "kwon"),
    ('안', "ahn"),
    ('유', "yoo"),
    ('고', "ko"),
    ('문', "moon"),
    ('노', "noh"),
    ('심', "shim"),
    ('곽', "kwak"),
    ('성', "sung"),
    ('주', "joo"),
    ('우', "woo"),
    ('구', "koo"),
    ('공', "kong"),
    ('현', "hyun"),
    ('변', "byun"),
    ('엄', "um"),
    ('추', "choo"),
];

fn romanize_syllable(syllable: char) -> Option<String> {
    let code = u32::from(syllable).checked_sub(0xAC00)?;
    if code >= 11172 {
        return None;
    }
    let (initial, medial, last) = (code / 588, code % 588 / 28, code % 28);
    Some(format!(
        "{}{}{}",
        INITIALS[initial as usize], MEDIALS[medial as usize], FINALS[last as usize]
    ))
}

fn is_hangul_syllable(c: char) -> bool {
    ('가'..='힣').contains(&c)
}

/// 전체 이름에서 계정 이름을 제안한다 (macOS 설정 지원처럼). 한글은 로마자로 옮기고 영문은
/// 소문자로 바꾸며, 그 밖의 글자와 띄어쓰기는 뺀다. 예: "홍길동" → "honggildong",
/// "John Appleseed" → "johnappleseed"
pub fn suggest_account_name(full_name: &str) -> String {
    let name = full_name.trim();
    let syllables = name.chars().filter(|c| is_hangul_syllable(*c)).count();
    let mut out = String::new();
    for (index, c) in name.chars().enumerate() {
        if is_hangul_syllable(c) {
            let surname = (index == 0 && syllables >= 2)
                .then(|| SURNAMES.iter().find(|(hangul, _)| *hangul == c))
                .flatten();
            match surname {
                Some((_, latin)) => out.push_str(latin),
                None => out.extend(romanize_syllable(c)),
            }
        } else if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        }
    }
    let start = out
        .find(|c: char| c.is_ascii_lowercase())
        .unwrap_or(out.len());
    out[start..].chars().take(ACCOUNT_NAME_MAX).collect()
}

/// 만들 계정
#[derive(Clone, PartialEq, Eq)]
pub struct NewAccount {
    pub account_name: String,
    pub full_name: String,
    pub password: String,
}

impl fmt::Debug for NewAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 암호는 로그에 남기지 않는다
        f.debug_struct("NewAccount")
            .field("account_name", &self.account_name)
            .field("full_name", &self.full_name)
            .finish_non_exhaustive()
    }
}

/// 시험·개발용 systemd 자격 증명 buntos.account를 읽는다. 형식은 한 줄
/// "계정 이름:전체 이름:암호"다. 전체 이름이 비면 계정 이름을 쓰고, 암호는 두 번째 ':' 뒤 전부라
/// ':'를 넣을 수 있다. 끝의 줄바꿈은 뗀다. 예: "buntos:buntOS:buntos"
pub fn parse_credential(text: &str) -> Result<NewAccount, Invalid> {
    let line = text.trim_end_matches(['\n', '\r']);
    let mut fields = line.splitn(3, ':');
    let (Some(account_name), Some(full_name), Some(password)) =
        (fields.next(), fields.next(), fields.next())
    else {
        return Err(Invalid::AccountName);
    };
    check_account_name(account_name)?;
    let full_name = if full_name.trim().is_empty() {
        account_name.to_owned()
    } else {
        check_full_name(full_name)?
    };
    check_password(password)?;
    Ok(NewAccount {
        account_name: account_name.to_owned(),
        full_name,
        password: password.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD_TEXT: &str = "root:x:0:0:root:/root:/bin/bash
daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin
_greetd:x:990:990:System Greeter:/var/lib/greetd:/usr/sbin/nologin
nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin
kim:x:1001:1001:김철수,,,:/home/kim:/bin/bash
buntos:x:1000:1000:buntOS:/home/buntos:/bin/bash
service:x:1002:1002::/home/service:/bin/false
broken line
";

    #[test]
    fn regular_users_have_a_uid_in_range_and_a_login_shell() {
        let users = regular_users(PASSWD_TEXT);
        let names: Vec<&str> = users.iter().map(|user| user.name.as_str()).collect();
        assert_eq!(names, ["buntos", "kim"]);
        assert_eq!(users[1].real_name, "김철수");
        assert_eq!(users[1].display_name(), "김철수");
        assert!(regular_users("root:x:0:0:root:/root:/bin/bash\n").is_empty());
    }

    #[test]
    fn display_name_falls_back_to_the_account_name() {
        let users = parse_passwd("ann:x:1000:1000::/home/ann:/bin/bash\n");
        assert_eq!(users[0].display_name(), "ann");
    }

    #[test]
    fn users_and_groups_take_names() {
        let group = "sudo:x:27:\nrender:x:991:\n";
        assert!(is_name_taken("buntos", PASSWD_TEXT, group));
        assert!(is_name_taken("sudo", PASSWD_TEXT, group));
        assert!(!is_name_taken("hong", PASSWD_TEXT, group));
        assert!(group_exists("render", group));
        assert!(!group_exists("rend", group));
    }

    #[test]
    fn account_names() {
        for good in [
            "buntos",
            "a",
            "hong-gildong",
            "user_2",
            "abcdefghijklmnopqrstuvwxyz012345",
        ] {
            assert_eq!(check_account_name(good), Ok(()), "{good}");
        }
        for bad in [
            "",
            "Buntos",
            "1user",
            "-user",
            "홍길동",
            "a b",
            "a:b",
            "abcdefghijklmnopqrstuvwxyz0123456",
        ] {
            assert_eq!(check_account_name(bad), Err(Invalid::AccountName), "{bad}");
        }
    }

    #[test]
    fn full_names() {
        assert_eq!(check_full_name("  홍길동 ").as_deref(), Ok("홍길동"));
        assert_eq!(
            check_full_name("John Appleseed").as_deref(),
            Ok("John Appleseed")
        );
        for bad in ["", "   ", "a:b", "Doe, John", "a\nb"] {
            assert_eq!(check_full_name(bad), Err(Invalid::FullName), "{bad:?}");
        }
    }

    #[test]
    fn passwords() {
        assert_eq!(check_password("buntos"), Ok(()));
        assert_eq!(check_password("한글암호여섯"), Ok(()));
        assert_eq!(check_password("short"), Err(Invalid::PasswordTooShort));
        assert_eq!(check_password("line\nbreak"), Err(Invalid::Password));
        assert_eq!(check_password(&"x".repeat(600)), Err(Invalid::Password));
    }

    #[test]
    fn account_names_are_suggested_from_full_names() {
        assert_eq!(suggest_account_name("홍길동"), "honggildong");
        assert_eq!(suggest_account_name("김철수"), "kimcheolsu");
        assert_eq!(suggest_account_name("이 영희"), "leeyeonghui");
        assert_eq!(suggest_account_name("John Appleseed"), "johnappleseed");
        assert_eq!(suggest_account_name("Jöhn O'Neil 3"), "jhnoneil3");
        // 계정 이름은 글자로 시작해야 하므로 앞의 숫자는 뺀다
        assert_eq!(suggest_account_name("2nd user"), "nduser");
        assert_eq!(suggest_account_name("김"), "gim");
        assert_eq!(suggest_account_name("   "), "");
        assert_eq!(
            suggest_account_name(&"가".repeat(40)).len(),
            ACCOUNT_NAME_MAX
        );
        for name in ["홍길동", "John Appleseed", "김", "박지성 Park"] {
            assert_eq!(
                check_account_name(&suggest_account_name(name)),
                Ok(()),
                "{name}"
            );
        }
    }

    #[test]
    fn credentials() {
        let account = parse_credential("buntos:buntOS:buntos\n").unwrap();
        assert_eq!(account.account_name, "buntos");
        assert_eq!(account.full_name, "buntOS");
        assert_eq!(account.password, "buntos");
        // 전체 이름이 비면 계정 이름, 암호에는 ':'를 쓸 수 있다
        let account = parse_credential("dev::pass:word").unwrap();
        assert_eq!(account.full_name, "dev");
        assert_eq!(account.password, "pass:word");
        assert_eq!(parse_credential("buntos:buntos"), Err(Invalid::AccountName));
        assert_eq!(parse_credential("Bad:x:buntos"), Err(Invalid::AccountName));
        assert_eq!(
            parse_credential("ok:x:short"),
            Err(Invalid::PasswordTooShort)
        );
        assert!(!format!("{account:?}").contains("pass:word"));
    }

    #[test]
    fn error_names_round_trip() {
        for invalid in Invalid::ALL {
            assert_eq!(
                Invalid::from_error_name(invalid.error_name()),
                Some(invalid)
            );
        }
        assert_eq!(Invalid::from_error_name("Failed"), None);
    }
}
