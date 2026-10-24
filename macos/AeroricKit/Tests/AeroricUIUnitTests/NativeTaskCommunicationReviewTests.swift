#if DEBUG
import Foundation
import XCTest
@testable import AeroricUI

/// Only finite value/Store transitions, injected entry decisions and temporary
/// directory metadata. No View/App/window assembly, defaults, sockets or agents.
final class NativeTaskCommunicationReviewTests: XCTestCase {
    private let otherKeys = [
        "AERORIC_NATIVE_WORK_ARRANGEMENTS_REVIEW", "AERORIC_NATIVE_SKILL_REVIEW",
        "AERORIC_NATIVE_KNOWLEDGE_REVIEW", "AERORIC_NATIVE_EVOLUTION_REVIEW",
        "AERORIC_NATIVE_VISUAL_REVIEW", "AERORIC_NATIVE_COLLABORATION_REVIEW",
    ]

    func testAbsentFlagLeavesAllOldKeyValueCombinationsInactive() throws {
        try withDirectoryFixture { fixture in
            let values: [String?] = [nil, "1", "0", "", "invalid"]
            for base in [[:], ["AERORIC_ENV": "wrong", "AERORIC_DATA_DIR": "relative"]] {
                for combination in 0..<15_625 { // 5^6, including all absent.
                    var environment = base
                    var digits = combination
                    for key in otherKeys {
                        environment[key] = values[digits % values.count]
                        digits /= values.count
                    }
                    XCTAssertEqual(decide(environment, fixture), .inactive)
                }
            }
        }
    }

    func testPresentInvalidFlagAndEnvironmentNeverFallBackToProduct() throws {
        try withDirectoryFixture { fixture in
            for value in ["0", "", "true", "2", "01", " 1", "1\n"] {
                var environment = fixture.environment
                environment[NativeTaskCommunicationReviewEntry.flagKey] = value
                XCTAssertEqual(decide(environment, fixture), .refused([.invalidFlag]))
                environment["AERORIC_ENV"] = "wrong"
                for key in otherKeys { environment[key] = "1" }
                XCTAssertEqual(decide(environment, fixture), .refused([
                    .invalidFlag, .invalidEnvironment, .conflictingReview,
                ]))
            }
            let invalidEnvironments: [String?] = [nil, "", "default", "production", "UI-TEST", "ui-test "]
            for value in invalidEnvironments {
                var environment = fixture.environment
                environment["AERORIC_ENV"] = value
                XCTAssertEqual(decide(environment, fixture), .refused([.invalidEnvironment]))
            }
        }
    }

    func testAllSixtyThreeConflictsAndZeroNonconflicts() throws {
        try withDirectoryFixture { fixture in
            for mask in 1..<64 {
                var environment = fixture.environment
                for (index, key) in otherKeys.enumerated() {
                    environment[key] = mask & (1 << index) == 0 ? "0" : "1"
                }
                XCTAssertEqual(decide(environment, fixture), .refused([.conflictingReview]), "mask \(mask)")
            }
            var environment = fixture.environment
            for key in otherKeys { environment[key] = "0" }
            XCTAssertEqual(decide(environment, fixture), .review)
            for key in otherKeys {
                var single = fixture.environment
                single[key] = "1"
                XCTAssertEqual(decide(single, fixture), .refused([.conflictingReview]), key)
            }
        }
    }

    func testDirectoryMetadataRejectsUnsafePathsWithoutCreatingCandidate() throws {
        try withDirectoryFixture { fixture in
            let fm = FileManager.default
            let existing = fixture.caches.appendingPathComponent("existing", isDirectory: true)
            try fm.createDirectory(at: existing, withIntermediateDirectories: false)
            let plainFile = fixture.caches.appendingPathComponent("plain-file")
            XCTAssertTrue(fm.createFile(atPath: plainFile.path, contents: Data()))
            let candidateLink = fixture.caches.appendingPathComponent("candidate-link")
            let ancestorLink = fixture.caches.appendingPathComponent("ancestor-link")
            let dangling = fixture.caches.appendingPathComponent("dangling")
            try fm.createSymbolicLink(at: candidateLink, withDestinationURL: existing)
            try fm.createSymbolicLink(at: ancestorLink, withDestinationURL: existing)
            try fm.createSymbolicLink(at: dangling, withDestinationURL: fixture.root.appendingPathComponent("missing"))
            let before = try snapshot(fixture.root)
            let unsafe: [String?] = [
                nil, "", "relative", fixture.caches.path, fixture.root.path,
                fixture.caches.path + "/.", fixture.caches.path + "/existing/../candidate",
                fixture.caches.path + "-collision/child", " " + fixture.candidate.path,
                plainFile.path, plainFile.path + "/child", candidateLink.path,
                ancestorLink.path + "/not-created", dangling.path, dangling.path + "/child",
            ]
            for directory in unsafe {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = directory
                XCTAssertEqual(decide(environment, fixture), .refused([.invalidDirectory]))
            }
            XCTAssertEqual(decide(fixture.environment, fixture), .review)
            var environment = fixture.environment
            environment["AERORIC_DATA_DIR"] = existing.path
            XCTAssertEqual(decide(environment, fixture), .review)
            XCTAssertFalse(fm.fileExists(atPath: fixture.candidate.path))
            let after = try snapshot(fixture.root)
            XCTAssertEqual(before, after)
            print("TC_ENTRY_METADATA before=\(before) after=\(after); candidate-not-created=true")
        }
    }

