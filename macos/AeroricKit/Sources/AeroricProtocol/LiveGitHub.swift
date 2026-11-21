import Foundation

/// D1 (2026-11-21): a GitHub pull request pinned as a snapshot, and its
/// read-only reviews, as `github.*` returns them.

public struct LivePullFile: Equatable, Sendable {
    public var path: String
    public var status: String
    public var additions: Int
    public var deletions: Int

    public init(path: String, status: String = "modified", additions: Int = 0, deletions: Int = 0) {
        self.path = path; self.status = status; self.additions = additions; self.deletions = deletions
    }
}

public struct LivePullCheck: Equatable, Sendable {
    public var name: String
    public var status: String
    public var conclusion: String?

    public init(name: String, status: String = "completed", conclusion: String? = nil) {
        self.name = name; self.status = status; self.conclusion = conclusion
    }
}

public struct LivePullComment: Equatable, Sendable {
    public var author: String
    public var body: String
    /// `path:line` for a review comment, nil for one on the whole PR.
    public var place: String?
    public var outdated: Bool

    public init(author: String, body: String, place: String? = nil, outdated: Bool = false) {
        self.author = author; self.body = body; self.place = place; self.outdated = outdated
    }
}

private func int(_ value: JSONValue?) -> Int { value?.numberText.flatMap { Int($0) } ?? 0 }

public struct LivePullSnapshot: Equatable, Sendable, Identifiable {
    public var id: String
    public var owner: String
    public var repo: String
    public var number: Int
    public var title: String
    public var author: String
    public var state: String
    public var merged: Bool
    public var url: String
    public var headRef: String
    public var baseRef: String
    public var headSha: String
    public var fetchedMs: Int
    public var files: [LivePullFile]
    public var checks: [LivePullCheck]
    public var comments: [LivePullComment]
    public var truncated: Bool
    /// The PR has a newer snapshot (PR 已更新).
    public var superseded: Bool

    public init(id: String, owner: String = "", repo: String = "", number: Int, title: String,
                author: String = "", state: String = "open", merged: Bool = false, url: String = "",
                headRef: String = "", baseRef: String = "", headSha: String, fetchedMs: Int = 0,
                files: [LivePullFile] = [], checks: [LivePullCheck] = [], comments: [LivePullComment] = [],
                truncated: Bool = false, superseded: Bool = false) {
        self.id = id; self.owner = owner; self.repo = repo; self.number = number; self.title = title
        self.author = author; self.state = state; self.merged = merged; self.url = url
        self.headRef = headRef; self.baseRef = baseRef; self.headSha = headSha; self.fetchedMs = fetchedMs
        self.files = files; self.checks = checks; self.comments = comments
        self.truncated = truncated; self.superseded = superseded
    }

    public static func decode(_ value: JSONValue) -> LivePullSnapshot? {
        let f = value.fields
        guard let id = f["id"]?.stringValue, let head = f["headSha"]?.stringValue else { return nil }
        let repo = f["repo"]?.fields ?? [:]
        return LivePullSnapshot(
            id: id, owner: repo["owner"]?.stringValue ?? "", repo: repo["name"]?.stringValue ?? "",
            number: int(f["number"]), title: f["title"]?.stringValue ?? "", author: f["author"]?.stringValue ?? "",
            state: f["state"]?.stringValue ?? "", merged: f["merged"]?.boolValue ?? false,
            url: f["url"]?.stringValue ?? "", headRef: f["headRef"]?.stringValue ?? "",
            baseRef: f["baseRef"]?.stringValue ?? "", headSha: head, fetchedMs: int(f["fetchedMs"]),
            files: (f["files"]?.arrayValue ?? []).compactMap { v in
                v["path"]?.stringValue.map {
                    LivePullFile(path: $0, status: v["status"]?.stringValue ?? "", additions: int(v["additions"]),
                                 deletions: int(v["deletions"]))
                }
            },
            checks: (f["checks"]?.arrayValue ?? []).compactMap { v in
                v["name"]?.stringValue.map {
                    LivePullCheck(name: $0, status: v["status"]?.stringValue ?? "",
                                  conclusion: v["conclusion"]?.stringValue)
                }
            },
            comments: (f["comments"]?.arrayValue ?? []).map { v in
                let anchor = v["anchor"]?.objectValue
                let place = anchor.map { a in
                    let line = a["line"]?.numberText.map { ":\($0)" } ?? ""
                    return (a["path"]?.stringValue ?? "") + line
                }
                return LivePullComment(author: v["author"]?.stringValue ?? "", body: v["body"]?.stringValue ?? "",
                                       place: place, outdated: v["outdated"]?.boolValue ?? false)
            },
            truncated: f["truncated"]?.boolValue ?? false, superseded: f["superseded"]?.boolValue ?? false)
    }
}

public struct LivePullFinding: Equatable, Sendable {
    public var path: String
    public var line: Int?
    /// `high`, `medium`, `low`.
    public var severity: String
    public var body: String
    public var basis: String

