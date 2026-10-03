import AeroricProtocol
import Foundation

/// P7-05：随手记（notebook）类型化客户端。宿主服务面与 Tauri `notebook_*`
/// 命令逐一镜像（`register_notebook_services`）；路径闸门 server 端权威
/// （vault allowlist），客户端只传路径文本，不做客户端侧过滤。
///
/// wire 形状与 core serde camelCase 输出对齐；解码走 JSONSerialization 桥
/// （`foundationObject` → `JSONDecoder`），结构体字段即契约。
public struct NotebookClient: Sendable {
    public enum MalformedReply: Error, Equatable {
        case malformedReply(String)
    }

    let connection: HostConnection
    private let decoder = JSONDecoder()

    public init(connection: HostConnection) {
        self.connection = connection
    }

    private func decode<T: Decodable>(_ value: JSONValue, as type: T.Type) throws -> T {
        let object = value.foundationObject
        guard JSONSerialization.isValidJSONObject(object) else {
            throw MalformedReply.malformedReply("notebook reply is not a JSON object")
        }
        let data = try JSONSerialization.data(withJSONObject: object)
        return try decoder.decode(T.self, from: data)
    }

    private func call<T: Decodable>(
        _ method: String,
        params: JSONValue,
        as type: T.Type
    ) async throws -> T {
        let frame = try await connection.request(method, params: params)
        return try decode(frame.result, as: type)
    }

    private func call(_ method: String, params: JSONValue) async throws {
        _ = try await connection.request(method, params: params)
    }

    // MARK: - 共用 wire 类型

    /// 文件指纹（mtime + 内容哈希）——多窗口 `expectedRevision` 冲突判定的基线。
    public struct FileSig: Codable, Equatable, Sendable {
        public var mtimeMs: Int64
        public var hash: String
    }

    public struct NoteEntry: Codable, Equatable, Sendable {
        public var name: String
        public var path: String
        public var isDir: Bool
        public var size: UInt64
        public var modifiedMs: Int64
        public var children: [NoteEntry]?
        /// 因触到深度/数量上限而没继续往下扫——UI 要能告诉用户，不假装空目录。
        public var truncated: Bool
    }

    public enum SaveOutcome: Equatable, Sendable {
        case saved(FileSig)
        case conflict(disk: FileSig)
    }

    public struct TrashItem: Codable, Equatable, Sendable {
        public var id: String
        public var name: String
        public var relativePath: String
        public var deletedAtMs: UInt64
        public var size: UInt64
        public var isDir: Bool
    }

    public struct RestoredItem: Codable, Equatable, Sendable {
        public var path: String
        public var isDir: Bool
    }

    public struct SavedAttachment: Codable, Equatable, Sendable {
        public var path: String
        public var name: String
        /// 相对**笔记所在目录**的路径，可直接写进 markdown。
        public var link: String
        /// 现成的 markdown 片段（图片带 `!` 前缀）。
        public var markdown: String
        public var size: UInt64
    }

    public struct Attachment: Codable, Equatable, Sendable {
        public var path: String
        public var name: String
        public var relativePath: String
        public var size: UInt64
        public var modifiedMs: Int64
        public var kind: String
    }

    public struct NoteStat: Codable, Equatable, Sendable {
        public var size: UInt64
        public var modifiedMs: Int64
        public var createdMs: Int64?
    }

    public struct VaultIndexEntry: Codable, Equatable, Sendable {
        public var path: String
        public var title: String
    }

    public struct NoteLinkRef: Codable, Equatable, Sendable {
        public var raw: String
        public var line: UInt32
        public var preview: String
        public var embed: Bool
    }

    public struct NoteLinkSource: Codable, Equatable, Sendable {
        public var path: String
        public var links: [NoteLinkRef]
    }

    public struct NoteTagRef: Codable, Equatable, Sendable {
        public var raw: String
        public var line: UInt32
        public var preview: String
    }

    public struct NoteTagSource: Codable, Equatable, Sendable {
        public var path: String
        public var tags: [NoteTagRef]
    }

