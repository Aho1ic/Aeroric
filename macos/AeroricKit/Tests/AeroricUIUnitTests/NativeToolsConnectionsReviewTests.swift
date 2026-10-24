#if DEBUG
import Foundation
import XCTest
@testable import AeroricUI

/// Reviewed XCTest-only suite: no View, hosting, window, App, preferences,
/// service, process or agent construction. Files belong only to EntryFixture.
final class NativeToolsConnectionsReviewTests: XCTestCase {
    private let oldKeys = [
        "AERORIC_NATIVE_TASK_DELIVERABLES_REVIEW",
        "AERORIC_NATIVE_TASK_COMMUNICATION_REVIEW",
        "AERORIC_NATIVE_WORK_ARRANGEMENTS_REVIEW",
        "AERORIC_NATIVE_SKILL_REVIEW",
        "AERORIC_NATIVE_KNOWLEDGE_REVIEW",
        "AERORIC_NATIVE_EVOLUTION_REVIEW",
        "AERORIC_NATIVE_VISUAL_REVIEW",
        "AERORIC_NATIVE_COLLABORATION_REVIEW",
    ]
    private let query = "demo-query"
    private let connection = "demo-connection"
    private let external = "demo-external-tool"

    func testOldKeyContractAndAbsentNewFlagPreserveAll256Combinations() throws {
        XCTAssertEqual(NativeToolsConnectionsReviewEntry.flagKey, "AERORIC_NATIVE_TOOLS_CONNECTIONS_REVIEW")
        XCTAssertEqual(Set(NativeToolsConnectionsReviewEntry.otherReviewKeys), Set(oldKeys))
        XCTAssertEqual(NativeToolsConnectionsReviewEntry.otherReviewKeys.count, oldKeys.count)
        XCTAssertEqual(oldKeys.count, 8, "A changed entry chain needs a fresh scope review")
        try withEntryFixture { fixture in
            for mask in 0..<(1 << oldKeys.count) {
                var environment: [String: String] = [:]
                for (index, key) in oldKeys.enumerated() {
                    environment[key] = mask & (1 << index) == 0 ? "0" : "1"
                }
                XCTAssertEqual(decide(environment, fixture), .inactive, "mask \(mask)")
            }
        }
    }

    func testValidNewRequestAndAll255NonemptyOldConflicts() throws {
        try withEntryFixture { fixture in
            XCTAssertEqual(decide(fixture.environment, fixture), .review)
            for mask in 1..<(1 << oldKeys.count) {
                var environment = fixture.environment
                for (index, key) in oldKeys.enumerated() {
                    environment[key] = mask & (1 << index) == 0 ? "0" : "1"
                }
                XCTAssertEqual(decide(environment, fixture), .refused([.conflictingReview]), "mask \(mask)")
            }
            var zeroes = fixture.environment
            for key in oldKeys { zeroes[key] = "0" }
            XCTAssertEqual(decide(zeroes, fixture), .review)
            for key in oldKeys { zeroes[key] = "true" }
            XCTAssertEqual(decide(zeroes, fixture), .review, "Only exact 1 conflicts")
        }
    }

    func testPresentInvalidFlagEnvironmentAndDirectoryRefuseWithoutFallback() throws {
        try withEntryFixture { fixture in
            for value in ["0", "", "true", "2", "01", " 1", "1\n", "demo-private-value"] {
                var environment = fixture.environment
                environment[NativeToolsConnectionsReviewEntry.flagKey] = value
                XCTAssertEqual(decide(environment, fixture), .refused([.invalidFlag]))
            }
            for value in [nil, "", "default", "UI-TEST", "ui-test "] as [String?] {
                var environment = fixture.environment
                environment["AERORIC_ENV"] = value
                XCTAssertEqual(decide(environment, fixture), .refused([.invalidEnvironment]))
            }
            for value in [nil, ""] as [String?] {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = value
                XCTAssertEqual(decide(environment, fixture), .refused([.invalidDirectory]))
            }
            let invalid = [
                NativeToolsConnectionsReviewEntry.flagKey: "demo-private-value",
                "AERORIC_ENV": "demo-private-value",
                "AERORIC_DATA_DIR": fixture.home.path + "/private-demo-value",
                oldKeys[0]: "1",
            ]
            XCTAssertEqual(decide(invalid, fixture),
                           .refused([.invalidFlag, .invalidEnvironment, .conflictingReview, .invalidDirectory]))
            let descriptions = ToolsConnectionsReviewRefusalReason.allCases
                .map { $0.title + $0.detail }.joined()
            XCTAssertFalse(descriptions.contains(fixture.home.path))
            XCTAssertFalse(descriptions.contains("demo-private-value"))
        }
    }

