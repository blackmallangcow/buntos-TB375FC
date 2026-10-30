// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! REPL에 넣은 JS 글을 훑는다.
//!
//! - [`replify`]: 최상위의 `let`·`const` 선언을 `var`로 바꾼다. QuickJS는 전역의 let·const를 호출 사이에 남기지만
//!   같은 이름을 다시 선언하면 "redeclaration of 'app'"으로 거절한다. AI는 호출마다 `const app = …`을 다시 쓰므로
//!   브라우저 콘솔처럼 다시 선언할 수 있게 한다. 글자 수가 같게 바꿔(`const`는 `var  `) 오류의 줄·칸 번호가 그대로다.
//! - [`complete`]: 괄호, 따옴표, 주석, 템플릿 글이 다 닫혔는지. 대화형 `bunt agent repl`이 여러 줄 입력을 모은다.
//!
//! 문법 분석기가 아니라 토큰만 훑는다. 정규식과 나누기는 앞 토큰으로 가린다. 틀리게 가리면 바꾸지 않을 뿐이다
//! (QuickJS가 다시 선언을 거절한다). 정규식은 줄을 넘지 못하므로 잘못 본 정규식도 그 줄에서 끝난다.

/// 바꿀 자리: 블록·괄호 밖에서 문장을 시작하는 `let`·`const`
#[derive(Debug, Default)]
struct Scan {
    /// (바이트 자리, 길이)
    declarations: Vec<(usize, usize)>,
    /// 끝까지 닫히지 않은 괄호·템플릿
    depth: usize,
    /// 끝나지 않은 따옴표 글·블록 주석·템플릿 글
    open: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Frame {
    Brace,
    Paren,
    Bracket,
    /// 템플릿 글의 글 부분
    Template,
    /// 템플릿 글 안의 `${ … }`
    Hole,
}

/// 앞의 뜻 있는 토큰
#[derive(Debug, Clone, Copy, PartialEq)]
enum Prev {
    Start,
    /// 식을 끝낼 수 있는 것: 이름, 수, 글, `)`, `]`, 정규식
    Operand,
    /// 뒤에 정규식이 올 수 있는 낱말 (return, typeof …)
    Keyword,
    /// `;`
    Semicolon,
    /// `}`
    CloseBrace,
    /// 그 밖의 기호
    Punct,
}

/// 뒤에 오는 `/`가 정규식인 낱말
const REGEX_KEYWORDS: [&str; 15] = [
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "case",
    "do",
    "else",
    "yield",
    "await",
    "extends",
];

fn is_identifier(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'#' | b'\\') || byte >= 0x80
}

