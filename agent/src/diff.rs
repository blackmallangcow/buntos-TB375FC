// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 두 스냅샷의 줄 단위 차이. Aside의 snapshot()처럼 바뀐 줄만 앞뒤 줄 없이 unified diff 꼴로
//! 돌려준다 (`@@ -3,0 +4,2 @@`, 지운 줄 `-`, 더한 줄 `+`). 앞뒤의 같은 줄을 먼저 떼고 나머지를
//! Myers 알고리즘으로 견준다. 거의 다 바뀌었으면 통째로 바꾼 한 덩어리로 낸다.
//!
//! 바뀐 줄이 너무 많으면([`limited`]) 줄 대신 요약 한 줄을 낸다: 문서가 통째로 바뀌면 diff는 앞 문서를 모두 지운
//! 줄이 되어, 읽을 것 없이 모델의 맥락만 채운다. 새 트리가 상한 안이면 요약 뒤에 새 트리를 붙인다.

/// 이보다 많이 다르면 견주기를 그만두고 통째로 바꾼 것으로 본다 (기억은 대략 이 값의 제곱)
const MAX_EDITS: usize = 1000;
/// diff가 줄로 보이는 바뀐 줄(지운 줄과 더한 줄) 수의 기본 상한. 넘으면 요약한다 (`--max-lines`)
pub const MAX_LINES: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Same,
    Delete,
    Insert,
}

/// `old`에서 `new`로 바뀐 줄. 같으면 빈 문자열
pub fn unified(old: &str, new: &str) -> String {
    limited(old, new, usize::MAX)
}

/// [`unified`]와 같되, 바뀐 줄(지운 줄과 더한 줄)이 `max_lines`보다 많으면 줄 대신 `#`로 시작하는 요약 한 줄을
/// 낸다 (이슈 #1 P3-2: 새 탭을 열자 diff가 `@@ -2,1885 +2,51 @@`처럼 앞 문서 1885줄을 지운 줄로 냈다). 대화 상자가
/// 뜨거나 닫혔으면 그렇게, 앞 트리가 거의 다 지워졌으면 문서가 바뀌었다고 알린다. 새 트리가 `max_lines` 줄 안이면
/// 요약 뒤에 새 트리를 통째로 붙이고, 아니면 새 스냅샷을 읽으라고 한다
pub fn limited(old: &str, new: &str, max_lines: usize) -> String {
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = new.lines().collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (a, b) = (
        &old[prefix..old.len() - suffix],
        &new[prefix..new.len() - suffix],
    );
    let ops = myers(a, b).unwrap_or_else(|| {
        let mut ops = vec![Op::Delete; a.len()];
        ops.extend(std::iter::repeat_n(Op::Insert, b.len()));
        ops
    });
    let deleted = ops.iter().filter(|op| **op == Op::Delete).count();
    let inserted = ops.iter().filter(|op| **op == Op::Insert).count();
    if deleted + inserted > max_lines {
        return summary(&old, &new, a, b, &ops, max_lines);
    }
    hunks(a, b, &ops, prefix)
}

/// 줄 대신 내는 요약. `a`, `b`는 앞뒤의 같은 줄을 뗀 나머지이고 `ops`는 그 편집이다
fn summary(
    old: &[&str],
    new: &[&str],
    a: &[&str],
    b: &[&str],
    ops: &[Op],
    max_lines: usize,
) -> String {
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    let (mut i, mut j) = (0, 0);
    for op in ops {
        match op {
            Op::Same => (i, j) = (i + 1, j + 1),
            Op::Delete => {
                removed.push(a[i]);
                i += 1;
            }
            Op::Insert => {
                added.push(b[j]);
                j += 1;
            }
        }
    }
    let (deleted, inserted) = (removed.len(), added.len());
    let head = if let Some(dialog) = added.iter().find_map(|line| dialog(line)) {
        // 창 안의 모달(Firefox의 종료 확인)은 뒤의 페이지를 접근성 트리에서 뺀다: 문서가 바뀐 것이 아니다 (이슈 #1
        // 리뷰: "the document was replaced: 2000 lines → 5 lines")
        format!(
            "# {dialog} opened; the rest of the window ({deleted} lines) left the tree while it is up"
        )
    } else if let Some(dialog) = removed.iter().find_map(|line| dialog(line)) {
        format!("# {dialog} closed and {inserted} lines came back")
    } else if deleted * 10 > old.len() * 9 {
        // 앞 트리가 거의 다(90% 넘게) 지워졌다: 머리줄(앱 이름) 말고는 다 바뀌었다
        format!("# the document was replaced: {deleted} lines → {inserted} lines")
    } else {
        format!(
            "# {} lines changed ({deleted} removed, {inserted} added), more than the {max_lines} a \
             diff shows",
            deleted + inserted
        )
    };
    // 새 트리가 diff의 상한 안이면 그대로 보인다: 스냅샷을 다시 찍어 읽을 것이 이것이다
    if new.len() <= max_lines {
        format!("{head}; it now reads:\n{}\n", new.join("\n"))
    } else {
        format!("{head}; take a snapshot to read it\n")
    }
}

