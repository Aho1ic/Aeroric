import Foundation

/// Window-owned presentation state. No persistence, discovery or business writes.
public enum SessionDraftScope: Hashable, Sendable {
    case global
    case project(String)

    public var projectID: String? {
        if case let .project(id) = self { return id }
        return nil
    }
}

public struct SessionCapabilities: Equatable, Sendable {
    public var team = false
    public var review = false
    public var evolution = false
    public init() {}
    public var enabledNames: [String] {
        [(team, "团队协作"), (review, "独立审查"), (evolution, "任务工具进化")]
            .compactMap { $0.0 ? $0.1 : nil }
    }
}

/// Sanitized configuration projection supplied by the App. This is not a
/// configuration-file parser, profile directory or a new wire protocol.
public struct SessionConfiguration: Identifiable, Equatable, Sendable {
    public var id: String
    public var version: String
    public var title: String
    public var agent: String
    public var harness: String
    public var model: String?
    public var effort: String?
    public var options: TaskDraft.Options
    public var blockedReasons: [String]
    public var requiresModel: Bool
    public var requiresPermissionIdentity: Bool

    public init(id: String, version: String, title: String, agent: String,
                model: String?, effort: String?, options: TaskDraft.Options,
                harness: String? = nil, blockedReasons: [String] = [],
                requiresModel: Bool = false, requiresPermissionIdentity: Bool = false) {
        self.id = id
        self.version = version
        self.title = title
        self.agent = agent
        self.harness = harness ?? agent
        self.model = model
        self.effort = effort
        self.options = options
        self.blockedReasons = blockedReasons
        self.requiresModel = requiresModel
        self.requiresPermissionIdentity = requiresPermissionIdentity
    }
}

public struct NewSessionDraft: Equatable, Sendable {
    public enum Submission: Equatable, Sendable {
        case editing
        case sending
        case failed(String)
        case unknown(String)
    }

    public var task = TaskDraft(agent: "", permissionMode: "", prompt: "")
    public var configurationID: String?
    public var configurationVersion: String?
    public var recipeID: String?
    public var capabilities = SessionCapabilities()
    /// Minted before I/O and retained through failure/unknown, never by runTask.
    public private(set) var submissionID: String
    public private(set) var submission: Submission = .editing
    public private(set) var harnessIdentity: String?
    public private(set) var permissionAgent: String?
    public var selectedHarness: String { harnessIdentity ?? task.agent }

    public init(submissionID: String = UUID().uuidString) {
        self.submissionID = submissionID
    }

    public var isEditable: Bool {
        switch submission {
        case .editing, .failed: true
        case .sending, .unknown: false
        }
    }

    /// Selecting a harness does not choose a profile or import defaults.
    /// Keep the previous choices visible until explicitly selecting a profile.
    public mutating func selectHarness(_ agent: String) {
        guard isEditable, selectedHarness != agent else { return }
        harnessIdentity = agent
        task.agent = agent
    }

    public mutating func apply(_ configuration: SessionConfiguration) {
        guard isEditable, configuration.harness == selectedHarness else { return }
        task.agent = configuration.agent
        harnessIdentity = configuration.harness
        configurationID = configuration.id
        configurationVersion = configuration.version
        task.model = configuration.model
        task.effort = configuration.effort
        // Permissions, prompt, attachments and role choices are independent.
    }

    public mutating func selectPermission(_ mode: String) {
        guard isEditable else { return }
        task.permissionMode = mode
        permissionAgent = task.agent
    }

    fileprivate mutating func renewAfterKnownFailure() {
        guard case .failed = submission else { return }
        submissionID = UUID().uuidString
        submission = .editing
    }

    fileprivate mutating func reconcileRejection(_ reason: String) {
        guard case .unknown = submission else { return }
        submission = .failed(reason)
    }

