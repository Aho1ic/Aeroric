import AeroricClient
import AeroricProtocol
@testable import AeroricUI
import XCTest

@MainActor
private final class FakeMemoryService: MemoryService {
    var items: [LiveMemoryItem] = []
    var enabled: Bool? = true
    var calls: [String] = []

    func list(projectID: String?, query: String?) async throws -> (items: [LiveMemoryItem], enabled: Bool?) {
        calls.append("list \(projectID ?? "global") \(query ?? "-")")
        return (items, projectID == nil ? nil : enabled)
    }
    func remember(projectID: String?, kind: String, category: String?, text: String,
                  supersedes: String?) async throws -> LiveMemoryItem {
        calls.append("remember \(projectID ?? "global") \(kind) \(category ?? "-") \(text) \(supersedes ?? "-")")
        return LiveMemoryItem(id: "m-new", projectId: projectID, kind: kind, text: text)
    }
    func confirm(id: String, accept: Bool) async throws { calls.append("confirm \(id) \(accept)") }
    var withdrawal: LiveMemoryWithdrawal?
    func archive(id: String, archived: Bool) async throws -> LiveMemoryWithdrawal? {
        calls.append("archive \(id) \(archived)")
        return archived ? withdrawal : nil
    }
    func pin(id: String, pinned: Bool) async throws { calls.append("pin \(id) \(pinned)") }
    func move(id: String, projectID: String?) async throws { calls.append("move \(id) \(projectID ?? "global")") }
    func forget(id: String) async throws -> LiveMemoryWithdrawal? {
        calls.append("forget \(id)")
        return withdrawal
    }
    func setEnabled(projectID: String, enabled: Bool) async throws { calls.append("enabled \(projectID) \(enabled)") }
    func native(projectID: String?) async throws -> [LiveNativeMemory] { [] }
    func attention() async throws -> [LiveMemoryItem] { items.filter { $0.state == "pending" } }
    var saved = LiveMemorySettings()
    func settings() async throws -> LiveMemorySettings { saved }
    func saveSettings(_ settings: LiveMemorySettings) async throws -> LiveMemorySettings {
        if settings.auto && !settings.hasModel { throw LiveSessionError.unsupported("先选模型") }
        saved = settings
        return settings
    }
    var page = LiveMemoryRunsPage()
    func runs() async throws -> LiveMemoryRunsPage { page }
    func extract(taskID: String) async throws -> LiveMemoryRun? { calls.append("extract \(taskID)"); return nil }
    func feedback(id: String, helpful: Bool) async throws { calls.append("feedback \(id) \(helpful)") }
    var skills: [LiveSkillProposal] = []
    func proposals() async throws -> [LiveSkillProposal] { skills }
    func editProposal(id: String, edit: LiveSkillEdit) async throws { calls.append("edit \(id) \(edit.global.map(String.init) ?? "-")") }
    func decideProposal(id: String, accept: Bool) async throws { calls.append("decide \(id) \(accept)") }
    func useSkillVersion(id: String, version: Int) async throws { calls.append("version \(id) \(version)") }
    func disableSkill(id: String, disabled: Bool) async throws { calls.append("disable \(id) \(disabled)") }
    func importNative(path: String, projectID: String?) async throws -> (imported: Int, skipped: Int) {
        calls.append("import \(path) \(projectID ?? "global")")
        return (3, 1)
    }
    var plan = LiveMemoryHistoryPlan()
    func historyPlan(projectID: String, sinceMs: UInt64?) async throws -> LiveMemoryHistoryPlan {
        calls.append("plan \(projectID) \(sinceMs.map(String.init) ?? "all")")
        return plan
    }
    var runFor: [String: LiveMemoryRun] = [:]
    func extractImported(id: String) async throws -> LiveMemoryRun? {
        calls.append("extractImported \(id)")
        guard let run = runFor[id] else { throw ConnectionError.io("今天已经整理了 30 次，到上限了，明天再整理") }
        return run
    }
    var dreams: [LiveDreamProposal] = []
    var dreamRun: LiveMemoryRun?
    func dream(projectID: String?) async throws -> LiveMemoryRun? {
        calls.append("dream \(projectID ?? "global")")
        return dreamRun
    }
    func dreamProposals(projectID: String?) async throws -> [LiveDreamProposal] { dreams }
    func decideDream(id: String, accept: Bool, keep: String?) async throws {
        calls.append("decideDream \(id) \(accept) \(keep ?? "-")")
    }
    func undoDream(runID: String) async throws { calls.append("undoDream \(runID)") }
    func revertDream(id: String) async throws { calls.append("revertDream \(id)") }
    func semanticStatus() async throws -> LiveSemanticStatus { LiveSemanticStatus(model: "bge-m3", embedded: 3, missing: 1) }
    var changes: [LiveCodeChange] = []
    func chain(id: String) async throws -> [LiveCodeChange] {
        calls.append("chain \(id)")
        return changes
    }
}

