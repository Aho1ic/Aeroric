import Foundation

/// B5.5-1 (2026-10-31): a project Wiki page as the host reports it.
public struct LiveWikiPage: Equatable, Sendable {
    public var slug: String
    /// `index` `decision` `runbook` `module`.
    public var kind: String
    public var title: String
    public var status: String?
    public var category: String?
    public var blocks: [LiveWikiBlock]
    public var revision: Int
    /// R4-1: the commit a module page's code was read at.
    public var sourceCommit: String?

    public init(slug: String, kind: String = "decision", title: String, status: String? = nil,
                category: String? = nil, blocks: [LiveWikiBlock] = [], revision: Int = 1,
                sourceCommit: String? = nil) {
        self.slug = slug
        self.kind = kind
        self.title = title
        self.status = status
        self.category = category
        self.blocks = blocks
        self.revision = revision
        self.sourceCommit = sourceCommit
    }

    public static func decode(_ value: JSONValue?) -> LiveWikiPage? {
        guard let f = value?.objectValue, let slug = f["slug"]?.stringValue else { return nil }
        return LiveWikiPage(slug: slug, kind: f["kind"]?.stringValue ?? "decision",
                            title: f["title"]?.stringValue ?? slug, status: f["status"]?.stringValue,
                            category: f["category"]?.stringValue,
                            blocks: (f["blocks"]?.arrayValue ?? []).compactMap(LiveWikiBlock.decode),
                            revision: f["revision"]?.numberText.flatMap { Int($0) } ?? 1,
                            sourceCommit: f["sourceCommit"]?.stringValue)
    }

    /// The page as Markdown (what 编辑 starts from); emptied blocks left out.
    public var markdown: String {
        var out = "# \(title)\n"
        for block in blocks where !(block.text.isEmpty && block.userEdited) {
            out += "\n"
            if let heading = block.heading { out += "## \(heading)\n\n" }
            out += block.text.trimmingCharacters(in: .whitespacesAndNewlines) + "\n"
        }
        return out
    }
}

public struct LiveWikiBlock: Equatable, Sendable, Identifiable {
    public var key: String
    public var heading: String?
    public var text: String
    public var source: LiveWikiSource?
    /// R4-1: every source that contributed (a module page's files).
    public var sources: [LiveWikiSource]
    public var userEdited: Bool
    public var id: String { key }

    public init(key: String, heading: String? = nil, text: String, source: LiveWikiSource? = nil,
                sources: [LiveWikiSource] = [], userEdited: Bool = false) {
        self.key = key
        self.heading = heading
        self.text = text
        self.source = source
        self.sources = sources
        self.userEdited = userEdited
    }

    public static func decode(_ value: JSONValue) -> LiveWikiBlock? {
        let f = value.fields
        guard let key = f["key"]?.stringValue else { return nil }
        return LiveWikiBlock(key: key, heading: f["heading"]?.stringValue, text: f["text"]?.stringValue ?? "",
                             source: f["source"].flatMap(LiveWikiSource.decode),
                             sources: (f["sources"]?.arrayValue ?? []).compactMap(LiveWikiSource.decode),
                             userEdited: f["userEdited"]?.boolValue ?? false)
    }
}

public struct LiveWikiSource: Equatable, Sendable {
    /// `note` `memory` `skill` `code`.
    public var kind: String
    /// A note's project-relative path `#` section; a memory id; a skill
    /// name; a code file's project-relative path.
    public var target: String
    public var label: String
    /// R4-1: a code file's git blob hash when it was read.
    public var version: String?

    public init(kind: String, target: String, label: String, version: String? = nil) {
        self.kind = kind
        self.target = target
        self.label = label
        self.version = version
    }

    public static func decode(_ value: JSONValue) -> LiveWikiSource? {
        let f = value.fields
        guard let kind = f["kind"]?.stringValue, let target = f["target"]?.stringValue else { return nil }
        return LiveWikiSource(kind: kind, target: target, label: f["label"]?.stringValue ?? target,
                              version: f["version"]?.stringValue)
    }

    /// A note's path without the `#section`; a code file's path.
    public var notePath: String? {
        switch kind {
        case "note": String(target.split(separator: "#", maxSplits: 1).first ?? "")
        case "code": target
        default: nil
        }
    }
}

public struct LiveWikiPageSummary: Equatable, Sendable, Identifiable {
    public var slug: String
    public var kind: String
    public var title: String
    public var status: String?
    public var category: String?
    public var edited: Int
    public var id: String { slug }

    public init(slug: String, kind: String, title: String, status: String? = nil, category: String? = nil,
                edited: Int = 0) {
        self.slug = slug
        self.kind = kind
        self.title = title
        self.status = status
        self.category = category
        self.edited = edited
    }

