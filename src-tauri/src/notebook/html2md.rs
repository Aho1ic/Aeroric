//! 务实、健壮的 HTML → Markdown 转换器。
//!
//! 不引入新依赖。HTML 常含未闭合标签(`<br>`、`<img>`、`<li>`、`<p>`),
//! quick-xml 严格模式会失败,所以这里自带一个容错的轻量 tokenizer:
//! 扫描 `<...>` 与文本,区分 开标签 / 闭标签 / 自闭标签 / 注释 / doctype。
//!
//! 公开 API:[`html_to_markdown`]。
//!
//! 来源:移植自 Markio(`src-tauri/src/html2md.rs`),逐字未改。随手记有两处
//! 用到它 —— P0 把 localStorage 里 `richtext` 格式的 HTML 正文转成 Markdown,
//! 以及后续 P10 的网页剪藏。

/// 把网页 HTML 转成 Markdown。`readability=true` 时额外剥离导航/页脚/侧栏等非正文容器。
pub fn html_to_markdown(html: &str, readability: bool) -> String {
    let tokens = tokenize(html);
    let mut renderer = Renderer::new(readability);
    renderer.run(&tokens);
    renderer.finish()
}

// ===========================================================================
// Tokenizer
// ===========================================================================

#[derive(Debug, Clone)]
enum Token {
    /// 开标签：名字（小写）+ 属性
    Open {
        name: String,
        attrs: Vec<(String, String)>,
    },
    /// 闭标签：名字（小写）
    Close { name: String },
    /// 自闭标签 `<br/>`：名字（小写）+ 属性
    SelfClose {
        name: String,
        attrs: Vec<(String, String)>,
    },
    /// 文本（未解码实体；解码留给渲染阶段，pre 内部也要原样保留）
    Text(String),
}

/// 这些标签视为「自闭/空元素」，即使没写 `/`，也不期待闭合。
fn is_void_element(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// 内容需要原样保留（不解析其中的子标签）的「裸文本」元素。
/// script/style 等的内容直接丢弃，但要正确跳到对应闭标签。
fn is_rawtext_element(name: &str) -> bool {
    matches!(name, "script" | "style" | "textarea" | "title")
}

/// 把 style 值放进 HTML 属性时转义引号 —— 裸引号会把属性提前闭合。
fn attr_escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;")
}

fn tokenize(html: &str) -> Vec<Token> {
    let bytes = html.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut tokens: Vec<Token> = Vec::new();

    while i < len {
        if bytes[i] == b'<' {
            // 注释 <!-- ... -->
            if html[i..].starts_with("<!--") {
                if let Some(end) = find_sub(html, i + 4, "-->") {
                    i = end + 3;
                } else {
                    i = len; // 未闭合注释，吞到结尾
                }
                continue;
            }
            // doctype / 其它 <! ...> 声明：跳过
            if i + 1 < len && bytes[i + 1] == b'!' {
                if let Some(gt) = find_byte(bytes, i + 2, b'>') {
                    i = gt + 1;
                } else {
                    i = len;
                }
                continue;
            }
            // CDATA-ish / processing instruction <? ... >
            if i + 1 < len && bytes[i + 1] == b'?' {
                if let Some(gt) = find_byte(bytes, i + 2, b'>') {
                    i = gt + 1;
                } else {
                    i = len;
                }
                continue;
            }

            // 找到与之匹配的 '>'（容错：属性值里允许出现 '<'，但 '>' 视为标签结束）
            if let Some(gt) = find_byte(bytes, i + 1, b'>') {
                let inner = &html[i + 1..gt];
                if let Some(tok) = parse_tag(inner) {
                    // 裸文本元素：把内容整体作为 Text 抓取到对应闭标签
                    let rawtext_name = match &tok {
                        Token::Open { name, .. } if is_rawtext_element(name) => Some(name.clone()),
                        _ => None,
                    };
                    if let Some(name) = rawtext_name {
                        let closing = format!("</{}", name);
                        let content_start = gt + 1;
                        let (raw, next) = find_close_ci(html, content_start, &closing);
                        tokens.push(tok);
                        // title 的文字最终会被 skip 容器丢弃，
                        // 但为了 tokenizer 通用，这里仍记录文本，渲染阶段决定取舍。
                        if !raw.is_empty() {
                            tokens.push(Token::Text(raw.to_string()));
                        }
                        tokens.push(Token::Close { name });
                        i = next;
                        continue;
                    }
                    tokens.push(tok);
                    i = gt + 1;
                    continue;
                } else {
                    // 解析不出标签（比如孤立的 '<'），当作文本
                    tokens.push(Token::Text("<".to_string()));
                    i += 1;
                    continue;
                }
            } else {
                // 没有 '>'：剩下全是文本（半截 HTML）
                tokens.push(Token::Text(html[i..].to_string()));
                break;
            }
        } else {
            // 文本：吃到下一个 '<'
            let start = i;
            while i < len && bytes[i] != b'<' {
                i += 1;
            }
            tokens.push(Token::Text(html[start..i].to_string()));
        }
    }

    tokens
}

