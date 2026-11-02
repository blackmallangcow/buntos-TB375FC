// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 설치 파일을 읽는다. 무슨 형식인지 보고 이름, 판, 설명, 아이콘을 꺼낸다. 설치 창이 뜨기 전에는 받은 파일의
//! 코드를 실행하지 않는다: .deb는 dpkg-deb로, AppImage는 ELF 머리와 unsquashfs로, Flatpak은 키 파일과
//! GVariant로 읽기만 한다. 파일 내용은 믿지 않는다(크기 제한, 경로 비교)

use std::{
    cell::RefCell,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    process::{Command, Stdio},
};

use gtk4::glib;

/// 다루는 설치 파일 형식
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Deb,
    AppImage,
    FlatpakBundle,
    FlatpakRef,
    FlatpakRepo,
}

/// 파일을 보고 안 형식
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detected {
    Package(Kind),
    /// 다른 배포판의 형식. 설치할 수 없다고 알린다
    Rpm,
    Snap,
    Unknown,
}

/// 형식을 알아내는 데 읽는 앞부분의 크기
const HEAD_SIZE: usize = 4096;

/// 설치 파일 안의 .desktop과 아이콘 하나의 크기 상한
const MAX_ENTRY_SIZE: u64 = 4 * 1024 * 1024;

/// 이 컴퓨터의 Debian 아키텍처 이름 (D3: 두 아키텍처를 같은 소스에서 빌드한다)
pub const HOST_ARCH: &str = match std::env::consts::ARCH.as_bytes() {
    b"aarch64" => "arm64",
    b"x86_64" => "amd64",
    _ => "unknown",
};

/// 파일 이름과 앞부분으로 형식을 알아낸다. 내용으로 알 수 있으면 확장자보다 내용을 믿는다
pub fn detect(name: &str, head: &[u8]) -> Detected {
    let lower = name.to_lowercase();
    if head.starts_with(b"!<arch>\ndebian-binary") {
        return Detected::Package(Kind::Deb);
    }
    if head.starts_with(b"\xed\xab\xee\xdb") || lower.ends_with(".rpm") {
        return Detected::Rpm;
    }
    let text = String::from_utf8_lossy(head);
    let text = text.trim_start();
    if text.starts_with("[Flatpak Ref]") {
        return Detected::Package(Kind::FlatpakRef);
    }
    if text.starts_with("[Flatpak Repo]") {
        return Detected::Package(Kind::FlatpakRepo);
    }
    if head.starts_with(b"\x7fELF")
        && (head.get(8..10) == Some(b"AI") || lower.ends_with(".appimage"))
    {
        return Detected::Package(Kind::AppImage);
    }
    if lower.ends_with(".snap") {
        return Detected::Snap;
    }
    if lower.ends_with(".flatpak") || lower.ends_with(".xdgapp") {
        return Detected::Package(Kind::FlatpakBundle);
    }
    Detected::Unknown
}

/// 설치 파일에서 읽은 것. 창에 보이는 값은 모두 파일이 적은 것이라 믿지 않는 글자로 다룬다
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Info {
    /// 패키지 이름, Flatpak 앱 ID, AppImage의 데스크톱 ID
    pub id: String,
    /// 보이는 이름. .desktop의 이름이 있으면 그것이다
    pub title: String,
    pub version: Option<String>,
    /// 한 줄 설명
    pub summary: Option<String>,
    /// 긴 설명
    pub description: Option<String>,
    pub maintainer: Option<String>,
    pub homepage: Option<String>,
    /// 설치한 뒤 크기 (바이트). 모르면 None
    pub installed_size: Option<u64>,
    /// 파일에 적힌 아키텍처 (Debian 이름). all이면 어디서나 돈다
    pub architecture: Option<String>,
    /// 아이콘 파일의 내용 (PNG, SVG)
    pub icon: Option<Vec<u8>>,
    /// Flatpak: 저장소 주소(.flatpakref의 Url, .flatpakrepo의 Url)
    pub source_url: Option<String>,
    /// Flatpak 앱의 브랜치
    pub branch: Option<String>,
    /// 찾은 .desktop 파일의 이름 (.deb, AppImage)
    pub desktop_file: Option<String>,
    /// 설치할 수 없는 까닭의 Fluent 메시지 ID
    pub blocked: Option<&'static str>,
}

