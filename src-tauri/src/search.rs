use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_TEXT_SEARCH_RESULTS: usize = 500;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSearchMatch {
    pub path: String,
    pub name: String,
    pub line: usize,
    pub column: usize,
    pub line_text: String,
    pub match_text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSearchOptions {
    pub case_sensitive: Option<bool>,
    pub regex: Option<bool>,
    pub whole_word: Option<bool>,
    pub include_glob: Option<String>,
    pub exclude_glob: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextReplacement {
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub match_text: String,
    pub replacement_text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacePreviewMatch {
    pub path: String,
    pub name: String,
    pub line: usize,
    pub column: usize,
    pub line_text: String,
    pub match_text: String,
    pub replacement_text: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacePreviewFile {
    pub path: String,
    pub name: String,
    pub matches: Vec<ReplacePreviewMatch>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacePreview {
    pub query: String,
    pub replacement: String,
    pub files: Vec<ReplacePreviewFile>,
    pub total_matches: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceSummary {
    pub files_changed: usize,
    pub replacements_applied: usize,
    pub replacements_skipped: usize,
}

impl TextSearchOptions {
    fn limit(&self) -> usize {
        self.limit.unwrap_or(120).clamp(1, MAX_TEXT_SEARCH_RESULTS)
    }
}

#[derive(Debug, Deserialize)]
struct RgJsonLine {
    #[serde(rename = "type")]
    kind: String,
    data: Option<RgMatchData>,
}

#[derive(Debug, Deserialize)]
struct RgMatchData {
    path: RgText,
    lines: RgText,
    line_number: usize,
    submatches: Vec<RgSubmatch>,
}

#[derive(Debug, Deserialize)]
struct RgText {
    text: String,
}

#[derive(Debug, Deserialize)]
struct RgSubmatch {
    #[serde(rename = "match")]
    matched: RgText,
    start: usize,
}

fn validate_project_root(project_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(project_path);
    if !path.is_absolute() {
        return Err("Project path must be absolute".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    if !canonical.is_dir() {
        return Err("Project path is not a directory".to_string());
    }
    Ok(canonical)
}

pub fn parse_rg_json_lines(root: &Path, stdout: &str, limit: usize) -> Vec<TextSearchMatch> {
    let mut matches = Vec::new();
    for line in stdout.lines() {
        if matches.len() >= limit {
            break;
        }
        let Ok(parsed) = serde_json::from_str::<RgJsonLine>(line) else {
            continue;
        };
        if parsed.kind != "match" {
            continue;
        }
        let Some(data) = parsed.data else {
            continue;
        };
        let Some(submatch) = data.submatches.first() else {
            continue;
        };
        let path = root.join(&data.path.text);
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| data.path.text.clone());
        matches.push(TextSearchMatch {
            path: path.to_string_lossy().into_owned(),
            name,
            line: data.line_number,
            column: submatch.start + 1,
            line_text: data.lines.text.trim_end_matches(['\r', '\n']).to_string(),
            match_text: submatch.matched.text.clone(),
        });
    }
    matches
}

fn is_ignored_dir(name: &str) -> bool {
    matches!(name, ".git" | "node_modules" | "dist" | "target")
}

fn split_glob_patterns(value: Option<&str>) -> Vec<String> {
    value
        .into_iter()
        .flat_map(|value| value.split([',', ';', '\n']))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_start_matches("./").to_string())
        .collect()
}

fn glob_to_regex(pattern: &str, path_mode: bool) -> String {
    let mut regex = String::from("^");
    let chars: Vec<char> = pattern.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '*' if chars.get(index + 1) == Some(&'*') => {
                index += 2;
                if chars.get(index) == Some(&'/') {
                    index += 1;
                    regex.push_str("(?:.*/)?");
                } else {
                    regex.push_str(".*");
                }
            }
            '*' => {
                if path_mode {
                    regex.push_str("[^/]*");
                } else {
                    regex.push_str(".*");
                }
                index += 1;
            }
            '?' => {
                if path_mode {
                    regex.push_str("[^/]");
                } else {
                    regex.push('.');
                }
                index += 1;
            }
            literal => {
                regex.push_str(&regex::escape(&literal.to_string()));
                index += 1;
            }
        }
    }
    regex.push('$');
    regex
}

fn glob_matches(pattern: &str, rel_path: &str, file_name: &str) -> bool {
    let path_mode = pattern.contains('/');
    let target = if path_mode { rel_path } else { file_name };
    let regex = glob_to_regex(pattern, path_mode);
    regex::Regex::new(&regex)
        .map(|regex| regex.is_match(target))
        .unwrap_or(false)
}

fn matches_any_glob(patterns: &[String], rel_path: &str, file_name: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| glob_matches(pattern, rel_path, file_name))
}

fn file_allowed_by_globs(root: &Path, file_path: &Path, options: &TextSearchOptions) -> bool {
    let rel_path = file_path
        .strip_prefix(root)
        .unwrap_or(file_path)
        .to_string_lossy()
        .replace('\\', "/");
    let file_name = file_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel_path.clone());
    let include_patterns = split_glob_patterns(options.include_glob.as_deref());
    if !include_patterns.is_empty() && !matches_any_glob(&include_patterns, &rel_path, &file_name) {
        return false;
    }
    let exclude_patterns = split_glob_patterns(options.exclude_glob.as_deref());
    !matches_any_glob(&exclude_patterns, &rel_path, &file_name)
}

fn is_left_word_boundary(text: &str, index: usize) -> bool {
    text[..index]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric() && c != '_')
}

fn is_right_word_boundary(text: &str, index: usize) -> bool {
    text[index..]
        .chars()
        .next()
        .is_none_or(|c| !c.is_alphanumeric() && c != '_')
}

fn fallback_match_line(
    line: &str,
    query: &str,
    options: &TextSearchOptions,
) -> Result<Option<(usize, String)>, String> {
    if options.regex.unwrap_or(false) {
        let pattern = if options.case_sensitive.unwrap_or(false) {
            query.to_string()
        } else {
            format!("(?i){query}")
        };
        let re = regex::Regex::new(&pattern).map_err(|e| format!("Invalid regex: {e}"))?;
        for found in re.find_iter(line) {
            if options.whole_word.unwrap_or(false)
                && (!is_left_word_boundary(line, found.start())
                    || !is_right_word_boundary(line, found.end()))
            {
                continue;
            }
            return Ok(Some((found.start() + 1, found.as_str().to_string())));
        }
        return Ok(None);
    }

    let haystack = if options.case_sensitive.unwrap_or(false) {
        line.to_string()
    } else {
        line.to_ascii_lowercase()
    };
    let needle = if options.case_sensitive.unwrap_or(false) {
        query.to_string()
    } else {
        query.to_ascii_lowercase()
    };
    let mut search_start = 0;
    while let Some(relative_index) = haystack[search_start..].find(&needle) {
        let index = search_start + relative_index;
        let end = index + needle.len();
        if !options.whole_word.unwrap_or(false)
            || (is_left_word_boundary(line, index) && is_right_word_boundary(line, end))
        {
            return Ok(Some((index + 1, line[index..end].to_string())));
        }
        search_start = end;
    }
    Ok(None)
}

fn fallback_search_file_content(
    root: &Path,
    file_path: &Path,
    content: &str,
    query: &str,
    options: &TextSearchOptions,
    remaining: usize,
) -> Result<Vec<TextSearchMatch>, String> {
    let mut matches = Vec::new();
    let rel = file_path.strip_prefix(root).unwrap_or(file_path);
    let name = file_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_path.to_string_lossy().into_owned());
    for (index, line) in content.lines().enumerate() {
        if matches.len() >= remaining {
            break;
        }
        if let Some((column, match_text)) = fallback_match_line(line, query, options)? {
            matches.push(TextSearchMatch {
                path: file_path.to_string_lossy().into_owned(),
                name: name.clone(),
                line: index + 1,
                column,
                line_text: line.to_string(),
                match_text,
            });
        }
    }
    let _ = rel;
    Ok(matches)
}