/// B5-1: the memory page's model and the words it shares with `/记住`.
@MainActor
final class MemoryTests: XCTestCase {
    /// C2 (2026-11-20): 归档 and 永久遗忘 leave a note of what else they
    /// reached; restoring, or reaching nothing else, leaves none, and the
    /// next action clears the last note.
    func testArchivingAndForgettingSayWhatElseTheyReached() async {
        let service = FakeMemoryService()
        let item = LiveMemoryItem(id: "m1", projectId: "p1", kind: "experience", text: "先跑 lint")
        service.items = [item]
        let store = MemoryStore(service: service)

        service.withdrawal = LiveMemoryWithdrawal(items: 1, skills: ["lint-first"], wikiPages: 1)
        await store.archive(item, true)
        XCTAssertEqual(store.withdrawalNote?.title, "已归档")
        XCTAssertEqual(store.withdrawalNote?.lines.count, 2)
        XCTAssertEqual(store.withdrawalNote?.footnote, "你导出到项目目录的 Wiki 文件不会被改动。")

        await store.archive(item, false)
        XCTAssertNil(store.withdrawalNote, "restoring says nothing")

        service.withdrawal = LiveMemoryWithdrawal(items: 3, wikiFlagged: 1,
                                                  failed: ["项目「demo」的 Wiki 没能更新：磁盘已满"])
        await store.forget(item)
        let note = store.withdrawalNote
        XCTAssertEqual(note?.title, "已永久遗忘")
        XCTAssertEqual(note?.lines.first, "连同旧版本和合并进来的重复项共删除 3 条")
        XCTAssertTrue(note?.failed.first?.hasPrefix("项目「demo」的 Wiki 没能更新：磁盘已满") ?? false)
        XCTAssertTrue(note?.footnote.contains("修订历史") ?? false)

        service.withdrawal = LiveMemoryWithdrawal(items: 1)
        await store.forget(item)
        XCTAssertNil(store.withdrawalNote, "nothing else reached, no note")
        XCTAssertEqual(service.calls.filter { $0.hasPrefix("archive") || $0.hasPrefix("forget") },
                       ["archive m1 true", "archive m1 false", "forget m1", "forget m1"])
    }

    /// C2: the withdrawal, the Wiki backlog and a skill's 依据已撤回 decode
    /// from the host's camelCase; an older host's reply decodes to nothing.
    func testWithdrawalBacklogAndWithdrawnSkillsDecode() {
        let withdrawal = LiveMemoryWithdrawal.decode(.object([
            "items": .number("2"), "skills": .array([.string("lint-first")]), "dreams": .number("1"),
            "wikiPages": .number("3"), "wikiFlagged": .number("1"),
            "failed": .array([.string("项目「demo」的 Wiki 没能更新：x")]),
        ]))
        XCTAssertEqual(withdrawal, LiveMemoryWithdrawal(items: 2, skills: ["lint-first"], dreams: 1, wikiPages: 3,
                                                        wikiFlagged: 1, failed: ["项目「demo」的 Wiki 没能更新：x"]))
        XCTAssertEqual(LiveMemoryWithdrawal.decode(.object(["items": .number("1")])), LiveMemoryWithdrawal(items: 1))
        XCTAssertNil(LiveMemoryWithdrawal.decode(nil))

        let row = LiveWikiBacklog.decode(.object([
            "projectId": .string("p1"), "ids": .array([.string("m1"), .string("m2")]),
            "forgotten": .bool(true), "reason": .string("Is a directory"), "atMs": .number("5"),
        ]))
        XCTAssertEqual(row, LiveWikiBacklog(projectId: "p1", ids: ["m1", "m2"], forgotten: true,
                                            reason: "Is a directory", atMs: 5))
        XCTAssertEqual(row.map(MemoryText.wikiBacklog), "Wiki 还留着 2 条已遗忘记忆的段落")
        XCTAssertEqual(row.map { MemoryText.wikiBacklogDetail($0, project: "demo") },
                       "demo · 上次失败：Is a directory · 会自动重试")

        let skill = LiveSkillProposal.decode(.object([
            "id": .string("s1"), "name": .string("lint-first"), "state": .string("accepted"),
            "sourceWithdrawnMs": .number("9"),
        ]))
        XCTAssertEqual(skill?.sourceWithdrawnMs, 9)
        XCTAssertEqual(skill?.state, "accepted", "the skill keeps its state")
        XCTAssertNil(LiveSkillProposal.decode(.object(["id": .string("s2"), "name": .string("x")]))?.sourceWithdrawnMs)
        XCTAssertEqual(WikiText.reason("withdraw"), "记忆撤回")
    }

