import Foundation

/// 2026-11-12: a round-table discussion, read back from `discussion.get`.
/// Mirrors the host's `session_live_services/discussion.rs`.
public struct LiveDiscussion: Equatable, Sendable, Identifiable {
    public var id: String
    public var projectId: String
    public var topic: String
    public var members: [Member]
    public var moderator: Int
    public var maxRounds: Int
    public var round: Int
    /// running / pausing / paused / summarizing / done / stopped
    public var state: String
    public var posts: [Post]
    /// Why it paused.
    public var note: String?

    public struct Member: Equatable, Sendable, Identifiable {
        public var id: String { taskId }
        public var taskId: String
        public var label: String
        public var agent: String
        public var model: String?
        public var angle: String?
        /// The family that runs it, for its logo.
        public var family: String?

        public init(taskId: String, label: String, agent: String = "", model: String? = nil,
                    angle: String? = nil, family: String? = nil) {
            self.taskId = taskId
            self.label = label
            self.agent = agent
            self.model = model
            self.angle = angle
            self.family = family
        }
    }

    public struct Usage: Equatable, Sendable {
        public var tokens: UInt64
        public var costUsd: Double?
        public var durationMs: UInt64?

        public init(tokens: UInt64, costUsd: Double? = nil, durationMs: UInt64? = nil) {
            self.tokens = tokens
            self.costUsd = costUsd
            self.durationMs = durationMs
        }
    }

    public struct Post: Equatable, Sendable, Identifiable {
        public var id: Int { index }
        public var index: Int
        public var round: Int
        public var member: Int
        /// answer / summary
        public var kind: String
        /// speaking / waiting / done / failed
        public var state: String
        public var text: String
        /// keep / changed, from round 2 on.
        public var stance: String?
        public var error: String?
        /// What the member waits on.
        public var waiting: String?
        public var usage: Usage?

        public init(index: Int, round: Int, member: Int, kind: String = "answer", state: String = "done",
                    text: String = "", stance: String? = nil, error: String? = nil, waiting: String? = nil,
                    usage: Usage? = nil) {
            self.index = index
            self.round = round
            self.member = member
            self.kind = kind
            self.state = state
            self.text = text
            self.stance = stance
            self.error = error
            self.waiting = waiting
            self.usage = usage
        }
    }

    public init(id: String, projectId: String = "", topic: String = "", members: [Member] = [], moderator: Int = 0,
                maxRounds: Int = 3, round: Int = 1, state: String = "running", posts: [Post] = [],
                note: String? = nil) {
        self.id = id
        self.projectId = projectId
        self.topic = topic
        self.members = members
        self.moderator = moderator
        self.maxRounds = maxRounds
        self.round = round
        self.state = state
        self.posts = posts
        self.note = note
    }

    /// `{discussion, usage, families}` as the host sends it.
    public static func decode(_ value: JSONValue?) -> LiveDiscussion? {
        let f = value?.fields ?? [:]
        let d = f["discussion"]?.fields ?? [:]
        guard let id = d["id"]?.stringValue else { return nil }
        func int(_ value: JSONValue?) -> Int { value?.numberText.flatMap { Int($0) } ?? 0 }
        let families = f["families"]?.arrayValue ?? []
        let usage = f["usage"]?.arrayValue ?? []
        let members = (d["members"]?.arrayValue ?? []).enumerated().map { i, value in
            let m = value.fields
            return Member(taskId: m["taskId"]?.stringValue ?? "", label: m["label"]?.stringValue ?? "",
                          agent: m["agent"]?.stringValue ?? "", model: m["model"]?.stringValue,
                          angle: m["angle"]?.stringValue,
                          family: i < families.count ? families[i].stringValue : nil)
        }
        let posts = (d["posts"]?.arrayValue ?? []).enumerated().map { i, value in
            let p = value.fields
            var post = Post(index: i, round: int(p["round"]), member: int(p["member"]),
                            kind: p["kind"]?.stringValue ?? "answer", state: p["state"]?.stringValue ?? "done",
                            text: p["text"]?.stringValue ?? "", stance: p["stance"]?.stringValue,
                            error: p["error"]?.stringValue, waiting: p["waiting"]?.stringValue)
            if i < usage.count, let u = usage[i].objectValue,
               let tokens = u["tokens"]?.numberText.flatMap({ UInt64($0) }) {
                post.usage = Usage(tokens: tokens, costUsd: u["costUsd"]?.numberText.flatMap { Double($0) },
                                   durationMs: u["durationMs"]?.numberText.flatMap { UInt64($0) })
            }
            return post
        }
        return LiveDiscussion(id: id, projectId: d["projectId"]?.stringValue ?? "",
                              topic: d["topic"]?.stringValue ?? "", members: members,
                              moderator: int(d["moderator"]), maxRounds: int(d["maxRounds"]),
                              round: int(d["round"]), state: d["state"]?.stringValue ?? "running",
                              posts: posts, note: d["note"]?.stringValue)
    }
}

/// One member of a discussion about to start.
public struct LiveDiscussionChoice: Equatable, Sendable, Identifiable {
    public var id = UUID()
    public var configurationId: String
    public var model: String?
    public var angle: String

    public init(configurationId: String, model: String? = nil, angle: String = "") {
        self.configurationId = configurationId
        self.model = model
        self.angle = angle
    }
}
