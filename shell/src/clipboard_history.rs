// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 클립보드 기록의 내용: 무엇을 남기고, 얼마나 두고, 무엇을 버리는지. 여기에는 입출력이 없다. Wayland와 파이프는
//! clipboard.rs, 화면은 clipboard_panel.rs가 맡는다.
//!
//! - 메모리에만 둔다. 파일에도, 설정(dconf)에도 쓰지 않는다. 로그아웃하면 사라진다.
//! - 비밀번호 관리자 표시(`x-kde-passwordManagerHint`)가 붙은 복사는 내용을 읽지도 않는다. 값은 `secret`뿐이므로 표시가
//!   있는지만 본다. 값을 읽으려면 그 앱에 데이터를 청해야 해서 앱이 복사를 한 번 내주는 셈이 된다.
//! - 상한: 항목 수, 항목 크기, 합계 크기, 보관 기간. 넘은 복사는 남기지 않을 뿐 클립보드는 그대로 쓴다.
//! - 글, PNG 그림, 파일(`text/uri-list`)을 남긴다. 받은 앱이 고를 수 있는 다른 형식(서식 있는 글 등)은 남기지 않는다.

use std::{
    rc::Rc,
    time::{Duration, SystemTime},
};

use gtk4::{gio, glib, prelude::*};

/// 비밀번호 관리자가 복사에 붙이는 표시 (KDE Klipper가 정했고 KeePassXC 등이 붙인다)
const SECRET_MIME: &str = "x-kde-passwordManagerHint";
/// 셸이 기록에서 다시 내놓은 복사의 표시. 다시 기록하지 않는다
pub const OWN_MIME: &str = "application/x-buntos-clipboard-history";
const URI_LIST: &str = "text/uri-list";
/// 파일 앱(Nautilus)이 붙여 넣을 때 보는 형식. 첫 줄이 copy나 cut이다
const GNOME_COPIED_FILES: &str = "x-special/gnome-copied-files";
const PNG: &str = "image/png";
/// 글을 읽을 때 고르는 차례. UTF-8이라고 밝힌 것이 먼저다
const TEXT_MIMES: [&str; 3] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];
/// 글을 다시 내놓을 때의 형식. wl-copy와 같다
const SERVED_TEXT: [&str; 5] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "STRING",
    "TEXT",
];
const SERVED_FILES: [&str; 2] = [URI_LIST, GNOME_COPIED_FILES];
const SERVED_IMAGE: [&str; 1] = [PNG];

/// 항목 수 상한
const MAX_ITEMS: usize = 200;
/// 글과 파일 목록 하나의 크기 상한
const MAX_TEXT_BYTES: usize = 1 << 20;
/// 그림 하나의 크기 상한 (PNG 파일)
const MAX_IMAGE_BYTES: usize = 16 << 20;
/// 모든 항목의 크기 상한
const MAX_TOTAL_BYTES: usize = 32 << 20;
/// 이보다 픽셀이 많은 그림은 남기지 않는다(8K). 작은 PNG가 큰 그림을 밝혀 풀 때 메모리를 다 쓰는 일을 막는다
const MAX_IMAGE_PIXELS: u64 = 7680 * 4320;
/// 미리 보기 그림의 긴 변 (픽셀). 패널에서 40pt이고 TB375FC의 225%에서 90픽셀이다
const THUMBNAIL_SIDE: u32 = 128;
/// 목록의 한 항목에 쓰는 글자 수. 한 줄만 보이므로 더 넘겨도 쓸모없고 긴 글에서 느려진다
const PREVIEW_CHARS: usize = 200;
/// 파일 목록의 한 항목에 적는 이름 수
const PREVIEW_FILES: usize = 20;

/// 보관 기간의 기본값 (org.buntos.shell clipboard-history-period의 기본값과 같다)
const DEFAULT_KEEP: Duration = Duration::from_secs(8 * 60 * 60);

