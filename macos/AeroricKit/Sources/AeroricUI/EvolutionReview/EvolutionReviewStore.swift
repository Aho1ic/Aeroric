#if DEBUG
import Foundation
import Observation

// Task-evolution review sample — in-memory store (Q26; execution package §3).
//
// The business owner of this window is this store, and only this window: it is
// not a new production source of truth. It holds the sample, the role form
// draft and the selection; it performs a finite set of local demo transitions
// and nothing else. There is no I/O, no timer, no network call, no
// `AeroricClient`, no host, no task, no PTY, no persistence — closing or
// resetting the window restores the fixtures.
//
// Compiled out of ReleaseLocal.

@MainActor
@Observable
public final class EvolutionReviewStore {

    // MARK: - Sample state

    public private(set) var scenario: EvolutionReviewScenarioID
    public private(set) var sample: EvolutionReviewSample
    public private(set) var template: [EvolutionReviewRole: EvolutionReviewRoleTemplate]
    public private(set) var bindings: [EvolutionReviewRole: EvolutionReviewRoleBinding]

    /// Last user-visible consequence. Every demo action writes one, and every
    /// one of them says it only touched sample memory.
    public private(set) var notice: String
    public private(set) var lastAction: String = "无"

    public var selectedPhaseID: String?
    public var track: EvolutionReviewTrack = .projectDelivery
    public var detail: EvolutionReviewDetailTarget?
    /// Right inspector is closed by default (execution package §2.4).
    public var inspectorVisible: Bool = false

    /// Non-nil exactly while the role form is open. Cancelling drops it
    /// without touching the applied bindings.
    public var draft: EvolutionReviewRoleDraft?

    public static let sampleOnlyNotice = "仅本次样例，未保存真实配置"

    // MARK: - Init

    public init(scenario: EvolutionReviewScenarioID = .s1InterfacePending) {
        let sample = EvolutionReviewFixtures.sample(for: scenario)
        self.scenario = scenario
        self.sample = sample
        self.template = EvolutionReviewFixtures.roleTemplates
        self.bindings = Dictionary(
            uniqueKeysWithValues: EvolutionReviewRole.allCases.map { ($0, EvolutionReviewRoleBinding.inheriting) }
        )
        self.notice = "已载入示例场景 \(scenario.code)（覆盖的是样例状态，不写真实配置）"
        let firstPhase = sample.phases.first(where: { $0.track == .projectDelivery }) ?? sample.phases.first
        self.track = firstPhase?.track ?? .projectDelivery
        self.selectedPhaseID = firstPhase?.id
    }

    // MARK: - Derived

    public var phases: [EvolutionReviewPhase] {
        sample.phases.filter { $0.track == track }
    }

    public var selectedPhase: EvolutionReviewPhase? {
        guard let selectedPhaseID else { return nil }
        return sample.phases.first { $0.id == selectedPhaseID && $0.track == track }
    }

    public var governanceProcessNote: String { EvolutionReviewFixtures.governanceProcessNote }

    public func phase(id: String) -> EvolutionReviewPhase? { sample.phase(id: id) }

    public func template(for role: EvolutionReviewRole) -> EvolutionReviewRoleTemplate {
        template[role] ?? EvolutionReviewFixtures.template(for: role)
    }

    public func binding(for role: EvolutionReviewRole) -> EvolutionReviewRoleBinding {
        bindings[role] ?? .inheriting
    }

    public func resolvedConnectionName(for role: EvolutionReviewRole) -> String {
        EvolutionReviewFixtures.connectionName(binding(for: role).resolvedConnectionID(template: template(for: role)))
    }

    public func resolvedModelName(for role: EvolutionReviewRole) -> String {
        EvolutionReviewFixtures.modelName(binding(for: role).resolvedModelID(template: template(for: role)))
    }

    /// Readable binding summary for the role row and the AX leaf.
    public func bindingSummary(for role: EvolutionReviewRole) -> String {
        let binding = binding(for: role)
        let source = binding.mode == .inheriting ? "继承模板" : "覆盖"
        return "\(source) · \(resolvedConnectionName(for: role)) · \(resolvedModelName(for: role))"
    }

