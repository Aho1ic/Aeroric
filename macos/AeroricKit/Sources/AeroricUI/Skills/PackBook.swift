import AeroricClient
import AeroricProtocol
import Foundation
import Observation

/// Agent 配置 → 能力包 (H6, 2026-11-21): the capability packs, what each
/// harness gets from them, and the user's own MCP servers.
@MainActor @Observable
public final class PackBook {
    public struct Calls: Sendable {
        public var list: @Sendable () async throws -> LivePackOverview
        public var saveServer: @Sendable (LivePackServer) async throws -> Void
        public var importFolder: @Sendable (String) async throws -> Void
        public var setEnabled: @Sendable (String, Bool) async throws -> Void
        public var setConfirmed: @Sendable (String, Bool) async throws -> Void
        public var remove: @Sendable (String) async throws -> Void

        public init(list: @escaping @Sendable () async throws -> LivePackOverview,
                    saveServer: @escaping @Sendable (LivePackServer) async throws -> Void,
                    importFolder: @escaping @Sendable (String) async throws -> Void,
                    setEnabled: @escaping @Sendable (String, Bool) async throws -> Void,
                    setConfirmed: @escaping @Sendable (String, Bool) async throws -> Void,
                    remove: @escaping @Sendable (String) async throws -> Void) {
            self.list = list; self.saveServer = saveServer; self.importFolder = importFolder
            self.setEnabled = setEnabled; self.setConfirmed = setConfirmed; self.remove = remove
        }
    }

    public private(set) var overview = LivePackOverview()
    public private(set) var loaded = false
    public var errorText: String?
    @ObservationIgnored private var calls: Calls?

    public init() {}

    public func attach(client: PacksClient?) {
        guard let client else {
            attach(nil)
            return
        }
        attach(Calls(list: { try await client.list() },
                     saveServer: { try await client.saveServer($0) },
                     importFolder: { try await client.importFolder($0) },
                     setEnabled: { try await client.setEnabled(id: $0, enabled: $1) },
                     setConfirmed: { try await client.setConfirmed(id: $0, confirmed: $1) },
                     remove: { try await client.remove(id: $0) }))
    }

    public func attach(_ calls: Calls?) {
        self.calls = calls
        overview = LivePackOverview()
        loaded = false
        errorText = nil
    }

    public func refresh() async {
        guard let calls else { return }
        do {
            overview = try await calls.list()
            loaded = true
        } catch {
            errorText = "读取能力包失败：\(error)"
        }
    }

    /// Runs `change`, then reads the packs again; false (with `errorText`) on failure.
    @discardableResult
    private func change(_ what: String, _ run: (Calls) async throws -> Void) async -> Bool {
        guard let calls else { return false }
        do {
            try await run(calls)
            errorText = nil
            await refresh()
            return true
        } catch {
            errorText = "\(what)失败：\(error)"
            return false
        }
    }

    @discardableResult
    public func save(_ server: LivePackServer) async -> Bool { await change("保存") { try await $0.saveServer(server) } }
    @discardableResult
    public func importFolder(_ path: String) async -> Bool { await change("导入") { try await $0.importFolder(path) } }
    public func setEnabled(_ id: String, _ on: Bool) async { await change("保存") { try await $0.setEnabled(id, on) } }
    public func setConfirmed(_ id: String, _ on: Bool) async { await change("保存") { try await $0.setConfirmed(id, on) } }
    public func remove(_ id: String) async { await change("删除") { try await $0.remove(id) } }
}

/// The page's words.
public enum PackText {
    public static func kind(_ kind: String) -> String {
        switch kind {
        case "skill": "技能"
        case "mcpServer": "MCP 服务器"
        case "role": "角色"
        case "command": "命令"
        case "hook": "事件处理"
        default: kind
        }
    }

    /// "1 个 MCP 服务器 · 2 个技能".
    public static func contents(_ pack: LivePack) -> String {
        let parts: [(Int, String)] = [(pack.servers.count, " MCP 服务器"), (pack.skills.count, "技能"),
                                      (pack.commands.count, "命令"), (pack.roles.count, "角色"),
                                      (pack.hooks.count, "事件处理")]
        // "1 个 MCP 服务器": a space before Latin letters, none before Chinese.
        let text = parts.filter { $0.0 > 0 }.map { "\($0.0) 个\($0.1)" }.joined(separator: " · ")
        return text.isEmpty ? "空包" : text
    }

    public static func source(_ pack: LivePack) -> String {
        if pack.isOwnServer { return "你在 Aeroric 里添加的" }
        switch pack.sourceKind {
        case "builtin": return "Aeroric 内置"
        case "official-marketplace": return "官方市场"
        default:
            let from = pack.sourceOrigin.isEmpty ? "本机" : pack.sourceOrigin
            return "从 \(from) 导入" + (pack.revision.map { " · 版本 \($0)" } ?? "")
        }
    }

    /// "OMP：2/3 项可用".
    public static func place(_ place: LivePackPlace) -> String {
        let ready = place.items.filter(\.ready).count
        return "\(place.place)：\(ready)/\(place.items.count) 项可用"
    }

    /// "MCP 服务器 github：已写进…".
    public static func item(_ item: LivePackItem) -> String {
        "\(kind(item.kind)) \(item.name)：\(item.basis)"
    }
}

/// The add-server form.
public struct PackServerDraft: Equatable {
    public var name = ""
    public var description = ""
    public var command = ""
    /// Space-separated.
    public var args = ""
    /// One `KEY=VALUE` per line.
    public var env = ""
    /// Editing an existing server: the name is fixed.
    public var editing = false

    public init() {}

    public init(_ server: LivePackServer) {
        name = server.name
        description = server.description
        command = server.command
        args = server.args.joined(separator: " ")
        env = server.env.keys.sorted().map { "\($0)=\(server.env[$0] ?? "")" }.joined(separator: "\n")
        editing = true
    }

    public var ready: Bool {
        !name.trimmingCharacters(in: .whitespaces).isEmpty && !command.trimmingCharacters(in: .whitespaces).isEmpty
    }

    public var server: LivePackServer {
        var pairs: [String: String] = [:]
        for line in env.split(whereSeparator: \.isNewline) {
            guard let at = line.firstIndex(of: "=") else { continue }
            let key = line[..<at].trimmingCharacters(in: .whitespaces)
            if !key.isEmpty { pairs[key] = String(line[line.index(after: at)...]) }
        }
        return LivePackServer(name: name.trimmingCharacters(in: .whitespaces),
                              description: description.trimmingCharacters(in: .whitespacesAndNewlines),
                              command: command.trimmingCharacters(in: .whitespaces),
                              args: args.split(separator: " ").map(String.init), env: pairs)
    }
}
