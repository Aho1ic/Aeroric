import Foundation
import Observation

/// Local presentation guard, NOT a host lease or approval. Leaving a task
/// makes subsequent returns read-only until a future authoritative control
/// acquisition flow is wired. Mode/appearance changes never grant input.
@MainActor @Observable
public final class TaskPresentationInputGate {
    public private(set) var visibleTaskID: String?
    private var relinquished: Set<String> = []
    public init() {}
    public func update(taskID: String?, visible: Bool) {
        if let old = visibleTaskID, old != taskID || !visible { relinquished.insert(old) }
        visibleTaskID = visible ? taskID : nil
    }
    public func leave() { update(taskID: visibleTaskID, visible: false) }
    public func permits(_ taskID: String) -> Bool {
        visibleTaskID == taskID && !relinquished.contains(taskID)
    }
}
