import Foundation

/// P5-03 Action 3：任务草稿。字段集按规格：agent/model/effort/permission/
/// prompt/attachments/worktree。**选项由宿主 capability 决定**（注入），
/// 客户端不硬编码可用集合；创建时 revision 与 idempotency key 由调用方
/// 提交（本类型只带载体）。
public struct TaskDraft: Equatable, Sendable {
    public var agent: String
    public var model: String?
    public var effort: String?
    public var permissionMode: String
    public var prompt: String
    public var attachmentPaths: [String]
    public var worktree: String?

    /// 宿主 capability 决定的选项集（注入）。
    public struct Options: Equatable, Sendable {
        public var agents: [String]
        public var models: [String]
        public var efforts: [String]
        public var permissionModes: [String]

        public init(
            agents: [String],
            models: [String],
            efforts: [String],
            permissionModes: [String]
        ) {
            self.agents = agents
            self.models = models
            self.efforts = efforts
            self.permissionModes = permissionModes
        }

        /// 默认选项＝空集：宿主没有回答前，UI 不伪造可用项。
        public static let none = Options(agents: [], models: [], efforts: [], permissionModes: [])
    }

    public init(
        agent: String,
        model: String? = nil,
        effort: String? = nil,
        permissionMode: String,
        prompt: String,
        attachmentPaths: [String] = [],
        worktree: String? = nil
    ) {
        self.agent = agent
        self.model = model
        self.effort = effort
        self.permissionMode = permissionMode
        self.prompt = prompt
        self.attachmentPaths = attachmentPaths
        self.worktree = worktree
    }

    /// The empty-prompt reason. The send button already says it by being
    /// disabled, so the new-session page does not print it (2026-10-27).
    public static let emptyPromptMessage = "提示词不能为空"

    /// 草稿校验：prompt 非空、agent 非空且在宿主提供的选项内、model/effort
    /// 若给值也须在选项内。错误按字段返回（逐字段提示，不合并）。
    public func validate(against options: Options) -> [String: String] {
        var errors: [String: String] = [:]
        if prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            errors["prompt"] = Self.emptyPromptMessage
        }
        if agent.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            errors["agent"] = "Agent 不能为空"
        } else if !options.agents.isEmpty, !options.agents.contains(agent) {
            errors["agent"] = "Agent \(agent) 不在宿主提供的选项内"
        }
        if let model, !model.isEmpty, !options.models.isEmpty, !options.models.contains(model) {
            errors["model"] = "Model \(model) 不在宿主提供的选项内"
        }
        if let effort, !effort.isEmpty, !options.efforts.isEmpty, !options.efforts.contains(effort) {
            errors["effort"] = "Effort \(effort) 不在宿主提供的选项内"
        }
        if permissionMode.isEmpty {
            errors["permissionMode"] = "权限模式不能为空"
        } else if !options.permissionModes.isEmpty, !options.permissionModes.contains(permissionMode) {
            errors["permissionMode"] = "权限模式 \(permissionMode) 不在宿主提供的选项内"
        }
        return errors
    }
}
