import Foundation

/// P7-05：随手记簿记与纯逻辑（Action 2 十视图的数据源）。
///
/// 身份模型：**path 就是 noteId**（vault 相对/绝对路径在前端只用宿主回传的绝对
/// 路径当主键），display title 只用于展示——重命名标题不改主键，改的只有
/// `renameNote` 产生的新路径。
///
/// 冲突模型：打开时记 baseline [`FileSig`]（mtime+hash），保存时把客户端手里的
/// 基线交给宿主判定；`.conflict` 回来的磁盘现状**必须**交给用户决策——
/// 多窗口 expectedRevision 冲突提示，不静默覆盖。
@MainActor
@Observable
public final class NotebookStore {
    // MARK: - 共用 wire 类型（与 core serde camelCase 对齐；宿主 NotebookClient
    // 的同型结构在此重复一份以避免 AeroricUI → AeroricClient 的测试期强耦合）

    public struct FileSig: Equatable, Sendable {
        public var mtimeMs: Int64
        public var hash: String

        public init(mtimeMs: Int64, hash: String) {
            self.mtimeMs = mtimeMs
            self.hash = hash
        }
    }

    public struct TreeEntry: Equatable, Sendable, Identifiable {
        public var name: String
        public var path: String
        public var isDir: Bool
        public var children: [TreeEntry]?
        public var truncated: Bool

        public var id: String { path }

        public init(name: String, path: String, isDir: Bool, children: [TreeEntry]?, truncated: Bool) {
            self.name = name
            self.path = path
            self.isDir = isDir
            self.children = children
            self.truncated = truncated
        }
    }

    public struct TrashEntry: Identifiable, Equatable, Sendable {
        public var id: String
        public var name: String
        public var relativePath: String
        public var deletedAtMs: UInt64
        public var isDir: Bool

        public init(id: String, name: String, relativePath: String, deletedAtMs: UInt64, isDir: Bool) {
            self.id = id
            self.name = name
            self.relativePath = relativePath
            self.deletedAtMs = deletedAtMs
            self.isDir = isDir
        }
    }

    public struct TagRef: Equatable, Sendable {
        public var raw: String
        public var line: UInt32
        public var preview: String

        public init(raw: String, line: UInt32, preview: String) {
            self.raw = raw
            self.line = line
            self.preview = preview
        }
    }

    public struct TagSource: Equatable, Sendable {
        public var path: String
        public var tags: [TagRef]

        public init(path: String, tags: [TagRef]) {
            self.path = path
            self.tags = tags
        }
    }

    /// 聚合后的一条标签：归一化 key 相同的出现合并计数（重名标签的每一处
    /// 出现单独可数，大小写不敏感聚合与面板排序都在同一份数据上算）。
    public struct TagSummary: Identifiable, Equatable {
        public var id: String { key }
        public var key: String
        public var sample: String
        public var count: Int
        public var files: Set<String>

        public init(key: String, sample: String, count: Int, files: Set<String>) {
            self.key = key
            self.sample = sample
            self.count = count
            self.files = files
        }
    }

    public struct TaskEntry: Identifiable, Equatable, Sendable {
        public var path: String
        public var line: UInt32
        public var checked: Bool
        public var text: String

        public var id: String { "\(path)#\(line)" }

        public init(path: String, line: UInt32, checked: Bool, text: String) {
            self.path = path
            self.line = line
            self.checked = checked
            self.text = text
        }
    }

    public struct Template: Identifiable, Equatable, Sendable {
        public var id: String
        public var title: String
        public var name: String
        public var body: String

        public init(id: String, title: String, name: String, body: String) {
            self.id = id
            self.title = title
            self.name = name
            self.body = body
        }
    }

    public struct ImportSummary: Equatable, Sendable {
        public var provider: String
        public var imported: Int
        public var skipped: Int
        public var failed: Int
        public var reportPath: String?

        public init(provider: String, imported: Int, skipped: Int, failed: Int, reportPath: String?) {
            self.provider = provider
            self.imported = imported
            self.skipped = skipped
            self.failed = failed
            self.reportPath = reportPath
        }
    }

    // MARK: - 簿记

    public private(set) var vaults: [String] = []
    public private(set) var vaultRoot: String?
    public private(set) var tree: [TreeEntry] = []
    /// 打开文件的基线指纹（path → sig）。保存的 expected 来源；关闭/软删时清除。
    private var baselines: [String: FileSig] = [:]
    public private(set) var dirtyPaths: Set<String> = []
    public private(set) var trash: [TrashEntry] = []
    public private(set) var tags: [TagSummary] = []
    public private(set) var tasks: [TaskEntry] = []
    public private(set) var templates: [Template] = []
    /// 最近一次导入的部分报告（含失败样本——导入失败不清掉已落地的部分）。
    public private(set) var lastImport: ImportSummary?

    public init() {}

    // MARK: - vault 与树

    public func ingestVaults(_ vaults: [String], root: String?) {
        self.vaults = vaults
        if let root { self.vaultRoot = root }
    }

    public func ingestTree(_ entries: [TreeEntry]) {
        tree = entries
    }

    /// 树懒加载：展开目录时把子层接进同一棵树。
    public func ingestChildren(of path: String, entries: [TreeEntry]) {
        ingestChildren(&tree, of: path, entries: entries)
    }

    private func ingestChildren(_ nodes: inout [TreeEntry], of path: String, entries: [TreeEntry]) {
        for index in nodes.indices {
            if nodes[index].path == path {
                nodes[index].children = entries
                return
            }
            if nodes[index].children != nil {
                var children = nodes[index].children ?? []
                ingestChildren(&children, of: path, entries: entries)
                nodes[index].children = children
            }
        }
    }

    // MARK: - 基线（打开/保存/关闭）

