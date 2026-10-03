//! 导入的转换报告。
//!
//! 这一节的准入条件是「成功 / 跳过 / 资源丢失 / 格式降级 **逐项列出**」,所以报告
//! 不是移植 Markio 的 `ImportReport` —— 那个是两个计数加一串**封顶 50 条**的自由
//! 文本(`common.rs:358` 的 `push_warning_limited`,第 51 条起变成「后续警告已省略」),
//! 「资源丢失」和「格式降级」没有独立表示,全压在同一个会截断的 `warnings` 里。
//!
//! 形状是**两层**,不是一个平坦的四选一:
//!
//! - [`ItemStatus`] —— 这一条**落地了没有**。三选一,互斥。
//! - [`ItemIssue`] —— 这一条**丢了什么**。可以有多条,也可以为空。
//!
//! 分两层是因为那四个类别本来就不互斥:一篇笔记完全可以「导入成功了,但里面那张图
//! 没跟过来」。强行选一个的话,选「资源丢失」会瞒掉笔记其实落地了,选「成功」会瞒掉
//! 那张图丢了 —— 两种都是在报告里说谎,而报告是这一节唯一的交付物。

use serde::Serialize;

/// 一条记录**落地了没有**。互斥三档。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ItemStatus {
    /// 写进 vault 了。**不代表无损** —— 看 [`ImportItem::issues`]。
    Imported,
    /// 没写,而且这是预期内的。
    Skipped { reason: SkipReason },
    /// 没写,且是意外。和 `Skipped` 分开:一个是「按规矩没收」,一个是「该收却没收上」,
    /// 混在一起用户没法判断要不要重试。
    Failed { detail: String },
}

/// 跳过的理由。做成 enum 而不是自由文本,因为报告要按理由分组统计 ——
/// 「300 条已经导过」和「300 条格式不支持」对用户是完全不同的两件事。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SkipReason {
    /// 增量清单里已有同一指纹。重复导入的正常结果。
    AlreadyImported,
    /// 这类文件这个导入器不收(例如 Logseq 的 `config.edn`)。
    Unsupported { extension: String },
    /// 单体超过上限。带上实际大小 —— 只说「太大」用户没法判断是差一点还是差一个量级。
    TooLarge { bytes: u64 },
    /// 撞到整轮上限(条目数 / 总字节 / 目录深度),后面的都没看。
    LimitReached { limit: &'static str },
    /// 源端拿不到内容(Apple Notes 里锁定的笔记走这条)。
    Unreadable { detail: String },
    /// 符号链接,不跟随。
    ///
    /// 单独一档而不是塞进 `Unreadable`:它读得出来,是**我们选择**不跟 —— 跟随会让
    /// 落点跑出源目录(指向 `/etc` 的软链),也可能自己成环。目录型导入几乎每次都会
    /// 遇到,所以它值得一个能单独计数的类别。
    Symlink,
}

/// 这一条**丢了什么**。和 [`ItemStatus`] 正交:`Imported` 的条目也可以带一串 issue。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ItemIssue {
    /// 引用的资源没跟过来(图片、附件、`<en-media>` 找不到对应 resource)。
    ///
    /// 正文里那条引用**保持原样**不动。改成纯文本会把「这里本来有张图」这件事也
    /// 抹掉,而报告里这一条的意义正是让用户能回源端把它找回来。
    ResourceLost { target: String, detail: String },
    /// 格式降级:内容进来了,但表达能力掉了一档(`.org` 没转、ENML 的结构被铺平)。
    Degraded { detail: String },
}

/// 报告里的一条。`source` 是**源端**的标识(zip 内路径 / 文件相对路径 / 笔记标题),
/// 不是落点 —— 用户要拿它回源端对账,所以即使跳过或失败也必须有。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    pub source: String,
    /// 落点的 vault 相对路径。`Skipped` / `Failed` 时是 `None` —— 那时候没有落点,
    /// 填一个"本来会落在哪"只会让人以为文件在那儿。
    pub dest: Option<String>,
    pub status: ItemStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<ItemIssue>,
}

impl ImportItem {
    /// 成功落地,无损。
    pub fn imported(source: impl Into<String>, dest: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            dest: Some(dest.into()),
            status: ItemStatus::Imported,
            issues: Vec::new(),
        }
    }

    pub fn skipped(source: impl Into<String>, reason: SkipReason) -> Self {
        Self {
            source: source.into(),
            dest: None,
            status: ItemStatus::Skipped { reason },
            issues: Vec::new(),
        }
    }

    pub fn failed(source: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            dest: None,
            status: ItemStatus::Failed {
                detail: detail.into(),
            },
            issues: Vec::new(),
        }
    }

    pub fn with_issue(mut self, issue: ItemIssue) -> Self {
        self.issues.push(issue);
        self
    }
}

