//! E4 (2026-11-19): review rules by file type. A rule names the files it
//! applies to by glob and the level a breach is reported at; the reviewer's
//! prompt carries only the rules that match a file in scope, each with the
//! files it applies to. Aeroric ships a few; a project adds its own, turns
//! built-ins off or replaces them in `.aeroric/review-rules.json`.
//!
//! The idea is OCR's (`system_rules.json`: rules matched by path are steadier
//! than guidance in prose), with every matching rule applying instead of the
//! first (OCR #1177) and a level on each rule (OCR #96).
//!
//! The project file is read from the round's snapshot before the turn when
//! it is there, so a turn cannot loosen the rules of its own review.

use std::path::Path;

use aeroric_protocol::review::{ReviewRule, ReviewRules, RuleSource, RulesFileState, Severity};
use serde::Deserialize;

/// The project rules file, from the repository's top.
pub const FILE: &str = ".aeroric/review-rules.json";
/// The rules' text in one prompt at most; the rest are named only.
const MAX_PROMPT_CHARS: usize = 12_000;
/// Files named under one rule in the prompt.
const MAX_FILES_SHOWN: usize = 8;
/// Variants one pattern's braces may expand to.
const MAX_VARIANTS: usize = 64;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectFile {
    #[serde(default)]
    rules: Vec<ProjectRule>,
    /// Built-in rule ids to turn off.
    #[serde(default)]
    disable: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectRule {
    id: String,
    paths: Vec<String>,
    #[serde(default)]
    severity: Option<Severity>,
    text: String,
}

/// Aeroric's own rules.
pub fn builtin() -> Vec<ReviewRule> {
    BUILTIN
        .iter()
        .map(|(id, paths, severity, text)| ReviewRule {
            id: (*id).into(),
            source: RuleSource::Builtin,
            paths: paths.iter().map(|p| (*p).into()).collect(),
            severity: *severity,
            text: (*text).into(),
            disabled: false,
        })
        .collect()
}

const WORKFLOWS: &[&str] = &[
    ".github/workflows/**/*.{yml,yaml}",
    ".github/actions/**/*.{yml,yaml}",
];

const BUILTIN: &[(&str, &[&str], Severity, &str)] = &[
    (
        "gha-untrusted-code",
        WORKFLOWS,
        Severity::Blocker,
        "- `pull_request_target` 或 `workflow_run` 里检出并运行 PR 的代码（`ref` 指向 PR head），等于用有写权限的令牌跑不可信代码；\n- `${{ github.event.* }}`（标题、分支名、评论）直接写进 `run:`，可以被注入命令，要先放进 `env:` 再引用；\n- 机密被打印到日志，或写进了文件、产物。",
    ),
    (
        "gha-hygiene",
        WORKFLOWS,
        Severity::Major,
        "- `permissions` 没写或是 `write-all`，应只给需要的权限；\n- 第三方 action 只用标签引用，应钉到完整的提交 SHA；\n- 任务没有 `timeout-minutes`；\n- 用 `continue-on-error`、`|| true` 吞掉本该失败的步骤。",
    ),
    (
        "sql-migration",
        &[
            "**/migrations/**/*.sql",
            "**/migrate/**/*.sql",
            "*migration*.sql",
        ],
        Severity::Blocker,
        "- 删除表、列或数据的迁移，没有回滚办法或备份；\n- 给已有数据的表加 `NOT NULL` 列却没有默认值，或改列类型会截断数据；\n- 在大表上加锁的操作（不加 `CONCURRENTLY` 建索引、整表重写）没有说明影响；\n- 已经发布的迁移文件被改写，而不是新加一个迁移。",
    ),
    (
        "rust",
        &["*.rs"],
        Severity::Major,
        "- 处理外部输入、文件、网络的路径上用 `unwrap`、`expect`、下标越界会 panic；\n- 在 async 函数里做阻塞调用（同步 IO、`std::thread::sleep`、持有同步锁跨 `.await`）；\n- `unsafe` 块没有写明为什么安全；\n- 错误被 `let _ =`、`.ok()` 悄悄丢掉，调用方以为成功了。",
    ),
    (
        "swift",
        &["*.swift"],
        Severity::Major,
        "- 对外部数据强制解包（`!`、`try!`、`as!`）；\n- 在后台线程更新界面状态，或在主线程做耗时 IO；\n- 闭包强引用 `self` 造成循环引用；\n- `Task` 被取消后仍继续写状态。",
    ),
    (
        "ts-js",
        &["*.{ts,tsx,js,jsx,mjs,cjs}"],
        Severity::Major,
        "- 漏了 `await`，或 Promise 的失败没人处理；\n- 用户能控制的内容进了 `innerHTML`、`dangerouslySetInnerHTML`、`eval`；\n- `any`、类型断言掩盖了真实的类型错误；\n- 副作用（订阅、定时器）没有清理。",
    ),
    (
        "python",
        &["*.{py,pyi}"],
        Severity::Major,
        "- 可变默认参数；\n- 裸 `except:` 或 `except Exception: pass` 吞掉错误；\n- `subprocess` 用 `shell=True` 拼接外部输入；\n- 网络请求没有超时。",
    ),
    (
        "shell",
        &["*.{sh,bash,zsh}"],
        Severity::Major,
        "- 变量没加引号，含空格的路径会被拆开；\n- 没有 `set -euo pipefail`，前一步失败后继续执行；\n- `rm -rf \"$X/\"` 这类命令在变量为空时会删错地方。",
    ),
    (
        "dependencies",
        &[
            "{Cargo.toml,package.json,Package.swift,go.mod,pyproject.toml}",
            "requirements*.txt",
        ],
        Severity::Major,
        "- 新加的依赖是否必要，是否有维护、许可证是否可用；\n- 版本没有约束，或从 git 引用却没钉提交；\n- 改了清单却没有同步更新锁文件。",
    ),
    (
        "dockerfile",
        &["Dockerfile", "Dockerfile.*", "*.Dockerfile"],
        Severity::Major,
        "- 基础镜像用 `latest` 或没有版本；\n- 以 root 运行；\n- 机密通过 `ARG`、`ENV`、`COPY` 留在镜像层里。",
    ),
];

/// The rules in force: the built-ins, then the project's (a project rule with
/// a built-in's id replaces it; `disable` turns built-ins off).
pub fn load(top: &Path, before: &str) -> ReviewRules {
    let mut out = ReviewRules {
        file: FILE.into(),
        rules: builtin(),
        ..ReviewRules::default()
    };
    let Some(text) = read_file(top, before) else {
        return out;
    };
    let file: ProjectFile = match serde_json::from_str(&text) {
        Ok(file) => file,
        Err(error) => {
            out.state = RulesFileState::Invalid;
            out.error = Some(format!("{FILE} 读不懂：{error}"));
            return out;
        }
    };
    if let Some(bad) = file
        .rules
        .iter()
        .find(|r| r.id.trim().is_empty() || r.paths.is_empty() || r.text.trim().is_empty())
    {
        out.state = RulesFileState::Invalid;
        out.error = Some(format!("{FILE} 里的规则「{}」缺少编号、路径或内容", bad.id));
        return out;
    }
    out.state = RulesFileState::Loaded;
    for rule in &mut out.rules {
        rule.disabled = file.disable.contains(&rule.id);
    }
    for rule in file.rules {
        let rule = ReviewRule {
            id: rule.id.trim().into(),
            source: RuleSource::Project,
            paths: rule.paths,
            severity: rule.severity.unwrap_or(Severity::Major),
            text: rule.text.trim().into(),
            disabled: false,
        };
        match out.rules.iter_mut().find(|r| r.id == rule.id) {
            Some(same) => *same = rule,
            None => out.rules.push(rule),
        }
    }
    out
}

/// The file as the round's snapshot before the turn has it, or else (it is
/// ignored or untracked) as it is on disk now.
fn read_file(top: &Path, before: &str) -> Option<String> {
    if !before.is_empty() {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(top)
            .args(["cat-file", "-p", &format!("{before}:{FILE}")])
            .output()
            .ok()?;
        if output.status.success() {
            return Some(String::from_utf8_lossy(&output.stdout).into_owned());
        }
    }
    std::fs::read_to_string(top.join(FILE)).ok()
}

/// Each rule in force that matches a file in `selected`, with those files.
pub fn applicable<'a>(
    rules: &'a ReviewRules,
    selected: &[String],
) -> Vec<(&'a ReviewRule, Vec<String>)> {
    rules
        .rules
        .iter()
        .filter(|r| !r.disabled)
        .filter_map(|r| {
            let files: Vec<String> = selected
                .iter()
                .filter(|path| r.paths.iter().any(|p| matches(p, path)))
                .cloned()
                .collect();
            (!files.is_empty()).then_some((r, files))
        })
        .collect()
}