    /// B6 (2026-11-13): a dream's row and proposals decode, and say what
    /// they did; only a dream that changed something can be undone, once.
    func testADreamRowAndItsProposalsDecodeAndSayWhatTheyDid() {
        let run = LiveMemoryRun.decode(.object([
            "id": .string("run-1"), "taskId": .string(""), "title": .string("trainer"), "state": .string("done"),
            "kind": .string("dream"), "proposed": .number("1"),
            "retired": .array([.object(["id": .string("mem-2"), "keep": .string("mem-1")]),
                               .object(["id": .string("mem-3"), "keep": .string("mem-1")])]),
        ]))!
        XCTAssertTrue(run.isDream)
        XCTAssertEqual(run.merged, 2)
        XCTAssertEqual(MemoryText.runTitle(run), "整理记忆 · trainer")
        XCTAssertEqual(MemoryText.runOutput(run), "合并 2 条 · 待确认 1 条建议")
        XCTAssertTrue(MemoryText.canUndo(run))
        var undone = run
        undone.undoneMs = 9
        XCTAssertFalse(MemoryText.canUndo(undone))
        let nothing = LiveMemoryRun(id: "r", state: "done", kind: "dream")
        XCTAssertEqual(MemoryText.dreamResult(nothing), "没有发现重复、矛盾或过时")
        XCTAssertFalse(MemoryText.canUndo(nothing))
        XCTAssertFalse(LiveMemoryRun(id: "x", state: "done", facts: 1).isDream)

        let proposal = LiveDreamProposal.decode(.object([
            "id": .string("dream-1"), "runId": .string("run-1"), "kind": .string("fact"),
            "relation": .string("conflict"), "ids": .array([.string("a"), .string("b")]),
            "keep": .string("b"), "reason": .string("缩进只能一种"), "state": .string("pending"),
        ]))!
        XCTAssertEqual(proposal.ids, ["a", "b"])
        XCTAssertEqual(MemoryText.dreamHeading(proposal), "这几条互相矛盾，留哪一条？")
    }

    /// B6: 代码版本链 rows decode and read as a place and a line.
    func testCodeChangesDecodeAndTheStoreLoadsThemForAMemory() async {
        let change = LiveCodeChange.decode(.object([
            "path": .string("src/ssh.rs"), "symbol": .string("connect"), "branch": .string("main"),
            "taskId": .string("t1"), "turnId": .string("u1"), "atMs": .number("0"),
            "additions": .number("3"), "deletions": .number("1"), "state": .string("undone"),
            "diff": .string("@@ -1 +1 @@"),
        ]))!
        XCTAssertEqual(MemoryText.changePlace(change), "src/ssh.rs::connect")
        XCTAssertTrue(MemoryText.changeDetail(change).hasPrefix("main · +3 −1 · 已撤销"))
        XCTAssertEqual(MemoryText.changePlace(LiveCodeChange(path: "logo.png")), "logo.png")

        let typed = LiveMemoryItem(id: "a", text: "x")
        let fromSession = LiveMemoryItem(id: "b", text: "y", evidence: [LiveMemoryEvidence(taskId: "t1", excerpt: "y")])
        XCTAssertFalse(MemoryText.hasChain(typed))
        XCTAssertTrue(MemoryText.hasChain(fromSession))
        let service = FakeMemoryService()
        service.changes = [change]
        let store = MemoryStore(service: service)
        await store.loadChain(fromSession)
        XCTAssertEqual(store.chain, [change])
        XCTAssertTrue(service.calls.contains("chain b"))
    }