/// 在 `bytes[from..]` 中找到字节 `b` 的索引。
fn find_byte(bytes: &[u8], from: usize, b: u8) -> Option<usize> {
    bytes[from..].iter().position(|&c| c == b).map(|p| p + from)
}

/// 在 `s[from..]` 中找到子串 `needle` 的起始索引（字节）。
fn find_sub(s: &str, from: usize, needle: &str) -> Option<usize> {
    if from > s.len() {
        return None;
    }
    s[from..].find(needle).map(|p| p + from)
}

/// 大小写不敏感地寻找闭标签，返回 (内容切片, 闭标签之后的位置)。
/// `closing` 形如 "</script"（不含末尾 '>'）。
fn find_close_ci<'a>(s: &'a str, from: usize, closing: &str) -> (&'a str, usize) {
    let hay = s.as_bytes();
    let lower_closing = closing.to_ascii_lowercase();
    let cl = lower_closing.as_bytes();
    let mut j = from;
    while j + cl.len() <= hay.len() {
        let mut matched = true;
        for k in 0..cl.len() {
            if hay[j + k].to_ascii_lowercase() != cl[k] {
                matched = false;
                break;
            }
        }
        if matched {
            // 找到 "</name"，再吃到 '>'
            let content = &s[from..j];
            let after_name = j + cl.len();
            if let Some(gt) = find_byte(hay, after_name, b'>') {
                return (content, gt + 1);
            } else {
                return (content, s.len());
            }
        }
        j += 1;
    }
    // 没找到闭标签：内容吃到结尾
    (&s[from..], s.len())
}

/// 解析尖括号内部（不含 `<` `>`）为一个标签 Token。返回 None 表示不是合法标签。
fn parse_tag(inner: &str) -> Option<Token> {
    let inner_trimmed = inner.trim();
    if inner_trimmed.is_empty() {
        return None;
    }

    let bytes = inner_trimmed.as_bytes();
    let is_close = bytes[0] == b'/';
    let rest = if is_close {
        inner_trimmed[1..].trim_start()
    } else {
        inner_trimmed
    };

    // 末尾的 '/' 表示自闭
    let (rest, self_close_marker) = if let Some(stripped) = rest.strip_suffix('/') {
        (stripped.trim_end(), true)
    } else {
        (rest, false)
    };

    // 读取标签名
    let name_end = rest
        .find(|c: char| c.is_whitespace() || c == '/')
        .unwrap_or(rest.len());
    let name = rest[..name_end].to_ascii_lowercase();
    if name.is_empty() || !name.chars().next().unwrap().is_ascii_alphabetic() {
        // 标签名必须以字母开头，否则当作非标签
        return None;
    }

    if is_close {
        return Some(Token::Close { name });
    }

    let attrs = parse_attrs(&rest[name_end..]);

    if self_close_marker || is_void_element(&name) {
        Some(Token::SelfClose { name, attrs })
    } else {
        Some(Token::Open { name, attrs })
    }
}

/// 解析属性串：支持 双引号 / 单引号 / 无引号 / 布尔属性，大小写混合。
fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        // 跳过空白与多余的 '/'
        while i < n && (chars[i].is_whitespace() || chars[i] == '/') {
            i += 1;
        }
        if i >= n {
            break;
        }
        // 读取属性名
        let name_start = i;
        while i < n && !chars[i].is_whitespace() && chars[i] != '=' && chars[i] != '/' {
            i += 1;
        }
        if i == name_start {
            // 没推进，避免死循环
            i += 1;
            continue;
        }
        let name: String = chars[name_start..i]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase();

        // 跳过空白
        while i < n && chars[i].is_whitespace() {
            i += 1;
        }

        let mut value = String::new();
        if i < n && chars[i] == '=' {
            i += 1;
            while i < n && chars[i].is_whitespace() {
                i += 1;
            }
            if i < n && (chars[i] == '"' || chars[i] == '\'') {
                let quote = chars[i];
                i += 1;
                let v_start = i;
                while i < n && chars[i] != quote {
                    i += 1;
                }
                value = chars[v_start..i].iter().collect();
                if i < n {
                    i += 1; // 跳过结束引号
                }
            } else {
                // 无引号值
                let v_start = i;
                while i < n && !chars[i].is_whitespace() {
                    i += 1;
                }
                value = chars[v_start..i].iter().collect();
            }
        }

        attrs.push((name, value));
    }

    attrs
}

