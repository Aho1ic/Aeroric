@testable import AeroricUI
import XCTest

/// B2-3: only the newest history read writes back; the one before is
/// cancelled, and its cancellation is not an error to show.
@MainActor
final class HistoryReadsTests: XCTestCase {
    func testOnlyTheNewestReadLandsAndTheOlderIsCancelled() async {
        var reads = HistoryReads()
        var first: Task<Void, Never>?
        reads.begin { _ in
            let task = Task<Void, Never> { try? await Task.sleep(nanoseconds: 5_000_000_000) }
            first = task
            return task
        }
        XCTAssertTrue(reads.lands(1))
        reads.begin { _ in Task {} }
        XCTAssertFalse(reads.lands(1))
        XCTAssertTrue(reads.lands(2))
        XCTAssertEqual(first?.isCancelled, true)
        XCTAssertFalse(HistoryReads.shows(CancellationError()))
        XCTAssertTrue(HistoryReads.shows(URLError(.timedOut)))
    }
}
