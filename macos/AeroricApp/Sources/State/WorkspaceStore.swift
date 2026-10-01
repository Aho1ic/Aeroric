import Foundation
import Observation

/// Per-window workspace state. P4-02 only establishes the store and its
/// identity fields; the real shell scenes in P4-04 write `windowID` on
/// appear and attach selection/draft/layout semantics — the fields are
/// live bindings, not placeholders.
@MainActor
@Observable
final class WorkspaceStore {
    var windowID = UUID()
    var selection: String?
    var draft: String?
}
