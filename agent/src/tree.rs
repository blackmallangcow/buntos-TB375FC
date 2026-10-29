// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 접근성 객체 목록을 에이전트가 읽을 짧은 트리 글로 만든다 (Aside의 snapshot()과 같은 꼴).
//! - 한 줄에 요소 하나: `- button "저장" [ref=e3] [focused]`. 자식 줄이 있으면 끝에 `:`, 들여쓰기 2칸
//! - 역할 이름은 ARIA(Playwright)의 것을 쓴다. 모델이 웹에서 익힌 이름 그대로다
//! - 뜻 없는 그릇(이름 없는 panel, filler, scroll pane 등)은 줄 없이 자식을 끌어올린다
//! - 누를 수 있는 요소와 범위로 쓸 그릇(창, 대화 상자, 목록 등)에 ref를 단다
//! - 숨은 요소는 뺀다. 긴 글은 자르고 남은 글자 수를 적는다

use std::collections::HashMap;

/// AT-SPI 역할 번호 (at-spi2-core의 atspi-constants.h)
pub mod role {
    pub const ACCELERATOR_LABEL: u32 = 1;
    pub const ALERT: u32 = 2;
    pub const ANIMATION: u32 = 3;
    pub const CALENDAR: u32 = 5;
    pub const CHECK_BOX: u32 = 7;
    pub const CHECK_MENU_ITEM: u32 = 8;
    pub const COLOR_CHOOSER: u32 = 9;
    pub const COLUMN_HEADER: u32 = 10;
    pub const COMBO_BOX: u32 = 11;
    pub const DIAL: u32 = 15;
    pub const DIALOG: u32 = 16;
    pub const FILE_CHOOSER: u32 = 19;
    pub const FONT_CHOOSER: u32 = 22;
    pub const FRAME: u32 = 23;
    pub const ICON: u32 = 26;
    pub const IMAGE: u32 = 27;
    pub const LABEL: u32 = 29;
    pub const LIST: u32 = 31;
    pub const LIST_ITEM: u32 = 32;
    pub const MENU: u32 = 33;
    pub const MENU_BAR: u32 = 34;
    pub const MENU_ITEM: u32 = 35;
    pub const PAGE_TAB: u32 = 37;
    pub const PAGE_TAB_LIST: u32 = 38;
    pub const PANEL: u32 = 39;
    pub const PASSWORD_TEXT: u32 = 40;
    pub const POPUP_MENU: u32 = 41;
    pub const PROGRESS_BAR: u32 = 42;
    pub const BUTTON: u32 = 43;
    pub const RADIO_BUTTON: u32 = 44;
    pub const RADIO_MENU_ITEM: u32 = 45;
    pub const ROW_HEADER: u32 = 47;
    pub const SCROLL_BAR: u32 = 48;
    pub const SEPARATOR: u32 = 50;
    pub const SLIDER: u32 = 51;
    pub const SPIN_BUTTON: u32 = 52;
    pub const STATUS_BAR: u32 = 54;
    pub const TABLE: u32 = 55;
    pub const TABLE_CELL: u32 = 56;
    pub const TABLE_COLUMN_HEADER: u32 = 57;
    pub const TABLE_ROW_HEADER: u32 = 58;
    pub const TEAROFF_MENU_ITEM: u32 = 59;
    pub const TERMINAL: u32 = 60;
    pub const TEXT: u32 = 61;
    pub const TOGGLE_BUTTON: u32 = 62;
    pub const TOOL_BAR: u32 = 63;
    pub const TOOL_TIP: u32 = 64;
    pub const TREE: u32 = 65;
    pub const TREE_TABLE: u32 = 66;
    pub const WINDOW: u32 = 69;
    pub const PARAGRAPH: u32 = 73;
    pub const RULER: u32 = 74;
    pub const APPLICATION: u32 = 75;
    pub const AUTOCOMPLETE: u32 = 76;
    pub const EDITBAR: u32 = 77;
    pub const ENTRY: u32 = 79;
    pub const CAPTION: u32 = 81;
    pub const DOCUMENT_FRAME: u32 = 82;
    pub const HEADING: u32 = 83;
    pub const FORM: u32 = 87;
    pub const LINK: u32 = 88;
    pub const TABLE_ROW: u32 = 90;
    pub const TREE_ITEM: u32 = 91;
    pub const DOCUMENT_SPREADSHEET: u32 = 92;
    pub const DOCUMENT_PRESENTATION: u32 = 93;
    pub const DOCUMENT_TEXT: u32 = 94;
    pub const DOCUMENT_WEB: u32 = 95;
    pub const DOCUMENT_EMAIL: u32 = 96;
    pub const LIST_BOX: u32 = 98;
    pub const IMAGE_MAP: u32 = 100;
    pub const NOTIFICATION: u32 = 101;
    pub const INFO_BAR: u32 = 102;
    pub const LEVEL_BAR: u32 = 103;
    pub const ARTICLE: u32 = 109;
    pub const LANDMARK: u32 = 110;
    pub const STATIC: u32 = 116;
    pub const DESCRIPTION_TERM: u32 = 122;
    pub const DESCRIPTION_VALUE: u32 = 123;
    pub const FOOTNOTE: u32 = 124;
    pub const PUSH_BUTTON_MENU: u32 = 129;
    pub const SWITCH: u32 = 130;
}

