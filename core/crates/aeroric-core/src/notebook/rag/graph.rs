//! 笔记引用图:把 `links` 表里的**字面目标**解析成 doc_id,再据此取邻居。
//!
//! 为什么解析要单独一层:`links.target` 存的是方括号里的原文(`计划`、
//! `sub/foo`、`设计#架构` 的前半段),它不是路径。同一篇笔记可以被三种写法指到 ——
//! 文件名 stem、frontmatter 标题、路径尾段 —— 而随手记的文件名是新建时定下的
//! slug,之后改标题**不改文件名**([[notebook-filename-is-a-link-target]]),所以
//! 只按 stem 匹配会整片漏掉按标题写的链接。
//!
//! 解析规则与前端 `noteLinks.ts` 的 `resolveLink` 对齐:归一化后,不含 `/` 的先
//! 试 stem 再试 title,含 `/` 的先试整段路径再试尾段。两边分叉的表现是图谱里的边
//! 和反链面板里的条目不一致 —— 而那种不一致是静默的。
//!
//! 归一化(`normalize_target`)也照抄前端:大小写不敏感是刻意的,macOS 默认文件
//! 系统本身不区分大小写,让 `[[foo]]` 和 `[[Foo]]` 指向不同笔记会造出一种「看起来
//! 一样却打不开」的链接。
//!
//! `links.target_norm` 存的就是归一化后的目标,反链因此是一次索引查询而不是全表
//! 扫描 + 逐行归一化。

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

/// 图里的一个节点:一篇笔记的身份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocRef {
    pub id: i64,
    pub path: String,
    pub title: String,
}

/// 一次解析的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub id: i64,
    /// 靠哪一路匹配上的。歧义提示要用它区分「改过标题」和「真的重名」。
    pub via: Via,
    /// 同一归一化键下是否还有别的笔记。
    pub ambiguous: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Stem,
    Title,
    Path,
}

/// 归一化到可比形式:trim → `\` 换 `/` → 解百分号编码 → 去尾部 `.md` → 去首尾
/// `/` → 小写。
pub fn normalize_target(input: &str) -> String {
    let swapped = input.trim().replace('\\', "/");
    let decoded = percent_decode(&swapped);
    let no_ext = strip_md_extension(&decoded);
    no_ext.trim_matches('/').to_lowercase()
}

/// 去掉尾部的 `.md`(大小写不敏感)。
fn strip_md_extension(input: &str) -> &str {
    let len = input.len();
    if len >= 3 && input[len - 3..].eq_ignore_ascii_case(".md") {
        &input[..len - 3]
    } else {
        input
    }
}

