#if DEBUG
import Foundation
import XCTest
@testable import AeroricUI

/// Entry policy, finite memory transitions and unshown lifecycle metadata.
/// All fixture I/O belongs to an exclusively created temporary fake home,
/// never the account home. No View, window, host or background work is invoked.
final class NativeWorkArrangementsReviewTests: XCTestCase {
    private let otherReviewKeys = [
        "AERORIC_NATIVE_SKILL_REVIEW",
        "AERORIC_NATIVE_KNOWLEDGE_REVIEW",
        "AERORIC_NATIVE_EVOLUTION_REVIEW",
        "AERORIC_NATIVE_VISUAL_REVIEW",
        "AERORIC_NATIVE_COLLABORATION_REVIEW",
    ]

    // MARK: - Explicit entry ownership

    func testAbsentFlagLeavesEveryOtherReviewValueCombinationInactive() throws {
        try withEntryFixture { fixture in
            let values: [String?] = [nil, "0", "1", "", "invalid"]
            let combinationCount = otherReviewKeys.reduce(1) { total, _ in total * values.count }
            var validContext = fixture.environment
            validContext[NativeWorkArrangementsReviewEntry.flagKey] = nil
            let contexts: [[String: String]] = [
                [:],
                validContext,
                ["AERORIC_ENV": "ui-test"],
                ["AERORIC_ENV": "", "AERORIC_DATA_DIR": ""],
                ["AERORIC_ENV": "not-a-review", "AERORIC_DATA_DIR": "relative-fixture"],
            ]
            for context in contexts {
                for combination in 0..<combinationCount {
                    var environment = context
                    var remaining = combination
                    for key in otherReviewKeys {
                        environment[key] = values[remaining % values.count]
                        remaining /= values.count
                    }
                    XCTAssertEqual(decision(environment, fixture), .inactive, "combination \(combination)")
                }
            }
        }
    }

