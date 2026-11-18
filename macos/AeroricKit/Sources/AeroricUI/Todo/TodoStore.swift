import AeroricProtocol
import Foundation
import Observation

/// 2026-11-15 工作安排第一批: what the 待办 page asks of the host.
@MainActor
public protocol TodoService {
    func list() async throws -> [LiveTodo]
    func save(_ todo: LiveTodo) async throws -> LiveTodo
    func delete(id: String) async throws
    /// A new session for the todo; the todo as it is now, and the session.
    func assign(id: String, to runner: LiveTodoRunner) async throws -> (LiveTodo, String)
    /// `accept` `cancel` `reopen` `take`.
    func decide(id: String, decision: String) async throws -> LiveTodo
    /// 第二批: run the 验收检查 now (重新检查).
    func check(id: String) async throws -> LiveTodo
}

@MainActor
@Observable
public final class TodoStore {
    public private(set) var todos: [LiveTodo] = []
    public var errorText: String?
    /// The todo an action is waiting on.
    public private(set) var busy: String?
    @ObservationIgnored private let service: TodoService?

    public init(service: TodoService?) {
        self.service = service
    }

    /// Agent proposals waiting for the user (第二批).
    public var proposals: [LiveTodo] { todos.filter(\.isProposed) }
    public var open: [LiveTodo] { todos.filter { !$0.isClosed && !$0.isProposed } }
    public var closed: [LiveTodo] { todos.filter(\.isClosed) }
    /// Assigned todos whose session ended: the user accepts or hands out again.
    public var reviewCount: Int { todos.filter { $0.state == "assigned" && $0.progress == "review" }.count }
    public var proposalCount: Int { proposals.count }
    /// A session is working or a check runs: the page looks again soon.
    public var inFlight: Bool {
        todos.contains { $0.state == "assigned" && (["running", "waiting"].contains($0.progress ?? "")
            || $0.lastCheck?.state == "running") }
    }

    public func reload() async {
        guard let service else { return }
        do {
            todos = try await service.list()
        } catch {
            errorText = "读取待办失败：\(Self.describe(error))"
        }
    }

    /// B2-7: the id a save or delete is working on (a new todo's is its
    /// draft id); a second click while it runs does nothing.
    public private(set) var writing: Set<String> = []

    /// Editors whose save went through: the sheet is still on screen while
    /// it closes, and a second click of a double click that lands after the
    /// first save finished must not make a second todo (round-2 review E1).
    private var savedEditors: Set<UUID> = []

    /// Save a new or changed todo; `false` (with the reason) when refused.
    /// `editor` names the open editor: once its save succeeded, it saves
    /// nothing more.
    public func save(_ todo: LiveTodo, from editor: UUID? = nil) async -> Bool {
        guard let service, !writing.contains(todo.id) else { return false }
        if let editor, savedEditors.contains(editor) { return false }
        writing.insert(todo.id)
        defer { writing.remove(todo.id) }
        do {
            _ = try await service.save(todo)
            if let editor { savedEditors.insert(editor) }
            await reload()
            return true
        } catch {
            errorText = "保存失败：\(Self.describe(error))"
            return false
        }
    }

    public func delete(_ todo: LiveTodo) async {
        guard let service, !writing.contains(todo.id) else { return }
        writing.insert(todo.id)
        defer { writing.remove(todo.id) }
        do {
            try await service.delete(id: todo.id)
            await reload()
        } catch {
            errorText = "删除失败：\(Self.describe(error))"
        }
    }

    /// Hand the todo to `runner`; the new session's id, or nil (with the reason).
    public func assign(_ todo: LiveTodo, to runner: LiveTodoRunner) async -> String? {
        guard let service, busy == nil else { return nil }
        busy = todo.id
        defer { busy = nil }
        do {
            let (_, task) = try await service.assign(id: todo.id, to: runner)
            await reload()
            return task
        } catch {
            errorText = "交办失败：\(Self.describe(error))"
            return nil
        }
    }

    public func decide(_ todo: LiveTodo, _ decision: String) async {
        guard let service, busy == nil else { return }
        busy = todo.id
        defer { busy = nil }
        do {
            _ = try await service.decide(id: todo.id, decision: decision)
            await reload()
        } catch {
            errorText = "没能更新：\(Self.describe(error))"
        }
    }

    /// 重新检查: the host runs it in the background; the row shows 检查中.
    public func recheck(_ todo: LiveTodo) async {
        guard let service, busy == nil else { return }
        busy = todo.id
        defer { busy = nil }
        do {
            _ = try await service.check(id: todo.id)
            await reload()
        } catch {
            errorText = "没能检查：\(Self.describe(error))"
        }
    }

    private static func describe(_ error: Error) -> String {
        AgentConfigStore.describe(error)
    }
}

/// The words of the 待办 page.
public enum TodoText {
    public static let intro = "写下要在项目里做的事，交给一个 Agent 去做。会话结束不算完成，你验收了才算。"
    public static let proposalsIntro = "Agent 做事时发现、但不在那一轮范围里的事。收下后变成待交办；不要的直接删掉。"