/// org.buntos.shell clipboard-history-period의 값을 기간으로. macOS의 Spotlight 설정과 같은 셋이다
pub fn keep_for(period: &str) -> Duration {
    match period {
        "30m" => Duration::from_secs(30 * 60),
        "7d" => Duration::from_secs(7 * 24 * 60 * 60),
        _ => DEFAULT_KEEP,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Image,
    Files,
}

impl Kind {
    /// 이 종류의 복사를 읽을 때 넘기지 않는 크기
    pub fn max_bytes(self) -> usize {
        match self {
            Kind::Text | Kind::Files => MAX_TEXT_BYTES,
            Kind::Image => MAX_IMAGE_BYTES,
        }
    }
}

/// 새 복사를 어떻게 할지
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// 남기지 않는다: 비밀번호 관리자의 복사, 셸이 다시 내놓은 복사, 남기지 않는 형식
    Ignore,
    /// 이 형식으로 읽어 남긴다. 앱이 밝힌 철자 그대로다
    Read { mime: String, kind: Kind },
}

/// 앱이 밝힌 형식만 보고 정한다. 파일이 먼저다: 파일을 복사하면 경로를 글로도 함께 내놓는다. 글이 그림보다 먼저다:
/// 글을 복사하면 글을 그린 그림을 함께 내놓는 앱이 있다
pub fn decide(mimes: &[String]) -> Decision {
    let offered = |wanted: &str| mimes.iter().find(|mime| mime.eq_ignore_ascii_case(wanted));
    if offered(SECRET_MIME).is_some() || offered(OWN_MIME).is_some() {
        return Decision::Ignore;
    }
    let read = |mime: &String, kind| Decision::Read {
        mime: mime.clone(),
        kind,
    };
    if let Some(mime) = offered(URI_LIST) {
        return read(mime, Kind::Files);
    }
    if let Some(mime) = TEXT_MIMES.iter().find_map(|wanted| offered(wanted)) {
        return read(mime, Kind::Text);
    }
    if let Some(mime) = offered(PNG) {
        return read(mime, Kind::Image);
    }
    Decision::Ignore
}

/// 그림 항목의 미리 보기. 알파를 곱한 RGBA 8비트, 줄 사이 빈틈 없음
#[derive(Clone, Debug)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub pixels: glib::Bytes,
}

/// 기록에 넣을 내용
#[derive(Debug)]
pub struct NewItem {
    pub kind: Kind,
    pub data: glib::Bytes,
    /// 목록에 보이는 글. 그림은 "1280×800"처럼 크기다
    pub preview: String,
    pub thumbnail: Option<Thumbnail>,
}

impl NewItem {
    /// 읽은 글. UTF-8이 아니거나 빈 글은 남기지 않는다
    pub fn text(data: Vec<u8>) -> Option<Self> {
        let preview = text_preview(std::str::from_utf8(&data).ok()?);
        (!preview.is_empty()).then(|| Self {
            kind: Kind::Text,
            data: glib::Bytes::from_owned(data),
            preview,
            thumbnail: None,
        })
    }

