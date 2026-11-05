import Foundation

/// R2-2 (2026-11-04): a session's changes by file and hunk, with the user's
/// verdicts. Mirrors `aeroric_protocol::change_review`.
public struct LiveReviewDiff: Equatable, Sendable {
    public struct Line: Equatable, Sendable, Hashable {
        /// `" "`, `"+"` or `"-"`.
        public var kind: String
        public var text: String
        public var old: Int?
        public var new: Int?
        public init(kind: String, text: String, old: Int? = nil, new: Int? = nil) {
            self.kind = kind; self.text = text; self.old = old; self.new = new
        }
    }
    public struct Hunk: Equatable, Sendable, Identifiable {
        public var id: String
        public var header: String
        public var lines: [Line]
        /// `pending` / `accepted` / `rejected` / `stale`.
        public var state: String
        public var comment: String?
        public var commentSent: Bool
        public init(id: String, header: String, lines: [Line], state: String = "pending", comment: String? = nil,
                    commentSent: Bool = false) {
            self.id = id; self.header = header; self.lines = lines; self.state = state
            self.comment = comment; self.commentSent = commentSent
        }
    }
    public struct File: Equatable, Sendable, Identifiable {
        public var id: String { path }
        public var path: String
        public var binary: Bool
        public var additions: Int
        public var deletions: Int
        public var hunks: [Hunk]
        public init(path: String, binary: Bool = false, additions: Int = 0, deletions: Int = 0, hunks: [Hunk] = []) {
            self.path = path; self.binary = binary; self.additions = additions; self.deletions = deletions
            self.hunks = hunks
        }
    }

    public var files: [File]
    public var unsent: Int
    public var seenMs: UInt64?
    public var emptyReason: String?

    public init(files: [File] = [], unsent: Int = 0, seenMs: UInt64? = nil, emptyReason: String? = nil) {
        self.files = files; self.unsent = unsent; self.seenMs = seenMs; self.emptyReason = emptyReason
    }

    public static func decode(_ value: JSONValue?) -> LiveReviewDiff {
        let f = value?.fields ?? [:]
        func int(_ v: JSONValue?) -> Int? { v?.numberText.flatMap { Int($0) } }
        let files = (f["files"]?.arrayValue ?? []).compactMap { item -> File? in
            let g = item.fields
            guard let path = g["path"]?.stringValue else { return nil }
            let hunks = (g["hunks"]?.arrayValue ?? []).compactMap { h -> Hunk? in
                let k = h.fields
                guard let id = k["id"]?.stringValue else { return nil }
                let lines = (k["lines"]?.arrayValue ?? []).map { l in
                    Line(kind: l.fields["kind"]?.stringValue ?? " ", text: l.fields["text"]?.stringValue ?? "",
                         old: int(l.fields["old"]), new: int(l.fields["new"]))
                }
                return Hunk(id: id, header: k["header"]?.stringValue ?? "", lines: lines,
                            state: k["state"]?.stringValue ?? "pending", comment: k["comment"]?.stringValue,
                            commentSent: k["commentSent"]?.boolValue ?? false)
            }
            return File(path: path, binary: g["binary"]?.boolValue ?? false, additions: int(g["additions"]) ?? 0,
                        deletions: int(g["deletions"]) ?? 0, hunks: hunks)
        }
        return LiveReviewDiff(files: files, unsent: int(f["unsent"]) ?? 0,
                              seenMs: f["seenMs"]?.numberText.flatMap { UInt64($0) },
                              emptyReason: f["emptyReason"]?.stringValue)
    }
}
