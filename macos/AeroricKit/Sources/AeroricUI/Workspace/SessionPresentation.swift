import Foundation

/// P5-02 Action 3：由模式＋能力决定一个会话用什么 renderer 呈现。
/// 纯决策（无 UI、无副作用）：native 模式下「结构化能力未验证」的会话
/// 显示状态卡与「切换到高级模式」按钮（用户触发；**不自动改全局偏好**）。
public enum SessionPresentation: Equatable, Sendable {
    /// 原生面板（结构化能力已验证的会话；P5-04 前仅显示任务元数据与
    /// 明确的未接通状态——不伪造 composer/approval）。
    case native
    /// 真实终端（advanced 模式，或 native 下该会话不支持原生呈现）。
    case terminal
    /// native 模式但该会话不支持原生呈现：显示状态卡（UI 层渲染
    /// 「此 Agent 的当前交互需要高级模式」＋切换按钮）。
    case requiresAdvanced(reason: String)

    /// 决策入口。`structuredCapabilitiesVerified`＝该会话的宿主侧能力协商
    /// 已通过（§2.4 表：DSH 已验证结构化能力才有原生 composer；P5-04 兑现）。
    public static func resolve(
        mode: InteractionMode,
        structuredCapabilitiesVerified: Bool
    ) -> SessionPresentation {
        switch mode {
        case .advanced:
            return .terminal
        case .native:
            return structuredCapabilitiesVerified ? .native : .requiresAdvanced(
                reason: "此 Agent 的当前交互需要高级模式"
            )
        }
    }
}

/// P5-02 Action 4：切换的**顺序**即契约——暂缓 IME → 保存草稿/滚动锚 →
/// 释放旧输入租约 → 挂新 presentation → 获取必要 lease → 恢复 focus。
/// 全程**不调用** start/resume/cancel/reset_task_process（任务协议不变）。
/// 步骤是显式枚举：测试断言顺序与内容，UI 按序执行。
public enum ModeSwitchStep: String, Equatable, Sendable, CaseIterable {
    case deferIME = "defer-ime"
    case saveDraftAndScrollAnchor = "save-draft"
    case releaseOldLease = "release-lease"
    case attachPresentation = "attach-presentation"
    case acquireLease = "acquire-lease"
    case restoreFocus = "restore-focus"
}

public struct ModeSwitchSequence: Equatable, Sendable {
    public let from: InteractionMode
    public let to: InteractionMode
    public let steps: [ModeSwitchStep]

    /// 构造一次切换的完整步骤表。同模式切换＝空表（no-op）。
    public init(from: InteractionMode, to: InteractionMode) {
        self.from = from
        self.to = to
        self.steps = from == to ? [] : ModeSwitchStep.allCases.map { $0 }
    }

    /// 切换过程**禁止**出现的任务协议方法（§2.4：不 start/resume/cancel/
    /// reset——模式切换与任务协议转换的差别在测试里成立）。
    public static let forbiddenMethods: Set<String> = [
        "terminal.run_task",
        "terminal.resume_task",
        "terminal.cancel_task",
        "terminal.reset_task_process",
        "task.run",
        "task.resume",
        "task.cancel",
    ]

    /// 校验一次切换记录的请求清单里没有禁用方法。
    public static func violates(requestedMethods: [String]) -> Bool {
        requestedMethods.contains { forbiddenMethods.contains($0) }
    }
}
