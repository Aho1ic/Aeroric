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
    /// 全部确认 / 全部拒绝 in one call; how many were decided.
    func confirmMany(ids: [String], accept: Bool) async throws -> Int
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
    /// Q4: 整理历史会话 — the plan, then one imported session at a time.
    func historyPlan(projectID: String, sinceMs: UInt64?) async throws -> LiveMemoryHistoryPlan
    func extractImported(id: String) async throws -> LiveMemoryRun?
    /// B5-3a: 有用 / 不对.
    func feedback(id: String, helpful: Bool) async throws
    /// B5-3b: skill proposals.
    func proposals() async throws -> [LiveSkillProposal]
    func editProposal(id: String, edit: LiveSkillEdit) async throws
    func decideProposal(id: String, accept: Bool) async throws
    func useSkillVersion(id: String, version: Int) async throws
    func disableSkill(id: String, disabled: Bool) async throws
}

extension MemoryService {
    public func confirmMany(ids: [String], accept: Bool) async throws -> Int {
        for id in ids { try await confirm(id: id, accept: accept) }
        return ids.count
    }
    public func settings() async throws -> LiveMemorySettings { LiveMemorySettings() }
    public func saveSettings(_ settings: LiveMemorySettings) async throws -> LiveMemorySettings { settings }
    public func runs() async throws -> LiveMemoryRunsPage { LiveMemoryRunsPage() }
    public func models() async throws -> [LiveMemoryModelChoice] { [] }
    public func extract(taskID: String) async throws -> LiveMemoryRun? { nil }
    public func importNative(path: String, projectID: String?) async throws -> (imported: Int, skipped: Int) { (0, 0) }
    public func historyPlan(projectID: String, sinceMs: UInt64?) async throws -> LiveMemoryHistoryPlan { LiveMemoryHistoryPlan() }
    public func extractImported(id: String) async throws -> LiveMemoryRun? { nil }
    public func feedback(id: String, helpful: Bool) async throws {}
    public func proposals() async throws -> [LiveSkillProposal] { [] }
    public func editProposal(id: String, edit: LiveSkillEdit) async throws {}
    public func decideProposal(id: String, accept: Bool) async throws {}
    public func useSkillVersion(id: String, version: Int) async throws {}
    public func disableSkill(id: String, disabled: Bool) async throws {}
}

/// The memory page's five tabs (§6.7).
public enum MemoryTab: String, CaseIterable, Identifiable, Sendable {
    case about, context, history, experience, skills, log

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .about: "关于你"
        case .context: "项目上下文"
        case .history: "历史线索"
        case .experience: "学会的经验"
        case .skills: "技能提案"
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
        case .skills, .log: nil
        }
    }

    /// Why a later tab is empty (none since B5-3a; kept for tabs to come).
    public var later: String? { nil }

    /// Tabs whose items Aeroric writes, not the user (no add row).
    public var modelWritten: Bool { self == .history || self == .experience || self == .skills }
}

@MainActor
@Observable
public final class MemoryStore {
    /// nil is global.
    public var scope: String?
    public var tab: MemoryTab = .about { didSet { if tab != oldValue { regroup() } } }
    public var query = ""
    public private(set) var items: [LiveMemoryItem] = [] { didSet { regroup() } }
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
    /// B5-3b: every skill proposal (the 技能提案 tab, experience badges).
    public private(set) var proposals: [LiveSkillProposal] = []
    /// 导入为候选's result per file path.
    public private(set) var importNotes: [String: String] = [:]
    /// Q4: 整理历史会话's plan and how the run is going.
    public private(set) var historyPlan: LiveMemoryHistoryPlan?
    public private(set) var history = MemoryHistoryProgress()
    @ObservationIgnored private var historyTask: Task<Void, Never>?

    @ObservationIgnored private let service: (any MemoryService)?

    public init(service: (any MemoryService)?) {
        self.service = service
    }

    public var available: Bool { service != nil }

