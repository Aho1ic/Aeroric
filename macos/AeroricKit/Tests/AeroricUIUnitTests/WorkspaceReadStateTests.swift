import AeroricClient
import XCTest
@testable import AeroricUI

/// B2-1: a read the view cancelled (it started a newer one) is not a failure
/// that blocks the new-session page.
final class WorkspaceReadStateTests: XCTestCase {
    func testACancelledReadLeavesWhatWasThere() {
        XCTAssertEqual(WorkspaceReadState.after(CancellationError(), hasEarlierRead: true, cancelled: false), .loaded)
        XCTAssertEqual(WorkspaceReadState.after(CancellationError(), hasEarlierRead: false, cancelled: false), .notLoaded)
        XCTAssertEqual(WorkspaceReadState.after(ConnectionError.cancelled, hasEarlierRead: true, cancelled: false), .loaded)
        XCTAssertEqual(WorkspaceReadState.after(RequestResolution.cancelled, hasEarlierRead: false, cancelled: false), .notLoaded)
        // The task itself was cancelled, whatever the error says.
        XCTAssertEqual(WorkspaceReadState.after(ConnectionError.disconnected, hasEarlierRead: true, cancelled: true), .loaded)
    }

    func testARealFailureIsStillAFailure() {
        guard case .failed = WorkspaceReadState.after(ConnectionError.disconnected, hasEarlierRead: true, cancelled: false) else {
            return XCTFail("a lost connection is a failure")
        }
    }
}
