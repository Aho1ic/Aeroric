//! DSH 终端 projection 的行生成原语（P3-05 第七片，Action 1 的 `projection` 前身）。
//!
//! 逐字迁自宿主 `src-tauri/src/dsh_webui/terminal_render.rs` 的无状态一半：折叠摘要、
//! 回显行、ANSI 剥离与宽度裁剪。**Action 2 的边界随迁言明：DSH terminal projection
//! 只是适配输出，不能借用到 OMP**——本文件服务 dsh Web-API 会话的 xterm 视图
//! （`convertEol: false`，每行显式 CRLF 收尾），不是通用终端模型。
//! 有状态的两台（`TerminalWrap` 软换行、`ReasoningFold` 折叠累加器）仍留宿主，
//! 它们随 executor 接线片再议；`ANSI_GREEN`／`ANSI_RED` 被宿主的
//! `render_tool_event_view` 消费，也留宿主。

use serde_json::Value;

pub const ANSI_RESET: &str = "\x1b[0m";
pub const ANSI_DIM: &str = "\x1b[2m";
pub const ANSI_BOLD: &str = "\x1b[1m";

/// Widest folded fragment the bridge prints. Rows stay well inside a narrow
/// split pane, so a folded summary never wraps into a second terminal line.
pub const FOLD_SUMMARY_CHARS: usize = 96;

/// Non-empty string field, matching how the harness treats a blank title.
pub fn view_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

pub fn push_line(out: &mut String, style: &str, text: &str) {
    out.push_str(style);
    out.push_str(text);
    if !style.is_empty() {
        out.push_str(ANSI_RESET);
    }
    out.push_str("\r\n");
}

/// 用户输入在终端里的回显行。Web API 会话没有 PTY,终端不会自动回显,不自己
/// 打一行的话发出去的内容在终端里完全看不到。
///
/// 斜杠命令由 `command/run` 事件负责回显,这里跳过,避免同一条输入出现两次。
pub fn user_prompt_echo(prompt: &str, image_count: usize) -> Option<String> {
    let text = prompt.trim();
    if text.starts_with('/') {
        return None;
    }
    if text.is_empty() && image_count == 0 {
        return None;
    }
    let attachment = match image_count {
        0 => String::new(),
        1 => format!(" {ANSI_DIM}· 1 image{ANSI_RESET}"),
        count => format!(" {ANSI_DIM}· {count} images{ANSI_RESET}"),
    };
    // 续行缩进两格,与首行的 "❯ " 对齐。
    let body = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n  ");
    let mut out = String::from("\r\n");
    push_line(
        &mut out,
        "",
        &format!("{ANSI_BOLD}❯{ANSI_RESET} {body}{attachment}"),
    );
    Some(out)
}

/// Drop ANSI escape sequences and control bytes from one line. Tool output is
/// frequently styled, and a folded row that carried a cursor move or a
/// half-open colour span would corrupt every row printed after it.
pub fn plain_single_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\x1b' {
            if character == '\t' {
                out.push(' ');
            } else if !character.is_control() {
                out.push(character);
            }
            continue;
        }
        match characters.peek() {
            // CSI: parameters and intermediates, terminated by a final byte.
            Some('[') => {
                let _ = characters.next();
                for next in characters.by_ref() {
                    if ('\x40'..='\x7e').contains(&next) {
                        break;
                    }
                }
            }
            // OSC: terminated by BEL or by ST (ESC \).
            Some(']') => {
                let _ = characters.next();
                let mut saw_escape = false;
                for next in characters.by_ref() {
                    if saw_escape || next == '\x07' {
                        break;
                    }
                    saw_escape = next == '\x1b';
                }
            }
            _ => {
                let _ = characters.next();
            }
        }
    }
    out.trim().to_string()
}

