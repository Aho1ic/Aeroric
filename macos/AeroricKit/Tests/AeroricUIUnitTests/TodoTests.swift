import AeroricProtocol
@testable import AeroricUI
import XCTest

@MainActor
private final class FakeTodoService: TodoService {
    var todos: [LiveTodo] = []
    var calls: [String] = []
    var refuse: String?
    /// B2-7: how long a save takes (nanoseconds).
    var pause: UInt64 = 0

    func list() async throws -> [LiveTodo] { todos }
    func save(_ todo: LiveTodo) async throws -> LiveTodo {
        if pause > 0 { try await Task.sleep(nanoseconds: pause) }
        if let refuse { throw NSError(domain: "x", code: 1, userInfo: [NSLocalizedDescriptionKey: refuse]) }
        calls.append("save \(todo.title)")
        var saved = todo
        if saved.id.isEmpty { saved.id = "todo-1" }
        todos.removeAll { $0.id == saved.id }
        todos.append(saved)
        return saved
    }
    func delete(id: String) async throws {
        calls.append("delete \(id)")
        todos.removeAll { $0.id == id }
    }
    func assign(id: String, to runner: LiveTodoRunner) async throws -> (LiveTodo, String) {
        let agent = runner.agent
        calls.append("assign \(id) \(agent)" + (runner.configurationId.map { " \($0)" } ?? "")
                     + (runner.model.map { " \($0)" } ?? ""))
        guard let i = todos.firstIndex(where: { $0.id == id }) else { throw NSError(domain: "x", code: 2) }
        todos[i].state = "assigned"
        todos[i].progress = "running"
        todos[i].assignments.append(LiveTodoAssignment(taskId: "task-9", agent: agent))
        return (todos[i], "task-9")
    }
    func decide(id: String, decision: String) async throws -> LiveTodo {
        calls.append("\(decision) \(id)")
        guard let i = todos.firstIndex(where: { $0.id == id }) else { throw NSError(domain: "x", code: 2) }
        todos[i].state = decision == "accept" ? "done" : decision == "cancel" ? "cancelled"
            : decision == "take" ? "open" : "assigned"
        return todos[i]
    }
    func check(id: String) async throws -> LiveTodo {
        calls.append("check \(id)")
        guard let i = todos.firstIndex(where: { $0.id == id }) else { throw NSError(domain: "x", code: 2) }
        todos[i].assignments[todos[i].assignments.count - 1].check = LiveTodoCheck(state: "running")
        return todos[i]
    }
}

/// 2026-11-15 工作安排第一批: 待办.
@MainActor
final class TodoTests: XCTestCase {
    /// A todo reads off the wire with its hand-outs; a save sends only its words.
    func testATodoDecodesAndASaveSendsOnlyItsWords() {
        let record: JSONValue = .object([
            "id": .string("todo-1"), "projectId": .string("p"), "title": .string("修复登录超时"),
            "notes": .string("先看 issue #12"), "state": .string("assigned"),
            "assignments": .array([.object(["taskId": .string("t1"), "agent": .string("codex"), "atMs": .number("5")]),
                                   .object(["taskId": .string("t2"), "agent": .string("claude"), "atMs": .number("9")])]),
            "progress": .string("notRun"), "progressDetail": .string("额度用完"),
        ])
        let todo = LiveTodo.decode(record)
        XCTAssertEqual(todo?.latestTaskId, "t2")
        XCTAssertEqual(todo?.assignments.count, 2)
        XCTAssertEqual(todo?.progressDetail, "额度用完")
        let sent = todo?.json.objectValue
        XCTAssertEqual(Set(sent?.keys.map { $0 } ?? []), ["id", "projectId", "title", "notes", "fixOnFail"])
        XCTAssertNil(LiveTodo.decode(.object(["title": .string("x")])), "no id")
    }