    public func templateSummary(for role: EvolutionReviewRole) -> String {
        let template = template(for: role)
        return "\(EvolutionReviewFixtures.connectionName(template.connectionID)) · \(EvolutionReviewFixtures.modelName(template.modelID))"
    }

    // MARK: - Sample lifecycle

    public func load(_ scenario: EvolutionReviewScenarioID) {
        self.scenario = scenario
        sample = EvolutionReviewFixtures.sample(for: scenario)
        bindings = Dictionary(
            uniqueKeysWithValues: EvolutionReviewRole.allCases.map { ($0, EvolutionReviewRoleBinding.inheriting) }
        )
        detail = nil
        inspectorVisible = false
        draft = nil
        let firstPhase = sample.phases.first(where: { $0.track == track }) ?? sample.phases.first
        selectedPhaseID = firstPhase?.id
        if firstPhase?.track != track {
            track = firstPhase?.track ?? .projectDelivery
        }
        notice = "已载入示例场景 \(scenario.code)（覆盖样例状态与角色绑定，不写真实配置）"
        lastAction = "载入 \(scenario.code)"
    }

    public func reset() {
        let scenario = self.scenario
        load(scenario)
        notice = "已重置样例状态与三角色继承模板（仅内存；关闭重开不保留表单改动）"
        lastAction = "重置样例"
    }

    public func setTrack(_ track: EvolutionReviewTrack) {
        self.track = track
        detail = nil
        inspectorVisible = false
        selectedPhaseID = sample.phases.first(where: { $0.track == track })?.id
    }

    public func selectPhase(_ id: String) {
        guard sample.phases.contains(where: { $0.id == id && $0.track == track }) else { return }
        selectedPhaseID = id
        detail = nil
        inspectorVisible = false
    }

    public func openDetail(_ target: EvolutionReviewDetailTarget) {
        detail = target
    }

    public func closeDetail() {
        detail = nil
        inspectorVisible = false
    }

    // MARK: - Role form

    public func beginRoleConfiguration() {
        draft = EvolutionReviewRoleDraft(bindings: bindings)
    }

    public func cancelDraft() {
        guard draft != nil else { return }
        draft = nil
        notice = "已取消角色配置草稿；未应用任何改动"
    }

    /// Applies the draft to this sample's memory only.
    public func applyDraft() {
        guard let draft else { return }
        bindings = draft.bindings
        self.draft = nil
        notice = Self.sampleOnlyNotice
        lastAction = "应用角色配置"
    }

    public func setDraftMode(_ mode: EvolutionReviewBindingMode, for role: EvolutionReviewRole) {
        guard var draft else { return }
        var binding = draft[role]
        binding.mode = mode
        if mode == .overriding {
            let template = template(for: role)
            if binding.overrideConnectionID == nil { binding.overrideConnectionID = template.connectionID }
            if binding.overrideModelID == nil { binding.overrideModelID = template.modelID }
        }
        draft[role] = binding
        self.draft = draft
    }

    public func setDraftConnection(_ id: String, for role: EvolutionReviewRole) {
        guard var draft else { return }
        var binding = draft[role]
        binding.overrideConnectionID = id
        draft[role] = binding
        self.draft = draft
    }

    public func setDraftModel(_ id: String, for role: EvolutionReviewRole) {
        guard var draft else { return }
        var binding = draft[role]
        binding.overrideModelID = id
        draft[role] = binding
        self.draft = draft
    }

    /// Restores one role to its template value inside the draft. Only that
    /// role changes (Q14).
    public func restoreInheritanceInDraft(for role: EvolutionReviewRole) {
        guard var draft else { return }
        draft[role] = .inheriting
        self.draft = draft
    }

    public func draftBinding(for role: EvolutionReviewRole) -> EvolutionReviewRoleBinding {
        draft?[role] ?? binding(for: role)
    }

    // MARK: - Gates