    func testDirectoryValidationRejectsBoundaryDotSymlinkAndFileWithoutMutation() throws {
        try withEntryFixture { fixture in
            let fm = FileManager.default
            let directory = fixture.caches.appendingPathComponent("existing", isDirectory: true)
            try fm.createDirectory(at: directory, withIntermediateDirectories: false)
            let file = fixture.caches.appendingPathComponent("file")
            let sentinel = Data("only this suite owns this sentinel".utf8)
            try sentinel.write(to: file)
            let link = fixture.caches.appendingPathComponent("link")
            try fm.createSymbolicLink(at: link, withDestinationURL: directory)
            let dangling = fixture.caches.appendingPathComponent("dangling")
            try fm.createSymbolicLink(at: dangling, withDestinationURL: fixture.home.appendingPathComponent("absent"))
            let before = try tree(fixture.home)
            let invalidPaths = [
                fixture.caches.path, "/", "relative/child", " " + fixture.candidate.path,
                fixture.candidate.path + "\n", fixture.caches.path + "/./child",
                fixture.caches.path + "/../outside", fixture.caches.path + "-collision/child",
                file.path, file.path + "/child", link.path, link.path + "/missing/child",
                dangling.path, dangling.path + "/child",
            ]
            for path in invalidPaths {
                var environment = fixture.environment
                environment["AERORIC_DATA_DIR"] = path
                XCTAssertEqual(decide(environment, fixture), .refused([.invalidDirectory]), path)
            }
            var existing = fixture.environment
            existing["AERORIC_DATA_DIR"] = directory.path
            XCTAssertEqual(decide(existing, fixture), .review)
            XCTAssertEqual(decide(fixture.environment, fixture), .review)
            XCTAssertFalse(fm.fileExists(atPath: fixture.candidate.path), "A valid absent child is not created")
            XCTAssertEqual(try tree(fixture.home), before)
            XCTAssertEqual(try Data(contentsOf: file), sentinel)
        }
    }

    @MainActor
    func testInjectedLifecycleMetadataDoesNotConstructWindowOrRoot() async {
        let review = NativeToolsConnectionsReviewWindowLifecycle(decision: .review)
        let refusal = NativeToolsConnectionsReviewWindowLifecycle(decision: .refused([.invalidFlag]))
        let inactive = NativeToolsConnectionsReviewWindowLifecycle(decision: .inactive)
        XCTAssertEqual(review.windowTitle, "工具库与连接审阅样例")
        XCTAssertEqual(refusal.windowTitle, "工具库与连接审阅未启动")
        XCTAssertEqual(inactive.windowTitle, "")
        XCTAssertEqual(review.windowIdentifier?.rawValue, "native-tools-connections-review")
        XCTAssertEqual(refusal.windowIdentifier?.rawValue, "native-tools-connections-review-refusal")
        XCTAssertNil(inactive.windowIdentifier)
        XCTAssertFalse(review.isWindowCreated)
        XCTAssertFalse(refusal.isWindowCreated)
        XCTAssertFalse(inactive.isWindowCreated)
    }

    @MainActor
    func testTC1ConfigurationEnableReferenceAuthorizationAreIndependent() async {
        let store = ToolsConnectionsReviewStore()
        XCTAssertFalse(store.eligibility(for: query).eligible)
        store.saveConfiguration()
        XCTAssertEqual(store.configuredIDs, [query])
        XCTAssertFalse(store.isEnabled(query, in: .alphaOne))
        XCTAssertTrue(store.authorizedTasks.isEmpty)
        store.referenceForTask()
        XCTAssertTrue(store.taskReferences.isEmpty, "Project must enable, or user must choose temporary")
        store.setProjectEnabled(true)
        store.referenceForTask()
        XCTAssertTrue(store.taskReferences[.alphaOne, default: []].contains(query))
        XCTAssertFalse(store.eligibility(for: query).eligible)
        store.selectItem(connection)
        store.saveConfiguration()
        store.setProjectEnabled(true)
        store.selectItem(query)
        store.loadExampleEvent(.taskAuthorized)
        XCTAssertTrue(store.eligibility(for: query).eligible)
        XCTAssertEqual(store.detailLayers.count, 6)
        XCTAssertEqual(store.loadedEvents.count, 1)
        store.selectItem(connection)
        store.loadExampleEvent(.taskAuthorized)
        XCTAssertEqual(store.loadedEvents.count, 1, "The same task grant is not duplicated by browsing")
        XCTAssertEqual(store.externalExecutionCount, 0)
    }

    @MainActor
    func testTC1ProjectTaskAndStandaloneScopesDoNotLeak() async {
        let store = ToolsConnectionsReviewStore()
        store.saveConfiguration()
        store.referenceForTask(temporary: true)
        store.loadExampleEvent(.taskAuthorized)
        XCTAssertTrue(store.temporaryReferences[.alphaOne, default: []].contains(query))
        XCTAssertTrue(store.projectEnabled.isEmpty)
        store.selectContext(.alphaTwo)
        XCTAssertFalse(store.isEnabled(query, in: .alphaTwo))
        XCTAssertFalse(store.authorizedTasks.contains(.alphaTwo))
        store.setProjectEnabled(true)
        XCTAssertTrue(store.isProjectEnabled(query, in: .alphaOne))
        store.selectContext(.betaOne)
        XCTAssertFalse(store.isEnabled(query, in: .betaOne))
        XCTAssertFalse(store.authorizedTasks.contains(.betaOne))
        store.selectContext(.standalone)
        store.setProjectEnabled(true)
        XCTAssertFalse(store.isProjectEnabled(query, in: .standalone))
        store.referenceForTask()
        XCTAssertTrue(store.taskEnabled[.standalone, default: []].contains(query))
        XCTAssertTrue(store.temporaryReferences[.standalone, default: []].contains(query))
        XCTAssertFalse(store.authorizedTasks.contains(.standalone))
        XCTAssertEqual(store.projectEnabled.keys.sorted(), ["demo-project-alpha"])
    }