    /// The status says where it stands; a session that ended asks for review.
    func testTheStatusWordsAndWhenToRedo() {
        func todo(_ state: String, _ progress: String? = nil) -> LiveTodo {
            LiveTodo(id: "t", state: state, progress: progress)
        }
        XCTAssertEqual(TodoText.status(todo("open")), "待交办")
        XCTAssertEqual(TodoText.status(todo("assigned", "running")), "进行中")
        XCTAssertEqual(TodoText.status(todo("assigned", "review")), "待验收")
        XCTAssertEqual(TodoText.status(todo("assigned", "notRun")), "没跑成")
        XCTAssertEqual(TodoText.status(todo("assigned", "missing")), "会话不在了")
        XCTAssertEqual(TodoText.status(todo("done")), "已完成")
        XCTAssertEqual(TodoText.status(todo("cancelled")), "已取消")
        XCTAssertTrue(TodoText.needsRedo(todo("assigned", "failed")))
        XCTAssertFalse(TodoText.needsRedo(todo("assigned", "review")))
        var twice = todo("assigned", "review")
        twice.assignments = [LiveTodoAssignment(taskId: "a", agent: "codex"), LiveTodoAssignment(taskId: "b", agent: "claude")]
        XCTAssertEqual(TodoText.assignments(twice) { $0 == "claude" ? "Claude Code" : $0 }, "交办 2 次 · 上次 Claude Code")
        XCTAssertEqual(TodoText.attention(review: 3, proposals: 0), "待办：3 条待验收")
        XCTAssertEqual(TodoText.prompt(LiveTodo(title: "加测试", notes: "  ")),
                       "加测试\n\n做完后用几句话说明：改了什么、怎么验证的、还有什么没做完。")
    }

    /// The store saves, hands out, accepts and deletes; a refusal keeps the reason.
    func testTheStoreSendsEachActionAndKeepsTheReasonForARefusal() async {
        let service = FakeTodoService()
        let store = TodoStore(service: service)
        let added = await store.save(LiveTodo(projectId: "p", title: "加测试"))
        XCTAssertTrue(added)
        XCTAssertEqual(store.open.count, 1)
        let task = await store.assign(store.todos[0], to: LiveTodoRunner(agent: "codex"))
        XCTAssertEqual(task, "task-9")
        XCTAssertEqual(store.todos[0].progress, "running")
        XCTAssertEqual(store.reviewCount, 0)
        service.todos[0].progress = "review"
        await store.reload()
        XCTAssertEqual(store.reviewCount, 1)
        await store.decide(store.todos[0], "accept")
        XCTAssertEqual(store.closed.count, 1)
        XCTAssertEqual(store.reviewCount, 0)
        await store.delete(store.todos[0])
        XCTAssertTrue(store.todos.isEmpty)
        XCTAssertEqual(service.calls, ["save 加测试", "assign todo-1 codex", "accept todo-1", "delete todo-1"])

        service.refuse = "写上要做什么"
        let saved = await store.save(LiveTodo(projectId: "p", title: " "))
        XCTAssertFalse(saved)
        XCTAssertTrue(store.errorText?.contains("写上要做什么") == true, store.errorText ?? "")
        let none = await TodoStore(service: nil).assign(LiveTodo(id: "x"), to: LiveTodoRunner(agent: "codex"))
        XCTAssertNil(none)
    }
}

/// 待办第二批: Agent proposals, 验收检查 and 接着修.
@MainActor
final class TodoSecondBatchTests: XCTestCase {
    /// A proposal and a hand-out's check read off the wire; a save sends the
    /// check only when there is one, and 接着修 only with it.
    func testProposalsAndChecksDecodeAndASaveSendsTheCheck() {
        let proposal = LiveTodo.decode(.object([
            "id": .string("todo-p"), "projectId": .string("p"), "title": .string("补 NMS 单测"),
            "state": .string("proposed"), "proposedBy": .string("task-3"),
        ]))
        XCTAssertEqual(proposal?.isProposed, true)
        XCTAssertEqual(proposal?.proposedBy, "task-3")
        let checked = LiveTodo.decode(.object([
            "id": .string("todo-c"), "title": .string("修复超时"), "state": .string("assigned"),
            "check": .string("cargo test"), "fixOnFail": .bool(true),
            "assignments": .array([.object(["taskId": .string("t1"), "agent": .string("codex"), "fixes": .number("1"),
                                            "check": .object(["state": .string("failed"), "output": .string("退出码 101：1 failed"),
                                                              "atMs": .number("7"), "turnMs": .number("5")])])]),
        ]))
        XCTAssertEqual(checked?.lastCheck, LiveTodoCheck(state: "failed", output: "退出码 101：1 failed", atMs: 7))
        XCTAssertEqual(checked?.assignments.first?.fixes, 1)
        XCTAssertEqual(checked?.json.objectValue?["check"], .string("cargo test"))
        XCTAssertEqual(checked?.json.objectValue?["fixOnFail"], .bool(true))
        let blank = LiveTodo(title: "x", check: "  ", fixOnFail: true).json.objectValue
        XCTAssertNil(blank?["check"])
        XCTAssertEqual(blank?["fixOnFail"], .bool(false), "nothing to fix against")
    }