pub fn clip_fragment(text: &str) -> String {
    if text.chars().count() <= FOLD_SUMMARY_CHARS {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(FOLD_SUMMARY_CHARS).collect();
    clipped.push('…');
    clipped
}

/// Reduce a possibly long, possibly styled body to the single line a folded row
/// can carry: the first line with content, stripped of control bytes and
/// clipped. `None` means the body had nothing to say.
pub fn fold_summary(text: &str) -> Option<String> {
    text.split('\n')
        .map(plain_single_line)
        .find(|line| !line.is_empty())
        .map(|line| clip_fragment(&line))
}

/// Lines with content, used as a folded row's size hint. Blank lines are the
/// padding of a body that is not being printed, so they are not counted.
pub fn count_content_lines(text: &str) -> usize {
    text.split('\n')
        .filter(|line| !line.trim().is_empty())
        .count()
}

pub fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 {
        one
    } else {
        many
    }
}

// ── 第九片（补记二百五十八）：有状态两台与文本 helpers 机械迁移自宿主
// `terminal_render.rs`——源文件头注释自证「这 800 多行对宿主句柄、API client、
// manager 全都没有引用」；`render_tool_event_view`/`diff_rows` 等渲染分支
// 暂留宿主，re-export 按引用面收窄。`MIN_WRAP_COLS` 等三常量、`WordPart`、
// `read_ansi_sequence` 是两台的内部依赖，一并搬走。
/// xterm is intentionally configured with `convertEol: false`, so the DSH
/// bridge owns the line-ending contract. Normalize every newline form without
/// touching ANSI escape bytes or Unicode text.
pub fn normalize_terminal_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\r' => {
                normalized.push('\r');
                if chars.peek() == Some(&'\n') {
                    let _ = chars.next();
                }
                normalized.push('\n');
            }
            '\n' => normalized.push_str("\r\n"),
            _ => normalized.push(character),
        }
    }
    normalized
}

/// 低于这个列数(或者前端还没上报过尺寸)就不做软换行:窄到这种程度时按词
/// 折行只会把每个词排成竖列,不如交给 xterm 自己硬折。
pub const MIN_WRAP_COLS: usize = 20;

/// 制表位宽度,与 xterm 默认一致。
const TAB_WIDTH: usize = 8;

/// 连续换行的上限,也就是最多留一个空行。
const MAX_CONSECUTIVE_NEWLINES: usize = 2;

/// 待落地单词里的一段。转义序列不占列宽,但必须跟着单词一起搬到下一行,
/// 否则换行会把一个半开的颜色区间拆开。
#[derive(Debug)]
pub enum WordPart {
    Escape(String),
    Glyph(char, usize),
}

/// 一个任务的软换行状态。
///
/// `cols` 是前端最近上报的终端列数;`column` 是跨 chunk 累积的光标列,因为
/// 一段回答是按增量分多次到达的;`word` 是行尾尚未落地的单词——只有确定它
/// 放得下当前行,才会写出去,这样右边界上就不会出现被劈成两半的单词。
#[derive(Debug, Default)]
pub struct TerminalWrap {
    cols: usize,
    column: usize,
    word: Vec<WordPart>,
    word_width: usize,
    /// 已写出的尾部连续换行数,跨 chunk 累积——一段回答是分多次到达的,空行
    /// 常常正好落在两个 chunk 的接缝上。
    newline_run: usize,
    /// 是否已经写出过实际内容。首屏之前的空行一律丢掉。
    wrote_content: bool,
}

impl TerminalWrap {
    fn wraps(&self) -> bool {
        self.cols >= MIN_WRAP_COLS
    }

    pub fn set_cols(&mut self, cols: usize) {
        if self.cols == cols {
            return;
        }
        self.cols = cols;
        // 变窄之后旧的光标列可能已经越界,按新宽度重新起一行计算。
        if self.column >= cols {
            self.column = 0;
        }
    }

    /// 落地待定单词:整词放不下当前行时先换行,再逐字排布。超长单词(URL、
    /// 长路径)仍然按列硬折,但折点落在行末,而不是把普通单词拦腰截断。
    pub fn flush(&mut self) -> String {
        if self.word.is_empty() {
            self.word_width = 0;
            return String::new();
        }
        let mut out = String::new();
        let cols = self.cols;
        let wraps = self.wraps();
        if wraps
            && self.column > 0
            && self.column + self.word_width > cols
            && self.word_width <= cols
        {
            out.push_str("\r\n");
            self.column = 0;
        }
        for part in self.word.drain(..) {
            match part {
                WordPart::Escape(sequence) => out.push_str(&sequence),
                WordPart::Glyph(character, width) => {
                    if wraps && self.column > 0 && self.column + width > cols {
                        out.push_str("\r\n");
                        self.column = 0;
                    }
                    out.push(character);
                    self.column += width;
                }
            }
        }
        self.word_width = 0;
        out
    }

