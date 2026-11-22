import AeroricProtocol
import Foundation
import Observation

/// A12 (2026-11-14): what the 定时任务 page asks of the host.
@MainActor
public protocol ScheduleService {
    func list() async throws -> [LiveScheduleRule]
    func save(_ rule: LiveScheduleRule) async throws -> LiveScheduleRule
    func delete(id: String) async throws
    func runNow(id: String) async throws -> LiveScheduleRun
    func runs(id: String) async throws -> [LiveScheduleRun]
}

@MainActor
@Observable
public final class ScheduleStore {
    public private(set) var rules: [LiveScheduleRule] = []
    /// A rule's runs, once its 运行记录 was opened.
    public private(set) var runs: [String: [LiveScheduleRun]] = [:]
    public var errorText: String?
    public private(set) var loading = false
    /// The rule a 立即运行 is waiting on.
    public private(set) var running: String?
    @ObservationIgnored private let service: ScheduleService?

    public init(service: ScheduleService?) {
        self.service = service
    }

    public func reload() async {
        guard let service else { return }
        loading = true
        defer { loading = false }
        do {
            rules = try await service.list()
        } catch {
            errorText = "读取定时任务失败：\(Self.describe(error))"
        }
    }

    /// Save a new or changed rule; `false` (with the reason) when refused.
    public func save(_ rule: LiveScheduleRule) async -> Bool {
        guard let service else { return false }
        do {
            _ = try await service.save(rule)
            await reload()
            return true
        } catch {
            errorText = "保存失败：\(Self.describe(error))"
            return false
        }
    }

    public func setEnabled(_ rule: LiveScheduleRule, _ enabled: Bool) async {
        var next = rule
        next.enabled = enabled
        _ = await save(next)
    }

    public func delete(_ rule: LiveScheduleRule) async {
        guard let service else { return }
        do {
            try await service.delete(id: rule.id)
            runs[rule.id] = nil
            await reload()
        } catch {
            errorText = "删除失败：\(Self.describe(error))"
        }
    }

    public func runNow(_ rule: LiveScheduleRule) async {
        guard let service, running == nil else { return }
        running = rule.id
        defer { running = nil }
        do {
            _ = try await service.runNow(id: rule.id)
            await loadRuns(rule)
        } catch {
            errorText = "运行失败：\(Self.describe(error))"
        }
    }

    public func loadRuns(_ rule: LiveScheduleRule) async {
        guard let service else { return }
        runs[rule.id] = (try? await service.runs(id: rule.id)) ?? runs[rule.id] ?? []
    }

    private static func describe(_ error: Error) -> String {
        AgentConfigStore.describe(error)
    }
}

/// A12: the words of the 定时任务 page.
public enum ScheduleText {
    static let weekdayNames = ["一", "二", "三", "四", "五", "六", "日"]

    /// 2026-11-22: who runs a rule, in its row and the editor's picker —
    /// the Agent, or its configuration and model ("Magpie · 42/deepseek-flash");
    /// a configuration the project no longer offers says so.
    public static func runner(_ rule: LiveScheduleRule, targets: [RerunTarget], agentName: String) -> String {
        guard let id = rule.configurationId else { return agentName }
        let title = configuration(id, targets: targets)
        guard targets.contains(where: { $0.id == id }) else { return title }
        return rule.model.map { "\(title) · \($0)" } ?? title
    }

    /// A configuration's name, or that the project no longer offers it.
    public static func configuration(_ id: String, targets: [RerunTarget]) -> String {
        guard let target = targets.first(where: { $0.id == id }) else {
            let name = id.hasPrefix("agent-config:") ? String(id.dropFirst("agent-config:".count)) : id
            return "配置 \(name) · 现在不能用"
        }
        return RerunText.title(target, among: targets)
    }

    public static func clock(_ minute: Int) -> String {
        String(format: "%02d:%02d", minute / 60, minute % 60)
    }

