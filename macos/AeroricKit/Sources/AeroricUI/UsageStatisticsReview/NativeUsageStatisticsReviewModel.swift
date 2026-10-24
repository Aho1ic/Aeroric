#if DEBUG
/// Finite fictional review values, not production DTOs, a ledger or a scheduler.
enum USReviewScope: String, CaseIterable, Identifiable, Sendable {
    case task, project, global
    var id: String { rawValue }
    var title: String {
        switch self {
        case .task: "任务 T-A"
        case .project: "项目 P-A"
        case .global: "用户全局"
        }
    }
}

enum USReviewDimension: String, CaseIterable, Identifiable, Sendable {
    case executor, role, api, model
    var id: String { rawValue }
    var title: String {
        switch self {
        case .executor: "执行器"
        case .role: "角色"
        case .api: "API"
        case .model: "模型"
        }
    }
}

enum USReviewPeriod: String, CaseIterable, Identifiable, Sendable {
    case day, rolling, completeInterval
    var id: String { rawValue }
    var title: String {
        switch self {
        case .day: "示例自然日"
        case .rolling: "固定滚动 24h"
        case .completeInterval: "完整累计区间"
        }
    }
}

enum USReviewZone: String, CaseIterable, Identifiable, Sendable {
    case utc, plus8, minus7
    var id: String { rawValue }
    var title: String {
        switch self {
        case .utc: "UTC"
        case .plus8: "UTC+08"
        case .minus7: "UTC−07"
        }
    }
    var offsetMinutes: Int {
        switch self {
        case .utc: 0
        case .plus8: 480
        case .minus7: -420
        }
    }
}

struct USReviewContext: Hashable, Sendable {
    var scope: USReviewScope
    var period: USReviewPeriod
    var zone: USReviewZone

    /// Minutes relative to 2026-09-12 00:00 UTC; never reads a system clock.
    var interval: Range<Int> {
        switch period {
        case .day:
            // The +08 fixture deliberately selects Sep 13, as in the browser reference.
            let start = (zone == .plus8 ? 1440 : 0) - zone.offsetMinutes
            return start..<(start + 1440)
        case .rolling: return 720..<2160
        case .completeInterval: return -1440..<2880
        }
    }
    var title: String {
        switch period {
        case .day: "\(zone == .plus8 ? "2026.09.13" : "2026.09.12") · \(zone.title)"
        case .rolling: "09.12 12:00 → 09.13 12:00 UTC · 固定滚动 24h"
        case .completeInterval: "09.11 00:00 → 09.14 00:00 UTC · 完整累计区间"
        }
    }
}

enum USReviewBasis: String, Sendable {
    case reported = "已报告"
    case estimated = "估算"
    case unknown = "尚未取得"
    case conflict = "冲突，待核对"
    case observedZero = "声明范围内观测真零"
}

struct USReviewMetric: Equatable, Sendable {
    var value: Int?
    var basis: USReviewBasis
    var source: String
    var coverage: String
}

enum USReviewOccurrence: Equatable, Sendable {
    case instant(Int)
    case dateOnly
    case cumulative(Range<Int>)

    func belongs(to context: USReviewContext) -> Bool {
        switch self {
        case .instant(let minute): context.interval.contains(minute)
        case .dateOnly: context.period == .completeInterval || (context.period == .day && context.zone == .utc)
        case .cumulative(let interval):
            context.interval.lowerBound <= interval.lowerBound
                && context.interval.upperBound >= interval.upperBound
        }
    }
    var label: String {
        switch self {
        case .instant(1410): "2026-09-12 23:30 UTC · 可靠发生依据"
        case .instant: "2026-09-12 UTC · 固定结构化发生依据"
        case .dateOnly: "2026-09-12 · 仅日期精度，不造秒级时间"
        case .cumulative: "09.11 → 09.13 · 累计缺拆分，只完整包含时纳入"
        }
    }
}

struct USReviewUsage: Identifiable, Equatable, Sendable {
    let id: String
    let attempt: String
    let task: String?
    let project: String?
    let role: String?
    let executor: String
    let api: String?
    let model: String?
    let relation: String
    let shared: Bool
    let occurrence: USReviewOccurrence
    var tokens: USReviewMetric
    var input: Int?
    var cacheRead: Int?
    var output: Int?
    var revision: String

    func isInScope(_ scope: USReviewScope) -> Bool {
        switch scope {
        case .task: !shared && (task == "T-A" || task == "T-A-child")
        case .project: project == "P-A"
        case .global: true
        }
    }
    func dimension(_ dimension: USReviewDimension) -> String {
        switch dimension {
        case .executor: executor
        case .role: role ?? "缺角色"
        case .api: api ?? "缺 API"
        case .model: model ?? "缺模型"
        }
    }
}