    /// 压掉多余空行:连续空行最多留一个,首屏之前的空行全部丢掉。
    ///
    /// DSH 的各个渲染分支(工具卡片、推理折叠、用户回显)各自都会先写一个
    /// `\r\n` 开头做间隔,markdown 正文自己又带空行,叠起来终端一屏能少放好几
    /// 行内容。这里是唯一的写出口,统一在这里收口比逐个分支去调更可靠。
    ///
    /// 只含转义序列的"空行"照样透传序列(否则颜色区间会被拆开),但不算内容。
    fn collapse_blank_lines(&mut self, text: String) -> String {
        if text.is_empty() {
            return text;
        }
        let mut out = String::with_capacity(text.len());
        // `normalize_terminal_text` 已把换行统一成 `\r\n`,按 `\n` 切即可。
        let mut segments = text.split('\n').peekable();
        while let Some(segment) = segments.next() {
            // 除最后一段外,每段后面都跟着一个被 split 吃掉的换行。
            let terminated = segments.peek().is_some();
            let body = segment.strip_suffix('\r').unwrap_or(segment);
            let (visible, escapes) = split_ansi_sequences(body);
            if visible.trim().is_empty() {
                // 空行:转义序列照旧透传(否则颜色区间被拆),换行本身按上限收。
                out.push_str(&escapes);
                if !terminated {
                    continue;
                }
                // 首屏之前的空行直接丢;之后连续换行到上限就停。
                if self.wrote_content && self.newline_run < MAX_CONSECUTIVE_NEWLINES {
                    out.push_str("\r\n");
                    self.newline_run += 1;
                }
                continue;
            }
            out.push_str(body);
            self.wrote_content = true;
            if terminated {
                out.push_str("\r\n");
                self.newline_run = 1;
            } else {
                self.newline_run = 0;
            }
        }
        out
    }

    /// 接入一段终端输出,返回可直接写给 xterm 的文本。
    pub fn push(&mut self, text: &str) -> String {
        let text = self.collapse_blank_lines(normalize_terminal_text(text));
        if text.is_empty() {
            return text;
        }
        if !self.wraps() {
            let mut out = self.flush();
            out.push_str(&text);
            return out;
        }
        let cols = self.cols;
        let mut out = String::with_capacity(text.len() + text.len() / 8);
        let mut characters = text.chars().peekable();
        while let Some(character) = characters.next() {
            match character {
                '\x1b' => {
                    // 转义序列不占列宽,但要跟着单词一起搬到下一行,否则换行会
                    // 落在一个半开的颜色区间里。
                    let sequence = read_ansi_sequence(&mut characters);
                    self.word.push(WordPart::Escape(sequence));
                }
                '\r' | '\n' => {
                    out.push_str(&self.flush());
                    out.push(character);
                    self.column = 0;
                }
                '\t' => {
                    out.push_str(&self.flush());
                    let advance = TAB_WIDTH - self.column % TAB_WIDTH;
                    if self.column + advance >= cols {
                        out.push_str("\r\n");
                        self.column = 0;
                    } else {
                        out.push('\t');
                        self.column += advance;
                    }
                }
                ' ' => {
                    out.push_str(&self.flush());
                    // 行末的空格本身就是断行点,换行取代这个空格,避免下一行
                    // 以一个空格开头。
                    if self.column + 1 >= cols {
                        out.push_str("\r\n");
                        self.column = 0;
                    } else {
                        out.push(' ');
                        self.column += 1;
                    }
                }
                _ if character.is_control() => {
                    // 其它控制字节不占列宽,原样透传。
                    out.push_str(&self.flush());
                    out.push(character);
                }
                _ => {
                    let width = terminal_char_width(character);
                    if width == 2 {
                        // CJK / emoji 每个字都能断行,不需要攒成单词。
                        out.push_str(&self.flush());
                        if self.column > 0 && self.column + width > cols {
                            out.push_str("\r\n");
                            self.column = 0;
                        }
                        out.push(character);
                        self.column += width;
                        continue;
                    }
                    self.word.push(WordPart::Glyph(character, width));
                    self.word_width += width;
                    // 单词本身已经宽过一整行,再攒下去也只能硬折,先落地。
                    if self.word_width >= cols {
                        out.push_str(&self.flush());
                    }
                }
            }
        }
        out
    }
}