/// 明细最多留这么多条。十万条 `ImportItem` 全带着字符串留在内存里、再整份
/// 序列化过 IPC 给前端,是拿报告把应用撑死。
const MAX_DETAIL_ITEMS: usize = 2_000;

/// 一次导入的完整报告。
///
/// **计数不封顶,明细封顶。** 这是和 Markio 那份报告最要紧的一处不同:它的 `warnings`
/// 到 50 条就截断,于是「有多少条出了问题」这个数字本身就丢了 —— 用户看到 50 条警告,
/// 不知道后面还有 5 条还是 5000 条。这里明细同样会截断(理由见 [`MAX_DETAIL_ITEMS`]),
/// 但四个计数是**在截断之前**累加的,所以规模永远是真的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub provider: String,
    /// 落点目录的 vault 相对路径。
    pub dest: String,
    pub imported: usize,
    pub skipped: usize,
    pub failed: usize,
    /// **带**资源丢失的条目数。跨状态计数 —— 一条 `Imported` 也可能算在里面,
    /// 所以它和上面三个不构成划分,不该被加在一起。
    pub resource_lost: usize,
    /// **带**格式降级的条目数。同上。
    pub degraded: usize,
    /// 明细。`items.len()` 可能小于 `imported + skipped + failed`,差额见 `truncated`。
    pub items: Vec<ImportItem>,
    /// 明细被截掉了多少条。`0` = 上面那份是全部。
    pub truncated: usize,
    /// 报告落盘成的那篇笔记(vault 相对路径)。写不进去时是 `None`,
    /// 并且**不算导入失败** —— 笔记已经在 vault 里了。
    pub report_path: Option<String>,
}

impl ImportReport {
    pub fn new(provider: impl Into<String>, dest: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            dest: dest.into(),
            imported: 0,
            skipped: 0,
            failed: 0,
            resource_lost: 0,
            degraded: 0,
            items: Vec::new(),
            truncated: 0,
            report_path: None,
        }
    }

    /// 记一条。计数先加,再决定明细要不要留 —— 顺序反了截断就会同时吃掉计数。
    pub fn push(&mut self, item: ImportItem) {
        match &item.status {
            ItemStatus::Imported => self.imported += 1,
            ItemStatus::Skipped { .. } => self.skipped += 1,
            ItemStatus::Failed { .. } => self.failed += 1,
        }
        // 用 `any` 而不是数 issue 的条数:这两个计数的语义是「**多少条**目受了影响」,
        // 一条笔记丢三张图是一条受影响的笔记,不是三条。
        if item
            .issues
            .iter()
            .any(|issue| matches!(issue, ItemIssue::ResourceLost { .. }))
        {
            self.resource_lost += 1;
        }
        if item
            .issues
            .iter()
            .any(|issue| matches!(issue, ItemIssue::Degraded { .. }))
        {
            self.degraded += 1;
        }
        if self.items.len() < MAX_DETAIL_ITEMS {
            self.items.push(item);
        } else {
            self.truncated += 1;
        }
    }

    /// 这一轮有没有值得用户看一眼的东西。空报告不必弹面板。
    ///
    /// **这个判断的实际执行方在前端**(`noteImport.ts` 从序列化出去的三个计数自己判),
    /// 所以 Rust 侧只剩测试在用它 —— 留着是为了把「哪些计数算值得一看」这条语义
    /// 钉在离字段最近的地方,两边跑偏时测试会先响。
    #[cfg(test)]
    pub fn needs_attention(&self) -> bool {
        self.failed > 0 || self.resource_lost > 0 || self.degraded > 0
    }

    /// 报告渲染成一篇笔记的正文。
    ///
    /// 落成 `.md` 是因为 vault 是被索引的:这篇报告因此能被搜索、被 wikilink 指到、
    /// 跟着 P8 的同步走到别的设备上。代价是它自己也算一篇笔记 —— 所以名字带时间戳,
    /// 见 `landing::report_name`。
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("---\ntitle: 导入报告 · ");
        out.push_str(&self.provider);
        out.push_str("\ntags:\n  - 导入报告\n---\n\n");

        out.push_str("| 项 | 数 |\n| --- | --- |\n");
        out.push_str(&format!("| 导入成功 | {} |\n", self.imported));
        out.push_str(&format!("| 跳过 | {} |\n", self.skipped));
        out.push_str(&format!("| 失败 | {} |\n", self.failed));
        out.push_str(&format!("| 带资源丢失 | {} |\n", self.resource_lost));
        out.push_str(&format!("| 带格式降级 | {} |\n\n", self.degraded));
        out.push_str(&format!("落点:`{}`\n", self.dest));

        // 只列**需要用户处理**的那些。成功且无损的条目动辄上万条,全列出来会把报告
        // 变成一份文件清单 —— 而文件清单在 vault 里已经有了,就是那些笔记本身。
        let notable: Vec<&ImportItem> = self
            .items
            .iter()
            .filter(|item| !matches!(item.status, ItemStatus::Imported) || !item.issues.is_empty())
            .collect();

        if notable.is_empty() {
            out.push_str("\n全部导入成功,没有跳过、丢失或降级。\n");
        } else {
            out.push_str("\n## 需要留意\n\n");
            for item in notable {
                out.push_str("- ");
                out.push_str(&describe_status(&item.status));
                out.push_str(" `");
                out.push_str(&item.source);
                out.push('`');
                if let Some(dest) = &item.dest {
                    out.push_str(" → `");
                    out.push_str(dest);
                    out.push('`');
                }
                out.push('\n');
                for issue in &item.issues {
                    out.push_str("  - ");
                    out.push_str(&describe_issue(issue));
                    out.push('\n');
                }
            }
        }

        if self.truncated > 0 {
            out.push_str(&format!(
                "\n明细只保留了前 {MAX_DETAIL_ITEMS} 条,另有 {} 条未列出(上面的计数是全量)。\n",
                self.truncated
            ));
        }
        out
    }
}