    public func recordOpen(path: String, sig: FileSig) {
        baselines[path] = sig
        dirtyPaths.remove(path)
    }

    public func recordClose(path: String) {
        baselines.removeValue(forKey: path)
        dirtyPaths.remove(path)
    }

    public func recordDirty(path: String) {
        dirtyPaths.insert(path)
    }

    public func baseline(of path: String) -> FileSig? {
        baselines[path]
    }

    public enum SaveDisposition: Equatable {
        case saved(FileSig)
        /// 磁盘内容已经不是手里的基线——交给用户决策，**这里不写任何盘**。
        case conflict(disk: FileSig)
    }

    /// 保存成功的唯一记账点：更新基线并清 dirty。
    public func applySaved(path: String, sig: FileSig) {
        baselines[path] = sig
        dirtyPaths.remove(path)
    }

    /// 冲突时宿主带回的磁盘现状。**这里不写任何盘、不改基线**——决策
    /// （force 覆盖 / 放弃）由视图层弹窗承载，只有 force 保存成功的那次
    /// `applySaved` 才会更新基线。
    public func conflictDisk(path: String) -> FileSig? {
        baselines[path] == nil ? nil : baselines[path]
    }

    // MARK: - 回收站

    public func ingestTrash(_ entries: [TrashEntry]) {
        // 宿主已按新删在前排序；这里保持原序。
        trash = entries
    }

    public func removeTrash(id: String) {
        trash.removeAll { $0.id == id }
    }

    public func clearTrash() {
        trash.removeAll()
    }

    // MARK: - 标签聚合

    /// 归一化 key：小写化。与面板聚合口径一致（raw 保持原大小写）。
    public static func tagKey(_ raw: String) -> String {
        raw.lowercased()
    }

    public func ingestTags(_ sources: [TagSource]) {
        var order: [String] = []
        var byKey: [String: TagSummary] = [:]
        for source in sources {
            for tag in source.tags {
                let key = Self.tagKey(tag.raw)
                if var summary = byKey[key] {
                    summary.count += 1
                    summary.files.insert(source.path)
                    byKey[key] = summary
                } else {
                    order.append(key)
                    byKey[key] = TagSummary(
                        key: key, sample: tag.raw, count: 1, files: [source.path])
                }
            }
        }
        tags = order.compactMap { byKey[$0] }.sorted {
            $0.count != $1.count ? $0.count > $1.count : $0.key < $1.key
        }
    }

    // MARK: - 任务

    public func ingestTasks(_ entries: [TaskEntry]) {
        tasks = entries
    }

    /// 把 `line`（1-based）上的复选框翻转；返回新正文。行号按整个 `.md` 文件数
    /// （与扫描同一坐标系）。翻不动（行不存在/不是任务）返回 `nil`——调用方
    /// 不该拿着改错的正文去保存。
    public static func flipTaskLine(content: String, line: UInt32) -> String? {
        guard line >= 1 else { return nil }
        var lines = content.components(separatedBy: "\n")
        let index = Int(line) - 1
        guard index < lines.count else { return nil }
        let original = lines[index]
        let flipped: String
        if let range = findCheckbox(in: original, marker: "[ ]") {
            flipped = original.replacingCharacters(in: range, with: "[x]")
        } else if let range = findCheckbox(in: original, marker: "[X]") {
            flipped = original.replacingCharacters(in: range, with: "[ ]")
        } else if let range = findCheckbox(in: original, marker: "[x]") {
            flipped = original.replacingCharacters(in: range, with: "[ ]")
        } else {
            return nil
        }
        lines[index] = flipped
        return lines.joined(separator: "\n")
    }

    private static func findCheckbox(in text: String, marker: String) -> Range<String.Index>? {
        guard let range = text.range(of: marker) else { return nil }
        // 复选框前必须正好是列表/任务前缀（`-`、`*`、`1.` + 空格），行中段的
        // `[x]` 文本不算任务——翻它会写坏正文。
        let prefix = text[text.startIndex..<range.lowerBound].trimmingCharacters(in: .whitespaces)
        return prefix.allSatisfy { $0 == "-" || $0 == "*" || $0 == ">" || $0.isNumber || $0 == "." }
            ? range : nil
    }

    // MARK: - 模板占位符

    /// 展开 `{{date}}` / `{{time}}`（大小写不敏感）。`{{title}}` **不动**——
    /// 要等标题最终定下来（与 React `fillDateTime`/`fillTitle` 同语义；
    /// 日期必须按用户本地时区算，所以留在客户端）。
    public static func fillDateTime(_ text: String, now: Date = Date()) -> String {
        let components = Calendar.current.dateComponents([.year, .month, .day, .hour, .minute], from: now)
        let pad = { (value: Int) in String(format: "%02d", value) }
        let ymd = "\(components.year ?? 0)-\(pad(components.month ?? 0))-\(pad(components.day ?? 0))"
        let hhmm = "\(pad(components.hour ?? 0)):\(pad(components.minute ?? 0))"
        return text
            .replacingOccurrences(
                of: "\\{\\{\\s*date\\s*\\}\\}", with: ymd, options: [.regularExpression, .caseInsensitive])
            .replacingOccurrences(
                of: "\\{\\{\\s*time\\s*\\}\\}", with: hhmm, options: [.regularExpression, .caseInsensitive])
    }

    /// 展开 `{{title}}`。标题定下来之后才调。
    public static func fillTitle(_ text: String, title: String) -> String {
        text.replacingOccurrences(
            of: "\\{\\{\\s*title\\s*\\}\\}", with: title, options: [.regularExpression, .caseInsensitive])
    }

    // MARK: - 导入

    public func ingestImport(_ summary: ImportSummary) {
        lastImport = summary
    }
}