    public struct NoteField: Codable, Equatable, Sendable {
        public var key: String
        public var values: [String]
    }

    public struct NoteFieldSource: Codable, Equatable, Sendable {
        public var path: String
        public var fields: [NoteField]
    }

    public struct NoteTaskRef: Codable, Equatable, Sendable {
        public var line: UInt32
        public var checked: Bool
        public var text: String
    }

    public struct NoteTaskSource: Codable, Equatable, Sendable {
        public var path: String
        public var tasks: [NoteTaskRef]
    }

    public struct MentionHit: Codable, Equatable, Sendable {
        public var needle: String
        public var text: String
        public var line: UInt32
        public var start: Int
        public var end: Int
        public var confidence: String?
    }

    public struct MentionSource: Codable, Equatable, Sendable {
        public var path: String
        public var mentions: [MentionHit]
    }

    /// 链接一处提及的请求载荷（用户在列表里看见过的那处：路径＋字节区间＋原文）。
    public struct MentionTarget: Equatable, Sendable {
        public var path: String
        public var start: Int
        public var end: Int
        public var text: String

        var json: JSONValue {
            .object([
                "path": .string(path),
                "start": .number(String(start)),
                "end": .number(String(end)),
                "text": .string(text),
            ])
        }
    }

    public struct LocalHistoryEntry: Codable, Equatable, Sendable {
        public var id: String
        public var filePath: String
        public var relativePath: String
        public var createdAtMs: UInt64
        public var size: UInt64
    }

    public struct LocalHistorySnapshot: Codable, Equatable, Sendable {
        public var entry: LocalHistoryEntry
        public var content: String
    }

    public struct RestoredNote: Codable, Equatable, Sendable {
        public var content: String
        public var sig: FileSig
        public var entry: LocalHistoryEntry
    }

    public struct UserTemplate: Codable, Equatable, Sendable {
        public var id: String
        public var title: String
        public var name: String
        public var body: String
    }

    public struct ImportReport: Codable, Equatable, Sendable {
        public var provider: String
        public var dest: String
        public var imported: Int
        public var skipped: Int
        public var failed: Int
        public var resourceLost: Int
        public var degraded: Int
        public var truncated: Int
        public var reportPath: String?
    }

    // MARK: - vault 注册

    public func registerVault(path: String) async throws -> String {
        let frame = try await connection.request(
            "notebook.register_vault", params: .object(["path": .string(path)]))
        guard case let .string(canon)? = frame.result["path"] else {
            throw MalformedReply.malformedReply("notebook.register_vault reply missing path")
        }
        return canon
    }

    public func unregisterVault(path: String) async throws {
        try await call(
            "notebook.unregister_vault", params: .object(["path": .string(path)]))
    }

    public func listVaults() async throws -> [String] {
        let frame = try await connection.request(
            "notebook.list_vaults", params: .object([:]))
        guard case let .array(items)? = frame.result["vaults"] else {
            throw MalformedReply.malformedReply("notebook.list_vaults reply missing vaults")
        }
        return items.compactMap(\.stringValue)
    }

    /// 确保全局默认 vault 存在并注册。客户端启动时调一次——不调则所有文件命令
    /// 因 allowlist 为空被拒。
    public func ensureDefaultVault() async throws -> String {
        let frame = try await connection.request(
            "notebook.ensure_default_vault", params: .object([:]))
        guard case let .string(path)? = frame.result["path"] else {
            throw MalformedReply.malformedReply("notebook.ensure_default_vault reply missing path")
        }
        return path
    }

    public func ensureProjectVault(projectPath: String) async throws -> String {
        let frame = try await connection.request(
            "notebook.ensure_project_vault",
            params: .object(["projectPath": .string(projectPath)]))
        guard case let .string(path)? = frame.result["path"] else {
            throw MalformedReply.malformedReply(
                "notebook.ensure_project_vault reply missing path")
        }
        return path
    }

    // MARK: - 读写

