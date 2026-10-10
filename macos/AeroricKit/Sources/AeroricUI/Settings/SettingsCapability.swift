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

    /// 内置 DSH 的 effort 词表，与 core/React 的 off/high/max 保持一致。
    public static let dshEffortLevels: [String] = ["off", "high", "max"]

    /// Claude/Codex 保留原有的 effort 选项，与 DSH 词表分离。
    public static let claudeCodexEffortLevels: [String] = ["low", "medium", "high"]

    /// 按族提供词表，不代表具体 Agent 有保存权限；
    /// `settings.update_dsh_reasoning_effort` 仅允许内置 `dsh`。
    public static func effortLevels(for family: String) -> [String] {
        switch family {
        case "omp":
            return ompThinkingLevels
        case "dsh":
            return dshEffortLevels
        case "claude", "codex":
            return claudeCodexEffortLevels
        default:
            return []
        }
    }

    /// 该家是否支持该 effort；与选项展示使用同一词表。
    public static func supportsEffort(_ effort: String, family: String) -> Bool {
        effortLevels(for: family).contains(effort)
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
