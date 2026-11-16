import Foundation
import Observation

/// Q-R4 (2026-11-13): text another pane hands to a session's composer, such
/// as a PDF excerpt. The session page owns its draft; it takes what waits
/// here and appends it, never sends it. One per app, keyed by task id.
@MainActor @Observable
public final class ComposerInbox {
    private var pending: [String: [String]] = [:]

    public init() {}

    public func post(_ text: String, to taskID: String) {
        let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        pending[taskID, default: []].append(text)
    }

    public func count(for taskID: String) -> Int { pending[taskID]?.count ?? 0 }

    /// Everything waiting for `taskID`, oldest first; the inbox forgets it.
    public func take(_ taskID: String) -> [String] {
        pending.removeValue(forKey: taskID) ?? []
    }

    /// `draft` with `pieces` after it, a blank line between each.
    public static func appending(_ pieces: [String], to draft: String) -> String {
        let head = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        return ([head].filter { !$0.isEmpty } + pieces).joined(separator: "\n\n")
    }
}