    /// B6 second batch: a stale and a rewrite proposal decode and say what
    /// they ask; accepted ones can be taken back; 需要我处理 gets one row per
    /// scope, newest first.
    func testStaleAndRewriteProposalsSayWhatTheyAskAndJoinTheAttentionQueue() async {
        let stale = LiveDreamProposal.decode(.object([
            "id": .string("d-s"), "kind": .string("fact"), "relation": .string("stale"),
            "ids": .array([.string("a")]), "keep": .string("a"), "state": .string("pending"),
            "missing": .array([.string("src/ssh.rs"), .string("connect_timeout")]),
        ]))!
        XCTAssertEqual(stale.missing, ["src/ssh.rs", "connect_timeout"])
        XCTAssertEqual(MemoryText.dreamMissing(stale), "找不到：src/ssh.rs、connect_timeout")
        XCTAssertEqual(MemoryText.dreamHeading(stale), "这条可能过时了：提到的文件和代码都找不到了")
        let compress = LiveDreamProposal.decode(.object([
            "id": .string("d-c"), "kind": .string("profile"), "relation": .string("compress"),
            "ids": .array([.string("p1"), .string("p2"), .string("p3")]), "state": .string("accepted"),
            "decidedMs": .number("9"),
            "rewrite": .array([.object(["text": .string("偏好甲乙"), "category": .string("preference")])]),
        ]))!
        XCTAssertEqual(compress.rewrite, [LiveDreamRewrite(text: "偏好甲乙", category: "preference")])
        XCTAssertEqual(compress.decidedMs, 9)
        XCTAssertEqual(MemoryText.dreamHeading(compress), "关于你的条目太长，新会话带不全，改写成 1 条？")
        XCTAssertEqual(MemoryText.dreamDone(compress), "已把 3 条改写成 1 条")

        let service = FakeMemoryService()
        service.dreams = [stale, compress]
        let store = MemoryStore(service: service)
        store.show(scope: "p", tab: .about)
        await store.reload()
        XCTAssertEqual(store.acceptedDreams(.about).map(\.id), ["d-c"])
        XCTAssertTrue(store.acceptedDreams(.context).isEmpty)
        XCTAssertEqual(store.pendingDreams(.context).map(\.id), ["d-s"])
        await store.revertDream(compress)
        XCTAssertTrue(service.calls.contains("revertDream d-c"))

        let rows = AttentionQueue.dreamRows([
            LiveDreamProposal(id: "1", projectId: "p", kind: "fact", createdMs: 5),
            LiveDreamProposal(id: "2", projectId: "p", kind: "experience", createdMs: 7),
            LiveDreamProposal(id: "3", kind: "profile", createdMs: 6),
            LiveDreamProposal(id: "4", projectId: "q", state: "rejected", createdMs: 9),
        ])
        XCTAssertEqual(rows.map(\.id), ["dream#p", "dream#global"])
        XCTAssertEqual(rows[0].count, 2)
        XCTAssertEqual(rows[0].kind, "experience", "opens the newest one's tab")
        XCTAssertEqual(MemoryText.dreamAttention(rows[0].count), "记忆整理待确认 2 条")
    }

    /// B6 可选 embedding: the switch round-trips, and the status line says
    /// how far the vectors are, or why they are not.
    func testTheSemanticSwitchRoundTripsAndItsLineSaysHowFarItIs() async {
        let on = LiveMemorySettings.decode(.object(["semantic": .bool(true)]))
        XCTAssertTrue(on.semantic)
        XCTAssertEqual(on.json.objectValue?["semantic"], .bool(true))
        XCTAssertFalse(LiveMemorySettings.decode(.object([:])).semantic, "off by default")
        let status = LiveSemanticStatus.decode(.object([
            "status": .object(["model": .string("bge-m3"), "embedded": .number("12"), "missing": .number("2")]),
            "model": .string("bge-m3"),
        ]))
        XCTAssertEqual(MemoryText.semanticLine(status), "bge-m3 · 已算 12 条 · 还差 2 条")
        XCTAssertEqual(MemoryText.semanticLine(LiveSemanticStatus(configured: "nomic-embed-text")),
                       "打开后在后台用随手记的 embedding 模型（nomic-embed-text）给记忆算向量")
        XCTAssertEqual(MemoryText.semanticLine(LiveSemanticStatus(model: "x", error: "连不上")), "向量没算成：连不上")
        let store = MemoryStore(service: FakeMemoryService())
        await store.loadSemantic()
        XCTAssertEqual(store.semantic?.embedded, 3)
    }

    func testDreamProposalsWaitInTheirTabAndTheStoreSendsTheChoice() async {
        let service = FakeMemoryService()
        service.items = [LiveMemoryItem(id: "a", projectId: "p", kind: "fact", text: "缩进用 tab"),
                         LiveMemoryItem(id: "b", projectId: "p", kind: "fact", text: "缩进用空格")]
        service.dreams = [LiveDreamProposal(id: "d1", kind: "fact", relation: "conflict", ids: ["a", "b"], keep: "b"),
                          LiveDreamProposal(id: "d2", kind: "fact", ids: ["a", "b"], state: "rejected")]
        service.dreamRun = LiveMemoryRun(id: "run-1", state: "done", kind: "dream", merged: 1, proposed: 1)
        let store = MemoryStore(service: service)
        store.show(scope: "p", tab: .context)
        await store.reload()
        XCTAssertEqual(store.pendingDreams(.context).map(\.id), ["d1"])
        XCTAssertTrue(store.pendingDreams(.about).isEmpty)
        XCTAssertEqual(store.count(.context), 3, "two items and one proposal")
        XCTAssertEqual(store.item("a")?.text, "缩进用 tab")

        await store.dream()
        XCTAssertEqual(store.dreamNote, "合并 1 条 · 待确认 1 条建议")
        await store.decideDream(store.pendingDreams(.context)[0], keep: "a", accept: true)
        await store.undoDream(service.dreamRun!)
        XCTAssertTrue(service.calls.contains("dream p"))
        XCTAssertTrue(service.calls.contains("decideDream d1 true a"))
        XCTAssertTrue(service.calls.contains("undoDream run-1"))
    }

