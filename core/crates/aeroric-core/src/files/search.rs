//! P6-01 Action 1：搜索替换的业务主体（core 独占；rg JSON 解析与宿主同源，
//! 替换先 preview 再按匹配区间提交——stale match 拒绝不覆盖）。
pub use super::path_policy::MAX_FILE_BYTES;

/// 搜索选项（与 Tauri `search::TextSearchOptions` 对齐；limit clamp 1..500）。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextSearchOptions {
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
    pub limit: usize,
}

impl Default for TextSearchOptions {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            regex: false,
            whole_word: false,
            limit: 120,
        }
    }
}

impl TextSearchOptions {
    pub fn effective_limit(&self) -> usize {
        self.limit.clamp(1, 500)
    }
}

/// 搜索命中（与 Tauri `TextSearchMatch` camelCase 字段对齐）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSearchMatch {
    pub path: String,
    pub name: String,
    pub line: u64,
    pub absolute_offset: u64,
    pub submatches: Vec<Submatch>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Submatch {
    pub match_text: String,
    pub start: u64,
    pub end: u64,
}

/// rg JSON 行解析（业务主体；与宿主 `parse_rg_json_lines` 同语义——begin 行
/// 记 path/name，match 行记 line/offset/submatches）。
pub fn parse_rg_json_lines(stdout: &str, limit: usize) -> Vec<TextSearchMatch> {
    let mut matches: Vec<TextSearchMatch> = Vec::new();
    let mut current_path = String::new();
    let mut current_name = String::new();
    for line in stdout.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match value.get("type").and_then(|t| t.as_str()) {
            Some("begin") => {
                let data = value.get("data");
                current_path = data
                    .and_then(|d| d.get("path"))
                    .and_then(|p| p.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or_default()
                    .to_string();
                current_name = std::path::Path::new(&current_path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
            }
            Some("match") => {
                if matches.len() >= limit {
                    continue;
                }
                let data = value.get("data");
                let line_number = data
                    .and_then(|d| d.get("line_number"))
                    .and_then(|l| l.as_u64())
                    .unwrap_or(0);
                let absolute_offset = data
                    .and_then(|d| d.get("absolute_offset"))
                    .and_then(|o| o.as_u64())
                    .unwrap_or(0);
                let submatches = data
                    .and_then(|d| d.get("submatches"))
                    .and_then(|s| s.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| {
                                Some(Submatch {
                                    match_text: item
                                        .get("match")
                                        .and_then(|m| m.get("text"))
                                        .and_then(|t| t.as_str())?
                                        .to_string(),
                                    start: item.get("start").and_then(|v| v.as_u64())?,
                                    end: item.get("end").and_then(|v| v.as_u64())?,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                matches.push(TextSearchMatch {
                    path: current_path.clone(),
                    name: current_name.clone(),
                    line: line_number,
                    absolute_offset,
                    submatches,
                });
            }
            _ => {}
        }
    }
    matches
}

/// 替换预览（Action 4：替换先 preview——逐文件列出命中区间，不写盘）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacePreviewFile {
    pub path: String,
    pub matches: Vec<(u64, u64)>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacePreview {
    pub query: String,
    pub replacement: String,
    pub files: Vec<ReplacePreviewFile>,
}

/// 从搜索命中构造替换预览（纯函数；区间来自 submatches，不重扫文件）。
pub fn replace_preview(
    query: &str,
    replacement: &str,
    matches: &[TextSearchMatch],
) -> ReplacePreview {
    let mut files: Vec<ReplacePreviewFile> = Vec::new();
    for m in matches {
        let intervals: Vec<(u64, u64)> = m
            .submatches
            .iter()
            .map(|s| (m.absolute_offset + s.start, m.absolute_offset + s.end))
            .collect();
        if intervals.is_empty() {
            continue;
        }
        if let Some(existing) = files.iter_mut().find(|f| f.path == m.path) {
            existing.matches.extend(intervals);
        } else {
            files.push(ReplacePreviewFile {
                path: m.path.clone(),
                matches: intervals,
            });
        }
    }
    ReplacePreview {
        query: query.to_string(),
        replacement: replacement.to_string(),
        files,
    }
}

/// 替换应用结果（与 Tauri `ReplaceSummary` 对齐）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceSummary {
    pub files_changed: usize,
    pub replacements_applied: usize,
    pub replacements_skipped: usize,
}

/// 应用替换（业务主体）：按文件的字节区间倒序替换（区间互不挪动）；文件
/// 在读取与写入之间被外部修改（长度不符预期）时**拒绝覆盖**——stale skip。
pub fn apply_replacements(
    root: &std::path::Path,
    replacement: &str,
    files: &[ReplacePreviewFile],
) -> Result<ReplaceSummary, String> {
    let mut summary = ReplaceSummary {
        files_changed: 0,
        replacements_applied: 0,
        replacements_skipped: 0,
    };
    for file in files {
        let canonical = super::path_policy::resolve_within(root, &file.path)?;
        let bytes = std::fs::read(&canonical)
            .map_err(|error| format!("cannot read {}: {error}", file.path))?;
        let mut content = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                summary.replacements_skipped += file.matches.len();
                continue;
            }
        };
        // 预览时的文件长度：最大 end 超过当前长度＝读与写之间被外部修改。
        let expected_len = file.matches.iter().map(|(_, end)| *end).max().unwrap_or(0);
        if (content.len() as u64) < expected_len {
            summary.replacements_skipped += file.matches.len();
            continue;
        }
        // 倒序替换：区间互不挪动。
        let mut sorted = file.matches.clone();
        sorted.sort_by(|a, b| b.0.cmp(&a.0));
        let mut applied = 0usize;
        for (start, end) in sorted {
            let (start, end) = (start as usize, end as usize);
            if end > content.len() {
                summary.replacements_skipped += 1;
                continue;
            }
            content.replace_range(start..end, replacement);
            applied += 1;
        }
        if applied > 0 {
            std::fs::write(&canonical, content.as_bytes())
                .map_err(|error| format!("cannot write {}: {error}", file.path))?;
            summary.files_changed += 1;
            summary.replacements_applied += applied;
        }
    }
    Ok(summary)
}