/// 파일 이름에서 판 번호 같은 것을 빼지 않고 확장자만 떼어 이름으로 쓴다 (읽을 수 있는 이름이 없을 때)
fn title_from_file(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 파일의 앞부분을 읽어 형식을 알아낸다
pub fn detect_file(path: &Path) -> io::Result<Detected> {
    let mut head = Vec::with_capacity(HEAD_SIZE);
    File::open(path)?
        .take(HEAD_SIZE as u64)
        .read_to_end(&mut head)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(detect(&name, &head))
}

/// 설치 파일을 읽는다. 오래 걸릴 수 있어 주 스레드에서 부르지 않는다
pub fn inspect(path: &Path, kind: Kind) -> io::Result<Info> {
    match kind {
        Kind::Deb => inspect_deb(path),
        Kind::AppImage => crate::appimage::inspect(path),
        Kind::FlatpakBundle => inspect_flatpak_bundle(path),
        Kind::FlatpakRef => inspect_flatpak_ref(path),
        Kind::FlatpakRepo => inspect_flatpak_repo(path),
    }
}

// .deb

/// deb의 제어 파일 (dpkg-deb --field). "이름: 값" 줄과 공백으로 시작하는 이어지는 줄이다
pub fn parse_control(text: &str) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = fields.last_mut() {
                value.push('\n');
                value.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            fields.push((name.trim().to_owned(), value.trim().to_owned()));
        }
    }
    fields
}

/// 제어 파일의 Description: 첫 줄은 한 줄 설명, 나머지는 긴 설명이다. 빈 줄은 " ."로 적는다
fn split_description(value: &str) -> (Option<String>, Option<String>) {
    let mut lines = value.lines();
    let summary = lines
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned);
    let body: Vec<&str> = lines
        .map(|line| if line == "." { "" } else { line })
        .collect();
    let body = body.join("\n");
    let body = body.trim();
    (summary, (!body.is_empty()).then(|| body.to_owned()))
}

fn inspect_deb(path: &Path) -> io::Result<Info> {
    let output = Command::new("dpkg-deb")
        .arg("--field")
        .arg(path)
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let fields = parse_control(&String::from_utf8_lossy(&output.stdout));
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    };
    let id = field("Package").ok_or_else(|| io::Error::other("no Package field"))?;
    let (summary, description) = field("Description")
        .map(|value| split_description(&value))
        .unwrap_or_default();
    let mut info = Info {
        title: id.clone(),
        id,
        version: field("Version"),
        summary,
        description,
        maintainer: field("Maintainer"),
        homepage: field("Homepage"),
        // Installed-Size는 KiB다
        installed_size: field("Installed-Size")
            .and_then(|value| value.parse::<u64>().ok())
            .map(|kib| kib * 1024),
        architecture: field("Architecture"),
        ..Info::default()
    };
    // 앱이면 안의 .desktop에서 보이는 이름과 아이콘을 얻는다. 못 읽어도 설치에는 지장이 없다
    if let Err(err) = read_deb_payload(path, &mut info) {
        eprintln!("bunt-packages: could not read {}: {err}", path.display());
    }
    Ok(info)
}

/// 데이터 부분을 tar로 흘려 받아 .desktop과 아이콘만 메모리에 담는다. 디스크에 풀지 않으므로 파일 안의
/// 경로나 링크가 무엇을 가리키든 상관없다
fn read_deb_payload(path: &Path, info: &mut Info) -> io::Result<()> {
    let mut child = Command::new("dpkg-deb")
        .arg("--fsys-tarfile")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let payload = RefCell::new(DebPayload::default());
    let result = read_tar(
        stdout,
        |name, size| payload.borrow().wants(name, size),
        |name, data| payload.borrow_mut().keep(name, data),
    );
    // 끝까지 읽지 않았으면 파이프가 닫혀 dpkg-deb가 끝난다
    let _ = child.kill();
    let _ = child.wait();
    result?;
    payload.into_inner().apply(info);
    Ok(())
}

