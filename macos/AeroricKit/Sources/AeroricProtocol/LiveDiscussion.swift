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
    /// 2026-11-13: members call each other 成员 A / B; the page still shows
    /// who is who.
    public var anonymous: Bool
    /// roundtable / review (draft review: the moderator writes, the others
    /// review, until it is final) / research (the moderator splits, each
    /// member researches its part, the moderator reports).
    public var format: String
    /// Draft review: how many times the author may revise.
    public var maxRevisions: Int
    /// What the person asked of every post; nil is the default wording.
    public var requirements: String?
    /// Research: each member's part by index (the moderator's is empty).
    public var assignments: [String]

    public struct Member: Equatable, Sendable, Identifiable {
        public var id: String { taskId }
        public var taskId: String
        public var label: String
        /// The Agent configuration it runs with, to start a new discussion
        /// with the same members.
        public var configurationId: String
        public var agent: String
        public var model: String?
        public var angle: String?
        /// The family that runs it, for its logo.
        public var family: String?

        public init(taskId: String, label: String, configurationId: String = "", agent: String = "",
                    model: String? = nil, angle: String? = nil, family: String? = nil) {
            self.taskId = taskId
            self.label = label
            self.configurationId = configurationId
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
        /// answer / summary / user (the person spoke) / reply (a member
        /// answering the person who asked it by name)
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
        /// A user post: the members it asked by name.
        public var mentions: [Int]
        /// Draft review: draft / review / revise / final, the version, and
        /// pass / revise (a review's call; on the final, whether all passed).
        public var stage: String?
        public var version: Int?
        public var verdict: String?

        public init(index: Int, round: Int, member: Int, kind: String = "answer", state: String = "done",
                    text: String = "", stance: String? = nil, error: String? = nil, waiting: String? = nil,
                    usage: Usage? = nil, mentions: [Int] = [], stage: String? = nil, version: Int? = nil,
                    verdict: String? = nil) {
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
            self.mentions = mentions
            self.stage = stage
            self.version = version
            self.verdict = verdict
        }
    }

    public init(id: String, projectId: String = "", topic: String = "", members: [Member] = [], moderator: Int = 0,
                maxRounds: Int = 3, round: Int = 1, state: String = "running", posts: [Post] = [],
                note: String? = nil, anonymous: Bool = false, format: String = "roundtable",
                maxRevisions: Int = 2, requirements: String? = nil, assignments: [String] = []) {
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
        self.anonymous = anonymous
        self.format = format
        self.maxRevisions = maxRevisions
        self.requirements = requirements
        self.assignments = assignments
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
                          configurationId: m["configurationId"]?.stringValue ?? "",
                          agent: m["agent"]?.stringValue ?? "", model: m["model"]?.stringValue,
                          angle: m["angle"]?.stringValue,
                          family: i < families.count ? families[i].stringValue : nil)
        }
        let posts = (d["posts"]?.arrayValue ?? []).enumerated().map { i, value in
            let p = value.fields
            var post = Post(index: i, round: int(p["round"]), member: int(p["member"]),
                            kind: p["kind"]?.stringValue ?? "answer", state: p["state"]?.stringValue ?? "done",
                            text: p["text"]?.stringValue ?? "", stance: p["stance"]?.stringValue,
                            error: p["error"]?.stringValue, waiting: p["waiting"]?.stringValue,
                            mentions: (p["mentions"]?.arrayValue ?? []).compactMap { $0.numberText.flatMap { Int($0) } },
                            stage: p["stage"]?.stringValue,
                            version: p["version"]?.numberText.flatMap { Int($0) },
                            verdict: p["verdict"]?.stringValue)
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
                              posts: posts, note: d["note"]?.stringValue,
                              anonymous: d["anonymous"]?.boolValue ?? false,
                              format: d["format"]?.stringValue ?? "roundtable",
                              maxRevisions: d["maxRevisions"]?.numberText.flatMap { Int($0) } ?? 2,
                              requirements: d["requirements"]?.stringValue,
                              assignments: (d["assignments"]?.arrayValue ?? []).map { $0.stringValue ?? "" })
    }
}

/// 2026-11-13: what `discussion.start` takes besides the project.
public struct LiveDiscussionStart: Equatable, Sendable {
    public var topic: String
    public var members: [LiveDiscussionChoice]
    public var moderator: Int
    public var rounds: Int
    public var anonymous: Bool
    /// roundtable / review / research
    public var format: String
    public var revisions: Int
    /// What every post should be like; empty asks nothing.
    public var requirements: String

    public init(topic: String, members: [LiveDiscussionChoice], moderator: Int = 0, rounds: Int = 3,
                anonymous: Bool = false, format: String = "roundtable", revisions: Int = 2,
                requirements: String = "") {
        self.topic = topic
        self.members = members
        self.moderator = moderator
        self.rounds = rounds
        self.anonymous = anonymous
        self.format = format
        self.revisions = revisions
        self.requirements = requirements
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
