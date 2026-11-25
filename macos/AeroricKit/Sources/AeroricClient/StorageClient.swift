import AeroricProtocol
import Foundation

/// 2026-11-24 设置 › 存储: what Aeroric's data takes on disk, by kind
/// (`storage.usage`, read only).
public struct StorageUsage: Sendable, Equatable {
    public struct Entry: Sendable, Equatable, Identifiable {
        public var path: String
        public var bytes: Int64
        public var id: String { path }

        public init(path: String, bytes: Int64) {
            self.path = path
            self.bytes = bytes
        }
    }

    public struct Group: Sendable, Equatable, Identifiable {
        /// sessions | memory | references | worktrees | logs | remotePackages | other
        public var id: String
        public var title: String
        public var bytes: Int64
        /// Largest first.
        public var entries: [Entry]

        public init(id: String, title: String, bytes: Int64, entries: [Entry]) {
            self.id = id
            self.title = title
            self.bytes = bytes
            self.entries = entries
        }
    }

    public var dataDir: String
    public var totalBytes: Int64
    public var groups: [Group]
    /// Files or folders the host could not read; not counted.
    public var unreadable: Int

    public init(dataDir: String, totalBytes: Int64, groups: [Group], unreadable: Int) {
        self.dataDir = dataDir
        self.totalBytes = totalBytes
        self.groups = groups
        self.unreadable = unreadable
    }

    /// Reads the host's answer; nil when it is not one.
    public static func decode(_ value: JSONValue) -> StorageUsage? {
        guard let object = value.objectValue,
              let dataDir = object["dataDir"]?.stringValue,
              let groups = object["groups"]?.arrayValue else { return nil }
        func int(_ value: JSONValue?) -> Int64 { value?.numberText.flatMap { Int64($0) } ?? 0 }
        return StorageUsage(
            dataDir: dataDir,
            totalBytes: int(object["totalBytes"]),
            groups: groups.compactMap { group in
                guard let id = group["id"]?.stringValue else { return nil }
                return Group(
                    id: id,
                    title: group["title"]?.stringValue ?? id,
                    bytes: int(group["bytes"]),
                    entries: (group["entries"]?.arrayValue ?? []).compactMap { entry in
                        guard let path = entry["path"]?.stringValue else { return nil }
                        return Entry(path: path, bytes: int(entry["bytes"]))
                    })
            },
            unreadable: Int(int(object["unreadable"])))
    }
}

public struct StorageClient: Sendable {
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    public func usage() async throws -> StorageUsage {
        let frame = try await connection.request("storage.usage", params: .object([:]))
        guard let usage = StorageUsage.decode(frame.result) else {
            throw StorageClientError.unreadableAnswer
        }
        return usage
    }
}

public enum StorageClientError: Error, LocalizedError, Equatable {
    case unreadableAnswer

    public var errorDescription: String? {
        switch self {
        case .unreadableAnswer: "宿主的回答读不懂"
        }
    }
}
