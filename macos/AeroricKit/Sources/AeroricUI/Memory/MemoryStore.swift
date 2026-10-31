import Foundation
import Observation
import AeroricProtocol

/// B5-1 (2026-10-31): what the memory page asks of the host. The App
/// implements it on top of `SessionsClient`.
@MainActor
public protocol MemoryService {
    func list(projectID: String?, query: String?) async throws -> (items: [LiveMemoryItem], enabled: Bool?)
    func remember(projectID: String?, kind: String, category: String?, text: String,
                  supersedes: String?) async throws -> LiveMemoryItem
    func confirm(id: String, accept: Bool) async throws
    func archive(id: String, archived: Bool) async throws
    func pin(id: String, pinned: Bool) async throws
    func move(id: String, projectID: String?) async throws
    func forget(id: String) async throws
    func setEnabled(projectID: String, enabled: Bool) async throws
    func native(projectID: String?) async throws -> [LiveNativeMemory]
    func attention() async throws -> [LiveMemoryItem]
    /// B5-2: the 整理设置, 整理记录, the models that can be the analysis model,
    /// and 导入为候选.
    func settings() async throws -> LiveMemorySettings
    func saveSettings(_ settings: LiveMemorySettings) async throws -> LiveMemorySettings
    func runs() async throws -> LiveMemoryRunsPage
    func models() async throws -> [LiveMemoryModelChoice]
    func extract(taskID: String) async throws -> LiveMemoryRun?
    func importNative(path: String, projectID: String?) async throws -> (imported: Int, skipped: Int)
    /// B5-3a: 有用 / 不对.
    func feedback(id: String, helpful: Bool) async throws
}

extension MemoryService {
    public func settings() async throws -> LiveMemorySettings { LiveMemorySettings() }
    public func saveSettings(_ settings: LiveMemorySettings) async throws -> LiveMemorySettings { settings }
    public func runs() async throws -> LiveMemoryRunsPage { LiveMemoryRunsPage() }
    public func models() async throws -> [LiveMemoryModelChoice] { [] }
    public func extract(taskID: String) async throws -> LiveMemoryRun? { nil }
    public func importNative(path: String, projectID: String?) async throws -> (imported: Int, skipped: Int) { (0, 0) }
    public func feedback(id: String, helpful: Bool) async throws {}
}

/// The memory page's five tabs (§6.7).
public enum MemoryTab: String, CaseIterable, Identifiable, Sendable {
    case about, context, history, experience, log

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .about: "关于你"
        case .context: "项目上下文"
        case .history: "历史线索"
        case .experience: "学会的经验"
        case .log: "整理记录"
        }
    }

    /// The item kind the tab lists; nil for tabs that open later.
    public var kind: String? {
        switch self {
        case .about: "profile"
        case .context: "fact"
        case .history: "sessionFact"
        case .experience: "experience"
        case .log: nil
        }
    }

    /// Why a later tab is empty (none since B5-3a; kept for tabs to come).
    public var later: String? { nil }

    /// Tabs whose items Aeroric writes, not the user (no add row).
    public var modelWritten: Bool { self == .history || self == .experience }
}

@MainActor
@Observable
public final class MemoryStore {
    /// nil is global.
    public var scope: String?
    public var tab: MemoryTab = .about
    public var query = ""
    public private(set) var items: [LiveMemoryItem] = []
    /// Whether the scoped project uses memory (nil for global).
    public private(set) var enabled: Bool?
    public private(set) var native: [LiveNativeMemory] = []
    /// Proposals in every scope (the sidebar badge, 需要我处理).
    public private(set) var pending: [LiveMemoryItem] = []
    public private(set) var loading = false
    public var errorText: String?
    /// B5-2: the 整理设置, 整理记录 and the analysis-model choices.
    public private(set) var settings = LiveMemorySettings()
    public private(set) var runsPage = LiveMemoryRunsPage()
    public private(set) var models: [LiveMemoryModelChoice] = []
    /// 导入为候选's result per file path.
    public private(set) var importNotes: [String: String] = [:]

    @ObservationIgnored private let service: (any MemoryService)?