    /// The words: the check capsule, the rounds sent back, the 需要我处理 row, the prompt.
    func testTheCheckWordsTheAttentionRowAndThePrompt() {
        var todo = LiveTodo(id: "t", title: "修复超时", state: "assigned", progress: "review", check: "cargo test")
        XCTAssertNil(TodoText.checkStatus(todo), "not run yet")
        todo.assignments = [LiveTodoAssignment(taskId: "a", agent: "codex", check: LiveTodoCheck(state: "running"))]
        XCTAssertEqual(TodoText.checkStatus(todo), "检查中…")
        todo.assignments[0].check = LiveTodoCheck(state: "passed")
        XCTAssertEqual(TodoText.checkStatus(todo), "检查通过")
        todo.assignments[0].check = LiveTodoCheck(state: "failed", output: " 1 failed ")
        todo.assignments[0].fixes = 2
        XCTAssertEqual(TodoText.checkStatus(todo), "检查没过")
        XCTAssertEqual(TodoText.fixes(todo), "已让 Agent 接着修 2/2 次")
        XCTAssertNil(TodoText.checkStatus(LiveTodo(state: "assigned")), "no check")
        XCTAssertEqual(TodoText.status(LiveTodo(state: "proposed")), "Agent 提议")

        XCTAssertEqual(TodoText.attention(review: 2, proposals: 1), "待办：2 条待验收、1 条 Agent 提议")
        XCTAssertEqual(TodoText.attention(review: 0, proposals: 3), "待办：3 条 Agent 提议")
        XCTAssertTrue(TodoText.attentionDetail(review: 0, proposals: 1).contains("收下"))

        // Same words as the host's `prompt_for`: the check, then what failed last time.
        XCTAssertEqual(TodoText.prompt(todo),
                       "修复超时\n\n验收检查：`cargo test`。这一轮结束后 Aeroric 会在项目目录里运行它，退出码为 0 才算通过。"
                       + "\n\n上一次交办后验收检查没过，输出：\n```\n1 failed\n```"
                       + "\n\n做完后用几句话说明：改了什么、怎么验证的、还有什么没做完。")
    }

    /// Proposals stand apart from open work; 收下 and 重新检查 reach the host;
    /// the page looks again sooner while a check runs.
    func testTheStoreTakesProposalsAndRechecks() async {
        let service = FakeTodoService()
        service.todos = [
            LiveTodo(id: "p1", projectId: "p", title: "补单测", state: "proposed", proposedBy: "task-1"),
            LiveTodo(id: "a1", projectId: "p", title: "修复超时", state: "assigned",
                     assignments: [LiveTodoAssignment(taskId: "t1", agent: "codex")], progress: "review", check: "cargo test"),
        ]
        let store = TodoStore(service: service)
        await store.reload()
        XCTAssertEqual(store.proposals.map(\.id), ["p1"])
        XCTAssertEqual(store.open.map(\.id), ["a1"])
        XCTAssertEqual(store.proposalCount, 1)
        XCTAssertFalse(store.inFlight)
        await store.recheck(store.open[0])
        XCTAssertEqual(store.open[0].lastCheck?.state, "running")
        XCTAssertTrue(store.inFlight)
        await store.decide(store.proposals[0], "take")
        XCTAssertEqual(store.proposalCount, 0)
        XCTAssertEqual(store.open.count, 2)
        XCTAssertEqual(service.calls, ["check a1", "take p1"])
    }

    // MARK: 2026-11-17 交办选配置和模型

    private let targets = [
        RerunTarget(id: "codex", title: "Codex", agent: "codex", models: ["gpt-6.1-sol", "gpt-6.1-mini"]),
        RerunTarget(id: "agent-config:ds", title: "DeepSeek", agent: "codex", models: ["deepseek-v4"]),
        RerunTarget(id: "claude", title: "Claude Code", agent: "claude", models: []),
    ]

