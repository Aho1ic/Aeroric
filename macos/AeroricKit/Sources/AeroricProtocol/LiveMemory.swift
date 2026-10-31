import Foundation

/// B5-1 (2026-10-31): one long-term memory item as the host reports it.
public struct LiveMemoryItem: Equatable, Sendable, Identifiable {
    public var id: String
    /// nil is global.
    public var projectId: String?
    /// `profile` (关于你) or `fact` (项目上下文).
    public var kind: String
    /// `language` `interaction` `preference` `constraint` (profile only).
    public var category: String?
    public var text: String
    /// `userExplicit` `agentConfirmed` `toolVerified` `modelInferred`.
    public var assertion: String
    /// `pending` `active` `superseded` `archived`.
    public var state: String
    public var pinned: Bool
    public var evidence: [LiveMemoryEvidence]
    public var supersedes: String?
    public var supersededBy: String?
    public var updatedMs: UInt64
    /// B5-3a: an experience's problem, fix and where it holds.
    public var experience: LiveExperienceDetail?
    /// `low` `medium` `high` (experiences only).
    public var confidence: String?
    public var stats: LiveMemoryStats

    public init(id: String, projectId: String? = nil, kind: String = "fact", category: String? = nil,
                text: String, assertion: String = "userExplicit", state: String = "active",
                pinned: Bool = false, evidence: [LiveMemoryEvidence] = [], supersedes: String? = nil,
                supersededBy: String? = nil, updatedMs: UInt64 = 0, experience: LiveExperienceDetail? = nil,
                confidence: String? = nil, stats: LiveMemoryStats = LiveMemoryStats()) {
        self.experience = experience
        self.confidence = confidence
        self.stats = stats
        self.id = id
        self.projectId = projectId
        self.kind = kind
        self.category = category
        self.text = text
        self.assertion = assertion
        self.state = state
        self.pinned = pinned
        self.evidence = evidence
        self.supersedes = supersedes
        self.supersededBy = supersededBy
        self.updatedMs = updatedMs
    }

    public static func decode(_ value: JSONValue?) -> LiveMemoryItem? {
        guard let f = value?.objectValue, let id = f["id"]?.stringValue,
              let text = f["text"]?.stringValue else { return nil }
        return LiveMemoryItem(
            id: id, projectId: f["projectId"]?.stringValue, kind: f["kind"]?.stringValue ?? "fact",
            category: f["category"]?.stringValue, text: text,
            assertion: f["assertion"]?.stringValue ?? "userExplicit", state: f["state"]?.stringValue ?? "active",
            pinned: f["pinned"]?.boolValue ?? false,
            evidence: (f["evidence"]?.arrayValue ?? []).compactMap(LiveMemoryEvidence.decode),
            supersedes: f["supersedes"]?.stringValue, supersededBy: f["supersededBy"]?.stringValue,
            updatedMs: f["updatedMs"]?.numberText.flatMap { UInt64($0) } ?? 0,
            experience: f["experience"].flatMap(LiveExperienceDetail.decode),
            confidence: f["confidence"]?.stringValue,
            stats: LiveMemoryStats.decode(f["stats"]))
    }

    /// The harness that proposed it, from its first evidence.
    public var proposer: String? { evidence.first?.harness.flatMap { $0 == "user" ? nil : $0 } }
}

/// B5-3a: what an experience says beyond its title.
public struct LiveExperienceDetail: Equatable, Sendable {
    public var problem: String
    public var solution: String
    public var searchKeys: [String]
    public var envs: [String]

    public init(problem: String = "", solution: String, searchKeys: [String] = [], envs: [String] = []) {
        self.problem = problem
        self.solution = solution
        self.searchKeys = searchKeys
        self.envs = envs
    }

    public static func decode(_ value: JSONValue) -> LiveExperienceDetail? {
        let f = value.fields
        guard let solution = f["solution"]?.stringValue else { return nil }
        return LiveExperienceDetail(problem: f["problem"]?.stringValue ?? "", solution: solution,
                                    searchKeys: (f["searchKeys"]?.arrayValue ?? []).compactMap(\.stringValue),
                                    envs: (f["envs"]?.arrayValue ?? []).compactMap(\.stringValue))
    }
}

/// B5-3a: how an item has been used.
public struct LiveMemoryStats: Equatable, Sendable {
    public var retrieved: Int
    public var injected: Int
    public var helpful: Int
    public var wrong: Int

    public init(retrieved: Int = 0, injected: Int = 0, helpful: Int = 0, wrong: Int = 0) {
        self.retrieved = retrieved
        self.injected = injected
        self.helpful = helpful
        self.wrong = wrong
    }

    public static func decode(_ value: JSONValue?) -> LiveMemoryStats {
        let f = value?.fields ?? [:]
        let int = { (key: String) in f[key]?.numberText.flatMap { Int($0) } ?? 0 }
        return LiveMemoryStats(retrieved: int("retrieved"), injected: int("injected"),
                               helpful: int("helpful"), wrong: int("wrong"))
    }
}

public struct LiveMemoryEvidence: Equatable, Sendable {
    public var taskId: String?
    public var harness: String?
    public var model: String?
    public var excerpt: String
    public var atMs: UInt64

    public init(taskId: String? = nil, harness: String? = nil, model: String? = nil, excerpt: String, atMs: UInt64 = 0) {
        self.taskId = taskId
        self.harness = harness
        self.model = model
        self.excerpt = excerpt
        self.atMs = atMs
    }

    public static func decode(_ value: JSONValue) -> LiveMemoryEvidence? {
        let f = value.fields
        guard let excerpt = f["excerpt"]?.stringValue else { return nil }
        return LiveMemoryEvidence(taskId: f["taskId"]?.stringValue, harness: f["harness"]?.stringValue,
                                  model: f["model"]?.stringValue, excerpt: excerpt,
                                  atMs: f["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0)
    }
}

/// A harness's own memory file, shown read-only.
public struct LiveNativeMemory: Equatable, Sendable, Identifiable {
    public var harness: String
    public var path: String
    public var title: String
    public var text: String
    public var truncated: Bool
    public var id: String { path }

    public init(harness: String, path: String, title: String, text: String, truncated: Bool = false) {
        self.harness = harness
        self.path = path
        self.title = title
        self.text = text
        self.truncated = truncated
    }

    public static func decode(_ value: JSONValue) -> LiveNativeMemory? {
        let f = value.fields
        guard let path = f["path"]?.stringValue, let text = f["text"]?.stringValue else { return nil }
        return LiveNativeMemory(harness: f["harness"]?.stringValue ?? "", path: path,
                                title: f["title"]?.stringValue ?? path, text: text,
                                truncated: f["truncated"]?.boolValue ?? false)
    }
}
