import XCTest
@testable import AeroricUI

/// P7-05 验收：NotebookStoreTests——基线/冲突簿记（不静默覆盖）、任务复选框
/// 行级翻转、重名标签聚合、模板占位符本地时区展开、path 即主键（标题改名不
/// 换身份）、回收站簿记、树懒加载合并。
@MainActor
final class NotebookStoreTests: XCTestCase {

    // MARK: - 基线与冲突

    func testBaselineOnlyMovesOnConfirmedSave() {
        let store = NotebookStore()
        let baseline = NotebookStore.FileSig(mtimeMs: 1_000, hash: "a")
        store.recordOpen(path: "/v/Note.md", sig: baseline)
        XCTAssertEqual(store.baseline(of: "/v/Note.md"), baseline)

        // 保存成功：基线随新 sig 前进。
        let next = NotebookStore.FileSig(mtimeMs: 2_000, hash: "b")
        store.applySaved(path: "/v/Note.md", sig: next)
        XCTAssertEqual(store.baseline(of: "/v/Note.md"), next)
        XCTAssertFalse(store.dirtyPaths.contains("/v/Note.md"))

        // 关闭：基线清空——旧基线不能留给下一个打开同名文件的人。
        store.recordClose(path: "/v/Note.md")
        XCTAssertNil(store.baseline(of: "/v/Note.md"))
    }

    func testConflictDoesNotTouchBaselineOrDisk() {
        // 冲突分支：store 只暴露磁盘现状给弹窗，不改任何簿记——
        // 覆盖与否由用户决策，决策前基线保持不动。
        let store = NotebookStore()
        let baseline = NotebookStore.FileSig(mtimeMs: 1_000, hash: "a")
        store.recordOpen(path: "/v/Shared.md", sig: baseline)
        store.recordDirty(path: "/v/Shared.md")

        let disk = NotebookStore.FileSig(mtimeMs: 5_000, hash: "zzz")
        // 冲突回执只读：baseline 不变（save 未成功），dirty 也保留（编辑仍在缓冲区）。
        XCTAssertEqual(store.baseline(of: "/v/Shared.md"), baseline)
        XCTAssertTrue(store.dirtyPaths.contains("/v/Shared.md"))
        XCTAssertNotEqual(store.baseline(of: "/v/Shared.md")?.hash, disk.hash)

        // 只有 force 保存成功后才前进基线。
        store.applySaved(path: "/v/Shared.md", sig: disk)
        XCTAssertEqual(store.baseline(of: "/v/Shared.md"), disk)
        XCTAssertFalse(store.dirtyPaths.contains("/v/Shared.md"))
    }

    // MARK: - 任务复选框翻转

    func testFlipTaskLineTogglesOnlyTheTargetLine() {
        let content = "# Todo\n\n- [ ] buy milk\n- [x] done thing\nsome [x] inline text\n"
        let flipped = NotebookStore.flipTaskLine(content: content, line: 3)
        let expected = "# Todo\n\n- [x] buy milk\n- [x] done thing\nsome [x] inline text\n"
        XCTAssertEqual(flipped, expected, "只翻第 3 行的任务框，行中段 [x] 文本不动")

        let flippedBack = NotebookStore.flipTaskLine(content: flipped ?? "", line: 3)
        XCTAssertEqual(flippedBack, content)
    }

    func testFlipTaskLineRefusesNonTasksAndOutOfRange() {
        XCTAssertNil(NotebookStore.flipTaskLine(content: "plain text\n", line: 1))
        XCTAssertNil(NotebookStore.flipTaskLine(content: "- [ ] a\n", line: 9), "行号越界返回 nil")
        XCTAssertNil(NotebookStore.flipTaskLine(content: "- [ ] a\n", line: 0))
        // 有序列表里的任务也认。
        XCTAssertEqual(
            NotebookStore.flipTaskLine(content: "1. [ ] step\n", line: 1),
            "1. [x] step\n")
    }

    // MARK: - 重名标签聚合

    func testDuplicateTagsAggregateByNormalizedKey() {
        let store = NotebookStore()
        store.ingestTags([
            NotebookStore.TagSource(
                path: "/v/A.md",
                tags: [
                    NotebookStore.TagRef(raw: "Alpha", line: 2, preview: "a #Alpha"),
                    NotebookStore.TagRef(raw: "alpha", line: 5, preview: "b #alpha"),
                ]),
            NotebookStore.TagSource(
                path: "/v/B.md",
                tags: [NotebookStore.TagRef(raw: "ALPHA", line: 1, preview: "c #ALPHA")]),
            NotebookStore.TagSource(
                path: "/v/C.md",
                tags: [NotebookStore.TagRef(raw: "beta", line: 3, preview: "d #beta")]),
        ])

        XCTAssertEqual(store.tags.count, 2)
        let alpha = store.tags[0]
        XCTAssertEqual(alpha.key, "alpha", "大小写不敏感聚合")
        XCTAssertEqual(alpha.sample, "Alpha", "sample 保留第一次出现的原始大小写")
        XCTAssertEqual(alpha.count, 3, "重名标签每一处出现单独可数")
        XCTAssertEqual(alpha.files, ["/v/A.md", "/v/B.md"])
        XCTAssertEqual(store.tags[1].key, "beta")
        XCTAssertEqual(store.tags[1].count, 1)
    }