    func testRememberCommandNeedsTextAfterIt() {
        XCTAssertEqual(MemoryText.rememberCommand("/记住 回复用中文"), "回复用中文")
        XCTAssertEqual(MemoryText.rememberCommand("/记住：只在 main 工作"), "只在 main 工作")
        XCTAssertEqual(MemoryText.rememberCommand("  /记住\u{3000}用 pnpm "), "用 pnpm")
        XCTAssertNil(MemoryText.rememberCommand("/记住"))
        XCTAssertNil(MemoryText.rememberCommand("/记住了吗"))
        XCTAssertNil(MemoryText.rememberCommand("请记住这个"))
        XCTAssertEqual(MemoryText.guessKind("回复一律用简体中文"), "profile")
        XCTAssertEqual(MemoryText.guessKind("core 依赖 dbx"), "fact")
    }

    func testAnItemDecodesFromTheWire() {
        let item = LiveMemoryItem.decode(.object([
            "id": .string("mem-1"), "projectId": .string("p"), "kind": .string("profile"),
            "category": .string("language"), "text": .string("中文"), "assertion": .string("agentConfirmed"),
            "state": .string("active"), "pinned": .bool(true), "updatedMs": .number("7"),
            "evidence": .array([.object(["taskId": .string("t"), "harness": .string("codex"),
                                         "excerpt": .string("原话"), "hash": .string("ab"), "atMs": .number("3")])]),
        ]))
        XCTAssertEqual(item?.category, "language")
        XCTAssertEqual(item?.pinned, true)
        XCTAssertEqual(item?.proposer, "codex")
        XCTAssertEqual(item?.evidence.first?.excerpt, "原话")
        XCTAssertNil(LiveMemoryItem.decode(.object(["id": .string("x")])))
    }

    func testSourceAndStateWords() {
        let said = LiveMemoryItem(id: "a", text: "x")
        XCTAssertEqual(MemoryText.source(said), "你说的")
        var confirmed = LiveMemoryItem(id: "b", text: "x", assertion: "agentConfirmed",
                                       evidence: [LiveMemoryEvidence(harness: "claude", excerpt: "x")])
        XCTAssertEqual(MemoryText.source(confirmed), "Claude 提议 · 你确认")
        confirmed.assertion = "modelInferred"
        XCTAssertEqual(MemoryText.source(confirmed), "Claude 提议")
        XCTAssertEqual(MemoryText.state(LiveMemoryItem(id: "c", text: "x", state: "superseded")), "已取代")
        XCTAssertEqual(MemoryText.category("constraint"), "限制")
        XCTAssertEqual(MemoryText.attention(said), "记忆待确认：x")
    }

    func testTheStoreSplitsPendingActiveAndHistoryPerTab() async {
        let service = FakeMemoryService()
        service.items = [
            LiveMemoryItem(id: "p1", kind: "fact", text: "提议", assertion: "modelInferred", state: "pending"),
            LiveMemoryItem(id: "a1", kind: "fact", text: "旧的", updatedMs: 5),
            LiveMemoryItem(id: "a2", kind: "fact", text: "置顶", pinned: true, updatedMs: 1),
            LiveMemoryItem(id: "h1", kind: "fact", text: "被取代", state: "superseded"),
            LiveMemoryItem(id: "u1", kind: "profile", text: "中文"),
        ]
        let store = MemoryStore(service: service)
        store.show(scope: "proj", tab: .context)
        await store.reload()
        await store.refreshPending()
        let shown = store.shown
        XCTAssertEqual(shown.pending.map(\.id), ["p1"])
        XCTAssertEqual(shown.active.map(\.id), ["a2", "a1"], "pinned first")
        XCTAssertEqual(shown.history.map(\.id), ["h1"])
        XCTAssertEqual(store.count(.context), 3)
        XCTAssertEqual(store.count(.about), 1)
        XCTAssertEqual(store.count(.history), 0)
        XCTAssertEqual(store.enabled, true)
        XCTAssertEqual(store.pending.map(\.id), ["p1"])
        XCTAssertTrue(MemoryTab.experience.modelWritten)

        await store.edit(service.items[1], to: "新的")
        await store.reload()
        await store.confirm(service.items[0], accept: false)
        XCTAssertFalse(store.shown.pending.contains { $0.id == "p1" }, "settled here, without a reload")
        await store.add("用 pnpm")
        XCTAssertTrue(service.calls.contains("remember global fact - 新的 a1"))
        XCTAssertTrue(service.calls.contains("confirm p1 false"))
        XCTAssertTrue(service.calls.contains("remember proj fact - 用 pnpm -"))
        XCTAssertEqual(service.calls.filter { $0.hasPrefix("list proj") }.count, 4, "a decision does not reload")
    }