    /// Why a demo action is unavailable, in readable Chinese. A disabled action
    /// always shows this text instead of only greying out.
    public func blockedReason(for action: EvolutionReviewAction) -> String? {
        guard let phase = selectedPhase else { return "当前轨道未选择阶段" }
        switch action {
        case .sendReflectionFeedback:
            guard phase.opinions.contains(where: { $0.sourceKind == .machineFeedback }) else {
                return "当前阶段没有机器来源的反思反馈示例"
            }
            return nil

        case .advanceDependents:
            guard phase.state.isClosed else {
                return "阶段尚未满足既定验证与反思（当前：\(phase.state.title)），不能推进依赖"
            }
            guard phase.dependencies.contains(where: { $0.state == .waiting }) else {
                return "该阶段没有等待中的依赖"
            }
            return nil

        case .addCandidateReview:
            guard reviewableCandidate != nil else {
                return "当前阶段没有缺同版本必要评审的候选"
            }
            guard !sample.supervision.isFailing else {
                return "监督 API 失败或超时，先演示恢复并载入当前版本新评审，不能以自审替代"
            }
            return nil

        case .trialCandidate:
            guard let candidate = triallableCandidate else {
                return "当前阶段没有可试用的候选"
            }
            switch candidate.trial {
            case .trialing: return "候选已在试用中"
            case .disabledAfterFailure, .fallbackActive: return "候选已停用，需重新评审后才能启用"
            case .blocked, .enabled: break
            }
            return gateReason(for: candidate)

        case .recoverSupervision:
            guard sample.supervision.isFailing else {
                return "监督通道没有失败或超时记录"
            }
            return nil

        case .reopenPhase:
            guard phase.state.isClosed else { return "阶段未关闭，无需重开" }
            guard phase.opinions.contains(where: {
                $0.severity == .blocking && $0.hasEvidence && !$0.isStylePreference && $0.resolutionNote == nil
            }) else {
                return "没有有据的实质漏检，不能重开已关闭阶段"
            }
            return nil

        case .completeRemediation:
            guard phase.state == .reopened else { return "阶段未处于局部重开状态" }
            guard EvolutionReviewFixtures.remediationRequirementIDs.allSatisfy({ id in
                phase.requirements.contains { $0.id == id }
            }) else { return "缺少本样例原漏检问题的四项整改门槛" }
            let hasPendingRemediation = phase.requirements.contains {
                EvolutionReviewFixtures.remediationRequirementIDs.contains($0.id) && !$0.satisfied
            }
            guard hasPendingRemediation
                || phase.productVersion != EvolutionReviewFixtures.remediationVersion
                || !Self.hasCurrentTargetedReview(in: phase) else {
                return "整改要求已全部满足"
            }
            return nil

        case .releaseRestriction:
            return Self.restrictionReleaseReason(for: phase)

        case .trialFailureFallback:
            guard selectedPhase?.candidates.contains(where: { $0.trial == .trialing }) == true else {
                return "没有正在试用的候选"
            }
            return nil
        }
    }

    /// Readable reason for a candidate that cannot be trialled.
    public func gateReason(for candidate: EvolutionReviewCandidate) -> String? {
        switch candidate.trialGate {
        case .satisfied:
            return nil
        case .failed:
            return "候选受保护验证失败，不能试用"
        case .missing:
            return "缺少同版本必要评审或既有审批，不能试用"
        case .staleVersion:
            let version = candidate.requiredReviewVersion ?? "旧版本"
            return "现有评审针对 \(version)，不能批准当前版本 \(candidate.version)"
        }
    }

    public var reviewableCandidate: EvolutionReviewCandidate? {
        selectedPhase?.candidates.first { !$0.requiredReview.isSatisfied }
    }

    public var triallableCandidate: EvolutionReviewCandidate? {
        selectedPhase?.candidates.first
    }

    // MARK: - Demo transitions

    /// Finite local transitions only. No scheduler, no dependency engine, no
    /// automatic propagation beyond the rows the user can see.
    @discardableResult
    public func perform(_ action: EvolutionReviewAction) -> EvolutionReviewActionResult {
        if let reason = blockedReason(for: action) {
            notice = "\(action.title)未执行：\(reason)"
            lastAction = "\(action.title) · 被拒绝"
            return .refused(reason)
        }
        switch action {
        case .sendReflectionFeedback:
            return applySendFeedback()
        case .advanceDependents:
            return applyAdvanceDependents()
        case .addCandidateReview:
            return applyAddCandidateReview()
        case .trialCandidate:
            return applyTrialCandidate()
        case .recoverSupervision:
            return applyRecoverSupervision()
        case .reopenPhase:
            return applyReopenPhase()
        case .completeRemediation:
            return applyCompleteRemediation()
        case .releaseRestriction:
            return applyReleaseRestriction()
        case .trialFailureFallback:
            return applyTrialFailureFallback()
        }
    }