/// .deb의 데이터에서 모은 것
#[derive(Default)]
struct DebPayload {
    /// (파일 이름, 내용)
    desktop: Vec<(String, Vec<u8>)>,
    /// (경로, 내용)
    icons: Vec<(String, Vec<u8>)>,
    kept: u64,
}

impl DebPayload {
    /// 아이콘을 담아 두는 합의 상한. 패키지에 아이콘이 많아도 메모리를 지나치게 쓰지 않는다
    const MAX_KEPT: u64 = 64 * 1024 * 1024;

    fn wants(&self, name: &str, size: u64) -> bool {
        if size > MAX_ENTRY_SIZE || self.kept + size > Self::MAX_KEPT {
            return false;
        }
        is_desktop_path(name) || is_icon_path(name)
    }

    fn keep(&mut self, name: &str, data: Vec<u8>) {
        self.kept += data.len() as u64;
        if is_desktop_path(name) {
            let file = name.rsplit('/').next().unwrap_or(name).to_owned();
            self.desktop.push((file, data));
        } else {
            self.icons.push((name.to_owned(), data));
        }
    }

    fn apply(self, info: &mut Info) {
        // 메뉴에 보이는 항목을 고른다. 없으면 앱이 아니다
        let entry = self.desktop.iter().find_map(|(file, data)| {
            let entry = DesktopEntry::parse(data)?;
            (!entry.hidden).then_some((file, entry))
        });
        let Some((file, entry)) = entry else {
            return;
        };
        info.desktop_file = Some(file.clone());
        if let Some(name) = entry.name {
            info.title = name;
        }
        if info.summary.is_none() {
            info.summary = entry.comment;
        }
        if let Some(icon) = entry.icon {
            info.icon = best_icon(&self.icons, &icon);
        }
    }
}

fn is_desktop_path(name: &str) -> bool {
    let name = name.trim_start_matches("./");
    name.starts_with("usr/share/applications/") && name.ends_with(".desktop")
}

fn is_icon_path(name: &str) -> bool {
    let name = name.trim_start_matches("./");
    let image = name.ends_with(".png") || name.ends_with(".svg");
    image && (name.starts_with("usr/share/icons/") || name.starts_with("usr/share/pixmaps/"))
        || (image && name.starts_with("opt/"))
}

/// Icon= 값에 맞는 아이콘을 고른다. 벡터 그림, 그다음 큰 것이 낫다. 절대 경로로 적은 앱도 있다
fn best_icon(icons: &[(String, Vec<u8>)], icon: &str) -> Option<Vec<u8>> {
    let score = |path: &str| -> Option<u32> {
        let path = path.trim_start_matches("./");
        if let Some(absolute) = icon.strip_prefix('/') {
            return (path == absolute).then_some(1);
        }
        let (stem, extension) = path.rsplit('/').next()?.rsplit_once('.')?;
        if stem != icon {
            return None;
        }
        if extension == "svg" {
            return Some(100_000);
        }
        // hicolor/256x256/apps/foo.png의 256. pixmaps에 그냥 있는 것은 크기를 모른다
        let size = path
            .split('/')
            .find_map(|part| part.split_once('x')?.0.parse::<u32>().ok())
            .unwrap_or(32);
        Some(size)
    };
    icons
        .iter()
        .filter_map(|(path, data)| Some((score(path)?, data)))
        .max_by_key(|(score, _)| *score)
        .map(|(_, data)| data.clone())
}