    @MainActor
    func testTC1DraftCancelContextAnchorAndBlockedConnection() async {
        let store = ToolsConnectionsReviewStore(variant: .blocked)
        store.beginScopeDraft()
        store.scopeDraft?.scope = .project
        store.cancelScopeDraft()
        XCTAssertTrue(store.taskEnabled.isEmpty)
        XCTAssertTrue(store.projectEnabled.isEmpty)
        store.saveConfiguration()
        store.beginScopeDraft()
        store.scopeDraft?.scope = .project
        store.commitScopeDraft()
        store.loadExampleEvent(.taskAuthorized)
        XCTAssertTrue(store.isEnabled(query, in: .alphaOne))
        XCTAssertFalse(store.eligibility(for: query).eligible)
        XCTAssertTrue(store.eligibility(for: query).reasons.contains { $0.contains("连接条件仍缺") })
        store.beginScopeDraft()
        store.selectContext(.betaOne)
        XCTAssertNil(store.scopeDraft)
        store.commitScopeDraft()
        XCTAssertFalse(store.isEnabled(query, in: .betaOne))
    }

    @MainActor
    func testTC2EligibilityAndProposalDoNotExecuteOrConsume() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC2)
        let before = store.results
        let identity = store.identity
        XCTAssertTrue(store.eligibility(for: query).eligible)
        store.proposeSelection()
        XCTAssertEqual(store.proposedID, query)
        XCTAssertEqual(store.results, before)
        XCTAssertEqual(store.identity, identity)
        XCTAssertTrue(store.loadedEvents.isEmpty)
        XCTAssertEqual(store.externalExecutionCount, 0)
        XCTAssertEqual(store.replayCount, 0)
    }

    @MainActor
    func testTC2UnsupportedDisconnectedUncheckedStaleUnknownStayBlocked() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC2)
        for id in ["demo-unsupported", "demo-disconnected", "demo-unchecked", "demo-stale", "demo-unknown", external] {
            store.selectItem(id)
            store.setUserConstraint(.specified)
            store.proposeSelection()
            XCTAssertFalse(store.eligibility(for: id).eligible, id)
            XCTAssertFalse(store.eligibility(for: id).reasons.isEmpty)
            XCTAssertNil(store.proposedID)
            XCTAssertTrue(store.loadedEvents.isEmpty)
        }
        XCTAssertTrue(store.independentWork.contains("独立文档"))
    }

    @MainActor
    func testTC2UserSpecifiedDisabledAndAmbiguousConstraintsAreAnchored() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC2)
        store.setUserConstraint(.specified)
        store.selectItem("demo-unsupported")
        store.proposeSelection()
        XCTAssertEqual(store.constrainedID, query)
        XCTAssertEqual(store.proposedID, query, "Browsing must not replace the explicitly specified item")
        store.selectItem(query)
        store.setUserConstraint(.disabled)
        store.selectItem(external)
        XCTAssertFalse(store.eligibility(for: query).eligible, "Disabled item stays disabled while browsing")
        XCTAssertTrue(store.eligibility(for: query).reasons.contains("用户禁用优先"))
        store.selectContext(.alphaTwo)
        XCTAssertEqual(store.userConstraint, .automatic)
        store.selectContext(.alphaOne)
        XCTAssertEqual(store.userConstraint, .disabled)
        store.selectItem(connection)
        store.setUserConstraint(.disabled)
        XCTAssertFalse(store.eligibility(for: query).eligible, "A disabled connection cannot be bypassed by its tool")
        XCTAssertTrue(store.eligibility(for: query).reasons.contains { $0.contains("依赖") && $0.contains("用户禁用") })
        store.setUserConstraint(.ambiguousTarget)
        store.proposeSelection()
        XCTAssertNil(store.proposedID)
        XCTAssertTrue(store.eligibility(for: query).reasons.contains { $0.contains("歧义") })
    }

    @MainActor
    func testTC2AuthorizationAndSavedNotEnabledBranches() async {
        let blocked = ToolsConnectionsReviewStore(scenario: .TC2, variant: .blocked)
        XCTAssertFalse(blocked.eligibility(for: query).eligible)
        XCTAssertTrue(blocked.selectedItem.service.state == .available)
        blocked.loadExampleEvent(.taskAuthorized)
        XCTAssertTrue(blocked.eligibility(for: query).eligible)
        let inactive = ToolsConnectionsReviewStore(scenario: .TC2, variant: .notEnabled)
        XCTAssertTrue(inactive.configuredIDs.contains(query))
        XCTAssertFalse(inactive.eligibility(for: query).eligible)
        inactive.setProjectEnabled(true)
        XCTAssertFalse(inactive.eligibility(for: query).eligible, "Referenced connection is still not enabled")
        inactive.selectItem(connection)
        inactive.setProjectEnabled(true)
        XCTAssertTrue(inactive.eligibility(for: query).eligible)
    }

    @MainActor
    func testTC3ReviewCancelConfirmAndResultAreSeparate() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC3)
        let initialFact = store.installedFact
        store.loadExampleEvent(.installationSucceeded)
        XCTAssertEqual(store.installedFact, initialFact)
        XCTAssertTrue(store.loadedEvents.isEmpty)
        store.beginIntroductionReview()
        XCTAssertEqual(store.reviewDraft?.itemID, external)
        XCTAssertEqual(store.reviewDraft?.target, store.identity.target)
        XCTAssertFalse(store.reviewDraft?.dependencies.isEmpty ?? true)
        store.cancelReview()
        XCTAssertTrue(store.approvedIntents.isEmpty)
        store.beginIntroductionReview()
        store.confirmReviewIntent()
        XCTAssertEqual(store.approvedIntents.count, 1)
        XCTAssertEqual(store.installedFact, initialFact, "Approval is not installation")
        store.loadExampleEvent(.installationSucceeded)
        XCTAssertEqual(store.installedFact.state, .available)
        XCTAssertTrue(store.installedFact.source.contains("示例事件"))
        let eventCount = store.eventLog.count
        store.loadExampleEvent(.installationSucceeded)
        XCTAssertEqual(store.eventLog.count, eventCount)
        XCTAssertEqual(store.externalExecutionCount, 0)
    }

    @MainActor
    func testTC3InstallPreauthorizationDoesNotAuthorizeUpdateOrAnotherTask() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC3, variant: .installCovered)
        store.beginIntroductionReview()
        XCTAssertNil(store.reviewDraft, "Covered install does not re-ask")
        XCTAssertEqual(store.approvedIntents.count, 1)
        XCTAssertTrue(store.approvedIntents[0].preauthorized)
        store.loadExampleEvent(.updateSucceeded)
        XCTAssertEqual(store.candidateVersionFact.state, .unknown)
        store.beginIntroductionReview(.update)
        XCTAssertEqual(store.reviewDraft?.operation, .update)
        store.cancelReview()
        store.selectContext(.betaOne)
        store.beginIntroductionReview()
        XCTAssertNotNil(store.reviewDraft, "Alpha-only preauthorization cannot leak")
        store.confirmReviewIntent()
        store.loadExampleEvent(.installationSucceeded)
        XCTAssertNotEqual(store.installedFact.state, .available, "The fixed event cannot move to another task")
    }

    @MainActor
    func testTC3UpdatePreauthorizationKeepsInflightAndInstallBoundary() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC3, variant: .updateCovered)
        let version = store.inFlightVersion
        let history = store.businessHistory
        store.beginIntroductionReview(.update)
        XCTAssertNil(store.reviewDraft)
        XCTAssertEqual(store.approvedIntents.first?.draft.version, "demo-v2")
        store.loadExampleEvent(.updateSucceeded)
        XCTAssertEqual(store.candidateVersionFact.state, .available)
        XCTAssertEqual(store.inFlightVersion, version)
        XCTAssertEqual(store.businessHistory, history)
        store.beginIntroductionReview(.install)
        XCTAssertNotNil(store.reviewDraft, "Update coverage is not installation coverage")
        XCTAssertEqual(store.replayCount, 0)
    }

    @MainActor
    func testTC3BundledNewSkillRequiresHumanConsentForExactVersionAndTask() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC3, variant: .bundledSkill)
        store.consentToBundledSkill()
        XCTAssertFalse(store.skillManuallyConsented, "No unanchored blanket skill consent")
        store.beginIntroductionReview()
        XCTAssertNil(store.reviewDraft)
        store.loadExampleEvent(.installationSucceeded)
        XCTAssertEqual(store.installedFact.state, .blocked)
        store.consentToBundledSkill()
        XCTAssertTrue(store.skillManuallyConsented)
        store.loadExampleEvent(.installationSucceeded)
        XCTAssertEqual(store.installedFact.state, .available)
        store.beginIntroductionReview(.update)
        XCTAssertFalse(store.skillManuallyConsented, "v1 consent does not cover v2")
        store.confirmReviewIntent()
        store.loadExampleEvent(.updateSucceeded)
        XCTAssertEqual(store.candidateVersionFact.state, .unknown)
        store.consentToBundledSkill()
        store.loadExampleEvent(.updateSucceeded)
        XCTAssertEqual(store.candidateVersionFact.state, .available)
        store.selectContext(.betaOne)
        XCTAssertFalse(store.skillManuallyConsented)
    }

    @MainActor
    func testTC3ReselectingEarlierApprovedOperationKeepsSkillConsentOnItsVersion() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC3, variant: .bundledSkill)
        store.beginIntroductionReview(.install)
        XCTAssertFalse(store.skillManuallyConsented)
        store.beginIntroductionReview(.update)
        store.confirmReviewIntent()
        XCTAssertEqual(store.approvedIntents.count, 2)

        store.beginIntroductionReview(.install)
        XCTAssertNil(store.reviewDraft, "Covered installation does not ask again")
        XCTAssertEqual(store.approvedIntents.count, 2, "Reselecting does not duplicate approval history")
        store.consentToBundledSkill()
        XCTAssertTrue(store.skillManuallyConsented)
        store.loadExampleEvent(.installationSucceeded)
        XCTAssertEqual(store.installedFact.state, .available, "Consent must follow reselected v1")
        store.loadExampleEvent(.updateSucceeded)
        XCTAssertEqual(store.candidateVersionFact.state, .unknown, "v1 consent never authorizes v2 skill")

        store.beginIntroductionReview(.update)
        XCTAssertFalse(store.skillManuallyConsented, "The selected v2 still needs its own consent")
        store.cancelReview()
        XCTAssertTrue(store.skillManuallyConsented, "Cancelling returns to the approved v1 operation")
        store.beginIntroductionReview(.update)
        store.confirmReviewIntent()
        XCTAssertEqual(store.approvedIntents.count, 2)
        store.consentToBundledSkill()
        store.loadExampleEvent(.updateSucceeded)
        XCTAssertEqual(store.candidateVersionFact.state, .available)
        store.beginIntroductionReview(.install)
        XCTAssertTrue(store.skillManuallyConsented, "Returning to v1 preserves its independent consent")
        XCTAssertEqual(store.externalExecutionCount, 0)
        XCTAssertEqual(store.replayCount, 0)
        store.selectItem(query)
        XCTAssertFalse(store.skillManuallyConsented, "The active operation is item-scoped")
        store.selectItem(external)
        XCTAssertTrue(store.skillManuallyConsented)
        store.setProjectEnabled(false)
        store.consentToBundledSkill()
        XCTAssertFalse(store.skillManuallyConsented, "Invalidating the scope also clears its active operation")
        store.reset()
        store.consentToBundledSkill()
        XCTAssertFalse(store.skillManuallyConsented, "Reset cannot resurrect the previous active operation")
    }

    @MainActor
    func testTC3ScopeChangesInvalidateIntentAndDraftCannotMigrate() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC3)
        store.beginIntroductionReview()
        store.selectItem(query)
        XCTAssertNil(store.reviewDraft)
        store.confirmReviewIntent()
        XCTAssertTrue(store.approvedIntents.isEmpty)
        store.selectItem(external)
        store.beginIntroductionReview()
        store.confirmReviewIntent()
        store.setProjectEnabled(false)
        XCTAssertTrue(store.approvedIntents.isEmpty)
        store.loadExampleEvent(.installationSucceeded)
        XCTAssertEqual(store.installedFact.state, .blocked)
        store.beginIntroductionReview(.update)
        store.selectContext(.alphaTwo)
        XCTAssertNil(store.reviewDraft)
        store.confirmReviewIntent()
        XCTAssertTrue(store.approvedIntents.isEmpty)
    }

    @MainActor
    func testTC3SkillConsentRemainsVisibleAfterAnotherTaskReview() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC3, variant: .bundledSkill)
        store.beginIntroductionReview()
        store.consentToBundledSkill()
        XCTAssertTrue(store.skillManuallyConsented)
        store.selectContext(.betaOne)
        store.beginIntroductionReview()
        store.confirmReviewIntent()
        store.selectContext(.alphaOne)
        XCTAssertTrue(store.skillManuallyConsented, "Another task's last intent cannot hide this consent")
        store.consentToBundledSkill()
        XCTAssertTrue(store.skillManuallyConsented)
    }

    @MainActor
    func testTC3InvalidatingOneProjectPreservesOtherProjectSkillConsent() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC3, variant: .bundledSkill)
        store.selectContext(.betaOne)
        store.beginIntroductionReview()
        store.confirmReviewIntent()
        store.consentToBundledSkill()
        XCTAssertTrue(store.skillManuallyConsented)
        store.selectContext(.alphaOne)
        store.setProjectEnabled(false)
        store.selectContext(.betaOne)
        XCTAssertTrue(store.skillManuallyConsented, "Scope invalidation must not clear another project's consent")
    }

    @MainActor
    func testTC3RemoteUnpinnedAndConditionalRollbackPreserveBusinessHistory() async {
        let remote = ToolsConnectionsReviewStore(scenario: .TC3, variant: .remoteUnpinned)
        remote.beginIntroductionReview(.update)
        XCTAssertTrue(remote.reviewDraft?.location.contains("remote-service") ?? false)
        XCTAssertFalse(remote.eligibility(for: external).eligible)
        XCTAssertTrue(remote.eligibility(for: external).reasons.contains { $0.contains("不可固定") })
        let unknown = ToolsConnectionsReviewStore(scenario: .TC3, variant: .rollbackUnknown)
        unknown.beginIntroductionReview(.rollback)
        XCTAssertFalse(unknown.rollbackProjection.eligible)
        XCTAssertNil(unknown.reviewDraft)
        let eligible = ToolsConnectionsReviewStore(scenario: .TC3, variant: .rollbackEligible)
        let history = eligible.businessHistory
        XCTAssertTrue(eligible.rollbackProjection.eligible)
        eligible.beginIntroductionReview(.rollback)
        XCTAssertEqual(eligible.reviewDraft?.version, "demo-v1")
        eligible.confirmReviewIntent()
        XCTAssertEqual(eligible.approvedIntents.first?.draft.operation, .rollback)
        XCTAssertEqual(eligible.businessHistory, history)
        XCTAssertTrue(eligible.updateHasFailed)
        XCTAssertEqual(eligible.replayCount, 0)
    }

    @MainActor
    func testTC4CancelDisableTaskScopeAndReferencedCapabilityCannotBypass() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC4)
        let history = store.businessHistory
        let configuration = store.configuredIDs
        store.beginDisableDraft()
        store.cancelDisableDraft()
        XCTAssertFalse(store.isDisabled(connection, in: .alphaOne))
        store.beginDisableDraft()
        store.confirmDisableIntent()
        XCTAssertTrue(store.isDisabled(connection, in: .alphaOne))
        XCTAssertFalse(store.isDisabled(connection, in: .alphaTwo))
        XCTAssertFalse(store.isDisabled(connection, in: .betaOne))
        XCTAssertEqual(store.stopState, .requested)
        XCTAssertFalse(store.eligibility(for: query).eligible, "Referenced connection disable cannot be bypassed")
        XCTAssertEqual(store.configuredIDs, configuration)
        XCTAssertEqual(store.businessHistory, history)
        store.loadExampleEvent(.stopped)
        XCTAssertEqual(store.stopState, .confirmed)
        store.reenableScope(.task)
        store.loadExampleEvent(.conditionsRestored)
        XCTAssertFalse(store.continuationProjection.eligible, "Reenable does not resume confirmed stop")
        store.recordExplicitResume()
        XCTAssertTrue(store.continuationProjection.eligible)
        XCTAssertEqual(store.externalExecutionCount, 0)
    }

    @MainActor
    func testTC4ProjectDisableFromSiblingCoversOnlyThatProjectAndRequestsKnownInflight() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC4)
        store.selectContext(.alphaTwo)
        store.beginDisableDraft(scope: .project)
        store.confirmDisableIntent()
        XCTAssertTrue(store.isDisabled(connection, in: .alphaOne))
        XCTAssertTrue(store.isDisabled(connection, in: .alphaTwo))
        XCTAssertFalse(store.isDisabled(connection, in: .betaOne))
        store.selectContext(.alphaOne)
        XCTAssertEqual(store.stopState, .requested)
        store.selectContext(.standalone)
        store.beginDisableDraft(scope: .project)
        store.confirmDisableIntent()
        XCTAssertFalse(store.isDisabled(connection, in: .standalone))
    }

    @MainActor
    func testTC4WaitingRestorationRechecksWithoutApprovalOrExecution() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC4)
        XCTAssertFalse(store.continuationProjection.eligible)
        store.loadExampleEvent(.conditionsRestored)
        XCTAssertTrue(store.continuationProjection.eligible)
        XCTAssertTrue(store.approvedIntents.isEmpty)
        XCTAssertEqual(store.continuation.disposition, .waiting)
        XCTAssertEqual(store.externalExecutionCount, 0)
        let blocked = ToolsConnectionsReviewStore(scenario: .TC4, variant: .blocked)
        blocked.loadExampleEvent(.conditionsRestored)
        XCTAssertFalse(blocked.continuationProjection.eligible)
        XCTAssertTrue(blocked.continuationProjection.reasons.contains { $0.contains("依赖") })
        XCTAssertTrue(blocked.continuationProjection.reasons.contains { $0.contains("目标") })
        XCTAssertTrue(blocked.continuationProjection.reasons.contains { $0.contains("授权") })
    }

    @MainActor
    func testTC4PausedStoppedCancelledAndExpiredDoNotReviveOnRestoration() async {
        for variant in [TConnReviewVariant.paused, .stopped, .cancelled, .expired] {
            let store = ToolsConnectionsReviewStore(scenario: .TC4, variant: variant)
            store.loadExampleEvent(.conditionsRestored)
            store.reenableScope(.task)
            XCTAssertFalse(store.continuationProjection.eligible, variant.rawValue)
            store.recordExplicitResume()
            XCTAssertEqual(store.continuationProjection.eligible, variant != .expired)
            XCTAssertEqual(store.replayCount, 0)
        }
    }

    @MainActor
    func testTC4ExplicitStopNeedsConfirmationAndResumeCannotCreateOverlap() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC4)
        store.recordExplicitStop()
        XCTAssertEqual(store.stopState, .requested)
        store.loadExampleEvent(.conditionsRestored)
        store.recordExplicitResume()
        XCTAssertEqual(store.continuation.disposition, .stopped)
        XCTAssertFalse(store.continuationProjection.eligible)
        store.loadExampleEvent(.stopped)
        XCTAssertEqual(store.stopState, .confirmed)
        XCTAssertFalse(store.continuationProjection.eligible)
        store.recordExplicitResume()
        XCTAssertTrue(store.continuationProjection.eligible)
        store.recordExplicitStop()
        XCTAssertEqual(store.stopState, .confirmed, "No second in-flight execution is manufactured")
        XCTAssertFalse(store.continuationProjection.eligible)
    }

    @MainActor
    func testTC4UnknownOriginalMustBeVerifiedAndEventsDoNotMigrate() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC4, variant: .unknownExecution)
        let history = store.businessHistory
        store.selectItem(query)
        store.loadExampleEvent(.originalExecutionVerified)
        XCTAssertFalse(store.continuation.originalResultKnown)
        XCTAssertTrue(store.loadedEvents.isEmpty)
        store.selectItem(connection)
        store.selectContext(.betaOne)
        store.loadExampleEvent(.conditionsRestored)
        XCTAssertFalse(store.continuation.resourceReady)
        store.selectContext(.alphaOne)
        store.loadExampleEvent(.conditionsRestored)
        XCTAssertFalse(store.continuationProjection.eligible)
        store.loadExampleEvent(.originalExecutionVerified)
        XCTAssertTrue(store.continuationProjection.eligible)
        XCTAssertEqual(store.businessHistory, history)
        XCTAssertEqual(store.replayCount, 0)
    }

    @MainActor
    func testTC5UseNeverGrantsOriginalExportOtherModelOrCrossHost() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC5)
        store.recordCredentialIntent(.use)
        XCTAssertTrue(store.credentialMessage.contains("未供给秘密"))
        for intent in [TConnReviewCredentialIntent.original, .export, .otherModel, .crossHost] {
            store.recordCredentialIntent(intent)
            XCTAssertTrue(store.credentialMessage.contains("缺独立适用授权"))
            XCTAssertTrue(store.credentialMessage.contains("未执行"))
        }
        XCTAssertEqual(ToolsConnectionsReviewFixtures.credentialReference, "demo-credential-reference")
        XCTAssertTrue(store.sharePreview.contains("[秘密省略]"))
        XCTAssertTrue(store.sharePreview.contains("不实现"))
        XCTAssertTrue(store.loadedEvents.isEmpty)
        XCTAssertEqual(store.externalExecutionCount, 0)
        store.selectContext(.betaOne)
        store.recordCredentialIntent(.use)
        XCTAssertFalse(store.credentialMessage.contains("已覆盖"))
    }

    @MainActor
    func testTC5MissingRevokedAndLocationMismatchNeverFillCredentials() async {
        for (variant, state) in [
            (TConnReviewVariant.credentialMissing, TConnReviewCredentialState.missing),
            (.credentialRevoked, .revoked), (.credentialLocationMismatch, .locationMismatch),
        ] {
            let store = ToolsConnectionsReviewStore(scenario: .TC5, variant: variant)
            XCTAssertEqual(store.credentialState, state)
            for intent in TConnReviewCredentialIntent.allCases {
                store.recordCredentialIntent(intent)
                XCTAssertTrue(store.credentialMessage.contains("不扫描个人配置"))
            }
            XCTAssertEqual(store.credentialState, state)
            XCTAssertEqual(store.identity, TConnReviewIdentity())
            XCTAssertTrue(store.availableEvents.isEmpty)
        }
    }

    @MainActor
    func testTC5CredentialIntentPresentationDoesNotLeakOnContextOrItemBrowsing() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC5)
        store.recordCredentialIntent(.use)
        let alpha = store.credentialMessage
        XCTAssertTrue(alpha.contains("已覆盖"))
        store.selectContext(.betaOne)
        XCTAssertFalse(store.credentialMessage.contains("已覆盖"), "Browsing is enough to update scoped presentation")
        store.selectContext(.alphaOne)
        XCTAssertEqual(store.credentialMessage, alpha)
        store.selectItem(query)
        XCTAssertFalse(store.credentialMessage.contains("已覆盖"), "A connection intent cannot migrate to a capability")
    }

    @MainActor
    func testTC6ExpandCompleteShareLeaveEveryEvidenceLayerAndErrorsUntouched() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC6)
        let before = store.results
        for result in before {
            store.toggleResultExpansion(result.id)
            XCTAssertTrue(store.expandedResultIDs.contains(result.id))
            store.requestResultCompletionIntent(result.id)
            store.recordResultShareIntent(result.id)
            XCTAssertEqual(store.results, before)
            store.toggleResultExpansion(result.id)
        }
        XCTAssertTrue(store.expandedResultIDs.isEmpty)
        XCTAssertTrue(store.loadedEvents.isEmpty)
        XCTAssertEqual(before[0].evidence.returned.state, .available)
        XCTAssertEqual(before[0].evidence.business.state, .blocked)
        XCTAssertFalse(before[0].errors.isEmpty)
        XCTAssertGreaterThan(before[1].body.count, before[1].preview.count)
        XCTAssertTrue(before[1].limits.contains("部分覆盖"))
        XCTAssertTrue(before[1].limits.contains { $0.contains("截断") })
        XCTAssertTrue(before[3].body.contains("资料"))
        XCTAssertTrue(store.approvedIntents.isEmpty, "Untrusted returned text is never an instruction")
        XCTAssertEqual(store.replayCount, 0)
    }

    @MainActor
    func testTC6ExplicitEventsAffectOnlyNamedLayerAndResultIdempotently() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC6)
        let first = store.results[0].id
        let second = store.results[1]
        let business = store.results[0].evidence.business
        store.loadExampleEvent(.resultRead, resultID: first)
        XCTAssertEqual(store.results[0].evidence.read.state, .available)
        XCTAssertEqual(store.results[0].evidence.adopted.state, .unknown)
        XCTAssertEqual(store.results[0].evidence.verified.state, .unknown)
        XCTAssertEqual(store.results[0].evidence.accepted.state, .unknown)
        XCTAssertEqual(store.results[1], second)
        store.loadExampleEvent(.resultRead, resultID: first)
        XCTAssertEqual(store.eventLog.count, 1)
        store.loadExampleEvent(.resultAdopted, resultID: first)
        XCTAssertEqual(store.results[0].evidence.verified.state, .unknown)
        store.loadExampleEvent(.resultVerified, resultID: first)
        XCTAssertEqual(store.results[0].evidence.accepted.state, .unknown)
        store.loadExampleEvent(.resultAccepted, resultID: first)
        XCTAssertEqual(store.results[0].evidence.accepted.state, .available)
        XCTAssertEqual(store.results[0].evidence.business, business, "Acceptance is not business success")
        XCTAssertEqual(store.eventLog.count, 4)
        XCTAssertTrue(store.results[0].evidence.modelReport.contains("非宿主"))
        store.selectItem(connection)
        store.loadExampleEvent(.resultRead, resultID: second.id)
        XCTAssertEqual(store.results[1], second)
        store.selectItem(query)
        store.selectContext(.betaOne)
        store.loadExampleEvent(.resultRead, resultID: second.id)
        XCTAssertEqual(store.results[1], second)
    }

    @MainActor
    func testTC6MissingReferencePreservesReturnedUnknownAndNeverReplaysWrite() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC6, variant: .referenceMissing)
        let result = store.results[2]
        XCTAssertFalse(result.referenceAvailable)
        XCTAssertTrue(result.limits.contains("示例引用已失效"))
        let history = store.businessHistory
        store.requestResultCompletionIntent(result.id)
        XCTAssertEqual(store.results[2], result)
        XCTAssertEqual(result.evidence.returned.state, .available)
        XCTAssertEqual(result.evidence.business.state, .unknown)
        XCTAssertEqual(store.businessHistory, history)
        XCTAssertEqual(store.replayCount, 0)
        store.loadExampleEvent(.resultVerified, resultID: "demo-nonexistent")
        XCTAssertTrue(store.loadedEvents.isEmpty)
    }

    @MainActor
    func testEveryScenarioResetRestoresFixtureAndInstancesStayIndependent() async {
        for scenario in ToolsConnectionsReviewScenario.allCases {
            let store = ToolsConnectionsReviewStore(scenario: scenario)
            let fresh = ToolsConnectionsReviewStore(scenario: scenario)
            store.beginScopeDraft()
            store.saveConfiguration()
            store.referenceForTask(temporary: true)
            store.beginIntroductionReview()
            store.confirmReviewIntent()
            store.beginDisableDraft()
            store.confirmDisableIntent()
            store.recordExplicitStop()
            store.toggleResultExpansion(store.results[0].id)
            for event in store.availableEvents { store.loadExampleEvent(event) }
            XCTAssertTrue(fresh.loadedEvents.isEmpty)
            XCTAssertTrue(fresh.disabledScopes.isEmpty)
            XCTAssertTrue(fresh.approvedIntents.isEmpty)
            XCTAssertTrue(fresh.expandedResultIDs.isEmpty)
            XCTAssertNil(fresh.scopeDraft)
            store.reset()
            XCTAssertEqual(store.context, fresh.context)
            XCTAssertEqual(store.configuredIDs, fresh.configuredIDs)
            XCTAssertEqual(store.projectEnabled, fresh.projectEnabled)
            XCTAssertEqual(store.taskEnabled, fresh.taskEnabled)
            XCTAssertEqual(store.taskReferences, fresh.taskReferences)
            XCTAssertEqual(store.authorizedTasks, fresh.authorizedTasks)
            XCTAssertEqual(store.results, fresh.results)
            XCTAssertEqual(store.continuation, fresh.continuation)
            XCTAssertEqual(store.identity, fresh.identity)
            XCTAssertTrue(store.loadedEvents.isEmpty)
            XCTAssertTrue(store.disabledScopes.isEmpty)
            XCTAssertTrue(store.approvedIntents.isEmpty)
            XCTAssertTrue(store.expandedResultIDs.isEmpty)
            XCTAssertNil(store.scopeDraft)
            XCTAssertNil(store.disableDraft)
            XCTAssertNil(store.reviewDraft)
            XCTAssertEqual(store.externalExecutionCount, 0)
            XCTAssertEqual(store.replayCount, 0)
        }
    }

    @MainActor
    func testScenarioSwitchDiscardsDraftsEventsSelectionAndStop() async {
        let store = ToolsConnectionsReviewStore(scenario: .TC4)
        store.beginDisableDraft()
        store.confirmDisableIntent()
        store.recordExplicitStop()
        store.loadExampleEvent(.stopped)
        store.beginScopeDraft()
        store.loadScenario(.TC3, variant: .bundledSkill)
        XCTAssertEqual(store.selectedID, external)
        XCTAssertTrue(store.stopStates.isEmpty)
        XCTAssertTrue(store.disabledScopes.isEmpty)
        XCTAssertTrue(store.loadedEvents.isEmpty)
        XCTAssertNil(store.scopeDraft)
        XCTAssertFalse(store.skillManuallyConsented)
        store.loadExampleEvent(.taskAuthorized)
        XCTAssertTrue(store.loadedEvents.isEmpty, "Events are scenario constrained")
        store.loadScenario(.TC1, variant: .bundledSkill)
        XCTAssertEqual(store.variant, .standard, "Unsupported preset is not silently reinterpreted")
        XCTAssertTrue(store.configuredIDs.isEmpty)
    }

    private struct EntryFixture {
        let home: URL
        var caches: URL { home.appendingPathComponent("Library/Caches", isDirectory: true) }
        var candidate: URL { caches.appendingPathComponent("review/not-created", isDirectory: true) }
        var environment: [String: String] {
            [
                NativeToolsConnectionsReviewEntry.flagKey: "1",
                "AERORIC_ENV": "ui-test",
                "AERORIC_DATA_DIR": candidate.path,
            ]
        }
    }

    private func withEntryFixture(_ body: (EntryFixture) throws -> Void) throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("tools-connections-unit-\(UUID().uuidString)", isDirectory: true)
        let fixture = EntryFixture(home: root.appendingPathComponent("fake-home", isDirectory: true))
        try FileManager.default.createDirectory(at: fixture.caches, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        try body(fixture)
    }

    private func decide(
        _ environment: [String: String], _ fixture: EntryFixture
    ) -> ToolsConnectionsReviewEntryDecision {
        NativeToolsConnectionsReviewEntry.decide(environment: environment, home: fixture.home.path)
    }

    /// Own fixture metadata only; symlinks are described, not traversed.
    private func tree(_ root: URL) throws -> [String] {
        var paths: [String] = []
        for child in try FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: [.isDirectoryKey]) {
            if let target = try? FileManager.default.destinationOfSymbolicLink(atPath: child.path) {
                paths.append(child.path + " -> " + target)
            } else {
                paths.append(child.path)
                if try child.resourceValues(forKeys: [.isDirectoryKey]).isDirectory == true {
                    paths.append(contentsOf: try tree(child))
                }
            }
        }
        return paths.sorted()
    }
}
#endif
