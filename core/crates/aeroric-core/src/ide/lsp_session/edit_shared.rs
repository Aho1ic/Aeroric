//! LSP 文本编辑的**共同算法**:UTF-16 位置 → 字节偏移、倒序区间替换。
//!
//! 纯函数,无副作用、不读盘。本地与远端两套落盘实现共同复用这一份(位置换算错了
//! 就会切在字符中间——LSP 的 character 是 **UTF-16 码元**偏移,不是字节也不是字符)。

use crate::ide::lsp_session::{LspPosition, LspTextEdit};

/// 一次 `apply_text_edits_to_content` 的结果(应用/跳过计数 + 新内容)。
#[derive(Debug)]
pub struct AppliedTextEdits {
    pub content: String,
    pub edits_applied: usize,
    pub edits_skipped: usize,
}

pub fn apply_text_edits_to_content(content: &str, edits: &[LspTextEdit]) -> AppliedTextEdits {
    let mut ranges = Vec::new();
    let mut edits_skipped = 0;
    for text_edit in edits {
        let Some(start) = lsp_position_to_offset(content, &text_edit.range.start) else {
            edits_skipped += 1;
            continue;
        };
        let Some(end) = lsp_position_to_offset(content, &text_edit.range.end) else {
            edits_skipped += 1;
            continue;
        };
        if start > end {
            edits_skipped += 1;
            continue;
        }
        ranges.push((start, end, text_edit.new_text.clone()));
    }

    ranges.sort_by_key(|right| std::cmp::Reverse(right.0));
    let mut next_content = content.to_string();
    let mut edits_applied = 0;
    for (start, end, new_text) in ranges {
        if end > next_content.len()
            || !next_content.is_char_boundary(start)
            || !next_content.is_char_boundary(end)
        {
            edits_skipped += 1;
            continue;
        }
        next_content.replace_range(start..end, &new_text);
        edits_applied += 1;
    }

    AppliedTextEdits {
        content: next_content,
        edits_applied,
        edits_skipped,
    }
}

pub fn lsp_position_to_offset(content: &str, position: &LspPosition) -> Option<usize> {
    let mut current_line = 0_u32;
    let mut line_start = 0_usize;
    for segment in content.split_inclusive('\n') {
        let line_end = line_start + segment.len();
        if current_line == position.line {
            return utf16_character_to_offset(content, line_start, line_end, position.character);
        }
        line_start = line_end;
        current_line += 1;
    }
    if current_line == position.line {
        return utf16_character_to_offset(content, line_start, content.len(), position.character);
    }
    None
}

pub fn utf16_character_to_offset(
    content: &str,
    line_start: usize,
    mut line_end: usize,
    character: u32,
) -> Option<usize> {
    if line_end > line_start && content.as_bytes().get(line_end - 1) == Some(&b'\n') {
        line_end -= 1;
    }
    if line_end > line_start && content.as_bytes().get(line_end - 1) == Some(&b'\r') {
        line_end -= 1;
    }

    let line = content.get(line_start..line_end)?;
    let mut units = 0_u32;
    for (relative, ch) in line.char_indices() {
        if units == character {
            return Some(line_start + relative);
        }
        units += ch.len_utf16() as u32;
        if units > character {
            return None;
        }
    }
    (units == character).then_some(line_end)
}