/// AT-SPI 상태 비트 (atspi-constants.h의 AtspiStateType)
pub mod state {
    pub const ACTIVE: u64 = 1 << 1;
    /// 불러오는 중 (Firefox의 문서가 페이지를 받는 동안)
    pub const BUSY: u64 = 1 << 3;
    pub const CHECKED: u64 = 1 << 4;
    pub const EDITABLE: u64 = 1 << 7;
    pub const ENABLED: u64 = 1 << 8;
    pub const EXPANDABLE: u64 = 1 << 9;
    pub const EXPANDED: u64 = 1 << 10;
    pub const FOCUSABLE: u64 = 1 << 11;
    pub const FOCUSED: u64 = 1 << 12;
    pub const MODAL: u64 = 1 << 16;
    pub const PRESSED: u64 = 1 << 20;
    pub const SELECTABLE: u64 = 1 << 22;
    pub const SELECTED: u64 = 1 << 23;
    pub const SENSITIVE: u64 = 1 << 24;
    pub const SHOWING: u64 = 1 << 25;
    pub const VISIBLE: u64 = 1 << 30;
    pub const INDETERMINATE: u64 = 1 << 32;
    pub const REQUIRED: u64 = 1 << 33;
    pub const INVALID_ENTRY: u64 = 1 << 36;
    pub const READ_ONLY: u64 = 1 << 43;
}

/// 객체가 내놓은 인터페이스
pub mod iface {
    pub const ACTION: u32 = 1;
    pub const VALUE: u32 = 1 << 1;
    pub const TEXT: u32 = 1 << 2;
    pub const EDITABLE_TEXT: u32 = 1 << 3;
    pub const COMPONENT: u32 = 1 << 4;
    pub const SELECTION: u32 = 1 << 5;

    /// D-Bus 인터페이스 이름에서
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> u32 {
        names.into_iter().fold(0, |set, name| {
            set | match name {
                "org.a11y.atspi.Action" => ACTION,
                "org.a11y.atspi.Value" => VALUE,
                "org.a11y.atspi.Text" => TEXT,
                "org.a11y.atspi.EditableText" => EDITABLE_TEXT,
                "org.a11y.atspi.Component" => COMPONENT,
                "org.a11y.atspi.Selection" => SELECTION,
                _ => 0,
            }
        })
    }
}

/// 접근성 객체 하나. 모양은 Cache.GetItems의 항목이고, 값과 글은 따로 읽어 채운다
#[derive(Clone, Debug, Default)]
pub struct Item {
    pub path: String,
    /// 부모의 경로. 부모가 다른 버스(레지스트리)에 있으면 빈 문자열
    pub parent: String,
    pub index: i32,
    pub child_count: i32,
    pub interfaces: u32,
    pub name: String,
    pub role: u32,
    pub description: String,
    pub states: u64,
    /// Value 인터페이스의 값 (막대, 숫자 칸)
    pub value: Option<Range>,
    /// 글자 칸의 내용, 이름 없는 글의 내용
    pub text: Option<String>,
    /// 글자 칸의 안내 글 (GetAttributes의 placeholder-text)
    pub placeholder: Option<String>,
    /// 제목의 단계 (GetAttributes의 level)
    pub level: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Range {
    pub current: f64,
    pub min: f64,
    pub max: f64,
}

impl Item {
    fn has(&self, state: u64) -> bool {
        self.states & state != 0
    }

    /// 화면에 보이는가 (숨은 위젯, 닫힌 팝오버의 내용은 VISIBLE이나 SHOWING이 없다)
    pub fn shown(&self) -> bool {
        self.has(state::VISIBLE) && self.has(state::SHOWING)
    }

    /// 불러오는 중인가 (페이지를 받는 문서)
    pub fn busy(&self) -> bool {
        self.has(state::BUSY)
    }

    /// 값을 따로 읽을 객체인가
    pub fn wants_value(&self) -> bool {
        self.interfaces & iface::VALUE != 0
    }

    /// 내용을 따로 읽을 객체인가. 암호 칸은 읽지 않는다. 글 요소 말고도 그릇(문단 밖의 div, 목록
    /// 항목, 칸)의 글을 읽는다: Firefox는 그릇에 바로 든 글을 자식으로 내놓지 않고 그릇의 Text에
    /// 넣으며, 자식 요소가 든 자리는 U+FFFC로 둔다
    pub fn wants_text(&self) -> bool {
        self.interfaces & iface::TEXT != 0
            && self.role != role::PASSWORD_TEXT
            && (self.is_editable()
                || match kind(self) {
                    Kind::Text => self.name.is_empty(),
                    Kind::Pass => true,
                    Kind::Line(role) => !control(role) && role != "window",
                    Kind::Skip => false,
                })
    }

    /// 속성(안내 글, 제목 단계)을 따로 읽을 객체인가
    pub fn wants_attributes(&self) -> bool {
        self.is_editable() || matches!(kind(self), Kind::Line(name) if name == "heading")
    }

