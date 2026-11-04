// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppImage. 설치는 관리자 권한이 필요 없다: ~/Applications/<이름>에 풀고 ~/.local/share/applications에 .desktop을
//! 둔다. 받은 파일을 그대로 실행하지 않고 푸는 까닭은 둘이다. libfuse2를 쓰는 AppImage가 많은데 Debian forky에는
//! libfuse2가 없어 그대로는 뜨지 않고, 설치만으로 받은 프로그램을 실행하지 않아도 된다.
//!
//! AppImage(2형)는 런타임 ELF 뒤에 squashfs를 붙인 파일이다(https://github.com/AppImage/AppImageSpec).
//! 그 자리는 ELF 섹션 표가 끝나는 곳이고, unsquashfs -o로 그 자리부터 읽는다

use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use gtk4::{gio, glib, prelude::*};

use crate::package::{DesktopEntry, Info};

/// 우리가 만든 .desktop에 다는 키: 푼 디렉터리. 이 키가 있는 항목만 AppImage로 알고 지운다
pub const DIR_KEY: &str = "X-BuntOS-AppImage-Dir";
const VERSION_KEY: &str = "X-BuntOS-AppImage-Version";
const SQUASHFS_MAGIC: &[u8; 4] = b"hsqs";
/// .desktop과 아이콘 하나의 크기 상한
const MAX_ENTRY_SIZE: u64 = 4 * 1024 * 1024;

/// 런타임 ELF에서 읽은 것
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Runtime {
    /// Debian 아키텍처 이름
    pub architecture: Option<&'static str>,
    /// squashfs가 시작하는 자리
    pub payload_offset: u64,
}

/// ELF 머리(앞 64바이트)를 읽는다. 섹션 표가 끝나는 곳이 런타임이 끝나는 곳이다 (--appimage-offset과 같다)
pub fn parse_runtime(head: &[u8]) -> Option<Runtime> {
    if head.len() < 64 || !head.starts_with(b"\x7fELF") {
        return None;
    }
    let is_64 = head[4] == 2;
    let little = head[5] == 1;
    let u16_at = |offset: usize| {
        let bytes = [head[offset], head[offset + 1]];
        if little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        }
    };
    let u32_at = |offset: usize| {
        let bytes: [u8; 4] = head[offset..offset + 4].try_into().expect("four bytes");
        if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    };
    let u64_at = |offset: usize| {
        let bytes: [u8; 8] = head[offset..offset + 8].try_into().expect("eight bytes");
        if little {
            u64::from_le_bytes(bytes)
        } else {
            u64::from_be_bytes(bytes)
        }
    };
    let (section_offset, entry_size, entries) = if is_64 {
        (u64_at(40), u16_at(58), u16_at(60))
    } else {
        (u64::from(u32_at(32)), u16_at(46), u16_at(48))
    };
    let architecture = match u16_at(18) {
        3 => Some("i386"),
        40 => Some("armhf"),
        62 => Some("amd64"),
        183 => Some("arm64"),
        _ => None,
    };
    Some(Runtime {
        architecture,
        payload_offset: section_offset.checked_add(u64::from(entry_size) * u64::from(entries))?,
    })
}

fn read_runtime(path: &Path) -> io::Result<Runtime> {
    let mut head = [0u8; 64];
    fs::File::open(path)?.read_exact(&mut head)?;
    parse_runtime(&head).ok_or_else(|| io::Error::other("not an ELF file"))
}

/// squashfs가 그 자리에 있는지. 없으면 1형이나 다른 형식이다
fn has_squashfs(path: &Path, offset: u64) -> io::Result<bool> {
    use std::io::{Seek, SeekFrom};
    let mut file = fs::File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut magic = [0u8; 4];
    match file.read_exact(&mut magic) {
        Ok(()) => Ok(&magic == SQUASHFS_MAGIC),
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        Err(err) => Err(err),
    }
}

/// squashfs 안의 파일 목록 (맨 위 항목만)
fn list_top(path: &Path, offset: u64) -> io::Result<Vec<String>> {
    let output = Command::new("unsquashfs")
        .arg("-o")
        .arg(offset.to_string())
        .args(["-d", "", "-lc"])
        .arg(path)
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(top_level_names(&String::from_utf8_lossy(&output.stdout)))
}