    // Async adapters avoid XCTest's synchronous @MainActor discovery casts.
    @MainActor
    func testUnshownLifecycleMetadataIsIsolatedAndInert() async {
        let review = NativeTaskCommunicationReviewWindowLifecycle(decision: .review)
        let refusal = NativeTaskCommunicationReviewWindowLifecycle(decision: .refused([.invalidFlag]))
        let inactive = NativeTaskCommunicationReviewWindowLifecycle(decision: .inactive)
        XCTAssertEqual(review.windowIdentifier?.rawValue, "native-task-communication-review")
        XCTAssertEqual(refusal.windowIdentifier?.rawValue, "native-task-communication-review-refusal")
        XCTAssertNil(inactive.windowIdentifier)
        XCTAssertEqual(inactive.windowTitle, "")
        XCTAssertFalse(review.isWindowCreated)
        XCTAssertFalse(refusal.isWindowCreated)
        XCTAssertFalse(inactive.isWindowCreated)
        // Do not call launch/show/reopen/close, construct a View or touch NSApp.
    }

    func testFixturesHaveEightFiniteScenesAndDistinctFacts() {
        XCTAssertEqual(TCReviewScenario.allCases.count, 8)
        for scenario in TCReviewScenario.allCases {
            let fixture = TaskCommunicationReviewFixtures.make(scenario)
            XCTAssertEqual(fixture.conditions.count, TCReviewMember.allCases.count)
            XCTAssertEqual(Set(fixture.messages.map(\.id)).count, fixture.messages.count)
            XCTAssertFalse(scenario.instruction.isEmpty)
        }
        let delivery = TaskCommunicationReviewFixtures.make(.delivery)
        XCTAssertEqual(delivery.messages.map(\.delivery), [.handedOff, .unknown])
        XCTAssertFalse(delivery.messages.contains(where: \.ownerConfirmed))
        let questions = TaskCommunicationReviewFixtures.make(.questions).questions
        XCTAssertEqual(Set(questions.map(\.asker)).count, 2)
        XCTAssertTrue(questions.allSatisfy { !$0.recommendationReason.isEmpty && !$0.impact.isEmpty })
    }

    func testAccessibilityDemonstrationOnlyStrengthensSystemValues() {
        var appearance = TCReviewAppearance()
        let system = TCReviewAccessibility(
            appearance: appearance, systemTransparency: true, systemContrast: true, systemMotion: true)
        XCTAssertEqual(system.surface, .classic)
        XCTAssertTrue(system.reduceTransparency && system.increaseContrast && system.reduceMotion)
        appearance.reduceTransparency = true
        appearance.increaseContrast = true
        appearance.reduceMotion = true
        let demo = TCReviewAccessibility(
            appearance: appearance, systemTransparency: false, systemContrast: false, systemMotion: false)
        XCTAssertEqual(demo, system)
    }

    @MainActor
    func testTC1DefaultCurrentIntentAndExplicitNextRoundRecipient() async throws {
        let store = TaskCommunicationReviewStore()
        XCTAssertEqual(store.draft.purpose, .current)
        XCTAssertEqual(store.draft.recipient, .coordinator)
        store.setDraftText(" \n\t ")
        XCTAssertFalse(store.submitDraft())
        XCTAssertTrue(store.messages.isEmpty)
        let current = try send("  保留错误说明。\n", in: store)
        XCTAssertEqual(current.text, "保留错误说明。")
        XCTAssertEqual(current.recipient, .coordinator)
        XCTAssertEqual(current.purpose, .current)
        XCTAssertEqual(current.intent, .pendingCurrent)
        XCTAssertEqual(current.delivery, .notHandedOff)
        XCTAssertFalse(current.consumed)
        XCTAssertTrue(store.continuationBlockers(.coordinator).contains { $0.contains("能力不明") })
        XCTAssertTrue(store.setPurpose(.nextRound))
        XCTAssertTrue(store.setRecipient(.documentation))
        let next = try send("下一轮独立整理术语。", in: store)
        XCTAssertEqual(next.recipient, .documentation)
        XCTAssertEqual(next.intent, .waitingNextRound)
        XCTAssertEqual(next.delivery, .notHandedOff)
        XCTAssertEqual(store.message(current.id), current)
    }

    @MainActor
    func testTC1ReplyKeepsOriginalQuestionDespiteViewedMemberAndCancel() async throws {
        let store = TaskCommunicationReviewStore()
        let originalConditions = store.conditions
        XCTAssertTrue(store.beginReply("q-interface"))
        XCTAssertEqual(store.draft.text, "") // Recommendation is never preselected.
        store.setDraftText("明确采用 code / message。")
        store.viewedMember = .documentation
        XCTAssertFalse(store.setRecipient(.documentation))
        XCTAssertFalse(store.setPurpose(.nextRound))
        XCTAssertEqual(store.draft.recipient, .interface)
        let cancelled = store.draft
        store.cancelComposer()
        XCTAssertNil(store.answer(to: "q-interface"))
        XCTAssertFalse(store.submitDraft(cancelled))
        XCTAssertTrue(store.beginReply("q-interface"))
        store.viewedMember = .governance
        let answer = try send("明确采用 code / message。", in: store)
        XCTAssertEqual(answer.questionID, "q-interface")
        XCTAssertEqual(answer.recipient, .interface)
        XCTAssertEqual(answer.purpose, .answer)
        XCTAssertEqual(answer.delivery, .notHandedOff)
        XCTAssertNil(store.question("q-interface")?.availableEvidence)
        XCTAssertEqual(store.conditions, originalConditions)
        XCTAssertFalse(store.beginReply("q-interface")) // No blind duplicate.
    }

    @MainActor
    func testTC1ChangingContextNeverSilentlyDiscardsDraft() async {
        let store = TaskCommunicationReviewStore(scenario: .editing)
        store.setDraftText("未提交的文字")
        let draft = store.draft
        XCTAssertFalse(store.beginReply("q-interface"))
        XCTAssertFalse(store.beginEdit(1))
        XCTAssertFalse(store.beginCorrection(1))
        XCTAssertEqual(store.draft, draft)
        store.cancelComposer()
        XCTAssertFalse(store.setPurpose(.answer))
        XCTAssertEqual(store.draft.context, .compose)
        XCTAssertTrue(store.messages.count == 1)
    }