/// The prompt's rules section, and the ids it carries. Rules past the budget
/// are named only, so the reviewer knows they exist.
pub fn section(
    applied: &[(&ReviewRule, Vec<String>)],
    changed_file: bool,
) -> (String, Vec<String>) {
    if applied.is_empty() && !changed_file {
        return (String::new(), Vec::new());
    }
    let mut out = String::from("\n## 审查规则（按文件匹配）\n");
    if changed_file {
        out.push_str(&format!(
            "这一轮改了规则文件 `{FILE}`；本轮按改动前的规则审查，规则文件本身的改动请照常审查。\n"
        ));
    }
    if applied.is_empty() {
        return (out, Vec::new());
    }
    out.push_str("下面每条规则只适用于它列出的文件。违反时按规则写的级别报，并在 finding 里写 \"rule\": \"规则编号\"。\n");
    let mut ids = Vec::new();
    let mut left = Vec::new();
    for (rule, files) in applied {
        let shown: Vec<&str> = files
            .iter()
            .take(MAX_FILES_SHOWN)
            .map(String::as_str)
            .collect();
        let more = if files.len() > MAX_FILES_SHOWN {
            format!(" 等 {} 个", files.len())
        } else {
            String::new()
        };
        let block = format!(
            "\n### {}（违反时按「{}」报）\n适用于：{}{more}\n{}\n",
            rule.id,
            super::prompt::severity(rule.severity),
            shown.join("、"),
            rule.text
        );
        if out.chars().count() + block.chars().count() > MAX_PROMPT_CHARS {
            left.push(rule.id.as_str());
            continue;
        }
        out.push_str(&block);
        ids.push(rule.id.clone());
    }
    if !left.is_empty() {
        out.push_str(&format!(
            "\n（另有 {} 条规则因篇幅没有列出：{}）\n",
            left.len(),
            left.join("、")
        ));
    }
    (out, ids)
}

