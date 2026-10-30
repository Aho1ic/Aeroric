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
        let findings = (f["findings"]?.arrayValue ?? []).compactMap { item -> LiveReviewFinding? in
            let g = item.fields
            guard let id = g["id"]?.stringValue, let text = g["text"]?.stringValue else { return nil }
            return LiveReviewFinding(id: id, severity: g["severity"]?.stringValue ?? "minor", text: text,
                                     file: g["file"]?.stringValue, line: g["line"]?.numberText.flatMap { UInt64($0) },
                                     decision: g["decision"]?.stringValue, reason: g["reason"]?.stringValue)
        }
        return LiveReviewCard(reviewId: reviewId, round: f["round"]?.numberText.flatMap { Int($0) } ?? 1,
                              state: state, turnId: f["turnId"]?.stringValue ?? "",
                              reviewerTaskId: f["reviewerTaskId"]?.stringValue,
                              reviewerLabel: f["reviewerLabel"]?.stringValue ?? "",
                              sameFamily: f["sameFamily"]?.boolValue ?? false, findings: findings,
                              summary: f["summary"]?.stringValue, raw: f["raw"]?.stringValue)
    }
}