    @MainActor
    func testCapturedDraftCannotUseStaleTextOrRecipientWithinSameComposer() async {
        let store = TaskCommunicationReviewStore()
        store.setDraftText("旧草稿")
        let beforeRecipientChange = store.draft
        store.setRecipient(.documentation)
        XCTAssertFalse(store.submitDraft(beforeRecipientChange))
        let beforeTextChange = store.draft
        store.setDraftText("当前草稿")
        XCTAssertFalse(store.submitDraft(beforeTextChange))
        XCTAssertTrue(store.messages.isEmpty)
        XCTAssertTrue(store.submitDraft())
        XCTAssertEqual(store.messages.last?.text, "当前草稿")
        XCTAssertEqual(store.messages.last?.recipient, .documentation)
    }

    @MainActor
    func testTC2EditCancelCommitAndWithdrawalPreserveHistory() async throws {
        let store = TaskCommunicationReviewStore(scenario: .editing)
        let original = try XCTUnwrap(store.message(1))
        XCTAssertTrue(store.beginEdit(1))
        store.setDraftText("不应提交")
        let cancelled = store.draft
        store.cancelComposer()
        XCTAssertEqual(store.message(1), original)
        XCTAssertFalse(store.submitDraft(cancelled))
        XCTAssertTrue(store.beginEdit(1))
        store.setDraftText("确认仍可控时的新文字")
        let beforeCommit = store.draft
        XCTAssertTrue(store.submitDraft())
        XCTAssertEqual(store.messages.count, 1)
        XCTAssertEqual(store.message(1)?.text, "确认仍可控时的新文字")
        XCTAssertEqual(store.message(1)?.revision, original.revision + 1)
        XCTAssertEqual(store.message(1)?.delivery, .notHandedOff)
        XCTAssertFalse(store.submitDraft(beforeCommit))
        XCTAssertTrue(store.withdraw(1))
        XCTAssertEqual(store.messages.count, 1)
        XCTAssertEqual(store.message(1)?.text, "确认仍可控时的新文字")
        XCTAssertEqual(store.message(1)?.withdrawn, true)
        XCTAssertEqual(store.message(1)?.ownerConfirmed, false)
        XCTAssertFalse(store.withdraw(1))
        XCTAssertFalse(store.loadMessageEvent(.handedOff, id: 1))
    }

    @MainActor
    func testTC2HandoffAndUnknownRacesRejectEditAndWithdrawThenAllowExplicitCorrection() async throws {
        for event in [TCReviewMessageEvent.handedOff, .deliveryUnknown] {
            let store = TaskCommunicationReviewStore(scenario: .editing)
            let original = try XCTUnwrap(store.message(1))
            XCTAssertTrue(store.beginEdit(1))
            store.setDraftText("保留这段更正文字")
            XCTAssertTrue(store.loadMessageEvent(event, id: 1))
            let afterEvent = try XCTUnwrap(store.message(1))
            XCTAssertFalse(store.submitDraft())
            XCTAssertFalse(store.withdraw(1))
            XCTAssertEqual(store.message(1), afterEvent)
            XCTAssertTrue(store.canConvertEditToCorrection)
            XCTAssertTrue(store.convertEditToCorrection())
            XCTAssertEqual(store.draft.text, "保留这段更正文字")
            XCTAssertTrue(store.submitDraft())
            let correction = try XCTUnwrap(store.messages.last)
            XCTAssertEqual(correction.correctionOf, 1)
            XCTAssertEqual(correction.delivery, .notHandedOff)
            XCTAssertEqual(correction.recipient, original.recipient)
            XCTAssertEqual(store.message(1)?.text, original.text)
            XCTAssertEqual(store.message(1)?.source, original.source)
            XCTAssertEqual(store.message(1)?.delivery, afterEvent.delivery)
            XCTAssertEqual(store.message(1)?.operation, afterEvent.operation)
            XCTAssertEqual(store.message(1)?.supersededBy, correction.id)
            XCTAssertEqual(store.message(1)?.withdrawn, false)
        }
    }

    @MainActor
    func testTC3CorrectionsPreserveHandoffUnknownAndOperationFacts() async throws {
        let store = TaskCommunicationReviewStore(scenario: .delivery)
        for id in [1, 2] {
            let original = try XCTUnwrap(store.message(id))
            XCTAssertFalse(store.beginEdit(id))
            XCTAssertFalse(store.withdraw(id))
            XCTAssertTrue(store.beginCorrection(id))
            let correction = try send("仅更正 M\(id) 的要求。", in: store)
            XCTAssertEqual(correction.correctionOf, id)
            XCTAssertEqual(correction.scope, original.scope)
            XCTAssertEqual(store.message(id)?.delivery, original.delivery)
            XCTAssertEqual(store.message(id)?.operation, original.operation)
            XCTAssertEqual(store.message(id)?.sourceDetail, original.sourceDetail)
            XCTAssertEqual(store.message(id)?.text, original.text)
            XCTAssertEqual(store.message(id)?.consumed, false)
            XCTAssertFalse(store.loadMessageEvent(.consumed, id: correction.id))
            XCTAssertTrue(store.loadMessageEvent(.consumed, id: id))
            XCTAssertEqual(store.message(id)?.consumed, true)
            XCTAssertEqual(store.message(id)?.delivery, original.delivery)
            XCTAssertEqual(store.message(id)?.operation, original.operation)
            XCTAssertTrue(store.message(id)?.consumptionSummary.contains("仍无证据") == true)
        }
        XCTAssertEqual(store.messages.count, 4) // Append corrections, never replay originals.
        XCTAssertTrue(store.continuationBlockers(.coordinator).contains { $0.contains("先核实") })
        XCTAssertFalse(store.loadMessageEvent(.deliveryUnknown, id: 1)) // Cannot erase known handoff.
    }