    func testPresentZeroEmptyAndMalformedFlagsAreRefused() throws {
        try withEntryFixture { fixture in
            for value in ["0", "", "2", "-1", "true", "01", " 1", "1 ", "\n1", "1\n"] {
                var environment = fixture.environment
                environment[NativeWorkArrangementsReviewEntry.flagKey] = value
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidFlag]))
            }
        }
    }

    func testPresentInvalidRequestsNeverBecomeInactiveOrSelectAnOldReview() throws {
        try withEntryFixture { fixture in
            for value in ["0", "", "invalid"] {
                var environment = [NativeWorkArrangementsReviewEntry.flagKey: value]
                XCTAssertEqual(
                    decision(environment, fixture),
                    .refused([.invalidFlag, .invalidEnvironment, .invalidDirectory])
                )
                for key in otherReviewKeys { environment[key] = "1" }
                XCTAssertEqual(
                    decision(environment, fixture),
                    .refused([.invalidFlag, .invalidEnvironment, .invalidDirectory, .conflictingReview])
                )
            }
        }
    }

    func testMissingAndNonExactReviewEnvironmentsAreRefused() throws {
        try withEntryFixture { fixture in
            let values: [String?] = [
                nil, "", "default", "production", "UI-TEST", "ui-test ", " ui-test", "ui-test\n",
            ]
            for value in values {
                var environment = fixture.environment
                environment["AERORIC_ENV"] = value
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidEnvironment]))
            }
        }
    }

    func testEachOfTheFiveOtherReviewFlagsConflictsIndividually() throws {
        try withEntryFixture { fixture in
            for key in otherReviewKeys {
                var environment = fixture.environment
                for other in otherReviewKeys { environment[other] = nil }
                environment[key] = "1"
                XCTAssertEqual(decision(environment, fixture), .refused([.conflictingReview]), key)
            }
        }
    }

    func testAllThirtyOneNonemptyOtherReviewCombinationsAreRefused() throws {
        try withEntryFixture { fixture in
            for mask in 1..<(1 << otherReviewKeys.count) {
                var environment = fixture.environment
                for (index, key) in otherReviewKeys.enumerated() {
                    environment[key] = mask & (1 << index) == 0 ? "0" : "1"
                }
                XCTAssertEqual(
                    decision(environment, fixture), .refused([.conflictingReview]), "mask \(mask)"
                )
            }
        }
    }

    func testOtherReviewKeysAbsentZeroAndNonOneValuesDoNotConflict() throws {
        try withEntryFixture { fixture in
            let values: [String?] = [nil, "0", "", "invalid", "true", "01", " 1", "1 ", "1\n"]
            for value in values {
                var environment = fixture.environment
                for key in otherReviewKeys { environment[key] = value }
                XCTAssertEqual(decision(environment, fixture), .review)
            }
            var mixed = fixture.environment
            for (index, key) in otherReviewKeys.enumerated() { mixed[key] = values[index] }
            XCTAssertEqual(decision(mixed, fixture), .review)
        }
    }

    func testCombinedRefusalsRetainOnlyFixedNonSensitiveReasons() throws {
        try withEntryFixture { fixture in
            for marker in ["fictional-private-input-a", "fictional-private-input-b"] {
                var environment = fixture.environment
                environment[NativeWorkArrangementsReviewEntry.flagKey] = marker
                environment["AERORIC_ENV"] = marker
                environment["AERORIC_DATA_DIR"] = fixture.home.appendingPathComponent(marker).path
                for key in otherReviewKeys { environment[key] = "1" }
                let result = decision(environment, fixture)
                XCTAssertEqual(
                    result, .refused([.invalidFlag, .invalidEnvironment, .invalidDirectory, .conflictingReview])
                )
                XCTAssertFalse(String(reflecting: result).contains(marker))
                XCTAssertFalse(String(reflecting: result).contains(fixture.root.path))
                for reason in WorkArrangementsReviewRefusalReason.allCases {
                    XCTAssertEqual(reason.id, reason.rawValue)
                    XCTAssertFalse(reason.title.isEmpty)
                    XCTAssertFalse(reason.detail.isEmpty)
                    for text in [reason.title, reason.detail] {
                        XCTAssertFalse(text.contains(marker))
                        XCTAssertFalse(text.contains(fixture.root.path))
                        XCTAssertFalse(text.contains("/"))
                        XCTAssertFalse(text.contains("AERORIC_"))
                        XCTAssertFalse(text.contains("launchID"))
                        XCTAssertFalse(text.contains("PID"))
                    }
                }
            }
        }
    }

    // MARK: - Existing helper policy, always with an explicit fake home

    func testMissingEmptyWhitespaceAndRelativeDirectoriesAreRefused() throws {
        try withEntryFixture { fixture in
            let values: [String?] = [
                nil, "", " ", "\t", "\n", "Library/Caches/review", "./review", "~/Library/Caches/review",
            ]
            for value in values {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = value
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidDirectory]))
            }
        }
    }

    func testCachesRootDotSegmentsPrefixSquattingAndPaddedPathsAreRefused() throws {
        try withEntryFixture { fixture in
            let rejected = [
                fixture.home.path, fixture.caches.path, fixture.caches.path + "/",
                fixture.home.path + "/Library/Caches-neighbour/review",
                fixture.home.path + "/Library/Caches2/review",
                fixture.home.path + "/Library/Application Support/review",
                fixture.caches.path + "/../review",
                fixture.caches.path + "/safe/../../review",
                fixture.caches.path + "/./review",
                fixture.caches.path + "/safe/../review",
                " " + fixture.safeChild.path, fixture.safeChild.path + " ",
                "\t" + fixture.safeChild.path, fixture.safeChild.path + "\t",
                "\n" + fixture.safeChild.path, fixture.safeChild.path + "\n",
            ]
            for path in rejected {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = path
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidDirectory]))
            }
        }
    }

    func testSafeMissingChildIsAcceptedWithoutCreatingItOrChangingParentListings() throws {
        try withEntryFixture { fixture in
            let manager = FileManager.default
            let parents = [
                fixture.root, fixture.home,
                fixture.home.appendingPathComponent("Library", isDirectory: true), fixture.caches,
            ]
            let before = try directoryListings(parents)
            XCTAssertFalse(manager.fileExists(atPath: fixture.safeChild.path))
            for _ in 0..<3 {
                XCTAssertEqual(decision(fixture.environment, fixture), .review)
            }
            XCTAssertFalse(manager.fileExists(atPath: fixture.safeChild.path))
            XCTAssertEqual(try directoryListings(parents), before)
        }
    }

    func testExistingDirectoryAndMissingNestedChildAreAcceptedWithoutWrites() throws {
        try withEntryFixture { fixture in
            let manager = FileManager.default
            let existing = fixture.caches.appendingPathComponent("existing-parent", isDirectory: true)
            let missing = existing.appendingPathComponent("missing/deep-child", isDirectory: true)
            try manager.createDirectory(at: existing, withIntermediateDirectories: false)
            let parents = [fixture.caches, existing]
            let before = try directoryListings(parents)
            for path in [existing.path, missing.path] {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = path
                XCTAssertEqual(decision(environment, fixture), .review)
            }
            XCTAssertFalse(manager.fileExists(atPath: existing.appendingPathComponent("missing").path))
            XCTAssertEqual(try directoryListings(parents), before)
        }
    }

    func testCandidateAndCachesDescendantSymlinksIncludingDanglingLinksAreRefused() throws {
        try withEntryFixture { fixture in
            let manager = FileManager.default
            let inside = fixture.caches.appendingPathComponent("real-inside", isDirectory: true)
            let outside = fixture.root.appendingPathComponent("outside-fake-home", isDirectory: true)
            let intermediate = fixture.caches.appendingPathComponent("intermediate", isDirectory: true)
            let missingTarget = fixture.root.appendingPathComponent("missing-target", isDirectory: true)
            let file = fixture.caches.appendingPathComponent("plain-target")
            for directory in [inside, outside, intermediate] {
                try manager.createDirectory(at: directory, withIntermediateDirectories: false)
            }
            try Data("fictional link target".utf8).write(to: file)
            let targets = [
                ("inside", inside), ("outside", outside), ("caches-root", fixture.caches),
                ("dangling", missingTarget), ("file", file),
            ]
            var candidates: [String] = []
            // Exercise links at/below Caches, not Caches itself or higher
            // ancestors: the shared helper does not inspect those levels.
            for parent in [fixture.caches, intermediate] {
                for (name, target) in targets {
                    let link = parent.appendingPathComponent("link-\(name)")
                    try manager.createSymbolicLink(at: link, withDestinationURL: target)
                    candidates += [link.path, link.appendingPathComponent("missing/deep-child").path]
                }
            }
            let parents = [fixture.root, fixture.caches, intermediate, inside, outside]
            let before = try directoryListings(parents)
            for path in candidates {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = path
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidDirectory]))
            }
            XCTAssertFalse(manager.fileExists(atPath: missingTarget.path))
            XCTAssertEqual(try directoryListings(parents), before)
        }
    }

    func testExistingFileCandidateAndFileAncestorAreRefusedWithoutWrites() throws {
        try withEntryFixture { fixture in
            let file = fixture.caches.appendingPathComponent("plain-fixture-file")
            try Data("fictional entry fixture".utf8).write(to: file)
            let before = try directoryListings([fixture.caches])
            for path in [file.path, file.appendingPathComponent("missing/deep-child").path] {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = path
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidDirectory]))
            }
            XCTAssertEqual(try directoryListings([fixture.caches]), before)
        }
    }

    // MARK: - Policy-only initialization; no View/window or lifecycle callbacks

    // The async XCTest adapter avoids synchronous @MainActor discovery casts.
    @MainActor
    func testInactiveLifecycleInitializationIsInertWithoutCreatingAWindow() async {
        let lifecycle = NativeWorkArrangementsReviewWindowLifecycle(decision: .inactive)
        XCTAssertNil(lifecycle.windowIdentifier)
        XCTAssertEqual(lifecycle.windowTitle, "")
        XCTAssertFalse(lifecycle.isWindowCreated)
    }

    @MainActor
    func testReviewAndRefusalLifecycleMetadataIsDistinctWithoutCreatingWindows() async {
        let review = NativeWorkArrangementsReviewWindowLifecycle(decision: .review)
        let refusal = NativeWorkArrangementsReviewWindowLifecycle(
            decision: .refused(WorkArrangementsReviewRefusalReason.allCases)
        )
        XCTAssertEqual(review.windowIdentifier?.rawValue, "native-work-arrangements-review")
        XCTAssertEqual(refusal.windowIdentifier?.rawValue, "native-work-arrangements-review-refusal")
        XCTAssertNotEqual(review.windowIdentifier?.rawValue, refusal.windowIdentifier?.rawValue)
        XCTAssertEqual(review.windowTitle, "工作安排审阅样例")
        XCTAssertEqual(refusal.windowTitle, "工作安排审阅未启动")
        XCTAssertFalse(review.isWindowCreated)
        XCTAssertFalse(refusal.isWindowCreated)
    }

    // MARK: - Exclusively owned temporary fixture and explicit home injection

    private struct EntryFixture {
        let root: URL
        let home: URL
        let caches: URL

        var safeChild: URL {
            caches.appendingPathComponent("missing-work-arrangements-review-child", isDirectory: true)
        }

        var environment: [String: String] {
            [
                "AERORIC_NATIVE_WORK_ARRANGEMENTS_REVIEW": "1",
                "AERORIC_ENV": "ui-test",
                "AERORIC_DATA_DIR": safeChild.path,
                "AERORIC_NATIVE_SKILL_REVIEW": "0",
                "AERORIC_NATIVE_KNOWLEDGE_REVIEW": "0",
                "AERORIC_NATIVE_EVOLUTION_REVIEW": "0",
                "AERORIC_NATIVE_VISUAL_REVIEW": "0",
                "AERORIC_NATIVE_COLLABORATION_REVIEW": "0",
            ]
        }
    }

    private func withEntryFixture(_ body: (EntryFixture) throws -> Void) throws {
        let manager = FileManager.default
        let root = manager.temporaryDirectory
            .appendingPathComponent("aeroric-work-arrangements-entry-\(UUID().uuidString)", isDirectory: true)
        try manager.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? manager.removeItem(at: root) }
        let home = root.appendingPathComponent("fake-home", isDirectory: true)
        let caches = home.appendingPathComponent("Library/Caches", isDirectory: true)
        try manager.createDirectory(at: caches, withIntermediateDirectories: true)
        try body(.init(root: root, home: home, caches: caches))
    }

    private func directoryListings(_ directories: [URL]) throws -> [[String]] {
        try directories.map { try FileManager.default.contentsOfDirectory(atPath: $0.path).sorted() }
    }

    private func decision(
        _ environment: [String: String], _ fixture: EntryFixture
    ) -> WorkArrangementsReviewEntryDecision {
        NativeWorkArrangementsReviewEntry.decide(environment: environment, home: fixture.home.path)
    }
}

// Integrated into the single whitelisted suite after entry-owner handoff.
extension NativeWorkArrangementsReviewTests {
    @MainActor
    func testW1CreationSourcesAndDirectExecutionDoNotImplyCompletion() async {
        let store = WorkArrangementsReviewStore(scenario: .w5)
        let runCount = store.executions.count
        var draft = store.makeNewTodoDraft()
        draft.title = "新增目标 · 非生产"
        draft.source = .agent
        XCTAssertTrue(store.saveNewTodo(draft))
        let item = store.todo(store.selectedTodoID)
        XCTAssertEqual(item?.source, .agent)
        XCTAssertEqual(item?.intent, .recorded)
        XCTAssertEqual(store.executions.count, runCount)
        XCTAssertFalse(store.isTechnicallyComplete(item?.id ?? ""))
        XCTAssertTrue(store.todos.contains { $0.source == .user })
        XCTAssertTrue(store.requestLaunch())
        XCTAssertEqual(store.executions.count, runCount, "启动意图不是执行事实")
        XCTAssertTrue(store.loadStartedRecord())
        XCTAssertNil(store.executions.last?.todoID, "保留无待办的直接执行")
    }

