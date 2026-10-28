// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 두 스냅샷의 줄 단위 차이. Aside의 snapshot()처럼 바뀐 줄만 앞뒤 줄 없이 unified diff 꼴로
//! 돌려준다 (`@@ -3,0 +4,2 @@`, 지운 줄 `-`, 더한 줄 `+`). 앞뒤의 같은 줄을 먼저 떼고 나머지를
//! Myers 알고리즘으로 견준다. 거의 다 바뀌었으면 통째로 바꾼 한 덩어리로 낸다.

/// 이보다 많이 다르면 견주기를 그만두고 통째로 바꾼 것으로 본다 (기억은 대략 이 값의 제곱)
const MAX_EDITS: usize = 1000;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Same,
    Delete,
    Insert,
}

/// `old`에서 `new`로 바뀐 줄. 같으면 빈 문자열
pub fn unified(old: &str, new: &str) -> String {
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
    hunks(a, b, &ops, prefix)
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
    fn a_whole_new_tree_is_one_hunk() {
        let old: Vec<String> = (0..1500).map(|n| format!("old {n}")).collect();
        let new: Vec<String> = (0..1500).map(|n| format!("new {n}")).collect();
        let diff = unified(&old.join("\n"), &new.join("\n"));
        assert!(diff.starts_with("@@ -1,1500 +1,1500 @@\n"));
        assert_eq!(diff.lines().count(), 3001);
    }
}