    @MainActor
    func testTC4SharedCorrectionTracksEachAffectedMemberWithoutBroadcast() async throws {
        let store = TaskCommunicationReviewStore(scenario: .targeting)
        let original = try XCTUnwrap(store.message(1))
        XCTAssertTrue(store.beginCorrection(1))
        let correction = try send("共享接口明确改为 newKey；只核对相关调用方。", in: store)
        XCTAssertEqual(correction.recipient, .interface)
        XCTAssertEqual(store.alignments.map(\.member), [.coordinator, .interface, .verification])
        XCTAssertTrue(store.alignments.allSatisfy { !$0.handedOff && !$0.consumed })
        XCTAssertFalse(store.alignments.contains { $0.member == .documentation || $0.member.isReadOnly })
        let coordinator = try XCTUnwrap(store.alignments.first { $0.member == .coordinator })
        XCTAssertFalse(store.loadAlignmentEvent(coordinator.id, consumed: true))
        XCTAssertTrue(store.loadAlignmentEvent(coordinator.id))
        XCTAssertTrue(store.loadAlignmentEvent(coordinator.id, consumed: true))
        XCTAssertTrue(store.alignments.filter { $0.member != .coordinator }.allSatisfy {
            !$0.handedOff && !$0.consumed
        })
        XCTAssertEqual(store.message(correction.id)?.delivery, .notHandedOff)
        XCTAssertTrue(store.loadMessageEvent(.handedOff, id: correction.id))
        XCTAssertEqual(store.alignments.first { $0.member == .interface }?.handedOff, true)
        XCTAssertEqual(store.alignments.first { $0.member == .interface }?.consumed, false)
        XCTAssertEqual(store.alignments.first { $0.member == .verification }?.handedOff, false)
        XCTAssertEqual(store.message(1)?.text, original.text)
        XCTAssertEqual(store.message(1)?.delivery, original.delivery)
        XCTAssertTrue(store.workSummary(.documentation).contains("安全独立工作可继续"))
    }

    @MainActor
    func testTC4TargetingReadOnlyMemberDoesNotGrantAuthorityOrRelabelSources() async throws {
        let store = TaskCommunicationReviewStore(scenario: .targeting)
        let conditions = store.conditions
        let machine = try XCTUnwrap(store.message(2))
        let relay = try XCTUnwrap(store.message(3))
        XCTAssertEqual(machine.source, .machine)
        XCTAssertEqual(relay.source, .relay)
        for member in [TCReviewMember.reflection, .governance] {
            XCTAssertTrue(store.setRecipient(member))
            let message = try send("请提供只读意见。", in: store)
            XCTAssertEqual(message.recipient, member)
            XCTAssertTrue(member.roleBoundary.contains("不写代码"))
        }
        XCTAssertEqual(store.conditions, conditions)
        XCTAssertEqual(store.message(2), machine)
        XCTAssertEqual(store.message(3), relay)
        XCTAssertTrue(store.alignments.isEmpty)
    }

    @MainActor
    func testTC4SupersededAlignmentDoesNotStartAnObsoleteSyncButKeepsKnownFacts() async throws {
        let store = TaskCommunicationReviewStore(scenario: .targeting)
        XCTAssertTrue(store.beginCorrection(1))
        let first = try send("共享接口明确采用 firstKey。", in: store)
        let coordinator = try XCTUnwrap(store.alignments.first { $0.member == .coordinator })
        let verification = try XCTUnwrap(store.alignments.first { $0.member == .verification })
        XCTAssertTrue(store.loadAlignmentEvent(coordinator.id))
        XCTAssertTrue(store.beginCorrection(first.id))
        let second = try send("再次明确：共享接口只采用 finalKey。", in: store)
        XCTAssertFalse(store.loadMessageEvent(.handedOff, id: first.id))
        XCTAssertFalse(store.loadAlignmentEvent(verification.id))
        XCTAssertTrue(store.loadAlignmentEvent(coordinator.id, consumed: true)) // Historical evidence is retained.
        XCTAssertEqual(store.alignments.first { $0.id == verification.id }?.handedOff, false)
        XCTAssertEqual(store.alignments.first { $0.id == coordinator.id }?.consumed, true)
        XCTAssertEqual(store.message(first.id)?.supersededBy, second.id)
        XCTAssertEqual(store.message(first.id)?.delivery, .notHandedOff)
        XCTAssertEqual(store.alignments.filter { $0.correctionID == second.id }.count, 3)
        XCTAssertTrue(store.alignments.filter { $0.correctionID == second.id }.allSatisfy {
            !$0.handedOff && !$0.consumed
        })
    }

    @MainActor
    func testTC5QuestionsDeferralAndAnswerAvailabilityNeverApproveOrCompleteWork() async throws {
        let store = TaskCommunicationReviewStore(scenario: .questions)
        XCTAssertEqual(store.questions.count, 2)
        XCTAssertEqual(Set(store.questions.map(\.asker)).count, 2)
        for question in store.questions {
            XCTAssertFalse(question.work.isEmpty || question.basis.isEmpty || question.recommendation.isEmpty)
            XCTAssertFalse(question.recommendationReason.isEmpty || question.impact.isEmpty)
            store.deferQuestion(question.id)
            XCTAssertNil(store.answer(to: question.id))
            XCTAssertNil(store.question(question.id)?.availableEvidence)
            XCTAssertTrue(store.answerSummary(question.id).contains("仍未回答"))
        }
        XCTAssertTrue(store.continuationBlockers(.interface).contains { $0.contains("q-interface") })
        XCTAssertTrue(store.workSummary(.documentation).contains("安全独立工作可继续"))
        store.loadGateEvent(.approvalMissing, member: .interface)
        store.loadGateEvent(.supervisionMissing, member: .interface)
        let conditions = store.conditions
        XCTAssertFalse(store.loadAnswerAvailable("q-interface"))
        XCTAssertTrue(store.beginReply("q-interface"))
        let answer = try send("保留独立字段，先核对受影响接口。", in: store)
        XCTAssertEqual(store.conditions, conditions)
        XCTAssertFalse(store.loadAnswerAvailable("q-interface"))
        XCTAssertTrue(store.loadMessageEvent(.handedOff, id: answer.id))
        XCTAssertTrue(store.continuationBlockers(.interface).contains { $0.contains("q-interface") })
        XCTAssertTrue(store.loadAnswerAvailable("q-interface"))
        XCTAssertFalse(store.continuationBlockers(.interface).contains { $0.contains("q-interface") })
        XCTAssertEqual(store.conditions[.interface]?.approvalSatisfied, false)
        XCTAssertEqual(store.conditions[.interface]?.supervisionSatisfied, false)
        XCTAssertTrue(store.answerSummary("q-interface").contains("不是审批 / 完成 / 验收"))
        XCTAssertNil(store.answer(to: "q-documentation"))
    }

