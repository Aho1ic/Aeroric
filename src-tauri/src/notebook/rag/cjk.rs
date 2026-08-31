//! FTS5 的中日韩分词垫片。
//!
//! SQLite 的 `unicode61` 分词器把连续的中日韩字符当作**一个** token —— 整句
//! 「随手记的导出功能已经完成」进 FTS5 索引后是一个词条,于是 `MATCH '导出'`
//! 恒零命中,`MATCH '随手记'` 也一样,连前缀 `导出*` 都查不到。英文却完全正常,
//! 所以这个故障在只有英文测试数据时全绿。
//!
//! `trigram` 分词器不是解药:它按三字符滑窗建索引,三字以上能命中,**两字查询
//! 恒零** —— 而「导出」「完成」「同步」这种两字词在中文里正是最常见的查询长度。
//!
//! 这里的做法是入库前把 CJK 字符逐字用空格隔开,查询侧做同样的变换再包成短语:
//!
//! ```text
//! 正文  「随手记的导出功能」   → 「随 手 记 的 导 出 功 能」
//! 查询  「导出」               → 「"导 出"」
//! ```
//!
//! 逐字之后 `unicode61` 把每个汉字切成独立 token,而**短语**查询要求它们相邻且
//! 有序 —— 这才等价于原来的子串匹配。不包引号会退化成 OR(任一字命中就算),
//! 「导出」会把所有含「导」或「出」的笔记全捞回来。
//!
//! 非 CJK 连续段原样保留,不插空格:`PDF` 拆成 `P D F` 会让它匹配到任何含这三个
//! 字母的地方,而英文本来就有词边界,`unicode61` 处理得很好。
//!
//! 连带影响:索引里存的是切分后的文本,所以 FTS5 的 `snippet()` 返回的是
//! 「随 手 记 的 [导 出]」这种带空格的东西。检索结果的高亮因此不走 `snippet()`,
//! 改在 Rust 侧按原文字节偏移自己算(和 Aeroric 已有的搜索一致)。

/// 判断一个字符是否属于「没有词边界、需要逐字切开」的区段。
///
/// 收的是汉字(含扩展 A 与兼容表意文字)、日文假名、韩文谚文,以及 CJK 标点。
/// 标点也算:「导出、完成」里的顿号若不切开,会和两侧的字粘成一个 token。
pub fn is_cjk(ch: char) -> bool {
    let c = ch as u32;
    // CJK 统一表意文字 + 扩展 A + 兼容表意文字
    (0x4e00..=0x9fff).contains(&c)
        || (0x3400..=0x4dbf).contains(&c)
        || (0xf900..=0xfaff).contains(&c)
        // 平假名 + 片假名(含半角片假名)
        || (0x3040..=0x30ff).contains(&c)
        || (0xff66..=0xff9f).contains(&c)
        // 韩文谚文音节 + 字母
        || (0xac00..=0xd7af).contains(&c)
        || (0x1100..=0x11ff).contains(&c)
        // CJK 标点(、。「」《》等)与全角形式
        || (0x3000..=0x303f).contains(&c)
        || (0xff01..=0xff65).contains(&c)
}

/// 把文本里的 CJK 字符逐字用空格隔开,非 CJK 段原样保留。
///
/// 入库(`chunks_fts` 的可检索列)和构造查询都走这一个函数 —— 两侧用不同的变换
/// 会让索引和查询对不上,而那种错误的表现是「永远查不到」,不报错。
pub fn segment(text: &str) -> String {
    // 最坏情况(全 CJK)每个字后面加一个空格。
    let mut out = String::with_capacity(text.len() + text.len() / 2);
    let mut prev_cjk = false;
    for ch in text.chars() {
        let cjk = is_cjk(ch);
        // 只在「CJK 紧挨着别的东西」的边界补空格,而且两侧都还没有空白时才补。
        //
        // 两个空白判断都是必要的:`out` 尾部已是空白时补会撑出连续空格;当前
        // 字符本身是空白时补也一样 —— 而这个函数必须幂等,因为入库前和查询前
        // 各调一次,重复施加要是会变长,索引里的文本和查询表达式就对不上了。
        let needs_space = (cjk || prev_cjk)
            && !out.is_empty()
            && !ch.is_whitespace()
            && !out.ends_with(char::is_whitespace);
        if needs_space {
            out.push(' ');
        }
        out.push(ch);
        prev_cjk = cjk;
    }
    out
}