    /// Pending first, then what is in effect, then old versions and the
    /// archive. Kept, not computed per frame: thousands of imported
    /// candidates made every redraw filter and sort them all (2026-11-02).
    public private(set) var shown: (pending: [LiveMemoryItem], active: [LiveMemoryItem], history: [LiveMemoryItem]) = ([], [], [])
    @ObservationIgnored private var counts: [String: Int] = [:]

    private func regroup() {
        let kind = tab.kind
        let mine = items.filter { kind == nil || $0.kind == kind }
        shown = (mine.filter { $0.state == "pending" },
                mine.filter { $0.state == "active" }.sorted { ($0.pinned ? 0 : 1, -Int64($0.updatedMs)) < ($1.pinned ? 0 : 1, -Int64($1.updatedMs)) },
                mine.filter { $0.state == "superseded" || $0.state == "archived" })
        var next: [String: Int] = [:]
        for item in items where item.state == "active" || item.state == "pending" {
            next[item.kind, default: 0] += 1
        }
        counts = next
    }

    public func count(_ tab: MemoryTab) -> Int {
        if tab == .skills { return proposals.filter { $0.state == "pending" }.count }
        guard let kind = tab.kind else { return 0 }
        return counts[kind] ?? 0
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
            proposals = (try? await service.proposals()) ?? proposals
            settings = try await service.settings()
        } catch where AgentConfigStore.isCancelled(error) {
            return
        } catch {
            errorText = "读取记忆失败：\(AgentConfigStore.describe(error))"
        }
    }

    /// Q4: plan 整理历史会话 for a project since a time (nil = all).
    public func planHistory(projectID: String, sinceMs: UInt64?) async {
        guard let service, !history.running else { return }
        do {
            historyPlan = try await service.historyPlan(projectID: projectID, sinceMs: sinceMs)
            history = MemoryHistoryProgress()
        } catch where AgentConfigStore.isCancelled(error) {
            return
        } catch {
            historyPlan = nil
            errorText = "读取历史会话失败：\(AgentConfigStore.describe(error))"
        }
    }

    /// Q4: extract the plan's sessions one by one, as many as today allows.
    /// Stopping only stops sending the next one.
    public func startHistory() {
        guard let service, let plan = historyPlan, !history.running,
              MemoryText.historyBlocker(plan) == nil else { return }
        let ids = plan.sessions.filter { !$0.done }.prefix(plan.remainingToday).map(\.id)
        history = MemoryHistoryProgress(total: ids.count, running: true)
        historyTask = Task { [weak self] in
            for id in ids {
                if Task.isCancelled { break }
                do {
                    let run = try await service.extractImported(id: id)
                    self?.history.count(run)
                } catch {
                    if Task.isCancelled { break }
                    let text = AgentConfigStore.describe(error)
                    if text.contains("到上限") || text.contains("还没有选") {
                        self?.history.note = text
                        break
                    }
                    self?.history.count(nil)
                }
            }
            guard let self else { return }
            history.running = false
            if history.note == nil { history.note = Task.isCancelled ? "已停止" : "整理完了" }
            historyTask = nil
            await reload()
            await refreshPending()
        }
    }

    public func stopHistory() {
        historyTask?.cancel()
    }

    /// The 整理设置 sheet opens: what can be the analysis model.
    public func loadModels() async {
        guard let service else { return }
        do { models = try await service.models() } catch { errorText = "读取可用模型失败：\(AgentConfigStore.describe(error))" }
    }

    /// Save the 整理设置; false when the host refused (no model, …).
    @discardableResult
    public func saveSettings(_ next: LiveMemorySettings) async -> Bool {
        guard let service else { return false }
        do {
            settings = try await service.saveSettings(next)
            return true
        } catch {
            errorText = "保存整理设置失败：\(AgentConfigStore.describe(error))"
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
            errorText = "导入失败：\(AgentConfigStore.describe(error))"
        }
        await reload()
        await refreshPending()
    }

    /// B5-3a: 有用 / 不对.
    public func feedback(_ item: LiveMemoryItem, helpful: Bool) async {
        await run(helpful ? "标为有用" : "标为不对") { try await $0.feedback(id: item.id, helpful: helpful) }
    }

    /// B5-3b: the proposal grown from an experience, if any.
    public func proposal(for experienceID: String) -> LiveSkillProposal? {
        proposals.first { $0.experienceId == experienceID }
    }

    /// B5-3b: proposals by state, pending first.
    public var proposalGroups: (pending: [LiveSkillProposal], accepted: [LiveSkillProposal],
                                closed: [LiveSkillProposal]) {
        (proposals.filter { $0.state == "pending" }, proposals.filter { $0.state == "accepted" },
         proposals.filter { $0.state == "disabled" || $0.state == "rejected" })
    }

    public func editProposal(_ proposal: LiveSkillProposal, _ edit: LiveSkillEdit) async {
        await run("保存技能提案") { try await $0.editProposal(id: proposal.id, edit: edit) }
    }

    public func decide(_ proposal: LiveSkillProposal, accept: Bool) async {
        await run(accept ? "采纳技能" : "拒绝技能") { try await $0.decideProposal(id: proposal.id, accept: accept) }
    }

    public func useVersion(_ proposal: LiveSkillProposal, _ n: Int) async {
        await run("回到旧版本") { try await $0.useSkillVersion(id: proposal.id, version: n) }
    }

    public func disable(_ proposal: LiveSkillProposal, _ disabled: Bool) async {
        await run(disabled ? "停用技能" : "启用技能") { try await $0.disableSkill(id: proposal.id, disabled: disabled) }
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
            errorText = "\(what)失败：\(AgentConfigStore.describe(error))"
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

    /// Decided here, shown at once: no full reload per click (2026-11-02).
    public func confirm(_ item: LiveMemoryItem, accept: Bool) async {
        guard let service else { return }
        do {
            try await service.confirm(id: item.id, accept: accept)
            settle([item.id], accept: accept)
        } catch {
            errorText = "\(accept ? "确认" : "拒绝")失败：\(AgentConfigStore.describe(error))"
            await reload()
        }
    }

    /// 全部确认 / 全部拒绝 of what is pending in this tab (or one group).
    public func confirmAll(accept: Bool, group: String? = nil) async {
        guard let service else { return }
        let ids = shown.pending.filter { group == nil || MemoryText.pendingGroup($0) == group }.map(\.id)
        guard !ids.isEmpty else { return }
        busyText = accept ? "正在确认 \(ids.count) 条…" : "正在拒绝 \(ids.count) 条…"
        defer { busyText = nil }
        do {
            _ = try await service.confirmMany(ids: ids, accept: accept)
            settle(ids, accept: accept)
        } catch {
            errorText = "批量处理失败：\(AgentConfigStore.describe(error))"
            await reload()
        }
    }

    /// What the host now holds, without reading everything back.
    private func settle(_ ids: [String], accept: Bool) {
        let set = Set(ids)
        if accept {
            items = items.map { item in
                guard set.contains(item.id) else { return item }
                var next = item
                next.state = "active"
                return next
            }
        } else {
            items.removeAll { set.contains($0.id) }
        }
        pending.removeAll { set.contains($0.id) }
    }

    /// A long batch's progress line.
    public private(set) var busyText: String?

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
        case "partial": "部分成功"
        case "failed": "失败"
        default: "跳过"
        }
    }

    /// "10:21 · 修复超时 · deepseek-flash · 6.1k tok · 约 $0.002".
    public static func runLine(_ run: LiveMemoryRun) -> String {
        let time = Date(timeIntervalSince1970: Double(run.atMs) / 1000).formatted(date: .omitted, time: .shortened)
        var parts = [time, run.title.isEmpty ? "未命名会话" : run.title]
        if run.state != "skipped", let model = run.model { parts.append(model) }
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

    /// B5-3b: a proposal's state word.
    public static func proposalState(_ state: String) -> String {
        switch state {
        case "accepted": "已采纳"
        case "rejected": "已拒绝"
        case "disabled": "已停用"
        default: "待审"
        }
    }

    /// The badge an experience row shows when it grew a proposal.
    public static func promotion(_ proposal: LiveSkillProposal?) -> String? {
        switch proposal?.state {
        case "pending": "已提议升格"
        case "accepted": "已成技能"
        default: nil
        }
    }

    /// 需要我处理's detail.
    public static func skillAttention(_ proposal: LiveSkillProposal) -> String {
        "技能提案待审：\(proposal.name) · \(proposal.description)"
    }

    /// 整理记录's header.
    /// More than one line, or too long for one: the row folds (2026-11-02).
    public static func isLong(_ text: String) -> Bool {
        text.contains("\n") || text.count > 60
    }

    /// Q4: why 整理历史会话 cannot start, if it cannot.
    public static func historyBlocker(_ plan: LiveMemoryHistoryPlan) -> String? {
        if !plan.enabled { return "这个项目没有开启记忆：先在记忆页打开“为这个项目启用记忆”" }
        if plan.model == nil { return "还没有选分析模型：先在“整理设置”里选一个" }
        if plan.sessions.isEmpty { return "这段时间没有导入的会话" }
        if plan.pending == 0 { return "这段时间的会话都整理过了" }
        if plan.remainingToday == 0 { return "今天的整理次数用完了，明天再来" }
        return nil
    }

    public static func historyCount(_ plan: LiveMemoryHistoryPlan) -> String {
        let done = plan.sessions.count - plan.pending
        return done > 0 ? "\(plan.pending) 个会话待整理 · \(done) 个整理过，跳过" : "\(plan.pending) 个会话待整理"
    }

    public static func historyCost(_ plan: LiveMemoryHistoryPlan) -> String {
        let tokens = "约 \(plan.inputTokens.formatted()) 输入 + \(plan.outputTokens.formatted()) 输出 token"
        guard let cost = plan.costUsd else { return tokens }
        return "\(tokens) · \(costText(cost))"
    }

    public static func historyLimit(_ plan: LiveMemoryHistoryPlan) -> String {
        plan.remainingToday >= plan.pending ? "今天还能整理 \(plan.remainingToday) 次"
            : "今天还能整理 \(plan.remainingToday) 次，其余 \(plan.pending - plan.remainingToday) 个明天再来"
    }

    public static func historyProgress(_ progress: MemoryHistoryProgress) -> String {
        let counts = "写入 \(progress.written) 条 · 跳过 \(progress.skipped) · 失败 \(progress.failed)"
        if progress.running { return "正在整理 \(progress.finished + 1)/\(progress.total) · \(counts)" }
        return "\(progress.note ?? "") · \(progress.finished)/\(progress.total) · \(counts)"
    }

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

    /// What a pending item is grouped under: the memory file it was imported
    /// from, or the extraction that proposed it.
    public static func pendingGroup(_ item: LiveMemoryItem) -> String {
        guard let first = item.evidence.first else { return "其他" }
        if first.taskId != nil { return "会话整理" }
        let who = first.harness.map(harness) ?? "导入"
        if let cut = first.excerpt.range(of: " · ") {
            return "\(who) · \(first.excerpt[..<cut.lowerBound])"
        }
        return "\(who) 的记忆文件"
    }

    /// Where a pending or opened item came from, in one line.
    public static func evidenceLine(_ item: LiveMemoryItem) -> String? {
        guard let first = item.evidence.first else { return nil }
        let excerpt = first.excerpt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !excerpt.isEmpty, excerpt != item.text else { return nil }
        return "依据：\(excerpt)"
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

/// Q4: how 整理历史会话 is going.
public struct MemoryHistoryProgress: Equatable, Sendable {
    public var total = 0
    public var finished = 0
    /// Facts, candidates and experiences written.
    public var written = 0
    public var skipped = 0
    public var failed = 0
    public var running = false
    /// Why it stopped, or that it finished.
    public var note: String?

    public init(total: Int = 0, running: Bool = false) {
        self.total = total
        self.running = running
    }

    mutating func count(_ run: LiveMemoryRun?) {
        finished += 1
        switch run?.state {
        case "done", "partial": written += (run?.facts ?? 0) + (run?.candidates ?? 0) + (run?.experiences ?? 0)
        case "skipped": skipped += 1
        default: failed += 1
        }
    }
}
