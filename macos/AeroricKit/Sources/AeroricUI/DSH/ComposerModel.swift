import SwiftUI
import Observation

/// P5-04 Action 3：DSH composer 面板集。模型/effort 控制只显示 capability
/// 允许的值（DshSessionModels.routable/groups 与
/// supported_dsh_reasoning_effort 是权威；客户端不硬编码可用集合）。
/// 本文件是纯模型＋纯决策（无 UI、无副作用）。
public struct DshComposerModel: Equatable, Sendable {
    /// 宿主 capability 快照（session model catalog 权威）。
    public struct Catalog: Equatable, Sendable {
        public var currentProvider: String
        public var currentModel: String
        public var currentEffort: String?
        /// capability 允许的 (provider, model) 组合；空＝provider 不支持
        /// 显式选择（UI 不伪造可用项）。
        public var choices: [(provider: String, model: String)]
        /// capability 允许的 effort 值；空＝provider 接受模型但无 effort。
        public var efforts: [String]

        public init(
            currentProvider: String,
            currentModel: String,
            currentEffort: String?,
            choices: [(provider: String, model: String)],
            efforts: [String]
        ) {
            self.currentProvider = currentProvider
            self.currentModel = currentModel
            self.currentEffort = currentEffort
            self.choices = choices
            self.efforts = efforts
        }

        public static func == (lhs: Catalog, rhs: Catalog) -> Bool {
            lhs.currentProvider == rhs.currentProvider && lhs.currentModel == rhs.currentModel
                && lhs.currentEffort == rhs.currentEffort && lhs.efforts == rhs.efforts
                && lhs.choices.map(\.provider) == rhs.choices.map(\.provider)
                && lhs.choices.map(\.model) == rhs.choices.map(\.model)
        }
    }

    /// 选择是否被 capability 允许（模型/effort 控制只显示 capability
    /// 允许的值——门控判定，无 UI）。
    public static func isAllowed(
        provider: String, model: String, effort: String?, in catalog: Catalog
    ) -> Bool {
        guard catalog.choices.contains(where: { $0.provider == provider && $0.model == model })
        else { return false }
        guard let effort, !effort.isEmpty else { return true }
        return catalog.efforts.contains(effort)
    }

    /// 斜杠命令解析（DSH `session/{list_commands,execute_command}`；客户端
    /// 镜像：首个 token 以 / 开头即命令行）。
    public static func parseSlashCommand(_ text: String) -> (command: String, rest: String)? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("/"), !trimmed.isEmpty else { return nil }
        let parts = trimmed.split(separator: " ", maxSplits: 1, omittingEmptySubsequences: true)
        let command = String(parts[0])
        let rest = parts.count > 1 ? String(parts[1]) : ""
        return (command, rest)
    }

    /// composer 发送前校验：空 prompt 不发送；附件路径非空。
    public static func validateComposer(text: String, attachmentPaths: [String]) -> String? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return "消息为空" }
        return nil
    }
}

/// P5-04 Action 3：轨迹条目（宿主 DshSessionEvent/DshTrajectoryEntry 的
/// Swift 镜像；seq 升序 append-only 合并）。
public struct DshTrajectoryEntry: Equatable, Sendable {
    public var seq: UInt64
    public var type: String
    public var time: String?
    public var turn: UInt64?

    public init(seq: UInt64, type: String, time: String? = nil, turn: UInt64? = nil) {
        self.seq = seq
        self.type = type
        self.time = time
        self.turn = turn
    }
}

public enum DshTrajectoryLogic {
    /// seq 去重合并（React `mergeDshSessionEvents` 的 Swift 对齐）：升序
    /// append-only 快路径；否则 Map bySeq 去重重建＋排序。
    public static func merge(
        current: [DshTrajectoryEntry], incoming: [DshTrajectoryEntry]
    ) -> [DshTrajectoryEntry] {
        if current.isEmpty || incoming.isEmpty {
            return current + incoming
        }
        if let last = current.last, incoming[0].seq > last.seq {
            return current + incoming
        }
        var bySeq = [UInt64: DshTrajectoryEntry](
            uniqueKeysWithValues: current.map { ($0.seq, $0) }
        )
        for entry in incoming {
            bySeq[entry.seq] = entry
        }
        return bySeq.values.sorted { $0.seq < $1.seq }
    }
}