    /// 글을 고칠 수 있는 칸: 글자 칸, 그리고 Firefox 주소 막대처럼 글을 쓰는 콤보 상자 등. Firefox는
    /// 창과 문서에도 EditableText를 붙이므로 EDITABLE 상태도 본다
    fn is_editable(&self) -> bool {
        (self.interfaces & iface::EDITABLE_TEXT != 0 && self.has(state::EDITABLE))
            || matches!(kind(self), Kind::Line(name) if name == "textbox")
    }
}

#[derive(Clone, Copy)]
pub struct Options {
    /// 상호작용할 수 있는 요소(와 그것을 담은 범위 그릇, 제목)만
    pub interactive: bool,
    /// 숨은 요소도
    pub hidden: bool,
}

/// 한 번에 내는 줄 수 상한. 넘으면 자르고 범위를 좁히라고 적는다
const MAX_LINES: usize = 2000;
/// 조상을 따라 올라가는 단계의 상한 (부모가 고리를 이루는 잘못된 목록에서 멈춘다)
const MAX_DEPTH: usize = 512;
/// 글 속에서 자식 요소가 든 자리 (U+FFFC OBJECT REPLACEMENT CHARACTER, Firefox의 하이퍼텍스트)
const OBJECT: char = '\u{FFFC}';
const MAX_NAME: usize = 120;
const MAX_TEXT: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// 줄 하나 (ARIA 역할 이름)
    Line(&'static str),
    /// 글 줄: `- text: "..."`
    Text,
    /// 줄 없이 자식만 끌어올린다
    Pass,
    /// 그리지 않는다
    Skip,
}

fn kind(item: &Item) -> Kind {
    let named = !item.name.is_empty();
    let line = |name| Kind::Line(name);
    let container = |name| if named { Kind::Line(name) } else { Kind::Pass };
    match item.role {
        role::FRAME | role::WINDOW => line("window"),
        role::DIALOG | role::FILE_CHOOSER | role::COLOR_CHOOSER | role::FONT_CHOOSER => {
            line("dialog")
        }
        role::ALERT => line("alertdialog"),
        role::BUTTON | role::PUSH_BUTTON_MENU | role::TOGGLE_BUTTON => line("button"),
        role::CHECK_BOX => line("checkbox"),
        role::RADIO_BUTTON => line("radio"),
        role::SWITCH => line("switch"),
        role::ENTRY
        | role::TEXT
        | role::PASSWORD_TEXT
        | role::TERMINAL
        | role::AUTOCOMPLETE
        | role::EDITBAR => line("textbox"),
        role::SPIN_BUTTON => line("spinbutton"),
        role::SLIDER | role::DIAL => line("slider"),
        role::COMBO_BOX => line("combobox"),
        role::LINK => line("link"),
        role::MENU | role::POPUP_MENU => line("menu"),
        role::MENU_BAR => line("menubar"),
        role::MENU_ITEM | role::TEAROFF_MENU_ITEM => line("menuitem"),
        role::CHECK_MENU_ITEM => line("menuitemcheckbox"),
        role::RADIO_MENU_ITEM => line("menuitemradio"),
        role::PAGE_TAB_LIST => line("tablist"),
        role::PAGE_TAB => line("tab"),
        role::LIST => line("list"),
        role::LIST_BOX => line("listbox"),
        role::LIST_ITEM => line("listitem"),
        role::TREE => line("tree"),
        role::TREE_TABLE => line("treegrid"),
        role::TREE_ITEM => line("treeitem"),
        role::TABLE => line("table"),
        role::TABLE_ROW => line("row"),
        role::TABLE_CELL => line("cell"),
        role::COLUMN_HEADER | role::TABLE_COLUMN_HEADER => line("columnheader"),
        role::ROW_HEADER | role::TABLE_ROW_HEADER => line("rowheader"),
        role::HEADING => line("heading"),
        role::PROGRESS_BAR => line("progressbar"),
        role::LEVEL_BAR => line("meter"),
        role::TOOL_BAR => line("toolbar"),
        role::STATUS_BAR | role::INFO_BAR => line("status"),
        role::NOTIFICATION => line("alert"),
        role::TOOL_TIP => line("tooltip"),
        role::CALENDAR => line("calendar"),
        role::IMAGE | role::ICON | role::ANIMATION | role::IMAGE_MAP => {
            if named {
                line("img")
            } else {
                Kind::Skip
            }
        }
        role::LABEL
        | role::STATIC
        | role::PARAGRAPH
        | role::CAPTION
        | role::ACCELERATOR_LABEL
        | role::DESCRIPTION_TERM
        | role::DESCRIPTION_VALUE
        | role::FOOTNOTE => Kind::Text,
        role::SEPARATOR | role::SCROLL_BAR | role::RULER => Kind::Skip,
        role::DOCUMENT_FRAME
        | role::DOCUMENT_WEB
        | role::DOCUMENT_TEXT
        | role::DOCUMENT_EMAIL
        | role::DOCUMENT_SPREADSHEET
        | role::DOCUMENT_PRESENTATION => container("document"),
        role::ARTICLE => container("article"),
        role::FORM => container("form"),
        role::LANDMARK => container("region"),
        // panel, filler, grouping, section, scroll pane, viewport 같은 그릇
        _ => container("group"),
    }
}

/// 늘 누르거나 쓰는 요소 (조작 위젯)
fn control(role: &str) -> bool {
    matches!(
        role,
        "button"
            | "checkbox"
            | "radio"
            | "switch"
            | "textbox"
            | "spinbutton"
            | "slider"
            | "combobox"
            | "link"
            | "menuitem"
            | "menuitemcheckbox"
            | "menuitemradio"
            | "tab"
    )
}

/// 목록·트리·표의 항목. 안에 조작 위젯이 없으면 항목 자체를 누르고(사이드바 항목), 있으면 그
/// 위젯들의 묶음이다(설정 행)
fn entry(role: &str) -> bool {
    matches!(role, "listitem" | "treeitem" | "row" | "cell")
}

/// 범위로 쓸 그릇인가 (ref를 달아 그 아래만 다시 읽게 한다)
fn scope(name: &str) -> bool {
    matches!(
        name,
        "window"
            | "dialog"
            | "alertdialog"
            | "menu"
            | "menubar"
            | "tablist"
            | "list"
            | "listbox"
            | "tree"
            | "treegrid"
            | "table"
            | "toolbar"
            | "group"
            | "document"
            | "form"
            | "region"
    )
}

/// 객체 목록의 부모·자식 연결
pub struct Tree<'a> {
    by_path: HashMap<&'a str, &'a Item>,
    children: HashMap<&'a str, Vec<&'a Item>>,
}