/// unsquashfs -lc의 줄(`/foo.desktop` 또는 `squashfs-root/foo.desktop`)에서 맨 위의 이름만 뽑는다
fn top_level_names(listing: &str) -> Vec<String> {
    listing
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let line = line
                .strip_prefix("squashfs-root/")
                .or_else(|| line.strip_prefix('/'))
                .unwrap_or(line);
            (!line.is_empty() && line != "squashfs-root" && !line.contains('/'))
                .then(|| line.to_owned())
        })
        .collect()
}

/// squashfs 안의 파일 하나를 메모리로 읽는다. 크기를 묶으므로 받은 파일이 크다고 메모리를 다 쓰지 않는다
fn cat(path: &Path, offset: u64, name: &str) -> Option<Vec<u8>> {
    let mut child = Command::new("unsquashfs")
        .arg("-o")
        .arg(offset.to_string())
        .arg("-cat")
        .arg(path)
        .arg(name)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut data = Vec::new();
    let stdout = child.stdout.take()?;
    let read = stdout.take(MAX_ENTRY_SIZE + 1).read_to_end(&mut data);
    let _ = child.kill();
    let _ = child.wait();
    read.ok()?;
    (!data.is_empty() && data.len() as u64 <= MAX_ENTRY_SIZE).then_some(data)
}

/// 맨 위의 .desktop과 아이콘을 읽는다. 읽지 못해도 파일 이름으로 설치한다
pub fn inspect(path: &Path) -> io::Result<Info> {
    let runtime = read_runtime(path)?;
    let mut info = Info {
        title: title_from_file_name(path),
        architecture: runtime.architecture.map(str::to_owned),
        ..Info::default()
    };
    info.id = desktop_id_from_name(&info.title);
    if has_squashfs(path, runtime.payload_offset)? {
        match list_top(path, runtime.payload_offset) {
            Ok(names) => read_metadata(path, runtime.payload_offset, &names, &mut info),
            Err(err) => eprintln!("bunt-packages: could not list {}: {err}", path.display()),
        }
    } else {
        // 1형(ISO 9660)과 DwarFS 같은 다른 형식은 받은 프로그램을 실행해야 풀 수 있다
        info.blocked = Some("installer-appimage-format");
    }
    Ok(info)
}

fn read_metadata(path: &Path, offset: u64, names: &[String], info: &mut Info) {
    let Some(desktop_name) = names.iter().find(|name| name.ends_with(".desktop")) else {
        return;
    };
    let Some(data) = cat(path, offset, desktop_name) else {
        return;
    };
    let Some(entry) = DesktopEntry::parse(&data) else {
        return;
    };
    info.desktop_file = Some(desktop_name.clone());
    info.id = desktop_name
        .strip_suffix(".desktop")
        .unwrap_or(desktop_name)
        .to_owned();
    if let Some(name) = entry.name {
        info.title = name;
    }
    info.summary = entry.comment;
    info.version = version_from_desktop(&data);
    if let Some(icon) = entry.icon {
        let candidates = [
            format!("{icon}.svg"),
            format!("{icon}.png"),
            ".DirIcon".to_owned(),
        ];
        info.icon = candidates
            .iter()
            .filter(|name| names.contains(name))
            .find_map(|name| cat(path, offset, name));
    }
}

fn version_from_desktop(data: &[u8]) -> Option<String> {
    let keys = glib::KeyFile::new();
    keys.load_from_bytes(&glib::Bytes::from(data), glib::KeyFileFlags::NONE)
        .ok()?;
    keys.string("Desktop Entry", "X-AppImage-Version")
        .ok()
        .map(|value| value.to_string())
        .filter(|value| !value.is_empty())
}

/// "Foo-1.2.3-aarch64.AppImage"에서 뒤의 아키텍처와 확장자를 뗀 이름 (안에 .desktop이 없을 때 쓴다)
fn title_from_file_name(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = strip_suffix_ignore_case(&name, ".appimage").unwrap_or(&name);
    let name = [
        "-aarch64", "-arm64", "-x86_64", "-amd64", "_aarch64", "_x86_64",
    ]
    .iter()
    .find_map(|suffix| strip_suffix_ignore_case(name, suffix))
    .unwrap_or(name);
    name.to_owned()
}