    /// 읽은 파일 목록. 파일이 하나도 없으면 남기지 않는다
    pub fn files(data: Vec<u8>) -> Option<Self> {
        let list = std::str::from_utf8(&data).ok()?;
        let names: Vec<String> = uris(list)
            .take(PREVIEW_FILES)
            .filter_map(|uri| {
                gio::File::for_uri(uri)
                    .basename()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .collect();
        (!names.is_empty()).then(|| Self {
            kind: Kind::Files,
            preview: names.join(", "),
            data: glib::Bytes::from_owned(data),
            thumbnail: None,
        })
    }

    /// 읽은 PNG. 풀 수 없으면 남기지 않는다. 그림을 끝까지 풀므로 따로 도는 스레드에서 부른다. PNG만 받는 것은 그 때문이다:
    /// 어느 앱이든 복사로 넣을 수 있는 바이트라서 메모리 안전한 Rust 해독기로만 푼다
    pub fn image(data: Vec<u8>) -> Option<Self> {
        let (thumbnail, width, height) = decode_thumbnail(&data)?;
        Some(Self {
            kind: Kind::Image,
            data: glib::Bytes::from_owned(data),
            preview: format!("{width}×{height}"),
            thumbnail: Some(thumbnail),
        })
    }
}

/// 기록의 항목 하나
#[derive(Clone, Debug)]
pub struct Item {
    pub id: u64,
    pub kind: Kind,
    data: glib::Bytes,
    pub preview: String,
    pub thumbnail: Option<Thumbnail>,
    pub copied_at: SystemTime,
}

impl Item {
    /// 클립보드에 다시 내놓을 때 밝힐 형식
    pub fn mime_types(&self) -> &'static [&'static str] {
        match self.kind {
            Kind::Text => &SERVED_TEXT,
            Kind::Files => &SERVED_FILES,
            Kind::Image => &SERVED_IMAGE,
        }
    }

    /// 그 형식으로 내놓을 바이트. 파일 앱의 형식은 잘라내기였어도 복사로 내놓는다: 옮긴 뒤의 기록에서 고른 파일을 또 옮기지 않게 한다
    pub fn payload(&self, mime: &str) -> glib::Bytes {
        if self.kind == Kind::Files && mime == GNOME_COPIED_FILES {
            let list = String::from_utf8_lossy(&self.data);
            let lines: Vec<&str> = std::iter::once("copy").chain(uris(&list)).collect();
            return glib::Bytes::from_owned(lines.join("\n").into_bytes());
        }
        self.data.clone()
    }

    /// 검색어(소문자)가 들어 있는지. 그림은 찾을 글이 없다
    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        match self.kind {
            Kind::Text => std::str::from_utf8(&self.data)
                .is_ok_and(|text| text.to_lowercase().contains(query)),
            Kind::Files => self.preview.to_lowercase().contains(query),
            Kind::Image => false,
        }
    }

    fn weight(&self) -> usize {
        let thumbnail = self.thumbnail.as_ref().map_or(0, |t| t.pixels.len());
        self.data.len() + thumbnail
    }
}

/// `text/uri-list`의 주소들. 줄 끝은 CRLF이고 #으로 시작하는 줄은 설명이다 (RFC 2483)
fn uris(list: &str) -> impl Iterator<Item = &str> {
    list.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
}

/// 목록에 보일 한 줄: 앞부분만, 빈칸과 줄 바꿈은 빈칸 하나로. 제어 문자는 Pango가 네모로 그리므로 빈칸으로 친다
fn text_preview(text: &str) -> String {
    let mut preview = String::new();
    let mut count = 0;
    let mut gap = false;
    for c in text.chars() {
        if c.is_whitespace() || c.is_control() {
            gap = !preview.is_empty();
            continue;
        }
        if count == PREVIEW_CHARS {
            break;
        }
        if gap {
            preview.push(' ');
            gap = false;
        }
        preview.push(c);
        count += 1;
    }
    preview
}

/// 기록. 최근 것이 먼저다
#[derive(Default)]
pub struct History {
    items: Vec<Rc<Item>>,
    next_id: u64,
}

impl History {
    pub fn items(&self) -> &[Rc<Item>] {
        &self.items
    }

    pub fn get(&self, id: u64) -> Option<Rc<Item>> {
        self.items.iter().find(|item| item.id == id).cloned()
    }

    /// 복사를 남긴다. 같은 내용이 있으면 맨 앞으로 옮긴다. 읽기가 늦게 끝나도(큰 그림) 복사한 차례대로 놓는다
    pub fn add(&mut self, new: NewItem, copied_at: SystemTime) {
        self.items
            .retain(|item| item.kind != new.kind || item.data != new.data);
        self.next_id += 1;
        let item = Rc::new(Item {
            id: self.next_id,
            kind: new.kind,
            data: new.data,
            preview: new.preview,
            thumbnail: new.thumbnail,
            copied_at,
        });
        self.insert(item);
        self.evict();
    }

