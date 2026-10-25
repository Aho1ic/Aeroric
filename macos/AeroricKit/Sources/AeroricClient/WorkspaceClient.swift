import Foundation
import AeroricProtocol

/// 项目/任务列表的类型化客户端（P5-03）。宿主应答的任务记录是开放对象
/// （P2-02 的 Record 语义），本类型做**宽容镜像**：认得的字段取走，
/// 不认得的忽略——UI 不推断业务，只呈现。
public struct WorkspaceClient: Sendable {
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    // MARK: - project.list

    public struct Project: Sendable, Equatable {
        public var id: String
        public var path: String
    }

    public func listProjects() async throws -> [Project] {
        let frame = try await connection.request("project.list", params: .null)
        guard let items = frame.result["projects"]?.arrayValue else {
            throw MalformedReply("project.list result missing projects array")
        }
        return items.compactMap { item in
            guard let id = item["id"]?.stringValue else { return nil }
            let path = item["path"]?.stringValue ?? ""
            return Project(id: id, path: path)
        }
    }

    // MARK: - project.open

    public struct OpenedProject: Sendable, Equatable {
        public var id: String
        public var path: String
        public var reused: Bool
        public var revision: UInt64
    }

    /// ADR-013：调用方铸 newId；宿主回执确认后的项目条目。
    @discardableResult
    public func openProject(path: String, newId: String) async throws -> OpenedProject {
        let frame = try await connection.request(
            "project.open",
            params: .object(["path": .string(path), "newId": .string(newId)])
        )
        guard
            let project = frame.result["project"],
            let id = project["id"]?.stringValue,
            let projectPath = project["path"]?.stringValue
        else {
            throw MalformedReply("project.open reply missing project.id/path")
        }
        let reused = frame.result["reused"]?.boolValue ?? false
        let revision = frame.result["revision"]?.numberText.flatMap(UInt64.init) ?? 0
        return OpenedProject(id: id, path: projectPath, reused: reused, revision: revision)
    }

    // MARK: - task.list

    public struct Task: Sendable, Equatable {
        public var id: String
        public var projectID: String
        public var title: String
        public var status: String
        public var agent: String?
        public var createdAtMs: UInt64?
        public var updatedAtMs: UInt64?
        public var archived: Bool
        public var starred: Bool
        public var sessionId: String?
        /// 十进制字符串（Quantity 约定）；终端轮询寻址用。
        public var generation: UInt64?
        public var worktreePath: String? = nil
        public var worktreeBranch: String? = nil
        public var additions: UInt64? = nil
        public var deletions: UInt64? = nil
    }

    /// 展示标题：记录无 title 字段时取 prompt 首行（展示推导，非业务推断）。
    static func displayTitle(_ record: JSONValue) -> String {
        if let title = record["title"]?.stringValue, !title.isEmpty { return title }
        let prompt = record["prompt"]?.stringValue ?? ""
        let firstLine = prompt.split(separator: "\n", omittingEmptySubsequences: true).first
        return firstLine.map(String.init) ?? (record["id"]?.stringValue ?? "（未命名）")
    }

    static func parseTask(_ record: JSONValue, projectID fallback: String?) -> Task? {
        guard let id = record["id"]?.stringValue else { return nil }
        let projectID = record["projectId"]?.stringValue ?? fallback ?? ""
        let status = record["status"]?.stringValue ?? "unknown"
        return Task(
            id: id,
            projectID: projectID,
            title: displayTitle(record),
            status: status,
            agent: record["agent"]?.stringValue,
            createdAtMs: record["createdAt"]?.numberText.flatMap(UInt64.init),
            updatedAtMs: record["updatedAt"]?.numberText.flatMap(UInt64.init)
                ?? record["completedAt"]?.numberText.flatMap(UInt64.init),
            archived: record["archived"]?.boolValue ?? false,
            starred: record["starred"]?.boolValue ?? false,
            sessionId: record["sessionId"]?.stringValue,
            generation: record["generation"]?.numberText.flatMap(UInt64.init),
            worktreePath: record["worktreePath"]?.stringValue,
            worktreeBranch: record["worktreeBranch"]?.stringValue,
            additions: record["additions"]?.numberText.flatMap(UInt64.init),
            deletions: record["deletions"]?.numberText.flatMap(UInt64.init)
        )
    }

    public func listTasks(projectPath: String? = nil) async throws -> [Task] {
        var params: [String: JSONValue] = [:]
        if let projectPath {
            params["projectPath"] = .string(projectPath)
        }
        let frame = try await connection.request(
            "task.list",
            params: params.isEmpty ? .null : .object(params)
        )
        guard let items = frame.result["tasks"]?.arrayValue else {
            throw MalformedReply("task.list result missing tasks array")
        }
        return items.compactMap { Self.parseTask($0, projectID: nil) }
    }

    // MARK: - task.get / task.run / task.cancel

    public func getTask(taskId: String) async throws -> Task? {
        let frame = try await connection.request(
            "task.get",
            params: .object(["taskId": .string(taskId)])
        )
        guard let record = frame.result["task"] else { return nil }
        return Self.parseTask(record, projectID: frame.result["projectId"]?.stringValue)
    }