    private func applySendFeedback() -> EvolutionReviewActionResult {
        guard let phaseIndex = phaseIndex(of: selectedPhaseID) else {
            return .refused("未选择阶段")
        }
        for opinionIndex in sample.phases[phaseIndex].opinions.indices
        where sample.phases[phaseIndex].opinions[opinionIndex].sourceKind == .machineFeedback {
            sample.phases[phaseIndex].opinions[opinionIndex].deliveryState =
                "已送出（样例）· 未确认主执行已停止或已整改"
        }
        let message = "反思反馈已送出（样例）：送出 ≠ 消费 ≠ 整改 ≠ 验证"
        notice = message
        lastAction = "送出反思反馈"
        return .applied(message)
    }

    private func applyAdvanceDependents() -> EvolutionReviewActionResult {
        guard let phaseIndex = phaseIndex(of: selectedPhaseID) else {
            return .refused("未选择阶段")
        }
        for index in sample.phases[phaseIndex].dependencies.indices
        where sample.phases[phaseIndex].dependencies[index].state == .waiting {
            sample.phases[phaseIndex].dependencies[index].state = .released
            sample.phases[phaseIndex].dependencies[index].reason =
                "既定验证与反思已满足，阶段已关闭，可推进依赖；治理仍待审，不算通过或免审"
        }
        let message = "依赖已推进（样例）：普通阶段关闭不等待治理签字，治理仍显示待审"
        notice = message
        lastAction = "推进依赖"
        return .applied(message)
    }

    private func applyAddCandidateReview() -> EvolutionReviewActionResult {
        guard let phaseIndex = phaseIndex(of: selectedPhaseID),
              let candidateIndex = sample.phases[phaseIndex].candidates.firstIndex(where: {
                  !$0.requiredReview.isSatisfied
              }) else {
            return .refused("当前阶段没有缺同版本必要评审的候选")
        }
        let version = sample.phases[phaseIndex].candidates[candidateIndex].version
        sample.phases[phaseIndex].candidates[candidateIndex].requiredReview = .satisfied
        sample.phases[phaseIndex].candidates[candidateIndex].requiredReviewVersion = version
        sample.phases[phaseIndex].candidates[candidateIndex].approval = .satisfied
        sample.phases[phaseIndex].candidates[candidateIndex].notes = [
            "\(version) 的同版本必要评审与既有审批已满足（样例）。",
            "缺审或旧版本意见保留为历史；受控试用仍是独立的演示动作。",
        ]
        sample.phases[phaseIndex].evidence.append(
            EvolutionReviewEvidence(
                id: "ev.tool.review.\(version)",
                title: "必要评审记录",
                version: version,
                sourceRole: .governance,
                kind: .governance,
                body: "虚构评审记录：\(version) 的验证范围、回退路径与影响范围已确认。\n（样例文本，未真实投递或评审）"
            )
        )
        for requirementIndex in sample.phases[phaseIndex].requirements.indices
        where sample.phases[phaseIndex].requirements[requirementIndex].id == "req.tool.review"
            || sample.phases[phaseIndex].requirements[requirementIndex].id == "req.tool.approval" {
            sample.phases[phaseIndex].requirements[requirementIndex].satisfied = true
            sample.phases[phaseIndex].requirements[requirementIndex].note = "\(version) 的同版本记录已满足（样例）"
        }
        for opinionIndex in sample.phases[phaseIndex].opinions.indices
        where sample.phases[phaseIndex].opinions[opinionIndex].severity == .blocking
            && sample.phases[phaseIndex].opinions[opinionIndex].requirementIDs.contains("req.tool.review") {
            sample.phases[phaseIndex].opinions[opinionIndex].resolutionNote =
                "已补入 \(version) 的同版本必要评审与既有审批（样例）；原缺审意见保留为历史，不再阻断当前候选。"
        }
        sample.phases[phaseIndex].governance = .reviewed
        if sample.phases[phaseIndex].unsatisfiedRequirements.isEmpty {
            sample.phases[phaseIndex].state = .passed
        }
        sample.phases[phaseIndex].summary =
            "\(version) 的同版本必要评审与既有审批已满足；原缺审记录保留，候选仍需单独演示受控试用。"
        if sample.supervision == .recoveredPendingReview {
            sample.supervision = .healthy
        }
        let message = "已补入 \(version) 的同版本必要评审与既有审批（样例）"
        notice = message
        lastAction = "补入必要评审"
        return .applied(message)
    }

