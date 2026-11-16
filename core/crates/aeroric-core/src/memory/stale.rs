//! B6 second batch (2026-11-14): 过时检测. A project fact or experience
//! goes stale when every file and code symbol it names is gone from the
//! project. No model: paths are looked up in `git ls-files`, symbols with
//! one `git grep -w`. A stale item only becomes a proposal — a name can be
//! gone because it was renamed, and the item may still hold.

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use aeroric_protocol::memory::MemoryItem;

/// Names looked for per item, and per `git grep` call.
const PER_ITEM: usize = 20;
const PER_GREP: usize = 200;

/// File extensions a bare word must end in to count as a path.
const EXTENSIONS: &[&str] = &[
    "rs", "py", "swift", "ts", "tsx", "js", "jsx", "mjs", "go", "java", "kt", "c", "h", "cc",
    "cpp", "hpp", "m", "mm", "rb", "php", "cs", "toml", "json", "yaml", "yml", "md", "sh", "sql",
    "proto", "html", "css", "vue", "lock", "txt", "cfg", "ini", "xml", "gradle",
];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ref {
    /// A path relative to the project, or the tail of one.
    Path(String),
    /// A code name as written (`Foo::bar`); `name` is the part grepped.
    Symbol { written: String, name: String },
}

impl Ref {
    pub fn written(&self) -> &str {
        match self {
            Ref::Path(p) => p,
            Ref::Symbol { written, .. } => written,
        }
    }
}

fn has_extension(word: &str) -> bool {
    let last = word.rsplit('/').next().unwrap_or(word);
    last.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty() && EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
    })
}

fn is_ident(part: &str) -> bool {
    let mut chars = part.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A path the project could hold: relative, with a known extension, not a
/// URL, an absolute path or a path out of the project.
fn as_path(word: &str) -> Option<String> {
    let word = word.trim_start_matches("./");
    let ok = !word.is_empty()
        && !word.contains("://")
        && !word.starts_with(['/', '~', '.', '$'])
        && !word.contains("..")
        && !word.contains(char::is_whitespace)
        && has_extension(word);
    ok.then(|| word.to_string())
}

/// A code name: one identifier or several joined by `::` or `.`, that looks
/// like code — an underscore, a `::`, or a capital after the first letter.
/// Plain words like `cargo` are left out: they name tools, not code.
fn as_symbol(word: &str) -> Option<Ref> {
    let word = word.trim_end_matches("()");
    let parts: Vec<&str> = word.split("::").flat_map(|p| p.split('.')).collect();
    if parts.is_empty() || !parts.iter().all(|p| is_ident(p)) {
        return None;
    }
    let name = parts.last()?.to_string();
    let codeish = word.contains("::")
        || word.contains('_')
        || word.chars().skip(1).any(|c| c.is_ascii_uppercase());
    (codeish && name.chars().count() >= 3).then(|| Ref::Symbol {
        written: word.to_string(),
        name,
    })
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '，' | '。'
                    | '；'
                    | '：'
                    | '、'
                    | '（'
                    | '）'
                    | '“'
                    | '”'
                    | '「'
                    | '」'
                    | '《'
                    | '》'
            )
    })
    .map(|w| {
        w.trim_matches(|c: char| matches!(c, ',' | ';' | ':' | '(' | ')' | '"' | '\'' | '[' | ']'))
    })
    .map(|w| w.trim_end_matches('.'))
    .filter(|w| !w.is_empty())
}

/// What one text names: paths anywhere, code names only in backticks.
pub fn refs_in(text: &str) -> Vec<Ref> {
    let mut out: Vec<Ref> = Vec::new();
    let mut push = |r: Ref| {
        if !out.contains(&r) {
            out.push(r);
        }
    };
    for (i, span) in text.split('`').enumerate() {
        if i % 2 == 1 {
            let span = span.trim();
            if let Some(path) = as_path(span) {
                push(Ref::Path(path));
            } else if let Some(symbol) = as_symbol(span) {
                push(symbol);
            }
            continue;
        }
        for word in words(span) {
            if word.contains('/') {
                if let Some(path) = as_path(word) {
                    push(Ref::Path(path));
                }
            }
        }
    }
    out
}

/// What an item names, in its text and an experience's problem and fix.
pub fn refs(item: &MemoryItem) -> Vec<Ref> {
    let mut all = refs_in(&item.text);
    if let Some(detail) = &item.experience {
        for r in refs_in(&detail.problem)
            .into_iter()
            .chain(refs_in(&detail.solution))
        {
            if !all.contains(&r) {
                all.push(r);
            }
        }
    }
    all.truncate(PER_ITEM);
    all
}