    public struct RunRequest: Sendable, Equatable {
        public var projectPath: String
        public var agent: String
        public var prompt: String
        public var taskId: String
        public var model: String?
        public var effort: String?
        public var permissionMode: String?
        public var worktree: String?
        public var attachmentPaths: [String]

        public init(
            projectPath: String,
            agent: String,
            prompt: String,
            taskId: String,
            model: String? = nil,
            effort: String? = nil,
            permissionMode: String? = nil,
            worktree: String? = nil,
            attachmentPaths: [String] = []
        ) {
            self.projectPath = projectPath
            self.agent = agent
            self.prompt = prompt
            self.taskId = taskId
            self.model = model
            self.effort = effort
            self.permissionMode = permissionMode
            self.worktree = worktree
            self.attachmentPaths = attachmentPaths
        }

        var json: JSONValue {
            var object: [String: JSONValue] = [
                "projectPath": .string(projectPath),
                "agent": .string(agent),
                "prompt": .string(prompt),
                "taskId": .string(taskId),
            ]
            if let model {
                object["selectedModel"] = .string(model)
                object["model"] = .string(model) // Existing client compatibility alias.
            }
            if let effort {
                object["reasoningEffort"] = .string(effort)
                object["effort"] = .string(effort)
            }
            if let permissionMode { object["permissionMode"] = .string(permissionMode) }
            if let worktree { object["worktree"] = .string(worktree) }
            if !attachmentPaths.isEmpty {
                object["attachments"] = .array(attachmentPaths.map { .string($0) })
            }
            return .object(object)
        }
    }

    /// 创建任务（Action 3：idempotency key 由调用方以 taskId 承担——
    /// 同 taskId 重发＝reused）。
    @discardableResult
    public func runTask(_ request: RunRequest) async throws -> String {
        let frame = try await connection.request("task.run", params: request.json)
        guard let taskId = frame.result["taskId"]?.stringValue ?? frame.result["task"]?["id"]?.stringValue else {
            throw MalformedReply("task.run reply missing taskId")
        }
        return taskId
    }

    public func cancelTask(taskId: String) async throws {
        _ = try await connection.request(
            "task.cancel",
            params: .object(["taskId": .string(taskId)])
        )
    }

    /// Versioned directory scoped to an explicit project. No raw settings IO.
    public func configurations(projectID: String) async throws -> NativeConfigurationCatalog {
        let frame = try await connection.request(
            "agent.configurations", params: .object(["projectId": .string(projectID)])
        )
        return try Self.decodeConfigurations(frame.result, projectID: projectID)
    }

    static func decodeConfigurations(_ value: JSONValue, projectID: String) throws -> NativeConfigurationCatalog {
        let catalog: NativeConfigurationCatalog = try decodeSubmissionValue(value)
        guard catalog.schemaVersion == 1, catalog.projectId == projectID,
              !catalog.revision.isEmpty, Quantity(catalog.asOf) != nil,
              Set(catalog.configurations.map(\.id)).count == catalog.configurations.count,
              catalog.configurations.allSatisfy({
                  !$0.id.isEmpty && !$0.agent.isEmpty && $0.version == catalog.revision
                  && ["codex", "claude", "omp", "dsh", "claude_gpt55"].contains($0.harness)
              })
        else { throw MalformedReply("invalid configuration directory identity/version") }
        return catalog
    }

    public func submitTask(_ request: NativeTaskSubmission) async throws -> NativeSubmissionReceipt {
        let frame = try await connection.request("task.run", params: request.json)
        return try Self.decodeSubmissionReceipt(frame.result, request: request)
    }

    static func decodeSubmissionReceipt(_ value: JSONValue, request: NativeTaskSubmission) throws -> NativeSubmissionReceipt {
        let receipt: NativeSubmissionReceipt = try decodeSubmissionValue(value)
        guard receipt.matches(request) else { throw MalformedReply("submission receipt does not match original options") }
        return receipt
    }

    public func submission(_ request: NativeTaskSubmission) async throws -> NativeSubmissionReceipt? {
        let frame = try await connection.request("task.submission.get", params: .object([
            "submissionId": .string(request.submissionId), "fingerprint": .string(request.fingerprint),
        ]))
        return try Self.decodeSubmissionLookup(frame.result, request: request)
    }

    static func decodeSubmissionLookup(_ value: JSONValue, request: NativeTaskSubmission) throws -> NativeSubmissionReceipt? {
        let lookup: NativeSubmissionLookup = try decodeSubmissionValue(value)
        guard lookup.schemaVersion == 1, lookup.submissionId == request.submissionId,
              lookup.fingerprint == request.fingerprint,
              lookup.receipt.map({ $0.matches(request) }) ?? true
        else { throw MalformedReply("submission lookup does not match original identity") }
        return lookup.receipt
    }

    private static func decodeSubmissionValue<T: Decodable>(_ value: JSONValue) throws -> T {
        do {
            return try JSONDecoder().decode(T.self, from: JSONSerialization.data(withJSONObject: value.foundationObject))
        } catch {
            throw MalformedReply("invalid typed submission reply")
        }
    }

    public struct MalformedReply: Error, Equatable {
        public let reason: String
        public init(_ reason: String) { self.reason = reason }
    }
}