    /// 2026-11-02: imports group by file; one group can be decided alone.
    func testPendingGroupByWhereTheyCameFrom() async {
        let service = FakeMemoryService()
        let from = { (file: String, n: Int) in
            LiveMemoryItem(id: "\(file)\(n)", kind: "fact", text: "行 \(n)", assertion: "modelInferred", state: "pending",
                           evidence: [LiveMemoryEvidence(harness: "claude", excerpt: "\(file) · 行 \(n)")])
        }
        service.items = [from("CLAUDE.md", 1), from("CLAUDE.md", 2), from("AGENTS.md", 1)]
        let store = MemoryStore(service: service)
        store.show(scope: "proj", tab: .context)
        await store.reload()
        XCTAssertEqual(Set(store.shown.pending.map(MemoryText.pendingGroup)), ["Claude · CLAUDE.md", "Claude · AGENTS.md"])
        await store.confirmAll(accept: false, group: "Claude · CLAUDE.md")
        XCTAssertEqual(store.shown.pending.map(\.id), ["AGENTS.md1"])
    }

    /// 2026-11-02: thousands of imported candidates, decided in one go.
    func testAllPendingAreDecidedTogether() async {
        let service = FakeMemoryService()
        service.items = (0..<300).map {
            LiveMemoryItem(id: "p\($0)", kind: "fact", text: "候选 \($0)", assertion: "modelInferred", state: "pending")
        }
        let store = MemoryStore(service: service)
        store.show(scope: "proj", tab: .context)
        await store.reload()
        XCTAssertEqual(store.shown.pending.count, 300)
        XCTAssertEqual(store.count(.context), 300)
        await store.confirmAll(accept: true)
        XCTAssertTrue(store.shown.pending.isEmpty)
        XCTAssertEqual(store.shown.active.count, 300)
        XCTAssertNil(store.busyText)
    }

    /// B5-2: 整理设置, 整理记录 and 导入为候选.
    func testTheExtractionSettingsRunsAndImport() async {
        let service = FakeMemoryService()
        service.page = LiveMemoryRunsPage(runs: [
            LiveMemoryRun(id: "r1", taskId: "t1", title: "修超时", atMs: 0, state: "done", model: "flash",
                          inputTokens: 5_800, outputTokens: 300, costUsd: 0.0012, facts: 1, candidates: 2),
            LiveMemoryRun(id: "r2", taskId: "t2", title: "试一下", state: "skipped", reason: "项目没有开启记忆"),
        ], usedToday: 1, dailyLimit: 30, monthCount: 18, monthCostUsd: 0.04)
        let store = MemoryStore(service: service)
        store.show(scope: "proj", tab: .log)
        await store.reload()
        XCTAssertEqual(store.runsPage.runs.count, 2)
        XCTAssertEqual(MemoryText.runsSummary(store.runsPage), "本月 18 次 · 约 $0.04 · 今天 1/30")
        XCTAssertTrue(MemoryText.runLine(store.runsPage.runs[0]).hasSuffix("修超时 · flash · 6.1k tok · 约 $0.0012"))
        XCTAssertTrue(MemoryText.runLine(store.runsPage.runs[1]).hasSuffix("试一下 · 项目没有开启记忆"))
        XCTAssertEqual(MemoryText.runState("failed"), "失败")
        // CU F2/F3: the details stand on their own; nothing costs "$-0".
        XCTAssertEqual(MemoryText.costText(-0.0), "$0")
        XCTAssertEqual(MemoryText.runOutput(LiveMemoryRun(id: "o", state: "done", facts: 1, candidates: 1, experiences: 1)),
                       "1 线索 · 1 候选 · 1 经验")
        XCTAssertEqual(LiveMemoryRun.decode(.object(["id": .string("d"), "state": .string("done"),
                                                     "experiences": .number("2")]))?.experiences, 2)
        XCTAssertEqual(MemoryText.runDetail(LiveMemoryRun(id: "x", state: "done", model: "cline-pass/deepseek-v4.1-flash",
                                                          inputTokens: 493, outputTokens: 1008, costUsd: 0.000905)),
                       "deepseek-v4.1-flash · 1.5k tok · 约 $0.0009")
        XCTAssertNil(MemoryText.runDetail(store.runsPage.runs[1]))
        XCTAssertEqual(MemoryText.runReason(LiveMemoryRun(id: "f", state: "failed",
                                                          reason: "调用分析模型失败：HTTP 500: request failed")),
                       "HTTP 500: request failed")
        let refused = await store.saveSettings(LiveMemorySettings(auto: true))
        XCTAssertFalse(refused)
        XCTAssertNotNil(store.errorText)
        let saved = await store.saveSettings(LiveMemorySettings(auto: true, connectionId: "c1", model: "flash"))
        XCTAssertTrue(saved)
        XCTAssertEqual(MemoryText.modelTitle(store.settings,
                                             choices: [LiveMemoryModelChoice(connectionId: "c1", connection: "cline", model: "flash")]),
                       "cline · flash")
        let file = LiveNativeMemory(harness: "claude", path: "/m/MEMORY.md", title: "MEMORY.md", text: "- a")
        await store.importNative(file)
        XCTAssertEqual(store.importNotes["/m/MEMORY.md"], "导入 3 条，跳过 1 条（已有或已遗忘）")
        XCTAssertTrue(service.calls.contains("import /m/MEMORY.md proj"))
        await store.retry(store.runsPage.runs[0])
        XCTAssertTrue(service.calls.contains("extract t1"))
        XCTAssertEqual(MemoryTab.history.kind, "sessionFact")
        XCTAssertNil(MemoryTab.history.later)
        XCTAssertEqual(MemoryText.source(LiveMemoryItem(id: "s", kind: "sessionFact", text: "x",
                                                        assertion: "modelInferred")), "模型整理")
        let run = LiveMemoryRun.decode(.object(["id": .string("r"), "state": .string("done"),
                                                "costUsd": .number("0.5"), "facts": .number("1")]))
        XCTAssertEqual(run?.costUsd, 0.5)
        XCTAssertEqual(run?.facts, 1)
    }