impl<'a> Tree<'a> {
    pub fn new(items: &'a [Item]) -> Self {
        let mut children: HashMap<&str, Vec<&Item>> = HashMap::new();
        for item in items {
            // 목록에 남아 있어도 보이지 않는 자식은 번호가 음수다
            if !item.parent.is_empty() && item.index >= 0 {
                children.entry(&item.parent).or_default().push(item);
            }
        }
        for list in children.values_mut() {
            list.sort_by_key(|item| item.index);
        }
        Self {
            by_path: items
                .iter()
                .map(|item| (item.path.as_str(), item))
                .collect(),
            children,
        }
    }

    pub fn get(&self, path: &str) -> Option<&'a Item> {
        self.by_path.get(path).copied()
    }

    pub fn children(&self, path: &str) -> &[&'a Item] {
        self.children.get(path).map_or(&[], Vec::as_slice)
    }

    /// 보이는 자식 수가 알려진 수보다 적은 객체 (아직 GTK가 만들지 않은 자식이 있다)
    pub fn missing(&self) -> Vec<&'a str> {
        self.by_path
            .values()
            .filter(|item| {
                usize::try_from(item.child_count)
                    .is_ok_and(|count| count > self.children(&item.path).len())
            })
            .map(|item| item.path.as_str())
            .collect()
    }
}

/// GTK4는 더 내놓지 않는 객체(다른 페이지로 넘긴 GtkStack 페이지의 내용 등)를 목록(캐시)에 VISIBLE,
/// SHOWING인 채 남긴다. 그 부모는 자식 수를 줄여 알리므로, 번호가 음수이거나 부모의 자식 수를 넘는
/// 객체와 그 아래를 숨은 것으로 고친다. 루트는 창이 있어도 자식 수를 0으로 알리므로 창은 그대로 둔다
pub fn hide_detached(items: &mut [Item]) {
    let index: HashMap<&str, usize> = items
        .iter()
        .enumerate()
        .map(|(at, item)| (item.path.as_str(), at))
        .collect();
    let detached = |start: usize| {
        let mut item = &items[start];
        for _ in 0..MAX_DEPTH {
            let Some(&at) = index.get(item.parent.as_str()) else {
                return false;
            };
            let parent = &items[at];
            if parent.path == crate::atspi::ROOT {
                return false;
            }
            if item.index < 0 || item.index >= parent.child_count {
                return true;
            }
            item = parent;
        }
        false
    };
    let hidden: Vec<usize> = (0..items.len()).filter(|&at| detached(at)).collect();
    for at in hidden {
        items[at].states &= !(state::VISIBLE | state::SHOWING);
    }
}

/// 렌더 중의 상태
struct Render<'a, 'r> {
    tree: &'a Tree<'a>,
    options: &'a Options,
    refs: &'r mut dyn FnMut(&str) -> String,
    lines: Vec<String>,
    /// 줄마다 쓴 이름 (lines와 같은 차례)
    names: Vec<String>,
    cut: bool,
}

/// `roots` 아래를 트리 글의 줄로 만든다. `refs`는 객체 경로에 ref 이름을 준다(세션이 기억한다)
pub fn render(
    tree: &Tree,
    roots: &[&str],
    options: &Options,
    refs: &mut dyn FnMut(&str) -> String,
) -> Vec<String> {
    let mut render = Render {
        tree,
        options,
        refs,
        lines: Vec::new(),
        names: Vec::new(),
        cut: false,
    };
    for root in roots {
        if let Some(item) = tree.get(root) {
            render.node(item, 0, TOP);
        }
    }
    let mut lines = render.lines;
    if render.cut {
        lines.push(format!(
            "# note: cut at {MAX_LINES} lines; pass a ref to read one part"
        ));
    }
    lines
}

/// 위 줄의 역할과 이름. 아래에서 같은 것이 되풀이되면 뺀다
#[derive(Clone, Copy)]
struct Parent<'p> {
    role: &'p str,
    name: &'p str,
}

const TOP: Parent<'static> = Parent { role: "", name: "" };

impl Render<'_, '_> {
    /// 요소 하나와 그 아래
    fn node(&mut self, item: &Item, depth: usize, parent: Parent) {
        if self.cut {
            return;
        }
        if !self.options.hidden && !item.shown() {
            return;
        }
        match kind(item) {
            Kind::Skip => {}
            Kind::Pass => self.contents(item, depth, parent),
            // Firefox의 문단은 글 사이에 링크 같은 자식을 둔다
            Kind::Text if item.name.is_empty() && !self.tree.children(&item.path).is_empty() => {
                self.contents(item, depth, parent)
            }
            Kind::Text => {
                let text = text_of(item);
                if !self.options.interactive && !text.is_empty() && text != parent.name {
                    self.push(depth, format!("- text: {}", quote(&text, MAX_TEXT)), &text);
                }
            }
            Kind::Line(role) => self.line_node(item, role, depth, parent),
        }
    }

    /// 자식과, 요소의 Text에 바로 든 글. Firefox는 자식 요소가 든 자리를 U+FFFC로 두므로 그 자리에
    /// 자식을 끼워 읽는 차례를 지킨다. 자리 수가 자식 수와 다르면 글을 먼저 낸다
    fn contents(&mut self, item: &Item, depth: usize, parent: Parent) {
        let Some(text) = item.text.as_deref().filter(|_| !item.is_editable()) else {
            return self.children(item, depth, parent);
        };
        let children = self.tree.children(&item.path);
        let chunks: Vec<&str> = text.split(OBJECT).collect();
        if chunks.len() == children.len() + 1 {
            for (index, chunk) in chunks.iter().enumerate() {
                self.own_text(chunk, depth, parent);
                if let Some(child) = children.get(index) {
                    self.node(child, depth, parent);
                }
            }
        } else {
            self.own_text(&text.replace(OBJECT, " "), depth, parent);
            self.children(item, depth, parent);
        }
    }