    // MARK: - 模板占位符

    func testTemplatePlaceholdersFillDateTimeButLeaveTitle() {
        let template = "# {{Title}}\n\n今天是 {{ date }}，写于 {{TIME}}。保持 {{title}} 原样。\n"
        let now = Date(timeIntervalSince1970: 1_759_456_000) // 固定时区下断言形状
        let filled = NotebookStore.fillDateTime(template, now: now)
        // {{title}} 不动（大小写不敏感的占位符名不算 title 本体——原文要等标题定稿）。
        XCTAssertFalse(filled.contains("{{ date }}"))
        XCTAssertFalse(filled.contains("{{TIME}}"))
        XCTAssertTrue(filled.contains("{{Title}}"))
        // 日期形状 YYYY-MM-DD，时间 HH:mm（本地时区口径）。
        XCTAssertTrue(filled.range(of: #"今天是\s+\d{4}-\d{2}-\d{2}"#, options: .regularExpression) != nil)
        XCTAssertTrue(filled.range(of: #"\d{2}:\d{2}。"#, options: .regularExpression) != nil)

        let titled = NotebookStore.fillTitle(filled, title: "周会")
        XCTAssertTrue(titled.contains("# 周会"))
        XCTAssertTrue(titled.contains("保持 周会 原样"))
        XCTAssertFalse(titled.contains("{{Title}}"))
    }

    // MARK: - path 即主键

    func testIdentitySurvivesTitleChangeWithoutRename() {
        // display title 只是展示：store 的簿记键是 path。标题改了（还没改名文件）
        // 的时候，基线/dirty 都仍按同一条 path 记账。
        let store = NotebookStore()
        let sig = NotebookStore.FileSig(mtimeMs: 1, hash: "h")
        store.recordOpen(path: "/v/untitled.md", sig: sig)
        store.recordDirty(path: "/v/untitled.md")

        // 用户把 frontmatter 标题从 untitled 改成 Plan——path 未变。
        XCTAssertEqual(store.baseline(of: "/v/untitled.md"), sig)
        XCTAssertTrue(store.dirtyPaths.contains("/v/untitled.md"))

        // 改名后（宿主返回新路径）才换键：旧键关、新键开。
        store.recordClose(path: "/v/untitled.md")
        store.recordOpen(path: "/v/Plan.md", sig: sig)
        XCTAssertNil(store.baseline(of: "/v/untitled.md"))
        XCTAssertEqual(store.baseline(of: "/v/Plan.md"), sig)
    }

    // MARK: - 回收站簿记

    func testTrashIngestKeepsNewestFirstAndRemovesById() {
        let store = NotebookStore()
        let newest = NotebookStore.TrashEntry(id: "2", name: "b.md", relativePath: "b.md", deletedAtMs: 200, isDir: false)
        let oldest = NotebookStore.TrashEntry(id: "1", name: "a.md", relativePath: "a.md", deletedAtMs: 100, isDir: false)
        store.ingestTrash([newest, oldest])
        XCTAssertEqual(store.trash.map(\.id), ["2", "1"], "宿主排序原样保留：新删在前")

        store.removeTrash(id: "2")
        XCTAssertEqual(store.trash.map(\.id), ["1"])
        store.clearTrash()
        XCTAssertTrue(store.trash.isEmpty)
    }

    // MARK: - 树懒加载合并

    func testTreeChildrenMergeLazily() {
        let store = NotebookStore()
        let sub = NotebookStore.TreeEntry(name: "sub", path: "/v/sub", isDir: true, children: nil, truncated: false)
        store.ingestTree([sub])
        store.ingestChildren(
            of: "/v/sub",
            entries: [NotebookStore.TreeEntry(name: "n.md", path: "/v/sub/n.md", isDir: false, children: nil, truncated: false)])
        let merged = store.tree[0]
        XCTAssertEqual(merged.children?.count, 1)
        XCTAssertEqual(merged.children?[0].path, "/v/sub/n.md")

        // 二级嵌套也并得进同一棵树。
        store.ingestChildren(
            of: "/v/sub/nested",
            entries: [NotebookStore.TreeEntry(name: "deep.md", path: "/v/sub/nested/deep.md", isDir: false, children: nil, truncated: false)])
        var nested = store.tree[0].children?[0]
        // deep.md 不是 /v/sub 的直接子级；再挂一层验证不误并。
        nested = NotebookStore.TreeEntry(name: "nested", path: "/v/sub/nested", isDir: true, children: nil, truncated: false)
        store.ingestChildren(
            of: "/v/sub",
            entries: [
                NotebookStore.TreeEntry(name: "n.md", path: "/v/sub/n.md", isDir: false, children: nil, truncated: false),
                nested!,
            ])
        XCTAssertEqual(store.tree[0].children?.count, 2)
    }
}