/// 解 `%XX`。与前端的 `decodeURIComponent` 对齐(UTF-8 解码)。
///
/// 解不出合法 UTF-8 时返回原文 —— 目标里有孤立的 `%`(`50%完成`)时前端那边
/// `decodeURIComponent` 会抛,它选择保留原样,这里同理:保留原文比丢掉整条链接好。
fn percent_decode(input: &str) -> String {
    if !input.contains('%') {
        return input.to_string();
    }
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

/// 从路径取不带扩展名的文件名。同时吃 `/` 和 `\`(Windows 路径)。
pub fn stem_of(path: &str) -> String {
    let tail = path.rsplit(['/', '\\']).next().unwrap_or(path);
    strip_md_extension(tail).to_string()
}

/// vault 的链接索引。三张表各管一种写法。
///
/// 值是 `Vec` 而不是单个:同一个键下可能有多篇(改过标题撞上别人的文件名、或者
/// 真的两篇同名),都留着才能报歧义 —— 静默取第一篇会让用户以为链接指向的是另
/// 一篇。
#[derive(Debug, Default)]
pub struct LinkIndex {
    by_stem: HashMap<String, Vec<i64>>,
    by_title: HashMap<String, Vec<i64>>,
    by_path: HashMap<String, i64>,
    /// 路径式链接的尾段匹配用。归一化路径 + doc_id,按 path 排序保证结果稳定。
    paths: Vec<(String, i64)>,
}

impl LinkIndex {
    pub fn build(docs: &[DocRef]) -> Self {
        let mut index = LinkIndex::default();
        for doc in docs {
            let stem = normalize_target(&stem_of(&doc.path));
            if !stem.is_empty() {
                index.by_stem.entry(stem).or_default().push(doc.id);
            }
            let title = normalize_target(&doc.title);
            if !title.is_empty() {
                index.by_title.entry(title).or_default().push(doc.id);
            }
            let path = normalize_target(&doc.path);
            if !path.is_empty() {
                index.by_path.entry(path.clone()).or_insert(doc.id);
                index.paths.push((path, doc.id));
            }
        }
        // 命中顺序必须稳定:HashMap 的迭代顺序不定,`docs` 的行序也不保证。不排序
        // 的话「歧义时取第一篇」会在两次运行间指向不同笔记。
        for ids in index.by_stem.values_mut() {
            ids.sort_unstable();
        }
        for ids in index.by_title.values_mut() {
            ids.sort_unstable();
        }
        index.paths.sort();
        index
    }

    /// 解析一条字面目标。语法后缀(`#小节`、`|别名`)应在调用前剥掉。
    pub fn resolve(&self, target: &str) -> Option<Resolved> {
        let needle = normalize_target(target);
        if needle.is_empty() {
            return None;
        }

        if !needle.contains('/') {
            if let Some(ids) = self.by_stem.get(&needle).filter(|v| !v.is_empty()) {
                return Some(Resolved {
                    id: ids[0],
                    via: Via::Stem,
                    ambiguous: ids.len() > 1,
                });
            }
            if let Some(ids) = self.by_title.get(&needle).filter(|v| !v.is_empty()) {
                return Some(Resolved {
                    id: ids[0],
                    via: Via::Title,
                    ambiguous: ids.len() > 1,
                });
            }
            return None;
        }

        if let Some(id) = self.by_path.get(&needle) {
            return Some(Resolved {
                id: *id,
                via: Via::Path,
                ambiguous: false,
            });
        }

        // 尾段匹配:`[[sub/foo]]` 命中 `/vault/notes/sub/foo.md`。前缀那个 `/`
        // 不能省,否则 `[[b/foo]]` 会命中 `/vault/ab/foo.md`。
        let tail = format!("/{needle}");
        let hits: Vec<i64> = self
            .paths
            .iter()
            .filter(|(norm, _)| norm.ends_with(&tail))
            .map(|(_, id)| *id)
            .collect();
        if hits.is_empty() {
            return None;
        }
        Some(Resolved {
            id: hits[0],
            via: Via::Path,
            ambiguous: hits.len() > 1,
        })
    }
}

/// 读出全部笔记身份。20k 篇量级下这是几 MB 的字符串,一次读完比逐条查省事。
pub fn load_docs(conn: &Connection) -> Result<Vec<DocRef>, String> {
    let mut stmt = conn
        .prepare("SELECT id, path, title FROM docs ORDER BY id")
        .map_err(|e| format!("Cannot prepare notebook doc query: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(DocRef {
                id: row.get(0)?,
                path: row.get(1)?,
                title: row.get(2)?,
            })
        })
        .map_err(|e| format!("Cannot query notebook docs: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("Cannot read notebook doc row: {e}"))?);
    }
    Ok(out)
}

/// 一篇笔记指出去、且能解析到本 vault 的其他笔记。
///
/// 排除自引用:`[[自己]]` 在图上是自环,拿去扩展检索上下文只会把已经命中的那篇
/// 再取一遍。
pub fn forward_targets(
    conn: &Connection,
    index: &LinkIndex,
    doc_id: i64,
) -> Result<Vec<i64>, String> {
    let mut stmt = conn
        .prepare("SELECT target FROM links WHERE doc_id = ?1 ORDER BY id")
        .map_err(|e| format!("Cannot prepare notebook link query: {e}"))?;
    let rows = stmt
        .query_map(rusqlite::params![doc_id], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Cannot query notebook links: {e}"))?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for row in rows {
        let target = row.map_err(|e| format!("Cannot read notebook link row: {e}"))?;
        if let Some(resolved) = index.resolve(&target) {
            if resolved.id != doc_id && seen.insert(resolved.id) {
                out.push(resolved.id);
            }
        }
    }
    Ok(out)
}

