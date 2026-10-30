// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 스냅샷 트리 글(tree.rs가 만든 줄)을 다시 읽는다. 기다리기(wait.rs)와 REPL의 getByRole·getByText가 모델이
//! 보는 것과 같은 줄로 요소를 찾도록, 줄을 역할·이름·ref·상태·값으로 나눈다.
//!
//! 줄의 꼴: `- button "저장" [ref=e3] [focused]`, `- textbox [ref=e9] [value="6×7"]`, `- text: "42"`.
//! 자식 줄이 있으면 끝에 `:`가 붙고 들여쓰기는 2칸이다. `#`로 시작하는 줄은 알림이고 `- app:` 줄은 머리줄이다.

/// 잘린 글의 꼬리: `"앞부분…[+120 chars]"`
const CUT_MARK: &str = "…[+";

/// 트리 글의 요소 한 줄
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Line {
    /// 들여쓰기 단계. 범위의 맨 위(창, ref로 읽은 요소)가 0이다
    pub depth: usize,
    /// ARIA 역할 (`button`, `textbox`, `window` …). 글 줄은 `text`
    pub role: String,
    /// 이름 (글 줄은 그 글). 잘렸으면 보이는 앞부분이다
    pub name: String,
    pub reference: Option<String>,
    /// 값 없는 상태 (`focused`, `checked`, `offscreen` …)
    pub states: Vec<String>,
    /// 값 있는 속성 (`value`, `range`, `level`, `placeholder`, `description`). 따옴표는 벗겼다
    pub props: Vec<(String, String)>,
    /// 트리가 길어 자른 것: `name`이나 속성 이름 (보이는 앞부분만 남았다)
    pub cut: Vec<String>,
    /// 들여쓰기를 뺀 원래 줄
    pub text: String,
}

impl Line {
    pub fn prop(&self, key: &str) -> Option<&str> {
        self.props
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    pub fn has(&self, state: &str) -> bool {
        self.states.iter().any(|known| known == state)
    }

    /// 요소의 값: 글 칸·막대의 `value`, 없으면 이름
    pub fn value(&self) -> &str {
        self.prop("value").unwrap_or(&self.name)
    }

    /// `value()`가 잘린 글인가
    pub fn value_cut(&self) -> bool {
        let key = if self.prop("value").is_some() {
            "value"
        } else {
            "name"
        };
        self.is_cut(key)
    }

    pub fn is_cut(&self, key: &str) -> bool {
        self.cut.iter().any(|cut| cut == key)
    }
}

/// 트리 글을 요소 줄로 나눈다. 알림(`#`)과 앱 머리줄은 뺀다
pub fn parse(tree: &str) -> Vec<Line> {
    tree.lines().filter_map(parse_line).collect()
}

fn parse_line(raw: &str) -> Option<Line> {
    let body = raw.trim_start_matches(' ');
    let depth = (raw.len() - body.len()) / 2;
    let rest = body.strip_prefix("- ")?;
    if rest.starts_with("app: ") {
        return None;
    }
    let mut line = Line {
        depth,
        text: body.to_string(),
        ..Line::default()
    };
    if let Some(text) = rest.strip_prefix("text: ") {
        line.role = "text".into();
        let (name, _) = quoted(text)?;
        line.set_text("name", &name);
        return Some(line);
    }
    let role_end = rest.find([' ', ':']).unwrap_or(rest.len());
    line.role = rest[..role_end].to_string();
    let mut rest = &rest[role_end..];
    loop {
        rest = rest.trim_start_matches(' ');
        if rest.starts_with('"') && line.name.is_empty() && line.reference.is_none() {
            let (name, after) = quoted(rest)?;
            line.set_text("name", &name);
            rest = after;
        } else if let Some(inner) = rest.strip_prefix('[') {
            let (attribute, after) = bracket(inner)?;
            match attribute.split_once('=') {
                Some(("ref", reference)) => line.reference = Some(reference.to_string()),
                Some((key, value)) => match quoted(value) {
                    Some((text, "")) => line.set_text(key, &text),
                    _ => line.props.push((key.to_string(), value.to_string())),
                },
                None => line.states.push(attribute.to_string()),
            }
            rest = after;
        } else {
            // 끝, 또는 자식 줄이 있다는 `:`
            break;
        }
    }
    Some(line)
}

/// `"…"`를 읽어 따옴표를 벗긴 글과 그 뒤를 돌려준다 (tree.rs의 quote가 `\`와 `"`를 \로 막는다)
fn quoted(text: &str) -> Option<(String, &str)> {
    let inner = text.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = inner.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '\\' => out.push(chars.next()?.1),
            '"' => return Some((out, &inner[at + 1..])),
            c => out.push(c),
        }
    }
    None
}