/// 트리 줄이 대화 상자면 그 역할과 이름(`dialog "Firefox 종료"`). 줄의 꼴은 tree.rs가 정한다:
/// `  - dialog "이름" [ref=e5] [modal]:`
fn dialog(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("- ")?;
    let role = rest.split([' ', ':']).next()?;
    if role != "dialog" && role != "alertdialog" {
        return None;
    }
    Some(
        rest.split(" [")
            .next()
            .unwrap_or(rest)
            .trim_end_matches(':')
            .trim_end(),
    )
}

/// 앞에서부터의 편집 순서 (같음, 지움, 더함). 편집이 MAX_EDITS를 넘으면 None
fn myers(a: &[&str], b: &[&str]) -> Option<Vec<Op>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m) as usize;
    if max == 0 {
        return Some(Vec::new());
    }
    let offset = max as isize;
    let mut v = vec![0isize; 2 * max + 2];
    // d마다 k = -d..=d의 v를 남긴다 (되짚기에 쓴다)
    let mut trace: Vec<Vec<isize>> = Vec::new();
    for d in 0..=max.min(MAX_EDITS) as isize {
        for k in (-d..=d).step_by(2) {
            let at = |k: isize| (k + offset) as usize;
            let mut x = if k == -d || (k != d && v[at(k - 1)] < v[at(k + 1)]) {
                v[at(k + 1)]
            } else {
                v[at(k - 1)] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[at(k)] = x;
            if x >= n && y >= m {
                trace.push(v[at(-d)..=at(d)].to_vec());
                return Some(backtrack(&trace, n, m));
            }
        }
        trace.push(v[(offset - d) as usize..=(offset + d) as usize].to_vec());
    }
    None
}

/// 끝에서부터 trace를 되짚어 편집 순서를 만든다
fn backtrack(trace: &[Vec<isize>], n: isize, m: isize) -> Vec<Op> {
    let mut ops = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (1..trace.len() as isize).rev() {
        let previous = &trace[(d - 1) as usize];
        // previous는 k = -(d-1)..=d-1을 담는다
        let get = |k: isize| previous[(k + d - 1) as usize];
        let k = x - y;
        let down = k == -d || (k != d && get(k - 1) < get(k + 1));
        let previous_k = if down { k + 1 } else { k - 1 };
        let previous_x = get(previous_k);
        let previous_y = previous_x - previous_k;
        // 편집 뒤의 대각선(같은 줄)
        let (start_x, start_y) = if down {
            (previous_x, previous_y + 1)
        } else {
            (previous_x + 1, previous_y)
        };
        while x > start_x && y > start_y {
            ops.push(Op::Same);
            x -= 1;
            y -= 1;
        }
        ops.push(if down { Op::Insert } else { Op::Delete });
        (x, y) = (previous_x, previous_y);
    }
    while x > 0 && y > 0 {
        ops.push(Op::Same);
        x -= 1;
        y -= 1;
    }
    ops.reverse();
    ops
}

/// 이어진 편집을 덩어리로 묶어 쓴다. `skipped`는 앞에서 뗀 같은 줄 수
fn hunks(a: &[&str], b: &[&str], ops: &[Op], skipped: usize) -> String {
    let mut out = String::new();
    let (mut i, mut j, mut at) = (0usize, 0usize, 0usize);
    while at < ops.len() {
        if ops[at] == Op::Same {
            i += 1;
            j += 1;
            at += 1;
            continue;
        }
        let (start_i, start_j) = (i, j);
        let end = ops[at..]
            .iter()
            .position(|op| *op == Op::Same)
            .map_or(ops.len(), |len| at + len);
        let deleted = ops[at..end].iter().filter(|op| **op == Op::Delete).count();
        let inserted = end - at - deleted;
        out.push_str(&format!(
            "@@ -{} +{} @@\n",
            range(skipped + start_i, deleted),
            range(skipped + start_j, inserted)
        ));
        for line in &a[start_i..start_i + deleted] {
            out.push('-');
            out.push_str(line);
            out.push('\n');
        }
        for line in &b[start_j..start_j + inserted] {
            out.push('+');
            out.push_str(line);
            out.push('\n');
        }
        i += deleted;
        j += inserted;
        at = end;
    }
    out
}

