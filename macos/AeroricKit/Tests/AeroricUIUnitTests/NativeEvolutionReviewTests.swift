#if DEBUG
import AppKit
import SwiftUI
import XCTest
@testable import AeroricUI

/// Unit coverage for the task-evolution review sample: entry decision, version
/// binding, role independence, scenario gates, in-memory reset and component
/// assembly. This is assembly/policy coverage, NOT real-window or AX
/// acceptance — that lives in `NativeEvolutionReviewUITests`.
final class NativeEvolutionReviewTests: XCTestCase {

    // MARK: - Entry decision

    @MainActor
    func testAbsentFlagLeavesTheOriginalStartupSemanticsUntouched() {
        // No new flag: whatever else is set, the sample must not activate and
        // must not refuse — the original visual-review/product path decides.
        for environment: [String: String] in [
            [:],
            [NativeEvolutionReviewEntry.environmentKey: "ui-test"],
            [NativeEvolutionReviewEntry.dataDirectoryKey: isolatedDirectory()],
            [NativeVisualReviewEntry.flagKey: "1",
             NativeVisualReviewEntry.environmentKey: "ui-test",
             NativeVisualReviewEntry.dataDirectoryKey: isolatedDirectory()],
        ] {
            let decision = NativeEvolutionReviewEntry.decide(environment: environment)
            XCTAssertEqual(decision, .inactive, "\(environment)")
            let lifecycle = NativeEvolutionReviewWindowLifecycle(decision: decision)
            XCTAssertNil(lifecycle.windowIdentifier)
            XCTAssertTrue(lifecycle.windowTitle.isEmpty)
            lifecycle.showReviewWindow()
            XCTAssertFalse(lifecycle.isWindowCreated,
                           "an inactive entry must not create even a DEBUG review window")
        }
    }

    @MainActor
    func testValidRequestEntersTheReviewSceneOnly() {
        let decision = NativeEvolutionReviewEntry.decide(environment: reviewEnvironment)
        XCTAssertEqual(decision, .review)
        let lifecycle = NativeEvolutionReviewWindowLifecycle(decision: decision)
        XCTAssertEqual(lifecycle.windowIdentifier?.rawValue, "native-evolution-review")
        XCTAssertEqual(lifecycle.windowTitle, "任务内进化审阅样例")
        XCTAssertFalse(lifecycle.isWindowCreated,
                       "policy initialization must not launch an App or construct a window")
    }

    @MainActor
    func testInvalidRequestsAreRefusedInsteadOfFallingBackToTheProduct() {
        var wrongEnvironment = reviewEnvironment
        wrongEnvironment[NativeEvolutionReviewEntry.environmentKey] = "default"
        XCTAssertEqual(NativeEvolutionReviewEntry.decide(environment: wrongEnvironment),
                       .refused([.environmentNotUITest]))

        var missingDirectory = reviewEnvironment
        missingDirectory.removeValue(forKey: NativeEvolutionReviewEntry.dataDirectoryKey)
        XCTAssertEqual(NativeEvolutionReviewEntry.decide(environment: missingDirectory),
                       .refused([.dataDirectoryMissing]))

        for environment in [wrongEnvironment, missingDirectory] {
            let lifecycle = NativeEvolutionReviewWindowLifecycle(
                decision: NativeEvolutionReviewEntry.decide(environment: environment)
            )
            XCTAssertEqual(lifecycle.windowIdentifier?.rawValue, "native-evolution-review-refusal")
            XCTAssertEqual(lifecycle.windowTitle, "审阅未启动")
            XCTAssertFalse(lifecycle.isWindowCreated)
        }

        for value in ["", "0", "true", " 1"] {
            var environment = reviewEnvironment
            environment[NativeEvolutionReviewEntry.flagKey] = value
            let decision = NativeEvolutionReviewEntry.decide(environment: environment)
            XCTAssertEqual(decision, .refused([.flagValueNotOne]), value)
            let lifecycle = NativeEvolutionReviewWindowLifecycle(decision: decision)
            XCTAssertEqual(lifecycle.windowIdentifier?.rawValue, "native-evolution-review-refusal")
            XCTAssertFalse(lifecycle.isWindowCreated)
        }
    }

    @MainActor
    func testBothReviewFlagsAtOnceAreRefusedRatherThanOrdered() {
        var environment = reviewEnvironment
        environment[NativeVisualReviewEntry.flagKey] = "1"
        let decision = NativeEvolutionReviewEntry.decide(environment: environment)
        XCTAssertEqual(decision, .refused([.legacyVisualReviewAlsoRequested]))
        XCTAssertNotEqual(decision, .review,
                          "code order must not silently pick the new entry over the old one")
        let lifecycle = NativeEvolutionReviewWindowLifecycle(decision: decision)
        XCTAssertEqual(lifecycle.windowIdentifier?.rawValue, "native-evolution-review-refusal")
        XCTAssertFalse(lifecycle.isWindowCreated)
    }

    func testEveryRefusalReasonHasAReadableTitleWithoutASensitivePath() {
        for reason in EvolutionReviewRefusalReason.allCases {
            XCTAssertFalse(reason.title.isEmpty, reason.rawValue)
            XCTAssertFalse(reason.title.contains("/"), reason.rawValue)
            XCTAssertFalse(reason.title.contains(NSHomeDirectory()), reason.rawValue)
        }
    }