struct USReviewBreakdown: Identifiable, Equatable, Sendable {
    var id: String { title }
    let title: String
    let tokens: Int
}

struct USReviewSummary: Equatable, Sendable {
    let knownTokens: Int?
    let recordCount: Int
    let freshInput: Int?
    let cacheRead: Int?
    let output: Int?
    let missing: [String]
    let coverage: String
    let hasSummaryGap: Bool
}

struct USReviewMoney: Identifiable, Equatable, Sendable {
    var id: String { usageID + basis.rawValue }
    let usageID: String
    let microUnits: Int
    let currency: String
    let basis: USReviewBasis
    let source: String
    var display: String { "\(currency) \(USReviewFormat.money(microUnits))" }
}

struct USReviewCosts: Equatable, Sendable {
    let amounts: [USReviewMoney]
    let reported: [USReviewMoney]
    let conversionUSD: Int?
    let limitations: [String]
    let subscription: String
}

struct USReviewPrice: Equatable, Sendable {
    let version: String
    let freshPerMillion: Int
    let cachePerMillion: Int
    let outputPerMillion: Int

    func estimate(_ usage: USReviewUsage) -> Int? {
        guard usage.id == "F", usage.api == "API-A", usage.model == "Model-A",
              let input = usage.input, let cache = usage.cacheRead,
              let output = usage.output, input >= cache else { return nil }
        // Rates are whole fictional USD per million; the result is integer micro-USD.
        return (input - cache) * freshPerMillion + cache * cachePerMillion + output * outputPerMillion
    }
}

struct USReviewPriceDraft: Equatable, Sendable {
    let context: USReviewContext
    let revision: Int
    let usageRevision: String
    let price: USReviewPrice
    let preview: Int?
}

struct USReviewSnapshot: Identifiable, Equatable, Sendable {
    let id: String
    let context: USReviewContext
    let asOf: String?
    let summary: USReviewSummary
    let fTokens: Int?
    let fRevision: String
    let costs: USReviewCosts
    let price: USReviewPrice
}

enum USReviewRole: String, CaseIterable, Identifiable, Sendable {
    case primary, reflection, governance, delegation
    var id: String { rawValue }
    var title: String {
        switch self {
        case .primary: "主执行"
        case .reflection: "反思"
        case .governance: "治理"
        case .delegation: "委派"
        }
    }
}

struct USReviewRoleSummary: Equatable, Sendable {
    let role: USReviewRole
    let knownTokens: Int?
    let rows: [String]
    let coverage: String
    let authorization: String
}

enum USReviewExportIntent: String, Sendable {
    case file = "文件导出意图 · 未执行"
    case external = "外部模型发送意图 · 未执行"
}

struct USReviewPreview: Equatable, Sendable {
    let role: USReviewRole
    let fields: [String]
    let lines: [String]
}

enum USReviewCleanupLayer: String, CaseIterable, Identifiable, Sendable {
    case details, summary, protectedRecords
    var id: String { rawValue }
    var title: String {
        switch self {
        case .details: "统计明细"
        case .summary: "该范围统计汇总"
        case .protectedRecords: "必要控制与核对记录（不可清）"
        }
    }
}

struct USReviewCleanupDraft: Equatable, Sendable {
    let context: USReviewContext
    let revision: Int
    let layer: USReviewCleanupLayer
    let recordIDs: Set<String>
    let impact: String
}

enum USReviewResourcePreset: String, CaseIterable, Identifiable, Sendable {
    case sharedCap, warning, displayUnknown, requiredUnknown, concurrency, capability, recovered
    var id: String { rawValue }
    var title: String {
        switch self {
        case .sharedCap: "项目共享上限已到"
        case .warning: "有据预警，不新增硬拦"
        case .displayUnknown: "仅展示统计未知"
        case .requiredUnknown: "必需准入计量未知"
        case .concurrency: "并发满，合法在途继续"
        case .capability: "必需硬限制能力不足"
        case .recovered: "资源恢复的固定预设"
        }
    }
}

enum USReviewExecution: String, Sendable {
    case running = "合法在途进行"
    case stopRequested = "已请求安全收束，未核实停止"
    case winding = "收束中，未核实停止"
    case stopped = "模拟事件已核实停止"
    case unknown = "原执行或副作用状态未知"
}