/// unified diff의 줄 범위. 빈 범위는 그 앞 줄 번호, 한 줄은 번호만 쓴다 (GNU diff와 같다)
fn range(start: usize, count: usize) -> String {
    match count {
        0 => format!("{start},0"),
        1 => format!("{}", start + 1),
        _ => format!("{},{count}", start + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_text_has_no_diff() {
        assert_eq!(unified("a\nb\nc", "a\nb\nc"), "");
        assert_eq!(unified("", ""), "");
    }

    #[test]
    fn inserted_lines_follow_the_line_before() {
        assert_eq!(
            unified("a\nb\nc", "a\nb\nc\nd\ne"),
            "@@ -3,0 +4,2 @@\n+d\n+e\n"
        );
        assert_eq!(unified("b", "a\nb"), "@@ -0,0 +1 @@\n+a\n");
    }

    #[test]
    fn a_changed_line_is_a_delete_and_an_insert() {
        assert_eq!(
            unified(
                "title\nslider [value=0.4]\nend",
                "title\nslider [value=0.6]\nend"
            ),
            "@@ -2 +2 @@\n-slider [value=0.4]\n+slider [value=0.6]\n"
        );
    }

    #[test]
    fn separate_changes_make_separate_hunks() {
        let old = "a\nb\nc\nd\ne\nf";
        let new = "a\nB\nc\nd\nf\ng";
        assert_eq!(
            unified(old, new),
            "@@ -2 +2 @@\n-b\n+B\n@@ -5 +4,0 @@\n-e\n@@ -6,0 +6 @@\n+g\n"
        );
    }

    #[test]
    fn diff_replays_to_the_new_text() {
        let old: Vec<String> = (0..40).map(|n| format!("line {n}")).collect();
        let mut new = old.clone();
        new.remove(3);
        new.insert(10, "added".into());
        new[20] = "changed".into();
        new.push("tail".into());
        let diff = unified(&old.join("\n"), &new.join("\n"));
        // 덩어리를 차례로 적용해 새 글이 되는지 본다
        let mut result: Vec<String> = Vec::new();
        let mut cursor = 0usize;
        let mut lines = diff.lines().peekable();
        while let Some(header) = lines.next() {
            let old_range = header.split(' ').nth(1).unwrap().trim_start_matches('-');
            let (start, count) = match old_range.split_once(',') {
                Some((s, c)) => (s.parse::<usize>().unwrap(), c.parse::<usize>().unwrap()),
                None => (old_range.parse::<usize>().unwrap(), 1),
            };
            let keep_until = if count == 0 { start } else { start - 1 };
            result.extend(old[cursor..keep_until].iter().cloned());
            cursor = keep_until + count;
            while let Some(line) = lines.next_if(|line| !line.starts_with("@@")) {
                if let Some(added) = line.strip_prefix('+') {
                    result.push(added.to_string());
                }
            }
        }
        result.extend(old[cursor..].iter().cloned());
        assert_eq!(result, new);
    }

    #[test]
    fn a_replaced_document_is_summed_up() {
        // 이슈 #1 P3-2: 위키백과 문서(1885줄)에서 새 탭(51줄)으로
        let tree = |lines: Vec<String>| {
            let mut tree = vec!["- app: \"Mozilla Firefox\" [pid=7]".to_string()];
            tree.extend(lines);
            tree.join("\n")
        };
        let article = tree(
            (0..1885)
                .map(|n| format!("  - text: \"문단 {n}\""))
                .collect(),
        );
        let new_tab = tree(
            (0..51)
                .map(|n| format!("  - link \"바로 가기 {n}\""))
                .collect(),
        );
        // 새 탭(52줄)은 상한 안이므로 요약 뒤에 통째로 붙는다
        assert_eq!(
            limited(&article, &new_tab, MAX_LINES),
            format!(
                "# the document was replaced: 1885 lines → 51 lines; it now reads:\n{new_tab}\n"
            )
        );
        // 새 트리도 상한을 넘으면 스냅샷을 읽으라고만 한다
        assert_eq!(
            limited(&article, &new_tab, 50),
            "# the document was replaced: 1885 lines → 51 lines; take a snapshot to read it\n"
        );
        // 상한을 올리면 줄로 낸다
        let full = limited(&article, &new_tab, 2000);
        assert!(full.starts_with("@@ -2,1885 +2,51 @@\n"), "{}", &full[..40]);
        assert_eq!(full, unified(&article, &new_tab));
    }

    #[test]
    fn a_big_change_is_summed_up_and_a_small_one_is_not() {
        let old: Vec<String> = (0..1000).map(|n| format!("line {n}")).collect();
        let mut new = old.clone();
        for line in new.iter_mut().skip(100).step_by(3).take(300) {
            line.push_str(" changed");
        }
        let (old, new) = (old.join("\n"), new.join("\n"));
        assert_eq!(
            limited(&old, &new, MAX_LINES),
            "# 600 lines changed (300 removed, 300 added), more than the 400 a diff shows; take a \
             snapshot to read it\n"
        );
        assert_eq!(limited(&old, &new, 600), unified(&old, &new));
        // 작은 diff는 그대로다
        assert_eq!(
            limited("a\nb\nc", "a\nB\nc", MAX_LINES),
            "@@ -2 +2 @@\n-b\n+B\n"
        );
        assert_eq!(limited("a", "a", 0), "");
    }

    #[test]
    fn a_modal_dialog_is_not_a_replaced_document() {
        // 이슈 #1 리뷰: Firefox에서 Ctrl+Q를 누르자 종료 확인 창이 창 안에 뜨고 페이지는 트리에서 빠졌다
        let page: Vec<String> = std::iter::once("- app: \"Firefox\" [pid=7]".to_string())
            .chain(std::iter::once(
                "  - window \"위키백과 — Mozilla Firefox\" [ref=e1] [active]:".to_string(),
            ))
            .chain((0..2000).map(|n| format!("    - text: \"문단 {n}\"")))
            .collect();
        let modal = [
            "- app: \"Firefox\" [pid=7]",
            "  - window \"위키백과 — Mozilla Firefox\" [ref=e1] [active]:",
            "    - dialog \"Firefox 종료\" [ref=e2001] [modal]:",
            "      - text: \"Firefox를 종료하시겠습니까?\"",
            "      - button \"Firefox 종료\" [ref=e2002]",
            "      - button \"취소\" [ref=e2003]",
        ]
        .join("\n");
        let page = page.join("\n");
        assert_eq!(
            limited(&page, &modal, MAX_LINES),
            format!(
                "# dialog \"Firefox 종료\" opened; the rest of the window (2000 lines) left the tree \
                 while it is up; it now reads:\n{modal}\n"
            )
        );
        // 닫히면 페이지가 돌아온다 (새 트리는 상한을 넘는다)
        assert_eq!(
            limited(&modal, &page, MAX_LINES),
            "# dialog \"Firefox 종료\" closed and 2000 lines came back; take a snapshot to read it\n"
        );
    }

    #[test]
    fn dialog_lines_give_their_role_and_name() {
        assert_eq!(
            dialog("    - dialog \"Firefox 종료\" [ref=e5] [modal]:"),
            Some("dialog \"Firefox 종료\"")
        );
        assert_eq!(
            dialog("  - alertdialog \"오류\" [ref=e9]"),
            Some("alertdialog \"오류\"")
        );
        assert_eq!(dialog("  - dialog [ref=e9]:"), Some("dialog"));
        assert_eq!(dialog("  - text: \"dialog\""), None);
        assert_eq!(dialog("  - button \"dialog\" [ref=e3]"), None);
    }

    #[test]
    fn a_whole_new_tree_is_one_hunk() {
        let old: Vec<String> = (0..1500).map(|n| format!("old {n}")).collect();
        let new: Vec<String> = (0..1500).map(|n| format!("new {n}")).collect();
        let diff = unified(&old.join("\n"), &new.join("\n"));
        assert!(diff.starts_with("@@ -1,1500 +1,1500 @@\n"));
        assert_eq!(diff.lines().count(), 3001);
    }
}
