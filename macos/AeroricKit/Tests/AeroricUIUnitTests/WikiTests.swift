import AeroricClient
import AeroricProtocol
@testable import AeroricUI
import XCTest

@MainActor
private final class FakeWikiService: WikiService {
    var summaries: [LiveWikiPageSummary] = []
    var pagesBySlug: [String: LiveWikiPage] = [:]
    var issueList: [LiveWikiIssue] = []
    var calls: [String] = []

    var gaps = LiveWikiSourceGaps()
    func pages(projectID: String) async throws -> LiveWikiPageList { LiveWikiPageList(pages: summaries, gaps: gaps) }
    var pageError: Error?
    func page(projectID: String, slug: String) async throws -> LiveWikiPage? {
        calls.append("page \(slug)")
        if let pageError { throw pageError }
        return pagesBySlug[slug]
    }
    func compile(projectID: String) async throws -> LiveWikiReport {
        calls.append("compile \(projectID)")
        return LiveWikiReport(added: 2, updated: 0, removed: 0, issues: 1, pages: 3)
    }
    func edit(projectID: String, slug: String, markdown: String) async throws -> LiveWikiPage? {
        calls.append("edit \(slug)")
        return nil
    }
    func revisions(projectID: String, slug: String) async throws -> [LiveWikiRevision] {
        [LiveWikiRevision(n: 2, reason: "edit"), LiveWikiRevision(n: 1, reason: "compile")]
    }
    func revert(projectID: String, slug: String, n: Int) async throws -> LiveWikiPage? { calls.append("revert \(n)"); return nil }
    func issues(projectID: String) async throws -> [LiveWikiIssue] { issueList }
    func issueAction(projectID: String, id: String, accept: Bool) async throws { calls.append("issue \(id) \(accept)") }
    func exportPlan(projectID: String, dir: String) async throws -> LiveWikiExportPlan {
        if dir.hasPrefix("..") { throw ConnectionError.io("导出目录必须是项目里的相对路径，且不能在 .git 里") }
        return LiveWikiExportPlan(files: ["\(dir)/index.md"], overwrites: [])
    }
    func export(projectID: String, dir: String) async throws -> [String] { calls.append("export \(dir)"); return ["\(dir)/index.md"] }
    var access = true
    func agentAccess(projectID: String, set: Bool?) async throws -> Bool {
        if let set { access = set; calls.append("access \(set)") }
        return access
    }
    var state = LiveWikiExplainState(model: "m")
    var explainStates: [LiveWikiExplainState] = []
    func explain(projectID: String, slug: String) async throws -> Int { calls.append("explain \(slug)"); return 1 }
    func explainAll(projectID: String) async throws -> Int { calls.append("explainAll"); return 2 }
    func explainState(projectID: String) async throws -> LiveWikiExplainState {
        explainStates.isEmpty ? state : explainStates.removeFirst()
    }
}

/// B5.5-1: the Wiki page's model and words.
@MainActor
final class WikiTests: XCTestCase {
    func testAPageDecodesAndRendersBackToItsMarkdown() {
        let page = LiveWikiPage.decode(.object([
            "slug": .string("decision-delegation"), "kind": .string("decision"), "title": .string("委派"),
            "status": .string("implemented"), "category": .string("feature"), "revision": .number("3"),
            "blocks": .array([
                .object(["key": .string("note:a#Problem"), "heading": .string("Problem"), "text": .string("单干"),
                         "source": .object(["kind": .string("note"), "target": .string(".agents/notes/a.md#Problem"),
                                            "label": .string("a.md#Problem")])]),
                .object(["key": .string("note:a#Decision"), "heading": .string("Decision"), "text": .string("契约"),
                         "userEdited": .bool(true)]),
                .object(["key": .string("note:a#Consequences"), "heading": .string("Consequences"), "text": .string(""),
                         "userEdited": .bool(true)]),
            ]),
        ]))!
        XCTAssertEqual(page.revision, 3)
        XCTAssertEqual(page.blocks[0].source?.notePath, ".agents/notes/a.md")
        XCTAssertTrue(page.blocks[1].userEdited)
        XCTAssertEqual(page.markdown, "# 委派\n\n## Problem\n\n单干\n\n## Decision\n\n契约\n")
        XCTAssertNil(LiveWikiSource(kind: "memory", target: "mem-1", label: "经验").notePath)
    }

