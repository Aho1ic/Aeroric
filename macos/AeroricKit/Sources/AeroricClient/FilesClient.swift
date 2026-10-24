import AeroricProtocol
import Foundation

/// P6-01 Action 4：文件服务类型化客户端（宿主 `files.read_dir`/
/// `files.read_file`/`files.write_file`/`files.create_file`/
/// `files.create_directory`/`files.delete_path`/`files.rename_path`/
/// `files.replace_preview`/`files.apply_replacements`，复用
/// HostConnection.request）。客户端只传 projectId + relative path——路径
/// 校验 server 端权威，不依赖客户端过滤 `..`。
public struct FilesClient: Sendable {
    public enum MalformedReply: Error, Equatable {
        case malformedReply(String)
    }

    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    // MARK: - files.read_dir

    public struct FileEntry: Sendable, Equatable {
        public var name: String
        public var path: String
        public var isDir: Bool
        public var isSymlink: Bool
        public var modifiedAtMs: UInt64?

        public init(name: String, path: String, isDir: Bool, isSymlink: Bool, modifiedAtMs: UInt64?) {
            self.name = name
            self.path = path
            self.isDir = isDir
            self.isSymlink = isSymlink
            self.modifiedAtMs = modifiedAtMs
        }
    }

    public func readDir(projectId: String, path: String) async throws -> [FileEntry] {
        let frame = try await connection.request(
            "files.read_dir",
            params: .object(["projectId": .string(projectId), "path": .string(path)])
        )
        return try Self.decodeDirectory(frame.result)
    }

    static func decodeDirectory(_ result: JSONValue) throws -> [FileEntry] {
        guard case let .array(items)? = result["entries"] else {
            throw MalformedReply.malformedReply("files.read_dir reply missing entries array")
        }
        return try items.map { item in
            guard case let .object(object) = item,
                  case let .string(name)? = object["name"],
                  case let .string(path)? = object["path"],
                  let isDir = object["isDir"]?.boolValue,
                  let isSymlink = object["isSymlink"]?.boolValue
            else { throw MalformedReply.malformedReply("files.read_dir contains an incomplete entry") }
            return FileEntry(
                name: name,
                path: path,
                isDir: isDir,
                isSymlink: isSymlink,
                modifiedAtMs: object["modifiedAtMs"]?.numberText.flatMap(UInt64.init)
            )
        }
    }

    // MARK: - files.read_file

    public func readFile(projectId: String, path: String) async throws -> String {
        let frame = try await connection.request(
            "files.read_file",
            params: .object(["projectId": .string(projectId), "path": .string(path)])
        )
        guard case let .string(content)? = frame.result["content"] else {
            throw MalformedReply.malformedReply("files.read_file reply missing content")
        }
        return content
    }

    // MARK: - files.write_file

    public func writeFile(projectId: String, path: String, content: String) async throws {
        _ = try await connection.request(
            "files.write_file",
            params: .object([
                "projectId": .string(projectId), "path": .string(path), "content": .string(content),
            ])
        )
    }

    // MARK: - files.create_file / files.create_directory

    public func createFile(projectId: String, path: String) async throws {
        _ = try await connection.request(
            "files.create_file",
            params: .object(["projectId": .string(projectId), "path": .string(path)])
        )
    }

    public func createDirectory(projectId: String, path: String) async throws {
        _ = try await connection.request(
            "files.create_directory",
            params: .object(["projectId": .string(projectId), "path": .string(path)])
        )
    }

    // MARK: - files.delete_path（进废纸篓，不默认永久删除）

    public func deletePath(projectId: String, path: String) async throws {
        _ = try await connection.request(
            "files.delete_path",
            params: .object(["projectId": .string(projectId), "path": .string(path)])
        )
    }

    // MARK: - files.rename_path

    public func renamePath(projectId: String, path: String, newName: String) async throws {
        _ = try await connection.request(
            "files.rename_path",
            params: .object([
                "projectId": .string(projectId), "path": .string(path), "newName": .string(newName),
            ])
        )
    }

