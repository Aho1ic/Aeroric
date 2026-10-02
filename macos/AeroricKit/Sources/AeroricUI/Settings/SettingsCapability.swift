import Foundation

/// P5-05 Action 5：四族记录、内置别名、自定义档案与用量列表的**能力
/// 门控**（agent_tools.rs/agent_ops.rs/schema.rs 的客户端镜像；纯决策无 UI）。
/// 原生设置不发 OMP 不支持的 effort。
public enum SettingsCapability {
    /// 四族内置别名（ProtocolFamily × BuiltInAgent 的客户端词汇表；与
    /// `ProtocolModels.swift` 的内置枚举对齐）。
    public static let builtinAgents: [(family: String, id: String)] = [
        ("claude", "claude"), ("claude", "claude_gpt55"),
        ("codex", "codex"),
        ("dsh", "dsh"),
        ("omp", "omp"),
    ]

    /// OMP 思考级别（`OMP_THINKING_LEVELS` 词汇表：off/minimal/low/medium/
    /// high/xhigh/max，无 ultra）。
    public static let ompThinkingLevels: [String] = [
        "off", "minimal", "low", "medium", "high", "xhigh", "max",
    ]

    /// DSH/claude 家的 effort 白名单（`dsh_permission_preset` 同源语义；
    /// OMP 白名单在 ompThinkingLevels，两者不相交时以家为单位门控）。
    public static let dshEffortLevels: [String] = ["low", "medium", "high"]

    /// 该家是否支持该 effort（Action 5：原生设置不发 OMP 不支持的 effort）。
    public static func supportsEffort(_ effort: String, family: String) -> Bool {
        switch family {
        case "omp":
            return ompThinkingLevels.contains(effort)
        case "dsh":
            return dshEffortLevels.contains(effort)
        case "claude", "codex":
            return dshEffortLevels.contains(effort)
        default:
            return false
        }
    }

    /// 自定义档案 id 是否合法（AgentFamily 派生：已知内置 → 对应 family；
    /// 未知 id → 自定义档案，family 可选显式声明）。
    public static func familyOf(agentID: String, declared: String?) -> String? {
        if let builtin = builtinAgents.first(where: { $0.id == agentID }) {
            return builtin.family
        }
        return declared
    }

    /// 升级渠道（versions.rs 的 npm/Homebrew/native 三渠道；显示计划的
    /// 安装渠道——用户确认后执行，不因打开设置自动 upgrade）。
    public static let updateChannels: [String] = ["npm", "homebrew", "native"]

    /// 升级前置确认（Action 4：显示安装渠道与影响任务；返回是否允许执行）。
    public static func canStartUpdate(
        channel: String, confirmedByUser: Bool
    ) -> Bool {
        guard confirmedByUser else { return false }
        return updateChannels.contains(channel)
    }

    /// 离线模型目录提示（验收：离线目录有来源/更新时间提示而非虚假在线
    /// 结果——判定 pure 函数）。
    public static func modelCatalogHint(lastCheckedAtMs: UInt64?, isOnline: Bool) -> String? {
        if isOnline { return nil }
        guard let lastCheckedAtMs else {
            return "模型目录离线：来源与更新时间未知"
        }
        let date = Date(timeIntervalSince1970: TimeInterval(lastCheckedAtMs) / 1000)
        let formatted = date.formatted(date: .abbreviated, time: .shortened)
        return "模型目录离线：最后更新 \(formatted)"
    }
}