    /// R4-1: module pages — grouped by package before the decisions, their
    /// code sources open the file and say the versions read, and a link to
    /// another page names its slug.
    func testModulePagesGroupByPackageAndTheirSourcesAreFiles() async {
        let service = FakeWikiService()
        service.summaries = [
            LiveWikiPageSummary(slug: "index", kind: "index", title: "索引"),
            LiveWikiPageSummary(slug: "decision-a", kind: "decision", title: "A", category: "feature"),
            LiveWikiPageSummary(slug: "module-b", kind: "module", title: "b-cli", category: "b-cli"),
            LiveWikiPageSummary(slug: "module-a-x", kind: "module", title: "a-core · x", category: "a-core"),
        ]
        let store = WikiStore(service: service)
        store.show(scope: "p1")
        await store.reload()
        XCTAssertEqual(store.tree.top.map(\.slug), ["index"])
        XCTAssertEqual(store.tree.decisions.map(\.category), ["模块 · a-core", "模块 · b-cli", "决策 · feature"])
        XCTAssertEqual(WikiText.kind("module"), "模块")

        let block = LiveWikiBlock.decode(.object([
            "key": .string("module:core/b#overview"), "heading": .string("概览"), "text": .string("x"),
            "source": .object(["kind": .string("code"), "target": .string("core/b/src/main.rs"),
                               "label": .string("main.rs"), "version": .string("ce013625030ba8db")]),
            "sources": .array([
                .object(["kind": .string("code"), "target": .string("core/b/src/main.rs"),
                         "label": .string("main.rs"), "version": .string("ce013625030ba8db")]),
                .object(["kind": .string("code"), "target": .string("core/b/src/cli.rs"), "label": .string("cli.rs")]),
            ]),
        ]))!
        let source = block.source!
        XCTAssertEqual(source.notePath, "core/b/src/main.rs", "a code source opens its file")
        XCTAssertEqual(WikiText.sourceLabel(source, all: block.sources), "main.rs 等 2 个文件")
        XCTAssertEqual(WikiText.sourceHelp(source, all: block.sources),
                       "打开来源\ncore/b/src/main.rs @ ce01362\ncore/b/src/cli.rs")
        XCTAssertEqual(WikiText.linkedSlug(URL(string: "module-a-x.md")!), "module-a-x")
        XCTAssertNil(WikiText.linkedSlug(URL(string: "https://example.com/a.md")!))
        let page = LiveWikiPage.decode(.object(["slug": .string("module-b"), "kind": .string("module"),
                                               "title": .string("b-cli"), "sourceCommit": .string("beec1c6a4bb6")]))
        XCTAssertEqual(page?.sourceCommit, "beec1c6a4bb6")
    }

    /// R4-2: generating explanations, following the background job, and
    /// the line that says what is left and what it cost.
    func testExplanationsAreGeneratedAndFollowed() async {
        let service = FakeWikiService()
        service.summaries = [LiveWikiPageSummary(slug: "module-b", kind: "module", title: "b")]
        let store = WikiStore(service: service)
        store.show(scope: "p1", slug: "module-b")
        await store.reload()
        service.explainStates = [LiveWikiExplainState(running: true, total: 1, model: "m"),
                                 LiveWikiExplainState(running: false, done: 1, total: 1, model: "m")]
        await store.explain()
        XCTAssertTrue(service.calls.contains("explain module-b"))
        XCTAssertEqual(store.notice, "已生成说明", "followed to the end, not the request's answer")
        XCTAssertNil(store.errorText)
        service.explainStates = [LiveWikiExplainState(running: true, done: 1, total: 2, model: "m"),
                                 LiveWikiExplainState(running: false, done: 2, total: 2, error: "没有选连接", model: "m")]
        await store.explainAll()
        XCTAssertTrue(service.calls.contains("explainAll"))
        XCTAssertEqual(store.errorText, "生成说明中断：没有选连接")
        XCTAssertEqual(WikiText.explainLine(LiveWikiExplainState(running: true, done: 3, total: 60)), "生成说明中 3/60")
        XCTAssertEqual(WikiText.explainLine(LiveWikiExplainState(pending: 12, monthRuns: 3, monthInputTokens: 25_000,
                                                                monthOutputTokens: 1_100, model: "m")),
                       "12 个模块待生成说明 · 本月生成 3 次 · 26.1K tok")
        XCTAssertEqual(WikiText.explainLine(LiveWikiExplainState()), "没有选分析模型")
        let state = LiveWikiExplainState.decode(.object([
            "running": .bool(true), "done": .number("1"), "total": .number("4"), "pending": .number("3"),
            "monthInputTokens": .number("900"), "model": .string("gpt"),
        ]))
        XCTAssertEqual(state, LiveWikiExplainState(running: true, done: 1, total: 4, pending: 3,
                                                   monthInputTokens: 900, model: "gpt"))
    }

    /// R4-3: the per-project switch that keeps the Wiki tools from agents.
    func testAgentAccessIsReadAndSwitched() async {
        let service = FakeWikiService()
        service.access = false
        let store = WikiStore(service: service)
        store.show(scope: "p1")
        await store.reload()
        XCTAssertFalse(store.agentAccess)
        await store.setAgentAccess(true)
        XCTAssertTrue(store.agentAccess)
        XCTAssertTrue(service.calls.contains("access true"))
        XCTAssertEqual(store.notice, "Agent 可以读这个项目的 Wiki")
    }