fn scan(code: &str) -> Scan {
    let bytes = code.as_bytes();
    let mut scan = Scan::default();
    let mut stack: Vec<Frame> = Vec::new();
    let mut prev = Prev::Start;
    // 앞 토큰 뒤에 줄바꿈이 있었다 (자동 세미콜론)
    let mut newline = false;
    let mut at = 0;
    while at < bytes.len() {
        if stack.last() == Some(&Frame::Template) {
            // 템플릿 글: `${`나 닫는 ` 까지
            match bytes[at] {
                b'\\' => at += 2,
                b'`' => {
                    stack.pop();
                    prev = Prev::Operand;
                    newline = false;
                    at += 1;
                }
                b'$' if bytes.get(at + 1) == Some(&b'{') => {
                    stack.push(Frame::Hole);
                    prev = Prev::Punct;
                    at += 2;
                }
                _ => at += 1,
            }
            continue;
        }
        let byte = bytes[at];
        match byte {
            b'\n' => {
                newline = true;
                at += 1;
            }
            b' ' | b'\t' | b'\r' => at += 1,
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                at = code[at..].find('\n').map_or(bytes.len(), |end| at + end);
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => match code[at + 2..].find("*/") {
                Some(end) => {
                    newline |= code[at..at + 2 + end].contains('\n');
                    at += 2 + end + 2;
                }
                None => {
                    scan.open = true;
                    at = bytes.len();
                }
            },
            b'/' if matches!(
                prev,
                Prev::Start | Prev::Keyword | Prev::Semicolon | Prev::CloseBrace | Prev::Punct
            ) =>
            {
                at = regex_end(bytes, at + 1);
                prev = Prev::Operand;
                newline = false;
            }
            b'\'' | b'"' => {
                match string_end(bytes, at + 1, byte) {
                    Some(end) => at = end,
                    None => {
                        scan.open = true;
                        at = bytes.len();
                    }
                }
                prev = Prev::Operand;
                newline = false;
            }
            b'`' => {
                stack.push(Frame::Template);
                at += 1;
            }
            b'{' | b'(' | b'[' => {
                stack.push(match byte {
                    b'{' => Frame::Brace,
                    b'(' => Frame::Paren,
                    _ => Frame::Bracket,
                });
                prev = Prev::Punct;
                newline = false;
                at += 1;
            }
            b'}' | b')' | b']' => {
                let wanted = match byte {
                    b'}' => [Frame::Brace, Frame::Hole],
                    b')' => [Frame::Paren, Frame::Paren],
                    _ => [Frame::Bracket, Frame::Bracket],
                };
                // 짝이 틀린 닫는 기호는 무시한다 (QuickJS가 오류를 낸다)
                if stack.last().is_some_and(|top| wanted.contains(top)) {
                    stack.pop();
                }
                prev = if byte == b'}' {
                    Prev::CloseBrace
                } else {
                    Prev::Operand
                };
                newline = false;
                at += 1;
            }
            b';' => {
                prev = Prev::Semicolon;
                newline = false;
                at += 1;
            }
            byte if byte.is_ascii_digit()
                || (byte == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit)) =>
            {
                while at < bytes.len()
                    && (bytes[at].is_ascii_alphanumeric() || matches!(bytes[at], b'.' | b'_'))
                {
                    at += 1;
                }
                prev = Prev::Operand;
                newline = false;
            }
            byte if is_identifier(byte) => {
                let start = at;
                while at < bytes.len() && is_identifier(bytes[at]) {
                    // `\u0061` 같은 이스케이프는 다음 글자까지 이름이다. 여러 바이트 글자는 바이트마다 이름 글자로 본다
                    at += match bytes[at] {
                        b'\\' => 1 + code[at + 1..].chars().next().map_or(0, char::len_utf8),
                        _ => 1,
                    };
                }
                let word = &code[start..at];
                let starts_statement =
                    matches!(prev, Prev::Start | Prev::Semicolon | Prev::CloseBrace)
                        || (newline && prev == Prev::Operand);
                if stack.is_empty() && starts_statement && declares(word, &bytes[at..]) {
                    scan.declarations.push((start, word.len()));
                }
                prev = if REGEX_KEYWORDS.contains(&word) {
                    Prev::Keyword
                } else {
                    Prev::Operand
                };
                newline = false;
                continue;
            }
            _ => {
                prev = Prev::Punct;
                newline = false;
                at += 1;
            }
        }
    }
    scan.depth = stack.len();
    scan.open |= stack.contains(&Frame::Template);
    scan
}

/// `word`가 선언을 여는 let·const인가. `let`은 이름으로도 쓸 수 있어 뒤에 이름이나 구조 분해(`[`, `{`)가 와야 한다
fn declares(word: &str, after: &[u8]) -> bool {
    match word {
        "const" => true,
        "let" => {
            let next = after
                .iter()
                .find(|byte| !byte.is_ascii_whitespace())
                .copied();
            next.is_some_and(|byte| {
                (is_identifier(byte) && byte != b'\\' && byte != b'#')
                    || byte == b'['
                    || byte == b'{'
            })
        }
        _ => false,
    }
}

/// 따옴표 글의 끝 다음 자리. 줄이 끝나도 닫히지 않았으면 그 줄 끝에서 끝난 것으로 본다(QuickJS가 오류를 낸다).
/// 글이 끝나도 닫히지 않았으면 None
fn string_end(bytes: &[u8], mut at: usize, quote: u8) -> Option<usize> {
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'\n' => return Some(at),
            byte if byte == quote => return Some(at + 1),
            _ => at += 1,
        }
    }
    None
}

/// 정규식 글의 끝(플래그 뒤) 다음 자리. 정규식은 줄을 넘지 못한다
fn regex_end(bytes: &[u8], mut at: usize) -> usize {
    let mut class = false;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'\n' => return at,
            b'[' => {
                class = true;
                at += 1;
            }
            b']' => {
                class = false;
                at += 1;
            }
            b'/' if !class => {
                at += 1;
                while at < bytes.len() && bytes[at].is_ascii_alphabetic() {
                    at += 1;
                }
                return at;
            }
            _ => at += 1,
        }
    }
    bytes.len()
}