/// `[` 다음부터 짝이 맞는 `]`까지의 속성 글과 그 뒤. 따옴표 안의 `]`는 건너뛴다
fn bracket(text: &str) -> Option<(&str, &str)> {
    let mut quoted = false;
    let mut escaped = false;
    for (at, c) in text.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ']' if !quoted => return Some((&text[..at], &text[at + 1..])),
            _ => {}
        }
    }
    None
}

impl Line {
    /// 따옴표에서 꺼낸 이름이나 속성 값을 넣는다. 잘린 글이면 꼬리를 떼고 `cut`에 적는다
    fn set_text(&mut self, key: &str, text: &str) {
        let visible = match text.rfind(CUT_MARK) {
            Some(at) if text.ends_with(" chars]") => {
                self.cut.push(key.to_string());
                &text[..at]
            }
            _ => text,
        };
        if key == "name" {
            self.name = visible.to_string();
        } else {
            self.props.push((key.to_string(), visible.to_string()));
        }
    }
}

/// 요소를 고르는 조건 (Playwright의 getByRole, getByText)
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Matcher {
    /// ARIA 역할. 대소문자를 가리지 않는다
    pub role: Option<String>,
    /// 이름에 이 글이 든 요소
    pub name: Option<String>,
    /// 이름, 글 줄, 값에 이 글이 든 줄
    pub text: Option<String>,
    /// 이름과 글이 통째로 같아야 한다 (대소문자도). 아니면 대소문자를 가리지 않고 일부만 맞아도 된다
    pub exact: bool,
}

impl Matcher {
    pub fn is_empty(&self) -> bool {
        self.role.is_none() && self.name.is_none() && self.text.is_none()
    }

    pub fn matches(&self, line: &Line) -> bool {
        if let Some(role) = &self.role
            && !role.eq_ignore_ascii_case(&line.role)
        {
            return false;
        }
        if let Some(name) = &self.name
            && (line.role == "text" || !fits(name, &line.name, line.is_cut("name"), self.exact))
        {
            return false;
        }
        if let Some(text) = &self.text {
            let in_value = line
                .prop("value")
                .is_some_and(|value| fits(text, value, line.is_cut("value"), self.exact));
            if !fits(text, &line.name, line.is_cut("name"), self.exact) && !in_value {
                return false;
            }
        }
        true
    }

    /// 사람이 읽을 조건: `button "OK"`, `text "42"`
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(role) = &self.role {
            parts.push(role.clone());
        }
        if let Some(name) = &self.name {
            parts.push(format!("{name:?}"));
        }
        if let Some(text) = &self.text {
            parts.push(format!("text {text:?}"));
        }
        if parts.is_empty() {
            return "an element".into();
        }
        parts.join(" ")
    }
}

/// 찾는 글이 이 글에 맞는가. `exact`면 통째로(대소문자도) 같아야 하고, 아니면 대소문자를 가리지 않고 일부만
/// 맞아도 된다. 잘린 글(`cut`)은 보이는 앞부분까지만 견준다. 트리는 글의 공백을 하나로 줄이므로 찾는 글도 줄인다
pub fn fits(wanted: &str, text: &str, cut: bool, exact: bool) -> bool {
    let wanted = collapse(wanted);
    let text = collapse(text);
    if wanted.is_empty() {
        return text.is_empty();
    }
    if exact {
        return text == wanted || (cut && wanted.starts_with(&text));
    }
    text.to_lowercase().contains(&wanted.to_lowercase())
}