/// 把一行拆成"可见文本"与"转义序列",一趟走完。
///
/// 判空行只能看可见部分:一行 SGR 颜色码的宽度是 0,但它仍要原样透传,否则
/// 空行被压掉的同时会把一个半开的颜色区间也吃掉。
pub fn split_ansi_sequences(text: &str) -> (String, String) {
    if !text.contains('\x1b') {
        return (text.to_string(), String::new());
    }
    let mut visible = String::with_capacity(text.len());
    let mut escapes = String::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\x1b' {
            escapes.push_str(&read_ansi_sequence(&mut characters));
        } else {
            visible.push(character);
        }
    }
    (visible, escapes)
}

/// 读走一个完整的转义序列(ESC 已由调用方消费),原样返回以便透传。
fn read_ansi_sequence(characters: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut sequence = String::from("\x1b");
    match characters.peek() {
        // CSI:参数与中间字节,以 final byte 结束。
        Some('[') => {
            sequence.push('[');
            let _ = characters.next();
            for next in characters.by_ref() {
                sequence.push(next);
                if ('\x40'..='\x7e').contains(&next) {
                    break;
                }
            }
        }
        // OSC:以 BEL 或 ST(ESC \)结束。
        Some(']') => {
            sequence.push(']');
            let _ = characters.next();
            let mut saw_escape = false;
            for next in characters.by_ref() {
                sequence.push(next);
                if saw_escape || next == '\x07' {
                    break;
                }
                saw_escape = next == '\x1b';
            }
        }
        Some(_) => {
            if let Some(next) = characters.next() {
                sequence.push(next);
            }
        }
        None => {}
    }
    sequence
}

/// 终端列宽近似:CJK / 全角 / 常见 emoji 占两列,组合符与零宽字符占零列,
/// 其余按一列算。仓库没有引入 unicode-width,这里只覆盖 DSH 输出实际会出现
/// 的区间——判宽偏窄只会让折行提前,不会把字符挤出右边界。
pub fn terminal_char_width(character: char) -> usize {
    let code = character as u32;
    if matches!(
        code,
        0x0300..=0x036F
            | 0x0483..=0x0489
            | 0x0591..=0x05BD
            | 0x0610..=0x061A
            | 0x064B..=0x065F
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x200B..=0x200F
            | 0x20D0..=0x20FF
            | 0xFE00..=0xFE0F
            | 0xFE20..=0xFE2F
            | 0xFEFF
    ) {
        return 0;
    }
    if matches!(
        code,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F004
            | 0x1F0CF
            | 0x1F18E
            | 0x1F191..=0x1F19A
            | 0x1F200..=0x1F2FF
            | 0x1F300..=0x1F64F
            | 0x1F680..=0x1F6FF
            | 0x1F7E0..=0x1F7EB
            | 0x1F900..=0x1F9FF
            | 0x1FA70..=0x1FAFF
            | 0x20000..=0x2FFFD
            | 0x30000..=0x3FFFD
    ) {
        return 2;
    }
    1
}

/// One folded reasoning row per assistant block.
///
/// dsh's own web UI renders reasoning as a collapsed "Think" disclosure and
/// settles on the block's first line once it finishes; nothing of the raw
/// `reasoning-delta` stream reaches the transcript unless the row is expanded.
/// The terminal has no disclosure to expand, so streaming those deltas verbatim
/// buried the answer and every tool row under the model's scratch work. This
/// accumulator keeps only what the folded row prints — the first line with
/// content and how many lines followed it — and the full reasoning text stays
/// available in the session record, which renders it as a collapsible block.
#[derive(Default)]
pub struct ReasoningFold {
    /// First line with content, clipped to one terminal row.
    summary: String,
    summary_chars: usize,
    /// The summary closes at the first newline that follows content.
    summary_closed: bool,
    /// Lines with content seen so far.
    lines: usize,
    /// Whether the line currently being accumulated carries content.
    line_has_content: bool,
}