fn fallback_search_dir(
    root: &Path,
    dir: &Path,
    query: &str,
    options: &TextSearchOptions,
    matches: &mut Vec<TextSearchMatch>,
) -> Result<(), String> {
    if matches.len() >= options.limit() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|e| format!("Read directory failed: {e}"))? {
        let entry = entry.map_err(|e| format!("Read directory entry failed: {e}"))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !is_ignored_dir(&name) {
                fallback_search_dir(root, &path, query, options, matches)?;
            }
            continue;
        }
        if !path.is_file() {
            continue;
        }
        if !file_allowed_by_globs(root, &path, options) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let remaining = options.limit().saturating_sub(matches.len());
        matches.extend(fallback_search_file_content(
            root, &path, &content, query, options, remaining,
        )?);
        if matches.len() >= options.limit() {
            break;
        }
    }
    Ok(())
}

fn fallback_search(
    root: &Path,
    query: &str,
    options: &TextSearchOptions,
) -> Result<Vec<TextSearchMatch>, String> {
    let mut matches = Vec::new();
    fallback_search_dir(root, root, query, options, &mut matches)?;
    Ok(matches)
}

#[derive(Debug, Clone)]
struct ReplacementMatch {
    start: usize,
    end: usize,
    match_text: String,
    replacement_text: String,
}