    /// 그릇에 바로 든 글 한 조각. 위 줄의 이름을 되풀이하면 뺀다
    fn own_text(&mut self, chunk: &str, depth: usize, parent: Parent) {
        let text = collapse(chunk);
        if !self.options.interactive && !text.is_empty() && text != parent.name {
            self.push(depth, format!("- text: {}", quote(&text, MAX_TEXT)), &text);
        }
    }

    fn line_node(&mut self, item: &Item, role: &'static str, depth: usize, parent: Parent) {
        let is_interactive = self.interactive(item, role);
        // 이름이 없으면 아래의 글 하나(단추, 목록 항목), 항목은 첫 글, 글자 칸은 안내 글을 이름으로
        // 쓴다 (브라우저의 이름 계산과 같다)
        let content;
        let name = if item.name.is_empty() {
            content = self
                .content_name(item)
                .or_else(|| entry(role).then(|| self.first_text(item)).flatten())
                .or_else(|| item.placeholder.clone().filter(|_| role == "textbox"))
                .unwrap_or_default();
            content.as_str()
        } else {
            item.name.as_str()
        };
        // 메뉴 단추 안의 단추, 단추와 이름이 같은 아이콘처럼 위 줄을 되풀이하는 요소
        let repeats =
            !name.is_empty() && name == parent.name && (role == parent.role || role == "img");
        let keep = !repeats
            && (!self.options.interactive
                || is_interactive
                || role == "heading"
                || scope(role)
                || (entry(role) && !name.is_empty()));
        if !keep {
            self.children(item, depth, parent);
            return;
        }
        let at = self.lines.len();
        let line = self.line(item, role, name, is_interactive);
        self.push(depth, line, name);
        if self.lines.len() == at {
            return;
        }
        if control(role) {
            self.children(item, depth + 1, Parent { role, name });
        } else {
            self.contents(item, depth + 1, Parent { role, name });
        }
        let below = self.lines.len() - at - 1;
        if entry(role)
            && !is_interactive
            && below == 1
            && (name.is_empty() || self.names[at + 1] == name)
        {
            // 조작 위젯 하나만 든 항목(설정 행)은 그 위젯만 남긴다
            let child = self.lines.remove(at + 1);
            self.lines[at] = child[2..].to_string();
            self.names.remove(at);
        } else if below > 0 {
            self.lines[at].push(':');
        } else if self.options.interactive && depth > 0 && !is_interactive && role != "heading" {
            // 상호작용할 요소가 없는 그릇은 뺀다 (맨 위의 창은 남긴다)
            self.lines.truncate(at);
            self.names.truncate(at);
        }
    }

    fn children(&mut self, item: &Item, depth: usize, parent: Parent) {
        for child in self.tree.children(&item.path) {
            self.node(child, depth, parent);
        }
    }

    /// 사람이 누르거나 쓰는 요소인가
    fn interactive(&self, item: &Item, role: &str) -> bool {
        let focusable = item.has(state::FOCUSABLE) || item.has(state::SELECTABLE);
        if control(role) {
            true
        } else if entry(role) {
            focusable && !self.has_control(item)
        } else {
            focusable && (role == "img" || role == "group")
        }
    }

    /// 아래에 보이는 조작 위젯이 있는가
    fn has_control(&self, item: &Item) -> bool {
        let mut stack: Vec<&Item> = self.tree.children(&item.path).to_vec();
        while let Some(child) = stack.pop() {
            if !child.shown() {
                continue;
            }
            if let Kind::Line(role) = kind(child)
                && control(role)
            {
                return true;
            }
            stack.extend(self.tree.children(&child.path));
        }
        false
    }

    /// 아래의 보이는 글이 하나뿐이고 다른 요소가 없으면 그 글
    fn content_name(&self, item: &Item) -> Option<String> {
        let mut found = None;
        let mut stack: Vec<&Item> = self.tree.children(&item.path).to_vec();
        while let Some(child) = stack.pop() {
            if !child.shown() {
                continue;
            }
            match kind(child) {
                Kind::Text => {
                    let text = text_of(child);
                    if !text.is_empty() && found.replace(text).is_some() {
                        return None;
                    }
                }
                Kind::Pass | Kind::Skip => stack.extend(self.tree.children(&child.path)),
                Kind::Line(_) => return None,
            }
        }
        found
    }

    /// 아래의 첫 글 (조작 위젯 안은 보지 않는다)
    fn first_text(&self, item: &Item) -> Option<String> {
        for child in self.tree.children(&item.path) {
            if !child.shown() {
                continue;
            }
            match kind(child) {
                Kind::Text => {
                    let text = text_of(child);
                    if !text.is_empty() {
                        return Some(text);
                    }
                }
                Kind::Line(role) if control(role) => continue,
                _ => {}
            }
            if let Some(text) = self.first_text(child) {
                return Some(text);
            }
        }
        None
    }

    fn push(&mut self, depth: usize, line: String, name: &str) {
        if self.lines.len() >= MAX_LINES {
            self.cut = true;
            return;
        }
        self.lines.push(format!("{}{line}", "  ".repeat(depth)));
        self.names.push(name.to_string());
    }

    fn line(&mut self, item: &Item, role: &str, name: &str, is_interactive: bool) -> String {
        let mut line = format!("- {role}");
        if !name.is_empty() {
            line.push(' ');
            line.push_str(&quote(name, MAX_NAME));
        }
        if is_interactive || scope(role) {
            line.push_str(&format!(" [ref={}]", (self.refs)(&item.path)));
        }
        for attribute in attributes(item, role, name, is_interactive) {
            line.push_str(&format!(" [{attribute}]"));
        }
        line
    }
}