    func testTheTreeGroupsDecisionsAndTheStoreActs() async {
        let service = FakeWikiService()
        service.summaries = [
            LiveWikiPageSummary(slug: "index", kind: "index", title: "索引"),
            LiveWikiPageSummary(slug: "runbook", kind: "runbook", title: "操作手册"),
            LiveWikiPageSummary(slug: "decision-b", kind: "decision", title: "B", category: "testing"),
            LiveWikiPageSummary(slug: "decision-a", kind: "decision", title: "A", category: "feature", edited: 1),
        ]
        service.pagesBySlug["index"] = LiveWikiPage(slug: "index", kind: "index", title: "索引")
        service.issueList = [LiveWikiIssue(id: "w1", slug: "decision-a", blockKey: "k", kind: "sourceChanged",
                                           text: "x", suggestion: "新"),
                             LiveWikiIssue(id: "w2", slug: "index", kind: "agent", text: "y", open: false)]
        let store = WikiStore(service: service)
        store.show(scope: "p1", slug: "gone")
        await store.reload()
        XCTAssertEqual(store.selected, "index", "an unknown page falls back to the first")
        XCTAssertEqual(store.page?.title, "索引")
        let tree = store.tree
        XCTAssertEqual(tree.top.map(\.slug), ["index", "runbook"])
        XCTAssertEqual(tree.decisions.map(\.category), ["决策 · feature", "决策 · testing"])
        XCTAssertEqual(store.openIssues.map(\.id), ["w1"])
        XCTAssertEqual(store.revisions.map(\.n), [2, 1])
        await store.compile()
        XCTAssertEqual(store.notice, "已编译：3 页 · 新增 2 · 新 issue 1")
        await store.save("# 索引\n")
        await store.revert(1)
        await store.resolve(store.openIssues[0], accept: true)
        let refused = await store.exportPlan("../outside")
        XCTAssertNil(refused)
        XCTAssertEqual(store.exportError, "导出目录必须是项目里的相对路径，且不能在 .git 里",
                       "a refused directory shows in the sheet, in plain words")
        XCTAssertNil(store.errorText, "not behind the sheet as an alert")
        let plan = await store.exportPlan("docs/wiki")
        XCTAssertNil(store.exportError)
        XCTAssertEqual(plan?.files, ["docs/wiki/index.md"])
        await store.export("docs/wiki")
        XCTAssertEqual(store.notice, "已导出 1 页到 docs/wiki/")
        for call in ["compile p1", "edit index", "revert 1", "issue w1 true", "export docs/wiki"] {
            XCTAssertTrue(service.calls.contains(call), call)
        }
        XCTAssertEqual(WikiText.report(LiveWikiReport(pages: 5)), "已编译：5 页 · 没有变化")
        XCTAssertEqual(WikiText.issueKind("sourceRemoved"), "来源已删除")
        XCTAssertEqual(WikiText.reason("revert"), "回到旧版")
        XCTAssertEqual(WikiText.kind("runbook"), "操作手册")
    }

    /// A read cancelled because the user clicked on is not an error; a real
    /// failure still says so, in words.
    func testACancelledReadIsNotAnError() async {
        let service = FakeWikiService()
        service.summaries = [LiveWikiPageSummary(slug: "index", kind: "index", title: "索引")]
        service.pageError = ConnectionError.cancelled
        let store = WikiStore(service: service)
        store.show(scope: "p1")
        await store.reload()
        XCTAssertNil(store.errorText)
        service.pageError = ConnectionError.timedOut
        await store.reload()
        XCTAssertEqual(store.errorText, "读取 Wiki 失败：宿主没有及时回应")
    }

    /// R1-1: what could not be read is said, on the pages it concerns.
    func testUnreadableSourcesAreSaid() async {
        let service = FakeWikiService()
        service.summaries = [LiveWikiPageSummary(slug: "decision-a", kind: "decision", title: "A")]
        service.gaps = LiveWikiSourceGaps(slugs: ["decision-a"])
        let store = WikiStore(service: service)
        store.show(scope: "p1")
        await store.reload()
        XCTAssertTrue(store.sourceUnavailable(slug: "decision-a", kind: "decision"))
        XCTAssertFalse(store.sourceUnavailable(slug: "index", kind: "index"))
        XCTAssertEqual(WikiText.gapNotice(store.gaps), "有 1 篇笔记读不到，相关页面保持上次内容")
        XCTAssertEqual(WikiText.gapNotice(LiveWikiSourceGaps(all: true)), "读不到项目文件夹，显示的是上次的内容。恢复后自动更新")
        XCTAssertNil(WikiText.gapNotice(LiveWikiSourceGaps()))
        XCTAssertEqual(WikiText.exportLine(LiveWikiExportPlan(files: ["a", "b", "c"], overwrites: ["a"])),
                       "将写入 3 个文件，其中 1 个会覆盖已有文件")
        XCTAssertEqual(WikiText.exportLine(LiveWikiExportPlan(files: ["a"])), "将写入 1 个文件")
        XCTAssertEqual(MemoryText.runState("partial"), "部分成功")
        XCTAssertEqual(WikiText.splitTitle("数据真实性（R1-1）：部分成功、按项目检索").main, "数据真实性（R1-1）")
        XCTAssertEqual(WikiText.splitTitle("索引").detail, nil)
        XCTAssertEqual(WikiText.section("Problem").title, "问题")
        XCTAssertEqual(WikiText.section("Alternatives considered").title, "考虑过的方案")
        XCTAssertEqual(WikiText.section("自定义").title, "自定义")
    }
}