fn regex_for_query(query: &str, options: &TextSearchOptions) -> Result<regex::Regex, String> {
    let pattern = if options.case_sensitive.unwrap_or(false) {
        query.to_string()
    } else {
        format!("(?i){query}")
    };
    regex::Regex::new(&pattern).map_err(|e| format!("Invalid regex: {e}"))
}

fn replacement_matches_in_line(
    line: &str,
    query: &str,
    replacement: &str,
    options: &TextSearchOptions,
) -> Result<Vec<ReplacementMatch>, String> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    if options.regex.unwrap_or(false) {
        let re = regex_for_query(query, options)?;
        let mut matches = Vec::new();
        for captures in re.captures_iter(line) {
            let Some(found) = captures.get(0) else {
                continue;
            };
            if found.start() == found.end() {
                continue;
            }
            if options.whole_word.unwrap_or(false)
                && (!is_left_word_boundary(line, found.start())
                    || !is_right_word_boundary(line, found.end()))
            {
                continue;
            }
            let mut replacement_text = String::new();
            captures.expand(replacement, &mut replacement_text);
            matches.push(ReplacementMatch {
                start: found.start(),
                end: found.end(),
                match_text: found.as_str().to_string(),
                replacement_text,
            });
        }
        return Ok(matches);
    }

    let haystack = if options.case_sensitive.unwrap_or(false) {
        line.to_string()
    } else {
        line.to_ascii_lowercase()
    };
    let needle = if options.case_sensitive.unwrap_or(false) {
        query.to_string()
    } else {
        query.to_ascii_lowercase()
    };
    let mut matches = Vec::new();
    let mut search_start = 0;
    while let Some(relative_index) = haystack[search_start..].find(&needle) {
        let start = search_start + relative_index;
        let end = start + needle.len();
        if !options.whole_word.unwrap_or(false)
            || (is_left_word_boundary(line, start) && is_right_word_boundary(line, end))
        {
            matches.push(ReplacementMatch {
                start,
                end,
                match_text: line[start..end].to_string(),
                replacement_text: replacement.to_string(),
            });
        }
        search_start = end;
    }
    Ok(matches)
}

fn collect_replace_preview_file(
    root: &Path,
    file_path: &Path,
    content: &str,
    query: &str,
    replacement: &str,
    options: &TextSearchOptions,
    remaining: usize,
) -> Result<Option<ReplacePreviewFile>, String> {
    let path = file_path.to_string_lossy().into_owned();
    let name = file_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let mut matches = Vec::new();
    let mut line_start = 0;
    for (line_index, segment) in content.split_inclusive('\n').enumerate() {
        if matches.len() >= remaining {
            break;
        }
        let line = segment.trim_end_matches(['\r', '\n']);
        for found in replacement_matches_in_line(line, query, replacement, options)? {
            if matches.len() >= remaining {
                break;
            }
            matches.push(ReplacePreviewMatch {
                path: path.clone(),
                name: name.clone(),
                line: line_index + 1,
                column: found.start + 1,
                line_text: line.to_string(),
                match_text: found.match_text,
                replacement_text: found.replacement_text,
                start: line_start + found.start,
                end: line_start + found.end,
            });
        }
        line_start += segment.len();
    }
    if !content.ends_with('\n') {
        // split_inclusive already yielded the final line without a trailing newline.
    }
    let _ = root;
    if matches.is_empty() {
        Ok(None)
    } else {
        Ok(Some(ReplacePreviewFile {
            path,
            name,
            matches,
        }))
    }
}

