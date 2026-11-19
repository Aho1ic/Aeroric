import Foundation

/// B4-1 (2026-10-30): one review loop as the host reports it — the latest
/// `review` update of a `reviewId` replaces the card.
public struct LiveReviewCard: Equatable, Sendable {
    public var reviewId: String
    public var round: Int
    /// `reviewing` `waitingSend` `waitingCoder` `passed` `settled` `escalated` `interrupted` `failed`.
    public var state: String
    public var turnId: String
    public var reviewerTaskId: String?
    public var reviewerLabel: String
    public var sameFamily: Bool
    public var findings: [LiveReviewFinding]
    public var summary: String?
    public var raw: String?
    /// E1: which changed files the reviewer accounted for.
    public var coverage: LiveReviewCoverage?
    /// E3: this round against the one before, from round 2 on.
    public var compare: LiveRoundCompare?

    public init(reviewId: String, round: Int = 1, state: String, turnId: String = "", reviewerTaskId: String? = nil,
                reviewerLabel: String = "", sameFamily: Bool = false, findings: [LiveReviewFinding] = [],
                summary: String? = nil, raw: String? = nil) {
        self.reviewId = reviewId
        self.round = round
        self.state = state
        self.turnId = turnId
        self.reviewerTaskId = reviewerTaskId
        self.reviewerLabel = reviewerLabel
        self.sameFamily = sameFamily
        self.findings = findings
        self.summary = summary
        self.raw = raw
    }

    /// Still moving: the reviewer or the writer is working, or findings wait to be sent.
    public var isOpen: Bool { ["reviewing", "waitingSend", "waitingCoder"].contains(state) }

    /// Settled with a blocker the writer rejected.
    public var rejectedBlocker: Bool {
        findings.contains { $0.severity == "blocker" && $0.decision == "reject" }
    }

    /// The user should look: escalated, or settled over a rejected blocker.
    public var needsUser: Bool { state == "escalated" || (state == "settled" && rejectedBlocker) }
}

public struct LiveReviewFinding: Equatable, Sendable {
    public var id: String
    /// `blocker` `major` `minor`.
    public var severity: String
    public var text: String
    public var file: String?
    public var line: UInt64?
    /// The writer's answer: `adopt` `reject` `ask`.
    public var decision: String?
    public var reason: String?
    /// E2: the code the finding is about, as the reviewer quoted it.
    public var snippet: String?
    public var category: String?
    /// E2: the line the reviewer wrote, when its code sits elsewhere.
    public var movedFrom: UInt64?
    /// E2: the quoted code is nowhere in the file.
    public var unlocated: Bool = false
    /// E2: findings saying the same that were folded into this one.
    public var merged: Int = 0
    /// E3: the previous round's finding this one says again.
    public var repeats: String?
    /// E4: the review rule it says was broken.
    public var rule: String?

    public init(id: String, severity: String, text: String, file: String? = nil, line: UInt64? = nil,
                decision: String? = nil, reason: String? = nil) {
        self.id = id
        self.severity = severity
        self.text = text
        self.file = file
        self.line = line
        self.decision = decision
        self.reason = reason
    }

    /// "src/a.rs:12", "src/a.rs", or nil.
    public var location: String? {
        guard let file else { return nil }
        return line.map { "\(file):\($0)" } ?? file
    }
}

/// Who reviews a task.
public enum LiveReviewerChoice: Equatable, Sendable {
    case auto
    case named(agent: String, configID: String? = nil, model: String? = nil)

    public var json: JSONValue {
        switch self {
        case .auto: return .string("auto")
        case let .named(agent, configID, model):
            var fields: [String: JSONValue] = ["agent": .string(agent)]
            if let configID { fields["agentConfigId"] = .string(configID) }
            if let model { fields["model"] = .string(model) }
            return .object(fields)
        }
    }

    public static func decode(_ value: JSONValue?) -> LiveReviewerChoice {
        guard let agent = value?.fields["agent"]?.stringValue else { return .auto }
        return .named(agent: agent, configID: value?.fields["agentConfigId"]?.stringValue,
                      model: value?.fields["model"]?.stringValue)
    }
}