    @MainActor
    func testTC6OneFIFOItemPerBoundaryWithIndependentMember() async throws {
        let store = TaskCommunicationReviewStore(scenario: .continuation)
        XCTAssertEqual(store.queue(.coordinator).map(\.id), [1, 2])
        XCTAssertFalse(store.loadMessageEvent(.handedOff, id: 1))
        store.setPurpose(.nextRound)
        store.setRecipient(.documentation)
        let independent = try send("独立下一轮整理。", in: store)
        XCTAssertTrue(store.queue(.coordinator).allSatisfy { $0.intent == .waitingNextRound })
        store.loadGateEvent(.roundEnded, member: .coordinator)
        XCTAssertEqual(store.message(1)?.intent, .followUpPending)
        XCTAssertEqual(store.message(1)?.delivery, .notHandedOff)
        XCTAssertEqual(store.message(2)?.intent, .waitingNextRound)
        XCTAssertEqual(store.message(independent.id)?.intent, .waitingNextRound)
        XCTAssertEqual(store.conditions[.coordinator]?.usedBoundary, 1)
        store.loadGateEvent(.resourceRestored, member: .coordinator)
        XCTAssertEqual(store.message(2)?.intent, .waitingNextRound)
        XCTAssertTrue(store.loadMessageEvent(.handedOff, id: 1))
        store.loadGateEvent(.resourceRestored, member: .coordinator)
        XCTAssertEqual(store.message(2)?.intent, .waitingNextRound) // Same boundary is spent.
        store.loadGateEvent(.roundEnded, member: .documentation)
        XCTAssertEqual(store.message(independent.id)?.intent, .followUpPending)
        store.loadGateEvent(.roundEnded, member: .coordinator)
        XCTAssertEqual(store.message(2)?.intent, .followUpPending)
        XCTAssertEqual(store.conditions[.coordinator]?.usedBoundary, 2)
        XCTAssertEqual(store.messages.count, 4)
        XCTAssertEqual(store.records.filter { $0.text.contains("示例规则自动接续") }.count, 3)
    }

    @MainActor
    func testTC6PendingFirstItemKeepsOrderUntilExplicitHandoffFact() async {
        let store = TaskCommunicationReviewStore(scenario: .continuation)
        store.loadGateEvent(.roundEnded, member: .coordinator)
        store.loadGateEvent(.roundEnded, member: .coordinator)
        XCTAssertEqual(store.message(2)?.intent, .waitingNextRound)
        XCTAssertTrue(store.continuationBlockers(.coordinator).contains { $0.contains("保留顺序") })
        XCTAssertTrue(store.loadMessageEvent(.handedOff, id: 1))
        XCTAssertEqual(store.message(2)?.intent, .followUpPending)
        XCTAssertEqual(store.message(2)?.delivery, .notHandedOff)
        XCTAssertEqual(store.conditions[.coordinator]?.usedBoundary, 2)
    }

    @MainActor
    func testTC6WithdrawalOrCorrectionAutomaticallyReevaluatesRemainingQueue() async throws {
        for correct in [false, true] {
            let store = TaskCommunicationReviewStore(scenario: .continuation)
            store.loadGateEvent(.roundEnded, member: .coordinator)
            store.loadGateEvent(.roundEnded, member: .coordinator)
            XCTAssertEqual(store.message(1)?.intent, .followUpPending)
            XCTAssertEqual(store.message(2)?.intent, .waitingNextRound)
            if correct {
                XCTAssertTrue(store.beginCorrection(1))
                let correction = try send("M1 明确更正，不再执行旧要求。", in: store)
                XCTAssertEqual(store.message(1)?.supersededBy, correction.id)
                XCTAssertEqual(store.message(correction.id)?.intent, .waitingNextRound)
            } else {
                XCTAssertTrue(store.withdraw(1))
                XCTAssertEqual(store.message(1)?.withdrawn, true)
            }
            XCTAssertEqual(store.message(2)?.intent, .followUpPending)
            XCTAssertEqual(store.conditions[.coordinator]?.usedBoundary, 2)
            XCTAssertEqual(store.message(1)?.delivery, .notHandedOff)
            XCTAssertEqual(store.message(2)?.delivery, .notHandedOff)
        }
    }