/// tar 흐름을 읽는다. 일반 파일마다 wants(이름, 크기)에 묻고, 원하면 내용을 keep으로 넘긴다
pub fn read_tar(
    mut reader: impl Read,
    mut wants: impl FnMut(&str, u64) -> bool,
    mut keep: impl FnMut(&str, Vec<u8>),
) -> io::Result<()> {
    const BLOCK: usize = 512;
    let mut header = [0u8; BLOCK];
    // GNU 형식의 긴 이름 (앞 항목 'L'의 내용)
    let mut long_name: Option<String> = None;
    loop {
        if read_block(&mut reader, &mut header)?.is_none() || header.iter().all(|byte| *byte == 0) {
            return Ok(());
        }
        let size = tar_octal(&header[124..136]);
        let padded = size.div_ceil(BLOCK as u64) * BLOCK as u64;
        let kind = header[156];
        match kind {
            b'L' => {
                let mut data = Vec::new();
                let take = size.min(MAX_ENTRY_SIZE);
                (&mut reader).take(take).read_to_end(&mut data)?;
                skip(&mut reader, padded - take)?;
                let end = data
                    .iter()
                    .position(|byte| *byte == 0)
                    .unwrap_or(data.len());
                long_name = Some(String::from_utf8_lossy(&data[..end]).into_owned());
            }
            // 일반 파일만 본다. 링크는 따라가지 않는다
            0 | b'0' | b'7' => {
                let name = long_name.take().unwrap_or_else(|| tar_name(&header));
                if wants(&name, size) {
                    let mut data = Vec::with_capacity(size as usize);
                    (&mut reader).take(size).read_to_end(&mut data)?;
                    if data.len() as u64 != size {
                        return Err(io::ErrorKind::UnexpectedEof.into());
                    }
                    skip(&mut reader, padded - size)?;
                    keep(&name, data);
                } else {
                    skip(&mut reader, padded)?;
                }
            }
            _ => {
                long_name = None;
                skip(&mut reader, padded)?;
            }
        }
    }
}

/// 한 블록을 채운다. 흐름이 끝났으면 None
fn read_block(reader: &mut impl Read, block: &mut [u8; 512]) -> io::Result<Option<()>> {
    let mut filled = 0;
    while filled < block.len() {
        let read = reader.read(&mut block[filled..])?;
        if read == 0 {
            return if filled == 0 {
                Ok(None)
            } else {
                Err(io::ErrorKind::UnexpectedEof.into())
            };
        }
        filled += read;
    }
    Ok(Some(()))
}

fn skip(reader: &mut impl Read, count: u64) -> io::Result<()> {
    let copied = io::copy(&mut reader.take(count), &mut io::sink())?;
    if copied != count {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    Ok(())
}

fn tar_octal(field: &[u8]) -> u64 {
    let text: String = field
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as char)
        .collect();
    u64::from_str_radix(text.trim(), 8).unwrap_or(0)
}

fn tar_name(header: &[u8; 512]) -> String {
    let field = |range: std::ops::Range<usize>| {
        let bytes = &header[range];
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    };
    let name = field(0..100);
    // ustar의 앞부분 (magic이 ustar일 때만)
    let prefix = if &header[257..262] == b"ustar" {
        field(345..500)
    } else {
        String::new()
    };
    if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    }
}

// .desktop

/// .desktop 파일에서 쓰는 것. 지금 언어의 값을 읽는다
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DesktopEntry {
    pub name: Option<String>,
    pub comment: Option<String>,
    pub icon: Option<String>,
    pub exec: Option<String>,
    /// NoDisplay=true 또는 Hidden=true: 메뉴에 없는 항목
    pub hidden: bool,
}

impl DesktopEntry {
    pub fn parse(data: &[u8]) -> Option<Self> {
        const GROUP: &str = "Desktop Entry";
        let keys = glib::KeyFile::new();
        keys.load_from_bytes(&glib::Bytes::from(data), glib::KeyFileFlags::NONE)
            .ok()?;
        let text = |key: &str| {
            keys.locale_string(GROUP, key, None)
                .ok()
                .map(|value| value.to_string())
                .filter(|value| !value.is_empty())
        };
        let flag = |key: &str| keys.boolean(GROUP, key).unwrap_or(false);
        Some(Self {
            name: text("Name"),
            comment: text("Comment"),
            icon: text("Icon"),
            exec: keys
                .string(GROUP, "Exec")
                .ok()
                .map(|value| value.to_string()),
            hidden: flag("NoDisplay") || flag("Hidden"),
        })
    }
}

// Flatpak

fn inspect_flatpak_ref(path: &Path) -> io::Result<Info> {
    const GROUP: &str = "Flatpak Ref";
    let keys = load_key_file(path)?;
    let text = |key: &str| {
        keys.string(GROUP, key)
            .ok()
            .map(|value| value.to_string())
            .filter(|value| !value.is_empty())
    };
    let id = text("Name").ok_or_else(|| io::Error::other("no Name key"))?;
    Ok(Info {
        title: text("Title").map_or_else(|| id.clone(), |title| ref_title(&title).to_owned()),
        id,
        summary: text("Comment"),
        description: text("Description"),
        homepage: text("Homepage"),
        branch: text("Branch"),
        source_url: text("Url"),
        ..Info::default()
    })
}