    /// The new entry reuses `NativeVisualReviewEntry.isIsolatedDataDirectory`
    /// rather than adding a looser prefix test of its own. These are the
    /// boundary shapes the execution package names: relative path, `..`, root,
    /// prefix squatting and symlinks.
    func testIsolatedDirectoryBoundaryMatrix() throws {
        let fixture = FileManager.default.temporaryDirectory
            .appendingPathComponent("aeroric-evolution-boundary-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: fixture) }
        let home = fixture.appendingPathComponent("home")
        let caches = home.appendingPathComponent("Library/Caches")
        let outside = fixture.appendingPathComponent("outside")
        try FileManager.default.createDirectory(at: caches, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: outside, withIntermediateDirectories: true)
        let link = caches.appendingPathComponent("linked-review")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: outside)

        let rejected: [String] = [
            "Library/Caches/review",                       // relative
            caches.path,                                   // the Caches root itself
            "/",                                           // filesystem root
            caches.path + "/../Application Support/x",     // traversal out
            home.path + "/Library/Caches-neighbour/x",     // prefix squatting
            home.path + "/Library/Application Support/Aeroric",
            link.appendingPathComponent("child").path,     // symlinked descendant
            caches.path + "/link/../escaped-review",       // traversal hiding a symlink
            " ",                                           // blank
        ]
        for path in rejected {
            XCTAssertFalse(NativeVisualReviewEntry.isIsolatedDataDirectory(path, home: home.path), path)
        }
        XCTAssertTrue(NativeVisualReviewEntry.isIsolatedDataDirectory(
            caches.appendingPathComponent("review-unique").path, home: home.path
        ))

        // The decision function must refuse on the same boundary.
        var environment = reviewEnvironment
        environment[NativeEvolutionReviewEntry.dataDirectoryKey] = link.appendingPathComponent("child").path
        XCTAssertEqual(NativeEvolutionReviewEntry.decide(environment: environment),
                       .refused([.dataDirectoryNotIsolated]))
    }

    // MARK: - Version binding

    @MainActor
    func testOldVersionReviewCannotApproveANewVersion() {
        let store = EvolutionReviewStore(scenario: .s7VersionMismatchAndStyle)
        store.setTrack(.capabilityImprovement)
        guard let candidate = store.selectedPhase?.candidates.first else {
            return XCTFail("S7 must expose a candidate")
        }
        XCTAssertEqual(candidate.requiredReview, .staleVersion)
        XCTAssertEqual(candidate.requiredReviewVersion, "v2")
        XCTAssertEqual(candidate.version, "v3")
        XCTAssertFalse(candidate.isTriallable)
        XCTAssertEqual(store.gateReason(for: candidate),
                       "现有评审针对 v2，不能批准当前版本 v3")
        let originalEvidence = store.selectedPhase?.evidence ?? []
        guard case .refused = store.perform(.trialCandidate) else {
            return XCTFail("an old-version review must refuse the actual demo action")
        }
        XCTAssertEqual(store.selectedPhase?.candidates.first, candidate)
        store.perform(.addCandidateReview)
        XCTAssertEqual(store.selectedPhase?.candidates.first?.requiredReviewVersion, "v3")
        XCTAssertEqual(Array((store.selectedPhase?.evidence ?? []).prefix(originalEvidence.count)), originalEvidence)
        XCTAssertNil(store.blockedReason(for: .trialCandidate))
        XCTAssertNotNil(store.selectedPhase?.opinions.first { $0.id == "op.version.mismatch" }?.resolutionNote)
    }

    func testCandidateTrialGateRequiresVerificationReviewAndApproval() {
        func candidate(verification: EvolutionReviewGateState,
                       review: EvolutionReviewGateState,
                       approval: EvolutionReviewGateState,
                       reviewVersion: String? = nil) -> EvolutionReviewCandidate {
            EvolutionReviewCandidate(
                id: "cand.unit", title: "单元候选", toolName: "单元工具", version: "v1",
                verification: verification, requiredReview: review,
                requiredReviewVersion: reviewVersion, approval: approval,
                trial: .blocked, notes: []
            )
        }
        XCTAssertTrue(candidate(verification: .satisfied, review: .satisfied, approval: .satisfied)
            .isTriallable)
        XCTAssertFalse(candidate(verification: .failed, review: .satisfied, approval: .satisfied)
            .isTriallable)
        XCTAssertFalse(candidate(verification: .satisfied, review: .missing, approval: .satisfied)
            .isTriallable)
        XCTAssertFalse(candidate(verification: .satisfied, review: .satisfied, approval: .missing)
            .isTriallable)
        XCTAssertFalse(candidate(verification: .satisfied, review: .staleVersion, approval: .satisfied)
            .isTriallable)
        XCTAssertFalse(candidate(verification: .satisfied, review: .satisfied, approval: .satisfied,
                                 reviewVersion: "v0").isTriallable,
                       "a satisfied flag must not override an explicit old review version")
        XCTAssertTrue(candidate(verification: .satisfied, review: .satisfied, approval: .satisfied,
                                reviewVersion: "v1").isTriallable)
    }

    // MARK: - Role independence

    @MainActor
    func testRebindingOneRoleNeverChangesTheOthers() {
        let store = EvolutionReviewStore()
        let reflectionBefore = store.bindingSummary(for: .reflection)
        let governanceBefore = store.bindingSummary(for: .governance)

        store.beginRoleConfiguration()
        store.setDraftMode(.overriding, for: .mainExecutor)
        store.setDraftConnection("conn.b", for: .mainExecutor)
        store.setDraftModel("model.2", for: .mainExecutor)
        store.applyDraft()

        XCTAssertEqual(store.bindingSummary(for: .mainExecutor), "覆盖 · 示例连接B · 示例模型2")
        XCTAssertEqual(store.bindingSummary(for: .reflection), reflectionBefore)
        XCTAssertEqual(store.bindingSummary(for: .governance), governanceBefore)
    }

    @MainActor
    func testRestoringInheritanceReturnsTheTemplateValueForThatRoleOnly() {
        let store = EvolutionReviewStore()
        let template = store.templateSummary(for: .mainExecutor)

        store.beginRoleConfiguration()
        store.setDraftMode(.overriding, for: .mainExecutor)
        store.setDraftModel("model.2", for: .mainExecutor)
        store.applyDraft()
        XCTAssertNotEqual(store.bindingSummary(for: .mainExecutor), "继承模板 · \(template)")

        store.beginRoleConfiguration()
        store.restoreInheritanceInDraft(for: .mainExecutor)
        store.applyDraft()
        XCTAssertEqual(store.bindingSummary(for: .mainExecutor), "继承模板 · \(template)")
    }

    @MainActor
    func testCancellingTheFormAppliesNoDraft() {
        let store = EvolutionReviewStore()
        let before = EvolutionReviewRole.allCases.map { store.bindingSummary(for: $0) }

        store.beginRoleConfiguration()
        store.setDraftMode(.overriding, for: .governance)
        store.setDraftModel("model.1", for: .governance)
        store.cancelDraft()

        XCTAssertNil(store.draft)
        XCTAssertEqual(EvolutionReviewRole.allCases.map { store.bindingSummary(for: $0) }, before)
    }