    @MainActor
    func testTC6EachFiniteGateBlocksAndRestorationAutomaticallyUsesExistingBoundary() async {
        let pairs: [(TCReviewGateEvent, TCReviewGateEvent, String)] = [
            (.resourceBlocked, .resourceRestored, "资源不足"),
            (.authorizationMissing, .authorizationAvailable, "缺少授权"),
            (.approvalMissing, .approvalAvailable, "审批未齐"),
            (.supervisionMissing, .supervisionAvailable, "监督证据未齐"),
            (.capabilityUnknown, .capabilityKnown, "能力不明"),
            (.operationUnknown, .operationVerified, "先核实"),
        ]
        for (block, restore, expected) in pairs {
            let store = TaskCommunicationReviewStore(scenario: .continuation)
            store.loadGateEvent(block, member: .coordinator)
            store.loadGateEvent(.roundEnded, member: .coordinator)
            XCTAssertEqual(store.message(1)?.intent, .waitingNextRound, block.rawValue)
            XCTAssertEqual(store.conditions[.coordinator]?.usedBoundary, 0)
            XCTAssertTrue(store.continuationBlockers(.coordinator).contains { $0.contains(expected) })
            store.loadGateEvent(restore, member: .coordinator)
            XCTAssertEqual(store.message(1)?.intent, .followUpPending, restore.rawValue)
            XCTAssertEqual(store.message(2)?.intent, .waitingNextRound)
            XCTAssertEqual(store.message(1)?.delivery, .notHandedOff)
            store.loadGateEvent(restore, member: .coordinator)
            XCTAssertEqual(store.message(2)?.intent, .waitingNextRound)
        }
    }

    @MainActor
    func testTC6AnswerIntentAndHandoffAloneDoNotReleaseAnswerGate() async throws {
        let store = TaskCommunicationReviewStore(scenario: .continuation)
        store.loadGateEvent(.waitForAnswer, member: .coordinator)
        store.loadGateEvent(.roundEnded, member: .coordinator)
        XCTAssertTrue(store.beginReply("q-interface"))
        let answer = try send("明确保留两个字段。", in: store)
        XCTAssertEqual(store.message(1)?.intent, .waitingNextRound)
        store.loadMessageEvent(.handedOff, id: answer.id)
        XCTAssertEqual(store.message(1)?.intent, .waitingNextRound)
        XCTAssertTrue(store.loadAnswerAvailable("q-interface"))
        XCTAssertEqual(store.message(1)?.intent, .followUpPending)
        XCTAssertEqual(store.message(2)?.intent, .waitingNextRound)
        XCTAssertEqual(store.message(1)?.consumed, false)
    }

    @MainActor
    func testTC6UnknownDeliveryRequiresItsOwnEvidenceAndNeverReplaysOriginal() async {
        let store = TaskCommunicationReviewStore(scenario: .continuation)
        XCTAssertTrue(store.loadMessageEvent(.deliveryUnknown, id: 1))
        store.loadGateEvent(.roundEnded, member: .coordinator)
        store.loadGateEvent(.resourceRestored, member: .coordinator)
        store.loadGateEvent(.operationVerified, member: .coordinator)
        XCTAssertEqual(store.message(1)?.delivery, .unknown)
        XCTAssertEqual(store.message(2)?.intent, .waitingNextRound)
        XCTAssertTrue(store.loadMessageEvent(.consumed, id: 1))
        XCTAssertEqual(store.message(1)?.delivery, .unknown) // Keep the unresolved delivery dimension.
        XCTAssertEqual(store.message(2)?.intent, .waitingNextRound)
        XCTAssertTrue(store.loadMessageEvent(.handedOff, id: 1)) // Explicit verification, not retry.
        XCTAssertEqual(store.message(2)?.intent, .followUpPending)
        XCTAssertEqual(store.messages.count, 3)
        XCTAssertEqual(store.message(1)?.text, "先补充错误处理说明。")
        XCTAssertEqual(store.message(1)?.withdrawn, false)
    }

    @MainActor
    func testTC6PauseStopRevocationAndCancellationCannotBeWokenByEvents() async throws {
        for control in ["pause", "stop", "verifiedStop", "revoke", "cancel"] {
            let store = TaskCommunicationReviewStore(scenario: .continuation)
            store.loadGateEvent(.resourceBlocked, member: .coordinator)
            switch control {
            case "pause": store.pause(.coordinator)
            case "stop": store.requestStop(.coordinator)
            case "verifiedStop":
                store.requestStop(.coordinator)
                XCTAssertFalse(store.conditions[.coordinator]?.stopVerified ?? true)
                XCTAssertTrue(store.loadGateEvent(.stopVerified, member: .coordinator))
            case "revoke": store.loadGateEvent(.authorizationRevoked, member: .coordinator)
            default: store.loadGateEvent(.queueCancelled, member: .coordinator)
            }
            for event in [TCReviewGateEvent.roundEnded, .resourceRestored, .authorizationAvailable,
                          .approvalAvailable, .supervisionAvailable, .capabilityKnown, .roundEnded] {
                store.loadGateEvent(event, member: .coordinator)
            }
            XCTAssertEqual(store.queue(.coordinator).map(\.id), [1, 2])
            XCTAssertTrue(store.queue(.coordinator).allSatisfy { $0.intent == .waitingNextRound }, control)
            XCTAssertEqual(store.conditions[.coordinator]?.usedBoundary, 0)
            XCTAssertFalse(store.conditions[.coordinator]?.controlBlockers.isEmpty ?? true)
            XCTAssertFalse(store.records.contains { $0.text.contains("示例规则自动接续") })
            store.setPurpose(.nextRound)
            store.setRecipient(.documentation)
            let independent = try send("独立文档接续。", in: store)
            store.loadGateEvent(.roundEnded, member: .documentation)
            XCTAssertEqual(store.message(independent.id)?.intent, .followUpPending)
        }
        let store = TaskCommunicationReviewStore(scenario: .continuation)
        XCTAssertFalse(store.loadGateEvent(.stopVerified, member: .coordinator))
        store.loadGateEvent(.roundEnded, member: .coordinator)
        store.requestStop(.coordinator)
        XCTAssertEqual(store.conditions[.coordinator]?.stopVerified, false)
        XCTAssertEqual(store.message(1)?.intent, .followUpPending) // Keep prior intent; do not pretend rollback.
        store.loadGateEvent(.roundEnded, member: .coordinator)
        store.loadGateEvent(.resourceRestored, member: .coordinator)
        XCTAssertEqual(store.message(2)?.intent, .waitingNextRound)
        XCTAssertEqual(store.message(1)?.delivery, .notHandedOff)
    }