impl ReasoningFold {
    pub fn push(&mut self, delta: &str) {
        for character in delta.chars() {
            if character == '\n' {
                if self.line_has_content {
                    self.lines += 1;
                    self.summary_closed = !self.summary.is_empty();
                }
                self.line_has_content = false;
                continue;
            }
            if character.is_control() {
                continue;
            }
            if !character.is_whitespace() {
                self.line_has_content = true;
            }
            if self.summary_closed || (self.summary.is_empty() && character.is_whitespace()) {
                continue;
            }
            match self.summary_chars.cmp(&FOLD_SUMMARY_CHARS) {
                std::cmp::Ordering::Less => self.summary.push(character),
                std::cmp::Ordering::Equal => self.summary.push('…'),
                std::cmp::Ordering::Greater => continue,
            }
            self.summary_chars += 1;
        }
    }

    /// Close the block and return its folded row. `None` means no reasoning was
    /// collected, so the terminal prints nothing at all.
    pub fn take_row(&mut self) -> Option<String> {
        if self.line_has_content {
            self.lines += 1;
        }
        let fold = std::mem::take(self);
        let summary = fold.summary.trim_end();
        if summary.is_empty() {
            return None;
        }
        let size = if fold.lines > 1 {
            format!(" · {} lines", fold.lines)
        } else {
            String::new()
        };
        let mut out = String::from("\r\n");
        push_line(&mut out, ANSI_DIM, &format!("✻ Thinking · {summary}{size}"));
        Some(out)
    }
}

// ── 第十片（补记二百五十九）：渲染分支一族（diff/paths/call/result/envelope）机械
// 迁移自宿主 terminal_render.rs——terminal 面最后一块纯地：消费 `&Value` 输出折叠行，
// 未知 card/shape 一律 `None`（回退 raw event 是 dsh 为无该能力 UI 规定的行为）。
/// Minimal ANSI styling for the tool render-intent output. The dsh stream lands
/// in an xterm view configured with `convertEol: false`, so every line this
/// module writes terminates with an explicit CRLF.
pub const ANSI_GREEN: &str = "\x1b[32m";
pub const ANSI_RED: &str = "\x1b[31m";
/// Split a single-file change into signed rows. `old_text` of `None` is the
/// harness's `oldText: null` — a create or an overwrite, with no before-image to
/// compare against, so every line reads as an addition.
pub fn diff_rows(old_text: Option<&str>, new_text: &str) -> Vec<(char, String)> {
    let next: Vec<&str> = new_text.split('\n').collect();
    let Some(old_text) = old_text else {
        return next
            .iter()
            .map(|line| ('+', line.trim_end_matches('\r').to_string()))
            .collect();
    };
    let prev: Vec<&str> = old_text.split('\n').collect();
    // A common prefix/suffix trim keeps the counted hunk tight without pulling a
    // full LCS diff into the backend; results already arrive as focused hunks.
    let mut head = 0;
    while head < prev.len() && head < next.len() && prev[head] == next[head] {
        head += 1;
    }
    let mut tail = 0;
    while tail < prev.len() - head
        && tail < next.len() - head
        && prev[prev.len() - 1 - tail] == next[next.len() - 1 - tail]
    {
        tail += 1;
    }
    let mut rows = Vec::new();
    for line in &prev[head..prev.len() - tail] {
        rows.push(('-', line.trim_end_matches('\r').to_string()));
    }
    for line in &next[head..next.len() - tail] {
        rows.push(('+', line.trim_end_matches('\r').to_string()));
    }
    rows
}