fn get_attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// 把 URL 安全地放进 markdown 链接/图片目标。含空格、括号或尖括号时用 `<...>` 包裹
/// （CommonMark 允许），并转义内部 `<` `>` `\`、去掉换行，避免 URL 里的 `)` 提前闭合
/// 链接造成内容/链接注入（如 `[x](http://a) javascript:...`）。
fn md_link_destination(url: &str) -> String {
    let url: String = url.chars().filter(|c| *c != '\n' && *c != '\r').collect();
    if url.is_empty() {
        return String::new();
    }
    let needs_wrap = url
        .chars()
        .any(|c| matches!(c, ' ' | '\t' | '(' | ')' | '<' | '>'));
    if needs_wrap {
        let esc = url
            .replace('\\', "\\\\")
            .replace('<', "\\<")
            .replace('>', "\\>");
        format!("<{esc}>")
    } else {
        url
    }
}

/// 转义会破坏 markdown 链接/图片文字的字符（`[` `]` 与反斜杠）。
fn md_bracket_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

// ===========================================================================
// Renderer
// ===========================================================================

/// 列表上下文（区分有序/无序，记录序号）。
#[derive(Clone)]
struct ListCtx {
    ordered: bool,
    index: usize,
}

/// 表格行构建中的单元缓存。
struct TableState {
    rows: Vec<Vec<String>>,
    header_rows: usize, // thead 中的行数（用作表头分隔线位置；至少 1）
    cur_row: Option<Vec<String>>,
    in_header_section: bool,
    saw_th_in_row: bool,
}

struct Renderer {
    readability: bool,
    /// 累积的输出（已成型的块之间用空行分隔，最后再清理）。
    out: String,
    /// 当前正在拼接的行内文本缓冲。
    line: String,
    /// 行前缀（列表缩进+标记、标题井号等不可折叠的前导文本）。
    line_prefix: String,
    /// 列表嵌套栈。
    list_stack: Vec<ListCtx>,
    /// blockquote 嵌套深度。
    quote_depth: usize,
    /// 当前处于「应跳过内容」的标签名栈（支持嵌套，按名字配对闭合）。
    skip_stack: Vec<String>,
    /// 当前是否处于 pre 内（保留原始空白）。
    pre_depth: usize,
    /// pre 内容缓冲。
    pre_buf: String,
    /// 表格栈（一般只一层，但保险用 Vec）。
    tables: Vec<TableState>,
    /// 当前在表格单元内时，行内文本写到单元缓冲而非 line。
    in_table_cell: bool,
    cell_buf: String,
    /// `<a>` 的 href 栈（Close 标签无属性，需在 Open 时压入）。
    a_href: Vec<String>,
    /// 每个 `<span>` 开标签是否被保留(带颜色 style 的才留)。闭标签据此决定
    /// 要不要补 `</span>` —— 不配对会产出孤立闭标签。
    span_kept: Vec<bool>,
}

impl Renderer {
    fn new(readability: bool) -> Self {
        Renderer {
            readability,
            out: String::new(),
            line: String::new(),
            line_prefix: String::new(),
            list_stack: Vec::new(),
            quote_depth: 0,
            skip_stack: Vec::new(),
            pre_depth: 0,
            pre_buf: String::new(),
            tables: Vec::new(),
            in_table_cell: false,
            cell_buf: String::new(),
            a_href: Vec::new(),
            span_kept: Vec::new(),
        }
    }

    fn run(&mut self, tokens: &[Token]) {
        // 维护一个开标签栈，用来正确处理跳过容器的闭合与块级元素隐式闭合。
        // 这里采用「事件驱动」的简单模型：每个标签独立处理，
        // 并用 skip_depth/pre_depth/list_stack 等状态机管理上下文。
        for tok in tokens {
            match tok {
                Token::Text(t) => self.on_text(t),
                Token::Open { name, attrs } => self.on_open(name, attrs),
                Token::SelfClose { name, attrs } => {
                    self.on_open(name, attrs);
                    self.on_close(name);
                }
                Token::Close { name } => self.on_close(name),
            }
        }
    }

    // ---- 块管理 ----

    /// 把当前行内缓冲作为一个块刷出。
    fn flush_line(&mut self) {
        let prefix = std::mem::take(&mut self.line_prefix);
        let raw = std::mem::take(&mut self.line);
        // line 内的 '\n' 只来自 <br>，作为硬换行处理：逐段折叠后用 "  \n" 连接。
        let mut segments: Vec<String> = raw
            .split('\n')
            .map(|seg| fold_spaces(seg).trim().to_string())
            .collect();
        // 去掉首尾空段（开头/结尾的 <br>）
        while segments.first().map(|s| s.is_empty()).unwrap_or(false) {
            segments.remove(0);
        }
        while segments.last().map(|s| s.is_empty()).unwrap_or(false) {
            segments.pop();
        }
        let body = segments.join("  \n");
        self.line.clear();
        self.line_prefix.clear();
        if body.is_empty() && prefix.trim().is_empty() {
            return;
        }
        let block = if prefix.is_empty() {
            body
        } else {
            format!("{}{}", prefix, body)
        };
        self.push_block(&block);
    }

