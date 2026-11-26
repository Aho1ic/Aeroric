import Foundation
import XCTest

@testable import AeroricClient

/// 2026-11-25: a terminate signal reaches the handler instead of ending the
/// process. Tested with SIGUSR2 so the test runner is never at risk.
final class TerminationSignalsTests: XCTestCase {
    func testTheSignalReachesTheHandlerAndStopRestoresTheDefault() {
        let reached = expectation(description: "handled")
        let queue = DispatchQueue(label: "termination-signals-test")
        let signals = TerminationSignals(signals: [SIGUSR2], queue: queue) { number in
            XCTAssertEqual(number, SIGUSR2)
            reached.fulfill()
        }
        signals.start()
        signals.start()  // a second start adds nothing
        kill(getpid(), SIGUSR2)
        wait(for: [reached], timeout: 5)

        signals.stop()
        var action = sigaction()
        sigaction(SIGUSR2, nil, &action)
        XCTAssertEqual(unsafeBitCast(action.__sigaction_u.__sa_handler, to: Int.self),
                       unsafeBitCast(SIG_DFL, to: Int.self), "default again after stop")
    }
}