    private func applyTrialCandidate() -> EvolutionReviewActionResult {
        guard let phaseIndex = phaseIndex(of: selectedPhaseID),
              let candidateIndex = sample.phases[phaseIndex].candidates.firstIndex(where: {
                  $0.trial == .blocked || $0.trial == .enabled
              }) else {
            return .refused("当前阶段没有可试用的候选")
        }
        sample.phases[phaseIndex].candidates[candidateIndex].trial = .trialing
        let version = sample.phases[phaseIndex].candidates[candidateIndex].version
        sample.phases[phaseIndex].candidates[candidateIndex].notes = [
            "\(version) 受控试用中（样例）；验证、同版本必要评审与既有审批已满足。",
            "尚未观察到运行故障；不执行真实工具，不产生外部副作用。",
        ]
        sample.phases[phaseIndex].summary = "\(version) 受控试用中（样例）；不执行真实工具。"
        let message = "候选 \(version) 进入受控试用（样例）：不执行真实工具，不产生外部副作用"
        notice = message
        lastAction = "受控试用"
        return .applied(message)
    }

    private func applyRecoverSupervision() -> EvolutionReviewActionResult {
        sample.supervision = .recoveredPendingReview
        guard let phaseIndex = phaseIndex(of: selectedPhaseID),
              let candidateIndex = sample.phases[phaseIndex].candidates.firstIndex(where: {
                  $0.requiredReview == .staleVersion || $0.requiredReview == .missing
              }) else {
            let message = "已载入当前版本的新评审示例（样例）；候选仍需评审，未自动启用"
            notice = message
            lastAction = "载入新评审示例"
            return .applied(message)
        }
        let version = sample.phases[phaseIndex].candidates[candidateIndex].version
        sample.phases[phaseIndex].candidates[candidateIndex].requiredReview = .missing
        sample.phases[phaseIndex].candidates[candidateIndex].requiredReviewVersion = nil
        sample.phases[phaseIndex].candidates[candidateIndex].notes = [
            "已载入 \(version) 的新评审示例，必要评审尚未完成，候选未启用。",
            "不静默换模型、不以主执行者自审替代、不回退有效工具。",
        ]
        for index in sample.phases[phaseIndex].requirements.indices
        where sample.phases[phaseIndex].requirements[index].id == "req.tool.review" {
            sample.phases[phaseIndex].requirements[index].satisfied = false
            sample.phases[phaseIndex].requirements[index].note = "\(version) 新评审示例已载入，尚未完成"
        }
        for index in sample.phases[phaseIndex].opinions.indices
        where sample.phases[phaseIndex].opinions[index].id == "op.supervision.failed" {
            sample.phases[phaseIndex].opinions[index].resolutionNote =
                "已演示恢复并载入 \(version) 新评审示例；原服务故障保留为历史，候选必要评审仍待完成。"
        }
        sample.phases[phaseIndex].summary = "监督通道已演示恢复；\(version) 新评审仍待完成，候选保持未启用。"
        sample.phases[phaseIndex].evidence.append(
            EvolutionReviewEvidence(
                id: "ev.supervision.recovery.\(version)",
                title: "当前版本新评审示例（未完成）",
                version: version,
                sourceRole: .governance,
                kind: .governance,
                body: "虚构记录：监督通道恢复后只载入 \(version) 的新评审示例；不静默换模型、不以自审替代、不回退有效工具。\n（样例文本）"
            )
        )
        let message = "已载入 \(version) 的新评审示例（样例）；候选保持未启用，待真实评审"
        notice = message
        lastAction = "载入新评审示例"
        return .applied(message)
    }