fn strip_suffix_ignore_case<'a>(text: &'a str, suffix: &str) -> Option<&'a str> {
    let split = text.len().checked_sub(suffix.len())?;
    (text.is_char_boundary(split) && text[split..].eq_ignore_ascii_case(suffix))
        .then(|| &text[..split])
}

/// 데스크톱 ID로 쓸 수 있는 글자만 남긴다
pub fn desktop_id_from_name(name: &str) -> String {
    let id: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let id = id.trim_matches(['-', '.']).to_owned();
    if id.is_empty() {
        "appimage".to_owned()
    } else {
        id
    }
}

/// ~/Applications 아래 디렉터리 이름으로 쓸 수 있게 고친다. 경로를 벗어나지 못한다
pub fn directory_name(title: &str) -> String {
    let name: String = title
        .chars()
        .map(|c| if c == '/' || c.is_control() { '-' } else { c })
        .take(80)
        .collect();
    let name = name.trim().trim_start_matches('.').trim().to_owned();
    if name.is_empty() {
        "AppImage".to_owned()
    } else {
        name
    }
}

/// 앱을 푸는 곳: ~/Applications (macOS의 사용자 응용 프로그램 폴더와 같은 이름)
pub fn applications_dir() -> PathBuf {
    glib::home_dir().join("Applications")
}

fn desktop_dir() -> PathBuf {
    glib::user_data_dir().join("applications")
}

/// 앱의 .desktop 파일 경로
fn desktop_path(id: &str) -> PathBuf {
    desktop_dir().join(format!("{id}.desktop"))
}

/// 설치된 AppImage 앱
#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    pub desktop: PathBuf,
    pub dir: PathBuf,
    pub version: Option<String>,
}

/// ID로 이미 설치한 앱을 찾는다
pub fn find_installed(id: &str) -> Option<Installed> {
    read_installed(&desktop_path(id))
        .or_else(|| read_installed(&desktop_path(&format!("appimage-{id}"))))
}

/// applications 바로 아래의 디렉터리인가. "~/Applications/.."도 부모는 같으므로 마지막 이름까지 본다
fn directly_under(applications: &Path, dir: &Path) -> bool {
    dir.parent() == Some(applications) && dir.file_name().is_some()
}

/// 우리가 만든 .desktop이면 읽는다. 디렉터리가 ~/Applications 바로 아래가 아니면 믿지 않는다
fn read_installed(desktop: &Path) -> Option<Installed> {
    let keys = glib::KeyFile::new();
    keys.load_from_file(desktop, glib::KeyFileFlags::NONE)
        .ok()?;
    let dir = keys.string("Desktop Entry", DIR_KEY).ok()?;
    let dir = PathBuf::from(dir.as_str());
    if !directly_under(&applications_dir(), &dir) {
        return None;
    }
    Some(Installed {
        desktop: desktop.to_owned(),
        dir,
        version: keys
            .string("Desktop Entry", VERSION_KEY)
            .ok()
            .map(|value| value.to_string()),
    })
}

/// 설치된 AppImage 앱을 모두 찾는다
pub fn list_installed() -> Vec<Installed> {
    let Ok(entries) = fs::read_dir(desktop_dir()) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "desktop"))
        .filter_map(|path| read_installed(&path))
        .collect()
}

/// Exec의 첫 낱말(프로그램)을 푼 곳의 AppRun으로 바꾸고 인자는 그대로 둔다. %U 같은 필드 코드도 남는다. 이 앱의 Exec에
/// 다른 프로그램을 적었어도 AppRun만 실행한다. AppImage 런타임이 AppRun에 주는 APPDIR을 대신 준다(AppRun 스크립트가 많이
/// 쓴다)
pub fn rewrite_exec(original: Option<&str>, dir: &Path) -> String {
    let arguments = original
        .and_then(|exec| split_first_word(exec.trim()))
        .unwrap_or("");
    let dir_text = dir.to_string_lossy();
    let app_dir = quote_exec(&format!("APPDIR={dir_text}"));
    let program = quote_exec(&dir.join("AppRun").to_string_lossy());
    if arguments.is_empty() {
        format!("env {app_dir} {program}")
    } else {
        format!("env {app_dir} {program} {arguments}")
    }
}

