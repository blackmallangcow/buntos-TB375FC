// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `bunt --version`과 `buntd-agent --version`이 크레이트 판과 빌드 커밋을 낸다 (이슈 #1 P1-2: 기기의 바이너리가
//! 어느 소스인지 알 길이 없었다). 커밋은 빌드할 때의 BUNTOS_COMMIT이고, 없으면 unknown이다.

use std::process::Command;

fn version_of(program: &str, args: &[&str]) -> String {
    let output = Command::new(program).args(args).output().unwrap();
    assert!(output.status.success(), "{program} {args:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn both_programs_print_their_version_and_commit() {
    let commit = option_env!("BUNTOS_COMMIT")
        .filter(|commit| !commit.is_empty())
        .unwrap_or("unknown");
    let version = env!("CARGO_PKG_VERSION");
    let cli = env!("CARGO_BIN_EXE_bunt");
    for args in [&["--version"][..], &["-V"], &["agent", "--version"]] {
        assert_eq!(
            version_of(cli, args),
            format!("bunt {version} (commit {commit})\n")
        );
    }
    assert_eq!(
        version_of(env!("CARGO_BIN_EXE_buntd-agent"), &["--version"]),
        format!("buntd-agent {version} (commit {commit})\n")
    );
}