    private func applyReopenPhase() -> EvolutionReviewActionResult {
        guard let phaseIndex = phaseIndex(of: selectedPhaseID) else {
            return .refused("未选择阶段")
        }
        sample.phases[phaseIndex].state = .reopened
        sample.phases[phaseIndex].governance = .pendingReview
        for requirement in EvolutionReviewFixtures.pendingRemediationRequirements {
            if let index = sample.phases[phaseIndex].requirements.firstIndex(where: { $0.id == requirement.id }) {
                sample.phases[phaseIndex].requirements[index].satisfied = false
                sample.phases[phaseIndex].requirements[index].note = requirement.note
            } else {
                sample.phases[phaseIndex].requirements.append(requirement)
            }
        }
        for index in sample.phases[phaseIndex].dependencies.indices
        where sample.phases[phaseIndex].dependencies[index].id == "dep.export.integration" {
            sample.phases[phaseIndex].dependencies[index].state = .restricted
            sample.phases[phaseIndex].dependencies[index].reason = "实际受影响的依赖链，需按新前提补验"
        }
        sample.phases[phaseIndex].summary =
            "原 v2.0 通过记录与下游成果保留；新增四项整改门槛，旧结论不能解除当前漏检限制。"
        let message = "阶段已局部重开（样例）：保留原通过记录与下游成果，只限制实际受影响依赖"
        notice = message
        lastAction = "局部重开"
        return .applied(message)
    }

    private func applyCompleteRemediation() -> EvolutionReviewActionResult {
        guard let phaseIndex = phaseIndex(of: selectedPhaseID) else {
            return .refused("未选择阶段")
        }
        sample.phases[phaseIndex] = Self.completedRemediationPhase(from: sample.phases[phaseIndex])
        let message = "已载入新版整改、反思、既定验证与原问题定向复审示例（样例）"
        notice = message
        lastAction = "载入整改与定向复审"
        return .applied(message)
    }

    private func applyReleaseRestriction() -> EvolutionReviewActionResult {
        guard let phaseIndex = phaseIndex(of: selectedPhaseID) else {
            return .refused("未选择阶段")
        }
        for index in sample.phases[phaseIndex].dependencies.indices
        where sample.phases[phaseIndex].dependencies[index].id == "dep.export.integration"
            && sample.phases[phaseIndex].dependencies[index].state == .restricted {
            sample.phases[phaseIndex].dependencies[index].state = .released
            sample.phases[phaseIndex].dependencies[index].reason =
                "整改、反思、既定验证及原问题定向复审已满足，仅解除对应限制"
        }
        sample.phases[phaseIndex].state = .passed
        sample.phases[phaseIndex].summary =
            "v2.1 整改链已满足，导出字段联调的对应限制已解除；其他依赖与历史成果保留。"
        let message = "已解除对应限制（样例）：只解除该问题对应的限制，未放宽其他依赖"
        notice = message
        lastAction = "解除对应限制"
        return .applied(message)
    }

    private func applyTrialFailureFallback() -> EvolutionReviewActionResult {
        guard let phaseIndex = phaseIndex(of: selectedPhaseID),
              let candidateIndex = sample.phases[phaseIndex].candidates.firstIndex(where: {
                  $0.trial == .trialing
              }) else {
            return .refused("没有正在试用的候选")
        }
        let candidate = sample.phases[phaseIndex].candidates[candidateIndex]
        let fallback = candidate.fallbackTool ?? "获准安全工具（样例）"
        sample.phases[phaseIndex].candidates[candidateIndex].trial = .fallbackActive
        sample.phases[phaseIndex].candidates[candidateIndex].verification = .failed
        sample.phases[phaseIndex].candidates[candidateIndex].notes = [
            "候选 \(candidate.version) 运行故障，已停用；\(fallback) 已生效（样例）。",
            "试用前通过验证保留为历史；项目成果保留，未撤销项目代码或外部副作用。",
        ]
        sample.phases[phaseIndex].state = .pendingRemediation
        sample.phases[phaseIndex].governance = .blockingCandidate
        sample.phases[phaseIndex].summary =
            "候选 \(candidate.version) 因运行故障已停用，\(fallback) 已生效；项目成果保留。"
        let verificationIDs = ["req.trial.verify", "req.tool.verify"]
        for index in sample.phases[phaseIndex].requirements.indices
        where verificationIDs.contains(sample.phases[phaseIndex].requirements[index].id) {
            sample.phases[phaseIndex].requirements[index].satisfied = false
            sample.phases[phaseIndex].requirements[index].note =
                "当前候选因运行故障不可用；试用前通过验证仍保留为历史，不能作为再次启用的依据"
        }
        sample.phases[phaseIndex].evidence.append(
            EvolutionReviewEvidence(
                id: "ev.trial.failure.\(candidate.version)",
                title: "试用故障与回退记录",
                version: candidate.version,
                sourceRole: .governance,
                kind: .governance,
                body: "虚构记录：候选 \(candidate.version) 运行故障，已停用并回退到 \(fallback)；安全工具已生效，项目成果保留，未撤销项目或外部副作用。\n（样例文本）"
            )
        )
        sample.phases[phaseIndex].opinions.append(
            EvolutionReviewOpinion(
                id: "op.trial.failure",
                title: "试用运行故障：候选 \(candidate.version) 失败",
                sourceRole: .governance,
                sourceKind: .machineFeedback,
                severity: .blocking,
                body: "虚构故障记录：候选 \(candidate.version) 已停用，\(fallback) 已生效；仅限制该候选，不撤销项目成果或外部副作用。API 服务故障本身不触发有效工具回退。",
                requirementIDs: sample.phases[phaseIndex].requirements.filter { verificationIDs.contains($0.id) }.map(\.id),
                impactScope: "仅该候选；项目成果保留",
                deliveryState: "故障记录已载入（样例）· 未真实投递"
            )
        )
        let message = "候选 \(candidate.version) 已停用并回退（样例）：项目成果保留，不声称撤销项目/外部副作用"
        notice = message
        lastAction = "故障停用并回退"
        return .applied(message)
    }