    @MainActor
    func testApplyingTheFormOnlyTouchesSampleMemory() {
        let store = EvolutionReviewStore()
        store.beginRoleConfiguration()
        store.setDraftMode(.overriding, for: .reflection)
        store.applyDraft()
        XCTAssertEqual(store.notice, EvolutionReviewStore.sampleOnlyNotice)
        XCTAssertTrue(store.notice.contains("未保存真实配置"))
    }

    @MainActor
    func testGovernanceIsNeverRenderedAsARunningService() {
        let store = EvolutionReviewStore()
        XCTAssertTrue(store.governanceProcessNote.contains("独立进程为产品设计要求"))
        XCTAssertTrue(store.governanceProcessNote.contains("本样例未启动进程"))
        XCTAssertFalse(store.governanceProcessNote.contains("运行中"))
    }

    // MARK: - Scenario gates

    @MainActor
    func testS1KeepsDependentWorkWaitingAndIndependentWorkContinuing() {
        let store = EvolutionReviewStore(scenario: .s1InterfacePending)
        guard let phase = store.selectedPhase else { return XCTFail("S1 must select a phase") }
        XCTAssertEqual(phase.state, .pendingReflection)
        XCTAssertEqual(phase.dependencies.first { $0.id == "dep.interface.integration" }?.state, .waiting)
        XCTAssertEqual(phase.dependencies.first { $0.id == "dep.interface.docs" }?.state, .continuing)
        XCTAssertEqual(store.blockedReason(for: .advanceDependents),
                       "阶段尚未满足既定验证与反思（当前：待反思核对），不能推进依赖")
        XCTAssertEqual(store.perform(.sendReflectionFeedback), .applied("反思反馈已送出（样例）：送出 ≠ 消费 ≠ 整改 ≠ 验证"))
        XCTAssertTrue(store.sample.feedbackDeliveryNote.contains("未确认主执行已停止或已整改"))
    }

    @MainActor
    func testS2ClosesTheOrdinaryPhaseWithoutWaitingForGovernance() {
        let store = EvolutionReviewStore(scenario: .s2OrdinaryPassedGovernancePending)
        guard let phase = store.selectedPhase else { return XCTFail("S2 must select a phase") }
        XCTAssertEqual(phase.state, .passed)
        XCTAssertEqual(phase.governance, .pendingReview)
        XCTAssertNil(store.blockedReason(for: .advanceDependents))
        store.perform(.advanceDependents)
        XCTAssertEqual(store.selectedPhase?.dependencies.first?.state, .released)
        XCTAssertEqual(store.selectedPhase?.governance, .pendingReview,
                       "advancing dependents must not turn governance pending into passed")
    }

    @MainActor
    func testS3AndS4KeepMissingReviewCandidatesDisabled() {
        for scenario in [EvolutionReviewScenarioID.s3CandidateMissingReview,
                         .s4SupervisionFailure, .s6RemediationAndDispute, .s7VersionMismatchAndStyle] {
            let store = EvolutionReviewStore(scenario: scenario)
            store.setTrack(.capabilityImprovement)
            guard let candidate = store.selectedPhase?.candidates.first else {
                return XCTFail("\(scenario.code) must expose a candidate")
            }
            XCTAssertEqual(candidate.trial, .blocked, scenario.code)
            XCTAssertFalse(candidate.isTriallable, scenario.code)
            guard let reason = store.blockedReason(for: .trialCandidate) else {
                return XCTFail("\(scenario.code): a disabled action must show a readable reason, not only grey out")
            }
            XCTAssertFalse(reason.isEmpty, scenario.code)
        }
    }

    @MainActor
    func testS3RequiresTheSameVersionReviewBeforeTrialBecomesAvailable() {
        let store = EvolutionReviewStore(scenario: .s3CandidateMissingReview)
        store.setTrack(.capabilityImprovement)
        XCTAssertNotNil(store.blockedReason(for: .trialCandidate))
        XCTAssertNil(store.blockedReason(for: .addCandidateReview))
        let originalOpinions = store.selectedPhase?.opinions ?? []
        let originalEvidence = store.selectedPhase?.evidence ?? []
        store.perform(.addCandidateReview)
        guard let candidate = store.selectedPhase?.candidates.first else {
            return XCTFail("S3 must keep its candidate")
        }
        XCTAssertEqual(candidate.requiredReview, .satisfied)
        XCTAssertEqual(candidate.requiredReviewVersion, candidate.version,
                       "the added review must be bound to the current candidate version")
        XCTAssertNil(store.blockedReason(for: .trialCandidate))
        XCTAssertEqual(store.selectedPhase?.governance, .reviewed)
        XCTAssertEqual(store.selectedPhase?.state, .passed)
        XCTAssertTrue(store.selectedPhase?.unsatisfiedRequirements.isEmpty ?? false)
        XCTAssertEqual(Array((store.selectedPhase?.evidence ?? []).prefix(originalEvidence.count)), originalEvidence)
        for id in ["req.tool.review", "req.tool.approval"] {
            let requirement = store.selectedPhase?.requirements.first { $0.id == id }
            XCTAssertEqual(requirement?.satisfied, true)
            XCTAssertTrue(requirement?.note?.contains("v2") ?? false)
            XCTAssertFalse(requirement?.note?.contains("尚未满足") ?? true)
        }
        for original in originalOpinions {
            guard let resolved = store.selectedPhase?.opinions.first(where: { $0.id == original.id }) else {
                return XCTFail("review completion must not delete the original opinion")
            }
            var historical = resolved
            historical.resolutionNote = original.resolutionNote
            XCTAssertEqual(historical, original, "only the current resolution annotation may change")
            XCTAssertNotNil(resolved.resolutionNote)
            let detail = store.detailPayload(for: .opinion(phaseID: "phase.tool", opinionID: original.id))
            XCTAssertEqual(detail?.fields.first { $0.id == "状态" }?.value, "历史意见（已解决）")
            XCTAssertTrue(detail?.body.contains(original.body) ?? false)
        }
        XCTAssertNotNil(store.blockedReason(for: .reopenPhase),
                        "resolved historical blocking opinions must not reopen a passed phase")
        XCTAssertTrue(candidate.notes.contains { $0.contains("同版本必要评审与既有审批已满足") })
        store.perform(.trialCandidate)
        XCTAssertEqual(store.selectedPhase?.candidates.first?.trial, .trialing)
        XCTAssertEqual(store.selectedPhase?.governance, .reviewed)
        XCTAssertFalse(store.selectedPhase?.summary.contains("仍因缺") ?? true)
        let reviewedPhase = store.selectedPhase
        guard case .refused = store.perform(.addCandidateReview) else {
            return XCTFail("repeating review completion must not append duplicate evidence")
        }
        XCTAssertEqual(store.selectedPhase, reviewedPhase)
    }

