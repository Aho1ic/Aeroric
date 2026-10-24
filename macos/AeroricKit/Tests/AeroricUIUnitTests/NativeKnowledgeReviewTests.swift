#if DEBUG
import AppKit
import Foundation
import SwiftUI
import XCTest
@testable import AeroricUI

/// Offline value/transition and actual root assembly coverage only. These
/// tests do not launch an App, host or agent, inspect real homes/defaults, or
/// establish real-window, AX, appearance or model-consumption acceptance.
final class NativeKnowledgeReviewTests: XCTestCase {
    private let otherReviewKeys = [
        "AERORIC_NATIVE_EVOLUTION_REVIEW",
        "AERORIC_NATIVE_VISUAL_REVIEW",
        "AERORIC_NATIVE_COLLABORATION_REVIEW",
    ]

    // MARK: - Entry: every path check uses a temporary fake home

    func testAbsentKnowledgeFlagLeavesEveryLegacyCombinationInactive() {
        let values: [String?] = [nil, "0", "1", "", "invalid"]
        for evolution in values {
            for visual in values {
                for collaboration in values {
                    var environment = [
                        "AERORIC_ENV": "not-a-review",
                        "AERORIC_DATA_DIR": "not-an-absolute-path",
                    ]
                    environment[otherReviewKeys[0]] = evolution
                    environment[otherReviewKeys[1]] = visual
                    environment[otherReviewKeys[2]] = collaboration
                    XCTAssertEqual(
                        NativeKnowledgeReviewEntry.decide(environment: environment, home: "/unused-fictional-home"),
                        .inactive, "Missing new flag must not reinterpret the old chain."
                    )
                }
            }
        }
        XCTAssertEqual(NativeKnowledgeReviewEntry.decide(environment: [:], home: "/unused-fictional-home"), .inactive)
    }

    func testValidEntryAcceptsMissingFinalSafeChildWithoutCreatingIt() throws {
        try withEntryFixture { fixture in
            let manager = FileManager.default
            let before = try manager.contentsOfDirectory(atPath: fixture.caches.path)
            XCTAssertFalse(manager.fileExists(atPath: fixture.safeChild.path))
            for _ in 0..<3 {
                XCTAssertEqual(decision(fixture.environment, fixture), .review)
            }
            XCTAssertFalse(manager.fileExists(atPath: fixture.safeChild.path))
            XCTAssertEqual(try manager.contentsOfDirectory(atPath: fixture.caches.path), before)
        }
    }

    func testValidEntryAcceptsExistingSafeDirectoriesAndNestedMissingChildren() throws {
        try withEntryFixture { fixture in
            let existing = fixture.caches.appendingPathComponent("existing-safe")
            try FileManager.default.createDirectory(at: existing, withIntermediateDirectories: true)
            for path in [existing.path, existing.appendingPathComponent("missing/child").path] {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = path
                XCTAssertEqual(decision(environment, fixture), .review)
            }
            XCTAssertFalse(FileManager.default.fileExists(atPath: existing.appendingPathComponent("missing").path))
        }
    }