    // MARK: - Detail payloads

    public func detailPayload(for target: EvolutionReviewDetailTarget) -> EvolutionReviewDetail? {
        switch target {
        case .dispute:
            guard let dispute = sample.disputeNote else { return nil }
            return EvolutionReviewDetail(
                title: "Q24 用户争议裁决示例",
                kind: "用户裁决",
                fields: [
                    .init(name: "来源", value: "用户（样例）"),
                    .init(name: "约束", value: "不替代候选必需评审或既定验证"),
                ],
                body: dispute
            )

        case let .opinion(phaseID, opinionID):
            guard let opinion = phase(id: phaseID)?.opinions.first(where: { $0.id == opinionID }) else { return nil }
            var fields: [EvolutionReviewDetail.Field] = [
                .init(name: "状态", value: opinion.resolutionNote == nil ? "当前意见" : "历史意见（已解决）"),
                .init(name: "来源角色", value: opinion.sourceRole.title),
                .init(name: "来源类型", value: opinion.sourceKind.title),
                .init(name: "严重度", value: opinion.severity.title),
                .init(name: "引用要求", value: opinion.requirementIDs.isEmpty ? "无" : opinion.requirementIDs.joined(separator: "、")),
                .init(name: "影响范围", value: opinion.impactScope),
                .init(name: "投递状态", value: opinion.deliveryState),
                .init(name: "有据", value: opinion.hasEvidence ? "是" : "否（不决定事实）"),
            ]
            if opinion.isStylePreference {
                fields.append(.init(name: "风格偏好", value: "是（不自动阻断阶段）"))
            }
            if let resolution = opinion.resolutionNote {
                fields.append(.init(name: "处理说明", value: resolution))
            }
            let body = opinion.resolutionNote.map {
                "历史意见（已解决）\n原意见：\(opinion.body)\n当前处理：\($0)"
            } ?? opinion.body
            return EvolutionReviewDetail(
                title: opinion.title,
                kind: opinion.resolutionNote == nil ? "意见 / 问题" : "历史意见（已解决）",
                fields: fields,
                body: body
            )

        case let .evidence(phaseID, evidenceID):
            guard let evidence = phase(id: phaseID)?.evidence.first(where: { $0.id == evidenceID }) else { return nil }
            return EvolutionReviewDetail(
                title: evidence.title,
                kind: evidence.kind.title,
                fields: [
                    .init(name: "版本", value: evidence.version),
                    .init(name: "来源角色", value: evidence.sourceRole.title),
                ],
                body: evidence.body
            )

        case let .candidate(phaseID, candidateID):
            guard let candidate = phase(id: phaseID)?.candidates.first(where: { $0.id == candidateID }) else { return nil }
            var fields: [EvolutionReviewDetail.Field] = [
                .init(name: "工具", value: candidate.toolName),
                .init(name: "候选版本", value: candidate.version),
                .init(name: "受保护验证", value: candidate.verification.title),
                .init(name: "同版本必要评审", value: candidate.requiredReview.title),
                .init(name: "既有审批", value: candidate.approval.title),
                .init(name: "试用状态", value: candidate.trial.title),
            ]
            if let version = candidate.requiredReviewVersion {
                fields.append(.init(name: "评审版本", value: version))
            }
            if let fallback = candidate.fallbackTool {
                fields.append(.init(name: "回退工具", value: fallback))
            }
            if let reason = gateReason(for: candidate) {
                fields.append(.init(name: "不能试用的原因", value: reason))
            }
            return EvolutionReviewDetail(
                title: candidate.title,
                kind: "工具候选",
                fields: fields,
                body: candidate.notes.joined(separator: "\n")
            )

        case let .dependency(phaseID, dependencyID):
            guard let dependency = phase(id: phaseID)?.dependencies.first(where: { $0.id == dependencyID }) else { return nil }
            return EvolutionReviewDetail(
                title: dependency.title,
                kind: "依赖影响",
                fields: [.init(name: "状态", value: dependency.state.title)],
                body: dependency.reason
            )
        }
    }