/// 글 요소의 내용: 이름, 없으면 Text 인터페이스로 읽은 글 (자식 자리 U+FFFC는 뺀다)
fn text_of(item: &Item) -> String {
    let text = if item.name.is_empty() {
        item.text.as_deref().unwrap_or("")
    } else {
        &item.name
    };
    collapse(&text.replace(OBJECT, " "))
}

/// 공백을 하나로 줄이고 앞뒤 공백을 뗀다
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 줄 끝에 붙는 상태와 값. `name`은 줄에 쓴 이름이다
fn attributes(item: &Item, role: &str, name: &str, is_interactive: bool) -> Vec<String> {
    let mut out = Vec::new();
    if !item.shown() {
        out.push("hidden".into());
    }
    if is_interactive && !item.has(state::SENSITIVE) && !item.has(state::ENABLED) {
        out.push("disabled".into());
    }
    if item.has(state::ACTIVE) && role == "window" {
        out.push("active".into());
    }
    if item.has(state::MODAL) {
        out.push("modal".into());
    }
    if item.has(state::FOCUSED) {
        out.push("focused".into());
    }
    if let Some(level) = item.level.as_deref().filter(|_| role == "heading") {
        out.push(format!("level={level}"));
    }
    let checkable = matches!(
        role,
        "checkbox" | "radio" | "switch" | "menuitemcheckbox" | "menuitemradio"
    );
    if item.has(state::INDETERMINATE) && checkable {
        out.push("mixed".into());
    } else if item.has(state::CHECKED) && checkable {
        out.push("checked".into());
    }
    if role == "button" && (item.has(state::PRESSED) || item.has(state::CHECKED)) {
        out.push("pressed".into());
    }
    if item.has(state::SELECTED) {
        out.push("selected".into());
    }
    if item.has(state::EXPANDABLE) {
        let expanded = item.has(state::EXPANDED);
        out.push(if expanded { "expanded" } else { "collapsed" }.into());
    }
    if role == "textbox" {
        if item.role == role::PASSWORD_TEXT {
            out.push("secret".into());
        } else if item.has(state::READ_ONLY) || !item.has(state::EDITABLE) {
            out.push("readonly".into());
        }
    }
    if item.has(state::REQUIRED) {
        out.push("required".into());
    }
    if item.has(state::INVALID_ENTRY) {
        out.push("invalid".into());
    }
    if let Some(range) = &item.value {
        out.push(format!("value={}", number(range.current)));
        out.push(format!(
            "range={}..{}",
            number(range.min),
            number(range.max)
        ));
    }
    if item.is_editable() && item.role != role::PASSWORD_TEXT {
        if let Some(text) = item
            .text
            .as_deref()
            .map(|text| text.replace(OBJECT, ""))
            .filter(|text| !text.is_empty())
        {
            out.push(format!("value={}", quote(&text, MAX_TEXT)));
        }
        if let Some(placeholder) = item
            .placeholder
            .as_deref()
            .filter(|text| !text.is_empty() && *text != name)
        {
            out.push(format!("placeholder={}", quote(placeholder, MAX_NAME)));
        }
    }
    if !item.description.is_empty() && item.description != name {
        out.push(format!(
            "description={}",
            quote(&item.description, MAX_NAME)
        ));
    }
    out
}

/// 따옴표에 넣고, 길면 자른다: `"앞부분…[+120 chars]"`
fn quote(text: &str, limit: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| {
            if c == '\n' || c == '\r' || c == '\t' {
                ' '
            } else {
                c
            }
        })
        .collect();
    let count = flat.chars().count();
    let shown: String = flat.chars().take(limit).collect();
    let escaped = shown.replace('\\', "\\\\").replace('"', "\\\"");
    if count > limit {
        format!("\"{escaped}…[+{} chars]\"", count - limit)
    } else {
        format!("\"{escaped}\"")
    }
}