    /// B5-3a: 学会的经验 — confidence, counts, votes.
    func testExperiencesShowTrustCountsAndTakeVotes() async {
        let item = LiveMemoryItem.decode(.object([
            "id": .string("mem-e"), "kind": .string("experience"), "text": .string("SSH 超时修法"),
            "assertion": .string("modelInferred"), "confidence": .string("medium"),
            "experience": .object(["problem": .string("卡住"), "solution": .string("connect_timeout"),
                                   "searchKeys": .array([.string("ssh")]), "envs": .array([.string("macOS")])]),
            "stats": .object(["retrieved": .number("6"), "helpful": .number("4"), "wrong": .number("1")]),
        ]))!
        XCTAssertEqual(item.experience?.envs, ["macOS"])
        XCTAssertEqual(MemoryText.confidence(item.confidence), "中")
        XCTAssertEqual(MemoryText.stats(item), "检索 6 · 有用 4 · 不对 1")
        let help = MemoryText.help(item)
        XCTAssertTrue(help.contains("解法：connect_timeout"))
        XCTAssertTrue(help.contains("适用：macOS"))
        XCTAssertEqual(MemoryText.source(item), "模型整理")
        let injected = LiveMemoryItem(id: "p", text: "x", stats: LiveMemoryStats(retrieved: 3, injected: 12))
        XCTAssertEqual(MemoryText.stats(injected), "注入 12 · 检索 3 · 有用 0")
        let service = FakeMemoryService()
        service.items = [item]
        let store = MemoryStore(service: service)
        store.show(scope: "p1", tab: .experience)
        await store.reload()
        XCTAssertEqual(store.shown.active.map(\.id), ["mem-e"])
        XCTAssertEqual(store.count(.experience), 1)
        await store.feedback(item, helpful: false)
        XCTAssertTrue(service.calls.contains("feedback mem-e false"))
        XCTAssertTrue(MemoryTab.history.modelWritten)
        XCTAssertFalse(MemoryTab.about.modelWritten)
    }