    /// The todo's status capsule.
    public static func status(_ todo: LiveTodo) -> String {
        switch todo.state {
        case "done": return "已完成"
        case "cancelled": return "已取消"
        case "proposed": return "Agent 提议"
        case "open": return "待交办"
        default:
            switch todo.progress {
            case "running": return "进行中"
            case "waiting": return "等你确认"
            case "review": return "待验收"
            case "notRun": return "没跑成"
            case "failed": return "失败"
            case "stopped": return "已中断"
            case "missing": return "会话不在了"
            default: return "已交办"
            }
        }
    }

    /// Something went wrong with the latest session: hand it out again.
    public static func needsRedo(_ todo: LiveTodo) -> Bool {
        todo.state == "assigned" && ["notRun", "failed", "stopped", "missing"].contains(todo.progress ?? "")
    }

    /// 2026-11-17: what the 交办 sheet starts with — the last hand-out's
    /// configuration and model while they still exist; else the first
    /// configuration of the last (or given) Agent, else the first one; its
    /// first model (none for a configuration without a model list).
    public static func defaultPick(_ todo: LiveTodo, targets: [RerunTarget],
                                   fallbackAgent: String) -> (configuration: String, model: String?)? {
        guard let first = targets.first else { return nil }
        let last = todo.assignments.last
        let target = last?.configurationId.flatMap { id in targets.first { $0.id == id } }
            ?? targets.first { $0.agent == (last?.agent ?? fallbackAgent) }
            ?? first
        let kept = last.flatMap { $0.configurationId == target.id ? $0.model : nil }
            .flatMap { target.models.contains($0) ? $0 : nil }
        return (target.id, kept ?? target.models.first)
    }

    /// The line under the prompt in the 交办 sheet.
    public static func assignHint(_ todo: LiveTodo, picksConfiguration: Bool) -> String {
        let runs = picksConfiguration ? "会话用选的配置和模型，以及这个 Agent 的默认权限模式。"
                                      : "会话用这个 Agent 的默认权限模式和模型。"
        let check = todo.check.isEmpty ? "会话结束后待办变成“待验收”" : "这一轮结束后会运行验收检查；待办变成“待验收”"
        return runs + check + "，你点“验收通过”才算完成。"
    }

    /// "交办 2 次 · 上次 Codex", or the configuration and model it ran with.
    public static func assignments(_ todo: LiveTodo, agentName: (String) -> String) -> String? {
        guard let last = todo.assignments.last else { return nil }
        return "交办 \(todo.assignments.count) 次 · 上次 \(last.label ?? agentName(last.agent))"
    }

    /// The 需要我处理 row: "待办：2 条待验收、1 条提议".
    public static func attention(review: Int, proposals: Int) -> String {
        var parts: [String] = []
        if review > 0 { parts.append("\(review) 条待验收") }
        if proposals > 0 { parts.append("\(proposals) 条 Agent 提议") }
        return "待办：" + parts.joined(separator: "、")
    }

    /// The line under it.
    public static func attentionDetail(review: Int, proposals: Int) -> String {
        switch (review > 0, proposals > 0) {
        case (true, true): return "看看结果再验收或重新交办；Agent 的提议收下或不要"
        case (false, true): return "Agent 发现的事，收下后变成待交办"
        default: return "会话已经结束，看看结果再验收或重新交办"
        }
    }

    /// The 验收检查 capsule; nil when there is no check or no run yet.
    public static func checkStatus(_ todo: LiveTodo) -> String? {
        guard !todo.check.isEmpty, todo.state == "assigned" || todo.state == "done", let run = todo.lastCheck else {
            return nil
        }
        switch run.state {
        case "running": return "检查中…"
        case "passed": return "检查通过"
        default: return "检查没过"
        }
    }

    /// "已让 Agent 接着修 1/2 次".
    public static func fixes(_ todo: LiveTodo) -> String? {
        guard let n = todo.assignments.last?.fixes, n > 0 else { return nil }
        return "已让 Agent 接着修 \(n)/\(fixRounds) 次"
    }

    /// The host's `FIX_ROUNDS`.
    public static let fixRounds = 2

    /// What the session is told — the host's wording (`prompt_for`), shown
    /// before handing out.
    public static func prompt(_ todo: LiveTodo) -> String {
        var text = todo.title.trimmingCharacters(in: .whitespacesAndNewlines)
        let notes = todo.notes.trimmingCharacters(in: .whitespacesAndNewlines)
        if !notes.isEmpty { text += "\n\n\(notes)" }
        let check = todo.check.trimmingCharacters(in: .whitespacesAndNewlines)
        if !check.isEmpty {
            text += "\n\n验收检查：`\(check)`。这一轮结束后 Aeroric 会在项目目录里运行它，退出码为 0 才算通过。"
            if let run = todo.lastCheck, run.state == "failed" {
                text += "\n\n上一次交办后验收检查没过，输出：\n```\n\(run.output.trimmingCharacters(in: .whitespacesAndNewlines))\n```"
            }
        }
        return text + "\n\n做完后用几句话说明：改了什么、怎么验证的、还有什么没做完。"
    }
}