    public init(service: (any MemoryService)?) {
        self.service = service
    }

    public var available: Bool { service != nil }

    /// Pending first, then what is in effect, then old versions and the archive.
    public var shown: (pending: [LiveMemoryItem], active: [LiveMemoryItem], history: [LiveMemoryItem]) {
        let kind = tab.kind
        let mine = items.filter { kind == nil || $0.kind == kind }
        return (mine.filter { $0.state == "pending" },
                mine.filter { $0.state == "active" }.sorted { ($0.pinned ? 0 : 1, -Int64($0.updatedMs)) < ($1.pinned ? 0 : 1, -Int64($1.updatedMs)) },
                mine.filter { $0.state == "superseded" || $0.state == "archived" })
    }

    public func count(_ tab: MemoryTab) -> Int {
        guard let kind = tab.kind else { return 0 }
        return items.filter { $0.kind == kind && ($0.state == "active" || $0.state == "pending") }.count
    }

    public func reload() async {
        guard let service else { return }
        loading = true
        defer { loading = false }
        do {
            let trimmed = query.trimmingCharacters(in: .whitespaces)
            let page = try await service.list(projectID: scope, query: trimmed.isEmpty ? nil : trimmed)
            items = page.items
            enabled = page.enabled
            if tab == .about { native = (try? await service.native(projectID: scope)) ?? [] }
            if tab == .log { runsPage = try await service.runs() }
            settings = try await service.settings()
        } catch {
            errorText = "读取记忆失败：\(error)"
        }
    }

    /// The 整理设置 sheet opens: what can be the analysis model.
    public func loadModels() async {
        guard let service else { return }
        do { models = try await service.models() } catch { errorText = "读取可用模型失败：\(error)" }
    }

    /// Save the 整理设置; false when the host refused (no model, …).
    @discardableResult
    public func saveSettings(_ next: LiveMemorySettings) async -> Bool {
        guard let service else { return false }
        do {
            settings = try await service.saveSettings(next)
            return true
        } catch {
            errorText = "保存整理设置失败：\(error)"
            return false
        }
    }

    /// 导入为候选: one file's lines into pending.
    public func importNative(_ file: LiveNativeMemory) async {
        guard let service else { return }
        do {
            let result = try await service.importNative(path: file.path, projectID: scope)
            importNotes[file.path] = "导入 \(result.imported) 条，跳过 \(result.skipped) 条（已有或已遗忘）"
        } catch {
            errorText = "导入失败：\(error)"
        }
        await reload()
        await refreshPending()
    }

    /// B5-3a: 有用 / 不对.
    public func feedback(_ item: LiveMemoryItem, helpful: Bool) async {
        await run(helpful ? "标为有用" : "标为不对") { try await $0.feedback(id: item.id, helpful: helpful) }
    }

    /// 重试 a failed row.
    public func retry(_ row: LiveMemoryRun) async {
        await run("重新整理") { _ = try await $0.extract(taskID: row.taskId) }
    }

    public func refreshPending() async {
        guard let service, let next = try? await service.attention() else { return }
        if next != pending { pending = next }
    }

    private func run(_ what: String, _ body: @MainActor (any MemoryService) async throws -> Void) async {
        guard let service else { return }
        do {
            try await body(service)
        } catch {
            errorText = "\(what)失败：\(error)"
        }
        await reload()
        await refreshPending()
    }

    public func add(_ text: String, category: String? = nil) async {
        let kind = tab.kind ?? "fact"
        await run("记住") { try await $0.remember(projectID: scope, kind: kind, category: category,
                                                text: text, supersedes: nil) }
    }

    /// 修改: a new item that supersedes the old one.
    public func edit(_ item: LiveMemoryItem, to text: String) async {
        await run("修改") { try await $0.remember(projectID: item.projectId, kind: item.kind, category: item.category,
                                                text: text, supersedes: item.id) }
    }

    public func confirm(_ item: LiveMemoryItem, accept: Bool) async {
        await run(accept ? "确认" : "拒绝") { try await $0.confirm(id: item.id, accept: accept) }
    }