/// A task's review loop settings (`review.get` / `review.set`).
public struct LiveReviewSettings: Equatable, Sendable {
    public var enabled: Bool
    public var reviewer: LiveReviewerChoice
    public var paused: Bool

    public init(enabled: Bool = false, reviewer: LiveReviewerChoice = .auto, paused: Bool = false) {
        self.enabled = enabled
        self.reviewer = reviewer
        self.paused = paused
    }

    public static func decode(_ value: JSONValue?) -> LiveReviewSettings? {
        guard let f = value?.objectValue else { return nil }
        return LiveReviewSettings(enabled: f["enabled"]?.boolValue ?? false,
                                  reviewer: LiveReviewerChoice.decode(f["reviewer"]),
                                  paused: f["paused"]?.boolValue ?? false)
    }
}

/// E4 (2026-11-19): the review rules in force for a task's project
/// (`review.rules`); mirrors `aeroric_protocol::review::ReviewRules`.
public struct LiveReviewRules: Equatable, Sendable {
    public struct Rule: Equatable, Sendable, Identifiable {
        public var id: String
        /// `builtin` / `project`.
        public var source: String
        public var paths: [String]
        /// `blocker` `major` `minor`.
        public var severity: String
        public var text: String
        public var disabled: Bool

        public init(id: String, source: String, paths: [String], severity: String, text: String,
                    disabled: Bool = false) {
            self.id = id
            self.source = source
            self.paths = paths
            self.severity = severity
            self.text = text
            self.disabled = disabled
        }
    }

    /// The project rules file, from the repository's top.
    public var file: String
    /// `missing` / `loaded` / `invalid` / `unavailable`.
    public var state: String
    public var error: String?
    public var rules: [Rule]

    public init(file: String, state: String, error: String? = nil, rules: [Rule]) {
        self.file = file
        self.state = state
        self.error = error
        self.rules = rules
    }

    public static func decode(_ value: JSONValue?) -> LiveReviewRules? {
        guard let f = value?.objectValue, let state = f["state"]?.stringValue else { return nil }
        let rules: [Rule] = (f["rules"]?.arrayValue ?? []).compactMap { item in
            let g = item.fields
            guard let id = g["id"]?.stringValue, let text = g["text"]?.stringValue else { return nil }
            return Rule(id: id, source: g["source"]?.stringValue ?? "builtin",
                        paths: (g["paths"]?.arrayValue ?? []).compactMap(\.stringValue),
                        severity: g["severity"]?.stringValue ?? "major", text: text,
                        disabled: g["disabled"]?.boolValue ?? false)
        }
        return LiveReviewRules(file: f["file"]?.stringValue ?? ".aeroric/review-rules.json", state: state,
                               error: f["error"]?.stringValue, rules: rules)
    }
}

/// A harness that could review a task (`review.reviewers`).
public struct LiveReviewerCandidate: Equatable, Sendable {
    public var agent: String
    public var label: String
    public var sameFamily: Bool

    public init(agent: String, label: String, sameFamily: Bool) {
        self.agent = agent
        self.label = label
        self.sameFamily = sameFamily
    }

    public static func decode(_ value: JSONValue?) -> LiveReviewerCandidate? {
        guard let f = value?.objectValue, let agent = f["agent"]?.stringValue,
              let label = f["label"]?.stringValue else { return nil }
        return LiveReviewerCandidate(agent: agent, label: label, sameFamily: f["sameFamily"]?.boolValue ?? false)
    }
}

extension LiveReviewCard {
    public static func decode(_ value: JSONValue) -> LiveReviewCard? {
        let f = value.fields
        guard let reviewId = f["reviewId"]?.stringValue, let state = f["state"]?.stringValue else { return nil }
        let findings = (f["findings"]?.arrayValue ?? []).compactMap(LiveReviewFinding.decode)
        var card = LiveReviewCard(reviewId: reviewId, round: f["round"]?.numberText.flatMap { Int($0) } ?? 1,
                              state: state, turnId: f["turnId"]?.stringValue ?? "",
                              reviewerTaskId: f["reviewerTaskId"]?.stringValue,
                              reviewerLabel: f["reviewerLabel"]?.stringValue ?? "",
                              sameFamily: f["sameFamily"]?.boolValue ?? false, findings: findings,
                              summary: f["summary"]?.stringValue, raw: f["raw"]?.stringValue)
        card.coverage = f["coverage"].flatMap(LiveReviewCoverage.decode)
        card.compare = f["compare"].flatMap(LiveRoundCompare.decode)
        return card
    }
}