    /// 기록에서 고른 항목을 지금 복사한 것으로 친다: 맨 앞으로 오고 보관 기간이 다시 시작된다
    pub fn touch(&mut self, id: u64, now: SystemTime) {
        if let Some(index) = self.items.iter().position(|item| item.id == id) {
            let old = self.items.remove(index);
            self.insert(Rc::new(Item {
                copied_at: now,
                ..(*old).clone()
            }));
        }
    }

    fn insert(&mut self, item: Rc<Item>) {
        let at = self
            .items
            .partition_point(|other| other.copied_at > item.copied_at);
        self.items.insert(at, item);
    }

    /// 오래된 것부터 버려 상한 안으로 줄인다
    fn evict(&mut self) {
        let mut total: usize = self.items.iter().map(|item| item.weight()).sum();
        while self.items.len() > MAX_ITEMS || total > MAX_TOTAL_BYTES {
            let Some(oldest) = self.items.pop() else {
                break;
            };
            total -= oldest.weight();
        }
    }

    /// 보관 기간이 지난 항목을 버린다. 버렸으면 true. 시계를 되돌렸으면 나이를 0으로 친다
    pub fn expire(&mut self, keep: Duration, now: SystemTime) -> bool {
        let before = self.items.len();
        self.items.retain(|item| {
            !now.duration_since(item.copied_at)
                .is_ok_and(|age| age >= keep)
        });
        self.items.len() != before
    }

    /// 다음으로 보관 기간이 끝나는 때 (가장 오래된 항목)
    pub fn next_expiry(&self, keep: Duration) -> Option<SystemTime> {
        self.items.last().map(|item| item.copied_at + keep)
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

/// 목록에 적는 복사한 지 얼마나 되었는지
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Age {
    JustNow,
    Minutes(u64),
    Hours(u64),
    Days(u64),
}

pub fn age(copied_at: SystemTime, now: SystemTime) -> Age {
    let minutes = now
        .duration_since(copied_at)
        .map_or(0, |age| age.as_secs() / 60);
    match minutes {
        0 => Age::JustNow,
        1..60 => Age::Minutes(minutes),
        60..1440 => Age::Hours(minutes / 60),
        _ => Age::Days(minutes / 1440),
    }
}

/// 미리 보기의 크기. 그림이 없거나 상한을 넘으면 None이다
fn thumbnail_size(width: u32, height: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return None;
    }
    let long = width.max(height);
    if long <= THUMBNAIL_SIDE {
        return Some((width, height));
    }
    let scaled = |side: u32| (u64::from(side) * u64::from(THUMBNAIL_SIDE) / u64::from(long)) as u32;
    Some((scaled(width).max(1), scaled(height).max(1)))
}

/// PNG를 풀어 미리 보기와 원래 크기를 돌려준다. 칸마다 평균을 내어 줄인다
fn decode_thumbnail(png_file: &[u8]) -> Option<(Thumbnail, u32, u32)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let (width, height) = {
        let info = reader.info();
        (info.width, info.height)
    };
    let (thumb_width, thumb_height) = thumbnail_size(width, height)?;
    let mut frame = vec![0; reader.output_buffer_size()?];
    let output = reader.next_frame(&mut frame).ok()?;
    let channels = output.color_type.samples();
    if output.bit_depth != png::BitDepth::Eight || !matches!(channels, 1..=4) {
        return None;
    }