    public static func decode(_ value: JSONValue) -> LiveWikiPageSummary? {
        let f = value.fields
        guard let slug = f["slug"]?.stringValue else { return nil }
        return LiveWikiPageSummary(slug: slug, kind: f["kind"]?.stringValue ?? "decision",
                                   title: f["title"]?.stringValue ?? slug, status: f["status"]?.stringValue,
                                   category: f["category"]?.stringValue,
                                   edited: f["edited"]?.numberText.flatMap { Int($0) } ?? 0)
    }
}

public struct LiveWikiRevision: Equatable, Sendable, Identifiable {
    public var n: Int
    /// `compile` `edit` `revert` `issue`.
    public var reason: String
    public var atMs: UInt64
    public var id: Int { n }

    public init(n: Int, reason: String, atMs: UInt64 = 0) {
        self.n = n
        self.reason = reason
        self.atMs = atMs
    }

    public static func decode(_ value: JSONValue) -> LiveWikiRevision? {
        let f = value.fields
        guard let n = f["n"]?.numberText.flatMap({ Int($0) }) else { return nil }
        return LiveWikiRevision(n: n, reason: f["reason"]?.stringValue ?? "compile",
                                atMs: f["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0)
    }
}

public struct LiveWikiIssue: Equatable, Sendable, Identifiable {
    public var id: String
    public var slug: String
    public var blockKey: String?
    /// `sourceChanged` `sourceRemoved` `agent`.
    public var kind: String
    public var text: String
    public var suggestion: String?
    public var open: Bool
    public var author: String

    public init(id: String, slug: String, blockKey: String? = nil, kind: String, text: String,
                suggestion: String? = nil, open: Bool = true, author: String = "compiler") {
        self.id = id
        self.slug = slug
        self.blockKey = blockKey
        self.kind = kind
        self.text = text
        self.suggestion = suggestion
        self.open = open
        self.author = author
    }

    public static func decode(_ value: JSONValue) -> LiveWikiIssue? {
        let f = value.fields
        guard let id = f["id"]?.stringValue, let slug = f["slug"]?.stringValue else { return nil }
        return LiveWikiIssue(id: id, slug: slug, blockKey: f["blockKey"]?.stringValue,
                             kind: f["kind"]?.stringValue ?? "agent", text: f["text"]?.stringValue ?? "",
                             suggestion: f["suggestion"]?.stringValue, open: f["open"]?.boolValue ?? true,
                             author: f["author"]?.stringValue ?? "compiler")
    }
}

public struct LiveWikiReport: Equatable, Sendable {
    public var added: Int
    public var updated: Int
    public var removed: Int
    public var issues: Int
    public var pages: Int

    public init(added: Int = 0, updated: Int = 0, removed: Int = 0, issues: Int = 0, pages: Int = 0) {
        self.added = added
        self.updated = updated
        self.removed = removed
        self.issues = issues
        self.pages = pages
    }

    public static func decode(_ value: JSONValue?) -> LiveWikiReport {
        let f = value?.fields ?? [:]
        let int = { (key: String) in f[key]?.numberText.flatMap { Int($0) } ?? 0 }
        return LiveWikiReport(added: int("added"), updated: int("updated"), removed: int("removed"),
                              issues: int("issues"), pages: int("pages"))
    }
}

/// R1-1: sources the last compile could not read.
public struct LiveWikiSourceGaps: Equatable, Sendable {
    public var all: Bool
    public var slugs: [String]

    public init(all: Bool = false, slugs: [String] = []) {
        self.all = all
        self.slugs = slugs
    }

    public static func decode(_ value: JSONValue?) -> LiveWikiSourceGaps {
        let f = value?.objectValue ?? [:]
        return LiveWikiSourceGaps(all: f["all"]?.boolValue ?? false,
                                  slugs: (f["slugs"]?.arrayValue ?? []).compactMap(\.stringValue))
    }
}

/// `wiki.pages`: the tree and what could not be read.
public struct LiveWikiPageList: Equatable, Sendable {
    public var pages: [LiveWikiPageSummary]
    public var gaps: LiveWikiSourceGaps

    public init(pages: [LiveWikiPageSummary] = [], gaps: LiveWikiSourceGaps = LiveWikiSourceGaps()) {
        self.pages = pages
        self.gaps = gaps
    }
}

/// `wiki.exportPlan`: what an export would write; `overwrites` already exist.
public struct LiveWikiExportPlan: Equatable, Sendable {
    public var files: [String]
    public var overwrites: [String]

    public init(files: [String] = [], overwrites: [String] = []) {
        self.files = files
        self.overwrites = overwrites
    }
}