    /// 推一个已成型的块到输出。会按当前 blockquote 深度给每行加 `> ` 前缀。
    fn push_block(&mut self, block: &str) {
        if block.is_empty() {
            return;
        }
        let prefixed = if self.quote_depth > 0 {
            let prefix = self.quote_prefix();
            block
                .lines()
                .map(|l| {
                    if l.is_empty() {
                        prefix.trim_end().to_string()
                    } else {
                        format!("{}{}", prefix, l)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            block.to_string()
        };
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            if self.out.ends_with('\n') {
                self.out.push('\n');
            } else {
                self.out.push_str("\n\n");
            }
        }
        self.out.push_str(&prefixed);
    }

    /// 当前列表缩进前缀。
    fn list_indent(&self) -> String {
        if self.list_stack.is_empty() {
            String::new()
        } else {
            "  ".repeat(self.list_stack.len() - 1)
        }
    }

    /// 引用前缀。
    fn quote_prefix(&self) -> String {
        "> ".repeat(self.quote_depth)
    }

    fn append_inline(&mut self, s: &str) {
        if !self.skip_stack.is_empty() {
            return;
        }
        if self.pre_depth > 0 {
            self.pre_buf.push_str(s);
            return;
        }
        if self.in_table_cell {
            self.cell_buf.push_str(s);
            return;
        }
        self.line.push_str(s);
    }

    // ---- 文本 ----

    fn on_text(&mut self, raw: &str) {
        if !self.skip_stack.is_empty() {
            return;
        }
        if self.pre_depth > 0 {
            // pre：保留原样（仅解码实体），不折叠
            self.pre_buf.push_str(&decode_entities(raw));
            return;
        }
        // 普通文本：解码实体；空白折叠交给 flush 阶段。
        let decoded = decode_entities(raw);
        // 行内拼接，连续空白先压成单空格以免缓冲爆炸（最终还会再折叠一次）。
        let mut piece = String::with_capacity(decoded.len());
        let mut last_ws = piece_ends_with_space(self.current_inline_target());
        for ch in decoded.chars() {
            if ch.is_ascii_whitespace() {
                if !last_ws {
                    piece.push(' ');
                    last_ws = true;
                }
            } else {
                piece.push(ch);
                last_ws = false;
            }
        }
        if !piece.is_empty() {
            self.append_inline(&piece);
        }
    }

    fn current_inline_target(&self) -> &str {
        if self.in_table_cell {
            &self.cell_buf
        } else {
            &self.line
        }
    }

    // ---- 开标签 ----

    /// 判断一个标签是否应触发「跳过其内容」。
    fn is_skip_trigger(&self, name: &str, attrs: &[(String, String)]) -> bool {
        if matches!(
            name,
            "script" | "style" | "head" | "title" | "noscript" | "template" | "svg"
        ) {
            return true;
        }
        // 随手记新增:富文本编辑器往每个代码块里塞了一个语言选择下拉
        // (`<select><option>Text</option><option>SQL</option>…</select>`)。
        // 不整段丢掉的话,所有选项的文字会被当成代码内容拼进正文。
        if matches!(name, "select") {
            return true;
        }
        if self.readability {
            let role_nav = get_attr(attrs, "role")
                .map(|r| r.eq_ignore_ascii_case("navigation"))
                .unwrap_or(false);
            if matches!(name, "nav" | "footer" | "aside" | "form") || role_nav {
                return true;
            }
        }
        false
    }

    fn on_open(&mut self, name: &str, attrs: &[(String, String)]) {
        // 跳过触发标签：压入跳过栈（即便已在跳过区，也要压入以保证配对闭合）。
        if self.is_skip_trigger(name, attrs) {
            self.skip_stack.push(name.to_string());
            return;
        }
        // 已在跳过区：忽略一切非跳过触发标签。
        if !self.skip_stack.is_empty() {
            return;
        }

        match name {
            "br" => {
                if self.pre_depth > 0 {
                    self.pre_buf.push('\n');
                } else {
                    // 行内换行：插入 '\n'，flush 时转为 Markdown 硬换行 "  \n"
                    self.append_inline("\n");
                }
            }
            "hr" => {
                self.flush_line();
                self.push_block("---");
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush_line();
                let level = (name.as_bytes()[1] - b'0') as usize;
                self.line_prefix = format!("{} ", "#".repeat(level));
            }
            "p" | "div" | "section" | "article" | "main" | "header" => {
                // 块级：先刷出上一段
                self.flush_line();
            }
            "blockquote" => {
                self.flush_line();
                self.quote_depth += 1;
            }
            "ul" => {
                self.flush_line();
                self.list_stack.push(ListCtx {
                    ordered: false,
                    index: 0,
                });
            }
            "ol" => {
                self.flush_line();
                let start = get_attr(attrs, "start")
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .unwrap_or(1);
                self.list_stack.push(ListCtx {
                    ordered: true,
                    index: start.saturating_sub(1),
                });
            }
            "li" => {
                self.flush_line();
                let indent = self.list_indent();
                let marker = if let Some(ctx) = self.list_stack.last_mut() {
                    if ctx.ordered {
                        ctx.index += 1;
                        format!("{}. ", ctx.index)
                    } else {
                        "- ".to_string()
                    }
                } else {
                    "- ".to_string()
                };
                self.line_prefix = format!("{}{}", indent, marker);
            }
            "pre" => {
                self.flush_line();
                self.pre_depth += 1;
                self.pre_buf.clear();
            }
            "code" => {
                if self.pre_depth > 0 {
                    // pre>code：不加行内反引号，内容已由 pre 处理
                } else {
                    self.append_inline("`");
                }
            }
            "strong" | "b" => self.append_inline("**"),
            "em" | "i" => self.append_inline("*"),
            "del" | "s" | "strike" => self.append_inline("~~"),
            // ── 随手记新增:保留 markdown 没有对应语法的行内格式 ──────────────
            //
            // Markio 原版把 <u> / <mark> / 带颜色的 <span> 当透明容器,只留文字
            // —— 因为 markdown 没有下划线、高亮、颜色的语法。
            //
            // 但 markdown **允许内联 HTML**,而随手记的渲染管线已经过 DOMPurify
            // 放行这些标签。原样保留能让「富文本 → Markdown」对它们无损,而不是
            // 让用户迁移后发现标注全没了。
            "u" | "mark" => self.append_inline(&format!("<{name}>")),
            "span" => {
                // 只保留带颜色 style 的 span。裸 span 是 contentEditable 的结构
                // 噪声,留着只会让正文里全是空标签。
                let keep = get_attr(attrs, "style")
                    .map(decode_entities)
                    .filter(|style| style.contains("color"))
                    .map(|style| {
                        self.append_inline(&format!("<span style=\"{}\">", attr_escape(&style)));
                    })
                    .is_some();
                self.span_kept.push(keep);
            }
            "a" => {
                // 链接文字后面在 close 处补 (href)；这里压栈 href。
                let href = get_attr(attrs, "href")
                    .map(decode_entities)
                    .unwrap_or_default();
                self.a_href.push(href);
                self.append_inline("[");
            }
            "img" => {
                let alt = get_attr(attrs, "alt").unwrap_or("");
                let src = get_attr(attrs, "src").unwrap_or("");
                if !src.is_empty() {
                    let piece = format!(
                        "![{}]({})",
                        md_bracket_escape(&decode_entities(alt)),
                        md_link_destination(&decode_entities(src))
                    );
                    self.append_inline(&piece);
                }
            }
            "table" => {
                self.flush_line();
                self.tables.push(TableState {
                    rows: Vec::new(),
                    header_rows: 0,
                    cur_row: None,
                    in_header_section: false,
                    saw_th_in_row: false,
                });
            }
            "thead" => {
                if let Some(t) = self.tables.last_mut() {
                    t.in_header_section = true;
                }
            }
            "tbody" | "tfoot" => {
                if let Some(t) = self.tables.last_mut() {
                    t.in_header_section = false;
                }
            }
            "tr" => {
                if let Some(t) = self.tables.last_mut() {
                    t.cur_row = Some(Vec::new());
                    t.saw_th_in_row = false;
                }
            }
            "th" | "td" if !self.tables.is_empty() => {
                self.in_table_cell = true;
                self.cell_buf.clear();
                if name == "th" {
                    if let Some(t) = self.tables.last_mut() {
                        t.saw_th_in_row = true;
                    }
                }
            }
            _ => {
                // 其它未知标签：当作透明容器，不影响行内拼接。
            }
        }
    }

    // ---- 闭标签 ----

    fn on_close(&mut self, name: &str) {
        // 跳过栈：若闭标签匹配栈顶，弹出（结束该跳过容器）。
        // 这同时覆盖 role="navigation" 的任意容器（如 div），因为压栈时记的是真实标签名。
        if let Some(top) = self.skip_stack.last() {
            if top == name {
                self.skip_stack.pop();
            }
            // 处于跳过区内的其它闭标签一律忽略。
            return;
        }

        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush_line();
            }
            "p" | "div" | "section" | "article" | "main" | "header" => {
                self.flush_line();
            }
            "blockquote" => {
                self.flush_line();
                if self.quote_depth > 0 {
                    self.quote_depth -= 1;
                }
            }
            "ul" | "ol" => {
                self.flush_line();
                self.list_stack.pop();
            }
            "li" => {
                self.flush_line();
            }
            "pre" => {
                self.pre_depth = self.pre_depth.saturating_sub(1);
                let code = std::mem::take(&mut self.pre_buf);
                let code = strip_one_trailing_newline(&code);
                let block = format!("```\n{}\n```", code);
                self.push_block(&block);
            }
            "code" => {
                if self.pre_depth > 0 {
                    // 由 pre 处理
                } else {
                    self.append_inline("`");
                }
            }
            "strong" | "b" => self.append_inline("**"),
            "em" | "i" => self.append_inline("*"),
            "del" | "s" | "strike" => self.append_inline("~~"),
            "u" | "mark" => self.append_inline(&format!("</{name}>")),
            "span" => {
                // 只在开标签时留过的 span 才补闭标签,否则会出现孤立的 </span>。
                if self.span_kept.pop().unwrap_or(false) {
                    self.append_inline("</span>");
                }
            }
            "a" => {
                // Close 标签没有属性，href 在 on_open 时压入 a_href_stack，这里弹出拼成 [text](href)
                if let Some(href) = self.a_href_stack_pop() {
                    let piece = format!("]({})", md_link_destination(&href));
                    self.append_inline(&piece);
                } else {
                    self.append_inline("]");
                }
            }
            "table" => {
                self.flush_table();
            }
            "thead" => {
                if let Some(t) = self.tables.last_mut() {
                    t.in_header_section = false;
                }
            }
            "tr" => {
                if let Some(t) = self.tables.last_mut() {
                    if let Some(row) = t.cur_row.take() {
                        if t.in_header_section || t.saw_th_in_row {
                            // 记为表头行
                            if t.header_rows == 0 {
                                t.header_rows = 1;
                            }
                            // 表头行也存进 rows 顶部顺序
                        }
                        t.rows.push(row);
                    }
                }
            }
            "th" | "td" if self.in_table_cell => {
                let cell = collapse_ws(&self.cell_buf);
                self.cell_buf.clear();
                self.in_table_cell = false;
                if let Some(t) = self.tables.last_mut() {
                    if let Some(row) = t.cur_row.as_mut() {
                        row.push(cell);
                    }
                }
            }
            _ => {}
        }
    }