    public init(path: String, line: Int? = nil, severity: String = "medium", body: String, basis: String = "") {
        self.path = path; self.line = line; self.severity = severity; self.body = body; self.basis = basis
    }
}

public struct LivePullReview: Equatable, Sendable, Identifiable {
    public var id: String
    public var snapshotId: String
    public var headSha: String
    public var taskId: String
    /// `running`, `done`, `failed`.
    public var state: String
    public var findings: [LivePullFinding]
    public var dropped: Int
    public var reason: String?
    public var createdMs: Int
    /// Its snapshot was superseded (针对旧版本).
    public var outdated: Bool

    public init(id: String, snapshotId: String, headSha: String = "", taskId: String = "", state: String,
                findings: [LivePullFinding] = [], dropped: Int = 0, reason: String? = nil,
                createdMs: Int = 0, outdated: Bool = false) {
        self.id = id; self.snapshotId = snapshotId; self.headSha = headSha; self.taskId = taskId
        self.state = state; self.findings = findings; self.dropped = dropped; self.reason = reason
        self.createdMs = createdMs; self.outdated = outdated
    }

    public static func decode(_ value: JSONValue) -> LivePullReview? {
        let f = value.fields
        guard let id = f["id"]?.stringValue, let snapshot = f["snapshotId"]?.stringValue else { return nil }
        return LivePullReview(
            id: id, snapshotId: snapshot, headSha: f["headSha"]?.stringValue ?? "",
            taskId: f["taskId"]?.stringValue ?? "", state: f["state"]?.stringValue ?? "running",
            findings: (f["findings"]?.arrayValue ?? []).compactMap { v in
                guard let path = v["path"]?.stringValue else { return nil }
                return LivePullFinding(path: path, line: v["line"]?.numberText.flatMap { Int($0) },
                                       severity: v["severity"]?.stringValue ?? "medium",
                                       body: v["body"]?.stringValue ?? "", basis: v["basis"]?.stringValue ?? "")
            },
            dropped: int(f["dropped"]), reason: f["reason"]?.stringValue, createdMs: int(f["createdMs"]),
            outdated: f["outdated"]?.boolValue ?? false)
    }
}

/// D2 (2026-11-21): one comment of a publishing draft.
public struct LiveWriteItem: Equatable, Sendable, Identifiable {
    public var id: String
    /// nil: the comment on the whole PR (总评).
    public var path: String?
    public var line: Int?
    public var body: String
    public var included: Bool
    /// `draft`, `sending`, `sent`, `failed`, `unknown`.
    public var state: String
    public var url: String?
    public var reason: String?

    public init(id: String, path: String? = nil, line: Int? = nil, body: String, included: Bool = true,
                state: String = "draft", url: String? = nil, reason: String? = nil) {
        self.id = id; self.path = path; self.line = line; self.body = body; self.included = included
        self.state = state; self.url = url; self.reason = reason
    }

    /// Still the user's to change (not out, not sent).
    public var editable: Bool { state == "draft" || state == "failed" }
}

/// D2: a review's publishing draft and what came of it.
public struct LiveWriteIntent: Equatable, Sendable, Identifiable {
    public var id: String
    public var reviewId: String
    public var owner: String
    public var repo: String
    public var number: Int
    public var headSha: String
    public var items: [LiveWriteItem]
    /// `draft`, `sending`, `done`, `partial`, `failed`, `unknown`, `targetMoved`.
    public var state: String
    public var reason: String?

    public init(id: String, reviewId: String, owner: String = "", repo: String = "", number: Int = 0,
                headSha: String = "", items: [LiveWriteItem] = [], state: String = "draft", reason: String? = nil) {
        self.id = id; self.reviewId = reviewId; self.owner = owner; self.repo = repo; self.number = number
        self.headSha = headSha; self.items = items; self.state = state; self.reason = reason
    }

    public static func decode(_ value: JSONValue) -> LiveWriteIntent? {
        let f = value.fields
        guard let id = f["id"]?.stringValue, let review = f["reviewId"]?.stringValue else { return nil }
        let repo = f["repo"]?.fields ?? [:]
        return LiveWriteIntent(
            id: id, reviewId: review, owner: repo["owner"]?.stringValue ?? "", repo: repo["name"]?.stringValue ?? "",
            number: int(f["number"]), headSha: f["headSha"]?.stringValue ?? "",
            items: (f["items"]?.arrayValue ?? []).compactMap { v in
                guard let id = v["id"]?.stringValue else { return nil }
                return LiveWriteItem(id: id, path: v["path"]?.stringValue,
                                     line: v["line"]?.numberText.flatMap { Int($0) },
                                     body: v["body"]?.stringValue ?? "", included: v["included"]?.boolValue ?? false,
                                     state: v["state"]?.stringValue ?? "draft", url: v["url"]?.stringValue,
                                     reason: v["reason"]?.stringValue)
            },
            state: f["state"]?.stringValue ?? "draft", reason: f["reason"]?.stringValue)
    }
}