/// 공백을 하나로 줄이고 앞뒤 공백을 뗀다 (트리의 이름도 이렇게 줄였다)
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// ref를 범위로 쓰는 그릇의 역할 (tree.rs의 scope와 같다). 안의 글을 누르려고 그릇을 누르면 엉뚱한 곳(창의
/// 가운데)이 눌리므로 대신 누를 요소로 고르지 않는다
const CONTAINERS: [&str; 16] = [
    "window",
    "dialog",
    "alertdialog",
    "menu",
    "menubar",
    "tablist",
    "list",
    "listbox",
    "tree",
    "treegrid",
    "table",
    "toolbar",
    "group",
    "document",
    "form",
    "region",
];

/// ref가 없는 줄(글 줄)을 대신 누를 요소: 그 줄, 아니면 그것을 담은 가장 가까운 요소 가운데 ref가 있는 것(목록
/// 항목, 단추). 가장 가까운 ref가 그릇(창, 목록 등)이면 없다
pub fn owner(lines: &[Line], at: usize) -> Option<usize> {
    let mut depth = lines.get(at)?.depth;
    if lines[at].reference.is_some() {
        return Some(at);
    }
    for index in (0..at).rev() {
        if lines[index].depth < depth {
            if lines[index].reference.is_some() {
                return (!CONTAINERS.contains(&lines[index].role.as_str())).then_some(index);
            }
            depth = lines[index].depth;
        }
    }
    None
}

