import Foundation

/// B6 (2026-11-18): what a task delivered. Mirrors `aeroric_protocol::artifacts`.
public struct LiveArtifact: Equatable, Sendable, Identifiable {
    public var id: String
    /// Relative to the repository's top directory.
    public var path: String
    /// `code` / `document` / `data` / `image` / `other`.
    public var kind: String
    public var createdMs: UInt64
    public var latestVersion: Int
    /// The user said it does not count as a deliverable.
    public var excluded: Bool
    /// A later turn deleted the file.
    public var removed: Bool
    /// The newest version not undone; nil when every version was undone.
    public var current: LiveArtifactVersion?

    public init(id: String, path: String, kind: String = "other", createdMs: UInt64 = 0, latestVersion: Int = 1,
                excluded: Bool = false, removed: Bool = false, current: LiveArtifactVersion? = nil) {
        self.id = id
        self.path = path
        self.kind = kind
        self.createdMs = createdMs
        self.latestVersion = latestVersion
        self.excluded = excluded
        self.removed = removed
        self.current = current
    }

    public var name: String { path.split(separator: "/").last.map(String.init) ?? path }

    /// The folder it is in; empty at the top.
    public var folder: String {
        guard let cut = path.lastIndex(of: "/") else { return "" }
        return String(path[..<cut])
    }

    public static func decode(_ value: JSONValue) -> LiveArtifact? {
        let f = value.fields
        guard let id = f["id"]?.stringValue, let path = f["path"]?.stringValue else { return nil }
        return LiveArtifact(
            id: id, path: path, kind: f["kind"]?.stringValue ?? "other",
            createdMs: f["createdMs"]?.numberText.flatMap { UInt64($0) } ?? 0,
            latestVersion: f["latestVersion"]?.numberText.flatMap { Int($0) } ?? 0,
            excluded: f["excluded"]?.boolValue ?? false, removed: f["removed"]?.boolValue ?? false,
            current: f["current"].flatMap(LiveArtifactVersion.decode))
    }
}

public struct LiveArtifactVersion: Equatable, Sendable, Identifiable {
    public var id: String { "\(artifactId)@\(version)" }
    public var artifactId: String
    public var version: Int
    public var turnId: String
    /// The Git blob id, which is also the content's hash.
    public var blob: String
    public var size: UInt64
    public var atMs: UInt64
    /// Its turn was undone.
    public var withdrawn: Bool
    /// A later version stands instead.
    public var superseded: Bool
    /// The checks that ran while it stood, oldest first.
    public var checks: [LiveArtifactCheck]

    public init(artifactId: String, version: Int, turnId: String = "", blob: String = "", size: UInt64 = 0,
                atMs: UInt64 = 0, withdrawn: Bool = false, superseded: Bool = false,
                checks: [LiveArtifactCheck] = []) {
        self.artifactId = artifactId
        self.version = version
        self.turnId = turnId
        self.blob = blob
        self.size = size
        self.atMs = atMs
        self.withdrawn = withdrawn
        self.superseded = superseded
        self.checks = checks
    }

    public static func decodeList(_ value: JSONValue?) -> [LiveArtifactVersion] {
        (value?.arrayValue ?? []).compactMap(decode)
    }

    public static func decode(_ value: JSONValue) -> LiveArtifactVersion? {
        let f = value.fields
        guard let artifactId = f["artifactId"]?.stringValue,
              let version = f["version"]?.numberText.flatMap({ Int($0) }) else { return nil }
        return LiveArtifactVersion(
            artifactId: artifactId, version: version, turnId: f["turnId"]?.stringValue ?? "",
            blob: f["blob"]?.stringValue ?? "", size: f["size"]?.numberText.flatMap { UInt64($0) } ?? 0,
            atMs: f["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0,
            withdrawn: f["withdrawn"]?.boolValue ?? false, superseded: f["superseded"]?.boolValue ?? false,
            checks: (f["checks"]?.arrayValue ?? []).compactMap(LiveArtifactCheck.decode))
    }
}

/// One check that saw a version.
public struct LiveArtifactCheck: Equatable, Sendable {
    /// The turn that ran it.
    public var turnId: String
    public var passed: Bool
    public var summary: String
    /// "结果可能不完整".
    public var incomplete: Bool

    public init(turnId: String, passed: Bool, summary: String = "", incomplete: Bool = false) {
        self.turnId = turnId
        self.passed = passed
        self.summary = summary
        self.incomplete = incomplete
    }

    static func decode(_ value: JSONValue) -> LiveArtifactCheck? {
        let f = value.fields
        guard let verdict = f["verdict"]?.stringValue else { return nil }
        return LiveArtifactCheck(turnId: f["evidence"]?.fields["detail"]?.stringValue ?? "",
                                 passed: verdict == "passed", summary: f["summary"]?.stringValue ?? "",
                                 incomplete: f["incomplete"]?.boolValue ?? false)
    }
}

/// One page of `artifacts.list`.
public struct LiveArtifactPage: Equatable, Sendable {
    public var artifacts: [LiveArtifact]
    public var nextCursor: String?
    /// Let go to keep the task under its limit.
    public var dropped: Int
    /// Why no versions can be kept (not a Git repository).
    public var unavailable: String?

    public init(artifacts: [LiveArtifact] = [], nextCursor: String? = nil, dropped: Int = 0,
                unavailable: String? = nil) {
        self.artifacts = artifacts
        self.nextCursor = nextCursor
        self.dropped = dropped
        self.unavailable = unavailable
    }

    public static func decode(_ value: JSONValue) -> LiveArtifactPage {
        let f = value.fields
        return LiveArtifactPage(
            artifacts: (f["artifacts"]?.arrayValue ?? []).compactMap(LiveArtifact.decode),
            nextCursor: f["nextCursor"]?.stringValue,
            dropped: f["dropped"]?.numberText.flatMap { Int($0) } ?? 0,
            unavailable: f["unavailable"]?.stringValue)
    }
}

/// A slice of one version's bytes (`artifacts.read`).
public struct LiveArtifactContent: Equatable, Sendable {
    public var version: Int
    public var size: UInt64
    public var offset: UInt64
    /// `utf8` or `base64`.
    public var encoding: String
    public var content: String
    public var nextOffset: UInt64?

    public init(version: Int, size: UInt64, offset: UInt64 = 0, encoding: String = "utf8", content: String,
                nextOffset: UInt64? = nil) {
        self.version = version
        self.size = size
        self.offset = offset
        self.encoding = encoding
        self.content = content
        self.nextOffset = nextOffset
    }

    /// The slice as text; nil for a binary file.
    public var text: String? { encoding == "utf8" ? content : nil }

    public static func decode(_ value: JSONValue) -> LiveArtifactContent? {
        let f = value.fields
        guard let content = f["content"]?.stringValue else { return nil }
        return LiveArtifactContent(
            version: f["version"]?.numberText.flatMap { Int($0) } ?? 0,
            size: f["size"]?.numberText.flatMap { UInt64($0) } ?? 0,
            offset: f["offset"]?.numberText.flatMap { UInt64($0) } ?? 0,
            encoding: f["encoding"]?.stringValue ?? "utf8", content: content,
            nextOffset: f["nextOffset"]?.numberText.flatMap { UInt64($0) })
    }
}