    let cells = thumb_width as usize * thumb_height as usize;
    // 칸마다 알파를 곱한 R, G, B, 알파의 합과 픽셀 수
    let mut sums = vec![[0u64; 4]; cells];
    let mut counts = vec![0u64; cells];
    for (y, row) in frame
        .chunks_exact(output.line_size)
        .take(height as usize)
        .enumerate()
    {
        let cell_y = y as u64 * u64::from(thumb_height) / u64::from(height);
        for (x, pixel) in row.chunks_exact(channels).take(width as usize).enumerate() {
            let cell_x = x as u64 * u64::from(thumb_width) / u64::from(width);
            let cell = (cell_y * u64::from(thumb_width) + cell_x) as usize;
            let [r, g, b, a] = rgba(pixel);
            let sum = &mut sums[cell];
            sum[0] += u64::from(r) * u64::from(a);
            sum[1] += u64::from(g) * u64::from(a);
            sum[2] += u64::from(b) * u64::from(a);
            sum[3] += u64::from(a);
            counts[cell] += 1;
        }
    }
    let mut pixels = Vec::with_capacity(cells * 4);
    for (sum, count) in sums.iter().zip(&counts) {
        let count = (*count).max(1);
        pixels.extend_from_slice(&[
            (sum[0] / 255 / count) as u8,
            (sum[1] / 255 / count) as u8,
            (sum[2] / 255 / count) as u8,
            (sum[3] / count) as u8,
        ]);
    }
    Some((
        Thumbnail {
            width: thumb_width,
            height: thumb_height,
            pixels: glib::Bytes::from_owned(pixels),
        },
        width,
        height,
    ))
}