    @MainActor
    func testTC6NewlySubmittedIntentUsesAlreadyAvailableUnspentBoundary() async throws {
        let store = TaskCommunicationReviewStore(scenario: .continuation)
        store.loadGateEvent(.roundEnded, member: .documentation)
        XCTAssertEqual(store.conditions[.documentation]?.usedBoundary, 0)
        store.setPurpose(.nextRound)
        store.setRecipient(.documentation)
        let message = try send("边界已可用时交办，不需要再次点击事件。", in: store)
        XCTAssertEqual(store.message(message.id)?.intent, .followUpPending)
        XCTAssertEqual(store.message(message.id)?.delivery, .notHandedOff)
        XCTAssertEqual(store.conditions[.documentation]?.usedBoundary, 1)
    }

    @MainActor
    func testTC7SupplementKeepsRequirementsAndCorrectionSkipsOnlyItsObsoleteScope() async throws {
        let store = TaskCommunicationReviewStore(scenario: .requirements)
        let keyboard = try XCTUnwrap(store.message(2))
        let supplement = try send("普通补充：说明保持简洁。", in: store)
        XCTAssertNil(supplement.correctionOf)
        XCTAssertEqual(store.effectiveRequirements.map(\.id), [1, 2])
        XCTAssertTrue(store.beginCorrection(1))
        let blue = try send("按钮提示色明确改为蓝色。", in: store)
        XCTAssertEqual(blue.correctionOf, 1)
        XCTAssertEqual(blue.scope, .color)
        XCTAssertEqual(store.effectiveRequirements.map(\.id), [2, blue.id])
        XCTAssertEqual(store.queue(.coordinator).map(\.id), [2, blue.id])
        XCTAssertEqual(store.message(1)?.text, "按钮提示色：红色。")
        XCTAssertEqual(store.message(1)?.withdrawn, false)
        XCTAssertEqual(store.message(1)?.delivery, .notHandedOff)
        XCTAssertEqual(store.message(2), keyboard)
        store.loadGateEvent(.roundEnded, member: .coordinator)
        XCTAssertEqual(store.message(1)?.intent, .waitingNextRound)
        XCTAssertEqual(store.message(2)?.intent, .followUpPending)
        XCTAssertFalse(store.loadMessageEvent(.handedOff, id: 1))
        store.loadMessageEvent(.handedOff, id: 2)
        store.loadGateEvent(.roundEnded, member: .coordinator)
        XCTAssertEqual(store.message(blue.id)?.intent, .followUpPending)
        XCTAssertEqual(store.message(supplement.id), supplement)
    }

    @MainActor
    func testTC7CorrectingAlreadyPendingButUnsentRequirementDoesNotSendObsoleteAction() async throws {
        let store = TaskCommunicationReviewStore(scenario: .requirements)
        store.loadGateEvent(.roundEnded, member: .coordinator)
        XCTAssertEqual(store.message(1)?.intent, .followUpPending)
        XCTAssertTrue(store.beginCorrection(1))
        let correction = try send("明确只按蓝色执行。", in: store)
        XCTAssertFalse(store.loadMessageEvent(.handedOff, id: 1))
        XCTAssertEqual(store.message(1)?.delivery, .notHandedOff)
        XCTAssertEqual(store.message(1)?.supersededBy, correction.id)
        store.loadGateEvent(.roundEnded, member: .coordinator)
        XCTAssertEqual(store.message(2)?.intent, .followUpPending)
        XCTAssertEqual(store.message(correction.id)?.delivery, .notHandedOff)
    }

    @MainActor
    func testTC7CorrectionNeverRollsBackOccurredOrUnknownOperation() async throws {
        let store = TaskCommunicationReviewStore(scenario: .delivery)
        XCTAssertTrue(store.loadMessageEvent(.operationOccurred, id: 1))
        XCTAssertTrue(store.beginCorrection(1))
        _ = try send("后续改用蓝色，不声称旧操作消失。", in: store)
        XCTAssertEqual(store.message(1)?.operation, .occurred)
        XCTAssertEqual(store.message(1)?.delivery, .handedOff)
        XCTAssertEqual(store.message(2)?.operation, .unknown)
        store.loadGateEvent(.operationVerified, member: .coordinator)
        XCTAssertEqual(store.message(2)?.operation, .unknown)
        XCTAssertTrue(store.continuationBlockers(.coordinator).contains { $0.contains("先核实") })
    }

    @MainActor
    func testTC8ConflictWaitsForExplicitScopedAnswerAndLeavesIndependentWork() async throws {
        let store = TaskCommunicationReviewStore(scenario: .conflict)
        let originals = store.messages
        let conflict = try XCTUnwrap(store.question("q-layout-conflict"))
        XCTAssertEqual(conflict.conflictingMessages, [1, 2])
        XCTAssertEqual(conflict.scope, .layout)
        store.deferQuestion(conflict.id)
        store.loadGateEvent(.roundEnded, member: .coordinator)
        XCTAssertNil(store.answer(to: conflict.id))
        XCTAssertEqual(store.messages, originals)
        XCTAssertTrue(store.continuationBlockers(.coordinator).contains { $0.contains(conflict.id) })
        XCTAssertTrue(store.workSummary(.documentation).contains("安全独立工作可继续"))
        XCTAssertTrue(store.beginReply(conflict.id))
        XCTAssertEqual(store.draft.text, "")
        let answer = try send("明确：宽窗紧凑列，窄窗为单列阅读面。", in: store)
        XCTAssertEqual(answer.questionID, conflict.id)
        XCTAssertEqual(answer.scope, .layout)
        XCTAssertEqual(answer.recipient, conflict.asker)
        XCTAssertEqual(answer.delivery, .notHandedOff)
        XCTAssertEqual(Array(store.messages.prefix(2)), originals)
        XCTAssertTrue(store.continuationBlockers(.coordinator).contains { $0.contains(conflict.id) })
        store.loadMessageEvent(.handedOff, id: answer.id)
        store.loadAnswerAvailable(conflict.id)
        XCTAssertTrue(store.workSummary(.coordinator).contains("仍待核对"))
        XCTAssertEqual(store.message(1)?.delivery, .handedOff)
        XCTAssertEqual(store.message(2)?.delivery, .notHandedOff)
        XCTAssertEqual(store.message(answer.id)?.consumed, false)
        XCTAssertTrue(store.workSummary(.documentation).contains("安全独立工作可继续"))
    }