/// 따옴표를 고려해 첫 낱말을 떼고 나머지를 돌려준다
fn split_first_word(exec: &str) -> Option<&str> {
    let mut in_quotes = false;
    let mut escaped = false;
    for (index, c) in exec.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => in_quotes = !in_quotes,
            ' ' if !in_quotes => return Some(exec[index..].trim_start()),
            _ => {}
        }
    }
    None
}

/// 데스크톱 항목 명세의 Exec 따옴표 규칙
fn quote_exec(argument: &str) -> String {
    let mut quoted = String::from("\"");
    for c in argument.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted
}

/// 푼 앱의 .desktop 내용을 만든다. 받은 항목에서 필요한 키만 옮기므로 파일이 자기 키를 넣어 다른 디렉터리를
/// 지우게 하거나 다른 프로그램을 실행하게 하지 못한다
pub fn desktop_contents(
    source: &DesktopEntry,
    source_data: Option<&[u8]>,
    title: &str,
    version: Option<&str>,
    dir: &Path,
    icon: Option<&Path>,
) -> String {
    const GROUP: &str = "Desktop Entry";
    let keys = glib::KeyFile::new();
    keys.set_string(GROUP, "Type", "Application");
    keys.set_string(GROUP, "Name", title);
    if let Some(comment) = &source.comment {
        keys.set_string(GROUP, "Comment", comment);
    }
    // 이름과 설명의 번역, 분류 같은 것을 옮긴다
    if let Some(data) = source_data {
        let original = glib::KeyFile::new();
        // 번역을 함께 옮기려면 이 표시가 있어야 한다 (없으면 지금 언어의 값만 남는다)
        if original
            .load_from_bytes(
                &glib::Bytes::from(data),
                glib::KeyFileFlags::KEEP_TRANSLATIONS,
            )
            .is_ok()
            && let Ok(names) = original.keys(GROUP)
        {
            for key in names.iter().map(|key| key.to_string()) {
                let copied = key.starts_with("Name[")
                    || key.starts_with("Comment[")
                    || key.starts_with("Keywords")
                    || matches!(
                        key.as_str(),
                        "Categories" | "StartupWMClass" | "MimeType" | "Terminal" | "StartupNotify"
                    );
                if copied && let Ok(value) = original.string(GROUP, &key) {
                    keys.set_string(GROUP, &key, &value);
                }
            }
        }
    }
    keys.set_string(GROUP, "Exec", &rewrite_exec(source.exec.as_deref(), dir));
    if let Some(icon) = icon {
        keys.set_string(GROUP, "Icon", &icon.to_string_lossy());
    }
    keys.set_string(GROUP, DIR_KEY, &dir.to_string_lossy());
    if let Some(version) = version {
        keys.set_string(GROUP, VERSION_KEY, version);
    }
    keys.to_data().to_string()
}

/// 푼 디렉터리에서 아이콘을 찾는다. 안의 링크가 디렉터리 밖을 가리키면 쓰지 않는다
pub fn find_icon(dir: &Path, icon: Option<&str>) -> Option<PathBuf> {
    let root = fs::canonicalize(dir).ok()?;
    let mut candidates = Vec::new();
    if let Some(icon) = icon.filter(|icon| !icon.contains('/')) {
        // AppImage 규격: usr/share/icons/hicolor를 맨 위의 아이콘보다 먼저 본다
        let hicolor = "usr/share/icons/hicolor";
        candidates.push(format!("{hicolor}/scalable/apps/{icon}.svg"));
        for size in ["512x512", "256x256", "128x128"] {
            candidates.push(format!("{hicolor}/{size}/apps/{icon}.png"));
        }
        candidates.push(format!("{icon}.svg"));
        candidates.push(format!("{icon}.png"));
    }
    candidates.push(".DirIcon".to_owned());
    candidates.into_iter().find_map(|name| {
        let path = fs::canonicalize(dir.join(name)).ok()?;
        (path.starts_with(&root) && path.is_file()).then_some(path)
    })
}