fn describe_status(status: &ItemStatus) -> String {
    match status {
        ItemStatus::Imported => "已导入".to_string(),
        ItemStatus::Failed { detail } => format!("失败({detail})"),
        ItemStatus::Skipped { reason } => match reason {
            SkipReason::AlreadyImported => "跳过(已导入过)".to_string(),
            SkipReason::Unsupported { extension } => format!("跳过(不支持 .{extension})"),
            SkipReason::TooLarge { bytes } => {
                format!("跳过(超过单体上限,{} MB)", bytes / 1024 / 1024)
            }
            SkipReason::LimitReached { limit } => format!("跳过(已达上限:{limit})"),
            SkipReason::Unreadable { detail } => format!("跳过(读不出:{detail})"),
            SkipReason::Symlink => "跳过(符号链接,不跟随)".to_string(),
        },
    }
}

fn describe_issue(issue: &ItemIssue) -> String {
    match issue {
        ItemIssue::ResourceLost { target, detail } => {
            format!("资源丢失:`{target}` —— {detail}")
        }
        ItemIssue::Degraded { detail } => format!("格式降级:{detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lost(target: &str) -> ItemIssue {
        ItemIssue::ResourceLost {
            target: target.to_string(),
            detail: "找不到对应资源".to_string(),
        }
    }

    fn degraded() -> ItemIssue {
        ItemIssue::Degraded {
            detail: "org-mode 未转换".to_string(),
        }
    }

    #[test]
    fn three_statuses_count_separately() {
        let mut report = ImportReport::new("notion", "imports/notion");
        report.push(ImportItem::imported("a.md", "imports/notion/a.md"));
        report.push(ImportItem::skipped("b.md", SkipReason::AlreadyImported));
        report.push(ImportItem::failed("c.md", "写入失败"));
        assert_eq!((report.imported, report.skipped, report.failed), (1, 1, 1));
    }

    #[test]
    fn an_imported_item_can_also_have_lost_resources() {
        // 这条是两层结构存在的理由:落地了 **并且** 丢了东西。压成一个平坦
        // 四选一的话这两件事必须牺牲掉一件。
        let mut report = ImportReport::new("evernote", "imports/evernote");
        report.push(
            ImportItem::imported("笔记", "imports/evernote/笔记.md").with_issue(lost("图片")),
        );
        assert_eq!(report.imported, 1);
        assert_eq!(report.resource_lost, 1);
    }

    #[test]
    fn issue_counts_are_per_item_not_per_issue() {
        // 一篇笔记丢三张图是**一条**受影响的笔记。按 issue 条数计会让「有多少篇
        // 笔记要去补」这个数字凭空放大。
        let mut report = ImportReport::new("evernote", "d");
        report.push(
            ImportItem::imported("n", "d/n.md")
                .with_issue(lost("a.png"))
                .with_issue(lost("b.png"))
                .with_issue(lost("c.png")),
        );
        assert_eq!(report.resource_lost, 1);
    }

    #[test]
    fn one_item_can_carry_both_issue_kinds() {
        let mut report = ImportReport::new("logseq", "d");
        report.push(
            ImportItem::imported("n", "d/n.md")
                .with_issue(lost("a.png"))
                .with_issue(degraded()),
        );
        assert_eq!((report.resource_lost, report.degraded), (1, 1));
    }

    #[test]
    fn counts_survive_detail_truncation() {
        // Markio 那份报告的核心毛病:`warnings` 到 50 条就截断,于是「一共多少条」
        // 这个数字本身也没了。这里明细照样截,但计数必须是全量的 —— 否则截断会
        // 静默地改掉报告的结论。
        let mut report = ImportReport::new("obsidian", "d");
        let total = MAX_DETAIL_ITEMS + 137;
        for i in 0..total {
            report.push(ImportItem::imported(
                format!("n{i}.md"),
                format!("d/n{i}.md"),
            ));
        }
        assert_eq!(report.imported, total);
        assert_eq!(report.items.len(), MAX_DETAIL_ITEMS);
        assert_eq!(report.truncated, 137);
    }

    #[test]
    fn truncation_is_disclosed_in_the_markdown() {
        let mut report = ImportReport::new("obsidian", "d");
        for i in 0..MAX_DETAIL_ITEMS + 5 {
            report.push(ImportItem::failed(format!("n{i}.md"), "boom"));
        }
        let md = report.to_markdown();
        assert!(md.contains("另有 5 条未列出"));
        assert!(md.contains(&format!("| 失败 | {} |", MAX_DETAIL_ITEMS + 5)));
    }

    #[test]
    fn markdown_lists_only_items_needing_attention() {
        let mut report = ImportReport::new("obsidian", "d");
        report.push(ImportItem::imported("clean.md", "d/clean.md"));
        report.push(ImportItem::imported("lossy.md", "d/lossy.md").with_issue(lost("img.png")));
        report.push(ImportItem::skipped("old.md", SkipReason::AlreadyImported));
        let md = report.to_markdown();
        assert!(!md.contains("clean.md"), "无损成功的条目不该列进明细");
        assert!(md.contains("lossy.md"));
        assert!(md.contains("old.md"));
        assert!(md.contains("img.png"));
    }

    #[test]
    fn an_all_clean_run_says_so_explicitly() {
        let mut report = ImportReport::new("obsidian", "d");
        report.push(ImportItem::imported("a.md", "d/a.md"));
        let md = report.to_markdown();
        assert!(md.contains("全部导入成功"));
        assert!(!report.needs_attention());
    }

    #[test]
    fn skipping_alone_does_not_demand_attention() {
        // 重复导入的正常结果就是一堆 `AlreadyImported`。那不值得弹面板。
        let mut report = ImportReport::new("obsidian", "d");
        report.push(ImportItem::skipped("a.md", SkipReason::AlreadyImported));
        assert!(!report.needs_attention());
    }

    #[test]
    fn each_of_the_three_attention_triggers_fires_on_its_own() {
        for item in [
            ImportItem::failed("a", "boom"),
            ImportItem::imported("b", "d/b.md").with_issue(lost("x")),
            ImportItem::imported("c", "d/c.md").with_issue(degraded()),
        ] {
            let mut report = ImportReport::new("p", "d");
            report.push(item);
            assert!(report.needs_attention());
        }
    }

    #[test]
    fn skipped_and_failed_items_have_no_dest() {
        // 填一个「本来会落在哪」会让用户以为文件在那儿。
        let skipped = ImportItem::skipped("a", SkipReason::AlreadyImported);
        let failed = ImportItem::failed("b", "boom");
        assert!(skipped.dest.is_none() && failed.dest.is_none());
    }

    #[test]
    fn frontmatter_comes_first_so_the_report_is_a_real_note() {
        // 报告是 vault 里的一篇笔记,frontmatter 不在第一行的话 `fields` 那一层
        // 读不到 title,笔记列表里会显示成文件名。
        let report = ImportReport::new("notion", "imports/notion");
        assert!(report
            .to_markdown()
            .starts_with("---\ntitle: 导入报告 · notion\n"));
    }

    #[test]
    fn skip_reasons_render_distinguishably() {
        let cases = [
            (SkipReason::AlreadyImported, "已导入过"),
            (
                SkipReason::Unsupported {
                    extension: "org".to_string(),
                },
                ".org",
            ),
            (SkipReason::TooLarge { bytes: 5 << 20 }, "5 MB"),
            (SkipReason::LimitReached { limit: "条目数" }, "条目数"),
            (
                SkipReason::Unreadable {
                    detail: "笔记已锁定".to_string(),
                },
                "笔记已锁定",
            ),
            (SkipReason::Symlink, "符号链接"),
        ];
        for (reason, expected) in cases {
            let mut report = ImportReport::new("p", "d");
            report.push(ImportItem::skipped("s", reason));
            let md = report.to_markdown();
            assert!(
                md.contains(expected),
                "报告里应能看出跳过理由:{expected}\n{md}"
            );
        }
    }
}
