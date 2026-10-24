#if DEBUG
import Foundation
import XCTest
@testable import AeroricUI

/// Pure entry, fixture and Store coverage only. This suite never constructs a
/// View/window, touches NSApplication.shared, launches the App, or reads a
/// real home.
final class NativeTaskDeliverablesReviewTests: XCTestCase {
    private let otherReviewKeys = [
        "AERORIC_NATIVE_TASK_COMMUNICATION_REVIEW",
        "AERORIC_NATIVE_WORK_ARRANGEMENTS_REVIEW",
        "AERORIC_NATIVE_SKILL_REVIEW",
        "AERORIC_NATIVE_KNOWLEDGE_REVIEW",
        "AERORIC_NATIVE_EVOLUTION_REVIEW",
        "AERORIC_NATIVE_VISUAL_REVIEW",
        "AERORIC_NATIVE_COLLABORATION_REVIEW",
    ]

    func testAbsentFlagLeavesAll127OtherReviewCombinationsInactive() throws {
        try withEntryFixture { fixture in
            for mask in 0..<128 {
                var environment = fixture.environment
                environment.removeValue(forKey: NativeTaskDeliverablesReviewEntry.flagKey)
                for (index, key) in otherReviewKeys.enumerated() {
                    environment[key] = mask & (1 << index) == 0 ? "0" : "1"
                }
                XCTAssertEqual(
                    decision(environment, fixture),
                    .inactive,
                    "mask \(mask)"
                )
            }
        }
    }

    func testValidEntryAndAll127ConflictsAreExclusive() throws {
        try withEntryFixture { fixture in
            XCTAssertEqual(decision(fixture.environment, fixture), .review)
            for mask in 1..<128 {
                var environment = fixture.environment
                for (index, key) in otherReviewKeys.enumerated() {
                    environment[key] = mask & (1 << index) == 0 ? "0" : "1"
                }
                XCTAssertEqual(
                    decision(environment, fixture),
                    .refused([.conflictingReview]),
                    "mask \(mask)"
                )
            }
        }
    }

    func testPresentInvalidRequestRefusesWithoutProductFallback() throws {
        try withEntryFixture { fixture in
            for value in ["0", "", "true", "2", "01", " 1", "1\n"] {
                var environment = fixture.environment
                environment[NativeTaskDeliverablesReviewEntry.flagKey] = value
                XCTAssertEqual(
                    decision(environment, fixture),
                    .refused([.invalidFlag])
                )
            }

            var invalid = fixture.environment
            invalid["AERORIC_ENV"] = "default"
            invalid["AERORIC_DATA_DIR"] = fixture.home.appendingPathComponent("private").path
            XCTAssertEqual(
                decision(invalid, fixture),
                .refused([.invalidEnvironment, .invalidDirectory])
            )
        }
    }

    func testDirectoryMetadataIsReadOnlyAndRejectsSymlinkOrNonDirectory() throws {
        try withEntryFixture { fixture in
            let fileManager = FileManager.default
            let existing = fixture.caches.appendingPathComponent("existing", isDirectory: true)
            try fileManager.createDirectory(at: existing, withIntermediateDirectories: false)
            let file = fixture.caches.appendingPathComponent("file")
            XCTAssertTrue(fileManager.createFile(atPath: file.path, contents: Data("x".utf8)))
            let link = fixture.caches.appendingPathComponent("link")
            try fileManager.createSymbolicLink(at: link, withDestinationURL: existing)
            let before = try fileManager.contentsOfDirectory(atPath: fixture.caches.path).sorted()

            var existingEnvironment = fixture.environment
            existingEnvironment["AERORIC_DATA_DIR"] = existing.path
            XCTAssertEqual(decision(existingEnvironment, fixture), .review)

            for path in [
                fixture.caches.path,
                file.path,
                link.path,
                fixture.caches.path + "/../outside",
                fixture.caches.path + "-collision/child",
            ] {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = path
                XCTAssertEqual(
                    decision(environment, fixture),
                    .refused([.invalidDirectory]),
                    path
                )
            }

            XCTAssertFalse(fileManager.fileExists(atPath: fixture.candidate.path))
            XCTAssertEqual(
                try fileManager.contentsOfDirectory(atPath: fixture.caches.path).sorted(),
                before
            )
        }
    }

    @MainActor
    func testLifecycleMetadataIsExclusiveAndDoesNotCreateWindow() async {
        let review = NativeTaskDeliverablesReviewWindowLifecycle(decision: .review)
        let refusal = NativeTaskDeliverablesReviewWindowLifecycle(
            decision: .refused([.invalidFlag]))
        let inactive = NativeTaskDeliverablesReviewWindowLifecycle(decision: .inactive)

        XCTAssertEqual(review.windowIdentifier?.rawValue, "native-task-deliverables-review")
        XCTAssertEqual(
            refusal.windowIdentifier?.rawValue,
            "native-task-deliverables-review-refusal"
        )
        XCTAssertNil(inactive.windowIdentifier)
        XCTAssertFalse(review.isWindowCreated)
        XCTAssertFalse(refusal.isWindowCreated)
        XCTAssertFalse(inactive.isWindowCreated)
        XCTAssertEqual(inactive.windowTitle, "")
    }