/// Flathub의 Title은 "org.gnome.Mines from flathub"처럼 앱 ID에 저장소 이름이 붙는다. 앱 ID만 남긴다
fn ref_title(title: &str) -> &str {
    match title.split_once(" from ") {
        Some((name, _)) if name.contains('.') && !name.contains(' ') => name,
        _ => title,
    }
}

fn inspect_flatpak_repo(path: &Path) -> io::Result<Info> {
    const GROUP: &str = "Flatpak Repo";
    let keys = load_key_file(path)?;
    let text = |key: &str| {
        keys.string(GROUP, key)
            .ok()
            .map(|value| value.to_string())
            .filter(|value| !value.is_empty())
    };
    let url = text("Url").ok_or_else(|| io::Error::other("no Url key"))?;
    let name = title_from_file(path);
    let summary = text("Comment");
    // Flathub의 파일처럼 두 키가 같은 글이면 한 번만 보인다
    let description = text("Description").filter(|value| summary.as_ref() != Some(value));
    Ok(Info {
        title: text("Title").unwrap_or_else(|| name.clone()),
        id: repo_name(&name),
        summary,
        description,
        homepage: text("Homepage"),
        source_url: Some(url),
        ..Info::default()
    })
}

/// 저장소 이름 (flatpak remote-add의 이름): 영문자, 숫자, -, _만 남긴다
pub fn repo_name(file_stem: &str) -> String {
    let name: String = file_stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches('-').to_owned();
    if name.is_empty() {
        "remote".to_owned()
    } else {
        name
    }
}

fn load_key_file(path: &Path) -> io::Result<glib::KeyFile> {
    // 크기를 묶는다. 키 파일 두 형식은 몇 KB다
    let mut data = Vec::new();
    File::open(path)?
        .take(MAX_ENTRY_SIZE)
        .read_to_end(&mut data)?;
    let keys = glib::KeyFile::new();
    keys.load_from_bytes(&glib::Bytes::from_owned(data), glib::KeyFileFlags::NONE)
        .map_err(io::Error::other)?;
    Ok(keys)
}