    // MARK: - Helpers

    /// Fixed, pure S5/S6 transition. Only these four sample requirements are
    /// satisfied; unrelated future requirements and all old records survive.
    static func completedRemediationPhase(from original: EvolutionReviewPhase) -> EvolutionReviewPhase {
        var phase = original
        phase.productVersion = EvolutionReviewFixtures.remediationVersion
        for index in phase.requirements.indices
        where EvolutionReviewFixtures.remediationRequirementIDs.contains(phase.requirements[index].id) {
            phase.requirements[index].satisfied = true
            phase.requirements[index].note = "\(phase.productVersion) 的对应整改记录已满足（样例）"
        }
        for evidence in EvolutionReviewFixtures.completedRemediationEvidence
        where !phase.evidence.contains(where: { $0.id == evidence.id && $0.version == evidence.version }) {
            phase.evidence.append(evidence)
        }
        for index in phase.opinions.indices
        where ["op.late.finding", "op.remediation.targeted"].contains(phase.opinions[index].id) {
            phase.opinions[index].resolutionNote =
                "\(phase.productVersion) 整改、反思、既定验证与原问题定向复审已满足（样例）；原意见保留为历史。"
        }
        phase.governance = .reviewed
        phase.summary = "\(phase.productVersion) 的四项整改记录已满足；仅可解除导出字段联调的对应限制。"
        return phase
    }

    private static func hasCurrentTargetedReview(in phase: EvolutionReviewPhase) -> Bool {
        phase.evidence.contains {
            $0.id == EvolutionReviewFixtures.targetedRemediationReviewID
                && $0.version == phase.productVersion
                && $0.kind == .governance && $0.sourceRole == .governance
        }
    }

    static func restrictionReleaseReason(for phase: EvolutionReviewPhase) -> String? {
        guard phase.state == .reopened else { return "阶段未处于局部重开状态" }
        guard phase.unsatisfiedRequirements.isEmpty else {
            return "仍有未满足的整改要求：\(phase.unsatisfiedRequirements.map(\.title).joined(separator: "、"))"
        }
        guard EvolutionReviewFixtures.remediationRequirementIDs.allSatisfy({ id in
            phase.requirements.contains { $0.id == id && $0.satisfied }
        }) else { return "缺少原问题的新版整改、反思、既定验证或定向复审门槛" }
        guard phase.productVersion == EvolutionReviewFixtures.remediationVersion,
              hasCurrentTargetedReview(in: phase) else {
            return "缺少当前产物版本的原问题定向复审证据，不能解除限制"
        }
        guard phase.dependencies.contains(where: {
            $0.id == "dep.export.integration" && $0.state == .restricted
        }) else { return "没有该漏检对应的导出字段联调限制可解除" }
        return nil
    }

    private func phaseIndex(of id: String?) -> Int? {
        guard let id else { return nil }
        return sample.phases.firstIndex { $0.id == id }
    }
}
#endif