/// 줄과 그 아래 줄의 이름·글을 차례로 이은 글 (Playwright의 textContent)
pub fn text_content(lines: &[Line], at: usize) -> String {
    let Some(top) = lines.get(at) else {
        return String::new();
    };
    let mut parts: Vec<&str> = Vec::new();
    for line in std::iter::once(top).chain(
        lines[at + 1..]
            .iter()
            .take_while(|line| line.depth > top.depth),
    ) {
        if !line.name.is_empty() {
            parts.push(&line.name);
        }
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CALCULATOR: &str = "\
- app: \"gnome-calculator\" [pid=812]
- window \"계산기\" [ref=e1] [active]:
  - button \"7\" [ref=e183]
  - button \"=\" [ref=e185] [description=\"결과 계산\"]
  - list [ref=e158]:
    - listitem \"6×7\" [ref=e192]:
      - text: \"=\"
      - text: \"42\"
  - textbox [ref=e159] [focused] [value=\"42\"]
  - slider \"키 반복 속도\" [ref=e26] [value=6] [range=0..8]
# note: cut at 2000 lines; pass a ref to read one part
";

    #[test]
    fn lines_split_into_role_name_ref_states_and_values() {
        let lines = parse(CALCULATOR);
        assert_eq!(lines.len(), 9, "the app header and notes are not elements");
        let window = &lines[0];
        assert_eq!(
            (window.depth, window.role.as_str(), window.name.as_str()),
            (0, "window", "계산기")
        );
        assert_eq!(window.reference.as_deref(), Some("e1"));
        assert!(window.has("active"));
        let equals = &lines[2];
        assert_eq!(equals.prop("description"), Some("결과 계산"));
        let text = &lines[6];
        assert_eq!(
            (text.depth, text.role.as_str(), text.name.as_str()),
            (3, "text", "42")
        );
        assert_eq!(text.reference, None);
        let field = &lines[7];
        assert_eq!(field.value(), "42");
        assert!(field.has("focused") && field.name.is_empty());
        let slider = &lines[8];
        assert_eq!(slider.prop("value"), Some("6"));
        assert_eq!(slider.prop("range"), Some("0..8"));
        assert_eq!(slider.value(), "6");
        assert_eq!(lines[1].value(), "7", "without a value the name stands in");
    }

    #[test]
    fn quotes_and_brackets_inside_names_and_values_are_kept() {
        let lines = parse(
            "- button \"say \\\"hi\\\" [now]\" [ref=e2]\n- textbox [ref=e3] [value=\"a]b\\\\c\"]\n",
        );
        assert_eq!(lines[0].name, "say \"hi\" [now]");
        assert_eq!(lines[0].reference.as_deref(), Some("e2"));
        assert_eq!(lines[1].value(), "a]b\\c");
    }

    #[test]
    fn cut_text_keeps_its_visible_part() {
        let lines = parse(
            "- text: \"가나다…[+120 chars]\"\n- link \"Read more…\" [ref=e4]\n- textbox [ref=e5] [value=\"abc…[+9 chars]\"]\n",
        );
        assert_eq!(
            (lines[0].name.as_str(), lines[0].is_cut("name")),
            ("가나다", true)
        );
        // 말줄임표로 끝나는 진짜 이름은 자르지 않는다
        assert_eq!(
            (lines[1].name.as_str(), lines[1].is_cut("name")),
            ("Read more…", false)
        );
        assert_eq!((lines[2].value(), lines[2].value_cut()), ("abc", true));
        assert!(!lines[1].value_cut());
        let exact = Matcher {
            text: Some("가나다라마".into()),
            exact: true,
            ..Matcher::default()
        };
        assert!(exact.matches(&lines[0]));
    }

    #[test]
    fn malformed_lines_are_skipped() {
        assert!(parse("button \"x\"\n- text: no quotes\n- button \"open\n").is_empty());
        assert!(parse("- button [ref=e1").is_empty());
        assert!(parse("").is_empty());
        assert_eq!(parse("- button\n")[0].role, "button");
    }

    #[test]
    fn matchers_follow_playwright() {
        let lines = parse(CALCULATOR);
        let find = |matcher: Matcher| -> Vec<String> {
            lines
                .iter()
                .filter(|line| matcher.matches(line))
                .map(|line| line.text.clone())
                .collect()
        };
        let by_role = |role: &str, name: Option<&str>, exact: bool| Matcher {
            role: Some(role.into()),
            name: name.map(str::to_string),
            exact,
            ..Matcher::default()
        };
        assert_eq!(find(by_role("BUTTON", Some("="), true)).len(), 1);
        assert_eq!(find(by_role("button", None, false)).len(), 2);
        // 이름은 대소문자를 가리지 않고 일부만 맞아도 된다. exact는 통째로
        assert_eq!(find(by_role("slider", Some("반복"), false)).len(), 1);
        assert!(find(by_role("slider", Some("반복"), true)).is_empty());
        // 글은 이름, 글 줄, 값 어디에 있어도 된다
        let text = |text: &str, exact: bool| Matcher {
            text: Some(text.into()),
            exact,
            ..Matcher::default()
        };
        assert_eq!(
            find(text("42", true)),
            [
                "- text: \"42\"",
                "- textbox [ref=e159] [focused] [value=\"42\"]"
            ]
        );
        assert_eq!(find(text("6×7", false)).len(), 1);
        // 글 줄은 이름이 없는 요소다: name 조건에 맞지 않는다
        assert!(find(by_role("text", Some("42"), false)).is_empty());
        assert_eq!(find(Matcher::default()).len(), lines.len());
        assert_eq!(
            by_role("button", Some("OK"), false).describe(),
            "button \"OK\""
        );
    }

    #[test]
    fn lines_without_a_ref_act_through_their_nearest_owner() {
        let lines = parse(CALCULATOR);
        // "42" 글 줄 → 그 위의 목록 항목
        assert_eq!(owner(&lines, 6), Some(4));
        assert_eq!(owner(&lines, 2), Some(2));
        assert_eq!(owner(&[], 0), None);
        let orphan = parse("- text: \"alone\"\n");
        assert_eq!(owner(&orphan, 0), None);
        // 창이나 목록에 바로 든 글은 그릇을 대신 누르지 않는다 (창의 가운데가 눌린다)
        let loose = parse(
            "- window \"w\" [ref=e1]:\n  - text: \"label\"\n  - list [ref=e2]:\n    - text: \"row\"\n",
        );
        assert_eq!((owner(&loose, 1), owner(&loose, 3)), (None, None));
        // 그릇을 ref로 바로 가리키면 그 그릇이다
        assert_eq!(owner(&loose, 2), Some(2));
    }

    #[test]
    fn text_content_joins_the_subtree() {
        let lines = parse(CALCULATOR);
        assert_eq!(text_content(&lines, 4), "6×7 = 42");
        assert_eq!(text_content(&lines, 7), "", "an unnamed field has no text");
        assert_eq!(text_content(&lines, 99), "");
    }
}