fn git(root: &Path, args: &[&str]) -> Option<(i32, String)> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    Some((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}

/// The names of `names` that appear as whole words in the project's text
/// files, tracked or not (ignored ones left out). `None`: git failed.
fn found_symbols(root: &Path, names: &[String]) -> Option<HashSet<String>> {
    let mut found = HashSet::new();
    for chunk in names.chunks(PER_GREP) {
        let mut args = vec!["grep", "--untracked", "-I", "-w", "-o", "-h", "-F"];
        for name in chunk {
            args.push("-e");
            args.push(name);
        }
        let (code, out) = git(root, &args)?;
        // 1 is "nothing matched".
        if code != 0 && code != 1 {
            return None;
        }
        found.extend(out.lines().map(str::to_string));
    }
    Some(found)
}

/// The stale ones of `items` (ids, with the names no longer found). `None`
/// when the folder is not a git repository or git failed: nothing is
/// judged then.
pub fn check(root: &Path, items: &[MemoryItem]) -> Option<Vec<(String, Vec<String>)>> {
    let named: Vec<(&MemoryItem, Vec<Ref>)> = items
        .iter()
        .map(|item| (item, refs(item)))
        .filter(|(_, refs)| !refs.is_empty())
        .collect();
    if named.is_empty() {
        return Some(Vec::new());
    }
    let (code, listing) = git(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    if code != 0 {
        return None;
    }
    let files: Vec<&str> = listing.split('\0').filter(|f| !f.is_empty()).collect();
    let mut names: Vec<String> = named
        .iter()
        .flat_map(|(_, refs)| refs)
        .filter_map(|r| match r {
            Ref::Symbol { name, .. } => Some(name.clone()),
            Ref::Path(_) => None,
        })
        .collect();
    names.sort();
    names.dedup();
    let symbols = if names.is_empty() {
        HashSet::new()
    } else {
        found_symbols(root, &names)?
    };
    let path_found = |path: &str| {
        let tail = format!("/{path}");
        files.iter().any(|f| *f == path || f.ends_with(&tail))
    };
    Some(
        named
            .into_iter()
            .filter(|(_, refs)| {
                refs.iter().all(|r| match r {
                    Ref::Path(p) => !path_found(p),
                    Ref::Symbol { name, .. } => !symbols.contains(name),
                })
            })
            .map(|(item, refs)| {
                (
                    item.id.clone(),
                    refs.iter().map(|r| r.written().to_string()).collect(),
                )
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(text: &str) -> Vec<String> {
        refs_in(text)
            .iter()
            .map(|r| r.written().to_string())
            .collect()
    }

    #[test]
    fn paths_anywhere_and_code_names_only_in_backticks() {
        assert_eq!(
            written("改 src/ssh.rs 里的 `connect_timeout`，见 `TurnLedger::planned`。"),
            vec!["src/ssh.rs", "connect_timeout", "TurnLedger::planned"]
        );
        // Plain words, tools, URLs, absolute and outside paths are not names.
        assert!(written("用 `cargo` 跑 `ls`，见 https://x.io/a/b.rs 和 /usr/lib/x.so").is_empty());
        assert!(written("`../other/a.rs` 和 `~/x.py` 和 docs/ 目录").is_empty());
        // A bare file name counts only in backticks; a method call keeps
        // its last part for the grep.
        assert_eq!(written("dream.rs 与 `dream.rs`"), vec!["dream.rs"]);
        let Ref::Symbol { name, .. } = &refs_in("`store.putChanges()`")[0] else {
            panic!("a symbol")
        };
        assert_eq!(name, "putChanges");
    }

    fn git_repo() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aeroric-stale-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("src/net")).unwrap();
        std::fs::write(dir.join("src/net/ssh.rs"), "fn open_session() {}\n").unwrap();
        std::fs::write(dir.join(".gitignore"), "target/\n").unwrap();
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join("target/old_name.rs"), "fn gone_fn() {}\n").unwrap();
        let ok = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success();
        assert!(ok);
        dir
    }

    fn item(id: &str, text: &str) -> MemoryItem {
        let mut item: MemoryItem = serde_json::from_value(serde_json::json!({
            "id": id, "kind": "fact", "text": text, "assertion": "userExplicit",
            "state": "active", "createdMs": 1, "updatedMs": 1
        }))
        .unwrap();
        item.text = text.into();
        item
    }

    #[test]
    fn an_item_is_stale_only_when_everything_it_names_is_gone() {
        let root = git_repo();
        let items = vec![
            item("a", "超时在 `net/ssh.rs` 的 `open_session` 里设"),
            item("b", "超时在 `src/ssh_old.rs` 的 `gone_fn` 里设"),
            item("c", "`src/ssh_old.rs` 搬走了，`open_session` 还在"),
            item("d", "回复用简体中文"),
        ];
        let stale = check(&root, &items).unwrap();
        // Untracked files count, ignored ones do not; a path's tail is
        // enough; one name still found keeps the item.
        assert_eq!(
            stale,
            vec![(
                "b".to_string(),
                vec!["src/ssh_old.rs".to_string(), "gone_fn".to_string()]
            )]
        );
        std::fs::remove_dir_all(&root).unwrap();
        let plain = std::env::temp_dir().join(format!("aeroric-stale-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(
            check(&plain, &items[3..]),
            Some(Vec::new()),
            "nothing named, no git"
        );
        assert_eq!(
            check(&plain, &items),
            None,
            "not a repository: no judgement"
        );
        std::fs::remove_dir_all(&plain).unwrap();
    }
}