/// 해독기가 내놓은 픽셀 하나를 RGBA로 (회색, 회색과 알파, RGB, RGBA)
fn rgba(pixel: &[u8]) -> [u8; 4] {
    match *pixel {
        [gray] => [gray, gray, gray, 255],
        [gray, alpha] => [gray, gray, gray, alpha],
        [r, g, b] => [r, g, b, 255],
        [r, g, b, a] => [r, g, b, a],
        _ => [0; 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mimes(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    fn text(content: &str) -> NewItem {
        NewItem::text(content.as_bytes().to_vec()).unwrap()
    }

    fn previews(history: &History) -> Vec<&str> {
        history
            .items()
            .iter()
            .map(|item| item.preview.as_str())
            .collect()
    }

    fn png_file(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
        let mut file = Vec::new();
        let mut encoder = png::Encoder::new(&mut file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&pixel.repeat((width * height) as usize))
            .unwrap();
        writer.finish().unwrap();
        file
    }

    #[test]
    fn secrets_are_never_read() {
        // KeePassXC가 내놓는 형식. 글이 함께 있어도 읽지 않는다
        let offered = mimes(&["text/plain", "x-kde-passwordManagerHint"]);
        assert_eq!(decide(&offered), Decision::Ignore);
        assert_eq!(
            decide(&mimes(&["text/plain", "X-KDE-PasswordManagerHint"])),
            Decision::Ignore
        );
    }

    #[test]
    fn copies_made_from_the_history_are_not_recorded_again() {
        let offered = mimes(&["text/plain", OWN_MIME]);
        assert_eq!(decide(&offered), Decision::Ignore);
    }

    #[test]
    fn the_best_format_is_chosen_with_the_spelling_the_app_offered() {
        let read = |mime: &str, kind| Decision::Read {
            mime: mime.into(),
            kind,
        };
        assert_eq!(
            decide(&mimes(&[
                "text/html",
                "text/plain",
                "text/plain;charset=UTF-8"
            ])),
            read("text/plain;charset=UTF-8", Kind::Text)
        );
        assert_eq!(
            decide(&mimes(&["UTF8_STRING", "text/plain"])),
            read("UTF8_STRING", Kind::Text)
        );
        // 파일 앱은 경로를 글로도 내놓는다
        assert_eq!(
            decide(&mimes(&["text/plain", "text/uri-list"])),
            read("text/uri-list", Kind::Files)
        );
        // 글을 그림으로도 내놓는 앱 (LibreOffice)
        assert_eq!(
            decide(&mimes(&["image/png", "text/plain"])),
            read("text/plain", Kind::Text)
        );
        assert_eq!(
            decide(&mimes(&["image/png", "text/html"])),
            read("image/png", Kind::Image)
        );
        assert_eq!(decide(&mimes(&["image/tiff"])), Decision::Ignore);
        assert_eq!(decide(&[]), Decision::Ignore);
    }

    #[test]
    fn text_that_cannot_be_shown_is_not_recorded() {
        assert!(NewItem::text(vec![0xff, 0xfe]).is_none());
        assert!(NewItem::text(b"".to_vec()).is_none());
        assert!(NewItem::text(b" \n\t ".to_vec()).is_none());
    }

    #[test]
    fn previews_are_short_and_printable() {
        assert_eq!(text("\n  첫 줄\n둘째\t줄\u{7}\n").preview, "첫 줄 둘째 줄");
        let long = "가".repeat(PREVIEW_CHARS * 2);
        assert_eq!(text(&long).preview.chars().count(), PREVIEW_CHARS);
    }

    #[test]
    fn newest_comes_first_and_the_same_content_moves_up() {
        let mut history = History::default();
        history.add(text("one"), at(1));
        history.add(text("two"), at(2));
        assert_eq!(previews(&history), ["two", "one"]);
        history.add(text("one"), at(3));
        assert_eq!(previews(&history), ["one", "two"]);
        // 읽기가 늦게 끝난 복사는 복사한 차례에 놓인다
        history.add(text("late"), at(2));
        assert_eq!(previews(&history), ["one", "late", "two"]);
    }

    #[test]
    fn the_item_count_has_a_limit() {
        let mut history = History::default();
        for number in 0..MAX_ITEMS as u64 + 5 {
            history.add(text(&format!("item {number}")), at(number));
        }
        assert_eq!(history.items().len(), MAX_ITEMS);
        assert_eq!(
            history.items()[0].preview,
            format!("item {}", MAX_ITEMS + 4)
        );
        assert_eq!(history.items()[MAX_ITEMS - 1].preview, "item 5");
    }

    #[test]
    fn the_total_size_has_a_limit() {
        let mut history = History::default();
        let megabytes = MAX_TOTAL_BYTES / MAX_TEXT_BYTES;
        for number in 0..megabytes + 2 {
            let mut content = vec![b'x'; MAX_TEXT_BYTES];
            content[0] = b'a' + (number % 26) as u8;
            content[1] = b'a' + (number / 26) as u8;
            history.add(NewItem::text(content).unwrap(), at(number as u64));
        }
        assert_eq!(history.items().len(), megabytes);
    }

    #[test]
    fn old_items_expire() {
        let mut history = History::default();
        let keep = keep_for("30m");
        history.add(text("old"), at(0));
        history.add(text("new"), at(1000));
        assert_eq!(history.next_expiry(keep), Some(at(1800)));
        assert!(!history.expire(keep, at(1799)));
        assert!(history.expire(keep, at(1800)));
        assert_eq!(previews(&history), ["new"]);
        // 시계를 되돌려도 지우지 않는다
        assert!(!history.expire(keep, at(0)));
        history.clear();
        assert!(history.items().is_empty());
        assert_eq!(history.next_expiry(keep), None);
    }

    #[test]
    fn picking_an_item_copies_it_again() {
        let mut history = History::default();
        history.add(text("one"), at(0));
        history.add(text("two"), at(1));
        let id = history.items()[1].id;
        history.touch(id, at(1000));
        assert_eq!(previews(&history), ["one", "two"]);
        assert_eq!(history.items()[0].copied_at, at(1000));
        assert_eq!(history.get(id).unwrap().preview, "one");
        history.touch(999, at(2000));
        assert_eq!(history.items()[0].copied_at, at(1000));
    }

    #[test]
    fn periods_match_the_settings_page() {
        assert_eq!(keep_for("30m"), Duration::from_secs(1800));
        assert_eq!(keep_for("8h"), Duration::from_secs(28_800));
        assert_eq!(keep_for("7d"), Duration::from_secs(604_800));
        assert_eq!(keep_for("nonsense"), DEFAULT_KEEP);
    }

    #[test]
    fn ages_read_like_a_clock() {
        assert_eq!(age(at(100), at(159)), Age::JustNow);
        assert_eq!(age(at(0), at(60)), Age::Minutes(1));
        assert_eq!(age(at(0), at(3599)), Age::Minutes(59));
        assert_eq!(age(at(0), at(3600)), Age::Hours(1));
        assert_eq!(age(at(0), at(86_399)), Age::Hours(23));
        assert_eq!(age(at(0), at(86_400)), Age::Days(1));
        assert_eq!(age(at(100), at(0)), Age::JustNow);
    }

    #[test]
    fn text_is_found_in_the_whole_copy_and_ignoring_case() {
        let mut long = "가".repeat(PREVIEW_CHARS);
        long.push_str(" Needle");
        let mut history = History::default();
        history.add(text(&long), at(0));
        let item = &history.items()[0];
        assert!(item.matches("needle"));
        assert!(!item.matches("thread"));
        assert!(item.matches(""));
    }

    #[test]
    fn files_show_their_names_and_paste_as_a_copy() {
        let item = NewItem::files(
            b"# comment\r\nfile:///home/u/a%20b.txt\r\nfile:///home/u/%ED%95%9C.png\r\n".to_vec(),
        )
        .unwrap();
        assert_eq!(item.preview, "a b.txt, 한.png");
        let mut history = History::default();
        history.add(item, at(0));
        let item = &history.items()[0];
        assert!(item.matches("한.png"));
        assert!(!item.matches("home"));
        assert_eq!(
            &*item.payload(GNOME_COPIED_FILES),
            b"copy\nfile:///home/u/a%20b.txt\nfile:///home/u/%ED%95%9C.png"
        );
        assert_eq!(
            &*item.payload(URI_LIST),
            b"# comment\r\nfile:///home/u/a%20b.txt\r\nfile:///home/u/%ED%95%9C.png\r\n"
        );
        assert!(NewItem::files(b"# only a comment\r\n".to_vec()).is_none());
        assert!(NewItem::files(vec![0xff]).is_none());
    }

    #[test]
    fn served_formats_include_what_was_read() {
        let mut history = History::default();
        history.add(text("글"), at(0));
        let item = &history.items()[0];
        for mime in TEXT_MIMES {
            assert!(item.mime_types().contains(&mime), "{mime}");
        }
        assert_eq!(&*item.payload("text/plain"), "글".as_bytes());
    }

    #[test]
    fn thumbnails_keep_the_shape_of_the_image() {
        assert_eq!(thumbnail_size(0, 10), None);
        assert_eq!(thumbnail_size(10, 0), None);
        assert_eq!(thumbnail_size(100, 50), Some((100, 50)));
        assert_eq!(thumbnail_size(1280, 800), Some((128, 80)));
        assert_eq!(thumbnail_size(800, 1280), Some((80, 128)));
        assert_eq!(thumbnail_size(5000, 1), Some((128, 1)));
        assert_eq!(thumbnail_size(7680, 4320), Some((128, 72)));
        assert_eq!(thumbnail_size(7681, 4320), None);
    }

    #[test]
    fn images_get_a_small_premultiplied_thumbnail() {
        let item = NewItem::image(png_file(256, 128, [200, 100, 50, 255])).unwrap();
        assert_eq!(item.kind, Kind::Image);
        assert_eq!(item.preview, "256×128");
        let thumbnail = item.thumbnail.unwrap();
        assert_eq!((thumbnail.width, thumbnail.height), (128, 64));
        assert_eq!(thumbnail.pixels.len(), 128 * 64 * 4);
        assert_eq!(&thumbnail.pixels[..4], [200, 100, 50, 255]);

        let half = NewItem::image(png_file(2, 2, [200, 100, 0, 128])).unwrap();
        let thumbnail = half.thumbnail.unwrap();
        assert_eq!(&thumbnail.pixels[..4], [100, 50, 0, 128]);
    }

    #[test]
    fn damaged_images_are_not_recorded() {
        assert!(NewItem::image(b"not a png".to_vec()).is_none());
        let mut cut = png_file(64, 64, [1, 2, 3, 255]);
        cut.truncate(cut.len() / 2);
        assert!(NewItem::image(cut).is_none());
    }
}