/// 값을 짧게: 0.5, 40, 0.333
fn number(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded == rounded.trunc() {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHOWN: u64 = state::VISIBLE | state::SHOWING | state::SENSITIVE | state::ENABLED;

    fn item(path: &str, parent: &str, index: i32, role: u32, name: &str) -> Item {
        Item {
            path: path.into(),
            parent: parent.into(),
            index,
            role,
            name: name.into(),
            states: SHOWN,
            ..Item::default()
        }
    }

    /// 창 > 이름 없는 상자 > (글 "음량", 막대) / 단추 "저장"(안에 같은 글) / 숨은 단추
    fn sample() -> Vec<Item> {
        let mut slider = item("/s", "/box", 1, role::SLIDER, "");
        slider.value = Some(Range {
            current: 0.4,
            min: 0.0,
            max: 1.0,
        });
        let mut hidden = item("/h", "/w", 2, role::BUTTON, "숨김");
        hidden.states = state::SENSITIVE;
        let mut window = item("/w", "", 0, role::FRAME, "설정");
        window.states |= state::ACTIVE;
        vec![
            window,
            item("/box", "/w", 0, role::PANEL, ""),
            item("/l", "/box", 0, role::LABEL, "음량"),
            slider,
            item("/b", "/w", 1, role::BUTTON, "저장"),
            item("/bl", "/b", 0, role::LABEL, "저장"),
            hidden,
        ]
    }

    fn refs() -> impl FnMut(&str) -> String {
        let mut known: Vec<String> = Vec::new();
        move |path: &str| {
            let at = known.iter().position(|p| p == path).unwrap_or_else(|| {
                known.push(path.into());
                known.len() - 1
            });
            format!("e{}", at + 1)
        }
    }

    #[test]
    fn web_text_is_read_in_order_around_its_links() {
        // Firefox: 문서와 div는 이름 없이 바로 든 글을 Text에 두고, 자식 자리에 U+FFFC를 둔다. 창과
        // 문서에도 EditableText가 붙지만 EDITABLE이 아니면 값이 아니다
        let mut window = item("/w", "", 0, role::FRAME, "Mozilla Firefox");
        window.interfaces = iface::TEXT | iface::EDITABLE_TEXT;
        window.text = Some("\u{fffc}".into());
        let mut document = item("/d", "/w", 0, role::DOCUMENT_WEB, "날씨");
        document.interfaces = iface::TEXT | iface::EDITABLE_TEXT;
        document.text = Some("\u{fffc}\u{fffc}".into());
        let mut overview = item("/s", "/d", 0, role::PANEL, "");
        overview.interfaces = iface::TEXT;
        overview.text = Some("오늘 서울은  \u{fffc} 이고\n밤부터 비가 옵니다".into());
        let mut link = item("/a", "/s", 0, role::LINK, "흐림");
        link.states |= state::FOCUSABLE;
        link.interfaces = iface::TEXT | iface::ACTION;
        let mut paragraph = item("/p", "/d", 1, role::PARAGRAPH, "");
        paragraph.interfaces = iface::TEXT;
        paragraph.text = Some("기온 12°C".into());
        let items = vec![window, document, overview, link, paragraph];
        let tree = Tree::new(&items);
        let options = Options {
            interactive: false,
            hidden: false,
        };
        let lines = render(&tree, &["/w"], &options, &mut refs());
        assert_eq!(
            lines,
            [
                "- window \"Mozilla Firefox\" [ref=e1]:",
                "  - document \"날씨\" [ref=e2]:",
                "    - text: \"오늘 서울은\"",
                "    - link \"흐림\" [ref=e3]",
                "    - text: \"이고 밤부터 비가 옵니다\"",
                "    - text: \"기온 12°C\"",
            ]
        );
        assert!(items[2].wants_text() && items[4].wants_text());
        assert!(
            !items[3].wants_text(),
            "links read their name, not their text"
        );
    }

    #[test]
    fn a_loading_document_is_busy() {
        let mut document = item("/d", "/w", 0, role::DOCUMENT_WEB, "");
        assert!(!document.busy());
        document.states |= state::BUSY;
        assert!(document.busy());
    }

    #[test]
    fn pages_gtk4_stopped_presenting_are_hidden() {
        // 시스템 정보: 파일시스템 탭으로 넘기면 "자원" 페이지는 자식 수를 0으로 알리지만 목록에는 그
        // 아래가 VISIBLE, SHOWING인 채 남는다. 루트는 창이 있어도 자식 수가 0이다
        let mut root = item(crate::atspi::ROOT, "", 0, role::APPLICATION, "앱");
        root.child_count = 0;
        let mut window = item("/w", crate::atspi::ROOT, 0, role::FRAME, "시스템 정보");
        window.child_count = 2;
        let resources = item("/r", "/w", 0, role::PANEL, "자원");
        let graph = item("/g", "/r", 0, role::BUTTON, "CPU");
        let label = item("/l", "/g", 0, role::LABEL, "CPU1");
        let mut files = item("/f", "/w", 1, role::PANEL, "파일시스템");
        files.child_count = 1;
        let table = item("/t", "/f", 0, role::TABLE, "");
        let mut items = vec![root, window, resources, graph, label, files, table];
        hide_detached(&mut items);
        let shown: Vec<&str> = items
            .iter()
            .filter(|item| item.shown())
            .map(|item| item.path.as_str())
            .collect();
        assert_eq!(shown, [crate::atspi::ROOT, "/w", "/r", "/f", "/t"]);
    }

    #[test]
    fn renders_a_compact_tree() {
        let items = sample();
        let tree = Tree::new(&items);
        let options = Options {
            interactive: false,
            hidden: false,
        };
        let lines = render(&tree, &["/w"], &options, &mut refs());
        assert_eq!(
            lines,
            [
                "- window \"설정\" [ref=e1] [active]:",
                "  - text: \"음량\"",
                "  - slider [ref=e2] [value=0.4] [range=0..1]",
                "  - button \"저장\" [ref=e3]",
            ]
        );
    }

    #[test]
    fn interactive_mode_keeps_controls_and_their_scope() {
        let items = sample();
        let tree = Tree::new(&items);
        let options = Options {
            interactive: true,
            hidden: false,
        };
        let lines = render(&tree, &["/w"], &options, &mut refs());
        assert_eq!(
            lines,
            [
                "- window \"설정\" [ref=e1] [active]:",
                "  - slider [ref=e2] [value=0.4] [range=0..1]",
                "  - button \"저장\" [ref=e3]",
            ]
        );
    }

    #[test]
    fn hidden_elements_appear_only_when_asked() {
        let items = sample();
        let tree = Tree::new(&items);
        let options = Options {
            interactive: true,
            hidden: true,
        };
        let lines = render(&tree, &["/w"], &options, &mut refs());
        assert_eq!(
            lines.last().unwrap(),
            "  - button \"숨김\" [ref=e4] [hidden]"
        );
    }

    #[test]
    fn states_and_secrets() {
        let mut switch = item("/sw", "", 0, role::SWITCH, "Wi-Fi");
        switch.states |= state::CHECKED | state::FOCUSED;
        let mut password = item("/pw", "", 0, role::PASSWORD_TEXT, "암호");
        password.states |= state::EDITABLE;
        password.text = Some("leak".into());
        let mut off = item("/off", "", 0, role::BUTTON, "연결");
        off.states = state::VISIBLE | state::SHOWING;
        let items = vec![switch, password, off];
        let tree = Tree::new(&items);
        let options = Options {
            interactive: false,
            hidden: false,
        };
        let lines = render(&tree, &["/sw", "/pw", "/off"], &options, &mut refs());
        assert_eq!(
            lines,
            [
                "- switch \"Wi-Fi\" [ref=e1] [focused] [checked]",
                "- textbox \"암호\" [ref=e2] [secret]",
                "- button \"연결\" [ref=e3] [disabled]",
            ]
        );
        // 데몬은 암호 칸의 내용을 읽지도 않는다
        assert!(!items[1].wants_text());
    }

    #[test]
    fn editable_comboboxes_show_their_text() {
        // Firefox 주소 막대: 글을 쓰는 콤보 상자
        let mut bar = item("/bar", "", 0, role::COMBO_BOX, "검색 또는 주소 입력");
        bar.states |= state::EDITABLE | state::FOCUSABLE;
        bar.interfaces = iface::TEXT | iface::EDITABLE_TEXT;
        bar.text = Some("example.org".into());
        assert!(bar.wants_text());
        let items = vec![bar];
        let tree = Tree::new(&items);
        let options = Options {
            interactive: true,
            hidden: false,
        };
        let lines = render(&tree, &["/bar"], &options, &mut refs());
        assert_eq!(
            lines,
            ["- combobox \"검색 또는 주소 입력\" [ref=e1] [value=\"example.org\"]"]
        );
    }

    #[test]
    fn names_come_from_a_lone_label_and_repeats_fold() {
        // 목록 > 항목 > 상자 > (이름 없는 아이콘, 글 "Wi-Fi")
        // 목록 > 메뉴 단추 "사운드" > 단추 "사운드" > 아이콘 "사운드" (셸의 메뉴 막대 항목)
        let mut row = item("/row", "/list", 0, role::LIST_ITEM, "");
        row.states |= state::FOCUSABLE | state::SELECTED;
        let mut search = item("/search", "/list", 2, role::ENTRY, "");
        search.states |= state::EDITABLE;
        search.text = Some(String::new());
        search.placeholder = Some("검색".into());
        let items = vec![
            item("/list", "", 0, role::LIST, ""),
            row,
            item("/box", "/row", 0, role::PANEL, ""),
            item("/icon", "/box", 0, role::IMAGE, ""),
            item("/label", "/box", 1, role::LABEL, "Wi-Fi"),
            item("/menu", "/list", 1, role::BUTTON, "사운드"),
            item("/inner", "/menu", 0, role::TOGGLE_BUTTON, "사운드"),
            item("/img", "/inner", 0, role::IMAGE, "사운드"),
            search,
        ];
        let tree = Tree::new(&items);
        for interactive in [false, true] {
            let options = Options {
                interactive,
                hidden: false,
            };
            let lines = render(&tree, &["/list"], &options, &mut refs());
            assert_eq!(
                lines,
                [
                    "- list [ref=e1]:",
                    "  - listitem \"Wi-Fi\" [ref=e2] [selected]",
                    "  - button \"사운드\" [ref=e3]",
                    "  - textbox \"검색\" [ref=e4]",
                ]
            );
        }
    }

    #[test]
    fn setting_rows_keep_their_controls_and_context() {
        // 목록 > 행(글 "키 반복 속도", 막대 "키 반복 속도", 글 "끔")
        //      > 행(글 "한글", 글 "한국어", 단추 "지우기")
        let row = |path: &str, index| {
            let mut row = item(path, "/list", index, role::LIST_ITEM, "");
            row.states |= state::FOCUSABLE | state::SELECTABLE;
            row
        };
        let items = vec![
            item("/list", "", 0, role::LIST, ""),
            row("/r1", 0),
            item("/r1t", "/r1", 0, role::LABEL, "키 반복 속도"),
            item("/r1s", "/r1", 1, role::SLIDER, "키 반복 속도"),
            item("/r1l", "/r1", 2, role::LABEL, "끔"),
            row("/r2", 1),
            item("/r2t", "/r2", 0, role::LABEL, "한글"),
            item("/r2d", "/r2", 1, role::LABEL, "한국어"),
            item("/r2b", "/r2", 2, role::BUTTON, "지우기"),
        ];
        let tree = Tree::new(&items);
        let full = Options {
            interactive: false,
            hidden: false,
        };
        assert_eq!(
            render(&tree, &["/list"], &full, &mut refs()),
            [
                "- list [ref=e1]:",
                "  - listitem \"키 반복 속도\":",
                "    - slider \"키 반복 속도\" [ref=e2]",
                "    - text: \"끔\"",
                "  - listitem \"한글\":",
                "    - text: \"한국어\"",
                "    - button \"지우기\" [ref=e3]",
            ]
        );
        let interactive = Options {
            interactive: true,
            hidden: false,
        };
        assert_eq!(
            render(&tree, &["/list"], &interactive, &mut refs()),
            [
                "- list [ref=e1]:",
                "  - slider \"키 반복 속도\" [ref=e2]",
                "  - listitem \"한글\":",
                "    - button \"지우기\" [ref=e3]",
            ]
        );
    }

    #[test]
    fn long_text_is_cut_with_a_count() {
        let long = "가".repeat(MAX_TEXT + 5);
        assert_eq!(
            quote(&long, MAX_TEXT),
            format!("\"{}…[+5 chars]\"", "가".repeat(MAX_TEXT))
        );
        assert_eq!(quote("a \"b\"\nc", 50), "\"a \\\"b\\\" c\"");
    }

    #[test]
    fn missing_children_are_found() {
        let mut window = item("/w", "", 0, role::FRAME, "");
        window.child_count = 2;
        let items = vec![window, item("/a", "/w", 0, role::BUTTON, "a")];
        assert_eq!(Tree::new(&items).missing(), ["/w"]);
    }
}