    // a href 缓存：因为 Close 无属性，用一个栈在 open 处压入。
    // 为了不在结构体里再加字段后又改这么多处，这里用 line 内联方式不可行，
    // 故在结构体补一个字段。见下方 impl 扩展。
    fn a_href_stack_pop(&mut self) -> Option<String> {
        self.a_href.pop()
    }

    fn flush_table(&mut self) {
        let t = match self.tables.pop() {
            Some(t) => t,
            None => return,
        };
        if t.rows.is_empty() {
            return;
        }
        // 列数取各行最大值
        let cols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if cols == 0 {
            return;
        }

        let fmt_row = |row: &Vec<String>| -> String {
            let mut cells: Vec<String> = row.iter().map(|c| escape_pipe(c.trim())).collect();
            while cells.len() < cols {
                cells.push(String::new());
            }
            format!("| {} |", cells.join(" | "))
        };

        let mut lines = Vec::new();
        // 第一行作为表头（GFM 要求表头 + 分隔线）
        lines.push(fmt_row(&t.rows[0]));
        let sep: Vec<String> = (0..cols).map(|_| "---".to_string()).collect();
        lines.push(format!("| {} |", sep.join(" | ")));
        for row in t.rows.iter().skip(1) {
            lines.push(fmt_row(row));
        }
        let block = lines.join("\n");
        self.push_block(&block);
    }

