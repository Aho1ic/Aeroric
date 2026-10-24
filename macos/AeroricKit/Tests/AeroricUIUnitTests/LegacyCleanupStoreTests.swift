import AeroricClient
import AeroricProtocol
import XCTest

@testable import AeroricUI

private actor FakeCleanup: LegacyCleanupServing {
    var planReplies: [Result<LegacyCleanupReport, Error>]
    var applyReply: Result<LegacyCleanupReport, Error>
    private(set) var applied: [[String]] = []
    private(set) var planCalls = 0

    init(plan: [Result<LegacyCleanupReport, Error>], apply: Result<LegacyCleanupReport, Error>) {
        planReplies = plan
        applyReply = apply
    }

    func plan() async throws -> LegacyCleanupReport {
        planCalls += 1
        return try (planReplies.count > 1 ? planReplies.removeFirst() : planReplies[0]).get()
    }

    func apply(sourceIDs: [String]) async throws -> LegacyCleanupReport {
        applied.append(sourceIDs)
        return try applyReply.get()
    }
}

private struct Boom: Error {}

private func report(_ states: [(String, LegacyCleanupState)], batch: String? = nil) -> LegacyCleanupReport {
    LegacyCleanupReport(
        candidates: states.map { LegacyCleanupCandidate(source: "custom:\($0.0)", state: $0.1) },
        quarantineBatch: batch
    )
}

private let initial = report([
    ("a", .eligible),
    ("b", .blocked(reason: "fingerprint mismatch")),
    ("c", .partial(remaining: [.deleteOldSecrets], reason: "DeleteOldSecrets: keychain locked")),
    ("d", .done),
])

@MainActor
final class LegacyCleanupStoreTests: XCTestCase {
    func testNothingRunsWithoutAConnectionOrTheCapability() async {
        let store = LegacyCleanupStore()
        XCTAssertEqual(store.availability, .notConnected)
        XCTAssertNotNil(store.unavailableReason)
        await store.reload()
        XCTAssertNil(store.report)

        let fake = FakeCleanup(plan: [.success(initial)], apply: .success(initial))
        store.attach(client: fake, capabilityEnabled: false)
        guard case .unsupported = store.availability else { return XCTFail("\(store.availability)") }
        await store.reload()
        let calls = await fake.planCalls
        XCTAssertEqual(calls, 0, "an unadvertised capability is never probed")
    }

    func testSelectionAcceptsOnlyActionableEntries() async {
        let store = LegacyCleanupStore()
        store.attach(client: FakeCleanup(plan: [.success(initial)], apply: .success(initial)), capabilityEnabled: true)
        await store.reload()
        for source in ["custom:a", "custom:b", "custom:c", "custom:d", "custom:missing"] {
            store.toggle(source)
        }
        XCTAssertEqual(store.selection, ["custom:a", "custom:c"])
        store.toggle("custom:a")
        XCTAssertEqual(store.selection, ["custom:c"])
    }

    func testApplyNeedsTheLaunchConfirmationAndSendsListOrder() async {
        let afterApply = report([("a", .done), ("b", .blocked(reason: "x")), ("c", .done), ("d", .done)], batch: "20261024T1-0")
        let afterReload = report([("b", .blocked(reason: "x")), ("a", .done), ("c", .done), ("d", .done)])
        let fake = FakeCleanup(plan: [.success(initial), .success(afterReload)], apply: .success(afterApply))
        let store = LegacyCleanupStore()
        store.attach(client: fake, capabilityEnabled: true)
        await store.reload()
        store.toggle("custom:c")
        store.toggle("custom:a")
        XCTAssertFalse(store.canApply, "Q5 confirmation is required")
        await store.apply()
        var applied = await fake.applied
        XCTAssertTrue(applied.isEmpty)

        store.launchConfirmed = true
        XCTAssertTrue(store.canApply)
        await store.apply()
        applied = await fake.applied
        XCTAssertEqual(applied, [["custom:a", "custom:c"]], "list order, exactly the selection")
        XCTAssertEqual(store.lastBatch, "20261024T1-0")
        XCTAssertEqual(store.report, afterReload, "reloaded after apply (D4)")
        XCTAssertTrue(store.selection.isEmpty)
        XCTAssertFalse(store.launchConfirmed, "confirmation is per apply")
        XCTAssertNil(store.lastError)
    }

    func testFailuresAreRememberedWithoutClearingTheList() async {
        let fake = FakeCleanup(plan: [.success(initial), .failure(Boom())], apply: .failure(Boom()))
        let store = LegacyCleanupStore()
        store.attach(client: fake, capabilityEnabled: true)
        await store.reload()
        store.toggle("custom:a")
        store.launchConfirmed = true
        await store.apply()
        XCTAssertNotNil(store.lastError)
        XCTAssertEqual(store.report, initial)
        XCTAssertEqual(store.selection, ["custom:a"], "a failed apply keeps the user's selection")
        XCTAssertFalse(store.isBusy)

        await store.reload()
        XCTAssertNotNil(store.lastError)
        XCTAssertEqual(store.report, initial, "a failed refresh is not an empty list")
    }

    func testReattachResetsEverything() async {
        let store = LegacyCleanupStore()
        store.attach(client: FakeCleanup(plan: [.success(initial)], apply: .success(initial)), capabilityEnabled: true)
        await store.reload()
        store.toggle("custom:a")
        store.launchConfirmed = true
        store.attach(client: nil, capabilityEnabled: true)
        XCTAssertEqual(store.availability, .notConnected)
        XCTAssertNil(store.report)
        XCTAssertTrue(store.selection.isEmpty)
        XCTAssertFalse(store.launchConfirmed)
    }
}
