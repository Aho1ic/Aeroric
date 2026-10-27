import AeroricClient
import Foundation

/// The import picker's state (2026-10-27): every session on disk grouped by
/// its working directory, one tick per session and a tri-state tick per
/// project. Pure, so the grouping and the ticks are unit-tested.
public struct ImportPick: Equatable, Sendable {
    public enum Tick: Equatable, Sendable { case all, some, none }

    public struct Group: Identifiable, Equatable, Sendable {
        /// The working directory, or "" for sessions that recorded none.
        public var id: String
        public var title: String
        public var path: String?
        /// No project has this directory yet; importing adds one.
        public var isNew: Bool
        public var sessions: [ImportsClient.ImportedSession]
    }

    public private(set) var rows: [ImportsClient.ImportedSession]
    public var selected: Set<String>
    public var query = ""

    public init(rows: [ImportsClient.PreviewSession]) {
        self.rows = rows.map(\.session)
        selected = Set(rows.filter(\.selected).map(\.id))
    }

    /// All groups, newest activity first; the no-directory group goes last.
    public var groups: [Group] {
        let byDir = Dictionary(grouping: rows) { $0.cwd ?? "" }
        return byDir.map { dir, sessions in
            Group(id: dir,
                  title: dir.isEmpty ? "未记录目录" : URL(fileURLWithPath: dir).lastPathComponent,
                  path: dir.isEmpty ? nil : dir,
                  isNew: !dir.isEmpty && sessions.allSatisfy { $0.projectId == nil },
                  sessions: sessions.sorted { ($0.lastAtMs, $0.id) > ($1.lastAtMs, $1.id) })
        }
        .sorted { lhs, rhs in
            if lhs.id.isEmpty != rhs.id.isEmpty { return rhs.id.isEmpty }
            let l = lhs.sessions.first?.lastAtMs ?? 0, r = rhs.sessions.first?.lastAtMs ?? 0
            return l != r ? l > r : lhs.id < rhs.id
        }
    }

    /// Groups narrowed by `query` (project name, path or session title);
    /// a project whose name matches keeps all its sessions.
    public var visibleGroups: [Group] {
        let needle = query.trimmingCharacters(in: .whitespaces)
        guard !needle.isEmpty else { return groups }
        return groups.compactMap { group in
            if AgentConfigText.fuzzyMatch(needle, group.title) { return group }
            var narrowed = group
            narrowed.sessions = group.sessions.filter { AgentConfigText.fuzzyMatch(needle, $0.title) }
            return narrowed.sessions.isEmpty ? nil : narrowed
        }
    }

    public func tick(_ group: Group) -> Tick {
        let count = group.sessions.filter { selected.contains($0.id) }.count
        return count == 0 ? .none : count == group.sessions.count ? .all : .some
    }

    /// A ticked (or partly ticked) project clears; an empty one fills.
    public mutating func toggle(group: Group) {
        let ids = group.sessions.map(\.id)
        if tick(group) == .none { selected.formUnion(ids) } else { selected.subtract(ids) }
    }

    public mutating func toggle(_ id: String) {
        if selected.contains(id) { selected.remove(id) } else { selected.insert(id) }
    }

    public mutating func selectAll() { selected.formUnion(visibleGroups.flatMap { $0.sessions.map(\.id) }) }
    public mutating func selectNone() { selected.subtract(visibleGroups.flatMap { $0.sessions.map(\.id) }) }

    public var selectedCount: Int { rows.filter { selected.contains($0.id) }.count }

    /// Projects importing would add: new directories with at least one tick.
    public var newProjectCount: Int {
        groups.filter { $0.isNew && tick($0) != .none }.count
    }
}