    public func readTree(root: String) async throws -> [NoteEntry] {
        let frame = try await connection.request(
            "notebook.read_tree", params: .object(["root": .string(root)]))
        guard case let .array(items)? = frame.result["entries"] else {
            throw MalformedReply.malformedReply("notebook.read_tree reply missing entries")
        }
        return try items.map { try decode($0, as: NoteEntry.self) }
    }

    public func readOrder(vault: String) async throws -> [String] {
        let frame = try await connection.request(
            "notebook.read_order", params: .object(["vault": .string(vault)]))
        guard case let .array(items)? = frame.result["order"] else { return [] }
        return items.compactMap(\.stringValue)
    }

    public func writeOrder(vault: String, names: [String]) async throws {
        try await call(
            "notebook.write_order",
            params: .object(["vault": .string(vault), "names": .array(names.map { .string($0) })]))
    }

    public func readIcons(vault: String) async throws -> [String: String] {
        let frame = try await connection.request(
            "notebook.read_icons", params: .object(["vault": .string(vault)]))
        guard case let .object(icons)? = frame.result["icons"] else { return [:] }
        return icons.compactMapValues(\.stringValue)
    }

    public func writeIcons(vault: String, icons: [String: String]) async throws {
        try await call(
            "notebook.write_icons",
            params: .object([
                "vault": .string(vault),
                "icons": .object(icons.mapValues { .string($0) }),
            ]))
    }

    public func openNote(path: String) async throws -> (content: String, sig: FileSig) {
        let frame = try await connection.request(
            "notebook.open_note", params: .object(["path": .string(path)]))
        guard case let .string(content)? = frame.result["content"],
              let sig = try? decode(frame.result["sig"] ?? .null, as: FileSig.self)
        else {
            throw MalformedReply.malformedReply("notebook.open_note reply missing content/sig")
        }
        return (content, sig)
    }

    /// 只读地取一篇笔记（嵌入 `![[note]]` 用）——**不**登记打开指纹。
    public func peekNote(path: String) async throws -> (content: String, sig: FileSig) {
        let frame = try await connection.request(
            "notebook.peek_note", params: .object(["path": .string(path)]))
        guard case let .string(content)? = frame.result["content"],
              let sig = try? decode(frame.result["sig"] ?? .null, as: FileSig.self)
        else {
            throw MalformedReply.malformedReply("notebook.peek_note reply missing content/sig")
        }
        return (content, sig)
    }

    public func closeNote(path: String) async throws {
        try await call("notebook.close_note", params: .object(["path": .string(path)]))
    }

    /// 保存。`expected` 是客户端持有的基线指纹；磁盘已变时返回 `.conflict`——
    /// 冲突不是错误，是需要用户决策的正常分支，**不静默覆盖**。
    public func saveNote(
        path: String, content: String, expected: FileSig?, force: Bool = false
    ) async throws -> SaveOutcome {
        var params: [String: JSONValue] = [
            "path": .string(path), "content": .string(content),
        ]
        if let expected {
            params["expected"] = .object([
                "mtimeMs": .number(String(expected.mtimeMs)), "hash": .string(expected.hash),
            ])
        }
        if force { params["force"] = .bool(true) }
        let frame = try await connection.request(
            "notebook.save_note", params: .object(params))
        guard case let .object(outcome)? = frame.result["outcome"] else {
            throw MalformedReply.malformedReply("notebook.save_note reply missing outcome")
        }
        switch outcome["status"]?.stringValue {
        case "Saved":
            guard let sig = try? decode(outcome["sig"] ?? .null, as: FileSig.self) else {
                throw MalformedReply.malformedReply("notebook.save_note Saved missing sig")
            }
            return .saved(sig)
        case "Conflict":
            guard let disk = try? decode(outcome["disk"] ?? .null, as: FileSig.self) else {
                throw MalformedReply.malformedReply("notebook.save_note Conflict missing disk")
            }
            return .conflict(disk: disk)
        default:
            throw MalformedReply.malformedReply("notebook.save_note unknown outcome status")
        }
    }