    @MainActor
    func testS4RecoveryLoadsANewReviewWithoutEnablingTheCandidate() {
        let store = EvolutionReviewStore(scenario: .s4SupervisionFailure)
        store.setTrack(.capabilityImprovement)
        let bindingsBefore = store.bindings
        let evidenceBefore = store.selectedPhase?.evidence ?? []
        XCTAssertTrue(store.sample.supervision.isFailing)
        XCTAssertNil(store.blockedReason(for: .recoverSupervision))
        XCTAssertNotNil(store.blockedReason(for: .addCandidateReview),
                        "a supervision failure must not be bypassed with a substitute review")
        let failedPhase = store.selectedPhase
        guard case .refused = store.perform(.addCandidateReview) else {
            return XCTFail("S4 must recover before loading completed review evidence")
        }
        XCTAssertEqual(store.selectedPhase, failedPhase)
        store.perform(.recoverSupervision)
        XCTAssertEqual(store.sample.supervision, .recoveredPendingReview)
        XCTAssertEqual(store.selectedPhase?.candidates.first?.trial, .blocked,
                       "recovery must not silently enable a candidate")
        XCTAssertNotNil(store.blockedReason(for: .trialCandidate))
        XCTAssertEqual(store.bindings, bindingsBefore, "recovery must not rebind any role")
        XCTAssertEqual(store.selectedPhase?.candidates.first?.verification, .satisfied)
        XCTAssertEqual(store.selectedPhase?.candidates.first?.requiredReview, .missing)
        XCTAssertNil(store.selectedPhase?.candidates.first?.requiredReviewVersion)
        XCTAssertEqual(Array((store.selectedPhase?.evidence ?? []).prefix(evidenceBefore.count)), evidenceBefore)
        XCTAssertEqual(store.selectedPhase?.evidence.last?.version, "v2")
        XCTAssertTrue(store.selectedPhase?.evidence.last?.title.contains("未完成") ?? false)
        let recoveryPhase = store.selectedPhase
        guard case .refused = store.perform(.recoverSupervision) else {
            return XCTFail("repeating recovery must not add duplicate evidence")
        }
        XCTAssertEqual(store.selectedPhase, recoveryPhase)
        store.perform(.addCandidateReview)
        XCTAssertEqual(store.selectedPhase?.candidates.first?.requiredReviewVersion, "v2")
        XCTAssertEqual(store.selectedPhase?.candidates.first?.trial, .blocked,
                       "even completed review does not itself start a trial")
        XCTAssertEqual(store.selectedPhase?.governance, .reviewed)
        XCTAssertEqual(store.sample.supervision, .healthy)
        XCTAssertNil(store.blockedReason(for: .trialCandidate))
    }

    @MainActor
    func testS5KeepsHistoryAndScopesDependencies() {
        let store = EvolutionReviewStore(scenario: .s5LateFinding)
        guard let phase = store.selectedPhase else { return XCTFail("S5 must select a phase") }
        let evidenceBefore = phase.evidence.map(\.id)
        let opinionsBefore = phase.opinions.map(\.id)
        XCTAssertEqual(phase.state, .passed)

        XCTAssertNil(store.blockedReason(for: .reopenPhase))
        store.perform(.reopenPhase)

        guard let reopened = store.selectedPhase else { return XCTFail("S5 lost its phase") }
        XCTAssertEqual(reopened.state, .reopened)
        XCTAssertEqual(reopened.evidence.map(\.id), evidenceBefore,
                       "the original pass record and downstream evidence must be kept")
        XCTAssertEqual(reopened.opinions.map(\.id), opinionsBefore)
        XCTAssertEqual(reopened.dependencies.first { $0.id == "dep.export.integration" }?.state, .restricted)
        XCTAssertEqual(reopened.dependencies.first { $0.id == "dep.export.docs" }?.state, .continuing)
        XCTAssertEqual(reopened.dependencies.first { $0.id == "dep.export.worker" }?.state,
                       .executingUnsettled,
                       "without pause evidence the sample must show still-executing, not stopped")
        XCTAssertTrue(store.sample.preservationNote?.contains("保留") ?? false)
        XCTAssertEqual(reopened.productVersion, "v2.0")
        XCTAssertEqual(reopened.evidence, phase.evidence)
        XCTAssertEqual(reopened.opinions, phase.opinions)
        XCTAssertEqual(Array(reopened.requirements.prefix(phase.requirements.count)), phase.requirements)
        let remediationIDs = EvolutionReviewFixtures.remediationRequirementIDs
        XCTAssertEqual(Set(reopened.unsatisfiedRequirements.map(\.id)), remediationIDs)
        XCTAssertNil(store.blockedReason(for: .completeRemediation))
        XCTAssertNotNil(store.blockedReason(for: .releaseRestriction))
        guard case .refused = store.perform(.releaseRestriction) else {
            return XCTFail("S5 must not reuse the original v2.0 pass to release the new finding")
        }
        XCTAssertEqual(store.selectedPhase, reopened)

        store.perform(.completeRemediation)
        guard let remediated = store.selectedPhase else { return XCTFail("S5 lost the remediated phase") }
        XCTAssertEqual(remediated.productVersion, "v2.1")
        XCTAssertEqual(Array(remediated.evidence.prefix(phase.evidence.count)), phase.evidence)
        XCTAssertEqual(Array(remediated.requirements.prefix(phase.requirements.count)), phase.requirements)
        XCTAssertTrue(remediationIDs.allSatisfy { id in
            remediated.requirements.contains { $0.id == id && $0.satisfied }
        })
        let currentEvidence = remediated.evidence.filter { $0.version == "v2.1" }
        XCTAssertEqual(currentEvidence, EvolutionReviewFixtures.completedRemediationEvidence)
        XCTAssertEqual(Set(remediated.evidence.map(\.id)).count, remediated.evidence.count)
        for original in phase.opinions {
            guard var historical = remediated.opinions.first(where: { $0.id == original.id }) else {
                return XCTFail("S5 remediation must preserve the original finding")
            }
            XCTAssertNotNil(historical.resolutionNote)
            historical.resolutionNote = original.resolutionNote
            XCTAssertEqual(historical, original)
        }
        guard case .refused = store.perform(.completeRemediation) else {
            return XCTFail("repeating completed remediation must not append duplicate records")
        }
        XCTAssertEqual(store.selectedPhase, remediated)
        XCTAssertNil(store.blockedReason(for: .releaseRestriction))
        store.perform(.releaseRestriction)
        guard let released = store.selectedPhase else { return XCTFail("S5 lost its released phase") }
        XCTAssertEqual(released.state, .passed)
        XCTAssertEqual(released.dependencies.first { $0.id == "dep.export.integration" }?.state, .released)
        for id in ["dep.export.docs", "dep.export.worker"] {
            XCTAssertEqual(released.dependencies.first { $0.id == id }, phase.dependencies.first { $0.id == id })
        }
        XCTAssertEqual(released.evidence, remediated.evidence)
        XCTAssertNotNil(store.blockedReason(for: .reopenPhase), "a resolved finding is historical")
        guard case .refused = store.perform(.releaseRestriction) else {
            return XCTFail("repeating release must leave all sample records unchanged")
        }
        XCTAssertEqual(store.selectedPhase, released)
    }

