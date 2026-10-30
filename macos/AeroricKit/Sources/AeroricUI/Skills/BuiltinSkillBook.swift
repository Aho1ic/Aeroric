import Foundation
import Observation

/// What the skill book may ask of the host (the App implements it).
@MainActor
public protocol BuiltinSkillActions {
    func list() async throws -> [BuiltinSkillBook.Item]
    func setEnabled(id: String, enabled: Bool) async throws
    func projectsWithCopies() async throws -> [BuiltinSkillBook.ProjectCopies]
    func removeFromProject(path: String) async throws -> Int
}

/// The built-in skills (B2 工作配方库, 2026-10-30) as the App shows them: the
/// 技能 page's switches, the recipe panel, and how a recipe card calls its
/// skill in each harness.
@MainActor
@Observable
public final class BuiltinSkillBook {
    public struct Item: Identifiable, Equatable, Sendable {
        public var id: String
        public var group: String
        public var title: String
        public var detail: String
        public var symbol: String
        public var needs: [String]
        public var enabled: Bool
        /// Harnesses where the user's own skill of the same name wins.
        public var shadowedBy: [String]
        /// Needs not met on this Mac, e.g. `python`.
        public var missing: [String]

        public init(id: String, group: String, title: String, detail: String, symbol: String,
                    needs: [String] = [], enabled: Bool = true, shadowedBy: [String] = [], missing: [String] = []) {
            self.id = id; self.group = group; self.title = title; self.detail = detail; self.symbol = symbol
            self.needs = needs; self.enabled = enabled; self.shadowedBy = shadowedBy; self.missing = missing
        }
    }

    public struct ProjectCopies: Identifiable, Equatable, Sendable {
        public var id: String { path }
        public var path: String
        public var name: String
        public var count: Int
        public init(path: String, name: String, count: Int) { self.path = path; self.name = name; self.count = count }
    }

    public private(set) var items: [Item] = []
    public private(set) var projects: [ProjectCopies] = []
    public private(set) var loaded = false
    public var errorText: String?
    public var actions: (any BuiltinSkillActions)?
    /// Whether the harness's local login runs through the gateway (its
    /// sessions then live in a managed home).
    public var loginOnGateway: (String) -> Bool = { _ in false }

    public init() {}

    public func item(_ id: String) -> Item? { items.first { $0.id == id } }

    public func refresh() async {
        guard let actions else { return }
        do {
            items = try await actions.list()
            projects = try await actions.projectsWithCopies()
            loaded = true
        } catch {
            errorText = "读取内置技能失败：\(error)"
        }
    }

    public func setEnabled(_ id: String, _ enabled: Bool) async {
        guard let actions, let index = items.firstIndex(where: { $0.id == id }) else { return }
        items[index].enabled = enabled
        do {
            try await actions.setEnabled(id: id, enabled: enabled)
        } catch {
            items[index].enabled = !enabled
            errorText = "保存失败：\(error)"
        }
    }

    public func removeFromProject(_ path: String) async -> Int {
        guard let actions else { return 0 }
        do {
            let removed = try await actions.removeFromProject(path: path)
            projects = try await actions.projectsWithCopies()
            return removed
        } catch {
            errorText = "移除失败：\(error)"
            return 0
        }
    }

    /// What a recipe card types to call `skill` (Q13), or nil when the skill
    /// is off or unknown and the card falls back to its plain template.
    public func invocation(skill: String, harness: String, configurationID: String?) -> String? {
        guard let item = item(skill), item.enabled else { return nil }
        let direct = configurationID == nil && !loginOnGateway(harness)
        return Self.invocation(skill: skill, harness: harness, direct: direct,
                               shadowed: item.shadowedBy.contains(harness))
    }

    /// Codex mentions a skill with `$`; a direct Claude Code session has the
    /// skills in the `aeroric` plugin; elsewhere a skill is `/name`.
    nonisolated public static func invocation(skill: String, harness: String, direct: Bool, shadowed: Bool) -> String {
        switch harness {
        case "codex": return "$\(skill)"
        case "claude" where direct && !shadowed: return "/aeroric:\(skill)"
        default: return "/\(skill)"
        }
    }

    /// One line for the 技能 page: off, shadowed, or missing a need.
    nonisolated public static func status(_ item: Item) -> String? {
        if !item.enabled { return nil }
        if !item.shadowedBy.isEmpty {
            let names = item.shadowedBy.map(Self.harnessName).joined(separator: "、")
            return "\(names) 里已被你的同名技能取代"
        }
        if item.missing.contains("python") { return "需要 Python" }
        return nil
    }

    nonisolated static func harnessName(_ harness: String) -> String {
        switch harness {
        case "claude": "Claude Code"
        case "codex": "Codex"
        case "omp": "OMP"
        case "dsh": "DeepSeek Harness"
        default: harness
        }
    }
}