enum USReviewContinuePolicy: String, CaseIterable, Identifiable, Sendable {
    case manual, automatic, ordinaryWait
    var id: String { rawValue }
    var title: String {
        switch self {
        case .manual: "已收束，需明确继续意图"
        case .automatic: "预先明确配置的自动续行"
        case .ordinaryWait: "有效交办，准入前普通资源等待"
        }
    }
}

enum USReviewContinuationBlock: String, CaseIterable, Identifiable, Sendable {
    case userStop, revoked, prohibition, hardSafety, requiredMeasurement, otherResource
    case originalExecution, unknownSideEffect, changedInput, insufficientScope, invalidAssignment
    case missingContinue, retryLimit, requiredVerification, completedWork
    var id: String { rawValue }
    var title: String {
        switch self {
        case .userStop: "用户 Stop / 暂停仍有效"
        case .revoked: "授权已撤回"
        case .prohibition: "显式禁令"
        case .hardSafety: "宿主硬安全限制"
        case .requiredMeasurement: "必需计量 / 适用范围未知或过时"
        case .otherResource: "另一适用共享 / 独立资源仍到限"
        case .originalExecution: "原执行 / 占用未核实安全收束"
        case .unknownSideEffect: "副作用结果未知，不重试未知操作"
        case .changedInput: "目标 / 输入 / 依赖变化需局部对齐"
        case .insufficientScope: "授权范围不足"
        case .invalidAssignment: "交办无效或已取消"
        case .missingContinue: "已收束但无继续意图或预配策略"
        case .retryLimit: "独立重试上限用尽"
        case .requiredVerification: "准入必需监督 / 验证未满足"
        case .completedWork: "已完成工作不可重放"
        }
    }
}

struct USReviewResourceProjection: Equatable, Sendable {
    let projectKnown: Int?
    let projectLimit: Int
    let taskKnown: Int?
    let taskLimit: Int
    let newAdmissionBlocked: Bool
    let reasons: [String]
    let independentWork: String
    let basis: String
    let execution: USReviewExecution
    let verification: String
}

struct USReviewQuotaDraft: Equatable, Sendable {
    let context: USReviewContext
    let revision: Int
    let project: String
    let previousLimit: Int
    let proposedLimit: Int
}

struct USReviewProtectedFacts: Equatable, Sendable {
    let task = "T-A"
    let session = "demo-session-original"
    let artifact = "demo-deliverable-reference"
    let executor = "Codex"
    let api = "API-A"
    let model = "Model-A"
    let cwd = "demo-workspace"
    let ptyGeneration = 4
    let consumedControlTokens = 3600
    let resourceCycle = "固定控制周期 C-1"
    let retries = 2
    let retryLimit = 3
    let userStop = true
    let supervision = "仍待监督核验"
    let acceptance = "尚未用户验收"
}

struct USReviewPresentation: Equatable, Sendable {
    let opaque: Bool
    let strongContrast: Bool
    let disableAnimation: Bool
    static func resolve(classic: Bool, systemTransparency: Bool, systemContrast: Bool,
                        systemMotion: Bool, demoTransparency: Bool, demoContrast: Bool,
                        demoMotion: Bool) -> Self {
        .init(opaque: classic || systemTransparency || demoTransparency,
              strongContrast: systemContrast || demoContrast,
              disableAnimation: systemMotion || demoMotion)
    }
}

struct USReviewTrendBar: Identifiable, Equatable, Sendable {
    let id: String
    let label: String
    let parts: [Int]?
    let explanation: String
    var total: Int? { parts.map { $0.reduce(0, +) } }
}

struct USReviewTimeProjection: Equatable, Sendable {
    let cumulativeSeconds: Int
    let elapsedSeconds: Int
    let interval: Range<Int>
    let unallocatedRecordIDs: [String]
    let limitations: [String]
}

struct USReviewPreset: Identifiable, Equatable, Sendable {
    let id: String
    let title: String
    var group: String { String(id.prefix(3)) }
}

enum USReviewFormat {
    static func number(_ value: Int?) -> String {
        guard let value else { return "未知" }
        let digits = Array(String(value))
        return digits.enumerated().map { index, digit in
            (index > 0 && (digits.count - index) % 3 == 0 ? "," : "") + String(digit)
        }.joined()
    }
    static func money(_ microUnits: Int) -> String {
        let fraction = String(microUnits % 1_000_000)
        let padded = String(repeating: "0", count: 6 - fraction.count) + fraction
        return "\(microUnits / 1_000_000)." + String(padded.prefix(4))
    }
}
#endif