/// 번들의 메타데이터 사전을 읽는다. 번들은 ostree의 정적 델타 하나이고 GVariant 튜플이다
/// (OSTREE_STATIC_DELTA_SUPERBLOCK_FORMAT, 첫 값이 a{sv}). 튜플의 프레이밍 값은 파일 끝에 거꾸로 놓이므로 맨 끝의 값이
/// 첫 값이 끝나는 자리다. 파일 전체를 메모리에 올리지 않고 그 앞부분만 읽는다
fn read_bundle_metadata(path: &Path) -> io::Result<Vec<u8>> {
    /// 메타데이터(이름, 아이콘, appdata)의 크기 상한
    const MAX_METADATA: u64 = 32 * 1024 * 1024;
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    // GVariant 규칙: 프레이밍 값의 폭은 전체 크기로 정한다
    let width = match size {
        0..=0xff => 1,
        0x100..=0xffff => 2,
        0x1_0000..=0xffff_ffff => 4,
        _ => 8,
    };
    if size <= width {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    file.seek(SeekFrom::End(-(width as i64)))?;
    let mut last = [0u8; 8];
    file.read_exact(&mut last[..width as usize])?;
    let end = u64::from_le_bytes(last);
    if end == 0 || end > size || end > MAX_METADATA {
        return Err(io::Error::other("not a Flatpak bundle"));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut data = Vec::with_capacity(end as usize);
    file.take(end).read_to_end(&mut data)?;
    Ok(data)
}

/// Flatpak 번들(.flatpak). 어떤 값이 있는지는 flatpak_bundle_load와 같다
fn inspect_flatpak_bundle(path: &Path) -> io::Result<Info> {
    let data = read_bundle_metadata(path)?;
    let metadata = glib::Variant::from_bytes_with_type(
        &glib::Bytes::from_owned(data),
        glib::VariantTy::new("a{sv}").expect("valid type string"),
    );
    let dict = glib::VariantDict::new(Some(&metadata));
    let lookup = |key: &str| -> Option<String> { dict.lookup_value(key, None)?.get::<String>() };
    let reference = lookup("ref").ok_or_else(|| io::Error::other("no ref in the bundle"))?;
    let (id, architecture, branch) =
        parse_ref(&reference).ok_or_else(|| io::Error::other("bad ref in the bundle"))?;
    Ok(Info {
        title: id.clone(),
        id,
        branch: Some(branch),
        architecture: flatpak_arch_to_debian(&architecture),
        source_url: lookup("origin").or_else(|| lookup("runtime-repo")),
        icon: dict
            .lookup_value("icon-128", None)
            .or_else(|| dict.lookup_value("icon-64", None))
            .and_then(|value| value.get::<Vec<u8>>()),
        ..Info::default()
    })
}

/// "app/org.foo.App/aarch64/stable"를 (앱 ID, 아키텍처, 브랜치)로 나눈다
pub fn parse_ref(reference: &str) -> Option<(String, String, String)> {
    let mut parts = reference.split('/');
    let _kind = parts.next()?;
    let id = parts.next()?;
    let architecture = parts.next()?;
    let branch = parts.next()?;
    (parts.next().is_none() && !id.is_empty())
        .then(|| (id.to_owned(), architecture.to_owned(), branch.to_owned()))
}

/// Flatpak의 아키텍처 이름을 Debian 이름으로 바꾼다
fn flatpak_arch_to_debian(arch: &str) -> Option<String> {
    Some(
        match arch {
            "aarch64" => "arm64",
            "x86_64" => "amd64",
            "i386" => "i386",
            "arm" => "armhf",
            other => other,
        }
        .to_owned(),
    )
}

/// 경로를 받은 파일 그대로의 형태로 돌려준다
pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// 이 컴퓨터에서 쓸 수 있는 아키텍처인지. 파일에 적히지 않았으면 막지 않는다
pub fn architecture_matches(architecture: Option<&str>, foreign: &[String]) -> bool {
    match architecture {
        None | Some("all") => true,
        Some(architecture) => {
            architecture == HOST_ARCH || foreign.iter().any(|other| other == architecture)
        }
    }
}

/// dpkg에 더한 다른 아키텍처 (dpkg --add-architecture)
pub fn foreign_architectures() -> Vec<String> {
    Command::new("dpkg")
        .arg("--print-foreign-architectures")
        .stdin(Stdio::null())
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use gtk4::prelude::ToVariant;

    use super::*;

    #[test]
    fn detects_by_content_before_the_extension() {
        let deb = b"!<arch>\ndebian-binary   1700000000  0     0     100644  4         `\n2.0\n";
        assert_eq!(detect("foo.deb", deb), Detected::Package(Kind::Deb));
        // 확장자가 달라도 내용이 deb이면 deb다
        assert_eq!(detect("foo.bin", deb), Detected::Package(Kind::Deb));
        // 확장자만 deb인 다른 파일은 설치 파일이 아니다
        assert_eq!(detect("foo.deb", b"hello"), Detected::Unknown);
        assert_eq!(detect("foo.rpm", b"hello"), Detected::Rpm);
        assert_eq!(detect("foo", b"\xed\xab\xee\xdbrest"), Detected::Rpm);
        assert_eq!(detect("foo.snap", b"hsqs"), Detected::Snap);
        assert_eq!(
            detect("foo.flatpakref", b"\n[Flatpak Ref]\nName=a\n"),
            Detected::Package(Kind::FlatpakRef)
        );
        assert_eq!(
            detect("foo.txt", b"[Flatpak Repo]\nUrl=a\n"),
            Detected::Package(Kind::FlatpakRepo)
        );
        assert_eq!(
            detect("foo.flatpak", b"anything"),
            Detected::Package(Kind::FlatpakBundle)
        );
        let mut elf = b"\x7fELF\x02\x01\x01\0AI\x02".to_vec();
        elf.resize(64, 0);
        assert_eq!(detect("App", &elf), Detected::Package(Kind::AppImage));
        // AI 표시가 없는 ELF도 확장자가 AppImage이면 그렇게 본다. 표시도 확장자도 없으면 설치 파일이 아니다
        let plain = b"\x7fELF\x02\x01\x01\0\0\0\0".to_vec();
        assert_eq!(
            detect("App.AppImage", &plain),
            Detected::Package(Kind::AppImage)
        );
        assert_eq!(detect("program", &plain), Detected::Unknown);
    }

    #[test]
    fn reads_debian_control_fields() {
        let fields = parse_control(
            "Package: hello\nVersion: 1:2.3-4\nDescription: says hello\n a long text\n .\n second paragraph\nHomepage: https://example.org/a:b\n",
        );
        assert_eq!(fields[0], ("Package".to_owned(), "hello".to_owned()));
        assert_eq!(fields[1], ("Version".to_owned(), "1:2.3-4".to_owned()));
        assert_eq!(
            fields[3],
            ("Homepage".to_owned(), "https://example.org/a:b".to_owned())
        );
        let (summary, body) = split_description(&fields[2].1);
        assert_eq!(summary.as_deref(), Some("says hello"));
        assert_eq!(body.as_deref(), Some("a long text\n\nsecond paragraph"));
        assert_eq!(
            split_description("only a line"),
            (Some("only a line".into()), None)
        );
    }

    fn tar_entry(name: &str, kind: u8, data: &[u8]) -> Vec<u8> {
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        let size = format!("{:011o}\0", data.len());
        header[124..136].copy_from_slice(size.as_bytes());
        header[156] = kind;
        let mut out = header.to_vec();
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(512) * 512, 0);
        out
    }

    #[test]
    fn tar_keeps_only_wanted_regular_files() {
        let long = "./usr/share/icons/hicolor/48x48/apps/".to_owned() + &"a".repeat(120) + ".png";
        let mut stream = Vec::new();
        stream.extend(tar_entry("./usr/share/applications/", b'5', b""));
        stream.extend(tar_entry(
            "./usr/share/applications/hello.desktop",
            b'0',
            b"[Desktop Entry]\nName=Hello\n",
        ));
        // 건너뛰는 파일 (내용 길이가 블록 경계를 넘는다)
        stream.extend(tar_entry("./usr/bin/hello", b'0', &[7u8; 700]));
        // 링크는 내용을 읽지 않는다
        stream.extend(tar_entry(
            "./usr/share/applications/link.desktop",
            b'2',
            b"",
        ));
        stream.extend(tar_entry("././@LongLink", b'L', long.as_bytes()));
        stream.extend(tar_entry("./usr/share/icons/short", b'0', b"png!"));
        stream.extend(vec![0u8; 1024]);

        let mut seen = Vec::new();
        let mut kept = Vec::new();
        read_tar(
            stream.as_slice(),
            |name, size| {
                seen.push((name.to_owned(), size));
                is_desktop_path(name) || name.ends_with(".png")
            },
            |name, data| kept.push((name.to_owned(), data)),
        )
        .unwrap();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].0, "./usr/share/applications/hello.desktop");
        assert_eq!(kept[1].0, long);
        assert_eq!(kept[1].1, b"png!");
        assert!(
            seen.iter()
                .any(|(name, size)| name == "./usr/bin/hello" && *size == 700)
        );
        assert!(!seen.iter().any(|(name, _)| name.contains("link.desktop")));
    }

    #[test]
    fn a_cut_off_tar_is_an_error() {
        let mut stream = tar_entry("./usr/bin/hello", b'0', &[7u8; 700]);
        stream.truncate(600);
        let result = read_tar(stream.as_slice(), |_, _| true, |_, _| {});
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn picks_the_best_icon() {
        let icons = vec![
            (
                "./usr/share/icons/hicolor/48x48/apps/foo.png".to_owned(),
                vec![48],
            ),
            (
                "./usr/share/icons/hicolor/256x256/apps/foo.png".to_owned(),
                vec![255],
            ),
            (
                "./usr/share/icons/hicolor/512x512/apps/bar.png".to_owned(),
                vec![1],
            ),
            ("./usr/share/pixmaps/foo.png".to_owned(), vec![32]),
        ];
        assert_eq!(best_icon(&icons, "foo"), Some(vec![255]));
        assert_eq!(best_icon(&icons, "nothing"), None);
        // 벡터 그림이 크기와 상관없이 낫다
        let mut with_svg = icons.clone();
        with_svg.push((
            "./usr/share/icons/hicolor/scalable/apps/foo.svg".to_owned(),
            vec![9],
        ));
        assert_eq!(best_icon(&with_svg, "foo"), Some(vec![9]));
        // 절대 경로로 적은 앱
        assert_eq!(
            best_icon(&icons, "/usr/share/pixmaps/foo.png"),
            Some(vec![32])
        );
    }

    #[test]
    fn hidden_desktop_entries_are_not_apps() {
        let entry = DesktopEntry::parse(
            b"[Desktop Entry]\nName=App\nIcon=app\nExec=app %U\nNoDisplay=true\n",
        )
        .unwrap();
        assert!(entry.hidden);
        assert_eq!(entry.name.as_deref(), Some("App"));
        assert_eq!(entry.exec.as_deref(), Some("app %U"));
        assert!(DesktopEntry::parse(b"not a key file").is_none());
    }

    #[test]
    fn flatpak_names() {
        assert_eq!(ref_title("org.gnome.Mines from flathub"), "org.gnome.Mines");
        assert_eq!(ref_title("Letters from Home"), "Letters from Home");
        assert_eq!(ref_title("Mines"), "Mines");
        assert_eq!(
            parse_ref("app/org.foo.App/aarch64/stable"),
            Some(("org.foo.App".into(), "aarch64".into(), "stable".into()))
        );
        assert_eq!(parse_ref("org.foo.App"), None);
        assert_eq!(repo_name("flathub"), "flathub");
        assert_eq!(repo_name("../my repo!"), "my-repo");
        assert_eq!(repo_name("///"), "remote");
    }

    #[test]
    fn a_repo_file_does_not_repeat_the_same_text() {
        let path = std::env::temp_dir().join(format!(
            "bunt-packages-test-{}.flatpakrepo",
            std::process::id()
        ));
        std::fs::write(
            &path,
            "[Flatpak Repo]\nTitle=Flathub Beta\nUrl=https://dl.flathub.org/beta-repo/\nComment=Beta builds\nDescription=Beta builds\n",
        )
        .unwrap();
        let info = inspect_flatpak_repo(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(info.title, "Flathub Beta");
        assert_eq!(info.summary.as_deref(), Some("Beta builds"));
        assert_eq!(info.description, None);
    }

    #[test]
    fn reads_a_bundle_without_loading_the_whole_file() {
        // 정적 델타 앞부분과 같은 모양의 튜플: (a{sv}tay)
        let dict = glib::VariantDict::new(None);
        dict.insert("ref", "app/org.foo.App/aarch64/stable");
        dict.insert("origin", "https://example.org/repo");
        dict.insert_value("icon-128", &vec![1u8, 2, 3].to_variant());
        let tuple = glib::Variant::tuple_from_iter([
            dict.end(),
            7u64.to_variant(),
            vec![0u8; 70_000].to_variant(),
        ]);
        let path = std::env::temp_dir().join(format!("bunt-bundle-{}", std::process::id()));
        std::fs::write(&path, tuple.data()).unwrap();
        let info = inspect_flatpak_bundle(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(info.id, "org.foo.App");
        assert_eq!(info.branch.as_deref(), Some("stable"));
        assert_eq!(info.architecture.as_deref(), Some("arm64"));
        assert_eq!(info.source_url.as_deref(), Some("https://example.org/repo"));
        assert_eq!(info.icon, Some(vec![1, 2, 3]));
    }

    #[test]
    fn architecture_check() {
        assert!(architecture_matches(None, &[]));
        assert!(architecture_matches(Some("all"), &[]));
        assert!(architecture_matches(Some(HOST_ARCH), &[]));
        assert!(!architecture_matches(Some("riscv64"), &[]));
        assert!(architecture_matches(
            Some("riscv64"),
            &["riscv64".to_owned()]
        ));
    }
}
