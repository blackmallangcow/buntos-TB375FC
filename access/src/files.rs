// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 파일을 원자적으로 쓴다. 읽는 쪽(에이전트 데몬, 설정 페이지)은 옛 파일이나 새 파일만 본다

use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions, Permissions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

/// 여러 프로세스(에이전트 데몬, 셸)가 같은 파일을 읽고 고쳐 쓰는 동안 잡는 잠금. 파일 옆의
/// `.<이름>.lock`을 잠그고, 돌려준 파일이 닫히면 풀린다. 원자적으로 쓰면 파일이 바뀌므로 파일 자체는
/// 잠그지 않는다
pub fn lock(path: &Path) -> io::Result<File> {
    let (dir, name) = split(path)?;
    fs::create_dir_all(dir)?;
    let mut lock_name = OsString::from(".");
    lock_name.push(name);
    lock_name.push(".lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(dir.join(lock_name))?;
    file.lock()?;
    Ok(file)
}

fn split(path: &Path) -> io::Result<(&Path, &std::ffi::OsStr)> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent directory"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    Ok((dir, name))
}

/// 같은 디렉터리의 임시 파일에 쓰고 디스크에 내린 뒤 이름을 바꿔 덮는다. 디렉터리가 없으면 만든다
pub fn write_atomic(path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
    let (dir, name) = split(path)?;
    fs::create_dir_all(dir)?;
    let mut temporary = name.to_owned();
    temporary.push(format!(".{}.tmp", std::process::id()));
    let temporary = dir.join(temporary);
    // 같은 pid가 남긴 것은 앞서 끊긴 쓰기다
    let _ = fs::remove_file(&temporary);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&temporary)?;
        // umask와 상관없이 정한 권한을 준다
        file.set_permissions(Permissions::from_mode(mode))?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_file_with_the_given_mode() {
        let dir = crate::policy::tests::temp_dir("atomic");
        let path = dir.join("sub").join("file.toml");
        write_atomic(&path, b"one", 0o644).unwrap();
        write_atomic(&path, b"two", 0o600).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // 임시 파일이 남지 않는다
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["file.toml"]);
        fs::remove_dir_all(dir).unwrap();
    }
}