/// Name the files a change touches without printing any of it: the exact path
/// when a change is single-file, a count otherwise. `None` means the harness
/// sent a `diff` view with nothing usable in it.
fn diff_paths_fragment(diffs: &[Value]) -> Option<String> {
    let paths: Vec<&str> = diffs
        .iter()
        .filter_map(|diff| view_str(diff, "path"))
        .collect();
    match paths.as_slice() {
        [] => None,
        [path] => {
            let created = diffs
                .first()
                .is_some_and(|diff| diff.get("oldText").is_some_and(Value::is_null));
            Some(if created {
                format!("{path} (new file)")
            } else {
                (*path).to_string()
            })
        }
        paths => Some(format!("{} files", paths.len())),
    }
}

/// Render a pending-call view as one folded row: the tool's title plus a
/// single-line hint of what it is about to do. dsh web shows the same header on
/// a collapsed card and keeps the body — the full command, the proposed hunks,
/// the raw input — behind a disclosure. A terminal has no disclosure, so the
/// body is left to the session record and the insights trajectory.
fn render_tool_call_view(view: &Value) -> Option<String> {
    let title = view_str(view, "title")?;
    let mut kind = String::new();
    let mut detail: Vec<String> = Vec::new();
    match view.get("card").and_then(Value::as_str)? {
        "terminal" => {
            if let Some(description) = view_str(view, "description").and_then(fold_summary) {
                detail.push(description);
            }
        }
        "diff" => {
            let diffs = view.get("diffs").and_then(Value::as_array)?;
            if let Some(paths) = diff_paths_fragment(diffs) {
                detail.push(paths);
            }
        }
        "generic" => {
            // rawInput is deliberately not printed: it is the unparsed tool
            // input and can be large. The insights trajectory shows it in full.
            if let Some(label) = view_str(view, "kind") {
                kind = format!(" {ANSI_DIM}({label}){ANSI_RESET}");
            }
            if let Some(location) = view
                .get("locations")
                .and_then(Value::as_array)
                .and_then(|locations| locations.first())
            {
                if let Some(path) = view_str(location, "path") {
                    let line = location
                        .get("line")
                        .and_then(Value::as_i64)
                        .map(|line| format!(":{line}"))
                        .unwrap_or_default();
                    detail.push(format!("{path}{line}"));
                }
            }
        }
        _ => return None,
    }
    let mut row = format!("{ANSI_BOLD}▸ {title}{ANSI_RESET}{kind}");
    if !detail.is_empty() {
        row.push_str(&format!(" {ANSI_DIM}· {}{ANSI_RESET}", detail.join(" · ")));
    }
    let mut out = String::from("\r\n");
    push_line(&mut out, "", &row);
    Some(out)
}