    public func createNote(path: String, content: String) async throws -> FileSig {
        let frame = try await connection.request(
            "notebook.create_note",
            params: .object(["path": .string(path), "content": .string(content)]))
        guard let sig = try? decode(frame.result["sig"] ?? .null, as: FileSig.self) else {
            throw MalformedReply.malformedReply("notebook.create_note reply missing sig")
        }
        return sig
    }

    /// 按标题在 vault 里新建笔记，文件名由后端分配（slug 规则只在后端一份）。
    public func createNoteInVault(vault: String, title: String, content: String) async throws
        -> (path: String, sig: FileSig)
    {
        let frame = try await connection.request(
            "notebook.create_note_in_vault",
            params: .object([
                "vault": .string(vault), "title": .string(title), "content": .string(content),
            ]))
        guard case let .string(path)? = frame.result["path"],
              let sig = try? decode(frame.result["sig"] ?? .null, as: FileSig.self)
        else {
            throw MalformedReply.malformedReply(
                "notebook.create_note_in_vault reply missing path/sig")
        }
        return (path, sig)
    }

    public func renameToTitle(vault: String, path: String, title: String) async throws -> String {
        let frame = try await connection.request(
            "notebook.rename_to_title",
            params: .object([
                "vault": .string(vault), "path": .string(path), "title": .string(title),
            ]))
        guard case let .string(newPath)? = frame.result["path"] else {
            throw MalformedReply.malformedReply("notebook.rename_to_title reply missing path")
        }
        return newPath
    }

    public func createFolder(path: String) async throws {
        try await call("notebook.create_folder", params: .object(["path": .string(path)]))
    }

    public func renameNote(from: String, to: String) async throws {
        try await call(
            "notebook.rename_note", params: .object(["from": .string(from), "to": .string(to)]))
    }

    /// 软删到 `<vault>/.notebook/trash/`——不是 unlink，也不是系统回收站。
    public func deleteNote(path: String) async throws -> TrashItem {
        let frame = try await connection.request(
            "notebook.delete_note", params: .object(["path": .string(path)]))
        guard let item = try? decode(frame.result["item"] ?? .null, as: TrashItem.self) else {
            throw MalformedReply.malformedReply("notebook.delete_note reply missing item")
        }
        return item
    }

    // MARK: - 回收站

    public func trashList(vault: String) async throws -> [TrashItem] {
        let frame = try await connection.request(
            "notebook.trash_list", params: .object(["vault": .string(vault)]))
        guard case let .array(items)? = frame.result["items"] else { return [] }
        return try items.map { try decode($0, as: TrashItem.self) }
    }

    public func trashRestore(vault: String, id: String) async throws -> RestoredItem {
        try await call(
            "notebook.trash_restore", params: .object(["vault": .string(vault), "id": .string(id)]),
            as: RestoredItem.self)
    }

    public func trashPurge(vault: String, id: String) async throws {
        try await call(
            "notebook.trash_purge", params: .object(["vault": .string(vault), "id": .string(id)]))
    }

    public func trashPurgeAll(vault: String) async throws -> Int {
        let frame = try await connection.request(
            "notebook.trash_purge_all", params: .object(["vault": .string(vault)]))
        guard case let .number(count)? = frame.result["purged"] else { return 0 }
        return Int(count) ?? 0
    }

    // MARK: - 附件

    public func attachmentSave(
        note: String, mime: String, dataBase64: String, fileName: String?
    ) async throws -> SavedAttachment {
        var params: [String: JSONValue] = [
            "note": .string(note), "mime": .string(mime), "dataBase64": .string(dataBase64),
        ]
        if let fileName { params["fileName"] = .string(fileName) }
        return try await call(
            "notebook.attachment_save", params: .object(params), as: SavedAttachment.self)
    }

    public func attachmentSaveFromPath(note: String, src: String) async throws -> SavedAttachment {
        try await call(
            "notebook.attachment_save_from_path",
            params: .object(["note": .string(note), "src": .string(src)]),
            as: SavedAttachment.self)
    }