    @MainActor
    func testS6ReleasesOnlyTheTargetedRestrictionAfterTargetedReview() {
        let store = EvolutionReviewStore(scenario: .s6RemediationAndDispute)
        guard let initial = store.selectedPhase else { return XCTFail("S6 must select the remediation phase") }
        XCTAssertEqual(initial.productVersion, "v2.1")
        XCTAssertTrue(initial.evidence.allSatisfy { $0.version == initial.productVersion })
        XCTAssertNotNil(store.blockedReason(for: .releaseRestriction),
                        "an unsatisfied remediation requirement must keep the restriction")
        XCTAssertNil(store.blockedReason(for: .completeRemediation))
        store.perform(.completeRemediation)
        XCTAssertNil(store.blockedReason(for: .releaseRestriction))
        guard let completed = store.selectedPhase else { return XCTFail("S6 lost its completed review") }
        XCTAssertEqual(Array(completed.evidence.prefix(initial.evidence.count)), initial.evidence)
        XCTAssertEqual(completed.evidence, EvolutionReviewFixtures.completedRemediationEvidence)
        XCTAssertNotNil(completed.opinions.first { $0.id == "op.remediation.targeted" }?.resolutionNote)

        var withUnrelatedRequirement = initial
        let unrelated = EvolutionReviewRequirement(id: "req.unrelated", title: "其他独立要求", satisfied: false)
        withUnrelatedRequirement.requirements.append(unrelated)
        let selectivelyCompleted = EvolutionReviewStore.completedRemediationPhase(from: withUnrelatedRequirement)
        XCTAssertEqual(selectivelyCompleted.requirements.first { $0.id == unrelated.id }, unrelated)
        XCTAssertNotNil(EvolutionReviewStore.restrictionReleaseReason(for: selectivelyCompleted))
        XCTAssertEqual(EvolutionReviewStore.completedRemediationPhase(from: completed), completed,
                       "the fixed remediation transition is idempotent")

        for id in EvolutionReviewFixtures.remediationRequirementIDs {
            var missingRequirement = completed
            missingRequirement.requirements.removeAll { $0.id == id }
            XCTAssertNotNil(EvolutionReviewStore.restrictionReleaseReason(for: missingRequirement), id)
            var pendingRequirement = completed
            if let index = pendingRequirement.requirements.firstIndex(where: { $0.id == id }) {
                pendingRequirement.requirements[index].satisfied = false
            }
            XCTAssertNotNil(EvolutionReviewStore.restrictionReleaseReason(for: pendingRequirement), id)
        }
        var missingReview = completed
        missingReview.evidence.removeAll { $0.id == EvolutionReviewFixtures.targetedRemediationReviewID }
        XCTAssertNotNil(EvolutionReviewStore.restrictionReleaseReason(for: missingReview))
        var staleReview = missingReview
        staleReview.evidence.append(EvolutionReviewEvidence(
            id: EvolutionReviewFixtures.targetedRemediationReviewID, title: "旧版定向复审",
            version: "v2.0", sourceRole: .governance, kind: .governance, body: "虚构旧版复审，不能用于 v2.1"
        ))
        XCTAssertNotNil(EvolutionReviewStore.restrictionReleaseReason(for: staleReview))
        var wrongSourceReview = missingReview
        wrongSourceReview.evidence.append(EvolutionReviewEvidence(
            id: EvolutionReviewFixtures.targetedRemediationReviewID, title: "非治理定向复审",
            version: "v2.1", sourceRole: .reflection, kind: .reflection, body: "虚构反思，不能替代治理定向复审"
        ))
        XCTAssertNotNil(EvolutionReviewStore.restrictionReleaseReason(for: wrongSourceReview))
        var unrelatedRestriction = completed
        unrelatedRestriction.dependencies = [
            EvolutionReviewDependency(id: "dep.unrelated", title: "另一问题的依赖",
                                      state: .restricted, reason: "与导出字段漏检无关")
        ]
        XCTAssertNotNil(EvolutionReviewStore.restrictionReleaseReason(for: unrelatedRestriction),
                        "another restricted dependency is not the target of this remediation")
        store.perform(.releaseRestriction)
        XCTAssertEqual(store.selectedPhase?.dependencies.first?.state, .released)
        XCTAssertEqual(store.selectedPhase?.dependencies.first?.reason.contains("仅解除对应限制") ?? false, true)
        XCTAssertEqual(store.selectedPhase?.state, .passed)
        XCTAssertFalse(store.selectedPhase?.title.contains("局部重开中") ?? true,
                       "a completed phase must not retain a current reopened-state claim in its title")
    }