/// 같은 앱의 옛 판이 있으면 그 자리, 없으면 비어 있는 이름을 고른다. 사용자가 직접 둔 폴더는 덮어쓰지 않는다
fn choose_target(applications: &Path, title: &str, existing: Option<&Installed>) -> PathBuf {
    if let Some(existing) = existing {
        return existing.dir.clone();
    }
    let name = directory_name(title);
    let mut target = applications.join(&name);
    let mut number = 2;
    while target.exists() {
        target = applications.join(format!("{name} {number}"));
        number += 1;
    }
    target
}

/// 이 앱의 .desktop 경로. 같은 이름의 다른 항목(사용자가 직접 만든 것)은 덮어쓰지 않는다
fn choose_desktop(id: &str, existing: Option<&Installed>) -> PathBuf {
    if let Some(existing) = existing {
        return existing.desktop.clone();
    }
    let plain = desktop_path(id);
    if plain.exists() {
        desktop_path(&format!("appimage-{id}"))
    } else {
        plain
    }
}

/// 진행 상황을 줄마다 받으며 푼다. 다 풀면 ~/Applications/<이름>으로 옮기고 .desktop을 쓴다. 같은 앱의 옛 판은
/// 다 푼 뒤에 바꿔 놓으므로 도중에 실패해도 옛 판이 남는다
pub async fn install(
    path: &Path,
    info: &Info,
    on_percent: impl Fn(u32) + 'static,
) -> Result<Installed, String> {
    let runtime = read_runtime(path).map_err(|err| err.to_string())?;
    let applications = applications_dir();
    fs::create_dir_all(&applications).map_err(|err| err.to_string())?;
    let existing = find_installed(&info.id);
    let target = choose_target(&applications, &info.title, existing.as_ref());
    let desktop = choose_desktop(&info.id, existing.as_ref());
    // unsquashfs는 -d의 디렉터리를 스스로 만든다
    let partial = applications.join(format!(
        ".{}.installing",
        target
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    if partial.exists() {
        fs::remove_dir_all(&partial).map_err(|err| err.to_string())?;
    }
    unpack(path, runtime.payload_offset, &partial, on_percent).await?;
    if !partial.join("AppRun").exists() {
        let _ = fs::remove_dir_all(&partial);
        return Err("no AppRun in the AppImage".to_owned());
    }
    finish(&partial, &target, &desktop, info).map_err(|err| {
        let _ = fs::remove_dir_all(&partial);
        err.to_string()
    })
}

async fn unpack(
    path: &Path,
    offset: u64,
    target: &Path,
    on_percent: impl Fn(u32) + 'static,
) -> Result<(), String> {
    let argv = [
        std::ffi::OsStr::new("unsquashfs"),
        std::ffi::OsStr::new("-o"),
        std::ffi::OsStr::new(&offset.to_string()),
        std::ffi::OsStr::new("-no-xattrs"),
        std::ffi::OsStr::new("-percentage"),
        std::ffi::OsStr::new("-d"),
        target.as_os_str(),
        path.as_os_str(),
    ]
    .map(std::ffi::OsString::from);
    let argv: Vec<&std::ffi::OsStr> = argv.iter().map(|arg| arg.as_os_str()).collect();
    let process = gio::Subprocess::newv(
        &argv,
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_MERGE,
    )
    .map_err(|err| err.to_string())?;
    let stdout = process.stdout_pipe().ok_or("no stdout")?;
    let lines = gio::DataInputStream::new(&stdout);
    let mut last_error = String::new();
    while let Ok(Some(line)) = lines.read_line_utf8_future(glib::Priority::DEFAULT).await {
        let line = line.trim();
        if let Ok(percent) = line.parse::<u32>() {
            on_percent(percent.min(100));
        } else if !line.is_empty() {
            last_error = line.to_owned();
        }
    }
    process.wait_check_future().await.map_err(|err| {
        if last_error.is_empty() {
            err.to_string()
        } else {
            last_error
        }
    })
}

fn finish(partial: &Path, target: &Path, desktop: &Path, info: &Info) -> io::Result<Installed> {
    // 옛 판을 치운다 (choose_target이 이 앱의 자리만 골랐다)
    if target.exists() {
        fs::remove_dir_all(target)?;
    }
    fs::rename(partial, target)?;
    let (source, data) = match info
        .desktop_file
        .as_ref()
        .and_then(|name| fs::read(target.join(name)).ok())
    {
        Some(data) => (DesktopEntry::parse(&data).unwrap_or_default(), Some(data)),
        None => (DesktopEntry::default(), None),
    };
    let icon = find_icon(target, source.icon.as_deref());
    let contents = desktop_contents(
        &source,
        data.as_deref(),
        &info.title,
        info.version.as_deref(),
        target,
        icon.as_deref(),
    );
    fs::create_dir_all(desktop_dir())?;
    let temporary = desktop.with_extension("desktop.tmp");
    fs::write(&temporary, contents)?;
    fs::rename(&temporary, desktop)?;
    Ok(Installed {
        desktop: desktop.to_owned(),
        dir: target.to_owned(),
        version: info.version.clone(),
    })
}

/// 앱을 지운다. 앱이 만든 설정(~/.config)은 macOS처럼 남긴다
pub fn uninstall(installed: &Installed) -> io::Result<()> {
    fs::remove_file(&installed.desktop)?;
    if directly_under(&applications_dir(), &installed.dir) && installed.dir.exists() {
        fs::remove_dir_all(&installed.dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elf64(machine: u16, section_offset: u64, entry_size: u16, entries: u16) -> Vec<u8> {
        let mut head = vec![0u8; 64];
        head[..4].copy_from_slice(b"\x7fELF");
        head[4] = 2;
        head[5] = 1;
        head[8..11].copy_from_slice(b"AI\x02");
        head[18..20].copy_from_slice(&machine.to_le_bytes());
        head[40..48].copy_from_slice(&section_offset.to_le_bytes());
        head[58..60].copy_from_slice(&entry_size.to_le_bytes());
        head[60..62].copy_from_slice(&entries.to_le_bytes());
        head
    }

    #[test]
    fn finds_the_payload_after_the_section_table() {
        let runtime = parse_runtime(&elf64(183, 1000, 64, 10)).unwrap();
        assert_eq!(runtime.architecture, Some("arm64"));
        assert_eq!(runtime.payload_offset, 1640);
        assert_eq!(
            parse_runtime(&elf64(62, 0, 0, 0)).unwrap().architecture,
            Some("amd64")
        );
        // ELF가 아니거나 짧으면 읽지 않는다
        assert_eq!(parse_runtime(b"hello"), None);
        assert_eq!(parse_runtime(&[0u8; 64]), None);
        // 넘치는 값으로 속지 않는다
        assert_eq!(parse_runtime(&elf64(62, u64::MAX, 64, 10)), None);
    }

    #[test]
    fn reads_top_level_names_from_a_listing() {
        let names = top_level_names(
            "squashfs-root\nsquashfs-root/.DirIcon\nsquashfs-root/AppRun\nsquashfs-root/app.desktop\nsquashfs-root/usr/bin/app\n/other.png\n",
        );
        assert_eq!(names, [".DirIcon", "AppRun", "app.desktop", "other.png"]);
    }

    #[test]
    fn names_from_files() {
        let title = |name: &str| title_from_file_name(Path::new(name));
        assert_eq!(title("/d/Foo-1.2.3-aarch64.AppImage"), "Foo-1.2.3");
        assert_eq!(title("Foo.appimage"), "Foo");
        assert_eq!(title("Foo-x86_64.APPIMAGE"), "Foo");
        assert_eq!(desktop_id_from_name("My App 1.0"), "My-App-1.0");
        assert_eq!(desktop_id_from_name("../.."), "appimage");
        assert_eq!(directory_name("../../etc"), "-..-etc");
        assert_eq!(directory_name("..."), "AppImage");
        assert_eq!(directory_name("a/b"), "a-b");
        assert_eq!(directory_name("한글 앱"), "한글 앱");
    }

    #[test]
    fn only_a_directory_right_under_applications_counts() {
        let top = Path::new("/home/u/Applications");
        assert!(directly_under(top, Path::new("/home/u/Applications/앱")));
        assert!(!directly_under(top, Path::new("/home/u/Applications/..")));
        assert!(!directly_under(top, Path::new("/home/u/Applications/a/b")));
        assert!(!directly_under(top, Path::new("/home/u/Applications")));
        assert!(!directly_under(top, Path::new("/home/u")));
    }

    #[test]
    fn exec_always_runs_app_run_and_keeps_arguments() {
        let dir = Path::new("/home/u/Applications/My App");
        assert_eq!(
            rewrite_exec(Some("foo --no-sandbox %U"), dir),
            "env \"APPDIR=/home/u/Applications/My App\" \"/home/u/Applications/My App/AppRun\" --no-sandbox %U"
        );
        assert_eq!(
            rewrite_exec(Some("\"/odd path/foo\" %F"), dir),
            "env \"APPDIR=/home/u/Applications/My App\" \"/home/u/Applications/My App/AppRun\" %F"
        );
        assert_eq!(
            rewrite_exec(None, dir),
            "env \"APPDIR=/home/u/Applications/My App\" \"/home/u/Applications/My App/AppRun\""
        );
        // 따옴표와 $는 이스케이프한다 (명세의 규칙. 키 파일의 이스케이프는 KeyFile이 한다)
        assert_eq!(
            rewrite_exec(Some("foo"), Path::new("/a$b")),
            "env \"APPDIR=/a\\$b\" \"/a\\$b/AppRun\""
        );
    }

    #[test]
    fn a_desktop_entry_cannot_choose_its_directory_or_program() {
        let source_data = b"[Desktop Entry]\nName=Evil\nName[ko]=\xec\x95\xb1\nExec=rm -rf /\nX-BuntOS-AppImage-Dir=/home/u\nCategories=Utility;\nTryExec=/bin/false\n";
        let source = DesktopEntry::parse(source_data).unwrap();
        let dir = Path::new("/home/u/Applications/Evil");
        let text = desktop_contents(&source, Some(source_data), "Evil", Some("1"), dir, None);
        let keys = glib::KeyFile::new();
        keys.load_from_data(&text, glib::KeyFileFlags::KEEP_TRANSLATIONS)
            .unwrap();
        let get = |key: &str| {
            keys.string("Desktop Entry", key)
                .ok()
                .map(|v| v.to_string())
        };
        assert_eq!(get(DIR_KEY).as_deref(), Some("/home/u/Applications/Evil"));
        let exec = get("Exec").unwrap();
        assert!(exec.contains("\"/home/u/Applications/Evil/AppRun\""));
        assert!(!exec.contains("rm -rf"));
        assert_eq!(get("TryExec"), None);
        assert_eq!(get("Categories").as_deref(), Some("Utility;"));
        assert_eq!(get("Name[ko]").as_deref(), Some("앱"));
        assert_eq!(get(VERSION_KEY).as_deref(), Some("1"));
    }

    #[test]
    fn a_folder_the_user_made_is_not_replaced() {
        let top = std::env::temp_dir().join(format!("bunt-packages-target-{}", std::process::id()));
        fs::create_dir_all(top.join("My App")).unwrap();
        // 이름이 같은 폴더가 있으면 새 이름을 쓴다
        assert_eq!(choose_target(&top, "My App", None), top.join("My App 2"));
        assert_eq!(choose_target(&top, "Other", None), top.join("Other"));
        // 같은 앱의 옛 판은 그 자리를 다시 쓴다
        let existing = Installed {
            desktop: top.join("my.desktop"),
            dir: top.join("My App"),
            version: None,
        };
        assert_eq!(
            choose_target(&top, "My App", Some(&existing)),
            top.join("My App")
        );
        fs::remove_dir_all(&top).unwrap();
    }

    #[test]
    fn an_icon_link_out_of_the_directory_is_not_used() {
        let top = std::env::temp_dir().join(format!("bunt-packages-test-{}", std::process::id()));
        let dir = top.join("app");
        fs::create_dir_all(&dir).unwrap();
        fs::write(top.join("secret.png"), b"x").unwrap();
        std::os::unix::fs::symlink("../secret.png", dir.join(".DirIcon")).unwrap();
        assert_eq!(find_icon(&dir, None), None);
        fs::write(dir.join("app.png"), b"x").unwrap();
        assert_eq!(
            find_icon(&dir, Some("app")),
            Some(fs::canonicalize(dir.join("app.png")).unwrap())
        );
        // 경로를 적은 Icon은 무시하고 .DirIcon을 본다
        assert_eq!(find_icon(&dir, Some("../secret")), None);
        fs::remove_dir_all(&top).unwrap();
    }
}