    @MainActor
    func testW1EndedSafeHandoffTechnicalProofAndUserAcceptanceAreSeparate() async {
        let store = WorkArrangementsReviewStore()
        let id = WorkArrangementsReviewFixtures.primaryTodoID
        let run = WorkArrangementsReviewFixtures.activeRunID
        let oldFailure = store.execution("run-first-failure")
        XCTAssertFalse(store.recordUserAcceptance(id))
        store.loadRunRecord(.ended, runID: run)
        XCTAssertFalse(store.isTechnicallyComplete(id))
        store.loadVerificationRecord(.complete, todoID: id)
        XCTAssertFalse(store.isTechnicallyComplete(id), "结束不证明原占用和副作用已核实")
        store.loadRunRecord(.safeClosure, runID: run)
        for missing in [WAReviewVerificationRecord.missingIntegration, .missingChecks, .missingSupervision, .unknown] {
            store.loadVerificationRecord(missing, todoID: id)
            XCTAssertFalse(store.isTechnicallyComplete(id), missing.rawValue)
            XCTAssertFalse(store.recordUserAcceptance(id))
        }
        store.loadVerificationRecord(.complete, todoID: id)
        XCTAssertTrue(store.isTechnicallyComplete(id))
        XCTAssertEqual(store.todo(id)?.userAccepted, false)
        XCTAssertTrue(store.recordUserAcceptance(id))
        XCTAssertEqual(store.todo(id)?.userAccepted, true)
        XCTAssertEqual(store.execution("run-first-failure"), oldFailure)
        XCTAssertEqual(store.executions.filter { $0.todoID == id }.count, 2)
    }

    @MainActor
    func testW1FailedStoppedAndUnknownRecordsCannotBecomeTechnicalSuccess() async {
        for record in [WAReviewRunRecord.failed, .unknown, .safeClosure] {
            let store = WorkArrangementsReviewStore()
            let id = WorkArrangementsReviewFixtures.primaryTodoID
            store.loadRunRecord(record, runID: WorkArrangementsReviewFixtures.activeRunID)
            store.loadVerificationRecord(.complete, todoID: id)
            XCTAssertFalse(store.isTechnicallyComplete(id), record.rawValue)
            XCTAssertFalse(store.recordUserAcceptance(id))
        }
        let stopped = WorkArrangementsReviewStore()
        stopped.stopRule()
        stopped.loadRunRecord(.ended, runID: WorkArrangementsReviewFixtures.activeRunID)
        stopped.loadRunRecord(.safeClosure, runID: WorkArrangementsReviewFixtures.activeRunID)
        stopped.loadVerificationRecord(.complete, todoID: WorkArrangementsReviewFixtures.primaryTodoID)
        XCTAssertFalse(stopped.isTechnicallyComplete(WorkArrangementsReviewFixtures.primaryTodoID))
    }

    @MainActor
    func testW1LaterEndedOrUnknownReceiptNeverOverwritesTheSameAttemptsFailure() async {
        let store = WorkArrangementsReviewStore()
        let run = WorkArrangementsReviewFixtures.activeRunID
        store.loadRunRecord(.failed, runID: run)
        store.loadRunRecord(.unknown, runID: run)
        store.loadRunRecord(.ended, runID: run)
        XCTAssertEqual(store.execution(run)?.phase, .unknown, "结束记录不能洗掉该次失败")
        XCTAssertEqual(store.execution(run)?.hasFailureRecord, true)
        store.loadRunRecord(.safeClosure, runID: run)
        XCTAssertEqual(store.execution(run)?.phase, .failed)
        store.loadVerificationRecord(.complete, todoID: WorkArrangementsReviewFixtures.primaryTodoID)
        XCTAssertFalse(store.isTechnicallyComplete(WorkArrangementsReviewFixtures.primaryTodoID))
        XCTAssertEqual(store.executions.filter { $0.todoID == WorkArrangementsReviewFixtures.primaryTodoID }.count, 2)
    }

    @MainActor
    func testW2IndependentSingleAssignmentDoesNotEnableRuleOrInheritFullAccess() async {
        let store = WorkArrangementsReviewStore(scenario: .w2)
        XCTAssertFalse(store.rule.enabled)
        XCTAssertFalse(store.requestLaunch(todoID: "agent-note"))
        XCTAssertTrue(store.assignTodo("agent-note"))
        XCTAssertEqual(store.todo("agent-note")?.assignmentPermission, .limited)
        XCTAssertEqual(store.rule.policy.permission, .unanswered)
        XCTAssertTrue(store.requestLaunch(todoID: "agent-note"))
        XCTAssertFalse(store.rule.enabled)
        XCTAssertEqual(store.executions.count, 1)
        XCTAssertTrue(store.loadStartedRecord())
        XCTAssertEqual(store.executions.last?.todoID, "agent-note")
        XCTAssertFalse(store.assignTodo("agent-note"), "在途不重复接单")
    }