    func testPresentZeroEmptyAndOtherKnowledgeFlagsAreRefused() throws {
        try withEntryFixture { fixture in
            for value in ["0", "", "2", "true", " 1", "1 ", "\n1"] {
                var environment = fixture.environment
                environment[NativeKnowledgeReviewEntry.flagKey] = value
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidFlag]), value)
            }
        }
    }

    func testMissingAndInvalidEnvironmentAreRefused() throws {
        try withEntryFixture { fixture in
            let values: [String?] = [nil, "", "development", "production", "ui-test ", "UI-TEST"]
            for value in values {
                var environment = fixture.environment
                environment["AERORIC_ENV"] = value
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidEnvironment]))
            }
        }
    }

    func testMissingEmptyAndNonAbsoluteDirectoryAreRefused() throws {
        try withEntryFixture { fixture in
            let values: [String?] = [nil, "", " ", "\n", "Library/Caches/review", "./review"]
            for value in values {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = value
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidDirectory]))
            }
        }
    }

    func testDirectoryRootPrefixSquattingAndTraversalAreRefusedByEntry() throws {
        try withEntryFixture { fixture in
            let rejected = [
                "/", fixture.home.path, fixture.caches.path, fixture.caches.path + "/",
                fixture.home.path + "/Library/Caches-neighbour/review",
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
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidDirectory]), path)
            }
        }
    }

    func testSymlinkedFinalPathsAndAncestorsAreRefusedEvenInsideCaches() throws {
        try withEntryFixture { fixture in
            let manager = FileManager.default
            let inside = fixture.caches.appendingPathComponent("real-inside")
            let outside = fixture.root.appendingPathComponent("outside-fake-home")
            try manager.createDirectory(at: inside, withIntermediateDirectories: true)
            try manager.createDirectory(at: outside, withIntermediateDirectories: true)
            let links = [
                ("link-inside", inside),
                ("link-outside", outside),
                ("link-dangling", fixture.root.appendingPathComponent("nonexistent-target")),
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
            XCTAssertFalse(manager.fileExists(atPath: fixture.root.appendingPathComponent("nonexistent-target").path))
        }
    }

    func testFileFinalPathAndFileAncestorAreRefused() throws {
        try withEntryFixture { fixture in
            let file = fixture.caches.appendingPathComponent("plain-fixture-file")
            try Data("fictional test marker only".utf8).write(to: file)
            for path in [file.path, file.appendingPathComponent("missing-child").path] {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = path
                XCTAssertEqual(decision(environment, fixture), .refused([.invalidDirectory]))
            }
        }
    }

    func testEveryOldOrReservedReviewFlagAndCombinationRefusesInsteadOfOrdering() throws {
        try withEntryFixture { fixture in
            for mask in 1..<8 {
                var environment = fixture.environment
                for (index, key) in otherReviewKeys.enumerated() {
                    environment[key] = mask & (1 << index) == 0 ? "0" : "1"
                }
                XCTAssertEqual(decision(environment, fixture), .refused([.conflictingReview]), "mask \(mask)")
            }
        }
    }

    func testOldFlagsZeroEmptyAbsentAndOtherValuesAreNotConflictingRequests() throws {
        try withEntryFixture { fixture in
            let values: [String?] = [nil, "0", "", "true", " 1"]
            for value in values {
                var environment = fixture.environment
                for key in otherReviewKeys { environment[key] = value }
                XCTAssertEqual(decision(environment, fixture), .review)
            }
        }
    }

    func testMultipleInvalidConditionsAccumulateReadableNonSensitiveReasons() throws {
        try withEntryFixture { fixture in
            var environment = fixture.environment
            environment[NativeKnowledgeReviewEntry.flagKey] = ""
            environment["AERORIC_ENV"] = nil
            environment["AERORIC_DATA_DIR"] = fixture.home.path
            for key in otherReviewKeys { environment[key] = "1" }
            XCTAssertEqual(
                decision(environment, fixture),
                .refused([.invalidFlag, .invalidEnvironment, .invalidDirectory, .conflictingReview])
            )
            for reason in KnowledgeReviewRefusalReason.allCases {
                XCTAssertFalse(reason.title.isEmpty)
                XCTAssertFalse(reason.detail.isEmpty)
                XCTAssertFalse(reason.title.contains(fixture.home.path))
                XCTAssertFalse(reason.detail.contains(fixture.root.path))
                XCTAssertEqual(reason.id, reason.rawValue)
            }
        }
    }

    // MARK: - Fictional fixture identity, sources and scope

    func testEveryScenarioHasCoherentFictionalItemsAndVersionAssociations() {
        XCTAssertEqual(KnowledgeReviewScenario.allCases.map(\.code), (1...8).map { "K\($0)" })
        for scenario in KnowledgeReviewScenario.allCases {
            let items = KnowledgeReviewFixtures.items(for: scenario)
            XCTAssertFalse(items.isEmpty, scenario.code)
            XCTAssertEqual(Set(items.map(\.id)).count, items.count, scenario.code)
            XCTAssertTrue(items.contains { $0.id == KnowledgeReviewFixtures.defaultSelection(for: scenario) })
            for item in items {
                XCTAssertTrue(item.title.contains("虚构"), item.id.rawValue)
                XCTAssertEqual(item.current.itemID, item.id)
                XCTAssertFalse(item.content.isEmpty)
                XCTAssertFalse(item.applicability.summary.isEmpty)
                XCTAssertFalse(item.applicability.limits.isEmpty)
                XCTAssertTrue(item.source.title.contains("虚构"))
                XCTAssertTrue(item.source.isAvailable)
                XCTAssertEqual(Set(item.versions.map(\.id)).count, item.versions.count)
                for version in item.versions {
                    XCTAssertEqual(version.itemID, item.id)
                    if let replaced = version.replaces {
                        XCTAssertEqual(replaced.itemID, item.id)
                        XCTAssertTrue(item.history.contains { $0.id == replaced })
                        XCTAssertNotNil(version.correctionBasis)
                    }
                    if version.evidence == .preloadedVerified {
                        XCTAssertTrue(version.evidence.title.contains("PRELOADED"))
                        XCTAssertTrue(version.source.title.contains("非本次实测"))
                    }
                }
                if item.id == .outOfScope {
                    XCTAssertEqual(item.applicability.project, .other)
                    XCTAssertNotNil(item.confirmationBlockedReason)
                } else {
                    XCTAssertEqual(item.applicability.project, .harbor)
                }
            }
        }
    }

    func testK1HasTwoBatchSafeCandidatesAnIndependentDiscardAndUnverifiedSelfReport() throws {
        let items = KnowledgeReviewFixtures.items(for: .candidates)
        let safe = items.filter { $0.state == .candidate && $0.confirmationBlockedReason == nil }
        XCTAssertGreaterThanOrEqual(safe.count, 3)
        XCTAssertTrue(safe.contains { $0.id == .conciseReplies })
        XCTAssertTrue(safe.contains { $0.id == .verifiedCheck })
        XCTAssertTrue(safe.contains { $0.id == .discardable })
        let selfReport = try XCTUnwrap(items.first { $0.id == .selfReport })
        XCTAssertEqual(selfReport.evidence, .unverifiedAgentReport)
        XCTAssertNotNil(selfReport.confirmationBlockedReason)
    }

    func testConcisePreferenceHasTwentySixSafeParagraphsForRealScrolling() throws {
        let excerpt = try XCTUnwrap(KnowledgeReviewFixtures.conciseReplies.source.excerpt)
        XCTAssertGreaterThanOrEqual(excerpt.components(separatedBy: "\n\n").count, 26)
        XCTAssertTrue(excerpt.hasPrefix("虚构来源首段"))
        XCTAssertTrue(excerpt.contains("虚构来源末段"))
        XCTAssertFalse(excerpt.contains("http://"))
        XCTAssertFalse(excerpt.contains("https://"))
    }

    func testAdmissionPresetsAreExactlyTwoFixedLocalCombinations() {
        XCTAssertEqual(KnowledgeReviewAdmissionPreset.allCases, [.userPreferences, .verifiedSolutions])
        for preset in KnowledgeReviewAdmissionPreset.allCases {
            XCTAssertEqual(preset.project, .harbor)
            XCTAssertFalse(preset.detail.isEmpty)
        }
        XCTAssertEqual(KnowledgeReviewAdmissionPreset.userPreferences.kind, .preference)
        XCTAssertEqual(KnowledgeReviewAdmissionPreset.userPreferences.sourceKind, .userStatement)
        XCTAssertEqual(KnowledgeReviewAdmissionPreset.verifiedSolutions.kind, .solution)
        XCTAssertEqual(KnowledgeReviewAdmissionPreset.verifiedSolutions.sourceKind, .preloadedCheck)
        XCTAssertEqual(Set(KnowledgeReviewFixtures.newBatch.map(\.id)).count, KnowledgeReviewFixtures.newBatch.count)
    }

    func testExceptionFixturesCarryDifferentReasonsAndOnlyAnInnocuousSensitivePlaceholder() {
        let exceptions = KnowledgeReviewFixtures.items(for: .exceptions).filter { $0.state == .candidate }
        XCTAssertEqual(Set(exceptions.map(\.id)), [.conflict, .sensitive, .insufficient, .outOfScope])
        XCTAssertEqual(Set(exceptions.compactMap(\.confirmationBlockedReason)).count, 4)
        XCTAssertEqual(KnowledgeReviewFixtures.sensitive.content, "［已移除的敏感内容占位］")
        XCTAssertTrue(KnowledgeReviewFixtures.sensitive.source.excerpt?.contains("没有任何凭据") == true)
        XCTAssertEqual(KnowledgeReviewFixtures.insufficient.evidence, .unverifiedAgentReport)
    }

    // Actor-isolated tests are async so XCTest discovery uses its async
    // adapter instead of casting a synchronous @MainActor method.

    // MARK: - Detail focus, batch operations and content draft

    @MainActor
    func testEveryScenarioStartsWithVisibleDetailAndDefaultIndependentConfiguration() async {
        for scenario in KnowledgeReviewScenario.allCases {
            let store = KnowledgeReviewStore(scenario: scenario)
            XCTAssertNotNil(store.selectedItem, scenario.code)
            XCTAssertTrue(store.visibleItems.contains { $0.id == store.selectedItemID })
            XCTAssertTrue(store.selectedCandidateIDs.isEmpty)
            XCTAssertFalse(store.admissionConfiguration.enabled)
            XCTAssertEqual(store.permission, .standard)
            XCTAssertFalse(store.hasProcessedNewCandidates)
            XCTAssertNil(store.contentDraft)
            XCTAssertNil(store.configurationDraft)
            XCTAssertNil(store.revisionDraft)
        }
    }

    @MainActor
    func testDetailSelectionAndBatchCheckboxSelectionAreIndependent() async {
        let store = KnowledgeReviewStore()
        let originalFocus = store.selectedItemID
        store.setCandidateSelected(.verifiedCheck, selected: true)
        XCTAssertEqual(store.selectedItemID, originalFocus)
        store.selectItem(.discardable)
        XCTAssertEqual(store.selectedCandidateIDs, [.verifiedCheck])
        store.setCandidateSelected(.verifiedCheck, selected: false)
        XCTAssertEqual(store.selectedItemID, .discardable)
        XCTAssertTrue(store.selectedCandidateIDs.isEmpty)
        store.setCandidateSelected(.databaseChoice, selected: true)
        XCTAssertTrue(store.selectedCandidateIDs.isEmpty, "A reusable item is not a checkbox candidate.")
    }

    @MainActor
    func testSelectionRejectsItemsOutsideCurrentSectionAndRepairsStaleBinding() async {
        let store = KnowledgeReviewStore()
        store.selectItem(.databaseChoice)
        XCTAssertEqual(store.selectedItemID, .conciseReplies)
        store.selectedItemID = .databaseChoice
        XCTAssertNil(store.selectedItem, "A direct native binding must not expose cross-section detail.")
        store.beginContentEdit()
        XCTAssertNil(store.contentDraft)
        store.setSection(.candidates)
        XCTAssertEqual(store.selectedItemID, .conciseReplies)
        store.setSection(.history)
        XCTAssertTrue(store.visibleItems.isEmpty)
        XCTAssertNil(store.selectedItemID)
        store.setSection(.reusable)
        XCTAssertNotNil(store.selectedItem)
    }

    @MainActor
    func testBatchConfirmationPreservesSourceScopeLimitsAndEvidence() async throws {
        let store = KnowledgeReviewStore()
        let first = try XCTUnwrap(store.item(id: .conciseReplies))
        let second = try XCTUnwrap(store.item(id: .verifiedCheck))
        for id in [KnowledgeReviewItemID.conciseReplies, .verifiedCheck] {
            store.setCandidateSelected(id, selected: true)
        }
        XCTAssertNil(store.batchConfirmationBlockedReason)
        XCTAssertTrue(store.confirmSelectedCandidates())
        for before in [first, second] {
            let after = try XCTUnwrap(store.item(id: before.id))
            XCTAssertEqual(after.state, .reusable)
            XCTAssertEqual(after.admissionOrigin, .manual)
            XCTAssertEqual(after.current, before.current)
            XCTAssertEqual(after.history, before.history)
            XCTAssertEqual(after.applicability.project, .harbor)
        }
        XCTAssertEqual(store.item(id: .selfReport)?.evidence, .unverifiedAgentReport)
        XCTAssertEqual(store.item(id: .selfReport)?.state, .candidate)
        XCTAssertTrue(store.selectedCandidateIDs.isEmpty)
        XCTAssertEqual(store.selectedItemID, .discardable)
    }

    @MainActor
    func testUnsafeCandidateMakesBatchConfirmationAtomicInsteadOfSilentlyForcingIt() async {
        let store = KnowledgeReviewStore()
        let before = store.items
        store.setCandidateSelected(.conciseReplies, selected: true)
        store.setCandidateSelected(.selfReport, selected: true)
        XCTAssertNotNil(store.batchConfirmationBlockedReason)
        XCTAssertFalse(store.confirmSelectedCandidates())
        XCTAssertEqual(store.items, before)
        XCTAssertEqual(store.selectedCandidateIDs, [.conciseReplies, .selfReport])
        store.setCandidateSelected(.selfReport, selected: false)
        XCTAssertTrue(store.confirmSelectedCandidates())
        XCTAssertEqual(store.item(id: .selfReport)?.state, .candidate)
        XCTAssertEqual(store.item(id: .selfReport)?.evidence, .unverifiedAgentReport)
    }

    @MainActor
    func testEmptyBatchAndWrongSectionDoNotChangeKnowledge() async {
        let store = KnowledgeReviewStore()
        let before = store.items
        XCTAssertFalse(store.confirmSelectedCandidates())
        XCTAssertFalse(store.discardSelectedCandidates())
        store.setCandidateSelected(.conciseReplies, selected: true)
        store.setSection(.reusable)
        XCTAssertFalse(store.confirmSelectedCandidates())
        XCTAssertFalse(store.discardSelectedCandidates())
        XCTAssertEqual(store.items, before)
    }

    @MainActor
    func testDiscardPreservesHistoryWithoutAdmittingCandidate() async throws {
        let store = KnowledgeReviewStore()
        let before = try XCTUnwrap(store.item(id: .discardable))
        store.setCandidateSelected(.discardable, selected: true)
        XCTAssertTrue(store.discardSelectedCandidates())
        let after = try XCTUnwrap(store.item(id: .discardable))
        XCTAssertEqual(after.state, .discarded)
        XCTAssertEqual(after.current, before.current)
        XCTAssertNil(after.admissionOrigin)
        XCTAssertFalse(store.reusableItems.contains { $0.id == .discardable })
        store.setSection(.history)
        store.selectItem(.discardable)
        XCTAssertEqual(store.selectedItem?.state, .discarded)
        XCTAssertEqual(store.selectedItem?.source, before.source)
    }

    @MainActor
    func testContentDraftCancelAndApplyAreDifferentAndKeepSourceAssociation() async throws {
        let store = KnowledgeReviewStore()
        let before = try XCTUnwrap(store.selectedItem)
        store.beginContentEdit()
        store.contentDraft?.content = "虚构编辑：先列当前要求。"
        store.contentDraft?.applicability = "仅海港项目的审阅说明。"
        XCTAssertEqual(store.selectedItem, before)
        store.cancelContentDraft()
        XCTAssertEqual(store.selectedItem, before)
        XCTAssertNil(store.contentDraft)

        store.beginContentEdit()
        store.contentDraft?.content = "  虚构编辑：先列当前要求。  "
        store.contentDraft?.applicability = "  仅海港项目的审阅说明。  "
        XCTAssertTrue(store.applyContentDraft())
        let after = try XCTUnwrap(store.selectedItem)
        XCTAssertEqual(after.content, "虚构编辑：先列当前要求。")
        XCTAssertEqual(after.applicability.summary, "仅海港项目的审阅说明。")
        XCTAssertEqual(after.source, before.source)
        XCTAssertEqual(after.applicability.project, before.applicability.project)
        XCTAssertEqual(after.applicability.limits, before.applicability.limits)
        XCTAssertEqual(after.state, .candidate)
        XCTAssertEqual(after.current.number, 1)
        XCTAssertTrue(after.history.isEmpty)
        XCTAssertNil(store.contentDraft)
    }

    @MainActor
    func testEmptyContentOrApplicabilityDraftIsRejectedWithoutMutation() async throws {
        let store = KnowledgeReviewStore()
        let before = store.items
        store.beginContentEdit()
        let valid = try XCTUnwrap(store.contentDraft)
        for blank in ["", " \n\t", "\u{00A0}"] {
            store.contentDraft = valid
            store.contentDraft?.content = blank
            XCTAssertFalse(store.applyContentDraft())
            XCTAssertEqual(store.items, before)
            store.contentDraft = valid
            store.contentDraft?.applicability = blank
            XCTAssertFalse(store.applyContentDraft())
            XCTAssertEqual(store.items, before)
        }
        XCTAssertNotNil(store.contentDraft, "Validation failure should leave an editable draft.")
    }

    @MainActor
    func testEditingSelfReportCannotRemoveItsUnverifiedEvidenceOrSafetyIssue() async {
        let store = KnowledgeReviewStore()
        store.selectItem(.selfReport)
        store.beginContentEdit()
        store.contentDraft?.content = "虚构重写：声称成功，但仍未核验。"
        XCTAssertTrue(store.applyContentDraft())
        XCTAssertEqual(store.selectedItem?.evidence, .unverifiedAgentReport)
        XCTAssertEqual(store.selectedItem?.issue, .insufficientEvidence)
        store.setCandidateSelected(.selfReport, selected: true)
        XCTAssertFalse(store.confirmSelectedCandidates())
    }

    @MainActor
    func testEditingPreloadedSolutionDoesNotReuseVerificationForChangedText() async throws {
        let store = KnowledgeReviewStore()
        store.selectItem(.verifiedCheck)
        let source = try XCTUnwrap(store.selectedItem?.source)
        store.beginContentEdit()
        store.contentDraft?.content = "虚构编辑后的新解法，尚未执行验证。"
        XCTAssertTrue(store.applyContentDraft())
        XCTAssertEqual(store.selectedItem?.evidence, .userStatement)
        XCTAssertEqual(store.selectedItem?.source, source)
        store.beginConfiguration()
        store.configurationDraft?.configuration = .init(enabled: true, preset: .verifiedSolutions)
        XCTAssertTrue(store.applyConfigurationDraft())
        XCTAssertNotNil(store.automaticAdmissionReason(for: try XCTUnwrap(store.selectedItem)))
    }

    @MainActor
    func testCancelledOrReplacedContentDraftCannotBeReapplied() async throws {
        let store = KnowledgeReviewStore()
        let before = store.items
        store.beginContentEdit()
        var old = try XCTUnwrap(store.contentDraft)
        old.content = "虚构迟到草稿。"
        store.cancelContentDraft()
        store.contentDraft = old
        XCTAssertFalse(store.applyContentDraft())
        XCTAssertEqual(store.items, before)

        store.beginContentEdit()
        let currentID = store.contentDraft?.id
        XCTAssertNotEqual(currentID, old.id)
        store.contentDraft = old
        XCTAssertFalse(store.applyContentDraft())
        XCTAssertEqual(store.items, before)
    }

    @MainActor
    func testConfirmingAnEditedCandidateInvalidatesItsOpenDraft() async throws {
        let store = KnowledgeReviewStore()
        store.beginContentEdit()
        let stale = try XCTUnwrap(store.contentDraft)
        store.setCandidateSelected(.conciseReplies, selected: true)
        XCTAssertTrue(store.confirmSelectedCandidates())
        XCTAssertNil(store.contentDraft)
        let afterConfirmation = store.items
        store.contentDraft = stale
        XCTAssertFalse(store.applyContentDraft())
        XCTAssertEqual(store.items, afterConfirmation)
    }

    // MARK: - Configuration is a draft and independent from task permission

    @MainActor
    func testConfigurationDraftCancelAndApplyDoNotMutatePermissionOrExistingKnowledge() async {
        let store = KnowledgeReviewStore(scenario: .admission)
        let before = store.items
        store.setPermission(.fullAccess)
        store.beginConfiguration()
        store.configurationDraft?.configuration = .init(enabled: true, preset: .verifiedSolutions)
        XCTAssertEqual(store.admissionConfiguration, .init())
        store.cancelConfigurationDraft()
        XCTAssertEqual(store.admissionConfiguration, .init())
        XCTAssertEqual(store.permission, .fullAccess)
        store.beginConfiguration()
        store.configurationDraft?.configuration = .init(enabled: true, preset: .verifiedSolutions)
        XCTAssertTrue(store.applyConfigurationDraft())
        XCTAssertEqual(store.admissionConfiguration, .init(enabled: true, preset: .verifiedSolutions))
        XCTAssertEqual(store.permission, .fullAccess)
        XCTAssertEqual(store.items, before)
        XCTAssertNil(store.configurationDraft)
        store.setPermission(.standard)
        XCTAssertTrue(store.admissionConfiguration.enabled)
        XCTAssertEqual(store.admissionConfiguration.preset, .verifiedSolutions)
    }

    @MainActor
    func testAllFourPermissionAdmissionCombinationsProduceIndependentFixedResults() async {
        for permission in KnowledgeReviewPermission.allCases {
            for enabled in [false, true] {
                let store = configuredStore(enabled: enabled, permission: permission)
                XCTAssertEqual(store.permission, permission)
                XCTAssertEqual(store.admissionConfiguration.enabled, enabled)
                XCTAssertTrue(store.processNewCandidates())
                XCTAssertEqual(store.item(id: .newPreference)?.state, enabled ? .reusable : .candidate)
                XCTAssertEqual(store.item(id: .newPreference)?.admissionOrigin, enabled ? .automatic : nil)
                XCTAssertEqual(store.item(id: .newSolution)?.state, .candidate)
                for id in [KnowledgeReviewItemID.conflict, .sensitive, .insufficient, .outOfScope] {
                    XCTAssertEqual(store.item(id: id)?.state, .candidate)
                }
                XCTAssertEqual(store.permission, permission)
                XCTAssertEqual(store.item(id: .outOfScope)?.applicability.project, .other)
            }
        }
    }

    @MainActor
    func testVerifiedSolutionPresetAdmitsOnlyTheMatchingPreloadedSolution() async throws {
        let store = configuredStore(enabled: true, preset: .verifiedSolutions, permission: .standard)
        XCTAssertTrue(store.processNewCandidates())
        let solution = try XCTUnwrap(store.item(id: .newSolution))
        XCTAssertEqual(solution.state, .reusable)
        XCTAssertEqual(solution.admissionOrigin, .automatic)
        XCTAssertEqual(solution.evidence, .preloadedVerified)
        XCTAssertTrue(solution.source.title.contains("PRELOADED"))
        XCTAssertTrue(solution.source.title.contains("非本次实测"))
        XCTAssertEqual(store.item(id: .newPreference)?.state, .candidate)
        XCTAssertEqual(store.permission, .standard)
    }

    @MainActor
    func testNewBatchIsBoundedAndRepeatedProcessingNeverDuplicatesOrRetroactivelyAdmits() async {
        let store = configuredStore(enabled: false, permission: .fullAccess)
        let initialCount = store.items.count
        XCTAssertTrue(store.processNewCandidates())
        XCTAssertEqual(store.items.count, initialCount + KnowledgeReviewFixtures.newBatch.count)
        let after = store.items
        store.beginConfiguration()
        store.configurationDraft?.configuration.enabled = true
        XCTAssertTrue(store.applyConfigurationDraft())
        for _ in 0..<3 { XCTAssertFalse(store.processNewCandidates()) }
        XCTAssertEqual(store.items, after)
        XCTAssertEqual(store.item(id: .newPreference)?.state, .candidate)
        XCTAssertEqual(Set(store.items.map(\.id)).count, store.items.count)
        XCTAssertTrue(store.notice.contains("追溯"))
    }

    @MainActor
    func testEnablingAdmissionNeverProcessesExistingSafeCandidates() async {
        let store = KnowledgeReviewStore()
        let before = store.items
        store.beginConfiguration()
        store.configurationDraft?.configuration.enabled = true
        XCTAssertTrue(store.applyConfigurationDraft())
        XCTAssertEqual(store.items, before)
        XCTAssertTrue(store.processNewCandidates())
        XCTAssertEqual(store.item(id: .conciseReplies)?.state, .candidate)
        XCTAssertEqual(store.item(id: .verifiedCheck)?.state, .candidate)
        XCTAssertEqual(store.item(id: .newPreference)?.state, .reusable)
    }

    @MainActor
    func testExceptionsCannotBeForcedByEitherPresetOrFullAccessAndAreNotDuplicated() async {
        for preset in KnowledgeReviewAdmissionPreset.allCases {
            let store = configuredStore(
                enabled: true, preset: preset, permission: .fullAccess, scenario: .exceptions
            )
            let exceptionIDs: [KnowledgeReviewItemID] = [.conflict, .sensitive, .insufficient, .outOfScope]
            let before = exceptionIDs.compactMap { store.item(id: $0) }
            XCTAssertTrue(store.processNewCandidates())
            XCTAssertEqual(exceptionIDs.compactMap { store.item(id: $0) }, before)
            XCTAssertEqual(Set(store.items.map(\.id)).count, store.items.count)
            for id in exceptionIDs {
                store.setCandidateSelected(id, selected: true)
                XCTAssertFalse(store.confirmSelectedCandidates(), id.rawValue)
                store.setCandidateSelected(id, selected: false)
            }
            XCTAssertEqual(exceptionIDs.compactMap { store.item(id: $0) }, before)
        }
    }

    @MainActor
    func testDiscardedExceptionIsNotResurrectedByTheFixedNewBatch() async {
        let store = configuredStore(enabled: true, permission: .fullAccess, scenario: .exceptions)
        store.setCandidateSelected(.sensitive, selected: true)
        XCTAssertTrue(store.discardSelectedCandidates())
        let discarded = store.item(id: .sensitive)
        XCTAssertTrue(store.processNewCandidates())
        XCTAssertEqual(store.item(id: .sensitive), discarded)
        XCTAssertEqual(store.items.filter { $0.id == .sensitive }.count, 1)
    }

    @MainActor
    func testClosingAutomaticAdmissionDoesNotRetireKnowledgeOrRewriteInFlightRows() async {
        let store = configuredStore(enabled: true, permission: .standard)
        XCTAssertTrue(store.processNewCandidates())
        let knowledge = store.items
        let impacts = store.impactRows
        let captured = store.capturedReferences
        store.beginConfiguration()
        store.configurationDraft?.configuration.enabled = false
        XCTAssertTrue(store.applyConfigurationDraft())
        XCTAssertEqual(store.items, knowledge)
        XCTAssertEqual(store.impactRows, impacts)
        XCTAssertEqual(store.capturedReferences, captured)
        XCTAssertEqual(store.item(id: .newPreference)?.state, .reusable)
        XCTAssertTrue(store.autoAdmissionPropagationNote.contains("未在本样例演示"))
        XCTAssertTrue(store.notice.contains(store.autoAdmissionPropagationNote))
    }

    @MainActor
    func testCancelledConfigurationCannotBeReappliedFromLateSheetCallback() async throws {
        let store = KnowledgeReviewStore()
        store.beginConfiguration()
        var stale = try XCTUnwrap(store.configurationDraft)
        stale.configuration.enabled = true
        store.cancelConfigurationDraft()
        store.configurationDraft = stale
        XCTAssertFalse(store.applyConfigurationDraft())
        XCTAssertEqual(store.admissionConfiguration, .init())
    }

    // MARK: - Scoped task/work-package reference projection

    @MainActor
    func testTaskAndWorkPackageReferencesContainOnlyTheirRelatedProjectKnowledge() async {
        let store = KnowledgeReviewStore(scenario: .references)
        XCTAssertEqual(store.works.filter { $0.kind == .task }.count, 2)
        XCTAssertEqual(store.works.filter { $0.kind == .workPackage }.count, 2)
        let allItems = store.items
        for work in store.works {
            store.setWork(work.id)
            XCTAssertEqual(store.selectedWork, work)
            XCTAssertEqual(store.referenceRows.map(\.itemID), work.relatedItemIDs)
            XCTAssertLessThan(store.referenceRows.count, allItems.count, "Do not copy the entire library.")
            for reference in store.referenceRows {
                XCTAssertEqual(reference.source, store.item(id: reference.itemID)?.source)
                XCTAssertEqual(reference.applicability, store.item(id: reference.itemID)?.applicability)
                XCTAssertEqual(reference.applicability.project, .harbor)
                XCTAssertEqual(reference.condition, .reference)
            }
        }
        store.setWork(.docsPackage)
        XCTAssertEqual(store.recommendedReferences.map(\.itemID), [.docsGuidance])
        XCTAssertFalse(store.referenceRows.contains { $0.itemID == .interfaceContract || $0.itemID == .databaseChoice })
        XCTAssertEqual(store.items, allItems, "Projection must not modify knowledge.")
    }

    @MainActor
    func testCurrentExplicitPostgresRequestCannotBeOverriddenByHistoricalSQLite() async throws {
        let store = KnowledgeReviewStore(scenario: .references)
        let historical = try XCTUnwrap(store.item(id: .databaseChoice))
        store.setWork(.storageTask)
        XCTAssertEqual(store.recommendedReferences.map(\.itemID), [.databaseChoice])
        store.setReferenceScenario(.currentUserRequest)
        XCTAssertTrue(store.currentRequirement.contains("使用 Postgres"))
        let reference = try XCTUnwrap(store.referenceRows.first)
        XCTAssertEqual(reference.condition, .currentRequestPriority)
        XCTAssertTrue(reference.note.contains("Postgres"))
        XCTAssertTrue(reference.summary.contains("SQLite"))
        XCTAssertFalse(reference.isRecommended)
        XCTAssertTrue(store.recommendedReferences.isEmpty)
        XCTAssertEqual(store.item(id: .databaseChoice), historical)
        store.setPermission(.fullAccess)
        XCTAssertEqual(store.referenceRows.first?.condition, .currentRequestPriority)
        XCTAssertTrue(store.recommendedReferences.isEmpty)
        XCTAssertTrue(KnowledgeReviewFixtures.referenceBoundaryNote.contains("不是系统指令"))
    }

    @MainActor
    func testMissingSourceIsAnExplicitProjectionWithoutInventingOrDestroyingTheOriginal() async throws {
        let store = KnowledgeReviewStore(scenario: .references)
        let original = try XCTUnwrap(store.item(id: .databaseChoice))
        store.setReferenceScenario(.missingSource)
        let reference = try XCTUnwrap(store.referenceRows.first)
        XCTAssertEqual(reference.condition, .missingSource)
        XCTAssertNil(reference.source.excerpt)
        XCTAssertFalse(reference.source.isAvailable)
        XCTAssertEqual(reference.source.id, original.source.id)
        XCTAssertEqual(reference.applicability, original.applicability)
        XCTAssertTrue(store.recommendedReferences.isEmpty)
        XCTAssertEqual(store.item(id: .databaseChoice), original)
        let missing = KnowledgeReviewSourcePresentation(source: reference.source, context: "缺失来源投影")
        XCTAssertEqual(missing.source.title, original.source.title)
        XCTAssertNil(missing.source.excerpt)
        XCTAssertEqual(missing.context, "缺失来源投影")
        store.setReferenceScenario(.sourced)
        XCTAssertEqual(store.referenceRows.first?.source, original.source)
        XCTAssertEqual(store.recommendedReferences.count, 1)
        let restored = KnowledgeReviewSourcePresentation(source: original.source, context: "已恢复的内嵌来源")
        XCTAssertEqual(missing.source.id, restored.source.id)
        XCTAssertNotEqual(missing.id, restored.id, "Source identity is not presentation identity.")
        XCTAssertNil(missing.source.excerpt, "A later presentation must not mutate the missing projection.")
        XCTAssertEqual(restored.source, original.source)
        XCTAssertEqual(restored.context, "已恢复的内嵌来源")
    }

    @MainActor
    func testOutdatedConflictAndMissingSourceHaveDistinctExplanationsAndNoCertainRecommendation() async throws {
        let store = KnowledgeReviewStore(scenario: .references)
        let before = store.items
        store.setReferenceScenario(.missingSource)
        let missing = try XCTUnwrap(store.referenceRows.first)
        store.setReferenceScenario(.outdatedConflict)
        let outdated = try XCTUnwrap(store.referenceRows.first)
        XCTAssertEqual(outdated.condition, .outdatedConflict)
        XCTAssertNotEqual(outdated.note, missing.note)
        XCTAssertTrue(outdated.source.isAvailable, "Outdated is not the same as missing.")
        XCTAssertFalse(outdated.isRecommended)
        XCTAssertTrue(store.recommendedReferences.isEmpty)
        XCTAssertEqual(store.items, before)
        store.setWork(.clientPackage)
        XCTAssertEqual(store.referenceRows.map(\.itemID), [.interfaceContract])
        XCTAssertEqual(store.referenceRows.first?.condition, .outdatedConflict)
    }

    @MainActor
    func testCandidatesAndOtherProjectItemsNeverEnterTheReferenceProjection() async {
        let store = configuredStore(
            enabled: true, permission: .fullAccess, scenario: .exceptions
        )
        XCTAssertTrue(store.processNewCandidates())
        for work in store.works {
            store.setWork(work.id)
            for reference in store.referenceRows {
                XCTAssertEqual(store.item(id: reference.itemID)?.state, .reusable)
                XCTAssertEqual(reference.applicability.project, .harbor)
                XCTAssertNotEqual(reference.itemID, .outOfScope)
                XCTAssertNotEqual(reference.itemID, .conflict)
                XCTAssertNotEqual(reference.itemID, .sensitive)
                XCTAssertNotEqual(reference.itemID, .insufficient)
            }
        }
    }

    // MARK: - Explicit user correction, bounded v2 and retained v1

    @MainActor
    func testRevisionCancellationCreatesNoVersionAndDoesNotChangeCapturedReferences() async {
        let store = KnowledgeReviewStore(scenario: .revisions)
        let before = store.items
        let impacts = store.impactRows
        let captured = store.capturedReferences
        store.beginRevision()
        store.revisionDraft?.basis = "虚构用户说明：采用稳定字段。"
        store.revisionDraft?.content = "虚构编辑内容。"
        XCTAssertEqual(store.items, before)
        store.cancelRevisionDraft()
        XCTAssertNil(store.revisionDraft)
        XCTAssertEqual(store.items, before)
        XCTAssertEqual(store.impactRows, impacts)
        XCTAssertEqual(store.capturedReferences, captured)
    }

    @MainActor
    func testRevisionRejectsEmptyContentApplicabilityAndUserBasis() async throws {
        let store = KnowledgeReviewStore(scenario: .revisions)
        let before = store.items
        store.beginRevision()
        XCTAssertEqual(store.revisionDraft?.basis, "")
        XCTAssertFalse(store.applyRevisionDraft(), "A displayed suggestion is not user correction evidence.")
        var valid = try XCTUnwrap(store.revisionDraft)
        valid.basis = "虚构用户明确纠正：使用稳定错误字段。"
        for blank in ["", " \n\t"] {
            store.revisionDraft = valid
            store.revisionDraft?.content = blank
            XCTAssertFalse(store.applyRevisionDraft())
            store.revisionDraft = valid
            store.revisionDraft?.applicability = blank
            XCTAssertFalse(store.applyRevisionDraft())
            store.revisionDraft = valid
            store.revisionDraft?.basis = blank
            XCTAssertFalse(store.applyRevisionDraft())
            XCTAssertEqual(store.items, before)
        }
        store.revisionDraft = valid
        store.revisionDraft?.content = KnowledgeReviewFixtures.interfaceV1.content
        store.revisionDraft?.applicability = KnowledgeReviewFixtures.interfaceV1.applicability.summary
        XCTAssertFalse(store.applyRevisionDraft(), "A no-op correction must not manufacture v2.")
        XCTAssertEqual(store.items, before)
    }

    @MainActor
    func testApplyingUserCorrectionPreservesV1SourceAndLinksBasisReplacementAndV2() async throws {
        let store = KnowledgeReviewStore(scenario: .revisions)
        let before = try XCTUnwrap(store.item(id: .interfaceContract))
        let docs = store.item(id: .docsGuidance)
        let captured = store.capturedReferences
        store.beginRevision()
        store.revisionDraft?.basis = "  虚构用户纠正：客户端按稳定 code 字段处理错误。  "
        XCTAssertTrue(store.applyRevisionDraft())
        let after = try XCTUnwrap(store.item(id: .interfaceContract))
        XCTAssertEqual(after.versions.map(\.number), [1, 2])
        XCTAssertEqual(after.current.number, 2)
        XCTAssertEqual(after.current.replaces, before.current.id)
        XCTAssertEqual(after.current.correctionBasis, "虚构用户纠正：客户端按稳定 code 字段处理错误。")
        XCTAssertEqual(after.current.source.kind, .userCorrection)
        XCTAssertEqual(after.current.source.excerpt, after.current.correctionBasis)
        XCTAssertEqual(after.current.evidence, .userStatement, "Correction is not a new executed verification.")
        XCTAssertEqual(after.current.applicability.project, before.applicability.project)
        XCTAssertEqual(after.current.applicability.limits, before.applicability.limits)
        var expectedOld = before.current
        expectedOld.state = .superseded
        XCTAssertEqual(after.history, [expectedOld])
        XCTAssertEqual(store.item(id: .docsGuidance), docs)
        XCTAssertEqual(store.capturedReferences, captured)
        XCTAssertNil(store.revisionDraft)
        XCTAssertTrue(store.impactRows.filter(\.isAffected).allSatisfy {
            $0.state == .pending && $0.requiredVersion == after.current.id && $0.loadedRecord == nil
        })
        store.setSection(.history)
        store.selectItem(.interfaceContract)
        XCTAssertEqual(store.selectedItem?.history.first?.source, before.source)
        let historical = KnowledgeReviewSourcePresentation(source: before.source, context: "v1 · 已替代")
        let current = KnowledgeReviewSourcePresentation(source: after.current.source, context: "v2 · 当前")
        XCTAssertNotEqual(historical.id, current.id)
        XCTAssertEqual(historical.source, before.source)
        XCTAssertEqual(historical.context, "v1 · 已替代")
        XCTAssertEqual(current.source.excerpt, after.current.correctionBasis)
        XCTAssertEqual(current.context, "v2 · 当前")
    }

    @MainActor
    func testRevisionDraftRemainsBoundToInterfaceWhenDetailFocusChanges() async {
        let store = KnowledgeReviewStore(scenario: .revisions)
        let database = store.item(id: .databaseChoice)
        store.beginRevision()
        store.revisionDraft?.basis = "虚构用户纠正：保留稳定字段。"
        store.selectItem(.databaseChoice)
        XCTAssertTrue(store.applyRevisionDraft())
        XCTAssertEqual(store.item(id: .databaseChoice), database)
        XCTAssertEqual(store.item(id: .interfaceContract)?.current.number, 2)
    }

    @MainActor
    func testRevisionIsBoundedToOneV2AndCancelledDraftCannotReturn() async throws {
        let store = KnowledgeReviewStore(scenario: .revisions)
        store.beginRevision()
        var stale = try XCTUnwrap(store.revisionDraft)
        stale.basis = "虚构迟到纠正依据。"
        store.cancelRevisionDraft()
        store.revisionDraft = stale
        XCTAssertFalse(store.applyRevisionDraft())
        XCTAssertEqual(store.item(id: .interfaceContract)?.current.number, 1)
        store.beginRevision()
        store.revisionDraft?.basis = "虚构用户确认本次 v2。"
        XCTAssertTrue(store.applyRevisionDraft())
        let v2 = store.items
        store.beginRevision()
        XCTAssertNil(store.revisionDraft)
        XCTAssertNotNil(store.revisionBlockedReason)
        store.revisionDraft = stale
        XCTAssertFalse(store.applyRevisionDraft())
        XCTAssertEqual(store.items, v2)
    }

    @MainActor
    func testAgentGuessCannotReplaceUserKnowledgeUnderFullAccessOrAutomaticAdmission() async {
        let store = configuredStore(
            enabled: true, permission: .fullAccess, scenario: .revisions
        )
        let userKnowledge = store.item(id: .interfaceContract)
        XCTAssertTrue(store.processNewCandidates())
        store.setSection(.candidates)
        store.selectItem(.agentGuess)
        store.beginRevision()
        XCTAssertNil(store.revisionDraft)
        store.beginContentEdit()
        store.contentDraft?.content = "虚构 Agent 改写自己的猜测，仍非用户纠正。"
        XCTAssertTrue(store.applyContentDraft())
        store.setCandidateSelected(.agentGuess, selected: true)
        XCTAssertFalse(store.confirmSelectedCandidates())
        XCTAssertEqual(store.item(id: .agentGuess)?.state, .candidate)
        XCTAssertEqual(store.item(id: .agentGuess)?.evidence, .unverifiedAgentReport)
        XCTAssertEqual(store.item(id: .interfaceContract), userKnowledge)
    }

    // MARK: - Explicit retirement is not memory deletion

    @MainActor
    func testRetirementExcludesFutureRecommendationsButPreservesHistoryAndCapturedV1() async throws {
        let store = KnowledgeReviewStore(scenario: .retirement)
        store.setWork(.apiTask)
        let before = try XCTUnwrap(store.item(id: .interfaceContract))
        let captured = store.capturedReferences
        XCTAssertEqual(store.recommendedReferences.map(\.itemID), [.interfaceContract])
        XCTAssertTrue(store.retireSelectedItem())
        let after = try XCTUnwrap(store.item(id: .interfaceContract))
        XCTAssertEqual(after.state, .retired)
        XCTAssertEqual(after.current.state, .retired)
        XCTAssertEqual(after.current.content, before.current.content)
        XCTAssertEqual(after.current.source, before.current.source)
        XCTAssertEqual(after.history, before.history)
        XCTAssertEqual(after.versions.map(\.number), [1, 2])
        XCTAssertTrue(store.recommendedReferences.isEmpty)
        XCTAssertEqual(store.referenceRows.first?.condition, .retired)
        XCTAssertEqual(store.section, .history)
        XCTAssertEqual(store.selectedItemID, .interfaceContract)
        XCTAssertEqual(store.capturedReferences, captured)
        for reference in store.capturedReferences {
            XCTAssertEqual(reference.version, KnowledgeReviewFixtures.interfaceV1)
            XCTAssertTrue(store.capturedReferenceNote(reference).contains("已停用"))
            XCTAssertTrue(store.capturedReferenceNote(reference).contains("旧引用 v1"))
        }
        store.setWork(.clientPackage)
        XCTAssertTrue(store.recommendedReferences.isEmpty)
        store.setWork(.docsPackage)
        XCTAssertEqual(store.recommendedReferences.map(\.itemID), [.docsGuidance])
    }

    @MainActor
    func testCandidateAndAlreadyRetiredItemCannotBeRetiredAgain() async {
        let candidates = KnowledgeReviewStore()
        let before = candidates.items
        XCTAssertFalse(candidates.retireSelectedItem())
        XCTAssertEqual(candidates.items, before)
        let store = KnowledgeReviewStore(scenario: .retirement)
        XCTAssertTrue(store.retireSelectedItem())
        let retired = store.items
        let impacts = store.impactRows
        XCTAssertFalse(store.retireSelectedItem())
        XCTAssertEqual(store.items, retired)
        XCTAssertEqual(store.impactRows, impacts)
    }

    @MainActor
    func testRetirementInvalidatesOutstandingRevisionWithoutErasingItsOldReference() async throws {
        let store = KnowledgeReviewStore(scenario: .revisions)
        store.beginRevision()
        var stale = try XCTUnwrap(store.revisionDraft)
        stale.basis = "虚构迟到修订。"
        let references = store.capturedReferences
        XCTAssertTrue(store.retireSelectedItem())
        XCTAssertNil(store.revisionDraft)
        let retired = store.items
        store.revisionDraft = stale
        XCTAssertFalse(store.applyRevisionDraft())
        XCTAssertEqual(store.items, retired)
        XCTAssertEqual(store.item(id: .interfaceContract)?.current.number, 1)
        XCTAssertEqual(store.capturedReferences, references)
    }

    // MARK: - Affected rows, notification / ACK and one matching preloaded check

    @MainActor
    func testImpactFixtureHasTwoPendingInterfacesAndOneIndependentSafeDocument() async {
        let store = KnowledgeReviewStore(scenario: .impact)
        let affected = store.impactRows.filter(\.isAffected)
        XCTAssertEqual(affected.map(\.id), [.apiTask, .clientPackage])
        for row in affected {
            XCTAssertEqual(row.state, .pending)
            XCTAssertEqual(row.delivery, .notSent)
            XCTAssertNil(row.loadedRecord)
            XCTAssertEqual(row.capturedReference?.version.id, KnowledgeReviewFixtures.interfaceV1.id)
            XCTAssertEqual(row.requiredVersion, KnowledgeReviewFixtures.interfaceV2.id)
            XCTAssertTrue(row.title.contains("虚构"))
        }
        let docs = store.impactRows.first { $0.id == .docsPackage }
        XCTAssertEqual(docs?.state, .unaffected)
        XCTAssertNil(docs?.capturedReference)
        XCTAssertNil(docs?.requiredVersion)
        XCTAssertNil(docs?.loadedRecord)
    }

    @MainActor
    func testNotifyAndACKChangeDeliveryOnlyAndLeaveTheAffectedRowPending() async throws {
        let store = KnowledgeReviewStore(scenario: .impact)
        let original = store.impactRows
        XCTAssertFalse(store.acknowledge(.apiTask))
        XCTAssertEqual(store.impactRows, original)
        XCTAssertTrue(store.notify(.apiTask))
        var row = try XCTUnwrap(store.impactRows.first { $0.id == .apiTask })
        XCTAssertEqual(row.delivery, .notified)
        XCTAssertEqual(row.state, .pending)
        XCTAssertNil(row.loadedRecord)
        XCTAssertFalse(store.notify(.apiTask))
        XCTAssertTrue(store.acknowledge(.apiTask))
        row = try XCTUnwrap(store.impactRows.first { $0.id == .apiTask })
        XCTAssertEqual(row.delivery, .acknowledged)
        XCTAssertEqual(row.state, .pending)
        XCTAssertNil(row.loadedRecord)
        XCTAssertFalse(store.acknowledge(.apiTask))
        XCTAssertEqual(store.impactRows.filter { $0.id != .apiTask }, original.filter { $0.id != .apiTask })
    }

    @MainActor
    func testLoadingPreloadedCheckChangesExactlyOneMatchingAffectedRowNotTheTeam() async throws {
        let store = KnowledgeReviewStore(scenario: .impact)
        XCTAssertTrue(store.notify(.clientPackage))
        XCTAssertTrue(store.acknowledge(.clientPackage))
        let before = store.impactRows
        let items = store.items
        let captured = store.capturedReferences
        XCTAssertNil(store.preloadedCheckBlockedReason(for: .apiTask))
        XCTAssertTrue(store.loadPreloadedCheck(for: .apiTask))
        let changedRows = zip(before, store.impactRows).filter { $0.0 != $0.1 }
        XCTAssertEqual(changedRows.count, 1)
        XCTAssertEqual(changedRows.first?.1.id, .apiTask)
        let api = try XCTUnwrap(store.impactRows.first { $0.id == .apiTask })
        XCTAssertEqual(api.state, .checkedPreloaded)
        XCTAssertEqual(api.loadedRecord, KnowledgeReviewFixtures.preloadedCheck)
        XCTAssertEqual(api.loadedRecord?.applicableVersion, KnowledgeReviewFixtures.interfaceV2.id)
        XCTAssertTrue(api.note.contains("PRELOADED"))
        XCTAssertTrue(api.note.contains("非本次实测"))
        let client = try XCTUnwrap(store.impactRows.first { $0.id == .clientPackage })
        XCTAssertEqual(client.state, .pending)
        XCTAssertEqual(client.delivery, .acknowledged)
        XCTAssertNil(client.loadedRecord)
        XCTAssertEqual(store.impactRows.filter { $0.id != .apiTask }, before.filter { $0.id != .apiTask })
        XCTAssertEqual(store.items, items)
        XCTAssertEqual(store.capturedReferences, captured)
        let once = store.impactRows
        XCTAssertFalse(store.loadPreloadedCheck(for: .apiTask))
        XCTAssertEqual(store.impactRows, once)
    }

    @MainActor
    func testPreloadedCheckRefusesWrongWorkItemVersionOrFabricatedBinding() async {
        let store = KnowledgeReviewStore(scenario: .impact)
        let before = store.impactRows
        let original = KnowledgeReviewFixtures.preloadedCheck
        for id in [KnowledgeReviewWorkID.clientPackage, .docsPackage, .storageTask] {
            XCTAssertNotNil(store.preloadedCheckBlockedReason(for: id))
            XCTAssertFalse(store.loadPreloadedCheck(for: id))
            XCTAssertEqual(store.impactRows, before)
        }
        let alteredRecords: [KnowledgeReviewCheckRecord] = [
            .init(
                id: original.id, workID: .clientPackage, capturedVersion: original.capturedVersion,
                applicableVersion: original.applicableVersion, source: original.source, summary: original.summary
            ),
            .init(
                id: original.id, workID: .apiTask,
                capturedVersion: .init(itemID: .interfaceContract, number: 0),
                applicableVersion: original.applicableVersion, source: original.source, summary: original.summary
            ),
            .init(
                id: original.id, workID: .apiTask, capturedVersion: original.capturedVersion,
                applicableVersion: .init(itemID: .databaseChoice, number: 2),
                source: original.source, summary: original.summary
            ),
            .init(
                id: original.id, workID: .apiTask, capturedVersion: original.capturedVersion,
                applicableVersion: .init(itemID: .interfaceContract, number: 3),
                source: original.source, summary: original.summary
            ),
        ]
        for record in alteredRecords {
            XCTAssertFalse(store.applyPreloadedCheck(record, to: record.workID))
            XCTAssertEqual(store.impactRows, before)
        }
    }

    @MainActor
    func testRecordForV2CannotVerifyV1AndOnlyMatchingUserCorrectionEnablesIt() async {
        let store = KnowledgeReviewStore(scenario: .revisions)
        let before = store.impactRows
        XCTAssertFalse(store.loadPreloadedCheck(for: .apiTask))
        XCTAssertEqual(store.impactRows, before)
        store.beginRevision()
        store.revisionDraft?.basis = KnowledgeReviewFixtures.correctionBasis
        XCTAssertTrue(store.applyRevisionDraft())
        XCTAssertTrue(store.loadPreloadedCheck(for: .apiTask))
        XCTAssertEqual(store.impactRows.first { $0.id == .apiTask }?.state, .checkedPreloaded)
        XCTAssertEqual(store.impactRows.first { $0.id == .clientPackage }?.state, .pending)
    }

    @MainActor
    func testSameVersionNumberWithDifferentContentOrScopeCannotUsePreloadedCheck() async {
        for changeContent in [true, false] {
            let store = KnowledgeReviewStore(scenario: .revisions)
            store.beginRevision()
            store.revisionDraft?.basis = "虚构用户纠正：另一种限定方案。"
            if changeContent {
                store.revisionDraft?.content = "虚构自定义 v2：字段含义不同，不能沿用预置检查。"
            } else {
                store.revisionDraft?.applicability = "虚构自定义 v2：仅用于尚未核验的新分支。"
            }
            XCTAssertTrue(store.applyRevisionDraft())
            XCTAssertEqual(store.item(id: .interfaceContract)?.current.number, 2)
            let before = store.impactRows
            XCTAssertFalse(store.loadPreloadedCheck(for: .apiTask))
            XCTAssertEqual(store.impactRows, before)
            XCTAssertTrue(store.notice.contains("内容或适用条件不同"))
        }
    }

    @MainActor
    func testRetirementInvalidatesPreloadedCheckButLeavesIndependentWorkContinuing() async {
        let store = KnowledgeReviewStore(scenario: .impact)
        XCTAssertTrue(store.loadPreloadedCheck(for: .apiTask))
        XCTAssertTrue(store.continueIndependentWork(.docsPackage))
        let docs = store.impactRows.first { $0.id == .docsPackage }
        let references = store.capturedReferences
        XCTAssertTrue(store.retireSelectedItem())
        XCTAssertTrue(store.impactRows.filter(\.isAffected).allSatisfy {
            $0.state == .pending && $0.loadedRecord == nil
        })
        XCTAssertEqual(store.impactRows.first { $0.id == .docsPackage }, docs)
        XCTAssertEqual(store.capturedReferences, references)
        let before = store.impactRows
        XCTAssertFalse(store.loadPreloadedCheck(for: .apiTask))
        XCTAssertEqual(store.impactRows, before)
    }

    @MainActor
    func testIndependentDocsContinueVisiblyWithoutReleasingAffectedInterfaces() async {
        let store = KnowledgeReviewStore(scenario: .impact)
        let affected = store.impactRows.filter(\.isAffected)
        let knowledge = store.items
        let references = store.capturedReferences
        XCTAssertFalse(store.continueIndependentWork(.apiTask))
        XCTAssertFalse(store.continueIndependentWork(.clientPackage))
        XCTAssertTrue(store.continueIndependentWork(.docsPackage))
        XCTAssertEqual(store.impactRows.first { $0.id == .docsPackage }?.state, .continuingDemo)
        XCTAssertTrue(store.impactRows.first { $0.id == .docsPackage }?.note.contains("仅改变内存") == true)
        XCTAssertEqual(store.impactRows.filter(\.isAffected), affected)
        XCTAssertEqual(store.items, knowledge)
        XCTAssertEqual(store.capturedReferences, references)
        let once = store.impactRows
        XCTAssertFalse(store.continueIndependentWork(.docsPackage))
        XCTAssertFalse(store.notify(.docsPackage))
        XCTAssertFalse(store.acknowledge(.docsPackage))
        XCTAssertEqual(store.impactRows, once)
    }

    // MARK: - Reset, scenario boundaries, window identity and stale callbacks

    @MainActor
    func testResetRestoresEveryScenarioAfterRealMemoryMutations() async {
        for scenario in KnowledgeReviewScenario.allCases {
            let store = KnowledgeReviewStore(scenario: scenario)
            let instanceID = store.instanceID
            let initialWorkID = store.selectedWorkID
            store.setPermission(.fullAccess)
            store.beginConfiguration()
            store.configurationDraft?.configuration = .init(enabled: true, preset: .verifiedSolutions)
            XCTAssertTrue(store.applyConfigurationDraft())
            XCTAssertTrue(store.processNewCandidates())
            store.setWork(.docsPackage)
            store.setReferenceScenario(.missingSource)
            XCTAssertTrue(store.continueIndependentWork(.docsPackage))
            XCTAssertTrue(store.notify(.apiTask))
            XCTAssertTrue(store.acknowledge(.apiTask))
            store.setSection(.reusable)
            store.selectItem(.interfaceContract)
            if store.item(id: .interfaceContract)?.current.number == 1 {
                store.beginRevision()
                store.revisionDraft?.basis = "虚构重置测试：用户纠正。"
                XCTAssertTrue(store.applyRevisionDraft())
            }
            XCTAssertTrue(store.retireSelectedItem())
            store.setSection(.candidates)
            if let candidate = store.candidateItems.first {
                store.selectItem(candidate.id)
                store.setCandidateSelected(candidate.id, selected: true)
                store.beginContentEdit()
                store.contentDraft?.content = "虚构尚未应用的草稿。"
            }
            store.reset()
            XCTAssertEqual(store.instanceID, instanceID, "Reset is not a different window instance.")
            XCTAssertEqual(store.scenario, scenario)
            XCTAssertEqual(store.items, KnowledgeReviewFixtures.items(for: scenario))
            XCTAssertEqual(store.impactRows, KnowledgeReviewFixtures.impacts(for: scenario))
            XCTAssertEqual(store.section, KnowledgeReviewFixtures.defaultSection(for: scenario))
            XCTAssertEqual(store.selectedItemID, KnowledgeReviewFixtures.defaultSelection(for: scenario))
            XCTAssertEqual(store.selectedWorkID, initialWorkID)
            XCTAssertEqual(store.referenceScenario, .sourced)
            XCTAssertEqual(store.admissionConfiguration, .init())
            XCTAssertEqual(store.permission, .standard)
            XCTAssertFalse(store.hasProcessedNewCandidates)
            XCTAssertTrue(store.selectedCandidateIDs.isEmpty)
            XCTAssertNil(store.contentDraft)
            XCTAssertNil(store.configurationDraft)
            XCTAssertNil(store.revisionDraft)
        }
    }

    @MainActor
    func testScenarioSwitchClearsAppliedStateAndAnUnappliedConfigurationDraft() async {
        let store = configuredStore(enabled: true, permission: .fullAccess)
        XCTAssertTrue(store.processNewCandidates())
        store.beginConfiguration()
        store.configurationDraft?.configuration.preset = .verifiedSolutions
        let identity = store.instanceID
        store.load(.revisions)
        XCTAssertEqual(store.instanceID, identity)
        XCTAssertEqual(store.scenario, .revisions)
        XCTAssertEqual(store.items, KnowledgeReviewFixtures.items(for: .revisions))
        XCTAssertEqual(store.selectedItemID, .interfaceContract)
        XCTAssertEqual(store.admissionConfiguration, .init())
        XCTAssertEqual(store.permission, .standard)
        XCTAssertNil(store.configurationDraft)
        XCTAssertFalse(store.hasProcessedNewCandidates)
    }

    @MainActor
    func testResetOrSameScenarioReloadRejectsAllThreeKindsOfStaleDraft() async throws {
        for useReset in [true, false] {
            let content = KnowledgeReviewStore()
            content.beginContentEdit()
            var contentDraft = try XCTUnwrap(content.contentDraft)
            contentDraft.content = "虚构迟到内容。"
            if useReset { content.reset() } else { content.load(.candidates) }
            let initialItems = content.items
            content.contentDraft = contentDraft
            XCTAssertFalse(content.applyContentDraft())
            XCTAssertEqual(content.items, initialItems)

            let configuration = KnowledgeReviewStore(scenario: .admission)
            configuration.beginConfiguration()
            var configurationDraft = try XCTUnwrap(configuration.configurationDraft)
            configurationDraft.configuration.enabled = true
            if useReset { configuration.reset() } else { configuration.load(.admission) }
            configuration.configurationDraft = configurationDraft
            XCTAssertFalse(configuration.applyConfigurationDraft())
            XCTAssertEqual(configuration.admissionConfiguration, .init())

            let revision = KnowledgeReviewStore(scenario: .revisions)
            revision.beginRevision()
            var revisionDraft = try XCTUnwrap(revision.revisionDraft)
            revisionDraft.basis = "虚构迟到纠正依据。"
            if useReset { revision.reset() } else { revision.load(.revisions) }
            let before = revision.items
            let impacts = revision.impactRows
            revision.revisionDraft = revisionDraft
            XCTAssertFalse(revision.applyRevisionDraft())
            XCTAssertEqual(revision.items, before)
            XCTAssertEqual(revision.impactRows, impacts)
        }
    }

    @MainActor
    func testOneWindowCannotApplyAnotherWindowsDraftAndNewInstancesKeepNoState() async throws {
        let first = configuredStore(
            enabled: true, preset: .verifiedSolutions, permission: .fullAccess, scenario: .revisions
        )
        first.beginRevision()
        first.revisionDraft?.basis = "虚构第一窗口纠正。"
        let firstDraft = try XCTUnwrap(first.revisionDraft)
        XCTAssertTrue(first.applyRevisionDraft())
        XCTAssertTrue(first.processNewCandidates())
        let second = KnowledgeReviewStore(scenario: .revisions)
        XCTAssertNotEqual(first.instanceID, second.instanceID)
        XCTAssertEqual(second.items, KnowledgeReviewFixtures.items(for: .revisions))
        XCTAssertEqual(second.admissionConfiguration, .init())
        XCTAssertEqual(second.permission, .standard)
        XCTAssertNil(second.contentDraft)
        XCTAssertNil(second.configurationDraft)
        XCTAssertNil(second.revisionDraft)
        XCTAssertFalse(second.hasProcessedNewCandidates)
        second.beginRevision()
        let secondDraft = try XCTUnwrap(second.revisionDraft)
        XCTAssertNotEqual(firstDraft.generation, secondDraft.generation)
        second.revisionDraft = firstDraft
        XCTAssertFalse(second.applyRevisionDraft())
        XCTAssertEqual(second.items, KnowledgeReviewFixtures.items(for: .revisions))
        XCTAssertEqual(first.item(id: .interfaceContract)?.current.number, 2)
    }

    @MainActor
    func testResetAllowsOneFreshBatchButNeverCarriesAppliedConfigurationAcrossResets() async {
        let store = configuredStore(enabled: true, permission: .fullAccess)
        XCTAssertTrue(store.processNewCandidates())
        XCTAssertEqual(store.item(id: .newPreference)?.state, .reusable)
        store.reset()
        XCTAssertNil(store.item(id: .newPreference))
        XCTAssertFalse(store.admissionConfiguration.enabled)
        XCTAssertEqual(store.permission, .standard)
        XCTAssertTrue(store.processNewCandidates())
        XCTAssertEqual(store.item(id: .newPreference)?.state, .candidate)
        XCTAssertFalse(store.processNewCandidates())
        XCTAssertEqual(store.items.filter { $0.id == .newPreference }.count, 1)
    }

    // MARK: - Actual root/refusal assembly and the motion policy used by the View

    @MainActor
    func testActualKnowledgeRootAssemblesWithoutAnApplicationWindowOrClient() async {
        let evidence = KnowledgeReviewNativeInputEvidence()
        XCTAssertEqual(evidence.value, "[]")
        for index in 0..<25 { evidence.append(["sequence": index]) }
        XCTAssertEqual(evidence.snapshots.count, 18)
        XCTAssertEqual(evidence.snapshots.first, "{\"sequence\":7,\"snapshotSequence\":8}")
        XCTAssertEqual(evidence.snapshots.last, "{\"sequence\":24,\"snapshotSequence\":25}")
        evidence.append(["notJSON": Date()])
        XCTAssertEqual(evidence.snapshots.count, 18, "Invalid diagnostics must not affect input or business state.")
        XCTAssertNoThrow(try JSONSerialization.jsonObject(with: Data(evidence.value.utf8)))
        let host = NSHostingView(rootView: NativeKnowledgeReviewView())
        host.frame = NSRect(x: 0, y: 0, width: 1000, height: 640)
        host.layoutSubtreeIfNeeded()
        XCTAssertNil(host.window)
        XCTAssertGreaterThan(host.fittingSize.width, 0)
        XCTAssertGreaterThan(host.fittingSize.height, 0)
    }

    @MainActor
    func testActualInjectedRootAssemblesAtAllSampleSizesWithLiveStoreChanges() async {
        var measurement = KnowledgeReviewWindowMeasurement()
        XCTAssertEqual(measurement.sheetSize, CGSize(width: 320, height: 320))
        for (content, layout, expected) in [
            (CGSize(width: 1260, height: 860), CGSize(width: 1260, height: 832), CGSize(width: 620, height: 680)),
            (CGSize(width: 1000, height: 640), CGSize(width: 1000, height: 612), CGSize(width: 620, height: 580)),
            (CGSize(width: 400, height: 600), CGSize(width: 400, height: 572), CGSize(width: 368, height: 540)),
        ] {
            measurement.content = content
            measurement.layout = layout
            XCTAssertEqual(measurement.sheetSize, expected, "Sheet sizing must use the current measured layout.")
        }
        let store = KnowledgeReviewStore()
        let host = NSHostingView(rootView: NativeKnowledgeReviewView(store: store))
        for size in [
            NSSize(width: 1260, height: 860),
            NSSize(width: 1000, height: 640),
            NSSize(width: 400, height: 600),
        ] {
            host.setFrameSize(size)
            host.layoutSubtreeIfNeeded()
            XCTAssertNil(host.window)
            XCTAssertGreaterThan(host.fittingSize.width, 0)
            XCTAssertGreaterThan(host.fittingSize.height, 0)
        }
        store.setCandidateSelected(.conciseReplies, selected: true)
        store.setCandidateSelected(.verifiedCheck, selected: true)
        XCTAssertTrue(store.confirmSelectedCandidates())
        let instance = store.instanceID
        host.setFrameSize(NSSize(width: 1000, height: 640))
        for section in KnowledgeReviewSection.allCases {
            store.setSection(section)
            host.layoutSubtreeIfNeeded()
            XCTAssertEqual(store.instanceID, instance, "A native collection identity must not recreate Store memory.")
            XCTAssertEqual(store.selectedItemID, store.visibleItems.first?.id)
            XCTAssertTrue(store.selectedCandidateIDs.isEmpty)
        }
        store.load(.impact)
        XCTAssertTrue(store.notify(.apiTask))
        XCTAssertTrue(store.acknowledge(.apiTask))
        XCTAssertTrue(store.loadPreloadedCheck(for: .apiTask))
        host.layoutSubtreeIfNeeded()
        XCTAssertEqual(store.impactRows.first { $0.id == .clientPackage }?.state, .pending)
        XCTAssertNil(host.window)
        // These are fitting/layout probes, not measurements of real windows
        // and not proof that compact controls are keyboard/AX reachable.
    }

    @MainActor
    func testActualRootAssemblesWithStrengtheningAccessibilityPreviewsInBothColorSchemes() async {
        for reduced in [false, true] {
            let host = NSHostingView(
                rootView: NativeKnowledgeReviewView(
                    store: KnowledgeReviewStore(),
                    reduceTransparencyPreview: reduced,
                    increaseContrastPreview: reduced,
                    reduceMotionPreview: reduced
                )
                    .environment(\.colorScheme, reduced ? .dark : .light)
            )
            host.frame = NSRect(x: 0, y: 0, width: 400, height: 600)
            host.layoutSubtreeIfNeeded()
            XCTAssertNil(host.window)
            XCTAssertGreaterThan(host.fittingSize.width, 0)
            XCTAssertGreaterThan(host.fittingSize.height, 0)
        }
        // These system preferences are read-only EnvironmentValues. Exercise
        // the real View's strengthening previews, never private environment
        // setters or changes to the user's global accessibility settings.
    }

    @MainActor
    func testActualRefusalRootAssemblesForEachReasonAndCombinedReasons() async {
        let combinations = KnowledgeReviewRefusalReason.allCases.map { [$0] }
            + [KnowledgeReviewRefusalReason.allCases]
        for reasons in combinations {
            let host = NSHostingView(rootView: NativeKnowledgeReviewRefusalView(reasons: reasons))
            host.frame = NSRect(x: 0, y: 0, width: 400, height: 600)
            host.layoutSubtreeIfNeeded()
            XCTAssertNil(host.window)
            XCTAssertGreaterThanOrEqual(host.fittingSize.width, 320)
            XCTAssertGreaterThanOrEqual(host.fittingSize.height, 240)
        }
    }

    @MainActor
    func testLiveMotionPolicyDistinguishesReducedAndNormalTransactionsWithoutChangingPermission() async {
        let reduced = KnowledgeReviewMotionPolicy.transaction(reduceMotion: true)
        XCTAssertTrue(reduced.disablesAnimations)
        XCTAssertNil(reduced.animation)
        let normal = KnowledgeReviewMotionPolicy.transaction(reduceMotion: false)
        XCTAssertFalse(normal.disablesAnimations)
        XCTAssertNotNil(normal.animation)
        let store = KnowledgeReviewStore()
        let originalConfiguration = store.admissionConfiguration
        withTransaction(reduced) {
            store.setCandidateSelected(.conciseReplies, selected: true)
            store.selectItem(.verifiedCheck)
        }
        withTransaction(normal) {
            store.selectItem(.discardable)
        }
        XCTAssertEqual(store.selectedCandidateIDs, [.conciseReplies])
        XCTAssertEqual(store.selectedItemID, .discardable)
        XCTAssertEqual(store.permission, .standard)
        XCTAssertEqual(store.admissionConfiguration, originalConfiguration)
        XCTAssertEqual(store.items, KnowledgeReviewFixtures.items(for: .candidates))
        // Policy/assembly coverage does not establish real-window animation,
        // compositing, reduced-transparency appearance or keyboard behavior.
    }

    @MainActor
    func testNativeInputEvidenceKeepsBoundedMonotonicDiagnosticSequence() throws {
        let evidence = KnowledgeReviewNativeInputEvidence()
        for index in 1...21 {
            evidence.append(["stage": "before", "eventTimestamp": Double(index),
                             "store": ["nativeListHasFocus": false, "selectedID": "conciseReplies"]])
        }
        let snapshots = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(evidence.value.utf8)) as? [[String: Any]]
        )
        XCTAssertEqual(snapshots.count, 18)
        XCTAssertEqual(snapshots.compactMap { $0["snapshotSequence"] as? Int }, Array(4...21))
        XCTAssertEqual(snapshots.compactMap { $0["eventTimestamp"] as? Double }, (4...21).map(Double.init))
        XCTAssertEqual((snapshots.last?["store"] as? [String: Any])?["nativeListHasFocus"] as? Bool, false)
        XCTAssertEqual((snapshots.last?["store"] as? [String: Any])?["selectedID"] as? String, "conciseReplies")
        // These scalars identify freshness only, not real keyboard acceptance.
    }

    func testNativeLeadingGutterFocusGuardExcludesControlsAndUnknownGeometry() {
        // The actual run09 AX geometry translated to outline coordinates is
        // only a representative fixture, never evidence of a runtime focus fix.
        let row = NSRect(x: 1, y: 11, width: 284, height: 55.5)
        let visible = NSRect(x: 0, y: 0, width: 286, height: 332)
        let cell = NSRect(x: 11, y: 11, width: 264, height: 55.5)
        func eligible(_ point: NSPoint, _ cells: [NSRect] = [NSRect(x: 11, y: 11, width: 264, height: 55.5)],
                      _ viewport: NSRect = NSRect(x: 0, y: 0, width: 286, height: 332)) -> Bool {
            KnowledgeReviewLeadingGutter.contains(point, row: row, visible: viewport, cells: cells)
        }
        XCTAssertTrue(eligible(NSPoint(x: 6, y: 27.5)))
        XCTAssertTrue(eligible(NSPoint(x: 1, y: 27.5)))
        for point in [NSPoint(x: 11, y: 27.5), NSPoint(x: 30, y: 27.5),
                      NSPoint(x: 280, y: 27.5), NSPoint(x: 0, y: 27.5),
                      NSPoint(x: 6, y: 10), NSPoint(x: 6, y: 67),
                      NSPoint(x: CGFloat.nan, y: 27.5), NSPoint(x: 6, y: CGFloat.infinity)] {
            XCTAssertFalse(eligible(point), "Not a visible leading gutter: \(point)")
        }
        XCTAssertFalse(eligible(NSPoint(x: 6, y: 27.5), []))
        for invalid in [NSRect.zero, NSRect(x: 11, y: 11, width: -1, height: 55.5),
                        NSRect(x: 11, y: 11, width: CGFloat.infinity, height: 55.5),
                        NSRect(x: 11, y: 100, width: 264, height: 55.5)] {
            XCTAssertFalse(eligible(NSPoint(x: 6, y: 27.5), [cell, invalid]))
        }
        XCTAssertFalse(eligible(NSPoint(x: 6, y: 27.5), [row])) // Full-row hosted control.
        XCTAssertFalse(eligible(NSPoint(x: 6, y: 27.5), [cell, NSRect(x: 3, y: 11, width: 4, height: 55.5)]))
        XCTAssertFalse(eligible(NSPoint(x: 6, y: 27.5), [cell], NSRect(x: 7, y: 0, width: 279, height: 332)))
        XCTAssertFalse(KnowledgeReviewLeadingGutter.contains(NSPoint(x: 6, y: 27.5), row: .zero,
                                                            visible: visible, cells: [cell]))
        XCTAssertFalse(KnowledgeReviewLeadingGutter.contains(NSPoint(x: 6, y: 27.5), row: row,
                                                            visible: .zero, cells: [cell]))
    }

    // MARK: - Helpers: only entry tests perform temporary fixture I/O

    private struct EntryFixture {
        let root: URL
        let home: URL
        let caches: URL
        var safeChild: URL { caches.appendingPathComponent("missing-knowledge-child") }
        var environment: [String: String] {
            [
                NativeKnowledgeReviewEntry.flagKey: "1",
                "AERORIC_ENV": "ui-test",
                "AERORIC_DATA_DIR": safeChild.path,
                "AERORIC_NATIVE_EVOLUTION_REVIEW": "0",
                "AERORIC_NATIVE_VISUAL_REVIEW": "0",
                "AERORIC_NATIVE_COLLABORATION_REVIEW": "0",
            ]
        }
    }

    private func withEntryFixture(_ body: (EntryFixture) throws -> Void) throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("aeroric-knowledge-entry-\(UUID().uuidString)", isDirectory: true)
        let home = root.appendingPathComponent("fake-home", isDirectory: true)
        let caches = home.appendingPathComponent("Library/Caches", isDirectory: true)
        try FileManager.default.createDirectory(at: caches, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        try body(.init(root: root, home: home, caches: caches))
    }

    private func decision(
        _ environment: [String: String], _ fixture: EntryFixture
    ) -> KnowledgeReviewEntryDecision {
        NativeKnowledgeReviewEntry.decide(environment: environment, home: fixture.home.path)
    }

    @MainActor
    private func configuredStore(
        enabled: Bool, preset: KnowledgeReviewAdmissionPreset = .userPreferences,
        permission: KnowledgeReviewPermission,
        scenario: KnowledgeReviewScenario = .admission
    ) -> KnowledgeReviewStore {
        let store = KnowledgeReviewStore(scenario: scenario)
        store.setPermission(permission)
        store.beginConfiguration()
        store.configurationDraft?.configuration = .init(enabled: enabled, preset: preset)
        XCTAssertTrue(store.applyConfigurationDraft())
        return store
    }
}
#endif
