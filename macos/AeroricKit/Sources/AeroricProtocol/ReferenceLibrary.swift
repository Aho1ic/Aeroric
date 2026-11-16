import Foundation

/// Q-R6 (2026-11-13): one folder of the reference library as `references.list` reports it.
public struct LiveReference: Equatable, Sendable, Identifiable {
    public var id: String
    public var name: String
    public var path: String
    public var purpose: String?
    public var files: Int
    public var symbols: Int
    public var indexedMs: UInt64?
    /// More files than the index takes: the rest are not searched.
    public var truncated: Bool
    public var indexing: Bool
    /// The folder changed since the last index.
    public var stale: Bool
    public var error: String?

    public init(id: String, name: String, path: String, purpose: String? = nil, files: Int = 0,
                symbols: Int = 0, indexedMs: UInt64? = nil, truncated: Bool = false,
                indexing: Bool = false, stale: Bool = false, error: String? = nil) {
        self.id = id
        self.name = name
        self.path = path
        self.purpose = purpose
        self.files = files
        self.symbols = symbols
        self.indexedMs = indexedMs
        self.truncated = truncated
        self.indexing = indexing
        self.stale = stale
        self.error = error
    }

    public static func decode(_ value: JSONValue?) -> LiveReference? {
        guard let f = value?.objectValue, let id = f["id"]?.stringValue,
              let path = f["path"]?.stringValue else { return nil }
        let int = { (key: String) in f[key]?.numberText.flatMap { Int($0) } ?? 0 }
        return LiveReference(
            id: id, name: f["name"]?.stringValue ?? id, path: path,
            purpose: f["purpose"]?.stringValue, files: int("files"), symbols: int("symbols"),
            indexedMs: f["indexedMs"]?.numberText.flatMap { UInt64($0) },
            truncated: f["truncated"]?.boolValue ?? false, indexing: f["indexing"]?.boolValue ?? false,
            stale: f["stale"]?.boolValue ?? false, error: f["error"]?.stringValue)
    }

    /// The row's second line: how far the index is.
    public var statusText: String {
        if let error { return "索引失败：\(error)" }
        if indexing { return files > 0 ? "正在更新索引…（已有 \(files) 个文件）" : "正在建索引…" }
        guard indexedMs != nil else { return "还没有索引" }
        var text = "\(files) 个文件 · \(symbols) 个定义"
        if truncated { text += " · 文件太多，只收了前一部分" }
        if stale { text += " · 有改动，下次检索时更新" }
        return text
    }
}