    public func attachmentList(vault: String, max: Int? = nil) async throws -> [Attachment] {
        var params: [String: JSONValue] = ["vault": .string(vault)]
        if let max { params["max"] = .number(String(max)) }
        let frame = try await connection.request(
            "notebook.attachment_list", params: .object(params))
        guard case let .array(items)? = frame.result["items"] else { return [] }
        return try items.map { try decode($0, as: Attachment.self) }
    }

    public func attachmentRead(path: String) async throws -> Data {
        let frame = try await connection.request(
            "notebook.attachment_read", params: .object(["path": .string(path)]))
        guard case let .string(base64)? = frame.result["dataBase64"],
              let data = Data(base64Encoded: base64)
        else {
            throw MalformedReply.malformedReply("notebook.attachment_read reply missing bytes")
        }
        return data
    }

    // MARK: - 元数据与扫描面

    public func noteStat(path: String) async throws -> NoteStat {
        try await call(
            "notebook.note_stat", params: .object(["path": .string(path)]), as: NoteStat.self)
    }

    public func vaultIndex(vault: String) async throws -> [VaultIndexEntry] {
        let frame = try await connection.request(
            "notebook.vault_index", params: .object(["vault": .string(vault)]))
        guard case let .array(items)? = frame.result["entries"] else { return [] }
        return try items.map { try decode($0, as: VaultIndexEntry.self) }
    }

    public func vaultLinks(vault: String) async throws -> [NoteLinkSource] {
        let frame = try await connection.request(
            "notebook.vault_links", params: .object(["vault": .string(vault)]))
        guard case let .array(items)? = frame.result["links"] else { return [] }
        return try items.map { try decode($0, as: NoteLinkSource.self) }
    }

    public func vaultTags(vault: String) async throws -> [NoteTagSource] {
        let frame = try await connection.request(
            "notebook.vault_tags", params: .object(["vault": .string(vault)]))
        guard case let .array(items)? = frame.result["tags"] else { return [] }
        return try items.map { try decode($0, as: NoteTagSource.self) }
    }

    public func vaultFields(vault: String) async throws -> [NoteFieldSource] {
        let frame = try await connection.request(
            "notebook.vault_fields", params: .object(["vault": .string(vault)]))
        guard case let .array(items)? = frame.result["fields"] else { return [] }
        return try items.map { try decode($0, as: NoteFieldSource.self) }
    }

    public func vaultTasks(vault: String) async throws -> [NoteTaskSource] {
        let frame = try await connection.request(
            "notebook.vault_tasks", params: .object(["vault": .string(vault)]))
        guard case let .array(items)? = frame.result["tasks"] else { return [] }
        return try items.map { try decode($0, as: NoteTaskSource.self) }
    }

    public func vaultMentions(vault: String, note: String, names: [String]) async throws
        -> [MentionSource]
    {
        let frame = try await connection.request(
            "notebook.vault_mentions",
            params: .object([
                "vault": .string(vault), "note": .string(note),
                "names": .array(names.map { .string($0) }),
            ]))
        guard case let .array(items)? = frame.result["mentions"] else { return [] }
        return try items.map { try decode($0, as: MentionSource.self) }
    }

    /// 把指定的那几处提及包成 `[[..]]`。targets 必须是用户看见过的那几处——
    /// 重读后对不上的报 `vanished`，不静默包新内容。
    public func linkMentions(vault: String, targets: [MentionTarget]) async throws {
        try await call(
            "notebook.link_mentions",
            params: .object([
                "vault": .string(vault), "targets": .array(targets.map(\.json)),
            ]))
    }

    public func renameTag(vault: String, old: String, new: String) async throws {
        try await call(
            "notebook.rename_tag",
            params: .object(["vault": .string(vault), "old": .string(old), "new": .string(new)]))
    }

    // MARK: - 版本历史