/// 哪些笔记指向了这一篇。
///
/// 走 `target_norm` 上的索引:一篇笔记有 stem、title、path 三种被指法,把这三个键
/// 都查一遍,比全表扫描 + 逐行解析便宜得多。
///
/// 反链**不重新做优先级判定** —— 也就是说,若 A 的 `[[foo]]` 实际按 stem 命中了
/// B,而 C 的标题恰好也是 `foo`,那么 C 的反链里会出现 A。这是刻意的宽松:反链回答
/// 的是「谁提到过我」,漏报比多报难查。
pub fn backlinks(conn: &Connection, doc: &DocRef) -> Result<Vec<i64>, String> {
    let mut keys: Vec<String> = Vec::new();
    for key in [
        normalize_target(&stem_of(&doc.path)),
        normalize_target(&doc.title),
        normalize_target(&doc.path),
    ] {
        if !key.is_empty() && !keys.contains(&key) {
            keys.push(key);
        }
    }
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = keys.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT DISTINCT doc_id FROM links
         WHERE target_norm IN ({placeholders}) AND doc_id != ?{}
         ORDER BY doc_id",
        keys.len() + 1
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Cannot prepare notebook backlink query: {e}"))?;
    let mut params: Vec<&dyn rusqlite::ToSql> =
        keys.iter().map(|k| k as &dyn rusqlite::ToSql).collect();
    params.push(&doc.id);
    let rows = stmt
        .query_map(params.as_slice(), |row| row.get::<_, i64>(0))
        .map_err(|e| format!("Cannot query notebook backlinks: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("Cannot read notebook backlink row: {e}"))?);
    }
    Ok(out)
}

/// 一篇笔记的邻居:出向 + 反链,出向在前。
///
/// 检索扩展只要「相关的几篇」,方向不重要,但出向是作者显式写下的关联,比反链更
/// 可能贴题,所以排在前面。
pub fn neighbors(
    conn: &Connection,
    index: &LinkIndex,
    doc: &DocRef,
    limit: usize,
) -> Result<Vec<i64>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for id in forward_targets(conn, index, doc.id)? {
        if seen.insert(id) {
            out.push(id);
            if out.len() >= limit {
                return Ok(out);
            }
        }
    }
    for id in backlinks(conn, doc)? {
        if seen.insert(id) {
            out.push(id);
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = format!(
            "{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(format!("aeroric-rag-graph-{unique}"));
        std::fs::create_dir_all(&dir).expect("create temp vault");
        dir
    }

    fn doc(id: i64, path: &str, title: &str) -> DocRef {
        DocRef {
            id,
            path: path.to_string(),
            title: title.to_string(),
        }
    }

    /// 建一个装好 docs / links 的库。`links` 是 (doc_id, 字面目标)。
    fn seeded(docs: &[DocRef], links: &[(i64, &str)]) -> Connection {
        let vault = temp_vault();
        let conn = super::super::db::open(&vault, 4).expect("open db");
        for d in docs {
            conn.execute(
                "INSERT INTO docs(id, path, title) VALUES (?1, ?2, ?3)",
                rusqlite::params![d.id, d.path, d.title],
            )
            .expect("insert doc");
        }
        for (doc_id, target) in links {
            conn.execute(
                "INSERT INTO links(doc_id, target, target_norm, kind)
                 VALUES (?1, ?2, ?3, 'wikilink')",
                rusqlite::params![doc_id, target, normalize_target(target)],
            )
            .expect("insert link");
        }
        conn
    }

    #[test]
    fn normalizes_case_and_extension() {
        // macOS 的文件系统本身不区分大小写,`[[Foo]]` 与 `[[foo]]` 必须同指一篇。
        assert_eq!(normalize_target("Foo.MD"), "foo");
        assert_eq!(normalize_target("  /sub/Bar.md "), "sub/bar");
        assert_eq!(normalize_target("a\\b"), "a/b");
    }

    #[test]
    fn a_trailing_slash_after_md_keeps_the_extension() {
        // 顺序与前端一致:先切 `.md` 再去首尾 `/`,所以 `.md/` 里的 `.md` 不在
        // 末尾、切不掉。写法本身是畸形的,这里只要求两侧同样处理 —— 分叉的表现
        // 是图谱的边和反链面板对不上。
        assert_eq!(normalize_target("/sub/Bar.md/"), "sub/bar.md");
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(normalize_target("%E8%AE%A1%E5%88%92"), "计划");
    }

    #[test]
    fn keeps_lone_percent_as_is() {
        // `50%完成` 里那个 `%` 不是转义。丢掉整条链接比留着原文糟。
        assert_eq!(normalize_target("50%完成"), "50%完成");
        assert_eq!(normalize_target("%zz"), "%zz");
    }

    #[test]
    fn strips_only_a_trailing_md() {
        // 中间的 `.md` 是文件名的一部分,不能切。
        assert_eq!(normalize_target("a.md.txt"), "a.md.txt");
        assert_eq!(normalize_target(".md"), "");
    }

    #[test]
    fn stem_takes_the_last_segment() {
        assert_eq!(stem_of("/vault/sub/foo.md"), "foo");
        assert_eq!(stem_of("C:\\notes\\bar.md"), "bar");
        assert_eq!(stem_of("plain"), "plain");
    }

    #[test]
    fn resolves_stem_before_title() {
        // 1 的文件名是 foo,2 的标题是 foo。stem 优先。
        let index = LinkIndex::build(&[
            doc(1, "/v/foo.md", "别的标题"),
            doc(2, "/v/other.md", "foo"),
        ]);
        let hit = index.resolve("foo").expect("resolve");
        assert_eq!(hit.id, 1);
        assert_eq!(hit.via, Via::Stem);
        assert!(!hit.ambiguous);
    }

    #[test]
    fn falls_back_to_title() {
        // 随手记的文件名是新建时的 slug,改标题不改文件名 —— 只按 stem 匹配会
        // 把所有按标题写的链接漏掉。
        let index = LinkIndex::build(&[doc(7, "/v/note-1738.md", "季度计划")]);
        let hit = index.resolve("季度计划").expect("resolve");
        assert_eq!(hit.id, 7);
        assert_eq!(hit.via, Via::Title);
    }

    #[test]
    fn flags_ambiguous_stems() {
        let index = LinkIndex::build(&[doc(1, "/v/a/foo.md", "x"), doc(2, "/v/b/foo.md", "y")]);
        let hit = index.resolve("foo").expect("resolve");
        assert!(hit.ambiguous);
    }

    #[test]
    fn ambiguous_resolution_is_stable() {
        // HashMap 的迭代顺序不定,不排序的话「取第一篇」会在两次运行间换人。
        let docs = [doc(9, "/v/b/foo.md", "y"), doc(4, "/v/a/foo.md", "x")];
        assert_eq!(LinkIndex::build(&docs).resolve("foo").unwrap().id, 4);
    }

    #[test]
    fn resolves_path_by_tail() {
        let index = LinkIndex::build(&[doc(3, "/vault/notes/sub/foo.md", "t")]);
        let hit = index.resolve("sub/foo").expect("resolve");
        assert_eq!(hit.id, 3);
        assert_eq!(hit.via, Via::Path);
    }

    #[test]
    fn tail_match_requires_a_segment_boundary() {
        // `[[b/foo]]` 不能命中 `/vault/ab/foo.md`。
        let index = LinkIndex::build(&[doc(1, "/vault/ab/foo.md", "t")]);
        assert!(index.resolve("b/foo").is_none());
    }

    #[test]
    fn unresolvable_target_is_none() {
        let index = LinkIndex::build(&[doc(1, "/v/foo.md", "t")]);
        assert!(index.resolve("不存在").is_none());
        assert!(index.resolve("   ").is_none());
    }

    #[test]
    fn forward_targets_resolve_and_dedupe() {
        let docs = vec![doc(1, "/v/a.md", "甲"), doc(2, "/v/b.md", "乙")];
        let conn = seeded(&docs, &[(1, "b"), (1, "b"), (1, "乙"), (1, "不存在")]);
        let index = LinkIndex::build(&docs);
        assert_eq!(forward_targets(&conn, &index, 1).expect("forward"), vec![2]);
    }

    #[test]
    fn forward_targets_skip_self_reference() {
        // `[[自己]]` 在图上是自环,扩展上下文时只会把已命中的那篇再取一遍。
        let docs = vec![doc(1, "/v/a.md", "甲")];
        let conn = seeded(&docs, &[(1, "a")]);
        let index = LinkIndex::build(&docs);
        assert!(forward_targets(&conn, &index, 1)
            .expect("forward")
            .is_empty());
    }

    #[test]
    fn backlinks_match_stem_title_and_path() {
        let docs = vec![
            doc(1, "/v/target.md", "目标标题"),
            doc(2, "/v/by-stem.md", "b"),
            doc(3, "/v/by-title.md", "c"),
            doc(4, "/v/by-path.md", "d"),
            doc(5, "/v/unrelated.md", "e"),
        ];
        let conn = seeded(
            &docs,
            &[
                (2, "target"),
                (3, "目标标题"),
                (4, "/v/target.md"),
                (5, "别的"),
            ],
        );
        assert_eq!(
            backlinks(&conn, &docs[0]).expect("backlinks"),
            vec![2, 3, 4]
        );
    }

    #[test]
    fn backlinks_exclude_self() {
        let docs = vec![doc(1, "/v/a.md", "甲")];
        let conn = seeded(&docs, &[(1, "a")]);
        assert!(backlinks(&conn, &docs[0]).expect("backlinks").is_empty());
    }

    #[test]
    fn backlinks_are_case_insensitive() {
        let docs = vec![doc(1, "/v/Target.md", "T"), doc(2, "/v/b.md", "B")];
        let conn = seeded(&docs, &[(2, "TARGET")]);
        assert_eq!(backlinks(&conn, &docs[0]).expect("backlinks"), vec![2]);
    }

    #[test]
    fn neighbors_put_forward_links_first() {
        let docs = vec![
            doc(1, "/v/a.md", "甲"),
            doc(2, "/v/b.md", "乙"),
            doc(3, "/v/c.md", "丙"),
        ];
        // 1 → 2;3 → 1。邻居应是 [2(出向), 3(反链)]。
        let conn = seeded(&docs, &[(1, "b"), (3, "a")]);
        let index = LinkIndex::build(&docs);
        assert_eq!(
            neighbors(&conn, &index, &docs[0], 8).expect("neighbors"),
            vec![2, 3]
        );
    }

    #[test]
    fn neighbors_respect_the_limit() {
        let docs = vec![
            doc(1, "/v/a.md", "甲"),
            doc(2, "/v/b.md", "乙"),
            doc(3, "/v/c.md", "丙"),
        ];
        let conn = seeded(&docs, &[(1, "b"), (1, "c")]);
        let index = LinkIndex::build(&docs);
        assert_eq!(
            neighbors(&conn, &index, &docs[0], 1).expect("neighbors"),
            vec![2]
        );
        assert!(neighbors(&conn, &index, &docs[0], 0)
            .expect("neighbors")
            .is_empty());
    }

    #[test]
    fn neighbors_dedupe_across_directions() {
        // 互指的两篇只能出现一次。
        let docs = vec![doc(1, "/v/a.md", "甲"), doc(2, "/v/b.md", "乙")];
        let conn = seeded(&docs, &[(1, "b"), (2, "a")]);
        let index = LinkIndex::build(&docs);
        assert_eq!(
            neighbors(&conn, &index, &docs[0], 8).expect("neighbors"),
            vec![2]
        );
    }

    #[test]
    fn load_docs_reads_every_row() {
        let docs = vec![doc(1, "/v/a.md", "甲"), doc(2, "/v/b.md", "乙")];
        let conn = seeded(&docs, &[]);
        assert_eq!(load_docs(&conn).expect("load"), docs);
    }
}