/// 최상위 `let`·`const` 선언을 같은 길이의 `var`로 바꾼 글
pub fn replify(code: &str) -> String {
    let scan = scan(code);
    let mut out = String::with_capacity(code.len());
    let mut copied = 0;
    for (start, length) in scan.declarations {
        out.push_str(&code[copied..start]);
        out.push_str("var");
        out.push_str(&" ".repeat(length - 3));
        copied = start + length;
    }
    out.push_str(&code[copied..]);
    out
}

/// 괄호, 따옴표 글, 블록 주석, 템플릿 글이 다 닫혔는가
pub fn complete(code: &str) -> bool {
    let scan = scan(code);
    !scan.open && scan.depth == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_declarations_become_var_of_the_same_length() {
        let code = "const app = await desktop.app('calc');\nlet snap = await app.snapshot()\nsnap";
        let out = replify(code);
        assert_eq!(
            out,
            "var   app = await desktop.app('calc');\nvar snap = await app.snapshot()\nsnap"
        );
        assert_eq!(out.len(), code.len());
        assert_eq!(
            replify("const {a, b} = x; let [c] = y"),
            "var   {a, b} = x; var [c] = y"
        );
        // 자동 세미콜론: 앞 줄이 식으로 끝났다
        assert_eq!(replify("x = 1\nconst y = 2"), "x = 1\nvar   y = 2");
        assert_eq!(replify("f()\nlet\nz = 3"), "f()\nvar\nz = 3");
        assert_eq!(replify("if (a) {}\nconst b = 1"), "if (a) {}\nvar   b = 1");
    }

    #[test]
    fn nested_and_disguised_declarations_stay() {
        for code in [
            "for (let i = 0; i < 3; i++) {}",
            "{ const scoped = 1 }",
            "if (x) { let y = 2 }",
            "async function f() { const r = await g(); return r }",
            "x = () => { let a = 1 }",
            "// const commented = 1\nnothing()",
            "/* let a = 1 */ b()",
            "t = `${ (() => { const inner = 1; return inner })() } const no`",
            "r = /const x/g; let_ = 1",
            "x = let\nfoo()",
            "letter = 1; constant = 2",
        ] {
            assert_eq!(replify(code), code);
        }
        // 글 안의 선언은 그대로이고 맨 앞의 선언만 바꾼다
        assert_eq!(
            replify("const s = 'let x = 1; const y = 2'"),
            "var   s = 'let x = 1; const y = 2'"
        );
        assert_eq!(
            replify("a = b / c; const d = 1"),
            "a = b / c; var   d = 1",
            "a division is not a regex"
        );
        assert_eq!(
            replify("x = typeof /a/; const d = 1"),
            "x = typeof /a/; var   d = 1"
        );
    }

    #[test]
    fn unicode_and_unfinished_code_do_not_break_the_scan() {
        assert_eq!(replify("const 계산기 = 1"), "var   계산기 = 1");
        assert_eq!(
            replify("s = '안녕'; const t = 2"),
            "s = '안녕'; var   t = 2"
        );
        // 끝나지 않은 글은 그대로 둔다 (QuickJS가 오류를 낸다)
        assert_eq!(replify("const a = 'open"), "var   a = 'open");
        assert_eq!(replify("x = `unclosed ${"), "x = `unclosed ${");
        assert_eq!(replify("a = \\"), "a = \\");
        assert_eq!(replify("a\\가 = 1; const b = 2"), "a\\가 = 1; var   b = 2");
        assert_eq!(replify(""), "");
    }

    #[test]
    fn complete_input_has_everything_closed() {
        assert!(complete("await app.click()"));
        assert!(complete("const f = () => {\n  return 1\n}"));
        assert!(complete("s = 'a{b'"));
        assert!(complete("t = `x${1}y`"));
        assert!(complete("r = /[(]/"));
        assert!(complete(""));
        assert!(!complete("const f = () => {"));
        assert!(!complete("call(1,"));
        assert!(!complete("t = `abc"));
        assert!(!complete("t = `a${b"));
        assert!(!complete("/* note"));
        assert!(!complete("s = 'open"));
        // 줄이 끝난 따옴표 글은 닫힌 것으로 본다 (다음 줄을 기다리지 않고 QuickJS가 알린다)
        assert!(complete("s = 'broken\nx"));
    }
}