/// Render a completed-call view as one folded row that closes the call row
/// printed just above it: a state glyph, the harness's replacement title when
/// it sends one, and a one-line verdict. Bodies are summarized rather than
/// printed — dsh web keeps them collapsed on the same row, and a terminal that
/// dumped every command's output, every hunk and every matched line drowned the
/// conversation it was supposed to frame.
fn render_tool_result_view(view: &Value) -> Option<String> {
    let card = view.get("card").and_then(Value::as_str)?;
    let mut failed = false;
    let mut detail: Vec<String> = Vec::new();
    match card {
        "terminal" => {
            // exitCode and signal are mutually exclusive; a signal is the
            // stronger statement about how the run ended, so it wins.
            if let Some(signal) = view_str(view, "signal") {
                failed = true;
                detail.push(signal.to_string());
            } else if let Some(code) = view
                .get("exitCode")
                .and_then(Value::as_i64)
                .filter(|code| *code != 0)
            {
                failed = true;
                detail.push(format!("exit {code}"));
            }
            let output = view
                .get("output")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match fold_summary(output) {
                Some(summary) => {
                    detail.push(summary);
                    let lines = count_content_lines(output);
                    if lines > 1 {
                        detail.push(format!("{lines} lines"));
                    }
                }
                None => detail.push("no output".to_string()),
            }
        }
        "diff" => {
            let diffs = view.get("diffs").and_then(Value::as_array)?;
            let paths = diff_paths_fragment(diffs)?;
            let (mut added, mut removed) = (0usize, 0usize);
            for diff in diffs {
                let Some(new_text) = diff.get("newText").and_then(Value::as_str) else {
                    continue;
                };
                for (sign, _) in diff_rows(diff.get("oldText").and_then(Value::as_str), new_text) {
                    if sign == '+' {
                        added += 1;
                    } else {
                        removed += 1;
                    }
                }
            }
            detail.push(format!("{paths} +{added} -{removed}"));
        }
        "search" => match view.get("shape").and_then(Value::as_str)? {
            "matches" => {
                let files = view.get("files").and_then(Value::as_array)?;
                let hits: usize = files
                    .iter()
                    .map(|file| {
                        file.get("matches")
                            .and_then(Value::as_array)
                            .map_or(0, Vec::len)
                    })
                    .sum();
                let scope = match files.as_slice() {
                    [file] => view_str(file, "path").unwrap_or("1 file").to_string(),
                    files => format!("{} files", files.len()),
                };
                detail.push(format!(
                    "{hits} {} in {scope}",
                    plural(hits, "match", "matches")
                ));
            }
            "paths" => {
                let paths: Vec<&str> = view
                    .get("paths")
                    .and_then(Value::as_array)?
                    .iter()
                    .filter_map(|path| path.as_str().filter(|path| !path.is_empty()))
                    .collect();
                detail.push(match paths.as_slice() {
                    [path] => (*path).to_string(),
                    paths => format!("{} paths", paths.len()),
                });
            }
            _ => return None,
        },
        "read" => {
            let path = view_str(view, "path")?;
            let lines = view.get("lines").and_then(Value::as_array)?;
            let total = view.get("totalLines").and_then(Value::as_i64).unwrap_or(0);
            let offset = view.get("offset").and_then(Value::as_i64).unwrap_or(1);
            detail.push(format!(
                "{path} — {} of {total} lines from {offset}",
                lines.len()
            ));
        }
        "web" => match view.get("kind").and_then(Value::as_str)? {
            "search" => {
                if let Some(answer) = view_str(view, "answer").and_then(fold_summary) {
                    detail.push(answer);
                }
                let sources = view.get("sources").and_then(Value::as_array)?;
                detail.push(format!(
                    "{} {}",
                    sources.len(),
                    plural(sources.len(), "source", "sources")
                ));
            }
            "fetch" => {
                let url = view_str(view, "url")?;
                let status = view.get("statusCode").and_then(Value::as_i64)?;
                failed = !(200..400).contains(&status);
                detail.push(format!("{status} {url}"));
            }
            _ => return None,
        },
        // A generic result view carries only a replacement title; without one it
        // says nothing the raw event does not already carry.
        "generic" => {
            view_str(view, "title")?;
        }
        _ => return None,
    }
    if view.get("truncated") == Some(&Value::Bool(true)) {
        detail.push("truncated by the harness".to_string());
    }
    let glyph = if failed {
        format!("{ANSI_RED}✖{ANSI_RESET}")
    } else {
        format!("{ANSI_GREEN}✔{ANSI_RESET}")
    };
    let mut row = format!("  {glyph}");
    if let Some(title) = view_str(view, "title") {
        row.push_str(&format!(" {title}"));
    }
    if !detail.is_empty() {
        row.push_str(&format!(" {ANSI_DIM}· {}{ANSI_RESET}", detail.join(" · ")));
    }
    let mut out = String::new();
    push_line(&mut out, "", &row);
    Some(out)
}

/// Render the host-computed render intent riding a `tool/call` or `tool/result`
/// delivery. dsh derives one `view` per delivery (never persisting it) and its
/// own web UI draws it as a card; the terminal gets the same information as
/// styled lines. `None` means the view was absent, addressed the other event
/// kind, or used a shape this renderer does not know — the caller then falls
/// back to the raw event, which is what dsh specifies for a UI without the
/// matching capability.
pub fn render_tool_event_view(payload: &Value, expected: &str) -> Option<String> {
    let envelope = payload.get("view")?;
    if envelope.get("for").and_then(Value::as_str)? != expected {
        return None;
    }
    let view = envelope.get("view")?;
    match expected {
        "call" => render_tool_call_view(view),
        "result" => render_tool_result_view(view),
        _ => None,
    }
}