fn collect_replace_preview_dir(
    root: &Path,
    dir: &Path,
    query: &str,
    replacement: &str,
    options: &TextSearchOptions,
    files: &mut Vec<ReplacePreviewFile>,
    total_matches: &mut usize,
) -> Result<bool, String> {
    if *total_matches >= options.limit() {
        return Ok(true);
    }
    for entry in fs::read_dir(dir).map_err(|e| format!("Read directory failed: {e}"))? {
        let entry = entry.map_err(|e| format!("Read directory entry failed: {e}"))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !is_ignored_dir(&name)
                && collect_replace_preview_dir(
                    root,
                    &path,
                    query,
                    replacement,
                    options,
                    files,
                    total_matches,
                )?
            {
                return Ok(true);
            }
            continue;
        }
        if !path.is_file() || !file_allowed_by_globs(root, &path, options) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let remaining = options.limit().saturating_sub(*total_matches);
        if let Some(file_preview) = collect_replace_preview_file(
            root,
            &path,
            &content,
            query,
            replacement,
            options,
            remaining,
        )? {
            *total_matches += file_preview.matches.len();
            files.push(file_preview);
        }
        if *total_matches >= options.limit() {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn replace_text_preview_for_root(
    root: &Path,
    query: &str,
    replacement: &str,
    options: &TextSearchOptions,
) -> Result<ReplacePreview, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(ReplacePreview {
            query,
            replacement: replacement.to_string(),
            files: Vec::new(),
            total_matches: 0,
            truncated: false,
        });
    }
    let mut files = Vec::new();
    let mut total_matches = 0;
    let truncated = collect_replace_preview_dir(
        root,
        root,
        &query,
        replacement,
        options,
        &mut files,
        &mut total_matches,
    )?;
    Ok(ReplacePreview {
        query,
        replacement: replacement.to_string(),
        files,
        total_matches,
        truncated,
    })
}

pub fn apply_text_replacements_for_root(
    root: &Path,
    replacements: &[TextReplacement],
) -> Result<ReplaceSummary, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    let mut grouped: BTreeMap<PathBuf, Vec<TextReplacement>> = BTreeMap::new();
    let mut skipped = 0;

    for replacement in replacements {
        let path = Path::new(&replacement.path);
        if !path.is_absolute() {
            skipped += 1;
            continue;
        }
        let Ok(canonical) = path.canonicalize() else {
            skipped += 1;
            continue;
        };
        if !canonical.starts_with(&root) || !canonical.is_file() {
            skipped += 1;
            continue;
        }
        let mut normalized = replacement.clone();
        normalized.path = canonical.to_string_lossy().into_owned();
        grouped.entry(canonical).or_default().push(normalized);
    }

    let mut files_changed = 0;
    let mut applied = 0;
    for (path, mut file_replacements) in grouped {
        let mut content =
            fs::read_to_string(&path).map_err(|e| format!("Read file failed: {e}"))?;
        file_replacements.sort_by(|left, right| right.start.cmp(&left.start));
        let mut applied_in_file = 0;
        for replacement in file_replacements {
            if replacement.start > replacement.end
                || replacement.end > content.len()
                || !content.is_char_boundary(replacement.start)
                || !content.is_char_boundary(replacement.end)
            {
                skipped += 1;
                continue;
            }
            if content.get(replacement.start..replacement.end) != Some(&replacement.match_text) {
                skipped += 1;
                continue;
            }
            content.replace_range(
                replacement.start..replacement.end,
                &replacement.replacement_text,
            );
            applied += 1;
            applied_in_file += 1;
        }
        if applied_in_file > 0 {
            fs::write(&path, content).map_err(|e| format!("Write file failed: {e}"))?;
            files_changed += 1;
        }
    }

    Ok(ReplaceSummary {
        files_changed,
        replacements_applied: applied,
        replacements_skipped: skipped,
    })
}

