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
    func archive(id: String, archived: Bool) async throws { calls.append("archive \(id) \(archived)") }
    func pin(id: String, pinned: Bool) async throws { calls.append("pin \(id) \(pinned)") }
    func move(id: String, projectID: String?) async throws { calls.append("move \(id) \(projectID ?? "global")") }
    func forget(id: String) async throws { calls.append("forget \(id)") }
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
    func importNative(path: String, projectID: String?) async throws -> (imported: Int, skipped: Int) {
        calls.append("import \(path) \(projectID ?? "global")")
        return (3, 1)
    }
}

/// B5-1: the memory page's model and the words it shares with `/记住`.
@MainActor
final class MemoryTests: XCTestCase {
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
        await store.confirm(service.items[0], accept: false)
        await store.add("用 pnpm")
        XCTAssertTrue(service.calls.contains("remember global fact - 新的 a1"))
        XCTAssertTrue(service.calls.contains("confirm p1 false"))
        XCTAssertTrue(service.calls.contains("remember proj fact - 用 pnpm -"))
        XCTAssertEqual(service.calls.filter { $0.hasPrefix("list proj") }.count, 4, "each action reloads")
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