    public func blockingReasons(scope: SessionDraftScope,
                                configurations: [SessionConfiguration],
                                projectExists: Bool) -> [String] {
        var reasons: [String] = []
        if scope.projectID == nil { reasons.append("请从项目入口明确选择工作目录；不会自动选择首项目。") }
        else if !projectExists { reasons.append("原项目不可用；草稿已保留，请核对原目标。") }
        guard let configuration = configurations.first(where: {
            $0.id == configurationID && $0.version == configurationVersion && $0.agent == task.agent
        }) else {
            reasons.append("宿主尚未提供匹配的 Agent 配置及版本，当前不能发送。")
            return reasons + unsupportedReasons
        }
        reasons += configuration.blockedReasons
        if configuration.requiresModel && task.model == nil {
            reasons.append("请明确选择此配置支持的模型；不会假设默认值。")
        }
        if configuration.requiresPermissionIdentity && permissionAgent != task.agent {
            reasons.append("请为当前 Agent 明确选择权限；不会继承其他 Agent 的授权。")
        }
        reasons += task.validate(against: configuration.options).sorted { $0.key < $1.key }.map(\.value)
        if !configuration.options.agents.contains(task.agent)
            || !configuration.options.permissionModes.contains(task.permissionMode) {
            reasons.append("当前配置尚未确认 Harness 或权限能力。")
        }
        if let model = task.model, !configuration.options.models.contains(model) {
            reasons.append("此配置不支持已选模型；请明确重新选择。")
        }
        if let effort = task.effort, !configuration.options.efforts.contains(effort) {
            reasons.append("此配置不支持已选推理强度；请明确重新选择。")
        }
        return reasons + unsupportedReasons
    }

    private var unsupportedReasons: [String] {
        var reasons = capabilities.enabledNames.map { "\($0)尚未接通真实交办能力。" }
        if recipeID != nil { reasons.append("工作配方尚未接通真实交办能力。") }
        if !task.attachmentPaths.isEmpty { reasons.append("附件引用尚未有宿主消费回执，不能随交办忽略。") }
        if task.worktree != nil { reasons.append("工作目录覆盖尚未有宿主消费回执，不能随交办忽略。") }
        return reasons
    }

    public mutating func beginSubmission() -> Bool {
        guard isEditable else { return false }
        submission = .sending
        return true
    }

    public mutating func fail(_ reason: String, acceptanceUnknown: Bool) {
        guard submission == .sending else { return }
        submission = acceptanceUnknown ? .unknown(reason) : .failed(reason)
    }
}

/// Successful submissions retain form choices only. Do not keep a copy of
/// the sent prompt, attachment references, submission identity or worktree.
private struct SuccessfulSessionPreferences: Equatable, Sendable {
    let agent: String
    let harness: String
    let model: String?
    let effort: String?
    let permissionMode: String
    let configurationID: String?
    let configurationVersion: String?
    let recipeID: String?
    let capabilities: SessionCapabilities
    let permissionAgent: String?

    init(_ draft: NewSessionDraft) {
        agent = draft.task.agent
        harness = draft.selectedHarness
        model = draft.task.model
        effort = draft.task.effort
        permissionMode = draft.task.permissionMode
        configurationID = draft.configurationID
        configurationVersion = draft.configurationVersion
        recipeID = draft.recipeID
        capabilities = draft.capabilities
        permissionAgent = draft.permissionAgent
    }

    func restoreChoices(into draft: inout NewSessionDraft) {
        draft.task.agent = agent
        draft.restorePresentationIdentity(harness: harness, permissionAgent: permissionAgent)
        draft.task.model = model
        draft.task.effort = effort
        draft.task.permissionMode = permissionMode
        draft.configurationID = configurationID
        draft.configurationVersion = configurationVersion
        draft.recipeID = recipeID
        draft.capabilities = capabilities
    }
}

private extension NewSessionDraft {
    mutating func restorePresentationIdentity(harness: String, permissionAgent: String?) {
        harnessIdentity = harness
        // A retained form choice, never an execution approval or credential.
        self.permissionAgent = permissionAgent
    }
}

public struct NewSessionDraftBook: Equatable, Sendable {
    private var drafts: [SessionDraftScope: NewSessionDraft] = [:]
    private var successfulPreferences: [String: SuccessfulSessionPreferences] = [:]
    public init() {}

    public mutating func open(_ scope: SessionDraftScope) {
        guard drafts[scope] == nil else { return }
        var next = NewSessionDraft()
        if let projectID = scope.projectID, let previous = successfulPreferences[projectID] {
            previous.restoreChoices(into: &next)
        }
        drafts[scope] = next
    }

    public subscript(scope: SessionDraftScope) -> NewSessionDraft? { drafts[scope] }