    fn finish(mut self) -> String {
        self.flush_line();
        // 规整：把 3+ 连续换行压成 2 个，并加引用/列表前缀已在内联处理。
        let normalized = normalize_blank_lines(&self.out);
        normalized.trim().to_string()
    }
}

// ---- 行内/块辅助函数 ----

fn piece_ends_with_space(s: &str) -> bool {
    s.ends_with(' ') || s.ends_with('\n') || s.is_empty()
}

/// 折叠连续空白为单空格，并 trim 两端。保留已有的 Markdown 硬换行 "  \n"。
fn collapse_ws(s: &str) -> String {
    // 先按硬换行切，分段折叠，避免把 "  \n" 折没。
    let mut result = String::new();
    let mut first = true;
    for segment in s.split('\n') {
        let folded = fold_spaces(segment);
        if first {
            result.push_str(folded.trim_end());
            first = false;
        } else {
            result.push('\n');
            result.push_str(folded.trim_end());
        }
    }
    result.trim().to_string()
}

fn fold_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_ws = false;
    for ch in s.chars() {
        // 仅折叠 ASCII 空白；NBSP( ) 等不可断空格保留原样。
        if ch.is_ascii_whitespace() {
            if !last_ws {
                out.push(' ');
                last_ws = true;
            }
        } else {
            out.push(ch);
            last_ws = false;
        }
    }
    out
}

fn strip_one_trailing_newline(s: &str) -> String {
    let s = s.strip_prefix('\n').unwrap_or(s);
    let s = s.strip_suffix('\n').unwrap_or(s);
    s.to_string()
}

fn escape_pipe(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// 把 3+ 连续空行压成最多一个空行。
fn normalize_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut newline_run = 0;
    for ch in s.chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                out.push('\n');
            }
        } else {
            newline_run = 0;
            out.push(ch);
        }
    }
    out
}