    @MainActor
    func testInstancesResetAndSceneChangesRejectOldDraftsAndIsolateMemory() async throws {
        let first = TaskCommunicationReviewStore(scenario: .continuation)
        let second = TaskCommunicationReviewStore(scenario: .continuation)
        let secondMessages = second.messages
        let secondConditions = second.conditions
        first.setDraftText("第一窗口的文字")
        let old = first.draft
        XCTAssertFalse(second.submitDraft(old))
        first.cancelComposer()
        XCTAssertFalse(first.submitDraft(old))
        XCTAssertTrue(first.beginReply("q-interface"))
        _ = try send("第一窗口的答案", in: first)
        first.requestStop(.coordinator)
        first.loadGateEvent(.resourceBlocked, member: .documentation)
        first.appearance.reduceMotion = true
        first.appearance.colorScheme = .dark
        XCTAssertEqual(second.messages, secondMessages)
        XCTAssertEqual(second.conditions, secondConditions)
        XCTAssertNil(second.answer(to: "q-interface"))
        XCTAssertTrue(second.records.isEmpty)
        XCTAssertFalse(second.appearance.reduceMotion)
        first.reset()
        XCTAssertEqual(first.messages, secondMessages)
        XCTAssertEqual(first.conditions, secondConditions)
        XCTAssertNil(first.answer(to: "q-interface"))
        XCTAssertTrue(first.records.isEmpty)
        XCTAssertEqual(first.draft.context, .compose)
        XCTAssertFalse(first.submitDraft(old))
        XCTAssertTrue(first.appearance.reduceMotion) // Window-only appearance is not a fixture fact.
        XCTAssertTrue(first.beginEdit(1))
        first.setDraftText("不能串到新场景的同名 M1")
        let edit = first.draft
        first.loadScenario(.editing)
        let newFixture = first.messages
        XCTAssertFalse(first.submitDraft(edit))
        XCTAssertEqual(first.messages, newFixture)
        XCTAssertEqual(first.viewedMember, .coordinator)
        XCTAssertTrue(first.alignments.isEmpty)
    }

    @MainActor
    func testAppearanceChangesLeaveMessagesAnswersQueueAndControlsUntouched() async throws {
        let store = TaskCommunicationReviewStore(scenario: .continuation)
        XCTAssertTrue(store.beginReply("q-interface"))
        _ = try send("仅答复原问题。", in: store)
        store.pause(.coordinator)
        let messages = store.messages
        let questions = store.questions
        let conditions = store.conditions
        let draft = store.draft
        let records = store.records
        store.appearance = TCReviewAppearance(
            surface: .classic, colorScheme: .dark, advanced: true,
            reduceTransparency: true, increaseContrast: true, reduceMotion: true)
        XCTAssertEqual(store.messages, messages)
        XCTAssertEqual(store.questions, questions)
        XCTAssertEqual(store.conditions, conditions)
        XCTAssertEqual(store.draft, draft)
        XCTAssertEqual(store.records, records)
        XCTAssertEqual(store.queue(.coordinator).map(\.id), [1, 2])
    }

    @MainActor
    private func send(
        _ text: String, in store: TaskCommunicationReviewStore,
        file: StaticString = #filePath, line: UInt = #line
    ) throws -> TCReviewMessage {
        store.setDraftText(text)
        XCTAssertTrue(store.submitDraft(), store.feedback, file: file, line: line)
        return try XCTUnwrap(store.messages.last, file: file, line: line)
    }

    // MARK: - All filesystem effects are owned by a fresh temporary fixture

    private struct DirectoryFixture {
        let root: URL
        let caches: URL
        var candidate: URL { caches.appendingPathComponent("not-created/communication", isDirectory: true) }
        var environment: [String: String] {
            [
                NativeTaskCommunicationReviewEntry.flagKey: "1",
                "AERORIC_ENV": "ui-test", "AERORIC_DATA_DIR": candidate.path,
            ]
        }
    }

    private func withDirectoryFixture(_ body: (DirectoryFixture) throws -> Void) throws {
        let fm = FileManager.default
        let root = fm.temporaryDirectory.appendingPathComponent("aeroric-communication-\(UUID().uuidString)", isDirectory: true)
        try fm.createDirectory(at: root, withIntermediateDirectories: false)
        defer { try? fm.removeItem(at: root) }
        let caches = root.appendingPathComponent("Library/Caches", isDirectory: true)
        try fm.createDirectory(at: caches, withIntermediateDirectories: true)
        let fixture = DirectoryFixture(root: root, caches: caches)
        let before = try snapshot(root)
        try body(fixture)
        print("TC_FAKE_HOME fixture-only before=\(before) after=\(try snapshot(root))")
    }

    private func snapshot(_ root: URL) throws -> [String] {
        try FileManager.default.subpathsOfDirectory(atPath: root.path).sorted()
    }

    private func decide(_ environment: [String: String], _ fixture: DirectoryFixture) -> TaskCommunicationReviewEntryDecision {
        NativeTaskCommunicationReviewEntry.decide(environment: environment, home: fixture.root.path)
    }
}
#endif