/// 把用户输入变成 FTS5 的 MATCH 表达式。
///
/// 返回 `None` 表示这个查询没有可检索的内容(空串、纯标点),调用方应当跳过
/// 关键词检索那一路而不是拿空串去 MATCH —— FTS5 对空表达式报语法错误。
///
/// 用户输入按空白拆成若干词,每个词各自变换后包成短语,词之间是 AND(FTS5 的
/// 默认连接)。所以「导出 PDF」要求两者都出现,与用户对搜索框的预期一致。
pub fn match_expression(query: &str) -> Option<String> {
    let mut phrases: Vec<String> = Vec::new();
    for word in query.split_whitespace() {
        let segmented = segment(word);
        // FTS5 的短语里只有双引号需要转义(写成两个双引号)。其余字符在引号内
        // 都是字面量,不会被解读成运算符 —— 这是不做通用转义的原因。
        let escaped = segmented.replace('"', "\"\"");
        // 变换后只剩标点或空白的词没有可匹配的 token(`"，"` 会让 FTS5 报错),
        // 跳过而不是让整个查询失败。
        if !escaped.chars().any(|c| c.is_alphanumeric()) {
            continue;
        }
        phrases.push(format!("\"{escaped}\""));
    }
    if phrases.is_empty() {
        return None;
    }
    Some(phrases.join(" AND "))
}

/// 建表时用的分词器配置。
///
/// `remove_diacritics 0` 保留变音符号 —— 去掉它会让 `café` 和 `cafe` 混为一谈,
/// 而随手记里存的是原文,用户搜什么就该匹配什么。
///
/// 不加 `tokenchars`:CJK 已在入库前切开,而给 `unicode61` 加中文 tokenchars 反而
/// 会把逐字切分的成果粘回去。
pub const FTS_TOKENIZER: &str = "unicode61 remove_diacritics 0";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_cjk_run_into_single_chars() {
        assert_eq!(segment("随手记"), "随 手 记");
    }

    #[test]
    fn keeps_ascii_words_intact() {
        // 英文有词边界,拆开会让 PDF 匹配到任何含 P、D、F 的地方。
        assert_eq!(segment("export PDF"), "export PDF");
    }

    #[test]
    fn separates_cjk_from_adjacent_ascii() {
        assert_eq!(segment("导出PDF文件"), "导 出 PDF 文 件");
    }

    #[test]
    fn does_not_double_existing_spaces() {
        // 撑出连续空格不影响检索,但这份文本要落库,长度虚高没有意义。
        assert_eq!(segment("导出 PDF"), "导 出 PDF");
        assert!(!segment("完成 了").contains("  "));
    }

    #[test]
    fn treats_cjk_punctuation_as_a_boundary() {
        // 顿号不切开的话会和两侧的字粘成一个 token。
        assert_eq!(segment("导出、完成"), "导 出 、 完 成");
    }

    #[test]
    fn segment_is_idempotent() {
        // 入库与查询各调一次,重复施加不能改变结果 —— 否则两侧会对不上。
        let once = segment("随手记的导出功能");
        assert_eq!(segment(&once), once);
    }

    #[test]
    fn preserves_every_original_char() {
        // 只允许插入空格。丢字会让索引与原文的偏移对不上。
        let source = "随手记 export PDF、完成";
        let stripped: String = segment(source).chars().filter(|c| *c != ' ').collect();
        let expected: String = source.chars().filter(|c| *c != ' ').collect();
        assert_eq!(stripped, expected);
    }

    #[test]
    fn wraps_query_words_as_phrases() {
        // 不包引号会退化成 OR,「导出」会捞回所有含「导」或「出」的笔记。
        assert_eq!(match_expression("导出").as_deref(), Some("\"导 出\""));
    }

    #[test]
    fn joins_multiple_words_with_and() {
        assert_eq!(
            match_expression("导出 PDF").as_deref(),
            Some("\"导 出\" AND \"PDF\"")
        );
    }

    #[test]
    fn rejects_queries_without_searchable_content() {
        assert_eq!(match_expression(""), None);
        assert_eq!(match_expression("   "), None);
        // 纯标点在 FTS5 里没有对应 token,拿去 MATCH 会报语法错。
        assert_eq!(match_expression("、。"), None);
        assert_eq!(match_expression("!!!"), None);
    }

    #[test]
    fn skips_punctuation_only_words_but_keeps_the_rest() {
        assert_eq!(match_expression("导出 、").as_deref(), Some("\"导 出\""));
    }

    #[test]
    fn escapes_double_quotes_in_query() {
        // 双引号是 FTS5 短语的定界符,不转义会让表达式提前闭合并报语法错。
        assert_eq!(
            match_expression("say\"hi").as_deref(),
            Some("\"say\"\"hi\"")
        );
    }
}