// ===========================================================================
// 实体解码
// ===========================================================================

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let n = bytes.len();
    while i < n {
        if bytes[i] == b'&' {
            // 找到 ';'，限制在合理长度内（<= 32）
            let mut semi = None;
            let limit = (i + 32).min(n);
            let mut j = i + 1;
            while j < limit {
                if bytes[j] == b';' {
                    semi = Some(j);
                    break;
                }
                // 实体里只允许字母数字和 '#'
                let c = bytes[j];
                if !(c.is_ascii_alphanumeric() || c == b'#') {
                    break;
                }
                j += 1;
            }
            if let Some(semi_idx) = semi {
                let entity = &s[i + 1..semi_idx];
                if let Some(decoded) = decode_one_entity(entity) {
                    out.push_str(&decoded);
                    i = semi_idx + 1;
                    continue;
                }
            }
            // 不是已知实体：原样保留 '&'
            out.push('&');
            i += 1;
        } else {
            // 推进一个完整 UTF-8 字符
            let ch_len = utf8_len(bytes[i]);
            let end = (i + ch_len).min(n);
            out.push_str(&s[i..end]);
            i = end;
        }
    }
    out
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

fn decode_one_entity(entity: &str) -> Option<String> {
    // 数字实体 &#NN; / &#xNN;
    if let Some(rest) = entity.strip_prefix('#') {
        let code = if let Some(hex) = rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            rest.parse::<u32>().ok()?
        };
        return char::from_u32(code).map(|c| c.to_string());
    }
    // 命名实体
    let mapped = match entity {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => "\u{00A0}",
        "mdash" => "\u{2014}",
        "ndash" => "\u{2013}",
        "hellip" => "\u{2026}",
        "copy" => "\u{00A9}",
        "reg" => "\u{00AE}",
        "trade" => "\u{2122}",
        "ldquo" => "\u{201C}",
        "rdquo" => "\u{201D}",
        "lsquo" => "\u{2018}",
        "rsquo" => "\u{2019}",
        "middot" => "\u{00B7}",
        "bull" => "\u{2022}",
        "deg" => "\u{00B0}",
        "times" => "\u{00D7}",
        "divide" => "\u{00F7}",
        "laquo" => "\u{00AB}",
        "raquo" => "\u{00BB}",
        "euro" => "\u{20AC}",
        "pound" => "\u{00A3}",
        "cent" => "\u{00A2}",
        "yen" => "\u{00A5}",
        "sect" => "\u{00A7}",
        "para" => "\u{00B6}",
        _ => return None,
    };
    Some(mapped.to_string())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn md(html: &str) -> String {
        html_to_markdown(html, false)
    }

    #[test]
    fn test_headings() {
        assert_eq!(md("<h1>Title</h1>"), "# Title");
        assert_eq!(md("<h3>Sub</h3>"), "### Sub");
        let out = md("<h1>A</h1><h2>B</h2>");
        assert_eq!(out, "# A\n\n## B");
    }

    #[test]
    fn test_paragraphs_and_blank_line() {
        let out = md("<p>Hello world</p><p>Second para</p>");
        assert_eq!(out, "Hello world\n\nSecond para");
    }

    #[test]
    fn test_bold_italic() {
        assert_eq!(md("<p><strong>bold</strong></p>"), "**bold**");
        assert_eq!(md("<p><b>bold</b></p>"), "**bold**");
        assert_eq!(md("<p><em>it</em></p>"), "*it*");
        assert_eq!(md("<p><i>it</i></p>"), "*it*");
        assert_eq!(md("<p><del>x</del></p>"), "~~x~~");
    }

    #[test]
    fn test_link() {
        assert_eq!(
            md(r#"<p><a href="https://example.com">click</a></p>"#),
            "[click](https://example.com)"
        );
        // 无引号属性
        assert_eq!(md("<a href=https://e.com>e</a>"), "[e](https://e.com)");
        // 单引号
        assert_eq!(md("<a href='https://e.com'>e</a>"), "[e](https://e.com)");
    }

    #[test]
    fn test_link_destination_injection_guarded() {
        // href 含 ) / 空格：用 <...> 包裹，避免 ) 提前闭合链接造成内容/链接注入。
        assert_eq!(
            md(r#"<a href="https://e.com/a)b c">x</a>"#),
            "[x](<https://e.com/a)b c>)"
        );
        // 目标包裹时内部尖括号会被转义（直接验证 helper，避开 tokenizer 对裸 < 的处理）。
        assert_eq!(
            md_link_destination("http://e.com/a)>b"),
            "<http://e.com/a)\\>b>"
        );
        // 普通 URL 不受影响，保持裸输出。
        assert_eq!(md_link_destination("https://e.com/x"), "https://e.com/x");
    }

    #[test]
    fn test_image() {
        assert_eq!(
            md(r#"<img src="a.png" alt="An image">"#),
            "![An image](a.png)"
        );
        // 无 alt
        assert_eq!(md(r#"<img src="b.jpg">"#), "![](b.jpg)");
        // alt 含 [] 转义、src 含空格用 <...> 包裹
        assert_eq!(
            md(r#"<img src="a b.png" alt="x[y]">"#),
            "![x\\[y\\]](<a b.png>)"
        );
    }

    #[test]
    fn test_unordered_list() {
        let out = md("<ul><li>one</li><li>two</li></ul>");
        assert_eq!(out, "- one\n\n- two");
    }

    #[test]
    fn test_ordered_list() {
        let out = md("<ol><li>a</li><li>b</li><li>c</li></ol>");
        assert_eq!(out, "1. a\n\n2. b\n\n3. c");
    }

    #[test]
    fn test_nested_list() {
        let out = md("<ul><li>a<ul><li>b</li></ul></li></ul>");
        assert!(out.contains("- a"), "got: {out}");
        assert!(out.contains("  - b"), "expected 2-space indent, got: {out}");
    }

    #[test]
    fn test_code_block() {
        let out = md("<pre><code>let x = 1;\nlet y = 2;</code></pre>");
        assert_eq!(out, "```\nlet x = 1;\nlet y = 2;\n```");
    }

    #[test]
    fn test_pre_preserves_whitespace() {
        let out = md("<pre>  indented\n    more</pre>");
        assert_eq!(out, "```\n  indented\n    more\n```");
    }

    #[test]
    fn test_inline_code() {
        assert_eq!(md("<p>use <code>foo()</code> here</p>"), "use `foo()` here");
    }

    #[test]
    fn test_blockquote() {
        let out = md("<blockquote><p>quoted text</p></blockquote>");
        assert_eq!(out, "> quoted text");
    }

    #[test]
    fn test_blockquote_multiparagraph() {
        let out = md("<blockquote><p>line one</p><p>line two</p></blockquote>");
        assert_eq!(out, "> line one\n\n> line two");
    }

    #[test]
    fn test_hr() {
        let out = md("<p>a</p><hr><p>b</p>");
        assert_eq!(out, "a\n\n---\n\nb");
    }

    #[test]
    fn test_br() {
        let out = md("<p>line1<br>line2</p>");
        assert_eq!(out, "line1  \nline2");
    }

    #[test]
    fn test_entities() {
        assert_eq!(
            md("<p>a &amp; b &lt; c &gt; d &quot;e&quot; &#39;f&#39;</p>"),
            r#"a & b < c > d "e" 'f'"#
        );
        assert_eq!(md("<p>x&nbsp;y</p>"), "x\u{00A0}y");
        assert_eq!(
            md("<p>&mdash;&ndash;&hellip;</p>"),
            "\u{2014}\u{2013}\u{2026}"
        );
        assert_eq!(md("<p>&#65;&#x42;</p>"), "AB");
    }

    #[test]
    fn test_readability_strips_nav_and_script() {
        let html = r#"
            <nav><a href="/">home</a></nav>
            <script>alert('x')</script>
            <main><p>Real content</p></main>
            <footer>copyright</footer>
        "#;
        let out = html_to_markdown(html, true);
        assert_eq!(out, "Real content");
    }

    #[test]
    fn test_script_style_always_skipped() {
        let html = "<style>.a{color:red}</style><p>visible</p><script>var a=1;</script>";
        assert_eq!(md(html), "visible");
    }

    #[test]
    fn test_role_navigation_stripped() {
        let html = r#"<div role="navigation"><a href="/">x</a></div><p>body</p>"#;
        let out = html_to_markdown(html, true);
        assert_eq!(out, "body");
    }

    #[test]
    fn test_whitespace_collapse() {
        let out = md("<p>too    many\n\n  spaces   here</p>");
        assert_eq!(out, "too many spaces here");
    }

    #[test]
    fn test_truncated_html_no_panic() {
        // 各种半截 / 畸形输入：只要求不 panic。
        let inputs = [
            "<p>unclosed paragraph",
            "<a href=\"http://x.com\">no close",
            "<div><span>nested <b>bold",
            "<!-- comment never closed",
            "<",
            "<<<>>>",
            "<img src=",
            "plain text only",
            "<ul><li>item",
            "<pre><code>code without close",
            "<table><tr><td>cell",
        ];
        for inp in inputs {
            let _ = html_to_markdown(inp, false);
            let _ = html_to_markdown(inp, true);
        }
    }

    #[test]
    fn test_table() {
        let html = "<table><thead><tr><th>A</th><th>B</th></tr></thead>\
            <tbody><tr><td>1</td><td>2</td></tr><tr><td>3</td><td>4</td></tr></tbody></table>";
        let out = md(html);
        let expected = "| A | B |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |";
        assert_eq!(out, expected);
    }

    #[test]
    fn test_mixed_case_tags() {
        assert_eq!(md("<P><STRONG>Hi</STRONG></P>"), "**Hi**");
    }

    #[test]
    fn test_doctype_and_head_skipped() {
        let html = "<!DOCTYPE html><html><head><title>T</title></head>\
            <body><p>Body</p></body></html>";
        assert_eq!(md(html), "Body");
    }
}
