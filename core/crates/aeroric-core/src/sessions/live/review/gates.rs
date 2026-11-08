//! C 批 (2026-11-08): what in a turn's diff may weaken its own checks —
//! tests deleted or skipped, checks silenced, CI loosened — found by fixed
//! rules (gstack `review/gate-integrity.md`), so the reviewer has to judge
//! each one instead of happening to notice it. A flag is a question, not a
//! verdict: the reviewer says why it is fine or reports it.

use std::collections::HashMap;

/// At most this many flags; the rest are counted.
const MAX_FLAGS: usize = 30;

const SKIPS: &[&str] = &[
    "#[ignore",
    "XCTSkip",
    ".skip(",
    ".only(",
    "@Disabled",
    "pytest.mark.skip",
    "unittest.skip",
    "pytest.skip(",
    "t.Skip(",
];
const SKIP_STARTS: &[&str] = &["xit(", "xdescribe(", "xtest("];
const SILENCERS: &[&str] = &[
    "eslint-disable",
    "swiftlint:disable",
    "@ts-ignore",
    "@ts-nocheck",
    "# noqa",
    "#[allow(",
    "#![allow(",
    "//nolint",
    "// nolint",
];
const CI_LOOSENERS: &[&str] = &[
    "continue-on-error: true",
    "allow_failure: true",
    "|| true",
    "--no-verify",
    "--skip",
];
const CI_CHECKS: &[&str] = &[
    "cargo test",
    "clippy",
    "fmt --check",
    "swift test",
    "xcodebuild test",
    "npm test",
    "pnpm test",
    "yarn test",
    "pytest",
    "lint",
    "tsc",
    "verify",
];

#[derive(Default)]
struct File {
    path: String,
    deleted: bool,
    removed_tests: Vec<u32>,
    added_tests: usize,
    removed_checks: Vec<String>,
    added_lines: Vec<String>,
}

/// The flags for `diff` (a unified `git diff`), one line each.
pub fn flags(diff: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut files: Vec<File> = Vec::new();
    let (mut old_line, mut new_line) = (0u32, 0u32);
    // A file's header lines come before its first hunk; inside a hunk a
    // removed `-- x` is a line of code, not a header.
    let mut in_hunk = false;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = rest.rsplit_once(" b/").map_or(rest, |(_, b)| b).to_string();
            files.push(File {
                path,
                ..File::default()
            });
            in_hunk = false;
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if let Some(hunk) = line.strip_prefix("@@ ") {
            (old_line, new_line) = hunk_start(hunk);
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            file.deleted |= line.starts_with("deleted file mode");
            continue;
        }
        if line.starts_with('\\') {
            continue; // "\ No newline at end of file"
        }
        if let Some(text) = line.strip_prefix('-') {
            let code = text.trim();
            if is_test(code) {
                file.removed_tests.push(old_line);
            }
            if is_ci(&file.path) && CI_CHECKS.iter().any(|c| code.contains(c)) {
                file.removed_checks.push(code.to_string());
            }
            old_line += 1;
        } else if let Some(text) = line.strip_prefix('+') {
            let code = text.trim();
            if is_test(code) {
                file.added_tests += 1;
            }
            file.added_lines.push(code.to_string());
            let at = format!("{}:{new_line}", file.path);
            if let Some(marker) = SKIPS
                .iter()
                .find(|m| code.contains(*m))
                .or_else(|| SKIP_STARTS.iter().find(|m| code.starts_with(*m)))
            {
                out.push(format!("{at} 加了 `{marker}`，跳过了测试"));
            } else if let Some(marker) = SILENCERS.iter().find(|m| code.contains(*m)) {
                out.push(format!("{at} 加了 `{marker}`，压掉了检查"));
            } else if is_ci(&file.path) {
                if let Some(marker) = CI_LOOSENERS.iter().find(|m| code.contains(*m)) {
                    out.push(format!("{at} 加了 `{marker}`，检查失败也算过"));
                }
            }
            new_line += 1;
        } else {
            old_line += 1;
            new_line += 1;
        }
    }
    for file in &files {
        if file.deleted && is_test_path(&file.path) {
            out.push(format!("{} 整个测试文件被删了", file.path));
        } else if file.removed_tests.len() > file.added_tests {
            out.push(format!(
                "{}:{} 删掉了 {} 个测试",
                file.path,
                file.removed_tests[0],
                file.removed_tests.len() - file.added_tests
            ));
        }
        let mut readded: HashMap<&str, usize> = HashMap::new();
        for line in &file.added_lines {
            *readded.entry(line.as_str()).or_default() += 1;
        }
        for check in &file.removed_checks {
            match readded.get_mut(check.as_str()) {
                Some(n) if *n > 0 => *n -= 1,
                _ => out.push(format!("{} 删掉了检查 `{check}`", file.path)),
            }
        }
    }
    if out.len() > MAX_FLAGS {
        let more = out.len() - MAX_FLAGS;
        out.truncate(MAX_FLAGS);
        out.push(format!("……还有 {more} 处"));
    }
    out
}

/// `-a,b +c,d @@ …` → (a, c).
fn hunk_start(hunk: &str) -> (u32, u32) {
    let mut parts = hunk.split_whitespace();
    let number = |part: Option<&str>| {
        part.and_then(|p| p[1..].split(',').next()?.parse().ok())
            .unwrap_or(0)
    };
    (number(parts.next()), number(parts.next()))
}

/// The line that starts a test, in the languages Aeroric's users write.
fn is_test(code: &str) -> bool {
    code.starts_with("#[test]")
        || code.starts_with("#[tokio::test")
        || code.starts_with("func test")
        || code.contains(" func test")
        || code.starts_with("def test_")
        || code.starts_with("async def test_")
        || code.starts_with("func Test")
        || code.starts_with("it(")
        || code.starts_with("test(")
}

fn is_test_path(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.starts_with("tests/")
        || path.contains("/tests/")
        || path.contains("/Tests/")
        || name.starts_with("test_")
        || name.contains("_test.")
        || name.contains("_tests.")
        || name.contains("Tests.")
        || name.contains(".test.")
        || name.contains(".spec.")
}

fn is_ci(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.starts_with(".github/workflows/")
        || name.starts_with(".gitlab-ci")
        || name == ".pre-commit-config.yaml"
        || name == "Makefile"
        || name == "justfile"
        || name == "package.json"
}

#[cfg(test)]
#[path = "gates_tests.rs"]
mod tests;
