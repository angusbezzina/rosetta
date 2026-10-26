//! A line-based unified diff, so a check that finds a generated file out of
//! date shows what differs. The inputs are small, so a full LCS table is fine.

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Same,
    Removed,
    Added,
}

struct Op<'a> {
    kind: Kind,
    text: &'a str,
    a_line: usize,
    b_line: usize,
}

pub fn unified_diff(expected: &str, actual: &str, from: &str, to: &str) -> String {
    const CONTEXT: usize = 3;
    let a: Vec<&str> = expected.split('\n').collect();
    let b: Vec<&str> = actual.split('\n').collect();
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut ops: Vec<Op<'_>> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            ops.push(Op {
                kind: Kind::Same,
                text: a[i],
                a_line: i,
                b_line: j,
            });
            i += 1;
            j += 1;
        } else if i < a.len() && (j == b.len() || lcs[i + 1][j] >= lcs[i][j + 1]) {
            ops.push(Op {
                kind: Kind::Removed,
                text: a[i],
                a_line: i,
                b_line: j,
            });
            i += 1;
        } else {
            ops.push(Op {
                kind: Kind::Added,
                text: b[j],
                a_line: i,
                b_line: j,
            });
            j += 1;
        }
    }

    let mut out = vec![format!("--- {from}"), format!("+++ {to}")];
    let mut index = 0;
    while index < ops.len() {
        if ops[index].kind == Kind::Same {
            index += 1;
            continue;
        }
        let start = index.saturating_sub(CONTEXT);
        let mut end = index;
        // Extend the hunk while the next change is within two contexts' reach.
        while end < ops.len() {
            if ops[end].kind != Kind::Same {
                end += 1;
                continue;
            }
            let mut run = end;
            while run < ops.len() && ops[run].kind == Kind::Same {
                run += 1;
            }
            if run == ops.len() || run - end > CONTEXT * 2 {
                end = ops.len().min(end + CONTEXT);
                break;
            }
            end = run;
        }
        let hunk = &ops[start..end];
        let a_count = hunk.iter().filter(|op| op.kind != Kind::Added).count();
        let b_count = hunk.iter().filter(|op| op.kind != Kind::Removed).count();
        out.push(format!(
            "@@ -{},{a_count} +{},{b_count} @@",
            hunk[0].a_line + 1,
            hunk[0].b_line + 1
        ));
        for op in hunk {
            let mark = match op.kind {
                Kind::Same => ' ',
                Kind::Removed => '-',
                Kind::Added => '+',
            };
            out.push(format!("{mark}{}", op.text));
        }
        index = end;
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_a_changed_line_with_context() {
        let diff = unified_diff("a\nb\nc\n", "a\nB\nc\n", "x", "y");
        assert_eq!(diff, "--- x\n+++ y\n@@ -1,4 +1,4 @@\n a\n-b\n+B\n c\n ");
    }
}
