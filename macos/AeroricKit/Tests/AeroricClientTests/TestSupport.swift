@testable import AeroricClient
import os
import XCTest

/// Minimal lock-backed box for test state touched from multiple executors.
/// NSLock.lock() is unavailable from async contexts on this SDK, so test
/// bodies use [`os.OSAllocatedUnfairLock`] instead.
final class Locked<T: Sendable>: @unchecked Sendable {
    private let state: OSAllocatedUnfairLock<T>

    init(_ initial: T) {
        state = OSAllocatedUnfairLock(initialState: initial)
    }

    var value: T {
        state.withLock { $0 }
    }

    func mutate(_ body: @Sendable (inout T) -> Void) {
        state.withLock { body(&$0) }
    }
}