/// Whether `path` (from the repository's top) matches `pattern`.
pub fn matches(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim().trim_start_matches("./");
    if pattern.is_empty() {
        return false;
    }
    let target = if pattern.contains('/') {
        path
    } else {
        path.rsplit('/').next().unwrap_or(path)
    };
    let mut variants = Vec::new();
    expand(pattern, &mut variants);
    variants
        .iter()
        .any(|p| regex::Regex::new(&to_regex(p)).is_ok_and(|r| r.is_match(target)))
}

/// `{a,b}` braces into each variant, nested ones too.
fn expand(pattern: &str, out: &mut Vec<String>) {
    if out.len() >= MAX_VARIANTS {
        return;
    }
    let Some(open) = pattern.find('{') else {
        out.push(pattern.into());
        return;
    };
    let mut depth = 0;
    let mut close = None;
    for (i, c) in pattern[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else {
        out.push(pattern.into());
        return;
    };
    let inner = &pattern[open + 1..close];
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0, 0);
    for (i, c) in inner.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&inner[start..]);
    for part in parts {
        expand(
            &format!("{}{part}{}", &pattern[..open], &pattern[close + 1..]),
            out,
        );
    }
}

fn to_regex(pattern: &str) -> String {
    let mut out = String::from("^");
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                if chars.get(i) == Some(&'/') {
                    i += 1;
                    out.push_str("(?:.*/)?");
                } else {
                    out.push_str(".*");
                }
                continue;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            c => out.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    out.push('$');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_by_path_or_by_name() {
        assert!(matches("*.rs", "core/src/a.rs"));
        assert!(matches("*.{ts,tsx}", "web/app.tsx"));
        assert!(!matches("*.{ts,tsx}", "web/app.js"));
        assert!(matches(
            ".github/workflows/**/*.{yml,yaml}",
            ".github/workflows/ci.yml"
        ));
        assert!(matches(
            ".github/workflows/**/*.{yml,yaml}",
            ".github/workflows/sub/x.yaml"
        ));
        assert!(!matches(
            ".github/workflows/**/*.yml",
            "docs/.github/workflows/ci.yml"
        ));
        assert!(matches(
            "**/migrations/**/*.sql",
            "db/migrations/2026/001.sql"
        ));
        assert!(matches("**/migrations/**/*.sql", "migrations/001.sql"));
        assert!(matches("{Cargo.toml,package.json}", "crates/a/Cargo.toml"));
        assert!(matches("./src/*.rs", "src/a.rs"));
        assert!(
            !matches("src/*.rs", "src/deep/a.rs"),
            "* stays within a name"
        );
        assert!(matches("Dockerfile.*", "deploy/Dockerfile.prod"));
        assert!(!matches("", "a.rs"));
    }

    #[test]
    fn every_matching_rule_applies_and_the_project_file_adds_replaces_and_disables() {
        let dir =
            std::env::temp_dir().join(format!("aeroric-rules-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(dir.join(".aeroric")).unwrap();

        // No file: the built-ins alone.
        let rules = load(&dir, "");
        assert_eq!(rules.state, RulesFileState::Missing);
        assert!(rules.rules.iter().all(|r| r.source == RuleSource::Builtin));

        std::fs::write(
            dir.join(FILE),
            r#"{"disable": ["shell"], "rules": [
                {"id": "rust", "paths": ["*.rs"], "severity": "blocker", "text": "不许 unwrap"},
                {"id": "money", "paths": ["src/billing/**"], "text": "金额用整数分"}
            ]}"#,
        )
        .unwrap();
        let rules = load(&dir, "");
        assert_eq!(rules.state, RulesFileState::Loaded);
        let rust = rules.rules.iter().find(|r| r.id == "rust").unwrap();
        assert_eq!(
            (rust.source, rust.severity, rust.text.as_str()),
            (RuleSource::Project, Severity::Blocker, "不许 unwrap")
        );
        let money = rules.rules.iter().find(|r| r.id == "money").unwrap();
        assert_eq!(
            money.severity,
            Severity::Major,
            "a project rule's default level"
        );
        assert!(
            rules
                .rules
                .iter()
                .find(|r| r.id == "shell")
                .unwrap()
                .disabled
        );

        // Stacked: a billing Rust file gets both rules; the shell script none.
        let selected = vec![
            "src/billing/fee.rs".to_string(),
            "build.sh".to_string(),
            "README.md".to_string(),
        ];
        let applied = applicable(&rules, &selected);
        let names: Vec<_> = applied
            .iter()
            .map(|(r, files)| (r.id.as_str(), files.clone()))
            .collect();
        assert_eq!(
            names,
            [
                ("rust", vec!["src/billing/fee.rs".to_string()]),
                ("money", vec!["src/billing/fee.rs".to_string()])
            ]
        );
        let (text, ids) = section(&applied, false);
        assert_eq!(ids, ["rust", "money"]);
        assert!(text
            .contains("### rust（违反时按「阻断」报）\n适用于：src/billing/fee.rs\n不许 unwrap"));
        assert!(!text.contains("set -euo pipefail"));

        // Unreadable: the built-ins alone, and why.
        std::fs::write(dir.join(FILE), r#"{"rules": [{"id": "x"}]}"#).unwrap();
        let rules = load(&dir, "");
        assert_eq!(rules.state, RulesFileState::Invalid);
        assert!(rules.error.is_some());
        assert!(rules
            .rules
            .iter()
            .all(|r| r.source == RuleSource::Builtin && !r.disabled));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rules_past_the_budget_are_named_only() {
        let big = ReviewRule {
            id: "big".into(),
            source: RuleSource::Project,
            paths: vec!["*".into()],
            severity: Severity::Minor,
            text: "长".repeat(MAX_PROMPT_CHARS),
            disabled: false,
        };
        let small = ReviewRule {
            id: "small".into(),
            text: "短".into(),
            ..big.clone()
        };
        let files = vec!["a".to_string()];
        let (text, ids) = section(&[(&small, files.clone()), (&big, files)], false);
        assert_eq!(ids, ["small"]);
        assert!(text.contains("另有 1 条规则因篇幅没有列出：big"));
        assert_eq!(section(&[], false), (String::new(), Vec::new()));
        assert!(section(&[], true).0.contains("这一轮改了规则文件"));
    }
}