    @MainActor
    func testS6DisputeSampleNeverSubstitutesForRequiredReview() {
        let store = EvolutionReviewStore(scenario: .s6RemediationAndDispute)
        XCTAssertNotNil(store.sample.disputeNote)
        XCTAssertNotNil(store.detailPayload(for: .dispute))
        store.setTrack(.capabilityImprovement)
        XCTAssertNotNil(store.blockedReason(for: .trialCandidate),
                        "viewing the Q24 dispute sample must not enable a candidate")
    }

    @MainActor
    func testS7StylePreferenceAndUnevidencedVotesDoNotBlockAPhase() {
        let store = EvolutionReviewStore(scenario: .s7VersionMismatchAndStyle)
        guard let phase = store.selectedPhase else { return XCTFail("S7 must select a phase") }
        XCTAssertEqual(phase.state, .passed)
        let style = phase.opinions.first { $0.isStylePreference }
        XCTAssertNotNil(style)
        XCTAssertEqual(style?.severity, .info)
        XCTAssertEqual(phase.opinions.first { $0.id == "op.vote.consensus" }?.hasEvidence, false)
        XCTAssertTrue(phase.unsatisfiedRequirements.isEmpty)
    }

    @MainActor
    func testS8DistinguishesToolFailureFromServiceFailure() {
        let store = EvolutionReviewStore(scenario: .s8ToolTrialFailure)
        store.setTrack(.capabilityImprovement)
        guard let trialBefore = store.selectedPhase else { return XCTFail("S8 must expose the trial phase") }
        let deliveryBefore = store.phase(id: "phase.delivery")
        XCTAssertEqual(trialBefore.state, .passed)
        XCTAssertEqual(trialBefore.governance, .reviewed)
        XCTAssertEqual(trialBefore.candidates.first?.trial, .trialing)
        XCTAssertEqual(trialBefore.candidates.first?.verification, .satisfied)
        XCTAssertTrue(trialBefore.opinions.isEmpty, "the pre-failure snapshot must not contain a future fault")
        XCTAssertFalse(trialBefore.evidence.contains { $0.id.hasPrefix("ev.trial.failure.") })
        XCTAssertTrue(trialBefore.summary.contains("尚未观察到运行故障"))
        let candidateTarget = EvolutionReviewDetailTarget.candidate(phaseID: "phase.trial", candidateID: "cand.export.v2")
        XCTAssertTrue(store.detailPayload(for: candidateTarget)?.body.contains("尚未观察到运行故障") ?? false)
        XCTAssertEqual(store.sample.supervision, .healthy,
                       "a tool failure is not a supervision service failure")
        XCTAssertNil(store.blockedReason(for: .trialFailureFallback))
        store.perform(.trialFailureFallback)
        guard let candidate = store.selectedPhase?.candidates.first else {
            return XCTFail("S8 must keep its candidate")
        }
        XCTAssertEqual(candidate.trial, .fallbackActive)
        XCTAssertEqual(candidate.verification, .failed)
        XCTAssertNotNil(candidate.fallbackTool)
        XCTAssertTrue(store.sample.preservationNote?.contains("项目成果保留") ?? false)
        XCTAssertFalse(store.notice.contains("已撤销"), "the sample must not claim side effects were undone")
        XCTAssertEqual(store.selectedPhase?.state, .pendingRemediation)
        XCTAssertEqual(store.selectedPhase?.governance, .blockingCandidate)
        XCTAssertEqual(store.selectedPhase?.requirements.first { $0.id == "req.trial.verify" }?.satisfied, false)
        XCTAssertEqual(Array((store.selectedPhase?.evidence ?? []).prefix(trialBefore.evidence.count)),
                       trialBefore.evidence, "the original successful verification must survive the runtime fault")
        XCTAssertEqual(store.phase(id: "phase.delivery"), deliveryBefore)
        XCTAssertEqual(store.sample.supervision, .healthy)
        XCTAssertNotNil(store.blockedReason(for: .trialCandidate))
        let candidateDetail = store.detailPayload(for: candidateTarget)
        XCTAssertFalse(candidateDetail?.body.contains("尚未观察到运行故障") ?? true)
        XCTAssertTrue(candidateDetail?.body.contains("已停用") ?? false)
        XCTAssertTrue(candidateDetail?.body.contains("v1（获准安全工具） 已生效") ?? false)
        XCTAssertEqual(candidateDetail?.fields.first { $0.name == "试用状态" }?.value,
                       EvolutionReviewTrialState.fallbackActive.title)
        let faultDetail = store.detailPayload(for: .evidence(phaseID: "phase.trial", evidenceID: "ev.trial.failure.v2"))
        XCTAssertTrue(faultDetail?.body.contains("安全工具已生效") ?? false)
        XCTAssertEqual(faultDetail?.fields.first { $0.id == "版本" }?.value, "v2")
        let faultOpinion = store.detailPayload(for: .opinion(phaseID: "phase.trial", opinionID: "op.trial.failure"))
        XCTAssertEqual(faultOpinion?.fields.first { $0.id == "状态" }?.value, "当前意见")
        XCTAssertTrue(faultOpinion?.body.contains("已停用") ?? false)
        let failedPhase = store.selectedPhase
        guard case .refused = store.perform(.trialFailureFallback) else {
            return XCTFail("repeating a fault transition must not duplicate opinions or evidence")
        }
        XCTAssertEqual(store.selectedPhase, failedPhase)

        let service = EvolutionReviewStore(scenario: .s4SupervisionFailure)
        XCTAssertEqual(service.sample.supervision, .failed)
        XCTAssertNil(service.sample.preservationNote)
        let servicePhase = service.selectedPhase
        guard case .refused = service.perform(.trialFailureFallback) else {
            return XCTFail("a supervision service failure is not a tool runtime fault")
        }
        XCTAssertEqual(service.selectedPhase, servicePhase)
        XCTAssertEqual(service.selectedPhase?.candidates.first?.verification, .satisfied)
        XCTAssertEqual(service.selectedPhase?.candidates.first?.trial, .blocked)
    }

