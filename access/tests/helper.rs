// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 특권 도우미를 실행해 본다. BUNT_ACCESS_POLICY_FILE로 임시 정책 파일을, BUNT_ACCESS_HOME으로 시험
//! 홈을 준다

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use bunt_access::{Client, Policy};

const HELPER: &str = env!("CARGO_BIN_EXE_bunt-access-helper");

fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("bunt-access-helper-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn helper(policy: &Path, args: &[&str]) -> Output {
    Command::new(HELPER)
        .args(args)
        .env("BUNT_ACCESS_POLICY_FILE", policy)
        .output()
        .unwrap()
}

/// base 아래에 실행 파일을 만든다. base부터 그 디렉터리까지는 umask와 상관없이 0700이다
fn program(base: &Path, relative: &str) -> PathBuf {
    let path = base.join(relative);
    let mut dir = path.parent().unwrap().to_path_buf();
    fs::create_dir_all(&dir).unwrap();
    while dir.starts_with(base) {
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        dir.pop();
    }
    fs::write(&path, b"\x7fELF").unwrap();
    fs::canonicalize(&path).unwrap()
}

#[test]
fn changes_the_policy_file() {
    let dir = temp_dir("changes");
    let policy = dir.join("buntos/agent-access.toml");
    let program = dir.join("agent");
    fs::write(&program, b"\x7fELF").unwrap();
    let program = fs::canonicalize(&program).unwrap();
    let exe = program.to_str().unwrap();

    assert!(
        helper(
            &policy,
            &["block-app", "org.gnome.Nautilus.desktop", "firefox-esr"]
        )
        .status
        .success()
    );
    assert!(
        helper(&policy, &["add-client", "My Agent", exe])
            .status
            .success()
    );
    let read = Policy::load_from(&policy);
    assert_eq!(read.blocked_apps, ["org.gnome.Nautilus", "firefox-esr"]);
    assert_eq!(read.clients.len(), 1);
    assert_eq!(read.clients[0].name, "My Agent");
    assert!(read.clients[0].enabled);

    assert!(helper(&policy, &["disable-client", exe]).status.success());
    assert!(!Policy::load_from(&policy).clients[0].enabled);
    assert!(helper(&policy, &["enable-client", exe]).status.success());
    assert!(Policy::load_from(&policy).clients[0].enabled);
    assert!(
        helper(&policy, &["unblock-app", "firefox-esr"])
            .status
            .success()
    );
    assert!(helper(&policy, &["remove-client", exe]).status.success());
    let read = Policy::load_from(&policy);
    assert!(read.clients.is_empty());
    assert_eq!(read.blocked_apps, ["org.gnome.Nautilus"]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn keeps_updating_ais_in_the_home_allowed() {
    let dir = temp_dir("versions");
    let home = dir.join("home");
    let policy = dir.join("agent-access.toml");
    let add = |exe: &Path| {
        let output = Command::new(HELPER)
            .args(["add-client", "claude", exe.to_str().unwrap()])
            .env("BUNT_ACCESS_POLICY_FILE", &policy)
            .env("BUNT_ACCESS_HOME", &home)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Policy::load_from(&policy)
    };
    let pattern_of = |policy: &Policy, exe: &Path| {
        let client = policy.clients.iter().find(|client| client.exe == exe);
        client.unwrap().exe_pattern.clone()
    };
    let client = |exe: PathBuf| Client {
        name: "claude".into(),
        exe,
        script: None,
        admin: false,
    };

    // Code 탭의 클로드 코드: 판 번호와 해시 디렉터리
    let code = ".config/Claude/claude-code";
    let old = program(&home, &format!("{code}/2.1.308/a93ded4b1b47/claude"));
    let home = fs::canonicalize(&home).unwrap();
    let read = add(&old);
    assert_eq!(
        pattern_of(&read, &old),
        Some(home.join(format!("{code}/*/*/claude")))
    );
    let updated = home.join(format!("{code}/2.1.310/0f3c9a1b2d4e/claude"));
    assert!(read.allows(&client(updated.clone())));
    assert!(!read.allows(&client(home.join("Downloads/claude"))));
    assert!(!read.allows(&client(
        home.join(format!("{code}/2.1.310/0f3c9a1b2d4e/agent"))
    )));

    // 네이티브 CLI: 판 번호가 파일 이름이다
    let native = program(&home, ".local/share/claude/versions/2.1.308");
    let read = add(&native);
    assert_eq!(
        pattern_of(&read, &native),
        Some(home.join(".local/share/claude/versions/*"))
    );
    assert!(read.allows(&client(home.join(".local/share/claude/versions/2.1.310"))));

    // 다른 사용자가 새 판을 만들 수 있는 디렉터리 아래와 홈 밖은 정확히 비교한다
    let shared = program(&home, "shared/1.0/agent");
    fs::set_permissions(home.join("shared"), fs::Permissions::from_mode(0o1777)).unwrap();
    assert_eq!(pattern_of(&add(&shared), &shared), None);
    let outside = program(&dir, "opt/agent/1.0/agent");
    assert_eq!(pattern_of(&add(&outside), &outside), None);

    // 업데이트한 판을 다시 더하면 옛 항목 자리에 한 줄로 합쳐진다
    let updated = program(&home, &format!("{code}/2.1.310/0f3c9a1b2d4e/claude"));
    let read = add(&updated);
    assert_eq!(read.clients.len(), 4);
    assert_eq!(read.clients[0].exe, updated);
    assert_eq!(
        read.clients[0].exe_pattern,
        Some(home.join(format!("{code}/*/*/claude")))
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rejects_bad_input_without_touching_the_file() {
    let dir = temp_dir("bad-input");
    let policy = dir.join("agent-access.toml");
    assert!(
        helper(&policy, &["block-app", "org.buntos.Settings"])
            .status
            .success()
    );
    let before = fs::read_to_string(&policy).unwrap();

    let cases: &[&[&str]] = &[
        &[],
        &["frobnicate"],
        &["block-app"],
        &["block-app", "../../etc/passwd"],
        &["block-app", "ok.App", "-bad"],
        &["add-client", "name", "relative/path"],
        &["add-client", "name", "/usr/bin/../bin/true"],
        &["add-client", "bad\nname", "/usr/bin/true"],
        &["add-client", "missing", "/no/such/program"],
        // 셸은 건너뛰므로 허용할 수 없다
        &["add-client", "shell", "/bin/sh"],
        &["enable-client", "/usr/bin/not-in-the-policy"],
        &["remove-client", "/usr/bin/a", "/usr/bin/b", "/usr/bin/c"],
    ];
    for args in cases {
        let output = helper(&policy, args);
        assert!(!output.status.success(), "{args:?} should fail");
        assert!(!output.stderr.is_empty(), "{args:?} should explain");
        assert_eq!(
            fs::read_to_string(&policy).unwrap(),
            before,
            "{args:?} changed the file"
        );
    }
    // 쓰기 명령이 아닌 잘못된 사용은 2
    assert_eq!(helper(&policy, &["frobnicate"]).status.code(), Some(2));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn refuses_to_run_without_root_on_the_system_policy() {
    let is_root = fs::read_to_string("/proc/self/status").is_ok_and(|status| {
        status
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))
            .and_then(|uids| uids.split_whitespace().nth(1))
            == Some("0")
    });
    if is_root {
        return;
    }
    let output = Command::new(HELPER)
        .args(["block-app", "org.gnome.Nautilus"])
        .env_remove("BUNT_ACCESS_POLICY_FILE")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("root"));
}