    public func archive(_ item: LiveMemoryItem, _ archived: Bool) async {
        await run(archived ? "归档" : "恢复") { try await $0.archive(id: item.id, archived: archived) }
    }

    public func pin(_ item: LiveMemoryItem) async {
        await run("置顶") { try await $0.pin(id: item.id, pinned: !item.pinned) }
    }

    public func move(_ item: LiveMemoryItem, to projectID: String?) async {
        await run("移动") { try await $0.move(id: item.id, projectID: projectID) }
    }

    public func forget(_ item: LiveMemoryItem) async {
        await run("永久遗忘") { try await $0.forget(id: item.id) }
    }

    public func setEnabled(_ on: Bool) async {
        guard let scope else { return }
        await run("切换项目记忆") { try await $0.setEnabled(projectID: scope, enabled: on) }
    }

    /// Open the page on a scope (项目记忆…, a 需要我处理 row).
    public func show(scope: String?, tab: MemoryTab) {
        self.scope = scope
        self.tab = tab
    }
}

/// B5-1: the words and helpers shared by the memory page and `/记住`.
public enum MemoryText {
    public static let categories = ["language", "interaction", "preference", "constraint"]

    public static func category(_ category: String?) -> String? {
        switch category {
        case "language": "语言"
        case "interaction": "习惯"
        case "preference": "偏好"
        case "constraint": "限制"
        default: nil
        }
    }

    /// "你说的" / "Claude 提议 · 你确认" / "Codex 提议" / "模型整理".
    public static func source(_ item: LiveMemoryItem) -> String {
        if item.kind == "sessionFact" || item.kind == "experience" { return "模型整理" }
        let who = item.proposer.map(harness) ?? "Agent"
        switch item.assertion {
        case "userExplicit": return "你说的"
        case "agentConfirmed": return "\(who) 提议 · 你确认"
        case "toolVerified": return "工具核实"
        default: return "\(who) 提议"
        }
    }

    static func harness(_ id: String) -> String {
        if id.hasPrefix("claude") { return "Claude" }
        if id.hasPrefix("codex") { return "Codex" }
        if id == "analysis" { return "整理" }
        return id
    }

    /// 整理记录's state word.
    public static func runState(_ state: String) -> String {
        switch state {
        case "done": "完成"
        case "failed": "失败"
        default: "跳过"
        }
    }

    /// "10:21 · 修复超时 · deepseek-flash · 6.1k tok · 约 $0.002".
    public static func runLine(_ run: LiveMemoryRun) -> String {
        let time = Date(timeIntervalSince1970: Double(run.atMs) / 1000).formatted(date: .omitted, time: .shortened)
        var parts = [time, run.title.isEmpty ? "未命名会话" : run.title]
        if run.state == "done" || run.state == "failed", let model = run.model { parts.append(model) }
        let tokens = run.inputTokens + run.outputTokens
        if tokens > 0 { parts.append(tokenText(tokens)) }
        if let cost = run.costUsd { parts.append(costText(cost)) }
        if let reason = run.reason { parts.append(reason) }
        return parts.joined(separator: " · ")
    }

    public static func tokenText(_ tokens: UInt64) -> String {
        tokens >= 1000 ? String(format: "%.1fk tok", Double(tokens) / 1000) : "\(tokens) tok"
    }

    public static func costText(_ usd: Double) -> String {
        // CU F3: a total of nothing must not read "$-0.0000".
        guard usd > 0 else { return "$0" }
        return usd < 0.01 ? String(format: "约 $%.4f", usd) : String(format: "约 $%.2f", usd)
    }