    func testFixturesCoverEightFiniteRelationshipsAndPreserveTemporaryRecord() {
        XCTAssertEqual(TaskDeliverablesReviewScenario.allCases.count, 8)
        for scenario in TaskDeliverablesReviewScenario.allCases {
            let fixture = TaskDeliverablesReviewFixtures.make(scenario)
            XCTAssertFalse(fixture.deliverables.isEmpty)
            XCTAssertFalse(scenario.instruction.isEmpty)
            XCTAssertNotNil(
                fixture.deliverables.first(where: { $0.id == .temporaryLog })
            )
        }
        let api = TaskDeliverablesReviewFixtures.make(.versions)
            .deliverables.first { $0.id == .apiDocument }
        XCTAssertEqual(api?.versions.map(\.id), ["api-v1", "api-v2"])
        XCTAssertEqual(
            api?.versions.first?.checks.first?.result,
            .passed
        )
        XCTAssertEqual(
            api?.versions.last?.checks.last?.result,
            .pending
        )
    }

    @MainActor
    func testTD1OrganizationRemovalAndCancelPreserveOriginalFacts() async throws {
        let store = TaskDeliverablesReviewStore(scenario: .collection)
        let originalTemporary = try XCTUnwrap(store.deliverable(.temporaryLog))
        let originalAPI = try XCTUnwrap(store.deliverable(.apiDocument))

        XCTAssertEqual(originalTemporary.collectionState, .temporary)
        XCTAssertTrue(store.includeExampleTemporaryRecord())
        XCTAssertEqual(store.deliverable(.temporaryLog)?.collectionState, .submitted)
        XCTAssertTrue(store.removeFromCollection(.apiDocument))
        XCTAssertEqual(store.deliverable(.apiDocument)?.collectionState, .hidden)
        XCTAssertEqual(store.deliverable(.apiDocument)?.versions, originalAPI.versions)

        XCTAssertTrue(store.selectDeliverable(.validationDiff))
        let beforeEdit = try XCTUnwrap(store.deliverable(.validationDiff))
        XCTAssertTrue(store.beginOrganizationEdit(.validationDiff))
        store.setOrganizationTitleDraft("不应应用的标题")
        store.cancelOrganizationEdit()
        XCTAssertEqual(store.deliverable(.validationDiff), beforeEdit)

        XCTAssertTrue(store.beginOrganizationEdit(.validationDiff))
        store.setOrganizationTitleDraft("整理后的差异标题")
        store.setOrganizationDescriptionDraft("整理后的展示说明")
        XCTAssertTrue(store.commitOrganizationEdit())
        XCTAssertEqual(store.deliverable(.validationDiff)?.title, "整理后的差异标题")
        XCTAssertEqual(store.deliverable(.validationDiff)?.versions, beforeEdit.versions)
        XCTAssertEqual(originalTemporary.versions, store.deliverable(.temporaryLog)?.versions)
    }

    @MainActor
    func testTD2VersionSelectionDoesNotOverwriteOldContentOrChecks() async throws {
        let store = TaskDeliverablesReviewStore(scenario: .versions)
        let v1 = try XCTUnwrap(store.version("api-v1"))
        let v2 = try XCTUnwrap(store.version("api-v2"))
        XCTAssertTrue(store.selectVersion("api-v2"))
        XCTAssertEqual(store.selectedVersion?.id, v2.id)
        XCTAssertNotEqual(v1.content, v2.content)
        XCTAssertEqual(store.version("api-v1"), v1)
        XCTAssertEqual(v1.checks.first?.result, .passed)
        XCTAssertEqual(v2.checks.last?.result, .pending)
        XCTAssertTrue(store.selectedDeliverable?.currentWorkFile?.note.contains("另看") == true)
        XCTAssertEqual(v2.readback, .embedded)
    }

    @MainActor
    func testTD3ReadingIsPassiveAndOperationIntentsNeverExecute() async {
        let store = TaskDeliverablesReviewStore(scenario: .reading)
        let before = store.selectedVersion
        XCTAssertTrue(store.recordOperation(.run))
        XCTAssertTrue(store.recordOperation(.install))
        XCTAssertTrue(store.recordOperation(.externalOpen))
        XCTAssertEqual(store.operations.count, 3)
        XCTAssertTrue(store.operations.allSatisfy { $0.state.contains("未执行") })
        XCTAssertEqual(store.selectedVersion, before)
        XCTAssertEqual(store.selectedVersion?.readback, .unavailable)
        XCTAssertTrue(store.selectedVersion?.content == nil)
    }

