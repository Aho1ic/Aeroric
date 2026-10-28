import Foundation

/// First-launch onboarding pages (M4, Agents-Anywhere structure plus the
/// import consent page from Q22).
public enum OnboardingStep: Int, CaseIterable, Identifiable, Sendable {
    case welcome, agents, projects, imports, done

    public var id: Int { rawValue }
    public var next: OnboardingStep? { OnboardingStep(rawValue: rawValue + 1) }
    public var previous: OnboardingStep? { OnboardingStep(rawValue: rawValue - 1) }

    public var title: String {
        switch self {
        case .welcome: "你的 Agent，在一个地方。"
        case .agents: "检测已安装的 Agent"
        case .projects: "选择要工作的项目"
        case .imports: "导入已有会话"
        case .done: "一切就绪。"
        }
    }

    public var symbol: String {
        switch self {
        case .welcome: "sparkles"
        case .agents: "cpu"
        case .projects: "folder.badge.plus"
        case .imports: "tray.and.arrow.down"
        case .done: "checkmark.seal"
        }
    }
}

public enum OnboardingFlow {
    public static let completedKey = "onboarding.completed"

    /// Present once the host is ready and onboarding was never finished or skipped.
    public static func shouldPresent(completed: Bool, phase: ConnectionPhaseSnapshot) -> Bool {
        !completed && phase == .ready
    }

    /// Agents the import page offers: importable ones first, then the rest as
    /// "即将支持" (D5). Since 2026-10-28 every listed agent is importable.
    public static func importRows(detected: [String]) -> [(agent: String, importable: Bool)] {
        ImportBook.importableAgents.map { ($0, true) }
    }
}