    /// CU F2: what a row always shows beside its title — "flash · 1.5k tok · 约 $0.0009".
    public static func runDetail(_ run: LiveMemoryRun) -> String? {
        var parts: [String] = []
        if run.state != "skipped", let model = run.model {
            parts.append(model.split(separator: "/").last.map(String.init) ?? model)
        }
        let tokens = run.inputTokens + run.outputTokens
        if tokens > 0 { parts.append(tokenText(tokens)) }
        if let cost = run.costUsd { parts.append(costText(cost)) }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    /// CU F2: why it failed or was skipped, without the repeated prefix.
    public static func runReason(_ run: LiveMemoryRun) -> String? {
        guard let reason = run.reason else { return nil }
        let prefix = "调用分析模型失败："
        return reason.hasPrefix(prefix) ? String(reason.dropFirst(prefix.count)) : reason
    }

    /// What one extraction produced — "1 线索 · 1 候选 · 1 经验".
    public static func runOutput(_ run: LiveMemoryRun) -> String {
        var parts = ["\(run.facts) 线索", "\(run.candidates) 候选"]
        if run.experiences > 0 { parts.append("\(run.experiences) 经验") }
        return parts.joined(separator: " · ")
    }

    /// 整理记录's header.
    public static func runsSummary(_ page: LiveMemoryRunsPage) -> String {
        "本月 \(page.monthCount) 次 · \(costText(page.monthCostUsd)) · 今天 \(page.usedToday)/\(page.dailyLimit)"
    }

    /// "cline · deepseek-v4.1-flash".
    public static func modelTitle(_ settings: LiveMemorySettings, choices: [LiveMemoryModelChoice]) -> String {
        guard let model = settings.model else { return "选择分析模型" }
        let connection = choices.first { $0.connectionId == settings.connectionId }?.connection
            ?? settings.connectionId ?? ""
        return connection.isEmpty ? model : "\(connection) · \(model)"
    }

    public static func state(_ item: LiveMemoryItem) -> String {
        switch item.state {
        case "superseded": "已取代"
        case "archived": "已归档"
        case "pending": "待确认"
        default: "生效中"
        }
    }

    /// B5-3a: 低 / 中 / 高.
    public static func confidence(_ level: String?) -> String {
        switch level {
        case "high": "高"
        case "medium": "中"
        default: "低"
        }
    }

    /// "检索 6 · 有用 4" (experiences) / "注入 12 · 检索 3" (what was injected).
    public static func stats(_ item: LiveMemoryItem) -> String {
        let s = item.stats
        var parts: [String] = []
        if s.injected > 0 { parts.append("注入 \(s.injected)") }
        parts.append("检索 \(s.retrieved)")
        parts.append("有用 \(s.helpful)")
        if s.wrong > 0 { parts.append("不对 \(s.wrong)") }
        return parts.joined(separator: " · ")
    }

    /// The hover text: the whole item and where it came from.
    public static func help(_ item: LiveMemoryItem) -> String {
        var lines = [item.text]
        if let e = item.experience {
            if !e.problem.isEmpty { lines.append("问题：\(e.problem)") }
            lines.append("解法：\(e.solution)")
            if !e.envs.isEmpty { lines.append("适用：\(e.envs.joined(separator: " · "))") }
            if !e.searchKeys.isEmpty { lines.append("关键词：\(e.searchKeys.joined(separator: "、"))") }
        }
        lines.append(stats(item))
        for evidence in item.evidence.prefix(3) {
            let date = Date(timeIntervalSince1970: Double(evidence.atMs) / 1000)
                .formatted(date: .abbreviated, time: .shortened)
            lines.append("依据（\(date)）：\(evidence.excerpt)")
        }
        return lines.joined(separator: "\n")
    }

    /// `/记住 …` (half- or full-width space after it); nil for anything else.
    public static func rememberCommand(_ text: String) -> String? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("/记住") else { return nil }
        let body = trimmed.dropFirst("/记住".count)
        guard body.isEmpty || body.first?.isWhitespace == true || body.first == "：" || body.first == ":" else {
            return nil
        }
        let text = body.trimmingCharacters(in: CharacterSet.whitespacesAndNewlines.union(.init(charactersIn: "：:")))
        return text.isEmpty ? nil : text
    }

    /// Words about the user themselves go to 关于你; the rest are facts.
    public static func guessKind(_ text: String) -> String {
        let lower = text.lowercased()
        let aboutUser = ["我", "回复", "以后", "叫我", "请用", "用中文", "always", "i "]
        return aboutUser.contains { lower.hasPrefix($0) } ? "profile" : "fact"
    }

    /// The 需要我处理 detail.
    public static func attention(_ item: LiveMemoryItem) -> String { "记忆待确认：\(item.text)" }
}