    /// B5-3b: 技能提案 — groups, badges, actions, the attention row.
    func testSkillProposalsGroupActAndBadgeTheirExperience() async {
        let pending = LiveSkillProposal.decode(.object([
            "id": .string("skp-1"), "experienceId": .string("mem-e"), "taskId": .string("t1"),
            "name": .string("learned-4a908e"), "description": .string("CSV 带 BOM"), "state": .string("pending"),
            "global": .bool(false), "versions": .array([]),
        ]))!
        let accepted = LiveSkillProposal(id: "skp-2", experienceId: "mem-f", name: "learned-1c77f0", state: "accepted",
                                         versions: [LiveSkillVersion(n: 1), LiveSkillVersion(n: 2)], activeVersion: 2)
        let service = FakeMemoryService()
        service.skills = [pending, accepted,
                          LiveSkillProposal(id: "skp-3", name: "learned-x", state: "rejected")]
        let store = MemoryStore(service: service)
        store.show(scope: "p1", tab: .skills)
        await store.reload()
        let groups = store.proposalGroups
        XCTAssertEqual(groups.pending.map(\.id), ["skp-1"])
        XCTAssertEqual(groups.accepted.map(\.id), ["skp-2"])
        XCTAssertEqual(groups.closed.map(\.id), ["skp-3"])
        XCTAssertEqual(store.count(.skills), 1)
        XCTAssertEqual(MemoryText.promotion(store.proposal(for: "mem-e")), "已提议升格")
        XCTAssertEqual(MemoryText.promotion(store.proposal(for: "mem-f")), "已成技能")
        XCTAssertNil(MemoryText.promotion(store.proposal(for: "mem-x")))
        XCTAssertEqual(MemoryText.proposalState("disabled"), "已停用")
        XCTAssertTrue(MemoryText.skillAttention(pending).hasPrefix("技能提案待审：learned-4a908e"))
        await store.decide(pending, accept: true)
        await store.editProposal(accepted, LiveSkillEdit(global: true))
        await store.useVersion(accepted, 1)
        await store.disable(accepted, true)
        XCTAssertTrue(service.calls.contains("decide skp-1 true"))
        XCTAssertTrue(service.calls.contains("edit skp-2 true"))
        XCTAssertTrue(service.calls.contains("version skp-2 1"))
        XCTAssertTrue(service.calls.contains("disable skp-2 true"))
        XCTAssertEqual(LiveSkillEdit(description: "x", global: false).json,
                       .object(["description": .string("x"), "global": .bool(false)]))
        XCTAssertTrue(MemoryTab.skills.modelWritten)
        let task = TaskStore.TaskEntry(id: "t1", projectID: "p", title: "t", status: "done", agent: "codex",
                                       createdAtMs: 1, updatedAtMs: 1, archived: false, starred: false)
        let items = AttentionQueue.items([task], skills: ["t1": AttentionQueue.CheckFailure(summary: "x", atMs: 2)], nowMs: 3)
        XCTAssertEqual(items.map(\.reason), [.skillProposalNeedsYou])
        // CU 2026-10-31: a pending memory from the same session no longer hides it.
        let both = AttentionQueue.items([task],
                                        memories: ["t1": AttentionQueue.CheckFailure(summary: "m", atMs: 1)],
                                        skills: ["t1": AttentionQueue.CheckFailure(summary: "s", atMs: 2)], nowMs: 3)
        XCTAssertEqual(both.map(\.reason), [.skillProposalNeedsYou, .memoryNeedsYou])
        XCTAssertEqual(Set(both.map(\.id)).count, 2)
    }

    /// Q4: the plan says why it cannot start; a run sends only what is not
    /// done, no more than today allows, and stops at the host's limit.
    func testHistoryRunSendsWhatIsLeftAndStopsAtTheLimit() async {
        let service = FakeMemoryService()
        let store = MemoryStore(service: service)
        var plan = LiveMemoryHistoryPlan(
            sessions: [.init(id: "a"), .init(id: "b", done: true), .init(id: "c"), .init(id: "d")],
            pending: 3, inputTokens: 12_000, outputTokens: 1_200, costUsd: 0.02, remainingToday: 2,
            model: nil, enabled: true)
        XCTAssertEqual(MemoryText.historyBlocker(plan), "还没有选分析模型：先在“整理设置”里选一个")
        plan.model = "flash"
        XCTAssertNil(MemoryText.historyBlocker(plan))
        XCTAssertEqual(MemoryText.historyCount(plan), "3 个会话待整理 · 1 个整理过，跳过")
        XCTAssertEqual(MemoryText.historyLimit(plan), "今天还能整理 2 次，其余 1 个明天再来")
        service.plan = plan
        service.runFor = ["a": LiveMemoryRun(id: "r1", state: "done", facts: 1, candidates: 2)]
        await store.planHistory(projectID: "p1", sinceMs: nil)
        XCTAssertEqual(store.historyPlan, plan)
        store.startHistory()
        while store.history.running { await Task.yield() }
        XCTAssertEqual(service.calls.filter { $0.hasPrefix("extractImported") }, ["extractImported a", "extractImported c"])
        XCTAssertEqual(store.history.written, 3)
        XCTAssertEqual(store.history.finished, 1)
        XCTAssertEqual(store.history.note, "今天已经整理了 30 次，到上限了，明天再整理")
    }

    func testAMemoryProposalJoinsTheAttentionQueue() {
        let task = TaskStore.TaskEntry(id: "t", projectID: "p", title: "t", status: "done", agent: "claude",
                                       createdAtMs: 1, updatedAtMs: 1, archived: false, starred: false)
        let items = AttentionQueue.items([task],
                                         memories: ["t": AttentionQueue.CheckFailure(summary: "记忆待确认：x", atMs: 9)],
                                         nowMs: 10)
        XCTAssertEqual(items.map(\.reason), [.memoryNeedsYou])
        XCTAssertEqual(AttentionQueue.Reason.memoryNeedsYou.text, "记忆待确认")
    }
}