    public func listSnapshots(path: String) async throws -> [LocalHistoryEntry] {
        let frame = try await connection.request(
            "notebook.list_snapshots", params: .object(["path": .string(path)]))
        guard case let .array(items)? = frame.result["entries"] else { return [] }
        return try items.map { try decode($0, as: LocalHistoryEntry.self) }
    }

    public func readSnapshot(path: String, entryId: String) async throws -> LocalHistorySnapshot {
        try await call(
            "notebook.read_snapshot",
            params: .object(["path": .string(path), "entryId": .string(entryId)]),
            as: LocalHistorySnapshot.self)
    }

    /// 回滚前自动留兜底快照——回滚本身可撤销。
    public func restoreSnapshot(path: String, entryId: String) async throws -> RestoredNote {
        try await call(
            "notebook.restore_snapshot",
            params: .object(["path": .string(path), "entryId": .string(entryId)]),
            as: RestoredNote.self)
    }

    // MARK: - 迁移与模板

    public func migrateLegacy(rawJson: String) async throws {
        try await call(
            "notebook.migrate_legacy", params: .object(["rawJson": .string(rawJson)]))
    }

    public func convertRichtext(vault: String) async throws {
        try await call("notebook.convert_richtext", params: .object(["vault": .string(vault)]))
    }

    public func listUserTemplates(vault: String) async throws -> [UserTemplate] {
        let frame = try await connection.request(
            "notebook.list_user_templates", params: .object(["vault": .string(vault)]))
        guard case let .array(items)? = frame.result["templates"] else { return [] }
        return try items.map { try decode($0, as: UserTemplate.self) }
    }

    // MARK: - 导入

    /// 七家导入格式的统一入口；report 带逐条 imported/skipped/failed 明细，
    /// 失败也有部分报告——已落地的部分不会被丢掉。
    public func runImport(provider: NotebookImportProvider, vault: String, source: String)
        async throws -> ImportReport
    {
        let method: String
        let sourceKey: String
        switch provider {
        case .obsidian:
            method = "notebook.import_obsidian"
            sourceKey = "sourcePath"
        case .logseq:
            method = "notebook.import_logseq"
            sourceKey = "sourcePath"
        case .notion:
            method = "notebook.import_notion"
            sourceKey = "zipPath"
        case .bear:
            method = "notebook.import_bear"
            sourceKey = "zipPath"
        case .roam:
            method = "notebook.import_roam"
            sourceKey = "zipPath"
        case .evernote:
            method = "notebook.import_evernote"
            sourceKey = "enexPath"
        case .appleNotes:
            // Apple Notes 不收源路径（读系统 Notes 库，仅 macOS）。
            return try await call(
                "notebook.import_apple_notes", params: .object(["vault": .string(vault)]),
                as: ImportReport.self)
        }
        return try await call(
            method, params: .object(["vault": .string(vault), sourceKey: .string(source)]),
            as: ImportReport.self)
    }

    // MARK: - 导出

    /// 单文档导出。路径来自客户端，白名单（vault 内＋桌面/文档/下载）在服务端。
    public func exportWriteFile(path: String, content: String) async throws {
        try await call(
            "notebook.export_write_file",
            params: .object(["path": .string(path), "content": .string(content)]))
    }

    public func exportSiteWrite(outDir: String, relPath: String, content: String) async throws {
        try await call(
            "notebook.export_site_write",
            params: .object([
                "outDir": .string(outDir), "relPath": .string(relPath),
                "content": .string(content),
            ]))
    }

    public func htmlToMarkdown(_ html: String) async throws -> String {
        let frame = try await connection.request(
            "notebook.html_to_markdown", params: .object(["html": .string(html)]))
        guard case let .string(markdown)? = frame.result["markdown"] else {
            throw MalformedReply.malformedReply("notebook.html_to_markdown reply missing markdown")
        }
        return markdown
    }
}

/// 导入器清单（与宿主 7 条 `notebook.import_*` 服务一一对应）。
public enum NotebookImportProvider: String, CaseIterable, Sendable {
    case obsidian
    case logseq
    case notion
    case bear
    case roam
    case evernote
    case appleNotes
}
