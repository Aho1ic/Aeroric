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
        if run.manual { text = "立即运行 · " + text } else if run.catchUp { text = "补跑 · " + text }
        return text
    }
}