    // MARK: - No forced override

    @MainActor
    func testNoScenarioOffersAForcedPassOrIgnoreAllAction() {
        let banned = ["强制通过", "忽略所有", "跳过评审", "免审"]
        for action in EvolutionReviewAction.allCases {
            for word in banned {
                XCTAssertFalse(action.title.contains(word), "\(action.rawValue): \(action.title)")
            }
        }
        for scenario in EvolutionReviewScenarioID.allCases {
            let sample = EvolutionReviewFixtures.sample(for: scenario)
            XCTAssertFalse(sample.hasForcedOverrideAction, scenario.code)
            XCTAssertFalse(sample.phases.isEmpty, scenario.code)
        }
    }

    @MainActor
    func testEveryBlockedActionExplainsItselfInReadableChinese() {
        for scenario in EvolutionReviewScenarioID.allCases {
            let store = EvolutionReviewStore(scenario: scenario)
            for track in EvolutionReviewTrack.allCases {
                store.setTrack(track)
                for phase in store.phases {
                    store.selectPhase(phase.id)
                    for action in EvolutionReviewAction.allCases {
                        guard let reason = store.blockedReason(for: action) else { continue }
                        XCTAssertFalse(reason.isEmpty, "\(scenario.code)/\(action.rawValue)")
                        XCTAssertFalse(reason.contains("nil"), "\(scenario.code)/\(action.rawValue)")
                    }
                }
            }
        }
    }

    // MARK: - Memory reset

    @MainActor
    func testResetAndScenarioSwitchRestoreTheFixtures() {
        let store = EvolutionReviewStore(scenario: .s5LateFinding)
        store.perform(.reopenPhase)
        XCTAssertEqual(store.selectedPhase?.state, .reopened)
        store.beginRoleConfiguration()
        for role in EvolutionReviewRole.allCases {
            let template = store.template(for: role)
            store.setDraftMode(.overriding, for: role)
            store.setDraftConnection(template.connectionID == "conn.a" ? "conn.b" : "conn.a", for: role)
            store.setDraftModel(template.modelID == "model.1" ? "model.2" : "model.1", for: role)
        }
        store.applyDraft()
        XCTAssertTrue(EvolutionReviewRole.allCases.allSatisfy { store.binding(for: $0).mode == .overriding })
        store.beginRoleConfiguration()
        store.restoreInheritanceInDraft(for: .mainExecutor)
        store.openDetail(.opinion(phaseID: "phase.delivery", opinionID: "op.late.finding"))
        store.inspectorVisible = true
        store.reset()
        XCTAssertEqual(store.selectedPhase?.state, .passed,
                       "reset must restore the fixture, not keep demo transitions")
        XCTAssertNil(store.detail)
        XCTAssertNil(store.draft)
        XCTAssertFalse(store.inspectorVisible)
        for role in EvolutionReviewRole.allCases {
            XCTAssertEqual(store.binding(for: role), .inheriting)
            XCTAssertEqual(store.bindingSummary(for: role), "继承模板 · \(store.templateSummary(for: role))")
        }

        store.beginRoleConfiguration()
        for role in EvolutionReviewRole.allCases {
            store.setDraftMode(.overriding, for: role)
            store.setDraftConnection("conn.b", for: role)
            store.setDraftModel("model.1", for: role)
        }
        store.applyDraft()
        store.beginRoleConfiguration()
        store.setDraftModel("model.2", for: .mainExecutor)
        store.openDetail(.evidence(phaseID: "phase.delivery", evidenceID: "ev.delivery.pass.v20"))
        store.inspectorVisible = true
        store.load(.s3CandidateMissingReview)
        XCTAssertEqual(store.scenario, .s3CandidateMissingReview)
        XCTAssertNil(store.draft)
        XCTAssertNil(store.detail)
        XCTAssertFalse(store.inspectorVisible)
        XCTAssertTrue(store.notice.contains("覆盖样例状态与角色绑定"))
        for role in EvolutionReviewRole.allCases {
            XCTAssertEqual(store.binding(for: role), .inheriting)
            XCTAssertEqual(store.bindingSummary(for: role), "继承模板 · \(store.templateSummary(for: role))")
        }
        // The track selection survives a scenario switch when the new sample
        // has a phase in that track; the candidate lives on the capability
        // track, so select it explicitly.
        store.setTrack(.capabilityImprovement)
        XCTAssertEqual(store.selectedPhase?.candidates.first?.requiredReview, .missing)
    }

    @MainActor
    func testAFreshStoreMatchesTheFixtureAndKeepsNoFormDraft() {
        let first = EvolutionReviewStore(scenario: .s2OrdinaryPassedGovernancePending)
        first.beginRoleConfiguration()
        first.setDraftMode(.overriding, for: .mainExecutor)
        first.applyDraft()
        XCTAssertNotEqual(first.bindingSummary(for: .mainExecutor),
                          EvolutionReviewStore().bindingSummary(for: .mainExecutor))

        let second = EvolutionReviewStore(scenario: .s2OrdinaryPassedGovernancePending)
        XCTAssertNil(second.draft)
        XCTAssertEqual(second.bindingSummary(for: .mainExecutor),
                       "继承模板 · \(second.templateSummary(for: .mainExecutor))",
                       "a new window must not inherit another window's form changes")
    }