    func testTheSheetStartsWithTheLastConfigurationAndModel() {
        var todo = LiveTodo(id: "t", title: "x")
        XCTAssertNil(TodoText.defaultPick(todo, targets: [], fallbackAgent: "codex"), "no catalog: the Agent alone")
        XCTAssert(TodoText.defaultPick(todo, targets: targets, fallbackAgent: "claude")! == ("claude", nil))
        XCTAssert(TodoText.defaultPick(todo, targets: targets, fallbackAgent: "codex")! == ("codex", "gpt-6.1-sol"))
        todo.assignments = [LiveTodoAssignment(taskId: "a", agent: "codex", model: "gpt-6.1-mini", configurationId: "codex")]
        XCTAssert(TodoText.defaultPick(todo, targets: targets, fallbackAgent: "claude")! == ("codex", "gpt-6.1-mini"))
        // A model the configuration no longer offers falls back to its first.
        todo.assignments = [LiveTodoAssignment(taskId: "a", agent: "codex", model: "gone", configurationId: "codex")]
        XCTAssert(TodoText.defaultPick(todo, targets: targets, fallbackAgent: "codex")! == ("codex", "gpt-6.1-sol"))
        // A configuration that is gone: the last Agent's first one.
        todo.assignments = [LiveTodoAssignment(taskId: "a", agent: "claude", configurationId: "agent-config:gone")]
        XCTAssert(TodoText.defaultPick(todo, targets: targets, fallbackAgent: "codex")! == ("claude", nil))
    }

    func testAHandOutSendsItsConfigurationAndModel() async {
        XCTAssertEqual(LiveTodoRunner(agent: "codex").params, ["agent": .string("codex")])
        XCTAssertEqual(LiveTodoRunner(agent: "codex", configurationId: "agent-config:ds", model: "deepseek-v4").params,
                       ["agent": .string("codex"), "configurationId": .string("agent-config:ds"),
                        "model": .string("deepseek-v4")])
        let service = FakeTodoService()
        service.todos = [LiveTodo(id: "todo-1", title: "x")]
        let store = TodoStore(service: service)
        await store.reload()
        _ = await store.assign(store.todos[0], to: LiveTodoRunner(agent: "codex", configurationId: "agent-config:ds",
                                                                   model: "deepseek-v4"))
        XCTAssertEqual(service.calls.last, "assign todo-1 codex agent-config:ds deepseek-v4")
    }

    func testTheListNamesTheConfigurationItRanWith() {
        let wire = JSONValue.object([
            "id": .string("todo-1"), "projectId": .string("p"), "title": .string("x"), "state": .string("assigned"),
            "assignments": .array([.object(["taskId": .string("t1"), "agent": .string("codex"),
                                            "model": .string("deepseek-v4"), "configurationId": .string("agent-config:ds"),
                                            "label": .string("DeepSeek · deepseek-v4"), "atMs": .number("5")])]),
        ])
        let todo = LiveTodo.decode(wire)!
        XCTAssertEqual(todo.assignments[0].configurationId, "agent-config:ds")
        XCTAssertEqual(TodoText.assignments(todo) { $0 }, "交办 1 次 · 上次 DeepSeek · deepseek-v4")
        XCTAssertTrue(TodoText.assignHint(todo, picksConfiguration: true).hasPrefix("会话用选的配置和模型"))
        XCTAssertTrue(TodoText.assignHint(todo, picksConfiguration: false).hasPrefix("会话用这个 Agent 的默认权限模式和模型"))
    }

    /// B2-7: a second click on 保存 while the first save runs sends nothing.
    func testADoubleSaveMakesOneTodo() async {
        let service = FakeTodoService()
        service.pause = 200_000_000
        let store = TodoStore(service: service)
        let draft = LiveTodo(projectId: "p", title: "加测试")
        async let first = store.save(draft)
        async let second = store.save(draft)
        let (a, b) = await (first, second)
        XCTAssertEqual([a, b].filter { $0 }.count, 1)
        XCTAssertEqual(service.calls.filter { $0 == "save 加测试" }.count, 1)
        XCTAssertTrue(store.writing.isEmpty)
    }

    /// Round-2 review E1: the second click of a double click can land after
    /// the first save already finished, while the sheet is still closing.
    /// The same editor saves once; a new editor saves again.
    func testAnEditorThatSavedSavesNothingMore() async {
        let service = FakeTodoService()
        let store = TodoStore(service: service)
        let draft = LiveTodo(projectId: "p", title: "加测试")
        let editor = UUID()
        let first = await store.save(draft, from: editor)
        let second = await store.save(draft, from: editor)
        XCTAssertTrue(first)
        XCTAssertFalse(second)
        XCTAssertEqual(service.calls.filter { $0 == "save 加测试" }.count, 1)
        let another = await store.save(draft, from: UUID())
        XCTAssertTrue(another)
        XCTAssertEqual(service.calls.filter { $0 == "save 加测试" }.count, 2)
    }

}
