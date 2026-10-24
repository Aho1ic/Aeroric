#if DEBUG
import AppKit
import Foundation
import SwiftUI
import XCTest
@testable import AeroricUI

/// Policy, memory transitions and actual root assembly. File I/O stays inside
/// disposable fake homes; no App/window, real home, defaults or agent is used.
final class NativeSkillReviewTests: XCTestCase {
    private let otherReviewKeys = [
        "AERORIC_NATIVE_KNOWLEDGE_REVIEW",
        "AERORIC_NATIVE_EVOLUTION_REVIEW",
        "AERORIC_NATIVE_VISUAL_REVIEW",
        "AERORIC_NATIVE_COLLABORATION_REVIEW",
    ]

    // MARK: - Entry matrix: always inject a temporary fake home

    func testAbsentSkillFlagLeavesEveryOtherReviewCombinationInactive() throws {
        try withEntryFixture { fixture in
            let values: [String?] = [nil, "0", "1", "", "invalid"]
            for knowledge in values {
                for evolution in values {
                    for visual in values {
                        for collaboration in values {
                            var environment = [
                                "AERORIC_ENV": "not-a-review",
                                "AERORIC_DATA_DIR": "not-an-absolute-path",
                            ]
                            environment[otherReviewKeys[0]] = knowledge
                            environment[otherReviewKeys[1]] = evolution
                            environment[otherReviewKeys[2]] = visual
                            environment[otherReviewKeys[3]] = collaboration
                            XCTAssertEqual(decision(environment, fixture), .inactive)
                        }
                    }
                }
            }
            XCTAssertEqual(decision([:], fixture), .inactive)
        }
    }

    func testValidEntryAcceptsMissingSafeChildWithoutCreatingIt() throws {
        try withEntryFixture { fixture in
            let manager = FileManager.default
            let before = try manager.contentsOfDirectory(atPath: fixture.caches.path).sorted()
            XCTAssertFalse(manager.fileExists(atPath: fixture.safeChild.path))
            for _ in 0..<3 {
                XCTAssertEqual(decision(fixture.environment, fixture), .review)
            }
            XCTAssertFalse(manager.fileExists(atPath: fixture.safeChild.path))
            XCTAssertEqual(try manager.contentsOfDirectory(atPath: fixture.caches.path).sorted(), before)
        }
    }

    func testValidEntryAcceptsExistingSafeDirectoryAndMissingNestedChildren() throws {
        try withEntryFixture { fixture in
            let manager = FileManager.default
            let existing = fixture.caches.appendingPathComponent("existing-safe", isDirectory: true)
            try manager.createDirectory(at: existing, withIntermediateDirectories: true)
            let before = try manager.contentsOfDirectory(atPath: existing.path).sorted()
            for path in [existing.path, existing.appendingPathComponent("missing/deep-child").path] {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = path
                XCTAssertEqual(decision(environment, fixture), .review)
            }
            XCTAssertFalse(manager.fileExists(atPath: existing.appendingPathComponent("missing").path))
            XCTAssertEqual(try manager.contentsOfDirectory(atPath: existing.path).sorted(), before)
        }
    }

