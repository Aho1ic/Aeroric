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
    }

    private(set) var projects: [ProjectEntry] = []

    /// Upsert keyed by id (ADR-013: ids are minted by the request sender;
    /// the host confirms them, the store only records). Re-opening the same
    /// path under the same id refreshes in place.
    @discardableResult
    func upsertProject(id: String, path: String) -> ProjectEntry {
        let entry = ProjectEntry(id: id, path: path)
        if let index = projects.firstIndex(where: { $0.id == id }) {
            projects[index] = entry
        } else {
            projects.append(entry)
        }
        return entry
    }
}