    public mutating func edit(_ scope: SessionDraftScope, _ update: (inout NewSessionDraft) -> Void) {
        open(scope)
        guard var value = drafts[scope], value.isEditable else { return }
        let previous = value
        update(&value)
        if value != previous { value.renewAfterKnownFailure() }
        drafts[scope] = value
    }

    public mutating func begin(_ scope: SessionDraftScope) -> NewSessionDraft? {
        guard var value = drafts[scope], value.beginSubmission() else { return nil }
        drafts[scope] = value
        return value
    }

    public mutating func fail(_ scope: SessionDraftScope, submissionID: String,
                              reason: String, acceptanceUnknown: Bool) {
        guard var value = drafts[scope], value.submissionID == submissionID else { return }
        value.fail(reason, acceptanceUnknown: acceptanceUnknown)
        drafts[scope] = value
    }

    /// Only a positive receipt clears input. A response for another draft
    /// cannot erase newer work. Callers navigate using the returned task ID.
    @discardableResult
    public mutating func acknowledge(_ scope: SessionDraftScope, submissionID: String) -> Bool {
        guard let sent = drafts[scope], sent.submissionID == submissionID,
              sent.submission == .sending else { return false }
        if let projectID = scope.projectID { successfulPreferences[projectID] = SuccessfulSessionPreferences(sent) }
        drafts.removeValue(forKey: scope)
        return true
    }

    /// Called only after the typed client verifies identity, fingerprint and
    /// every adopted option in an accepted lookup receipt.
    @discardableResult
    public mutating func reconcileAccepted(_ scope: SessionDraftScope, submissionID: String) -> Bool {
        guard let sent = drafts[scope], sent.submissionID == submissionID,
              case .unknown = sent.submission else { return false }
        if let projectID = scope.projectID { successfulPreferences[projectID] = SuccessfulSessionPreferences(sent) }
        drafts.removeValue(forKey: scope)
        return true
    }

    public mutating func reconcileRejected(_ scope: SessionDraftScope, submissionID: String, reason: String) {
        guard var value = drafts[scope], value.submissionID == submissionID else { return }
        value.reconcileRejection(reason)
        drafts[scope] = value
    }
}

public enum SessionNavigation {
    public static let globalDraft = "draft.global"
    public static let allTasks = "section.tasks"
    public static func draft(projectID: String) -> String { "draft.project.\(projectID)" }
    public static func task(_ id: String) -> String { "task.\(id)" }
    /// Imported (read-only) session row (M4).
    public static func imported(_ id: String) -> String { "imported.\(id)" }
    public static func importedID(_ selection: String?) -> String? {
        guard let selection, selection.hasPrefix("imported.") else { return nil }
        return String(selection.dropFirst("imported.".count))
    }
    public static func draftScope(_ selection: String?) -> SessionDraftScope? {
        guard let selection else { return nil }
        if selection == globalDraft { return .global }
        let prefix = "draft.project."
        guard selection.hasPrefix(prefix) else { return nil }
        return .project(String(selection.dropFirst(prefix.count)))
    }
    public static func taskID(_ selection: String?) -> String? {
        guard let selection, selection.hasPrefix("task.") else { return nil }
        return String(selection.dropFirst("task.".count))
    }

    /// All creation surfaces use this rule and the same window selection.
    /// A remembered project on a global/tool page never becomes an implicit
    /// submission target.
    public static func newDraftSelection(from selection: String?, items: [SidebarItem]) -> String {
        let project = items.first {
            $0.group == .projects
                && ($0.id == selection || $0.children?.contains { $0.id == selection } == true)
        }
        return project.map { draft(projectID: $0.id) } ?? globalDraft
    }
}

/// Only an explicit native picker/drop supplies these URLs. Never interpret
/// pasted strings, network URLs or remote file authorities as attachments.
/// This retains references without resolving symlinks, probing or reading files.
public enum SessionAttachmentReferences {
    @discardableResult
    public static func append(_ urls: [URL], to paths: inout [String]) -> Bool {
        var accepted = false
        for url in urls {
            guard url.isFileURL, url.host == nil || url.host == "" || url.host == "localhost",
                  url.user == nil, url.password == nil, url.query == nil, url.fragment == nil,
                  url.path.hasPrefix("/"), url.path != "/" else { continue }
            accepted = true
            if !paths.contains(url.path) { paths.append(url.path) }
        }
        return accepted
    }
}