    func testPresentZeroEmptyAndMalformedSkillFlagsAreRefused() throws {
        try withEntryFixture { fixture in
            for value in ["0", "", "2", "-1", "true", "01", " 1", "1 ", "\n1", "1\n"] {
                var environment = fixture.environment
                environment[NativeSkillReviewEntry.flagKey] = value
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidFlag]))
            }
        }
    }

    func testPresentInvalidFlagWithoutPrerequisitesDoesNotBecomeInactive() throws {
        try withEntryFixture { fixture in
            XCTAssertEqual(
                decision([NativeSkillReviewEntry.flagKey: "0"], fixture),
                .refused([.invalidFlag, .invalidEnvironment, .invalidDirectory])
            )
        }
    }

    func testMissingAndInvalidEnvironmentAreRefused() throws {
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

    func testMissingEmptyAndRelativeDirectoriesAreRefused() throws {
        try withEntryFixture { fixture in
            let values: [String?] = [
                nil, "", " ", "\n", "Library/Caches/review", "./review", "~/Library/Caches/review",
            ]
            for value in values {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = value
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidDirectory]))
            }
        }
    }

    func testCachesRootTraversalPrefixSquattingAndWhitespaceAreRefused() throws {
        try withEntryFixture { fixture in
            let rejected = [
                "/", fixture.home.path, fixture.caches.path, fixture.caches.path + "/",
                fixture.home.path + "/Library/Caches-neighbour/review",
                fixture.home.path + "/Library/Caches2/review",
                fixture.home.path + "/Library/Application Support/review",
                fixture.caches.path + "/../review",
                fixture.caches.path + "/safe/../../review",
                fixture.caches.path + "/./review",
                fixture.caches.path + "/safe/../review",
                " " + fixture.safeChild.path, fixture.safeChild.path + "\n",
            ]
            for path in rejected {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = path
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidDirectory]))
            }
        }
    }

    func testFinalAndAncestorSymlinksIncludingDanglingLinksAreRefused() throws {
        try withEntryFixture { fixture in
            let manager = FileManager.default
            let inside = fixture.caches.appendingPathComponent("real-inside", isDirectory: true)
            let outside = fixture.root.appendingPathComponent("outside-fake-home", isDirectory: true)
            let missingTarget = fixture.root.appendingPathComponent("nonexistent-target", isDirectory: true)
            try manager.createDirectory(at: inside, withIntermediateDirectories: true)
            try manager.createDirectory(at: outside, withIntermediateDirectories: true)
            let links = [
                ("link-inside", inside),
                ("link-outside", outside),
                ("link-caches-root", fixture.caches),
                ("link-dangling", missingTarget),
            ]
            for (name, target) in links {
                let link = fixture.caches.appendingPathComponent(name)
                try manager.createSymbolicLink(at: link, withDestinationURL: target)
                for path in [link.path, link.appendingPathComponent("missing/deep-child").path] {
                    var environment = fixture.environment
                    environment["AERORIC_DATA_DIR"] = path
                    XCTAssertEqual(decision(environment, fixture), .refused([.invalidDirectory]), name)
                }
            }
            XCTAssertFalse(manager.fileExists(atPath: missingTarget.path))
            XCTAssertTrue(try manager.contentsOfDirectory(atPath: inside.path).isEmpty)
            XCTAssertTrue(try manager.contentsOfDirectory(atPath: outside.path).isEmpty)
        }
    }

    func testFileFinalPathAndFileAncestorAreRefused() throws {
        try withEntryFixture { fixture in
            let file = fixture.caches.appendingPathComponent("plain-fixture-file")
            try Data("fictional entry fixture only".utf8).write(to: file)
            for path in [file.path, file.appendingPathComponent("missing-child").path] {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = path
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidDirectory]))
            }
        }
    }

    func testEachOtherReviewFlagIndividuallyRefusesInsteadOfSelectingAnEntry() throws {
        try withEntryFixture { fixture in
            for key in otherReviewKeys {
                var environment = fixture.environment
                for other in otherReviewKeys { environment[other] = nil }
                environment[key] = "1"
                XCTAssertEqual(decision(environment, fixture), .refused([.conflictingReview]), key)
            }
        }
    }

    func testAllFifteenNonemptyOtherReviewCombinationsAreRefused() throws {
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

    func testOtherReviewFlagsZeroAbsentAndNonOneValuesDoNotConflict() throws {
        try withEntryFixture { fixture in
            let values: [String?] = [nil, "0", "", "invalid", "true", " 1", "1 "]
            for value in values {
                var environment = fixture.environment
                for key in otherReviewKeys { environment[key] = value }
                XCTAssertEqual(decision(environment, fixture), .review)
            }
        }
    }

    func testMultipleFailuresReturnOnlyFixedNonSensitiveReasons() throws {
        try withEntryFixture { fixture in
            let marker = "fictional-private-value-\(UUID().uuidString)"
            var environment = fixture.environment
            environment[NativeSkillReviewEntry.flagKey] = marker
            environment["AERORIC_ENV"] = marker
            environment["AERORIC_DATA_DIR"] = fixture.home.appendingPathComponent(marker).path
            for key in otherReviewKeys { environment[key] = "1" }
            let result = decision(environment, fixture)
            XCTAssertEqual(
                result, .refused([.invalidFlag, .invalidEnvironment, .invalidDirectory, .conflictingReview])
            )
            XCTAssertFalse(String(reflecting: result).contains(marker))
            XCTAssertFalse(String(reflecting: result).contains(fixture.home.path))
            for reason in SkillReviewRefusalReason.allCases {
                XCTAssertFalse(reason.title.isEmpty)
                XCTAssertFalse(reason.detail.isEmpty)
                XCTAssertEqual(reason.id, reason.rawValue)
                for text in [reason.title, reason.detail] {
                    XCTAssertFalse(text.contains(marker))
                    XCTAssertFalse(text.contains(fixture.root.path))
                    XCTAssertFalse(text.contains("/"))
                }
            }
        }
    }

    func testLaunchCorrelationOnlyExposesValidatedUUIDAndNeverChangesEntryDecision() throws {
        try withEntryFixture { fixture in
            let key = "AERORIC_SKILL_REVIEW_TEST_LAUNCH_ID"
            let uuid = UUID().uuidString
            for value in [nil, "", "sensitive-not-a-uuid", fixture.safeChild.path, uuid] as [String?] {
                var environment = fixture.environment
                environment[key] = value
                XCTAssertEqual(decision(environment, fixture), .review)
                XCTAssertEqual(NativeSkillReviewEntry.testLaunchID(environment: environment),
                               value == uuid ? uuid : "无测试启动标识")
                environment[NativeSkillReviewEntry.flagKey] = nil
                XCTAssertEqual(decision(environment, fixture), .inactive)
            }
        }
    }

    // XCTest's async adapter avoids a synchronous @MainActor discovery cast.
    @MainActor
    func testInactiveLifecycleInitializationAndShowNeverCreateAWindow() async {
        let lifecycle = NativeSkillReviewWindowLifecycle(decision: .inactive)
        XCTAssertNil(lifecycle.windowIdentifier)
        XCTAssertEqual(lifecycle.windowTitle, "")
        XCTAssertFalse(lifecycle.isWindowCreated)
        for _ in 0..<3 {
            lifecycle.showReviewWindow()
            XCTAssertFalse(lifecycle.isWindowCreated)
        }
    }

    @MainActor
    func testReviewAndRefusalLifecycleMetadataIsExclusiveWithoutCreatingWindows() async {
        let review = NativeSkillReviewWindowLifecycle(decision: .review)
        let refusal = NativeSkillReviewWindowLifecycle(decision: .refused([.invalidFlag]))
        XCTAssertEqual(review.windowIdentifier?.rawValue, "native-skill-review")
        XCTAssertEqual(refusal.windowIdentifier?.rawValue, "native-skill-review-refusal")
        XCTAssertEqual(review.windowTitle, "技能审阅样例")
        XCTAssertEqual(refusal.windowTitle, "技能审阅未启动")
        XCTAssertNotEqual(review.windowIdentifier, refusal.windowIdentifier)
        XCTAssertFalse(review.isWindowCreated)
        XCTAssertFalse(refusal.isWindowCreated)
    }

    // MARK: - Value and fixture contracts, not Store/View behavior

    func testConsentTargetEqualityBindsBodyVersionAndProject() {
        let original = SkillReviewFixtures.version(.boundary)
        let target = SkillReviewConsentTarget(version: original, project: .harbor)
        XCTAssertEqual(target, SkillReviewConsentTarget(version: original, project: .harbor))

        var edited = original
        edited.content += "\n仅修改正文的虚构修订。"
        XCTAssertNotEqual(target, SkillReviewConsentTarget(version: edited, project: .harbor))

        let renumbered = SkillReviewVersion(
            itemID: original.itemID, title: original.title, number: "1.1",
            content: original.content, source: original.source,
            conditions: original.conditions, validation: original.validation,
            intendedUse: original.intendedUse, capabilities: original.capabilities,
            dependencies: original.dependencies, scopeNote: original.scopeNote,
            relevantWork: original.relevantWork, availability: original.availability
        )
        XCTAssertNotEqual(target, SkillReviewConsentTarget(version: renumbered, project: .harbor))
        XCTAssertNotEqual(target, SkillReviewConsentTarget(version: original, project: .tide))
    }

    func testS1DraftFixturesArePendingWithoutConsentResultsOrEnablement() {
        let items = SkillReviewFixtures.items(for: .drafts)
        XCTAssertEqual(items.count, 2)
        XCTAssertEqual(Set(items.map(\.id)), [.boundary, .keyboard])
        for item in items {
            XCTAssertEqual(item.contentState, .pending)
            XCTAssertEqual(item.response, .unanswered)
            XCTAssertNil(item.approvedTarget)
            XCTAssertEqual(item.installOutcome, .unloaded)
            XCTAssertNil(item.installedTarget)
            XCTAssertNil(item.enabledTarget)
            XCTAssertTrue(item.approvedHistory.isEmpty)
            XCTAssertTrue(item.installedHistory.isEmpty)
            XCTAssertFalse(item.hasMatchingConsent)
            XCTAssertFalse(item.canEnable)
        }
    }

    func testFourExecutorFixturesKeepAvailabilityAndMissingSourceDistinct() {
        XCTAssertEqual(SkillReviewExecutor.allCases.map(\.name), ["Claude Code", "Codex", "omp", "dsh"])
        let available = SkillReviewFixtures.version(.boundary)
        let blocked = SkillReviewFixtures.version(.dependency)
        let unknown = SkillReviewFixtures.version(.unknown)
        var missingSource = SkillReviewFixtures.version(.missingSource)
        XCTAssertNil(missingSource.source)
        for executor in SkillReviewExecutor.allCases {
            XCTAssertTrue(executor.title.contains("预设，非实际兼容结论"))
            XCTAssertEqual(available.availability(for: executor).state, .available)
            XCTAssertEqual(blocked.availability(for: executor).state, .blocked)
            XCTAssertEqual(unknown.availability(for: executor).state, .unknown)
            XCTAssertFalse(blocked.availability(for: executor).reason.isEmpty)
            XCTAssertFalse(unknown.availability(for: executor).reason.isEmpty)
            XCTAssertEqual(missingSource.availability(for: executor).state, .unknown)
            // Even an optimistic raw record cannot bypass the missing source.
            missingSource.availability[executor] = .init(state: .available, reason: "虚构冲突预设")
            XCTAssertEqual(missingSource.availability(for: executor).state, .unknown)
            XCTAssertTrue(missingSource.availability(for: executor).reason.contains("没有来源"))
        }
    }

    func testUpdateFixtureExposesVersionSourceContentCapabilityDependencyAndScopeDifferences() {
        let original = SkillReviewFixtures.version(.boundary)
        let updated = SkillReviewFixtures.version(.boundary, updated: true)
        XCTAssertEqual(original.number, "1.0")
        XCTAssertEqual(updated.number, "1.1")
        XCTAssertNotEqual(original.source, updated.source)
        XCTAssertNotEqual(original.content, updated.content)
        XCTAssertNotEqual(original.conditions, updated.conditions)
        XCTAssertNotEqual(original.validation, updated.validation)
        XCTAssertNotEqual(original.capabilities, updated.capabilities)
        XCTAssertNotEqual(original.dependencies, updated.dependencies)
        XCTAssertNotEqual(original.scopeNote, updated.scopeNote)
        XCTAssertTrue(updated.scopeNote.contains("不申请该扩围"))
        XCTAssertEqual(SkillReviewConsentTarget(version: updated, project: .harbor).project, .harbor)
    }

    func testInFlightFixtureKeepsV1ReferenceWhileNewTaskStartsUnselected() throws {
        let items = SkillReviewFixtures.items(for: .updates)
        let works = SkillReviewFixtures.works(items: items, scenario: .updates)
        let inFlight = try XCTUnwrap(works.first { $0.id == .inFlight })
        let newTask = try XCTUnwrap(works.first { $0.id == .newTask })
        XCTAssertTrue(inFlight.isInFlight)
        XCTAssertEqual(inFlight.references.count, 1)
        let reference = try XCTUnwrap(inFlight.references.first)
        XCTAssertEqual(reference.target.version.number, "1.0")
        XCTAssertEqual(reference.target.project, .harbor)
        XCTAssertEqual(reference.origin, .preloaded)
        XCTAssertFalse(newTask.isInFlight)
        XCTAssertTrue(newTask.references.isEmpty)
    }

    func testConsentBindingIncludesSourceConditionsValidationCapabilitiesDependenciesAndScope() {
        let original = SkillReviewFixtures.version(.boundary)
        var changes: [SkillReviewVersion] = []
        var value = original
        value.source = .init(title: "另一内嵌虚构来源", excerpt: "新的示例摘录。", limitation: "没有真实回源。")
        changes.append(value)
        value = original; value.conditions += " 新前提（示例）。"; changes.append(value)
        value = original; value.validation += " 新验证要求（示例）。"; changes.append(value)
        value = original; value.intendedUse += " 新拟用途（示例）。"; changes.append(value)
        value = original; value.capabilities.append("新增示例能力"); changes.append(value)
        value = original; value.dependencies.append("新增虚构依赖"); changes.append(value)
        value = original; value.scopeNote += " 新拟范围说明（未获准）。"; changes.append(value)
        for changed in changes {
            var item = SkillReviewFixtures.item(.boundary, preloaded: true)
            XCTAssertTrue(item.hasMatchingConsent)
            item.proposal = changed
            XCTAssertFalse(item.hasMatchingConsent)
            XCTAssertFalse(item.canEnable, "Old consent/result cannot approve any changed review object.")
            XCTAssertEqual(item.approvedTarget?.version, original)
        }
    }

    func testAllScenarioFixturesRemainEmbeddedFictionWithoutPathsURLsOrExecutableInstructions() {
        for scenario in SkillReviewScenario.allCases {
            for item in SkillReviewFixtures.items(for: scenario) {
                let version = item.proposal
                let text = [
                    version.title, version.content, version.conditions, version.validation,
                    version.intendedUse, version.scopeNote, version.capabilities.joined(),
                    version.dependencies.joined(), version.source?.title ?? "",
                    version.source?.excerpt ?? "", version.source?.limitation ?? "",
                ].joined(separator: "\n")
                XCTAssertTrue(text.contains("虚构"))
                XCTAssertTrue(text.contains("示例"))
                for forbidden in ["http://", "https://", "/Users/", "~/", "SKILL.md", "curl ", "ssh ", "sudo "] {
                    XCTAssertFalse(text.contains(forbidden), "\(scenario.rawValue)/\(item.id)")
                }
                XCTAssertTrue(SkillReviewFixtures.noConsumption.contains("无真实加载/消费证据"))
            }
        }
    }

    // MARK: - Window-local transitions, S1–S8

    @MainActor
    func testS1CancelAndLateApplyCannotChangeTheDisplayedDraft() async throws {
        let store = SkillReviewStore()
        let before = try XCTUnwrap(store.selectedItem)
        var draft = try openDraft(store)
        draft.version.content = "取消的虚构草稿，不应写回。"
        store.cancelSheet()
        XCTAssertFalse(store.apply(draft))
        XCTAssertEqual(store.selectedItem, before)
        XCTAssertNil(store.sheet)
    }

    @MainActor
    func testS1ApplyingDraftChangesOnlyMemoryAndNeedsFreshContentReview() async throws {
        let store = SkillReviewStore()
        var draft = try openDraft(store)
        draft.version.title = "人工修改后的虚构方法"
        draft.version.content = "人工修改后的虚构正文。"
        draft.version.conditions = "新的示例前提。"
        draft.version.validation = "新的示例验证要求。"
        XCTAssertTrue(store.apply(draft))
        let item = try XCTUnwrap(store.selectedItem)
        XCTAssertEqual(item.proposal, draft.version)
        XCTAssertEqual(item.contentState, .pending)
        XCTAssertEqual(item.response, .unanswered)
        XCTAssertEqual(item.installOutcome, .unloaded)
        XCTAssertNil(item.enabledTarget)
        XCTAssertTrue(item.approvedHistory.isEmpty)
        XCTAssertTrue(store.works.allSatisfy(\.references.isEmpty))
        XCTAssertNil(store.sheet)
    }

    @MainActor
    func testS1ContentConfirmationNeverGrantsInstallEnablementOrTaskSelection() async throws {
        let store = SkillReviewStore()
        XCTAssertTrue(store.confirmContent())
        let item = try XCTUnwrap(store.selectedItem)
        XCTAssertEqual(item.contentState, .confirmed)
        XCTAssertEqual(item.response, .unanswered)
        XCTAssertNil(item.approvedTarget)
        XCTAssertFalse(store.loadInstall(.success))
        XCTAssertFalse(store.enableVersion())
        XCTAssertFalse(store.select(item.id))
        XCTAssertTrue(store.works.allSatisfy(\.references.isEmpty))
    }

    @MainActor
    func testS1DiscardDiffersFromConfirmationAndCannotBeApprovedOrEdited() async throws {
        let store = SkillReviewStore()
        XCTAssertTrue(store.confirmContent())
        store.selectedItemID = .keyboard
        XCTAssertTrue(store.discardDraft())
        XCTAssertEqual(store.item(.boundary)?.contentState, .confirmed)
        XCTAssertEqual(store.item(.keyboard)?.contentState, .discarded)
        XCTAssertNil(store.makeDraft())
        XCTAssertFalse(store.confirmContent())
        XCTAssertFalse(store.respond(.agreed))
        XCTAssertFalse(store.loadInstall(.success))
        XCTAssertNil(store.item(.keyboard)?.enabledTarget)
        XCTAssertEqual(store.visibleItems.count, 2, "Discard remains visibly distinct.")
    }

    @MainActor
    func testEmptyDraftAndDifferentActiveSheetCannotWriteBack() async throws {
        let store = SkillReviewStore()
        let original = store.selectedItem
        var empty = try openDraft(store)
        empty.version.title = " \n "
        XCTAssertFalse(store.apply(empty))
        XCTAssertEqual(store.selectedItem, original)
        empty.version.title = "有效示例名称"
        empty.version.content = " \n "
        XCTAssertFalse(store.apply(empty))
        XCTAssertEqual(store.selectedItem, original)
        store.cancelSheet()
        var previous = try openDraft(store)
        previous.version.content = "已被另一张编辑表单取代的示例。"
        store.beginEdit()
        XCTAssertFalse(store.apply(previous))
        XCTAssertEqual(store.selectedItem, original)
    }

    @MainActor
    func testApplyingUnchangedEditKeepsConsentResultsEnablementAndReferencesIntact() async throws {
        let store = SkillReviewStore(scenario: .tasks)
        XCTAssertTrue(store.select(.boundary))
        let items = store.items
        let works = store.works
        let draft = try openDraft(store)
        XCTAssertTrue(store.apply(draft))
        XCTAssertEqual(store.items, items)
        XCTAssertEqual(store.works, works)
        XCTAssertNil(store.sheet)
        XCTAssertTrue(store.feedback.contains("没有变化"))
    }

    @MainActor
    func testSceneProjectSectionAndResetInvalidatePendingEdits() async throws {
        let store = SkillReviewStore()
        for transition in 0..<4 {
            store.reset()
            var draft = try openDraft(store)
            draft.version.title = "迟到的示例修改 \(transition)"
            switch transition {
            case 0: store.load(.consent)
            case 1: store.setProject(.tide); store.setProject(.harbor)
            case 2: store.setSection(.requests)
            default: store.reset()
            }
            XCTAssertFalse(store.apply(draft))
            XCTAssertNotEqual(store.item(.boundary)?.proposal.title, draft.version.title)
            XCTAssertNil(store.sheet)
        }
    }

    @MainActor
    func testS2UnansweredAndRefusedBlockBothResultsRegardlessOfFullAccess() async throws {
        for fullAccess in [false, true] {
            for response in [SkillReviewConsentResponse.unanswered, .refused] {
                let store = SkillReviewStore(scenario: .consent)
                store.preview.fullAccess = fullAccess
                XCTAssertTrue(store.respond(response))
                XCTAssertFalse(store.loadInstall(.success))
                XCTAssertFalse(store.loadInstall(.failure))
                XCTAssertFalse(store.enableVersion())
                XCTAssertFalse(store.select(.boundary))
                let item = try XCTUnwrap(store.item(.boundary))
                XCTAssertEqual(item.response, response)
                XCTAssertEqual(item.installOutcome, .unloaded)
                XCTAssertNil(item.approvedTarget)
                XCTAssertNil(item.enabledTarget)
            }
        }
    }

    @MainActor
    func testS2AgreementResultEnablementAndSelectionAreFourDifferentSteps() async throws {
        let store = SkillReviewStore(scenario: .consent)
        let displayed = try XCTUnwrap(store.selectedItem).target
        XCTAssertTrue(store.respond(.agreed))
        XCTAssertEqual(store.selectedItem?.approvedTarget, displayed)
        XCTAssertEqual(store.selectedItem?.installOutcome, .unloaded)
        XCTAssertNil(store.selectedItem?.enabledTarget)
        XCTAssertFalse(store.select(.boundary))
        XCTAssertTrue(store.loadInstall(.success))
        XCTAssertNil(store.selectedItem?.enabledTarget)
        XCTAssertTrue(store.selectedWork?.references.isEmpty == true)
        XCTAssertTrue(store.enableVersion())
        XCTAssertTrue(store.visibleItems.contains { $0.id == displayed.version.itemID },
                      "Completing a request must not remove its selected native List row.")
        XCTAssertTrue(store.selectedWork?.references.isEmpty == true)
        XCTAssertTrue(store.select(.boundary))
        XCTAssertEqual(store.selectedWork?.references.first?.target, displayed)
        XCTAssertEqual(store.selectedWork?.references.first?.origin, .manual)
        XCTAssertEqual(store.selectedItem?.response, .agreed, "No second use approval.")
        XCTAssertNil(store.sheet)
    }

    @MainActor
    func testS2FailureRecordCannotEnableOrSelectEvenWithOptimisticCompatibilityPreset() async throws {
        let store = SkillReviewStore(scenario: .consent)
        XCTAssertTrue(store.respond(.agreed))
        XCTAssertTrue(store.loadInstall(.failure))
        let item = try XCTUnwrap(store.selectedItem)
        XCTAssertEqual(item.installOutcome, .failure)
        XCTAssertNil(item.installedTarget)
        XCTAssertNil(item.enabledTarget)
        XCTAssertFalse(item.canEnable)
        XCTAssertTrue(try XCTUnwrap(store.proposalUsageBlock(for: .boundary)).contains("安装失败"))
        XCTAssertFalse(store.enableVersion())
        XCTAssertNotNil(store.selectionBlock(for: .boundary))
        XCTAssertFalse(store.select(.boundary))
        store.agentSelect()
        XCTAssertTrue(store.selectedWork?.references.isEmpty == true)
        XCTAssertTrue(store.loadInstall(.success))
        XCTAssertNotNil(store.proposalUsageBlock(for: .boundary), "Success is still not enablement.")
        XCTAssertTrue(store.enableVersion())
        XCTAssertNil(store.proposalUsageBlock(for: .boundary))
        XCTAssertTrue(store.select(.boundary))
        XCTAssertTrue(store.loadInstall(.failure))
        let reference = try XCTUnwrap(store.selectedWork?.references.first)
        let work = try XCTUnwrap(store.selectedWork)
        XCTAssertNotNil(store.referenceBlock(reference, work: work))
        XCTAssertFalse(store.select(.boundary))
    }

    @MainActor
    func testS2EditingAnAgreedObjectInvalidatesConsentWithoutRewritingHistory() async throws {
        let store = SkillReviewStore(scenario: .consent)
        XCTAssertTrue(store.respond(.agreed))
        XCTAssertTrue(store.loadInstall(.success))
        XCTAssertTrue(store.enableVersion())
        XCTAssertTrue(store.select(.boundary))
        let old = try XCTUnwrap(store.selectedItem).target
        var draft = try openDraft(store)
        draft.version.content += "\n新内容不在旧同意中。"
        draft.version.source = .init(title: "新虚构来源", excerpt: "仅内嵌摘录。", limitation: "未真实回源。")
        XCTAssertTrue(store.apply(draft))
        let changed = try XCTUnwrap(store.selectedItem)
        XCTAssertNotEqual(changed.target, old)
        XCTAssertEqual(changed.response, .unanswered)
        XCTAssertNil(changed.approvedTarget)
        XCTAssertEqual(changed.installOutcome, .unloaded)
        XCTAssertNil(changed.installedTarget)
        XCTAssertNil(changed.enabledTarget)
        XCTAssertEqual(changed.approvedHistory, [old])
        XCTAssertEqual(changed.installedHistory, [old])
        XCTAssertEqual(store.selectedWork?.references.first?.target, old)
        XCTAssertFalse(store.loadInstall(.success))
        XCTAssertFalse(store.enableVersion())
        XCTAssertFalse(store.select(.boundary))
    }

    @MainActor
    func testEditSheetCannotRetargetAnotherSkillOrInventAnotherVersion() async throws {
        let store = SkillReviewStore()
        let items = store.items
        let active = try openDraft(store)
        for version in [
            SkillReviewFixtures.version(.keyboard),
            SkillReviewFixtures.version(.boundary, updated: true),
        ] {
            let retargeted = SkillReviewEditDraft(
                id: active.id, generation: active.generation,
                viewingProject: active.viewingProject, version: version
            )
            XCTAssertFalse(store.apply(retargeted))
            XCTAssertEqual(store.items, items)
        }
    }

    @MainActor
    func testPreloadedConsentIsNeverDescribedAsThisUsersApproval() async throws {
        let store = SkillReviewStore(scenario: .tasks)
        XCTAssertTrue(try XCTUnwrap(store.item(.boundary)).consentIsPreloaded)
        XCTAssertTrue(try XCTUnwrap(store.item(.boundary)).consentTitle.contains("并非本次用户批准"))
        XCTAssertTrue(store.respond(.agreed))
        XCTAssertFalse(try XCTUnwrap(store.item(.boundary)).consentIsPreloaded)
        XCTAssertEqual(store.item(.boundary)?.consentTitle, SkillReviewConsentResponse.agreed.title)
        XCTAssertFalse(try XCTUnwrap(store.item(.candidate)).hasMatchingConsent)
    }

    @MainActor
    func testS3OtherProjectCannotMutateApproveInstallSelectOrExpandEvenWithFullAccess() async throws {
        let store = SkillReviewStore(scenario: .scope)
        XCTAssertTrue(store.select(.boundary))
        let items = store.items
        let works = store.works
        store.preview.fullAccess = true
        store.setProject(.tide)
        XCTAssertFalse(store.canMutateProject)
        XCTAssertNil(store.makeDraft())
        XCTAssertFalse(store.confirmContent())
        XCTAssertFalse(store.discardDraft())
        XCTAssertFalse(store.respond(.agreed))
        XCTAssertFalse(store.loadInstall(.success))
        XCTAssertFalse(store.enableVersion())
        XCTAssertFalse(store.select(.boundary))
        store.agentSelect()
        store.stageUpdate()
        store.loadInFlight(.revoked)
        store.requestStop()
        store.deselect(.boundary)
        store.sheet = .scope
        store.cancelSheet()
        XCTAssertEqual(store.items, items)
        XCTAssertEqual(store.works, works)
        XCTAssertEqual(store.inFlightCondition, .allowed)
        XCTAssertTrue(store.stoppedWorkIDs.isEmpty)
        XCTAssertNil(store.sheet)
        store.setProject(.harbor)
        XCTAssertNil(store.selectionBlock(for: .boundary))
        XCTAssertEqual(store.items, items)
    }

    @MainActor
    func testS4ManualSelectionAndAgentSelectionUseSameRelevantSubset() async throws {
        let manual = SkillReviewStore(scenario: .tasks)
        let simulated = SkillReviewStore(scenario: .tasks)
        for work in [SkillReviewWorkID.api, .keyboard] {
            manual.setWork(work)
            simulated.setWork(work)
            let eligible = manual.items.filter { manual.selectionBlock(for: $0.id) == nil }.map(\.id)
            XCTAssertEqual(eligible, work == .api ? [.boundary] : [.keyboard])
            XCTAssertEqual(
                Set(manual.relatedItems.map(\.id)),
                work == .api ? [.boundary, .dependency, .candidate, .missingSource] : [.keyboard, .unknown]
            )
            for id in eligible { XCTAssertTrue(manual.select(id)) }
            simulated.agentSelect()
            XCTAssertEqual(manual.selectedWork?.references.map(\.id), eligible)
            XCTAssertEqual(simulated.selectedWork?.references.map(\.id), eligible)
            XCTAssertEqual(
                manual.selectedWork?.references.map(\.target),
                simulated.selectedWork?.references.map(\.target)
            )
            XCTAssertTrue(simulated.selectedWork?.references.allSatisfy { $0.origin == .agent } == true)
            XCTAssertNil(simulated.sheet)
        }
        XCTAssertEqual(manual.works.first { $0.id == .api }?.references.map(\.id), [.boundary])
        XCTAssertEqual(manual.works.first { $0.id == .keyboard }?.references.map(\.id), [.keyboard])
    }

    @MainActor
    func testS4SelectionCannotLoadUnapprovedUnrelatedUnavailableOrUnknownItems() async throws {
        let store = SkillReviewStore(scenario: .tasks)
        for id in [SkillReviewItemID.candidate, .keyboard, .dependency, .unknown, .missingSource, .unrelated] {
            XCTAssertNotNil(store.selectionBlock(for: id))
            XCTAssertFalse(store.select(id))
        }
        store.agentSelect()
        XCTAssertEqual(store.selectedWork?.references.map(\.id), [.boundary])
        store.deselect(.boundary)
        XCTAssertTrue(store.selectedWork?.references.isEmpty == true)
        XCTAssertNotNil(store.item(.boundary)?.enabledTarget, "Task deselection is not uninstall.")
        XCTAssertTrue(store.select(.boundary))
        XCTAssertEqual(store.selectedWork?.references.count, 1)
        store.agentSelect()
        XCTAssertEqual(store.selectedWork?.references.count, 1, "No duplicate library injection.")
    }

    @MainActor
    func testS5FourExecutorsNeverTurnUnknownOrMissingSourceIntoUsableAssets() async throws {
        let store = SkillReviewStore(scenario: .availability)
        for executor in SkillReviewExecutor.allCases {
            store.executor = executor
            XCTAssertNil(store.selectionBlock(for: .boundary))
            XCTAssertNotNil(store.selectionBlock(for: .dependency))
            XCTAssertNotNil(store.selectionBlock(for: .missingSource))
            XCTAssertFalse(store.select(.dependency))
            XCTAssertFalse(store.select(.missingSource))
            store.setWork(.keyboard)
            XCTAssertNotNil(store.selectionBlock(for: .unknown))
            XCTAssertFalse(store.select(.unknown))
            store.agentSelect()
            XCTAssertEqual(store.selectedWork?.references.map(\.id), [.keyboard])
            store.setWork(.api)
        }
        XCTAssertTrue(store.items.allSatisfy { $0.target.project == .harbor })
        XCTAssertEqual(store.item(.unknown)?.installOutcome, .success,
                       "A preset installation record does not make unknown compatibility usable.")
    }

    @MainActor
    func testS6UnansweredRefusedAndDeferredUpdateLeaveOldEnabledAndInFlight() async throws {
        for response in [SkillReviewConsentResponse.unanswered, .refused] {
            let store = SkillReviewStore(scenario: .updates)
            let old = try XCTUnwrap(store.item(.boundary)?.enabledTarget)
            let inFlight = try requireWork(.inFlight, store)
            store.stageUpdate()
            XCTAssertEqual(store.item(.boundary)?.proposal.number, "1.1")
            XCTAssertTrue(store.respond(response))
            XCTAssertFalse(store.loadInstall(.success))
            XCTAssertEqual(store.item(.boundary)?.enabledTarget, old)
            XCTAssertEqual(try requireWork(.inFlight, store), inFlight)
            store.setWork(.newTask)
            XCTAssertTrue(store.select(.boundary))
            XCTAssertEqual(store.selectedWork?.references.first?.target, old)
        }
    }

    @MainActor
    func testS6EditingUnapprovedUpdateCandidateDoesNotDisableIndependentOldVersion() async throws {
        let store = SkillReviewStore(scenario: .updates)
        let old = try XCTUnwrap(store.item(.boundary)?.enabledTarget)
        let inFlight = try requireWork(.inFlight, store)
        store.stageUpdate()
        var draft = try openDraft(store)
        draft.version.title = "尚未获准的新候选名称（示例）"
        draft.version.content += "\n新候选的人工修订（示例），不是停用旧版本。"
        XCTAssertTrue(store.apply(draft))
        XCTAssertEqual(store.item(.boundary)?.proposal, draft.version)
        XCTAssertEqual(store.item(.boundary)?.response, .unanswered)
        XCTAssertEqual(store.item(.boundary)?.enabledTarget, old)
        XCTAssertEqual(try requireWork(.inFlight, store), inFlight)
        XCTAssertFalse(store.loadInstall(.success))
        store.setWork(.newTask)
        XCTAssertTrue(store.item(.boundary)?.taskSelectionVersion == old.version,
                      "The actual task selection label must name the enabled old specimen, not the unapproved proposal.")
        XCTAssertTrue(store.select(.boundary))
        XCTAssertEqual(store.selectedWork?.references.first?.target, old)
    }

    @MainActor
    func testS6UpdateComparisonCapturesCurrentRevisionWithoutRewritingInFlightHistory() async throws {
        let store = SkillReviewStore(scenario: .updates)
        let original = try XCTUnwrap(store.item(.boundary)?.enabledTarget)
        let inFlight = try requireWork(.inFlight, store)
        var revision = try openDraft(store)
        revision.version.title = "当前获准的人工修订版（示例）"
        revision.version.content += "\n当前获准修订的差异依据（示例）。"
        XCTAssertTrue(store.apply(revision))
        XCTAssertTrue(store.respond(.agreed))
        XCTAssertTrue(store.loadInstall(.success))
        XCTAssertTrue(store.enableVersion())
        let revised = try XCTUnwrap(store.item(.boundary)?.enabledTarget)
        XCTAssertNotEqual(original.version.title, revised.version.title)
        XCTAssertTrue(store.item(.boundary)?.installedHistory.first == original,
                      "Historical evidence remains immutable, but it is not the current update baseline.")
        XCTAssertTrue(store.item(.boundary)?.updateComparisonTarget == revised)
        store.stageUpdate()
        XCTAssertTrue(store.item(.boundary)?.updateComparisonTarget == revised)
        XCTAssertTrue(store.item(.boundary)?.taskSelectionVersion == revised.version)
        XCTAssertTrue(store.respond(.agreed))
        XCTAssertTrue(store.loadInstall(.success))
        XCTAssertTrue(store.enableVersion())
        let updated = try XCTUnwrap(store.item(.boundary)?.enabledTarget)
        XCTAssertEqual(updated.version.number, "1.1")
        XCTAssertTrue(store.item(.boundary)?.taskSelectionVersion == updated.version)
        XCTAssertTrue(store.item(.boundary)?.updateComparisonTarget == revised,
                      "Enabling the candidate must not replace its captured comparison baseline with itself.")
        XCTAssertEqual(try requireWork(.inFlight, store), inFlight)
        XCTAssertTrue(inFlight.references.first?.target == original,
                      "The separate in-flight historical version is not the current project baseline.")
    }

    @MainActor
    func testS6UpdatedConsentStillRequiresSuccessfulResultAndExplicitEnablement() async throws {
        let store = SkillReviewStore(scenario: .updates)
        let old = try XCTUnwrap(store.item(.boundary)?.enabledTarget)
        store.stageUpdate()
        let candidate = try XCTUnwrap(store.selectedItem).target
        XCTAssertTrue(store.respond(.agreed))
        XCTAssertEqual(store.item(.boundary)?.approvedTarget, candidate)
        XCTAssertEqual(store.item(.boundary)?.enabledTarget, old)
        XCTAssertEqual(store.item(.boundary)?.installOutcome, .unloaded)
        XCTAssertTrue(store.loadInstall(.failure))
        XCTAssertFalse(store.enableVersion())
        XCTAssertEqual(store.item(.boundary)?.enabledTarget, old)
        XCTAssertTrue(store.loadInstall(.success))
        XCTAssertEqual(store.item(.boundary)?.enabledTarget, old)
        XCTAssertTrue(try requireWork(.newTask, store).references.isEmpty)
        XCTAssertTrue(store.enableVersion())
        XCTAssertEqual(store.item(.boundary)?.enabledTarget, candidate)
        XCTAssertTrue(try requireWork(.newTask, store).references.isEmpty)
        XCTAssertEqual(try requireWork(.inFlight, store).references.first?.target, old)
    }

    @MainActor
    func testS6NewTaskSelectsNewVersionButNeitherHistoricalTaskIsHotRewritten() async throws {
        let store = SkillReviewStore(scenario: .updates)
        XCTAssertTrue(store.select(.boundary))
        let existing = try requireWork(.api, store)
        let inFlight = try requireWork(.inFlight, store)
        store.stageUpdate()
        XCTAssertTrue(store.respond(.agreed))
        XCTAssertTrue(store.loadInstall(.success))
        XCTAssertTrue(store.enableVersion())
        XCTAssertEqual(try requireWork(.api, store), existing)
        XCTAssertEqual(try requireWork(.inFlight, store), inFlight)
        let oldReference = try XCTUnwrap(existing.references.first)
        XCTAssertNotNil(store.referenceBlock(oldReference, work: existing))
        XCTAssertNil(store.referenceBlock(try XCTUnwrap(inFlight.references.first), work: inFlight))
        store.setWork(.newTask)
        store.agentSelect()
        XCTAssertEqual(store.selectedWork?.references.first?.target.version.number, "1.1")
        XCTAssertEqual(store.selectedWork?.references.first?.target.project, .harbor)
        store.setProject(.tide)
        XCTAssertFalse(store.select(.boundary))
        XCTAssertNotNil(store.referenceBlock(try XCTUnwrap(inFlight.references.first), work: inFlight))
    }

    @MainActor
    func testS7OldVersionConditionsBlockWithoutVersionSwapOrHistoryDeletion() async throws {
        let store = SkillReviewStore(scenario: .inFlight)
        let work = try XCTUnwrap(store.selectedWork)
        let reference = try XCTUnwrap(work.references.first)
        let items = store.items
        XCTAssertNil(store.referenceBlock(reference, work: work))
        for condition in [SkillReviewInFlightCondition.revoked, .unsafe, .hostBlocked] {
            store.loadInFlight(condition)
            XCTAssertNotNil(store.referenceBlock(reference, work: work))
            XCTAssertEqual(store.works.first { $0.id == .inFlight }, work)
            XCTAssertEqual(store.items, items)
            XCTAssertFalse(store.select(.boundary))
            store.agentSelect()
            store.deselect(.boundary)
            XCTAssertEqual(store.selectedWork, work)
            store.setWork(.newTask)
            XCTAssertFalse(store.select(.boundary), "Old v1.0 is not a permanent exemption in a new task either.")
            store.setWork(.inFlight)
        }
        store.loadInFlight(.allowed)
        XCTAssertNil(store.referenceBlock(reference, work: work))
        XCTAssertEqual(reference.target.version.number, "1.0")
    }

    @MainActor
    func testS7StopIsPerWorkAndAllowedPresetCannotReviveIt() async throws {
        let store = SkillReviewStore(scenario: .inFlight)
        let stopped = try XCTUnwrap(store.selectedWork)
        let reference = try XCTUnwrap(stopped.references.first)
        store.requestStop()
        store.loadInFlight(.allowed)
        XCTAssertTrue(store.stopRequested)
        XCTAssertTrue(try XCTUnwrap(store.inFlightReason).contains("Stop"))
        XCTAssertTrue(try XCTUnwrap(store.referenceBlock(reference, work: stopped)).contains("Stop"))
        XCTAssertEqual(store.selectedWork, stopped)
        store.setWork(.newTask)
        XCTAssertFalse(store.stopRequested)
        XCTAssertTrue(store.select(.boundary))
        store.setWork(.inFlight)
        XCTAssertTrue(store.stopRequested)
        XCTAssertFalse(store.select(.boundary))
        store.load(.inFlight)
        XCTAssertFalse(store.stopRequested, "Only an explicit fixture reload/reset clears this intent.")
    }

    @MainActor
    func testAgentFeedbackDoesNotCountBlockedHistoricalReferencesAsNewEligibleSelections() async throws {
        let store = SkillReviewStore(scenario: .inFlight)
        store.setWork(.api)
        XCTAssertTrue(store.select(.boundary))
        let work = try XCTUnwrap(store.selectedWork)
        let reference = try XCTUnwrap(work.references.first)
        store.loadInFlight(.revoked)
        store.agentSelect()
        XCTAssertEqual(store.selectedWork, work)
        XCTAssertNotNil(store.referenceBlock(reference, work: work))
        XCTAssertTrue(store.feedback.contains("按需选用 0 项"))
        XCTAssertTrue(store.feedback.contains("仅保留为历史"))
    }

    @MainActor
    func testS7RevokedV1DoesNotSilentlyRevokeOrSubstituteSeparatelyApprovedV11() async throws {
        let store = SkillReviewStore(scenario: .updates)
        let oldWork = try requireWork(.inFlight, store)
        let oldReference = try XCTUnwrap(oldWork.references.first)
        store.stageUpdate()
        XCTAssertTrue(store.respond(.agreed))
        XCTAssertTrue(store.loadInstall(.success))
        XCTAssertTrue(store.enableVersion())
        store.loadInFlight(.revoked)
        store.setWork(.newTask)
        XCTAssertTrue(store.select(.boundary))
        XCTAssertEqual(store.selectedWork?.references.first?.target.version.number, "1.1")
        XCTAssertNotNil(store.referenceBlock(oldReference, work: oldWork))
        XCTAssertEqual(try requireWork(.inFlight, store), oldWork)
    }

    @MainActor
    func testHistoricalApprovalDoesNotReviveAWithdrawnVersionAfterShowingAnUpdate() async throws {
        for response in [SkillReviewConsentResponse.refused, .unanswered] {
            let store = SkillReviewStore(scenario: .inFlight)
            let work = try XCTUnwrap(store.selectedWork)
            let reference = try XCTUnwrap(work.references.first)
            XCTAssertTrue(store.respond(response))
            XCTAssertNotNil(store.referenceBlock(reference, work: work))
            store.stageUpdate()
            XCTAssertNotNil(store.referenceBlock(reference, work: work),
                            "Displaying a new proposal cannot resurrect withdrawn old authorization.")
            XCTAssertEqual(store.item(.boundary)?.approvedHistory, [reference.target])
            XCTAssertEqual(store.item(.boundary)?.withdrawnTargets, [reference.target])
            XCTAssertEqual(store.selectedWork, work)
            store.loadInFlight(.allowed)
            XCTAssertNotNil(store.referenceBlock(reference, work: work))
        }
    }

    @MainActor
    func testHistoricalInstallSuccessDoesNotHideALaterFailedRecordAfterStagingUpdate() async throws {
        let store = SkillReviewStore(scenario: .inFlight)
        let work = try XCTUnwrap(store.selectedWork)
        let reference = try XCTUnwrap(work.references.first)
        XCTAssertTrue(store.loadInstall(.failure))
        XCTAssertNotNil(store.referenceBlock(reference, work: work))
        store.stageUpdate()
        XCTAssertNotNil(store.referenceBlock(reference, work: work))
        XCTAssertEqual(store.item(.boundary)?.installedHistory, [reference.target])
        XCTAssertEqual(store.item(.boundary)?.unusableInstallTargets, [reference.target])
        XCTAssertEqual(store.selectedWork, work)
    }

    @MainActor
    func testS8ResetRestoresEveryMemoryDomainAndCancelsAnOpenSheet() async throws {
        let store = SkillReviewStore(scenario: .updates)
        let instance = store.instanceID
        let generation = store.generation
        store.stageUpdate()
        XCTAssertTrue(store.respond(.agreed))
        XCTAssertTrue(store.loadInstall(.success))
        XCTAssertTrue(store.enableVersion())
        store.setWork(.newTask)
        store.agentSelect()
        store.requestStop()
        store.loadInFlight(.unsafe)
        store.preview = .init(appearance: .dark, glass: false, interaction: .advanced,
                              fullAccess: true, reduceTransparency: true, increaseContrast: true,
                              reduceMotion: true, sizeID: "minimum")
        store.sheet = .appearance
        store.reset()
        XCTAssertEqual(store.instanceID, instance, "Reset is not a new window.")
        XCTAssertNotEqual(store.generation, generation)
        XCTAssertEqual(store.scenario, .drafts)
        XCTAssertEqual(store.section, .drafts)
        XCTAssertEqual(store.project, .harbor)
        XCTAssertEqual(store.executor, .codex)
        XCTAssertEqual(store.selectedItemID, .boundary)
        XCTAssertEqual(store.selectedWorkID, .api)
        XCTAssertEqual(store.preview, SkillReviewPreview())
        XCTAssertEqual(store.items, SkillReviewFixtures.items(for: .drafts))
        XCTAssertEqual(store.works, SkillReviewFixtures.works(items: store.items, scenario: .drafts))
        XCTAssertEqual(store.inFlightCondition, .allowed)
        XCTAssertTrue(store.stoppedWorkIDs.isEmpty)
        XCTAssertNil(store.sheet)
    }

    @MainActor
    func testS8TwoStoresAndAReopenedFreshStoreDoNotShareAnyMutableMemory() async throws {
        let first = SkillReviewStore(scenario: .consent)
        let second = SkillReviewStore()
        let secondItems = second.items
        XCTAssertTrue(first.respond(.agreed))
        XCTAssertTrue(first.loadInstall(.success))
        first.preview.fullAccess = true
        first.preview.glass = false
        first.requestStop()
        first.sheet = .appearance
        XCTAssertEqual(second.items, secondItems)
        XCTAssertEqual(second.preview, SkillReviewPreview())
        XCTAssertFalse(second.stopRequested)
        XCTAssertNil(second.sheet)
        let reopened = SkillReviewStore()
        XCTAssertNotEqual(reopened.instanceID, first.instanceID)
        XCTAssertNotEqual(reopened.instanceID, second.instanceID)
        XCTAssertEqual(reopened.items, second.items)
        XCTAssertEqual(reopened.works, second.works)
        XCTAssertEqual(reopened.preview, second.preview)
    }

    @MainActor
    func testS8AppearanceInteractionAndFullAccessAreIndependentOfDomainState() async throws {
        let store = SkillReviewStore(scenario: .tasks)
        let items = store.items
        let works = store.works
        store.preview.glass = false
        XCTAssertEqual(store.preview.interaction, .native)
        XCTAssertFalse(store.preview.fullAccess)
        store.preview.interaction = .advanced
        XCTAssertFalse(store.preview.glass)
        XCTAssertFalse(store.preview.fullAccess)
        store.preview.fullAccess = true
        store.preview.appearance = .dark
        store.preview.reduceTransparency = true
        store.preview.increaseContrast = true
        store.preview.reduceMotion = true
        XCTAssertEqual(store.items, items)
        XCTAssertEqual(store.works, works)
        let preview = store.preview
        store.load(.availability)
        XCTAssertEqual(store.preview, preview, "Scenario switching permits independent preview comparisons.")
        store.reset()
        XCTAssertEqual(store.preview, SkillReviewPreview())
    }

    // MARK: - Actual hosted roots: assembly only, not real-window/AX evidence

    @MainActor
    func testActualRootHostsAllEightScenariosAtThreeSizesWithLiveStoreChanges() async throws {
        var measurement = SkillReviewWindowMeasurement()
        measurement.layout = CGSize(width: 1260, height: 832)
        XCTAssertEqual(measurement.sheetSize(target: CGSize(width: 1260, height: 860)),
                       CGSize(width: 650, height: 650))
        XCTAssertEqual(measurement.sheetSize(target: CGSize(width: 400, height: 600)),
                       CGSize(width: 368, height: 568),
                       "The attached sheet must shrink before the parent resize callback.")
        measurement.layout = CGSize(width: 400, height: 572)
        XCTAssertEqual(measurement.sheetSize(target: CGSize(width: 400, height: 600)),
                       CGSize(width: 368, height: 540))
        let store = SkillReviewStore()
        let host = NSHostingView(rootView: NativeSkillReviewView(store: store))
        let instance = store.instanceID
        for size in [
            NSSize(width: 1260, height: 860), NSSize(width: 1000, height: 640), NSSize(width: 400, height: 600),
        ] {
            host.setFrameSize(size)
            for scenario in SkillReviewScenario.allCases {
                store.load(scenario)
                host.layoutSubtreeIfNeeded()
                XCTAssertEqual(store.instanceID, instance)
                XCTAssertEqual(store.selectedItemID, store.visibleItems.first?.id)
                XCTAssertNil(host.window)
                XCTAssertGreaterThan(host.fittingSize.width, 0)
                XCTAssertGreaterThan(host.fittingSize.height, 0)
            }
        }
        store.load(.consent)
        XCTAssertTrue(store.respond(.agreed))
        XCTAssertTrue(store.loadInstall(.failure))
        host.layoutSubtreeIfNeeded()
        XCTAssertEqual(store.item(.boundary)?.installOutcome, .failure)
        XCTAssertFalse(store.enableVersion())
        store.reset()
        host.layoutSubtreeIfNeeded()
        XCTAssertEqual(store.items, SkillReviewFixtures.items(for: .drafts))
        XCTAssertNil(host.window)
        // The NSHostingView has no window: this proves assembly consumption
        // only, never outer-frame geometry, keyboard, Material pixels or AX.
    }

    @MainActor
    func testActualRootConsumesStrengthenedAccessibilityPreviewInBothColorSchemes() async {
        for reduced in [false, true] {
            for scheme in [ColorScheme.light, .dark] {
                let store = SkillReviewStore(scenario: .availability)
                store.preview.reduceTransparency = reduced
                store.preview.increaseContrast = reduced
                store.preview.reduceMotion = reduced
                store.preview.appearance = scheme == .light ? .light : .dark
                let host = NSHostingView(rootView: NativeSkillReviewView(store: store).environment(\.colorScheme, scheme))
                host.frame = NSRect(x: 0, y: 0, width: 400, height: 600)
                host.layoutSubtreeIfNeeded()
                XCTAssertNil(host.window)
                XCTAssertGreaterThan(host.fittingSize.width, 0)
                XCTAssertGreaterThan(host.fittingSize.height, 0)
                XCTAssertEqual(store.preview.reduceMotion, reduced)
                XCTAssertFalse(store.preview.fullAccess)
            }
        }
        // Read-only system EnvironmentValues are never overridden via private
        // setters or changed globally. Pixel/animation validation belongs to UI.
    }

    @MainActor
    func testActualRefusalRootHostsEveryReasonAndCombinedReasonsAtMinimumSize() async {
        for reasons in SkillReviewRefusalReason.allCases.map({ [$0] }) + [SkillReviewRefusalReason.allCases] {
            let host = NSHostingView(rootView: NativeSkillReviewRefusalView(reasons: reasons))
            host.frame = NSRect(x: 0, y: 0, width: 400, height: 600)
            host.layoutSubtreeIfNeeded()
            XCTAssertNil(host.window)
            XCTAssertGreaterThan(host.fittingSize.width, 0)
            XCTAssertGreaterThan(host.fittingSize.height, 0)
        }
    }

    @MainActor
    func testActualMotionTransactionsAffectPresentationNotConsentOrPermission() async {
        let reduced = SkillReviewMotionPolicy.transaction(reduceMotion: true)
        let normal = SkillReviewMotionPolicy.transaction(reduceMotion: false)
        XCTAssertTrue(reduced.disablesAnimations)
        XCTAssertNil(reduced.animation)
        XCTAssertFalse(normal.disablesAnimations)
        XCTAssertNotNil(normal.animation)
        let store = SkillReviewStore(scenario: .consent)
        let items = store.items
        withTransaction(reduced) { store.preview.interaction = .advanced }
        withTransaction(normal) { store.preview.glass = false }
        XCTAssertFalse(store.preview.fullAccess)
        XCTAssertEqual(store.items, items)
        XCTAssertFalse(store.loadInstall(.success))
    }

    @MainActor
    private func openDraft(_ store: SkillReviewStore) throws -> SkillReviewEditDraft {
        store.beginEdit()
        guard case let .edit(draft) = store.sheet else {
            XCTFail("Expected an actual pending memory edit sheet.")
            throw NSError(domain: "SkillReviewUnitTest", code: 1)
        }
        return draft
    }

    @MainActor
    private func requireWork(_ id: SkillReviewWorkID, _ store: SkillReviewStore) throws -> SkillReviewWork {
        try XCTUnwrap(store.works.first { $0.id == id })
    }

    // MARK: - Private fixture I/O; never inspect the real account home/defaults

    private struct EntryFixture {
        let root: URL
        let home: URL
        let caches: URL

        var safeChild: URL { caches.appendingPathComponent("missing-skill-review-child", isDirectory: true) }

        var environment: [String: String] {
            [
                NativeSkillReviewEntry.flagKey: "1",
                "AERORIC_ENV": "ui-test",
                "AERORIC_DATA_DIR": safeChild.path,
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
            .appendingPathComponent("aeroric-skill-entry-\(UUID().uuidString)", isDirectory: true)
        let home = root.appendingPathComponent("fake-home", isDirectory: true)
        let caches = home.appendingPathComponent("Library/Caches", isDirectory: true)
        try manager.createDirectory(at: caches, withIntermediateDirectories: true)
        defer { try? manager.removeItem(at: root) }
        try body(.init(root: root, home: home, caches: caches))
    }

    private func decision(
        _ environment: [String: String], _ fixture: EntryFixture
    ) -> SkillReviewEntryDecision {
        NativeSkillReviewEntry.decide(environment: environment, home: fixture.home.path)
    }
}
#endif