    @MainActor
    func testTD4AndTD5AnchorDraftAndRecipientWithoutSilentRebinding() async throws {
        let store = TaskDeliverablesReviewStore(scenario: .feedback)
        XCTAssertTrue(store.selectDeliverable(.apiDocument))
        XCTAssertTrue(store.selectVersion("api-v1"))
        store.beginFeedback(location: .known("文档 §1 · 字段形状"))
        let anchoredDraft = store.draft
        XCTAssertTrue(store.selectVersion("api-v2"))
        XCTAssertEqual(store.draft, anchoredDraft)
        XCTAssertFalse(store.canBeginFeedback)
        store.setDraftLocationChoice(.known)
        XCTAssertEqual(store.draft.location, anchoredDraft.location)

        store.setDraftText(" ")
        XCTAssertFalse(store.sendFeedback())
        XCTAssertTrue(store.feedbacks.isEmpty)
        store.cancelFeedback()
        XCTAssertFalse(store.canSendDraft)
        XCTAssertFalse(store.sendFeedback())
        XCTAssertTrue(store.feedbacks.isEmpty)
        XCTAssertTrue(store.selectVersion("api-v1"))
        store.beginFeedback(location: .known("文档 §1 · 字段形状"))
        store.setDraftText("请保留 v1 依据并补充空输入限制。")
        store.setDraftRecipient(.apiMember)
        XCTAssertTrue(store.sendFeedback())
        let sent = try XCTUnwrap(store.feedbacks.first)
        XCTAssertEqual(sent.versionID, "api-v1")
        XCTAssertEqual(sent.location, .known("文档 §1 · 字段形状"))
        XCTAssertEqual(sent.recipient, .apiMember)

        XCTAssertTrue(store.beginCorrection(sent.id))
        store.setDraftText("更正：只补充该段，不改变其他意见。")
        XCTAssertTrue(store.sendFeedback())
        let correction = try XCTUnwrap(store.feedbacks.last)
        XCTAssertEqual(correction.correctionOf, sent.id)
        XCTAssertEqual(store.feedback(id: sent.id), sent)
    }

    @MainActor
    func testTD4UnavailableLocationCannotBecomeReliableAnchor() async {
        let store = TaskDeliverablesReviewStore(scenario: .feedback)
        XCTAssertTrue(store.selectDeliverable(.validationDiff))
        XCTAssertTrue(store.selectVersion("diff-v2"))

        store.beginFeedback()
        store.setDraftLocationChoice(.known)

        guard case .unknown = store.draft.location else {
            return XCTFail(
                "A missing location must remain unknown, never become a fabricated anchor.")
        }
        XCTAssertFalse(store.draft.location.isReliable)
    }

    @MainActor
    func testTD4StartingFeedbackPreservesInputAndDoesNotMapOldLocationToNewVersion() async {
        let store = TaskDeliverablesReviewStore(scenario: .feedback)
        store.setDraftText("保留预先输入的反馈")
        store.setDraftRecipient(.apiMember)
        store.setDraftLocationChoice(.known)
        XCTAssertTrue(store.selectVersion("api-v2"))

        store.beginFeedback()

        XCTAssertEqual(store.draft.text, "保留预先输入的反馈")
        XCTAssertEqual(store.draft.recipient, .apiMember)
        XCTAssertEqual(store.draft.versionID, "api-v2")
        guard case .unknown = store.draft.location else {
            return XCTFail(
                "A v1 location selected before anchoring must not be represented as a v2 anchor.")
        }
        XCTAssertFalse(store.draft.location.isReliable)
    }

    @MainActor
    func testTD6SelfReportVerificationAndAcceptanceRemainSeparate() async throws {
        let store = TaskDeliverablesReviewStore(scenario: .evidence)
        let feedback = try XCTUnwrap(store.feedbacks.first)
        XCTAssertFalse(store.loadVerification(for: feedback.id, versionID: "diff-v1"))
        XCTAssertFalse(
            store.events(for: feedback.id).contains {
                $0.kind == .verification && $0.versionID == "diff-v1"
            })
        XCTAssertTrue(store.loadAgentReport(for: feedback.id))
        XCTAssertTrue(store.events(for: feedback.id).contains { $0.kind == .agentReport })
        XCTAssertFalse(store.events(for: feedback.id).contains { $0.kind == .verification })
        XCTAssertTrue(store.loadVerification(for: feedback.id, versionID: "api-v2"))
        XCTAssertTrue(store.loadUserAccepted(versionID: "api-v1"))
        XCTAssertTrue(
            store.events(for: feedback.id).contains {
                $0.kind == .verification && $0.versionID == "api-v2"
            })
        XCTAssertTrue(
            store.evidence.contains {
                $0.kind == .userAccepted && $0.versionID == "api-v1"
            })
        XCTAssertFalse(
            store.evidence.contains {
                $0.kind == .userAccepted && $0.versionID == "api-v2"
            })
    }

