import Foundation
import Observation

/// Environment identity for the shell (P4-04): which environment tag this
/// launch carries and where its data directory lives. Populated from the
/// injected configuration — never probed.
@MainActor
@Observable
final class ApplicationStore {
    let environmentTag: String
    let dataDirPath: String?
    let injectedExplicitly: Bool

    init(environmentTag: String, dataDirPath: String?, injectedExplicitly: Bool) {
        self.environmentTag = environmentTag
        self.dataDirPath = dataDirPath
        self.injectedExplicitly = injectedExplicitly
    }
}

/// One opened project, as shown in the sidebar (P4-04). The host owns the
/// registry; this store only mirrors what the app opened this session.
@MainActor
@Observable
final class ProjectStore {
    struct ProjectEntry: Identifiable, Equatable, Hashable {
        let id: String
        let path: String
        /// 更多 menu fields (2026-10-28), mirrored from the host record.
        var name: String? = nil
        var color: String? = nil
        var defaultAgent: String? = nil
        var group: String? = nil
    }

    private(set) var projects: [ProjectEntry] = []

    /// Upsert keyed by id (ADR-013: ids are minted by the request sender;
    /// the host confirms them, the store only records). Re-opening the same
    /// path under the same id refreshes in place; a plain open keeps the
    /// display fields the last list brought.
    @discardableResult
    func upsertProject(id: String, path: String) -> ProjectEntry {
        var entry = projects.first { $0.id == id } ?? ProjectEntry(id: id, path: path)
        entry = ProjectEntry(id: id, path: path, name: entry.name, color: entry.color,
                             defaultAgent: entry.defaultAgent, group: entry.group)
        return upsert(entry)
    }

    @discardableResult
    func upsert(_ entry: ProjectEntry) -> ProjectEntry {
        if let index = projects.firstIndex(where: { $0.id == entry.id }) {
            projects[index] = entry
        } else {
            projects.append(entry)
        }
        return entry
    }

    /// Keep only the projects the host still lists.
    func retain(ids: Set<String>) {
        projects.removeAll { !ids.contains($0.id) }
    }
}