    @MainActor
    func testW2SavingNeverGrantedRuleDoesNotRevokeIndependentTaskAuthority() async {
        let store = WorkArrangementsReviewStore(scenario: .w2)
        XCTAssertTrue(store.assignTodo("agent-note"))
        XCTAssertTrue(store.requestLaunch(todoID: "agent-note"))
        XCTAssertTrue(store.loadStartedRecord())
        let runID = store.executions.last?.id
        let basis = store.execution(runID)?.basis
        XCTAssertEqual(store.execution(runID)?.usesRuleAuthority, false)
        var draft = store.makeRuleDraft()
        draft.edited.configuration = .b
        XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: false))
        XCTAssertEqual(store.execution(runID)?.phase, .running)
        XCTAssertNil(store.execution(runID)?.stopCause)
        XCTAssertEqual(store.execution(runID)?.basis, basis)
        XCTAssertEqual(store.todo("agent-note")?.assignmentPermission, .limited)
        store.loadRevocationRecord()
        XCTAssertEqual(store.execution(runID)?.phase, .stopRequested)
        XCTAssertEqual(store.todo("agent-note")?.assignmentPermission, .revoked)
    }

    @MainActor
    func testW2IndependentTaskContinuationDoesNotSilentlyRequireEnablingUnrelatedAutomation() async {
        let store = WorkArrangementsReviewStore(scenario: .w2)
        XCTAssertTrue(store.assignTodo("agent-note"))
        XCTAssertTrue(store.requestLaunch(todoID: "agent-note"))
        XCTAssertTrue(store.loadStartedRecord())
        let run = store.executions.last?.id ?? ""
        store.loadLimitRecord(.localBudget, value: .failed)
        store.loadRunRecord(.safeClosure, runID: run)
        store.topUp(.local, continuingRunID: run)
        XCTAssertEqual(store.launchIntent?.origin, .resourceContinuation)
        XCTAssertTrue(store.loadStartedRecord())
        XCTAssertFalse(store.rule.enabled)
        XCTAssertEqual(store.rule.policy.permission, .unanswered)
        XCTAssertEqual(store.executions.last?.usesRuleAuthority, false)
    }

    @MainActor
    func testW2PreviewConfirmationPermissionEnablementAndBacklogAreDistinct() async {
        let store = WorkArrangementsReviewStore(scenario: .w2)
        let original = store.todos
        var draft = store.makeRuleDraft()
        draft.edited.permission = .fullAccess
        draft.edited.scope = .wholeProject
        XCTAssertEqual(store.todos, original)
        XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: false))
        XCTAssertEqual(store.rule.policy.permission, .fullAccess)
        XCTAssertFalse(store.rule.enabled)
        XCTAssertFalse(store.enableRule(), "选择Full Access但未确认不够")
        draft = store.makeRuleDraft()
        XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
        XCTAssertFalse(store.rule.enabled)
        XCTAssertTrue(store.enableRule())
        XCTAssertEqual(store.todos, original, "首次启用不回填")
        var batch = store.makeBatchDraft(kind: .backlog)
        batch.selected = ["agent-note", "backlog-docs"]
        batch.excluded.insert("backlog-docs")
        XCTAssertTrue(store.confirmBatch(batch))
        XCTAssertEqual(store.todo("agent-note")?.intent, .assigned)
        XCTAssertEqual(store.todo("backlog-docs")?.intent, .recorded)
        XCTAssertEqual(store.todo("backlog-docs")?.excluded, true)
        XCTAssertEqual(store.executions.count, 1)
    }

    @MainActor
    func testW2CancelledBatchAndRuleDraftHaveNoEffectsAndBadSelectionIsAtomic() async {
        let store = WorkArrangementsReviewStore(scenario: .w2)
        let originalRule = store.rule
        let originalTodos = store.todos
        var ruleDraft = store.makeRuleDraft()
        ruleDraft.edited.permission = .fullAccess
        var batch = store.makeBatchDraft(kind: .assignment)
        batch.selected = ["agent-note"]
        batch.excluded.insert("goal-interface")
        // Cancel means never calling apply/confirm; no mutation hides in preview.
        XCTAssertEqual(store.rule, originalRule)
        XCTAssertEqual(store.todos, originalTodos)
        batch.selected.insert("unclear")
        XCTAssertFalse(store.confirmBatch(batch))
        XCTAssertEqual(store.todos, originalTodos, "单个无效项不造成半批应用")
        XCTAssertFalse(store.assignTodo("record-only"))
        XCTAssertFalse(store.assignTodo("unclear"))
        XCTAssertEqual(store.rule, originalRule)
    }

    @MainActor
    func testW2ArrivalsNeedCurrentRuleCoverageNotCreatorIdentityAndNeverBackfill() async {
        for permission in [WAReviewPermission.unanswered, .unknown, .revoked, .limited, .fullAccess] {
            let store = WorkArrangementsReviewStore(scenario: .w2)
            var draft = store.makeRuleDraft()
            draft.edited.permission = permission
            XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
            if permission.isGranted { XCTAssertTrue(store.enableRule()) }
            store.loadArrivalRecord()
            let expected: WAReviewIntent = permission.isGranted ? .assigned : .recorded
            XCTAssertEqual(store.todo("arrival-user")?.intent, expected)
            XCTAssertEqual(store.todo("arrival-agent")?.intent, expected)
            for id in ["arrival-note", "arrival-unclear", "arrival-excluded", "agent-note"] {
                XCTAssertEqual(store.todo(id)?.intent, .recorded, id)
            }
            let count = store.todos.count
            store.loadArrivalRecord()
            XCTAssertEqual(store.todos.count, count)
        }
    }

    @MainActor
    func testW3PausePreservesInFlightWhileStopNeedsUnknownAndSafeRecords() async {
        let store = WorkArrangementsReviewStore(scenario: .w3)
        let run = WorkArrangementsReviewFixtures.activeRunID
        let before = store.execution(run)
        store.pauseRule()
        XCTAssertEqual(store.execution(run), before)
        XCTAssertFalse(store.requestLaunch())
        store.stopRule()
        XCTAssertEqual(store.execution(run)?.phase, .stopRequested)
        XCTAssertEqual(store.execution(run)?.stopCause, .user)
        store.loadRunRecord(.unknown, runID: run)
        XCTAssertEqual(store.execution(run)?.phase, .unknown)
        XCTAssertTrue(store.hasPrimaryOccupancy)
        store.topUp(.project)
        store.loadGateRecord(.dependency, value: .satisfied)
        XCTAssertTrue(store.rule.stopped)
        XCTAssertTrue(store.rule.paused)
        XCTAssertNil(store.launchIntent)
        store.loadRunRecord(.safeClosure, runID: run)
        XCTAssertEqual(store.execution(run)?.phase, .safelyStopped)
        XCTAssertFalse(store.hasPrimaryOccupancy)
        XCTAssertTrue(store.rule.stopped)
        XCTAssertEqual(store.execution(run)?.artifacts, before?.artifacts)
        XCTAssertTrue(store.explicitlyResumeRule())
        XCTAssertNil(store.launchIntent, "恢复范围意图也不复活旧工作")
        XCTAssertEqual(store.execution(run)?.stopCause, .user)
    }

    @MainActor
    func testW4NewRuleBasisNeverHotSwapsInFlightOrHistoricalResults() async {
        let store = WorkArrangementsReviewStore(scenario: .w4)
        let run = WorkArrangementsReviewFixtures.activeRunID
        let originalBasis = store.execution(run)?.basis
        var draft = store.makeRuleDraft()
        draft.edited.configuration = .b
        draft.edited.scope = .wholeProject
        draft.edited.permission = .fullAccess
        XCTAssertTrue(draft.difference.contains("扩围"))
        XCTAssertTrue(draft.difference.contains("提权"))
        XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
        XCTAssertEqual(store.execution(run)?.basis, originalBasis)
        XCTAssertFalse(store.requestLaunch(), "保存/策略更改不绕过占用")
        store.loadRunRecord(.ended, runID: run)
        store.loadRunRecord(.safeClosure, runID: run)
        let count = store.executions.count
        store.loadScheduledRecord()
        XCTAssertEqual(store.executions.count, count, "到点只生成可核实的意图")
        XCTAssertTrue(store.loadStartedRecord())
        XCTAssertEqual(store.executions.last?.basis.configuration, .b)
        XCTAssertEqual(store.executions.last?.basis.ruleRevision, store.rule.revision)
        XCTAssertEqual(store.execution(run)?.basis, originalBasis)
    }

    @MainActor
    func testW4StaleDraftAndUnconfirmedExpansionCannotBorrowOldConsent() async {
        let store = WorkArrangementsReviewStore(scenario: .w5)
        var first = store.makeRuleDraft()
        let stale = first
        first.edited.scope = .wholeProject
        first.edited.permission = .fullAccess
        XCTAssertTrue(store.applyRuleDraft(first, confirmingPolicy: false))
        XCTAssertFalse(store.applyRuleDraft(stale, confirmingPolicy: true))
        XCTAssertFalse(store.requestLaunch())
        XCTAssertFalse(store.rule.policyConfirmed)
        XCTAssertTrue(store.rule.enabled, "启用事实与未确认的新策略分别显示")
    }

    @MainActor
    func testW4RevocationBanAndHostHardLimitOverrideOldBasis() async {
        for kind in [WAReviewGate.noProhibition, .hostSafety, .hardLimitCapability] {
            let store = WorkArrangementsReviewStore(scenario: .w4)
            let basis = store.execution(WorkArrangementsReviewFixtures.activeRunID)?.basis
            store.loadGateRecord(kind, value: .unknown)
            XCTAssertEqual(store.execution(WorkArrangementsReviewFixtures.activeRunID)?.phase, .stopRequested)
            XCTAssertEqual(store.execution(WorkArrangementsReviewFixtures.activeRunID)?.basis, basis)
            XCTAssertFalse(store.requestLaunch())
        }
        let store = WorkArrangementsReviewStore(scenario: .w4)
        store.loadRevocationRecord()
        XCTAssertEqual(store.rule.policy.permission, .revoked)
        XCTAssertEqual(store.execution(WorkArrangementsReviewFixtures.activeRunID)?.phase, .stopRequested)
        XCTAssertFalse(store.requestLaunch(todoID: "goal-interface"))
    }

    @MainActor
    func testW5MissedOpportunitiesStayUnrunAndPreauthorizedRecoveryMakesOnlyOneIntent() async {
        let plain = WorkArrangementsReviewStore(scenario: .w5)
        plain.loadMissedRecords()
        plain.loadRecoveryRecord()
        XCTAssertEqual(plain.opportunities.count, 3)
        XCTAssertNil(plain.launchIntent)
        let store = WorkArrangementsReviewStore(scenario: .w5)
        var draft = store.makeRuleDraft()
        draft.edited.recoveryOnce = true
        XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
        XCTAssertFalse(store.rule.policy.afterBusyOnce)
        store.loadMissedRecords()
        let missed = store.opportunities
        store.loadRecoveryRecord()
        XCTAssertEqual(store.launchIntent?.origin, .recovery)
        let count = store.executions.count
        XCTAssertTrue(store.loadStartedRecord())
        store.loadRecoveryRecord()
        store.loadMissedRecords()
        XCTAssertEqual(store.executions.count, count + 1)
        XCTAssertNil(store.launchIntent)
        XCTAssertEqual(store.opportunities, missed)
        XCTAssertTrue(missed.allSatisfy { $0.description.contains("未启动") })
    }

    @MainActor
    func testW5LateOptInPauseStopDisabledAndUnknownOccupancyNeverReplayMisses() async {
        let late = WorkArrangementsReviewStore(scenario: .w5)
        late.loadMissedRecords()
        var draft = late.makeRuleDraft()
        draft.edited.recoveryOnce = true
        XCTAssertTrue(late.applyRuleDraft(draft, confirmingPolicy: true))
        late.loadRecoveryRecord()
        XCTAssertNil(late.launchIntent, "不是事先获准，不能回填")
        for control in 0..<4 {
            let store = WorkArrangementsReviewStore(scenario: .w5)
            draft = store.makeRuleDraft()
            draft.edited.recoveryOnce = true
            XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
            store.loadMissedRecords()
            switch control {
            case 0: store.pauseRule()
            case 1: store.stopRule()
            case 2: store.disableRule()
            default: store.loadGateRecord(.dependency, value: .unknown)
            }
            store.loadRecoveryRecord()
            XCTAssertNil(store.launchIntent)
        }
        let busy = WorkArrangementsReviewStore(scenario: .w6)
        busy.loadRunRecord(.unknown, runID: WorkArrangementsReviewFixtures.activeRunID)
        busy.loadMissedRecords()
        XCTAssertTrue(busy.opportunities.isEmpty, "失联/副作用未知不是确定漏跑")
    }

    @MainActor
    func testW5PreapprovedRecoveryRechecksRestoredDependenciesRatherThanRequiringThemDuringOutage() async {
        let store = WorkArrangementsReviewStore(scenario: .w5)
        var draft = store.makeRuleDraft()
        draft.edited.recoveryOnce = true
        XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
        store.loadGateRecord(.dependency, value: .failed)
        store.loadMissedRecords()
        XCTAssertEqual(store.opportunities.count, 3)
        XCTAssertTrue(store.recoveryEligible, "故障期的意图不冒充当时具备运行条件")
        XCTAssertNil(store.launchIntent)
        store.loadGateRecord(.dependency, value: .satisfied)
        store.loadRecoveryRecord()
        XCTAssertEqual(store.launchIntent?.origin, .recovery)
        XCTAssertTrue(store.loadStartedRecord())
        XCTAssertEqual(store.opportunities.count, 3)
    }

    @MainActor
    func testW5AndW6ChangingTriggerCategoryInvalidatesOldScheduledFollowups() async {
        for busy in [false, true] {
            let store = WorkArrangementsReviewStore(scenario: busy ? .w6 : .w5)
            var draft = store.makeRuleDraft()
            draft.edited.recoveryOnce = true
            draft.edited.afterBusyOnce = true
            XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
            if busy { store.loadBusyRecords() } else { store.loadMissedRecords() }
            draft = store.makeRuleDraft()
            draft.edited.trigger = .arrival
            XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
            XCTAssertFalse(store.recoveryEligible, "类别更改当时即失效，不等消费恢复事件")
            XCTAssertFalse(store.busyFollowupPending, "类别更改当时即失效，不等原执行结束")
            let count = store.executions.count
            if busy {
                store.loadRunRecord(.safeClosure, runID: WorkArrangementsReviewFixtures.activeRunID)
            } else {
                store.loadRecoveryRecord()
            }
            XCTAssertNil(store.launchIntent)
            XCTAssertFalse(store.loadStartedRecord())
            XCTAssertEqual(store.executions.count, count)
            XCTAssertFalse(store.recoveryEligible)
            XCTAssertFalse(store.busyFollowupPending)
            XCTAssertEqual(store.opportunities.count, 3, "旧跳过不改成成功或新队列")
        }
    }

    @MainActor
    func testW6BusyCatchupIsIndependentFiniteAndWaitsForAllOccupancyFacts() async {
        let plain = WorkArrangementsReviewStore(scenario: .w6)
        var draft = plain.makeRuleDraft()
        draft.edited.recoveryOnce = true
        XCTAssertTrue(plain.applyRuleDraft(draft, confirmingPolicy: true))
        plain.loadBusyRecords()
        XCTAssertFalse(plain.busyFollowupPending, "不继承恢复后补一次")
        let store = WorkArrangementsReviewStore(scenario: .w6)
        draft = store.makeRuleDraft()
        draft.edited.afterBusyOnce = true
        XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
        store.loadBusyRecords()
        XCTAssertEqual(store.opportunities.count, 3)
        XCTAssertTrue(store.busyFollowupPending)
        XCTAssertFalse(store.requestLaunch())
        store.loadScheduledRecord()
        XCTAssertNil(store.launchIntent)
        store.loadRunRecord(.processExited, runID: WorkArrangementsReviewFixtures.activeRunID)
        XCTAssertTrue(store.hasPrimaryOccupancy)
        XCTAssertNil(store.launchIntent)
        store.loadRunRecord(.safeClosure, runID: WorkArrangementsReviewFixtures.activeRunID)
        XCTAssertEqual(store.launchIntent?.origin, .afterBusy)
        XCTAssertFalse(store.busyFollowupPending)
        XCTAssertTrue(store.loadStartedRecord(), "不额外要求用户验收上一轮")
        store.loadBusyRecords()
        XCTAssertEqual(store.opportunities.count, 3)
        XCTAssertFalse(store.busyFollowupPending)
    }

    @MainActor
    func testW6PausedOrMissingSupervisionDoesNotContinueAfterSafeClosure() async {
        for paused in [true, false] {
            let store = WorkArrangementsReviewStore(scenario: .w6)
            var draft = store.makeRuleDraft()
            draft.edited.afterBusyOnce = true
            XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
            store.loadBusyRecords()
            if paused { store.pauseRule() } else { store.loadGateRecord(.supervision, value: .unanswered) }
            store.loadRunRecord(.safeClosure, runID: WorkArrangementsReviewFixtures.activeRunID)
            XCTAssertNil(store.launchIntent)
            XCTAssertEqual(store.rule.paused, paused)
        }
    }

    @MainActor
    func testW7LocalWaitRechecksReleaseInputsAndCurrentIntentWithoutRepeatingPermission() async {
        let store = WorkArrangementsReviewStore(scenario: .w7)
        let independent = store.execution("independent-docs")
        XCTAssertFalse(store.requestWaitingLaunch("wait-adapter"))
        store.loadSharedPointRecord(.unknown)
        XCTAssertFalse(store.requestWaitingLaunch("wait-adapter"))
        store.loadInputChangedRecord()
        store.loadSharedPointRecord(.satisfied)
        XCTAssertNil(store.launchIntent)
        XCTAssertEqual(store.todo("wait-adapter")?.wait, .alignment)
        store.loadLocalAlignmentRecord("wait-adapter")
        XCTAssertEqual(store.todo("wait-adapter")?.wait, .ready)
        XCTAssertEqual(store.launchIntent?.origin, .waiting)
        XCTAssertTrue(store.loadStartedRecord())
        XCTAssertFalse(store.requestWaitingLaunch("wait-tests"), "同修改点仍不可重叠")
        XCTAssertEqual(store.execution("independent-docs"), independent)
    }

    @MainActor
    func testW7IndependentWaitingStartPreservesAuthorityThroughRuleRevocationAndLimitContinuation() async {
        let store = WorkArrangementsReviewStore(scenario: .w7)
        store.loadSharedPointRecord(.satisfied)
        XCTAssertEqual(store.launchIntent?.origin, .waiting)
        XCTAssertTrue(store.loadStartedRecord())
        let run = store.executions.last?.id ?? ""
        XCTAssertEqual(store.execution(run)?.usesRuleAuthority, false)
        var draft = store.makeRuleDraft()
        draft.edited.permission = .revoked
        XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
        XCTAssertEqual(store.execution(run)?.phase, .running)
        XCTAssertNil(store.execution(run)?.stopCause)
        store.disableRule()
        store.loadLimitRecord(.localBudget, value: .failed)
        store.loadRunRecord(.safeClosure, runID: run)
        store.topUp(.local, continuingRunID: run)
        XCTAssertEqual(store.launchIntent?.continuingRunID, run)
        XCTAssertTrue(store.loadStartedRecord())
        XCTAssertEqual(store.executions.last?.usesRuleAuthority, false)
        XCTAssertFalse(store.rule.enabled)
        XCTAssertEqual(store.rule.policy.permission, .revoked)
    }

    @MainActor
    func testW7CancelledResolvedUnknownOrNoAttemptRightCannotBeRevivedByRelease() async {
        for disposition in [WAReviewIntent.cancelled, .resolvedElsewhere, .unknown] {
            let store = WorkArrangementsReviewStore(scenario: .w7)
            store.loadIntentRecord(disposition, todoID: "wait-adapter")
            store.loadIntentRecord(disposition, todoID: "wait-tests")
            store.loadSharedPointRecord(.satisfied)
            XCTAssertFalse(store.requestWaitingLaunch("wait-adapter"), disposition.rawValue)
            XCTAssertNil(store.launchIntent)
        }
        let store = WorkArrangementsReviewStore(scenario: .w7)
        store.loadAttemptPermissionRecord(.failed, todoID: "wait-adapter")
        store.loadAttemptPermissionRecord(.unknown, todoID: "wait-tests")
        store.loadSharedPointRecord(.satisfied)
        XCTAssertFalse(store.requestWaitingLaunch("wait-adapter"))
        XCTAssertNil(store.launchIntent)
        XCTAssertFalse(store.requestWaitingLaunch("agent-note"), "资源释放不产生交办权")
    }

    @MainActor
    func testW7IndependentWorkCanAdvanceWhileOnlyConflictingItemsStayBlocked() async {
        let store = WorkArrangementsReviewStore(scenario: .w7)
        XCTAssertEqual(store.execution("independent-docs")?.usesRuleAuthority, false)
        XCTAssertFalse(store.requestWaitingLaunch("wait-adapter"))
        store.loadRunRecord(.ended, runID: "independent-docs")
        store.loadRunRecord(.safeClosure, runID: "independent-docs")
        XCTAssertEqual(store.execution("independent-docs")?.phase, .ended)
        XCTAssertEqual(store.execution("independent-docs")?.safety, .satisfied)
        XCTAssertEqual(store.execution("independent-docs")?.usesRuleAuthority, false)
        XCTAssertEqual(store.sharedPoint, .failed)
        XCTAssertEqual(store.todo("wait-adapter")?.wait, .sharedPoint)
        XCTAssertEqual(store.todo("wait-tests")?.wait, .sharedPoint)
        XCTAssertNil(store.launchIntent)
    }

    @MainActor
    func testW8UsageDeduplicatesPerRecordAndKeepsEstimateReportAndUnknownSeparate() async {
        let store = WorkArrangementsReviewStore(scenario: .w8)
        XCTAssertNil(store.usageSummary(scope: .shared).reported)
        store.loadUsageRecords()
        XCTAssertEqual(store.usage.count, 5)
        XCTAssertEqual(store.usageSummary(scope: .local), WAReviewUsageSummary(reported: 10, estimated: 5, unknownCount: 0))
        XCTAssertEqual(store.usageSummary(scope: .project), WAReviewUsageSummary(reported: 13, estimated: 5, unknownCount: 1))
        let once = store.usage
        store.loadUsageRecords()
        XCTAssertEqual(store.usage, once)
        XCTAssertEqual(store.usageSummary(scope: .shared).unknownCount, 1)
    }

    @MainActor
    func testW8LocalTopupNeverBypassesProjectOrSharedBoundaries() async {
        for permission in [WAReviewPermission.limited, .fullAccess] {
            let store = WorkArrangementsReviewStore(scenario: .w8)
            var draft = store.makeRuleDraft()
            draft.edited.permission = permission
            XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
            let run = WorkArrangementsReviewFixtures.activeRunID
            for limit in [WAReviewLimit.localBudget, .projectBudget, .sharedBudget] {
                store.loadLimitRecord(limit, value: .failed)
            }
            store.loadRunRecord(.safeClosure, runID: run)
            store.topUp(.local, continuingRunID: run)
            XCTAssertNil(store.launchIntent)
            XCTAssertEqual(store.limits[.projectBudget], .failed)
            XCTAssertEqual(store.limits[.sharedBudget], .failed)
            store.topUp(.project, continuingRunID: run)
            XCTAssertNil(store.launchIntent)
            XCTAssertEqual(store.limits[.sharedBudget], .failed)
            store.topUp(.shared, continuingRunID: run)
            XCTAssertEqual(store.launchIntent?.origin, .resourceContinuation)
            XCTAssertTrue(store.loadStartedRecord())
        }
    }

    @MainActor
    func testW8AdmissionExhaustionDoesNotKillLastCompliantAttemptButRuntimeLimitRequestsClosure() async {
        let store = WorkArrangementsReviewStore(scenario: .w8)
        let run = WorkArrangementsReviewFixtures.activeRunID
        let original = store.execution(run)
        let independent = store.execution("independent-docs")
        store.loadLimitRecord(.concurrency, value: .failed)
        store.loadLimitRecord(.attempts, value: .failed)
        XCTAssertEqual(store.execution(run), original)
        XCTAssertFalse(store.requestLaunch())
        store.loadLimitRecord(.localBudget, value: .failed)
        XCTAssertEqual(store.execution(run)?.phase, .stopRequested)
        XCTAssertEqual(store.execution(run)?.stopCause, .resource)
        XCTAssertEqual(store.execution(run)?.artifacts, original?.artifacts)
        XCTAssertEqual(store.execution("independent-docs"), independent)
        XCTAssertEqual(store.execution("independent-docs")?.usesRuleAuthority, false)
        XCTAssertFalse(store.isTechnicallyComplete("goal-interface"))
    }

    @MainActor
    func testW8OrdinaryTopupDoesNotRestartOldLimitClosureButKeepsOrdinaryWaitSeparate() async {
        let store = WorkArrangementsReviewStore(scenario: .w8)
        store.loadLimitRecord(.localBudget, value: .failed)
        store.loadRunRecord(.safeClosure, runID: WorkArrangementsReviewFixtures.activeRunID)
        store.topUp(.local)
        XCTAssertEqual(store.execution(WorkArrangementsReviewFixtures.activeRunID)?.phase, .safelyStopped)
        XCTAssertNotEqual(store.launchIntent?.origin, .resourceContinuation)
        XCTAssertEqual(store.todo("wait-adapter")?.wait, .ready)
        XCTAssertEqual(store.launchIntent?.origin, .waiting, "原获准普通等待不必再次确认")
    }

    @MainActor
    func testW8PreopenedAutomaticContinuationNeedsSafeOriginalAndCurrentAllLimits() async {
        let store = WorkArrangementsReviewStore(scenario: .w8)
        var draft = store.makeRuleDraft()
        draft.edited.autoContinueAfterLimit = true
        XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
        store.loadLimitRecord(.localBudget, value: .failed)
        store.topUp(.local)
        XCTAssertNil(store.launchIntent, "原执行及占用未核实，不造替代")
        store.loadRunRecord(.safeClosure, runID: WorkArrangementsReviewFixtures.activeRunID)
        XCTAssertEqual(store.launchIntent?.origin, .resourceContinuation)
        XCTAssertEqual(store.launchIntent?.continuingRunID, WorkArrangementsReviewFixtures.activeRunID)
        XCTAssertTrue(store.loadStartedRecord())
        XCTAssertEqual(store.execution(WorkArrangementsReviewFixtures.activeRunID)?.continued, true)
        XCTAssertEqual(store.executions.last?.basis, store.execution(WorkArrangementsReviewFixtures.activeRunID)?.basis)
    }

    @MainActor
    func testW8ExplicitTopupAndContinueRejectsUnknownOriginalDurationAttemptAndSupervisionGaps() async {
        for blocker in 0..<6 {
            let store = WorkArrangementsReviewStore(scenario: .w8)
            let run = WorkArrangementsReviewFixtures.activeRunID
            store.loadLimitRecord(.localBudget, value: .failed)
            if blocker != 0 { store.loadRunRecord(.safeClosure, runID: run) }
            switch blocker {
            case 0: store.loadRunRecord(.unknown, runID: run)
            case 1: store.loadLimitRecord(.duration, value: .failed)
            case 2: store.loadLimitRecord(.attempts, value: .failed)
            case 3: store.loadGateRecord(.supervision, value: .unanswered)
            case 4: store.loadGateRecord(.skillConsent, value: .unanswered)
            default: store.stopRule()
            }
            store.topUp(.local, continuingRunID: run)
            XCTAssertNotEqual(store.launchIntent?.origin, .resourceContinuation, "blocker \(blocker)")
            XCTAssertNotEqual(store.launchIntent?.continuingRunID, run)
            XCTAssertEqual(store.execution(run)?.continued, false)
            if blocker == 1 {
                XCTAssertEqual(store.launchIntent?.origin, .waiting, "独立时长只限原执行，普通等待不是旧执行续行")
            } else {
                XCTAssertFalse(store.loadStartedRecord())
            }
            XCTAssertEqual(store.limits[.localBudget], .satisfied)
            if blocker == 1 { XCTAssertEqual(store.limits[.duration], .failed) }
            if blocker == 2 { XCTAssertEqual(store.limits[.attempts], .failed) }
            if blocker == 5 { XCTAssertTrue(store.rule.stopped) }
        }
        let allowed = WorkArrangementsReviewStore(scenario: .w8)
        allowed.loadLimitRecord(.localBudget, value: .failed)
        allowed.loadRunRecord(.safeClosure, runID: WorkArrangementsReviewFixtures.activeRunID)
        allowed.topUp(.local, continuingRunID: WorkArrangementsReviewFixtures.activeRunID)
        XCTAssertEqual(allowed.launchIntent?.origin, .resourceContinuation)
        XCTAssertTrue(allowed.loadStartedRecord())
    }

    @MainActor
    func testW8FullAccessCannotBypassHardCapabilityAndNewTriggerIsNotOldContinuation() async {
        let store = WorkArrangementsReviewStore(scenario: .w8)
        var draft = store.makeRuleDraft()
        draft.edited.permission = .fullAccess
        XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
        store.loadGateRecord(.hardLimitCapability, value: .failed)
        XCTAssertFalse(store.requestLaunch())
        store.loadGateRecord(.hardLimitCapability, value: .satisfied)
        store.loadRunRecord(.safeClosure, runID: WorkArrangementsReviewFixtures.activeRunID)
        store.loadScheduledRecord()
        XCTAssertEqual(store.launchIntent?.origin, .scheduled)
        XCTAssertNil(store.launchIntent?.continuingRunID)
        XCTAssertTrue(store.loadStartedRecord())
    }

    @MainActor
    func testAllCurrentConditionFailuresUnknownsAndUnansweredStatesBlockEvenFullAccess() async {
        for gate in WAReviewGate.allCases {
            for value in [WAReviewTruth.failed, .unknown, .unanswered] {
                let store = WorkArrangementsReviewStore(scenario: .w5)
                var draft = store.makeRuleDraft()
                draft.edited.permission = .fullAccess
                XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
                store.loadGateRecord(gate, value: value)
                XCTAssertFalse(store.requestLaunch(), "\(gate.rawValue): \(value.rawValue)")
                XCTAssertNil(store.launchIntent)
                XCTAssertFalse(store.loadStartedRecord())
                XCTAssertEqual(store.executions.count, 1)
            }
        }
        for limit in WAReviewLimit.allCases where limit != .duration {
            for value in [WAReviewTruth.failed, .unknown, .unanswered] {
                for permission in [WAReviewPermission.limited, .fullAccess] {
                    let store = WorkArrangementsReviewStore(scenario: .w5)
                    var draft = store.makeRuleDraft()
                    draft.edited.permission = permission
                    XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
                    store.loadLimitRecord(limit, value: value)
                    XCTAssertFalse(store.requestLaunch(), "\(limit.rawValue): \(value.rawValue), \(permission.rawValue)")
                    XCTAssertNil(store.launchIntent)
                }
            }
        }
    }

    @MainActor
    func testValidPreparedIntentIsRecheckedWhenDependencyOrSupervisionLaterDeteriorates() async {
        for gate in [WAReviewGate.dependency, .supervision] {
            for value in [WAReviewTruth.failed, .unknown, .unanswered] {
                let store = WorkArrangementsReviewStore(scenario: .w5)
                XCTAssertTrue(store.requestLaunch())
                XCTAssertNotNil(store.launchIntent)
                let runCount = store.executions.count
                store.loadGateRecord(gate, value: value)
                XCTAssertNotNil(store.launchIntent, "未由此记录提前丢弃，确实覆盖载入开始时再次核对")
                XCTAssertFalse(store.loadStartedRecord(), "\(gate.rawValue): \(value.rawValue)")
                XCTAssertNil(store.launchIntent)
                XCTAssertEqual(store.executions.count, runCount)
            }
        }
    }

    @MainActor
    func testW7ReleaseNeverClearsPauseStopOrMakesSkippedTriggersAQueue() async {
        let waiting = WorkArrangementsReviewStore(scenario: .w7)
        waiting.pauseRule(); waiting.stopRule()
        waiting.loadSharedPointRecord(.satisfied)
        XCTAssertTrue(waiting.rule.paused)
        XCTAssertTrue(waiting.rule.stopped)
        XCTAssertNil(waiting.launchIntent)
        XCTAssertFalse(waiting.requestWaitingLaunch("wait-adapter"))
        let skipped = WorkArrangementsReviewStore(scenario: .w5)
        skipped.loadMissedRecords()
        let original = skipped.opportunities
        skipped.loadSharedPointRecord(.satisfied)
        skipped.topUp(.shared)
        XCTAssertEqual(skipped.opportunities, original)
        XCTAssertNil(skipped.launchIntent)
        XCTAssertTrue(skipped.todos.allSatisfy { !$0.allowWaiting })
    }

    @MainActor
    func testW8LateAutoContinueAndOrdinaryManualStartCannotReopenLimitedOldWork() async {
        let store = WorkArrangementsReviewStore(scenario: .w8)
        let run = WorkArrangementsReviewFixtures.activeRunID
        store.loadLimitRecord(.localBudget, value: .failed)
        store.loadRunRecord(.safeClosure, runID: run)
        var draft = store.makeRuleDraft()
        draft.edited.autoContinueAfterLimit = true
        XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
        store.topUp(.local)
        XCTAssertNotEqual(store.launchIntent?.origin, .resourceContinuation, "晚开的策略不冒充事先同意")
        XCTAssertFalse(store.requestLaunch(todoID: WorkArrangementsReviewFixtures.primaryTodoID))
        XCTAssertEqual(store.execution(run)?.continued, false)
        store.topUp(.local, continuingRunID: run)
        XCTAssertEqual(store.launchIntent?.origin, .resourceContinuation, "仍可另明确补额并继续")
    }

    @MainActor
    func testW8OldExecutionDurationDoesNotFreezeAnOtherwiseLawfulNewTrigger() async {
        let store = WorkArrangementsReviewStore(scenario: .w8)
        let run = WorkArrangementsReviewFixtures.activeRunID
        store.loadLimitRecord(.duration, value: .failed)
        store.loadRunRecord(.safeClosure, runID: run)
        store.topUp(.local, continuingRunID: run)
        XCTAssertNotEqual(store.launchIntent?.origin, .resourceContinuation)
        XCTAssertEqual(store.limits[.duration], .failed, "补额不清原执行独立时长")
        store.loadScheduledRecord()
        XCTAssertEqual(store.launchIntent?.origin, .scheduled)
        XCTAssertNil(store.launchIntent?.continuingRunID)
        XCTAssertTrue(store.loadStartedRecord())
        XCTAssertEqual(store.execution(run)?.continued, false)
    }

    @MainActor
    func testW8ChangedRuleConfigurationCannotSilentlyRebindOrRestartOldLimitWork() async {
        for permission in [WAReviewPermission.limited, .fullAccess] {
            for automatic in [false, true] {
                let store = WorkArrangementsReviewStore(scenario: .w8)
                let run = WorkArrangementsReviewFixtures.activeRunID
                let originalBasis = store.execution(run)?.basis
                var initial = store.makeRuleDraft()
                initial.edited.permission = permission
                initial.edited.autoContinueAfterLimit = automatic
                XCTAssertTrue(store.applyRuleDraft(initial, confirmingPolicy: true))
                store.loadLimitRecord(.localBudget, value: .failed)
                XCTAssertEqual(store.execution(run)?.autoContinueGrantedBeforeLimit, automatic)
                store.loadRunRecord(.safeClosure, runID: run)
                var draft = store.makeRuleDraft()
                draft.edited.configuration = .b
                XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
                if automatic {
                    store.topUp(.local)
                } else {
                    store.topUp(.local, continuingRunID: run)
                }
                XCTAssertNotEqual(store.launchIntent?.origin, .resourceContinuation)
                let continuation = WAReviewLaunchIntent(
                    todoID: WorkArrangementsReviewFixtures.primaryTodoID,
                    origin: .resourceContinuation, continuingRunID: run
                )
                XCTAssertEqual(store.blockers(for: continuation), [
                    "当前规则配置/API 依据已变化；旧执行续行须先明确对齐，不静默换绑或借旧配置绕过新策略",
                ], "不因其他守卫偶然拒绝而掩盖配置变化检查")
                XCTAssertEqual(store.execution(run)?.basis, originalBasis)
                XCTAssertEqual(store.execution(run)?.continued, false)
                for origin in [WAReviewLaunchOrigin.manual, .waiting] {
                    let reopen = WAReviewLaunchIntent(
                        todoID: WorkArrangementsReviewFixtures.primaryTodoID,
                        origin: origin, continuingRunID: nil
                    )
                    XCTAssertTrue(store.blockers(for: reopen).contains(
                        "到限旧工作须独立续行意图；不能借普通启动/等待重开"
                    ))
                }
                XCTAssertFalse(store.requestLaunch(todoID: WorkArrangementsReviewFixtures.primaryTodoID))
                XCTAssertFalse(store.requestWaitingLaunch(WorkArrangementsReviewFixtures.primaryTodoID))
                XCTAssertEqual(store.execution(run)?.basis, originalBasis)
                store.loadScheduledRecord()
                XCTAssertEqual(store.launchIntent?.origin, .scheduled)
                XCTAssertTrue(store.loadStartedRecord())
                XCTAssertEqual(store.executions.last?.basis.configuration, .b)
                XCTAssertEqual(store.executions.last?.basis.configuration.apiAlias, WAReviewConfiguration.b.apiAlias)
                XCTAssertEqual(store.execution(run)?.basis, originalBasis)
                XCTAssertEqual(store.execution(run)?.continued, false)
            }
        }
    }

    @MainActor
    func testW8IndependentContinuationRetainsOriginalConfigurationWhenUnrelatedRuleChanges() async {
        for editBeforeLimit in [true, false] {
            let store = WorkArrangementsReviewStore(scenario: .w2)
            XCTAssertTrue(store.assignTodo("agent-note"))
            XCTAssertTrue(store.requestLaunch(todoID: "agent-note"))
            XCTAssertTrue(store.loadStartedRecord())
            let run = store.executions.last?.id ?? ""
            let originalBasis = store.execution(run)?.basis
            XCTAssertEqual(originalBasis?.configuration, .a)
            XCTAssertEqual(store.execution(run)?.usesRuleAuthority, false)
            if !editBeforeLimit {
                store.loadLimitRecord(.localBudget, value: .failed)
                store.loadRunRecord(.safeClosure, runID: run)
            }
            var draft = store.makeRuleDraft()
            draft.edited.configuration = .b
            XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: false))
            if editBeforeLimit {
                XCTAssertEqual(store.execution(run)?.phase, .running)
                store.loadLimitRecord(.localBudget, value: .failed)
                store.loadRunRecord(.safeClosure, runID: run)
            }
            store.topUp(.local, continuingRunID: run)
            XCTAssertEqual(store.launchIntent?.origin, .resourceContinuation)
            XCTAssertEqual(store.execution(run)?.continued, false, "意图尚不是已开始记录")
            XCTAssertTrue(store.loadStartedRecord())
            XCTAssertEqual(store.execution(run)?.continued, true)
            XCTAssertEqual(store.executions.last?.basis, originalBasis)
            XCTAssertEqual(store.executions.last?.basis.configuration.apiAlias, WAReviewConfiguration.a.apiAlias)
            XCTAssertEqual(store.executions.last?.usesRuleAuthority, false)
            XCTAssertFalse(store.rule.enabled)
            XCTAssertFalse(store.rule.policyConfirmed)
            XCTAssertEqual(store.rule.policy.permission, .unanswered)
            XCTAssertEqual(store.rule.policy.configuration, .b)
        }
    }

    @MainActor
    func testW8RuleEditInvalidatesPreviouslyPreparedContinuationBeforeStartedRecord() async {
        let store = WorkArrangementsReviewStore(scenario: .w8)
        let run = WorkArrangementsReviewFixtures.activeRunID
        let originalBasis = store.execution(run)?.basis
        store.loadLimitRecord(.localBudget, value: .failed)
        store.loadRunRecord(.safeClosure, runID: run)
        store.topUp(.local, continuingRunID: run)
        XCTAssertEqual(store.launchIntent?.origin, .resourceContinuation)
        let count = store.executions.count
        var draft = store.makeRuleDraft()
        draft.edited.configuration = .b
        XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
        XCTAssertNil(store.launchIntent, "保存新依据使旧待启动意图失效")
        XCTAssertFalse(store.loadStartedRecord())
        XCTAssertEqual(store.executions.count, count)
        store.topUp(.local, continuingRunID: run)
        XCTAssertNotEqual(store.launchIntent?.origin, .resourceContinuation)
        XCTAssertEqual(store.executions.count, count)
        XCTAssertEqual(store.execution(run)?.basis, originalBasis)
        XCTAssertEqual(store.execution(run)?.continued, false)
    }

    @MainActor
    func testW8ResourceRecordsCannotDowngradeUserStopOrRevocationIntoAutomaticContinuation() async {
        for userStop in [true, false] {
            let store = WorkArrangementsReviewStore(scenario: .w8)
            var draft = store.makeRuleDraft()
            draft.edited.autoContinueAfterLimit = true
            XCTAssertTrue(store.applyRuleDraft(draft, confirmingPolicy: true))
            if userStop { store.stopRule() } else { store.loadRevocationRecord() }
            store.loadLimitRecord(.localBudget, value: .failed)
            let run = WorkArrangementsReviewFixtures.activeRunID
            XCTAssertEqual(store.execution(run)?.stopCause, userStop ? .user : .permission)
            store.loadRunRecord(.safeClosure, runID: run)
            store.topUp(.local, continuingRunID: run)
            XCTAssertNotEqual(store.launchIntent?.origin, .resourceContinuation)
        }
    }

    @MainActor
    func testResetGenerationCrossWindowDraftsAndAllPresentationStateStayLocal() async {
        let first = WorkArrangementsReviewStore(scenario: .w2)
        let second = WorkArrangementsReviewStore(scenario: .w2)
        let oldRuleDraft = first.makeRuleDraft()
        let oldBatch = first.makeBatchDraft(kind: .assignment)
        var oldNew = first.makeNewTodoDraft()
        oldNew.title = "不可跨窗口应用"
        XCTAssertFalse(second.applyRuleDraft(oldRuleDraft, confirmingPolicy: true))
        XCTAssertFalse(second.confirmBatch(oldBatch))
        XCTAssertFalse(second.saveNewTodo(oldNew))
        let secondRule = second.rule
        let secondTodos = second.todos
        first.classicAppearance = true; first.advancedPresentation = true
        first.demonstrateReducedTransparency = true
        first.demonstrateIncreasedContrast = true
        first.demonstrateReducedMotion = true
        XCTAssertEqual(first.rule, secondRule, "外观不改权限/启用")
        first.loadUsageRecords()
        first.pauseRule(); first.stopRule()
        XCTAssertEqual(second.rule, secondRule)
        XCTAssertEqual(second.todos, secondTodos)
        XCTAssertTrue(second.events.isEmpty)
        XCTAssertTrue(second.usage.isEmpty)
        first.reset()
        XCTAssertFalse(first.applyRuleDraft(oldRuleDraft, confirmingPolicy: true))
        XCTAssertFalse(first.confirmBatch(oldBatch))
        XCTAssertFalse(first.saveNewTodo(oldNew))
        XCTAssertEqual(first.rule, secondRule)
        XCTAssertEqual(first.todos, secondTodos)
        XCTAssertFalse(first.classicAppearance)
        XCTAssertFalse(first.advancedPresentation)
        XCTAssertFalse(first.demonstrateReducedTransparency)
        XCTAssertFalse(first.demonstrateIncreasedContrast)
        XCTAssertFalse(first.demonstrateReducedMotion)
        XCTAssertTrue(first.events.isEmpty)
        XCTAssertTrue(first.usage.isEmpty)
        let fresh = WorkArrangementsReviewStore(scenario: .w2)
        XCTAssertEqual(fresh.rule, secondRule)
        XCTAssertEqual(fresh.todos, secondTodos)
    }
}
#endif