    @MainActor
    func testTD7ReworkUsesSameTaskAndPreservesHistory() async throws {
        let store = TaskDeliverablesReviewStore(scenario: .rework)
        let feedback = try XCTUnwrap(store.feedbacks.first)
        let oldEvidence = store.evidence
        XCTAssertTrue(store.beginRework(for: feedback.id))
        XCTAssertEqual(store.rounds.count, 1)
        XCTAssertEqual(store.rounds.first?.taskID, feedback.taskID)
        XCTAssertEqual(store.rounds.first?.sourceVersionID, "api-v1")
        XCTAssertEqual(store.evidence, oldEvidence)
        XCTAssertFalse(store.beginRework(for: feedback.id))
        XCTAssertEqual(store.feedback(id: feedback.id), feedback)
    }

    @MainActor
    func testTD8RestorationCanContinueButPauseAndStopStillBlock() async {
        let store = TaskDeliverablesReviewStore(scenario: .continuation)
        XCTAssertEqual(store.rounds.first?.state, .blocked)
        XCTAssertTrue(store.loadConditionEvent(.resourcesRestored))
        XCTAssertTrue(store.loadConditionEvent(.executionVerified))
        XCTAssertEqual(store.rounds.first?.state, .readyToContinue)
        XCTAssertTrue(store.loadConditionEvent(.authorizationRevoked))
        XCTAssertEqual(store.rounds.first?.state, .blocked)
        XCTAssertTrue(store.loadConditionEvent(.authorizationRestored))
        XCTAssertEqual(store.rounds.first?.state, .readyToContinue)

        XCTAssertTrue(store.loadConditionEvent(.pause))
        XCTAssertEqual(store.rounds.first?.state, .blocked)
        XCTAssertTrue(store.loadConditionEvent(.resume))
        XCTAssertEqual(store.rounds.first?.state, .readyToContinue)
        XCTAssertTrue(store.loadConditionEvent(.stopRequested))
        XCTAssertEqual(store.rounds.first?.state, .blocked)
        XCTAssertTrue(store.loadConditionEvent(.resourcesBlocked))
        XCTAssertTrue(store.conditions.blockers.contains("Stop 已请求"))
        XCTAssertTrue(store.conditions.blockers.contains("资源条件不足"))
    }

    @MainActor
    func testEachStoreInstanceOwnsIndependentMemoryAndResetClearsEvents() async {
        let first = TaskDeliverablesReviewStore(scenario: .feedback)
        let second = TaskDeliverablesReviewStore(scenario: .feedback)
        first.beginFeedback()
        first.setDraftText("只属于第一个窗口")
        XCTAssertTrue(first.sendFeedback())
        XCTAssertTrue(first.removeFromCollection(.apiDocument))
        XCTAssertTrue(second.visibleDeliverables.contains { $0.id == .apiDocument })
        XCTAssertTrue(second.feedbacks.isEmpty)

        first.loadScenario(.reset)
        XCTAssertTrue(first.feedbacks.isEmpty)
        XCTAssertTrue(first.records.isEmpty)
        XCTAssertTrue(first.visibleDeliverables.contains { $0.id == .apiDocument })
    }

    private func decision(
        _ environment: [String: String],
        _ fixture: EntryFixture
    ) -> TaskDeliverablesReviewEntryDecision {
        NativeTaskDeliverablesReviewEntry.decide(environment: environment, home: fixture.home.path)
    }

    private struct EntryFixture {
        let root: URL
        let home: URL
        let caches: URL
        let candidate: URL
        let environment: [String: String]
    }

    private func withEntryFixture(
        _ body: (EntryFixture) throws -> Void
    ) throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "task-deliverables-review-\(UUID().uuidString)", isDirectory: true)
        let home = root.appendingPathComponent("home", isDirectory: true)
        let caches = home.appendingPathComponent("Library/Caches", isDirectory: true)
        let candidate = caches.appendingPathComponent("sample", isDirectory: true)
        try FileManager.default.createDirectory(at: caches, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }

        let fixture = EntryFixture(
            root: root,
            home: home,
            caches: caches,
            candidate: candidate,
            environment: [
                NativeTaskDeliverablesReviewEntry.flagKey: "1",
                "AERORIC_ENV": "ui-test",
                "AERORIC_DATA_DIR": candidate.path,
            ]
        )
        try body(fixture)
    }
}
#endif