    @MainActor
    func testTrackSwitchSeparatesProjectDeliveryFromCapabilityImprovement() {
        let store = EvolutionReviewStore(scenario: .s1InterfacePending)
        XCTAssertEqual(store.track, .projectDelivery)
        XCTAssertTrue(store.phases.allSatisfy { $0.track == .projectDelivery })
        store.setTrack(.capabilityImprovement)
        XCTAssertTrue(store.phases.allSatisfy { $0.track == .capabilityImprovement })
        XCTAssertFalse(store.phases.isEmpty)
        XCTAssertEqual(store.selectedPhase?.track, .capabilityImprovement)

        for scenario in EvolutionReviewScenarioID.allCases {
            let initial = EvolutionReviewStore(scenario: scenario)
            XCTAssertNotNil(initial.selectedPhase, scenario.code)
            XCTAssertEqual(initial.selectedPhase?.track, initial.track, scenario.code)
            XCTAssertTrue(initial.phases.contains { $0.id == initial.selectedPhaseID }, scenario.code)
            let selectedID = initial.selectedPhaseID
            initial.selectPhase("phase.not.in.the.sample")
            XCTAssertEqual(initial.selectedPhaseID, selectedID, scenario.code)
            if let otherTrackPhase = initial.sample.phases.first(where: { $0.track != initial.track }) {
                initial.selectPhase(otherTrackPhase.id)
                XCTAssertEqual(initial.selectedPhaseID, selectedID, "cross-track selection must be rejected")
            }
        }

        let service = EvolutionReviewStore(scenario: .s4SupervisionFailure)
        XCTAssertEqual(service.track, .capabilityImprovement)
        service.openDetail(.opinion(phaseID: "phase.tool", opinionID: "op.supervision.failed"))
        service.inspectorVisible = true
        service.setTrack(.projectDelivery)
        XCTAssertTrue(service.phases.isEmpty)
        XCTAssertNil(service.selectedPhaseID)
        XCTAssertNil(service.selectedPhase)
        XCTAssertNil(service.detail)
        XCTAssertFalse(service.inspectorVisible)
        service.selectPhase("phase.tool")
        XCTAssertNil(service.selectedPhaseID, "the empty project track must reject a capability phase")
        // The native selection binding can write an ID directly; the derived
        // phase and action boundary must still enforce the current track.
        service.selectedPhaseID = "phase.tool"
        XCTAssertNil(service.selectedPhase)
        let before = service.sample.phases
        for action in EvolutionReviewAction.allCases {
            XCTAssertNotNil(service.blockedReason(for: action), action.rawValue)
            guard case .refused = service.perform(action) else {
                return XCTFail("every phase action, including supervision recovery, needs a phase in the current track")
            }
            XCTAssertEqual(service.sample.phases, before, action.rawValue)
            XCTAssertEqual(service.sample.supervision, .failed, action.rawValue)
        }
        service.setTrack(.projectDelivery)
        XCTAssertNil(service.selectedPhaseID, "even a repeated empty-track selection clears a stale ID")
        service.setTrack(.capabilityImprovement)
        XCTAssertEqual(service.selectedPhaseID, "phase.tool")
        XCTAssertEqual(service.selectedPhase?.track, service.track)
        XCTAssertNil(service.blockedReason(for: .recoverSupervision))
    }

    // MARK: - Assembly (no window, no client, no host)

    @MainActor
    func testSampleViewAssemblesWithoutAWindowOrClient() {
        let host = NSHostingView(rootView: NativeEvolutionReviewView())
        XCTAssertGreaterThanOrEqual(host.fittingSize.width, 380)
        XCTAssertGreaterThan(host.fittingSize.height, 0)
        let reduced = EvolutionReviewMotionPolicy.transaction(reduceMotion: true)
        XCTAssertTrue(reduced.disablesAnimations)
        XCTAssertNil(reduced.animation)
        let normal = EvolutionReviewMotionPolicy.transaction(reduceMotion: false)
        XCTAssertFalse(normal.disablesAnimations)
        XCTAssertNotNil(normal.animation)
    }

    @MainActor
    func testRefusalViewAssemblesForEveryReason() {
        for reason in EvolutionReviewRefusalReason.allCases {
            let host = NSHostingView(rootView: NativeEvolutionReviewRefusalView(reasons: [reason]))
            XCTAssertGreaterThanOrEqual(host.fittingSize.width, 420)
            XCTAssertGreaterThan(host.fittingSize.height, 0)
        }
    }

    @MainActor
    func testDetailPayloadsAreReadOnlyAndMarkedFictional() {
        let store = EvolutionReviewStore(scenario: .s5LateFinding)
        guard let phase = store.selectedPhase else { return XCTFail("S5 must select a phase") }
        let targets: [EvolutionReviewDetailTarget] = [
            .opinion(phaseID: phase.id, opinionID: phase.opinions[0].id),
            .evidence(phaseID: phase.id, evidenceID: phase.evidence[0].id),
            .dependency(phaseID: phase.id, dependencyID: phase.dependencies[0].id),
        ]
        for target in targets {
            guard let detail = store.detailPayload(for: target) else {
                return XCTFail("missing detail for \(target.identifierComponent)")
            }
            XCTAssertFalse(detail.title.isEmpty)
            XCTAssertTrue(detail.fictionalMarker.contains("虚构"), target.identifierComponent)
        }
        XCTAssertNil(store.detailPayload(for: .dispute),
                     "S5 has no dispute sample; the button must not be offered")
        let interface = EvolutionReviewStore(scenario: .s1InterfacePending)
        guard let longEvidence = interface.detailPayload(for: .evidence(
            phaseID: "phase.interface", evidenceID: "ev.interface.draft"
        )) else { return XCTFail("S1 must retain its embedded interface evidence ID") }
        let lines = longEvidence.body.split(separator: "\n").map(String.init)
        XCTAssertGreaterThanOrEqual(lines.count, 24)
        XCTAssertTrue(lines.first?.hasPrefix("虚构接口草案 v0.3") ?? false)
        XCTAssertEqual(lines.last, "内嵌证据末段：本页全部为虚构接口说明，未读取本机文件，未执行工具。")
        XCTAssertTrue(longEvidence.fictionalMarker.contains("虚构"))
        XCTAssertEqual(longEvidence.fields.first { $0.id == "版本" }?.value, "v0.3")
    }

    // MARK: - Helpers

    private func isolatedDirectory() -> String {
        NSHomeDirectory() + "/Library/Caches/aeroric-native/evolution-unit-\(UUID().uuidString)"
    }

    private var reviewEnvironment: [String: String] {
        [
            NativeEvolutionReviewEntry.flagKey: "1",
            NativeEvolutionReviewEntry.environmentKey: "ui-test",
            NativeEvolutionReviewEntry.dataDirectoryKey: isolatedDirectory(),
        ]
    }
}
#endif
