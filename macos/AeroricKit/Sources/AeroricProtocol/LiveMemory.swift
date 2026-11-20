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
    /// C1 (2026-11-19): the message the words were found in; nil before C1.
    public var location: LiveEvidenceLocation?

    public init(taskId: String? = nil, harness: String? = nil, model: String? = nil, excerpt: String, atMs: UInt64 = 0,
                location: LiveEvidenceLocation? = nil) {
        self.taskId = taskId
        self.harness = harness
        self.model = model
        self.excerpt = excerpt
        self.atMs = atMs
        self.location = location
    }

    public static func decode(_ value: JSONValue) -> LiveMemoryEvidence? {
        let f = value.fields
        guard let excerpt = f["excerpt"]?.stringValue else { return nil }
        return LiveMemoryEvidence(taskId: f["taskId"]?.stringValue, harness: f["harness"]?.stringValue,
                                  model: f["model"]?.stringValue, excerpt: excerpt,
                                  atMs: f["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0,
                                  location: f["location"].flatMap(LiveEvidenceLocation.decode))
    }
}

/// C1: where in its session an evidence's words were found.
public struct LiveEvidenceLocation: Equatable, Sendable {
    public var messageIndex: Int
    public var role: String

    public init(messageIndex: Int, role: String) {
        self.messageIndex = messageIndex
        self.role = role
    }

    public static func decode(_ value: JSONValue) -> LiveEvidenceLocation? {
        let f = value.fields
        guard let index = f["messageIndex"]?.numberText.flatMap({ Int($0) }) else { return nil }
        return LiveEvidenceLocation(messageIndex: index, role: f["role"]?.stringValue ?? "")
    }
}

/// C1: `memory.evidenceLocate` — what became of an evidence's source.
public struct LiveEvidenceLookup: Equatable, Sendable {
    /// `located` / `moved` / `changed` / `gone` / `unrecorded`.
    public var status: String
    public var taskId: String?
    public var excerpt: String
    public var message: LiveEvidenceMessage?

    public init(status: String, taskId: String? = nil, excerpt: String, message: LiveEvidenceMessage? = nil) {
        self.status = status
        self.taskId = taskId
        self.excerpt = excerpt
        self.message = message
    }

    public static func decode(_ value: JSONValue) -> LiveEvidenceLookup {
        let f = value.fields
        return LiveEvidenceLookup(status: f["status"]?.stringValue ?? "unrecorded",
                                  taskId: f["taskId"]?.stringValue,
                                  excerpt: f["excerpt"]?.stringValue ?? "",
                                  message: f["message"].flatMap(LiveEvidenceMessage.decode))
    }
}

/// C1: the message around the words (a window of a long one). `start` and
/// `end` count Unicode scalars, as the host counts chars.
public struct LiveEvidenceMessage: Equatable, Sendable {
    public var messageIndex: Int
    public var role: String
    public var text: String
    public var start: Int
    public var end: Int
    public var cutBefore: Bool
    public var cutAfter: Bool

    public init(messageIndex: Int, role: String, text: String, start: Int, end: Int,
                cutBefore: Bool = false, cutAfter: Bool = false) {
        self.messageIndex = messageIndex
        self.role = role
        self.text = text
        self.start = start
        self.end = end
        self.cutBefore = cutBefore
        self.cutAfter = cutAfter
    }

    public static func decode(_ value: JSONValue) -> LiveEvidenceMessage? {
        let f = value.fields
        guard let text = f["text"]?.stringValue else { return nil }
        let int = { (key: String) in f[key]?.numberText.flatMap { Int($0) } ?? 0 }
        return LiveEvidenceMessage(messageIndex: int("messageIndex"), role: f["role"]?.stringValue ?? "",
                                   text: text, start: int("start"), end: int("end"),
                                   cutBefore: f["cutBefore"]?.boolValue ?? false,
                                   cutAfter: f["cutAfter"]?.boolValue ?? false)
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

/// C2 (2026-11-20): what forgetting or archiving one memory did beyond
/// the item itself (`memory.forget` / `memory.archive`'s `withdrawal`).
public struct LiveMemoryWithdrawal: Equatable, Sendable {
    /// Items deleted or archived, with the versions they replaced.
    public var items: Int
    /// Adopted or proposed skills now marked 依据已撤回 (their names).
    public var skills: [String]
    /// Dream proposals that waited on it, withdrawn.
    public var dreams: Int
    /// Wiki pages that lost a paragraph compiled from it.
    public var wikiPages: Int
    /// Wiki paragraphs the user had changed: kept, with an issue to check.
    public var wikiFlagged: Int
    /// What could not be updated, and why; the host tries again.
    public var failed: [String]

    public init(items: Int = 0, skills: [String] = [], dreams: Int = 0, wikiPages: Int = 0,
                wikiFlagged: Int = 0, failed: [String] = []) {
        self.items = items
        self.skills = skills
        self.dreams = dreams
        self.wikiPages = wikiPages
        self.wikiFlagged = wikiFlagged
        self.failed = failed
    }

    public static func decode(_ value: JSONValue?) -> LiveMemoryWithdrawal? {
        guard let f = value?.objectValue else { return nil }
        func count(_ key: String) -> Int { f[key]?.numberText.flatMap { Int($0) } ?? 0 }
        return LiveMemoryWithdrawal(items: count("items"),
                                    skills: (f["skills"]?.arrayValue ?? []).compactMap(\.stringValue),
                                    dreams: count("dreams"), wikiPages: count("wikiPages"),
                                    wikiFlagged: count("wikiFlagged"),
                                    failed: (f["failed"]?.arrayValue ?? []).compactMap(\.stringValue))
    }
}

/// C2: one project's Wiki still holding paragraphs of withdrawn memories
/// (`memory.attention`'s `wikiBacklog`). Ids only, never the text.
public struct LiveWikiBacklog: Equatable, Sendable, Identifiable {
    public var projectId: String
    public var ids: [String]
    public var forgotten: Bool
    public var reason: String
    public var atMs: UInt64
    public var id: String { "wiki#\(projectId)" }

    public init(projectId: String, ids: [String] = [], forgotten: Bool = false, reason: String = "", atMs: UInt64 = 0) {
        self.projectId = projectId
        self.ids = ids
        self.forgotten = forgotten
        self.reason = reason
        self.atMs = atMs
    }

    public static func decode(_ value: JSONValue) -> LiveWikiBacklog? {
        let f = value.fields
        guard let project = f["projectId"]?.stringValue else { return nil }
        return LiveWikiBacklog(projectId: project, ids: (f["ids"]?.arrayValue ?? []).compactMap(\.stringValue),
                               forgotten: f["forgotten"]?.boolValue ?? false,
                               reason: f["reason"]?.stringValue ?? "",
                               atMs: f["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0)
    }
}