    // MARK: - files.replace_preview / files.apply_replacements

    public struct ReplacementMatch: Sendable, Equatable {
        public var start: UInt64
        public var end: UInt64
        public var matchText: String

        public init(start: UInt64, end: UInt64, matchText: String) {
            self.start = start
            self.end = end
            self.matchText = matchText
        }
    }

    public struct ReplacePreviewFile: Sendable {
        public var path: String
        public var matches: [ReplacementMatch]

        public init(path: String, matches: [ReplacementMatch]) {
            self.path = path
            self.matches = matches
        }
    }

    public struct ReplacePreview: Sendable {
        public var query: String
        public var replacement: String
        public var files: [ReplacePreviewFile]
    }

    public struct ReplaceSummary: Sendable, Equatable {
        public var filesChanged: Int
        public var replacementsApplied: Int
        public var replacementsSkipped: Int
    }

    /// 替换先 preview（逐文件命中区间，不写盘）。
    public func replacePreview(
        projectId: String, path: String, query: String, replacement: String
    ) async throws -> ReplacePreview {
        let frame = try await connection.request(
            "files.replace_preview",
            params: .object([
                "projectId": .string(projectId), "path": .string(path),
                "query": .string(query), "replacement": .string(replacement),
            ])
        )
        guard case let .array(files)? = frame.result["files"] else {
            throw MalformedReply.malformedReply("files.replace_preview reply missing files array")
        }
        guard case let .string(query)? = frame.result["query"],
              case let .string(replacement)? = frame.result["replacement"]
        else {
            throw MalformedReply.malformedReply("files.replace_preview reply requires string query and replacement")
        }
        let parsed = try files.map { item -> ReplacePreviewFile in
            guard case let .object(object) = item,
                  case let .string(path)? = object["path"],
                  case let .array(matches)? = object["matches"]
            else {
                throw MalformedReply.malformedReply("files.replace_preview file requires path and matches array")
            }
            let intervals = try matches.map { m -> ReplacementMatch in
                guard case let .object(pair) = m,
                      let start = pair["start"]?.numberText.flatMap(UInt64.init),
                      let end = pair["end"]?.numberText.flatMap(UInt64.init),
                      start <= end,
                      case let .string(matchText)? = pair["matchText"]
                else {
                    throw MalformedReply.malformedReply("files.replace_preview match requires ordered UInt64 offsets and matchText")
                }
                return ReplacementMatch(start: start, end: end, matchText: matchText)
            }
            return ReplacePreviewFile(path: path, matches: intervals)
        }
        return ReplacePreview(query: query, replacement: replacement, files: parsed)
    }

    /// 原样提交匹配原文与 UTF-8 字节区间（stale 由宿主拒绝覆盖——skip 不写）。
    public func applyReplacements(
        projectId: String, path: String, replacement: String, files: [ReplacePreviewFile]
    ) async throws -> ReplaceSummary {
        let file = files.first { $0.path == path }
        let matches: [JSONValue] = (file?.matches ?? []).map { pair in
            .object([
                "start": .number(String(pair.start)), "end": .number(String(pair.end)),
                "matchText": .string(pair.matchText),
            ])
        }
        let frame = try await connection.request(
            "files.apply_replacements",
            params: .object([
                "projectId": .string(projectId), "path": .string(path),
                "replacement": .string(replacement), "matches": .array(matches),
            ])
        )
        let filesChanged = frame.result["filesChanged"]?.numberText.flatMap(Int.init) ?? 0
        let applied = frame.result["replacementsApplied"]?.numberText.flatMap(Int.init) ?? 0
        let skipped = frame.result["replacementsSkipped"]?.numberText.flatMap(Int.init) ?? 0
        return ReplaceSummary(filesChanged: filesChanged, replacementsApplied: applied, replacementsSkipped: skipped)
    }
}