    /// "每天 09:00", "每周一、三 18:30", "每 6 小时", "一次 · 11月16日 09:00".
    public static func repeatText(_ repeatRule: LiveScheduleRepeat) -> String {
        switch repeatRule {
        case let .once(atMs): "一次 · \(date(atMs))"
        case let .daily(minute): "每天 \(clock(minute))"
        case let .weekly(days, minute):
            "每周" + days.sorted().compactMap { (1...7).contains($0) ? weekdayNames[$0 - 1] : nil }
                .joined(separator: "、") + " \(clock(minute))"
        case let .every(hours, _): hours % 24 == 0 ? "每 \(hours / 24) 天" : "每 \(hours) 小时"
        }
    }

    /// "09:00–18:00".
    public static func hours(_ hours: LiveActiveHours) -> String {
        "\(clock(hours.start))–\(clock(hours.end))"
    }

    public static func date(_ ms: UInt64) -> String {
        Date(timeIntervalSince1970: Double(ms) / 1000).formatted(.dateTime.month().day().hour().minute())
    }

    public static func next(_ rule: LiveScheduleRule) -> String {
        guard rule.enabled else { return "已停用" }
        if let retry = rule.quotaRetryMs { return "额度用完 · \(date(retry)) 后补跑" }
        if let retry = rule.hoursRetryMs { return "不在活跃时段 · \(date(retry)) 补跑" }
        if rule.busyPending { return "上一次还在跑 · 结束后补跑" }
        guard let next = rule.nextMs else { return "不会再运行" }
        return "下次 \(date(next))"
    }

    public static func outcome(_ run: LiveScheduleRun) -> String {
        var text = switch run.outcome {
        case "started": "已开会话"
        case "checkFailed": "检查没过，没唤醒模型"
        case "busy": "上一次还在跑，跳过"
        case "queued": "上一次还在跑，结束后补跑"
        case "outsideHours": "不在活跃时段，推迟"
        case "limited": "到了每天上限，跳过"
        case "quotaOut": run.manual ? "Codex 额度用完，没开会话" : "Codex 额度用完，重置后补跑"
        default: "失败"
        }
        // E16: a session asked for a result says how it came out.
        if run.outcome == "started", run.resultExpected {
            text += run.resultError != nil ? " · 结果不合格" : run.result != nil ? " · 结果合格" : " · 等结果"
        }
        if run.manual { text = "立即运行 · " + text } else if run.catchUp { text = "补跑 · " + text }
        return text
    }

    /// E16: a run counts as failed when its result did not fit.
    public static func failed(_ run: LiveScheduleRun) -> Bool {
        run.outcome == "failed" || run.resultError != nil
    }

    /// The line after the outcome: why the result failed, the result, or
    /// what the run noted.
    public static func detail(_ run: LiveScheduleRun) -> String? {
        if let error = run.resultError { return error }
        if let result = run.result { return compact(result) }
        return run.detail
    }

    /// E16: a structure or a result as one line of JSON, keys sorted.
    public static func compact(_ value: JSONValue) -> String {
        json(value, options: [.sortedKeys, .fragmentsAllowed, .withoutEscapingSlashes])
    }

    /// E16: the structure as the editor shows it.
    public static func pretty(_ value: JSONValue) -> String {
        json(value, options: [.sortedKeys, .prettyPrinted, .fragmentsAllowed, .withoutEscapingSlashes])
    }

    private static func json(_ value: JSONValue, options: JSONSerialization.WritingOptions) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: value.foundationObject, options: options),
              let text = String(data: data, encoding: .utf8) else { return "" }
        return text
    }

    /// E16: the editor's text as a structure: empty asks for none; text that
    /// is not a JSON object is refused before the host sees it.
    public static func schema(from text: String) -> Result<JSONValue?, ScheduleSchemaError> {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return .success(nil) }
        guard let data = trimmed.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]),
              object is [String: Any] else {
            return .failure(ScheduleSchemaError(message: "结果结构要是一个 JSON 对象，例如 {\"type\": \"object\"}"))
        }
        return .success(JSONValue.from(object))
    }
}

/// E16: why the editor's structure was refused.
public struct ScheduleSchemaError: Error, Equatable {
    public let message: String
}