#[tauri::command]
pub async fn search_text(
    project_path: String,
    query: String,
    options: TextSearchOptions,
) -> Result<Vec<TextSearchMatch>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = validate_project_root(&project_path)?;
        let query = query.trim().to_string();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let limit = options.limit();
        let mut args = vec![
            "--json".to_string(),
            "--line-number".to_string(),
            "--column".to_string(),
            "--hidden".to_string(),
            "--glob".to_string(),
            "!.git/**".to_string(),
            "--glob".to_string(),
            "!node_modules/**".to_string(),
            "--glob".to_string(),
            "!dist/**".to_string(),
            "--glob".to_string(),
            "!target/**".to_string(),
        ];
        if !options.case_sensitive.unwrap_or(false) {
            args.push("--ignore-case".to_string());
        }
        if !options.regex.unwrap_or(false) {
            args.push("--fixed-strings".to_string());
        }
        if options.whole_word.unwrap_or(false) {
            args.push("--word-regexp".to_string());
        }
        for include in split_glob_patterns(options.include_glob.as_deref()) {
            args.push("--glob".to_string());
            args.push(include);
        }
        for exclude in split_glob_patterns(options.exclude_glob.as_deref()) {
            args.push("--glob".to_string());
            args.push(format!("!{exclude}"));
        }
        args.push(query.clone());

        let mut cmd = Command::new("rg");
        crate::subprocess::configure_background_command(&mut cmd);
        let output = match cmd.args(args).current_dir(&root).output() {
            Ok(output) => output,
            Err(_) => return fallback_search(&root, &query, &options),
        };

        if !output.status.success() && output.status.code() != Some(1) {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_rg_json_lines(&root, &stdout, limit))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn replace_text_preview(
    project_path: String,
    query: String,
    replacement: String,
    options: TextSearchOptions,
) -> Result<ReplacePreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = validate_project_root(&project_path)?;
        replace_text_preview_for_root(&root, &query, &replacement, &options)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn apply_text_replacements(
    project_path: String,
    replacements: Vec<TextReplacement>,
) -> Result<ReplaceSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = validate_project_root(&project_path)?;
        apply_text_replacements_for_root(&root, &replacements)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rg_json_match_lines() {
        let stdout = r#"{"type":"match","data":{"path":{"text":"src/App.tsx"},"lines":{"text":"const title = \"Aeroric\";\n"},"line_number":7,"absolute_offset":12,"submatches":[{"match":{"text":"title"},"start":6,"end":11}]}}"#;

        let matches = parse_rg_json_lines(Path::new("/repo"), stdout, 20);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "/repo/src/App.tsx");
        assert_eq!(matches[0].name, "App.tsx");
        assert_eq!(matches[0].line, 7);
        assert_eq!(matches[0].column, 7);
        assert_eq!(matches[0].line_text, "const title = \"Aeroric\";");
        assert_eq!(matches[0].match_text, "title");
    }

    #[test]
    fn respects_match_limit() {
        let stdout = r#"{"type":"match","data":{"path":{"text":"a.ts"},"lines":{"text":"alpha\n"},"line_number":1,"submatches":[{"match":{"text":"a"},"start":0,"end":1}]}}
{"type":"match","data":{"path":{"text":"b.ts"},"lines":{"text":"beta\n"},"line_number":2,"submatches":[{"match":{"text":"b"},"start":0,"end":1}]}}"#;

        let matches = parse_rg_json_lines(Path::new("/repo"), stdout, 1);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "/repo/a.ts");
    }

    #[test]
    fn fallback_fixed_search_respects_case_and_whole_word() {
        let options = TextSearchOptions {
            case_sensitive: Some(false),
            regex: Some(false),
            whole_word: Some(true),
            include_glob: None,
            exclude_glob: None,
            limit: Some(20),
        };

        let found = fallback_match_line("subtitle title", "title", &options).unwrap();

        assert_eq!(found, Some((10, "title".to_string())));
    }

    #[test]
    fn fallback_regex_search_returns_first_match() {
        let options = TextSearchOptions {
            case_sensitive: Some(false),
            regex: Some(true),
            whole_word: Some(false),
            include_glob: None,
            exclude_glob: None,
            limit: Some(20),
        };

        let found = fallback_match_line("let value = 42", r"\d+", &options).unwrap();

        assert_eq!(found, Some((13, "42".to_string())));
    }

    #[test]
    fn split_glob_patterns_accepts_common_separators() {
        let patterns = split_glob_patterns(Some(" ./src/**/*.ts,docs/*.md;\n tests/*.rs "));

        assert_eq!(patterns, vec!["src/**/*.ts", "docs/*.md", "tests/*.rs"]);
    }

    fn make_test_project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "aeroric-search-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn fallback_search_respects_include_and_exclude_globs() {
        let root = make_test_project("glob");
        fs::create_dir_all(root.join("src/generated")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("src/app.ts"), "needle in ts\n").unwrap();
        fs::write(root.join("src/app.md"), "needle in markdown\n").unwrap();
        fs::write(root.join("src/generated/skip.ts"), "needle in generated\n").unwrap();
        fs::write(root.join("docs/readme.md"), "needle in docs\n").unwrap();
        let options = TextSearchOptions {
            case_sensitive: Some(false),
            regex: Some(false),
            whole_word: Some(false),
            include_glob: Some("src/**/*.ts; docs/*.md".to_string()),
            exclude_glob: Some("src/generated/**\ndocs/readme.md".to_string()),
            limit: Some(20),
        };

        let matches = fallback_search(&root, "needle", &options).unwrap();

        assert_eq!(matches.len(), 1);
        assert_eq!(Path::new(&matches[0].path), root.join("src/app.ts"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replace_preview_returns_matches_without_writing_files() {
        let root = make_test_project("preview");
        let file = root.join("src/app.ts");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "const title = 'old';\nconst oldValue = title;\n").unwrap();
        let options = TextSearchOptions {
            case_sensitive: Some(true),
            regex: Some(false),
            whole_word: Some(true),
            include_glob: Some("src/**/*.ts".to_string()),
            exclude_glob: None,
            limit: Some(20),
        };

        let preview = replace_text_preview_for_root(&root, "old", "new", &options).unwrap();

        assert_eq!(preview.total_matches, 1);
        assert_eq!(preview.files.len(), 1);
        assert_eq!(preview.files[0].path, file.to_string_lossy());
        assert_eq!(preview.files[0].matches[0].match_text, "old");
        assert_eq!(preview.files[0].matches[0].replacement_text, "new");
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "const title = 'old';\nconst oldValue = title;\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn apply_text_replacements_rejects_paths_outside_project_root() {
        let root = make_test_project("apply-root");
        let outside = make_test_project("outside").join("outside.txt");
        fs::write(&outside, "old\n").unwrap();
        let replacements = vec![TextReplacement {
            path: outside.to_string_lossy().into_owned(),
            start: 0,
            end: 3,
            match_text: "old".to_string(),
            replacement_text: "new".to_string(),
        }];

        let summary = apply_text_replacements_for_root(&root, &replacements).unwrap();

        assert_eq!(summary.files_changed, 0);
        assert_eq!(summary.replacements_applied, 0);
        assert_eq!(summary.replacements_skipped, 1);
        assert_eq!(fs::read_to_string(&outside).unwrap(), "old\n");
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside.parent().unwrap()).unwrap();
    }

    #[test]
    fn apply_text_replacements_skips_stale_matches_without_writing_file() {
        let root = make_test_project("apply-stale");
        let file = root.join("src/app.ts");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "fresh\n").unwrap();
        let replacements = vec![TextReplacement {
            path: file.to_string_lossy().into_owned(),
            start: 0,
            end: 3,
            match_text: "old".to_string(),
            replacement_text: "new".to_string(),
        }];

        let summary = apply_text_replacements_for_root(&root, &replacements).unwrap();

        assert_eq!(summary.files_changed, 0);
        assert_eq!(summary.replacements_applied, 0);
        assert_eq!(summary.replacements_skipped, 1);
        assert_eq!(fs::read_to_string(&file).unwrap(), "fresh\n");
        fs::remove_dir_all(root).unwrap();
    }
}