extension LiveReviewFinding {
    public static func decode(_ item: JSONValue) -> LiveReviewFinding? {
        let g = item.fields
        guard let id = g["id"]?.stringValue, let text = g["text"]?.stringValue else { return nil }
        var finding = LiveReviewFinding(
            id: id, severity: g["severity"]?.stringValue ?? "minor", text: text,
            file: g["file"]?.stringValue, line: g["line"]?.numberText.flatMap { UInt64($0) },
            decision: g["decision"]?.stringValue, reason: g["reason"]?.stringValue)
        finding.snippet = g["snippet"]?.stringValue
        finding.category = g["category"]?.stringValue
        finding.movedFrom = g["movedFrom"]?.numberText.flatMap { UInt64($0) }
        finding.unlocated = g["unlocated"]?.boolValue ?? false
        finding.merged = g["merged"]?.numberText.flatMap { Int($0) } ?? 0
        finding.repeats = g["repeats"]?.stringValue
        finding.rule = g["rule"]?.stringValue
        return finding
    }
}

/// E3 (2026-11-19): mirrors `aeroric_protocol::review::RoundCompare` — what
/// became of the previous round's adopted findings not said again.
public struct LiveRoundCompare: Equatable, Sendable {
    public var previousRound: Int
    /// Its file was reviewed again and it was not raised again.
    public var resolved: [LiveReviewFinding]
    /// Its file was not in front of the reviewer this round.
    public var unchecked: [LiveReviewFinding]

    public init(previousRound: Int, resolved: [LiveReviewFinding] = [], unchecked: [LiveReviewFinding] = []) {
        self.previousRound = previousRound
        self.resolved = resolved
        self.unchecked = unchecked
    }

    public static func decode(_ value: JSONValue) -> LiveRoundCompare? {
        let f = value.fields
        guard let round = f["previousRound"]?.numberText.flatMap({ Int($0) }) else { return nil }
        return LiveRoundCompare(
            previousRound: round,
            resolved: (f["resolved"]?.arrayValue ?? []).compactMap(LiveReviewFinding.decode),
            unchecked: (f["unchecked"]?.arrayValue ?? []).compactMap(LiveReviewFinding.decode))
    }
}

/// E1 (2026-11-19): mirrors `aeroric_protocol::review::ReviewCoverage`.
public struct LiveReviewCoverage: Equatable, Sendable {
    public struct File: Equatable, Sendable, Identifiable {
        public var id: String { path }
        public var path: String
        /// `reviewed` / `skipped` / `seen` / `missed` / `excluded`.
        public var state: String
        public var reason: String?

        public init(path: String, state: String, reason: String? = nil) {
            self.path = path
            self.state = state
            self.reason = reason
        }
    }

    /// `complete` / `partial` / `unreviewed` / `empty`.
    public var state: String
    public var files: [File]

    public init(state: String, files: [File]) {
        self.state = state
        self.files = files
    }

    /// The files in scope (all but the excluded).
    public var inScope: [File] { files.filter { $0.state != "excluded" } }
    /// Reviewed or at least in front of the reviewer.
    public var looked: Int { inScope.filter { $0.state == "reviewed" || $0.state == "seen" }.count }

    public static func decode(_ value: JSONValue) -> LiveReviewCoverage? {
        let f = value.fields
        guard let state = f["state"]?.stringValue else { return nil }
        let files: [File] = (f["files"]?.arrayValue ?? []).compactMap { item in
            let g = item.fields
            guard let path = g["path"]?.stringValue, let state = g["state"]?.stringValue else { return nil }
            return File(path: path, state: state, reason: g["reason"]?.stringValue)
        }
        return LiveReviewCoverage(state: state, files: files)
    }
}
