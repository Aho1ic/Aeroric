#if DEBUG
import Foundation

// Task-evolution review sample — value types (Q26; execution package
// `e3836e62:docs/native/task-evolution-review-stage-01.md` §3).
//
// These are SAMPLE types for an offline, in-memory review window. They are not
// a production protocol: nothing here is exported to `AeroricProtocol` or the
// Rust core, no wire enum is added, and no field is persisted. The real
// task-evolution contract (phase boundaries, trusted completion events,
// evidence sufficiency, governance IPC) is still undecided — see
// `e3836e62:docs/native/task-evolution-design-checkpoint-01.md` §8 — so every name
// carries `EvolutionReview`/`Review` and every fixture is explicitly fictional.
//
// The whole file is compiled out of ReleaseLocal.

// MARK: - Scenario

/// The eight offline scenarios the sample must be able to demonstrate
/// (execution package §3 table S1–S8).
public enum EvolutionReviewScenarioID: String, CaseIterable, Identifiable, Sendable {
    case s1InterfacePending
    case s2OrdinaryPassedGovernancePending
    case s3CandidateMissingReview
    case s4SupervisionFailure
    case s5LateFinding
    case s6RemediationAndDispute
    case s7VersionMismatchAndStyle
    case s8ToolTrialFailure

    public var id: String { rawValue }

    public var code: String {
        switch self {
        case .s1InterfacePending: "S1"
        case .s2OrdinaryPassedGovernancePending: "S2"
        case .s3CandidateMissingReview: "S3"
        case .s4SupervisionFailure: "S4"
        case .s5LateFinding: "S5"
        case .s6RemediationAndDispute: "S6"
        case .s7VersionMismatchAndStyle: "S7"
        case .s8ToolTrialFailure: "S8"
        }
    }

    public var title: String {
        switch self {
        case .s1InterfacePending: "接口待核对 / 待整改"
        case .s2OrdinaryPassedGovernancePending: "普通阶段通过、治理待审"
        case .s3CandidateMissingReview: "工具候选缺必要评审"
        case .s4SupervisionFailure: "监督 API 失败 / 超时"
        case .s5LateFinding: "迟到实质漏检"
        case .s6RemediationAndDispute: "漏检整改 / 争议"
        case .s7VersionMismatchAndStyle: "版本不匹配 / 风格建议"
        case .s8ToolTrialFailure: "工具试用运行故障"
        }
    }

    public var menuTitle: String { "\(code) \(title)" }
}

// MARK: - Roles, connections, models

/// The three roles of the confirmed model (checkpoint Q13–Q16). The sample
/// never claims a governance process is running: the role summary states the
/// product requirement and that this sample starts no process.
public enum EvolutionReviewRole: String, CaseIterable, Identifiable, Sendable {
    case mainExecutor
    case reflection
    case governance

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .mainExecutor: "主执行"
        case .reflection: "反思"
        case .governance: "治理"
        }
    }

    /// Stable ASCII component for accessibility identifiers.
    public var identifierComponent: String {
        switch self {
        case .mainExecutor: "mainExecutor"
        case .reflection: "reflection"
        case .governance: "governance"
        }
    }

    /// What the role may do, per checkpoint Q16 — shown so the sample cannot be
    /// read as "three interchangeable API labels".
    public var authorityNote: String {
        switch self {
        case .mainExecutor: "授权内写工作区 / 执行命令"
        case .reflection: "只读获准证据，产意见"
        case .governance: "只读获准证据，产意见或补丁草案"
        }
    }
}

/// Fictional connection. No key, no URL, no network probe.
public struct EvolutionReviewConnection: Identifiable, Hashable, Sendable {
    public let id: String
    public let name: String

    public init(id: String, name: String) {
        self.id = id
        self.name = name
    }
}

/// Fictional model. No capability is inferred from the name.
public struct EvolutionReviewModelOption: Identifiable, Hashable, Sendable {
    public let id: String
    public let name: String

    public init(id: String, name: String) {
        self.id = id
        self.name = name
    }
}

/// One row of the read-only sample global three-role template (Q14).
public struct EvolutionReviewRoleTemplate: Equatable, Sendable {
    public let role: EvolutionReviewRole
    public let connectionID: String
    public let modelID: String

    public init(role: EvolutionReviewRole, connectionID: String, modelID: String) {
        self.role = role
        self.connectionID = connectionID
        self.modelID = modelID
    }
}

public enum EvolutionReviewBindingMode: String, CaseIterable, Identifiable, Sendable {
    case inheriting
    case overriding

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .inheriting: "继承模板"
        case .overriding: "覆盖"
        }
    }
}

/// One role's binding for the current sample task. Inheriting resolves to the
/// template; overriding changes only this role (Q14: rebinding the main
/// executor never implicitly rebinds reflection or governance).
public struct EvolutionReviewRoleBinding: Equatable, Sendable {
    public var mode: EvolutionReviewBindingMode
    public var overrideConnectionID: String?
    public var overrideModelID: String?

    public init(mode: EvolutionReviewBindingMode,
                overrideConnectionID: String? = nil,
                overrideModelID: String? = nil) {
        self.mode = mode
        self.overrideConnectionID = overrideConnectionID
        self.overrideModelID = overrideModelID
    }

    public static let inheriting = EvolutionReviewRoleBinding(mode: .inheriting)

    public func resolvedConnectionID(template: EvolutionReviewRoleTemplate) -> String {
        mode == .overriding ? (overrideConnectionID ?? template.connectionID) : template.connectionID
    }

    public func resolvedModelID(template: EvolutionReviewRoleTemplate) -> String {
        mode == .overriding ? (overrideModelID ?? template.modelID) : template.modelID
    }
}

/// Form draft. `nil` means no form is open; cancelling drops the draft without
/// touching the applied bindings.
public struct EvolutionReviewRoleDraft: Equatable, Sendable {
    public var bindings: [EvolutionReviewRole: EvolutionReviewRoleBinding]

    public init(bindings: [EvolutionReviewRole: EvolutionReviewRoleBinding]) {
        self.bindings = bindings
    }

    public subscript(role: EvolutionReviewRole) -> EvolutionReviewRoleBinding {
        get { bindings[role] ?? .inheriting }
        set { bindings[role] = newValue }
    }
}

// MARK: - Tracks and phase state

/// Project delivery and capability improvement are separate tracks (checkpoint
/// §2): swapping a model or fixing a project bug is never counted as tool
/// self-evolution.
public enum EvolutionReviewTrack: String, CaseIterable, Identifiable, Sendable {
    case projectDelivery
    case capabilityImprovement

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .projectDelivery: "项目交付"
        case .capabilityImprovement: "能力改进"
        }
    }

    public var note: String {
        switch self {
        case .projectDelivery: "主执行者改项目、跑获准验证"
        case .capabilityImprovement: "任务专属工具/执行逻辑候选"
        }
    }
}

public enum EvolutionReviewPhaseState: String, Sendable {
    case passed
    case pendingReflection
    case pendingRemediation
    case reopened

    public var title: String {
        switch self {
        case .passed: "已通过"
        case .pendingReflection: "待反思核对"
        case .pendingRemediation: "待整改"
        case .reopened: "局部重开"
        }
    }

    public var isClosed: Bool { self == .passed }
}

/// Governance is a separate axis from the ordinary phase gate (Q23): an
/// ordinary phase closes without waiting for a governance signature, and
/// "pending review" is never rendered as "passed" or "exempt".
public enum EvolutionReviewGovernanceState: String, Sendable {
    case notRequired
    case pendingReview
    case reviewed
    case blockingCandidate

    public var title: String {
        switch self {
        case .notRequired: "无需治理签字"
        case .pendingReview: "治理待审"
        case .reviewed: "治理已复审"
        case .blockingCandidate: "治理阻断相关候选"
        }
    }
}

/// Supervision channel health (Q19). A failure is recorded as failed/pending —
/// never as a pass, and never as a model veto.
public enum EvolutionReviewSupervisionState: String, Sendable {
    case healthy
    case failed
    case timedOut
    case recoveredPendingReview

    public var title: String {
        switch self {
        case .healthy: "监督通道正常（样例）"
        case .failed: "监督 API 调用失败"
        case .timedOut: "监督 API 超时"
        case .recoveredPendingReview: "已载入当前版本新评审示例 · 仍待审"
        }
    }

    public var isFailing: Bool { self == .failed || self == .timedOut }
}

// MARK: - Phase content

public struct EvolutionReviewRequirement: Identifiable, Hashable, Sendable {
    public let id: String
    public let title: String
    public var satisfied: Bool
    public var note: String?

    public init(id: String, title: String, satisfied: Bool, note: String? = nil) {
        self.id = id
        self.title = title
        self.satisfied = satisfied
        self.note = note
    }
}

public enum EvolutionReviewEvidenceKind: String, Sendable {
    case establishedVerification
    case reflection
    case governance
    case userDecision
    case artifact

    public var title: String {
        switch self {
        case .establishedVerification: "既定验证"
        case .reflection: "反思"
        case .governance: "治理"
        case .userDecision: "用户裁决"
        case .artifact: "产物"
        }
    }
}

/// Embedded, read-only, explicitly fictional evidence. The sample never reads
/// a real path, session, log or artifact for these.
public struct EvolutionReviewEvidence: Identifiable, Hashable, Sendable {
    public let id: String
    public let title: String
    public let version: String
    public let sourceRole: EvolutionReviewRole
    public let kind: EvolutionReviewEvidenceKind
    public let body: String

    public init(id: String,
                title: String,
                version: String,
                sourceRole: EvolutionReviewRole,
                kind: EvolutionReviewEvidenceKind,
                body: String) {
        self.id = id
        self.title = title
        self.version = version
        self.sourceRole = sourceRole
        self.kind = kind
        self.body = body
    }
}

public enum EvolutionReviewOpinionSourceKind: String, Sendable {
    case modelReflection
    case machineFeedback
    case userDecision

    public var title: String {
        switch self {
        case .modelReflection: "模型反思（样例）"
        case .machineFeedback: "机器来源示例 · 未真实投递"
        case .userDecision: "用户裁决（样例）"
        }
    }
}

public enum EvolutionReviewSeverity: String, Sendable {
    case info
    case warning
    case blocking

    public var title: String {
        switch self {
        case .info: "提示"
        case .warning: "需关注"
        case .blocking: "实质问题"
        }
    }
}

public struct EvolutionReviewOpinion: Identifiable, Hashable, Sendable {
    public let id: String
    public let title: String
    public let sourceRole: EvolutionReviewRole
    public let sourceKind: EvolutionReviewOpinionSourceKind
    public let severity: EvolutionReviewSeverity
    public let body: String
    public let requirementIDs: [String]
    public let impactScope: String
    /// Delivery is a separate fact from consumption and remediation: a sent or
    /// queued message never means the main executor stopped or fixed anything.
    /// `var` so a demo action can advance it without rewriting the record.
    public var deliveryState: String
    /// S7: a pure style preference never auto-blocks a phase.
    public let isStylePreference: Bool
    /// S7: a model title or vote without evidence does not decide a fact.
    public let hasEvidence: Bool
    /// Current disposition, separate from the immutable historical opinion.
    /// A resolved finding must not be treated as a fresh blocking opinion.
    public var resolutionNote: String?

    public init(id: String,
                title: String,
                sourceRole: EvolutionReviewRole,
                sourceKind: EvolutionReviewOpinionSourceKind,
                severity: EvolutionReviewSeverity,
                body: String,
                requirementIDs: [String],
                impactScope: String,
                deliveryState: String,
                isStylePreference: Bool = false,
                hasEvidence: Bool = true,
                resolutionNote: String? = nil) {
        self.id = id
        self.title = title
        self.sourceRole = sourceRole
        self.sourceKind = sourceKind
        self.severity = severity
        self.body = body
        self.requirementIDs = requirementIDs
        self.impactScope = impactScope
        self.deliveryState = deliveryState
        self.isStylePreference = isStylePreference
        self.hasEvidence = hasEvidence
        self.resolutionNote = resolutionNote
    }
}

/// Verification / review / approval gates (Q17, Q23). `staleVersion` is the
/// S7 case: a review bound to an older version cannot approve the current one.
public enum EvolutionReviewGateState: String, Sendable {
    case satisfied
    case failed
    case missing
    case staleVersion

    public var title: String {
        switch self {
        case .satisfied: "已满足"
        case .failed: "失败"
        case .missing: "缺失"
        case .staleVersion: "旧版本 · 不适用当前版本"
        }
    }

    public var isSatisfied: Bool { self == .satisfied }
}

public enum EvolutionReviewTrialState: String, Sendable {
    case blocked
    case enabled
    case trialing
    case disabledAfterFailure
    case fallbackActive

    public var title: String {
        switch self {
        case .blocked: "未启用"
        case .enabled: "已获准 · 可试用"
        case .trialing: "试用中（样例）"
        case .disabledAfterFailure: "已停用"
        case .fallbackActive: "已回退到获准安全工具"
        }
    }
}

public struct EvolutionReviewCandidate: Identifiable, Hashable, Sendable {
    public let id: String
    public let title: String
    public let toolName: String
    public let version: String
    public var verification: EvolutionReviewGateState
    public var requiredReview: EvolutionReviewGateState
    /// Version the existing review was written against, when one exists.
    public var requiredReviewVersion: String?
    public var approval: EvolutionReviewGateState
    public var trial: EvolutionReviewTrialState
    public var notes: [String]
    /// Approved safe tool the sample falls back to (S8). Never a project or
    /// external side-effect rollback.
    public let fallbackTool: String?

    public init(id: String,
                title: String,
                toolName: String,
                version: String,
                verification: EvolutionReviewGateState,
                requiredReview: EvolutionReviewGateState,
                requiredReviewVersion: String? = nil,
                approval: EvolutionReviewGateState,
                trial: EvolutionReviewTrialState,
                notes: [String],
                fallbackTool: String? = nil) {
        self.id = id
        self.title = title
        self.toolName = toolName
        self.version = version
        self.verification = verification
        self.requiredReview = requiredReview
        self.requiredReviewVersion = requiredReviewVersion
        self.approval = approval
        self.trial = trial
        self.notes = notes
        self.fallbackTool = fallbackTool
    }

    /// The single gate the sample enforces before a candidate may be trialled.
    public var trialGate: EvolutionReviewGateState {
        if !verification.isSatisfied { return verification }
        if !requiredReview.isSatisfied { return requiredReview }
        if let requiredReviewVersion, requiredReviewVersion != version { return .staleVersion }
        if !approval.isSatisfied { return approval }
        return .satisfied
    }

    public var isTriallable: Bool { trialGate.isSatisfied }
}

public enum EvolutionReviewDependencyState: String, Sendable {
    case waiting
    case continuing
    case restricted
    case released
    case executingUnsettled

    public var title: String {
        switch self {
        case .waiting: "等待该结论"
        case .continuing: "可继续"
        case .restricted: "受限"
        case .released: "已解除限制"
        case .executingUnsettled: "仍在执行 / 待收束"
        }
    }
}

public struct EvolutionReviewDependency: Identifiable, Hashable, Sendable {
    public let id: String
    public let title: String
    public var state: EvolutionReviewDependencyState
    public var reason: String

    public init(id: String, title: String, state: EvolutionReviewDependencyState, reason: String) {
        self.id = id
        self.title = title
        self.state = state
        self.reason = reason
    }
}

public struct EvolutionReviewPhase: Identifiable, Hashable, Sendable {
    public let id: String
    public let title: String
    public let track: EvolutionReviewTrack
    public var state: EvolutionReviewPhaseState
    public var governance: EvolutionReviewGovernanceState
    public var productVersion: String
    public var requirements: [EvolutionReviewRequirement]
    public var evidence: [EvolutionReviewEvidence]
    public var opinions: [EvolutionReviewOpinion]
    public var candidates: [EvolutionReviewCandidate]
    public var dependencies: [EvolutionReviewDependency]
    public var summary: String

    public init(id: String,
                title: String,
                track: EvolutionReviewTrack,
                state: EvolutionReviewPhaseState,
                governance: EvolutionReviewGovernanceState,
                productVersion: String,
                requirements: [EvolutionReviewRequirement],
                evidence: [EvolutionReviewEvidence],
                opinions: [EvolutionReviewOpinion],
                candidates: [EvolutionReviewCandidate],
                dependencies: [EvolutionReviewDependency],
                summary: String) {
        self.id = id
        self.title = title
        self.track = track
        self.state = state
        self.governance = governance
        self.productVersion = productVersion
        self.requirements = requirements
        self.evidence = evidence
        self.opinions = opinions
        self.candidates = candidates
        self.dependencies = dependencies
        self.summary = summary
    }

    public var unsatisfiedRequirements: [EvolutionReviewRequirement] {
        requirements.filter { !$0.satisfied }
    }
}

/// One scenario's whole in-memory sample.
public struct EvolutionReviewSample: Sendable {
    public var scenario: EvolutionReviewScenarioID
    public var taskName: String
    public var phases: [EvolutionReviewPhase]
    public var supervision: EvolutionReviewSupervisionState
    /// Product requirement for governance, stated as a requirement — never as
    /// a running service.
    public var governanceProcessNote: String
    /// What a "feedback sent" demo action does and does not prove.
    public var feedbackDeliveryNote: String
    /// Read-only Q24 user-adjudication sample (S6). It never substitutes for a
    /// candidate's required review.
    public var disputeNote: String?
    /// S8: project results are preserved; nothing is rolled back.
    public var preservationNote: String?
    /// Machine-readable guard for the sample: no scenario may offer a
    /// force-pass / ignore-all / business-stop action.
    public var hasForcedOverrideAction: Bool

    public init(scenario: EvolutionReviewScenarioID,
                taskName: String,
                phases: [EvolutionReviewPhase],
                supervision: EvolutionReviewSupervisionState,
                governanceProcessNote: String,
                feedbackDeliveryNote: String,
                disputeNote: String? = nil,
                preservationNote: String? = nil,
                hasForcedOverrideAction: Bool = false) {
        self.scenario = scenario
        self.taskName = taskName
        self.phases = phases
        self.supervision = supervision
        self.governanceProcessNote = governanceProcessNote
        self.feedbackDeliveryNote = feedbackDeliveryNote
        self.disputeNote = disputeNote
        self.preservationNote = preservationNote
        self.hasForcedOverrideAction = hasForcedOverrideAction
    }

    public func phase(id: String) -> EvolutionReviewPhase? {
        phases.first { $0.id == id }
    }

    public func phases(in track: EvolutionReviewTrack) -> [EvolutionReviewPhase] {
        phases.filter { $0.track == track }
    }
}

// MARK: - Detail targets

/// A read-only detail the user opened by clicking an opinion, evidence row,
/// candidate, dependency or the dispute sample.
public enum EvolutionReviewDetailTarget: Hashable, Sendable {
    case opinion(phaseID: String, opinionID: String)
    case evidence(phaseID: String, evidenceID: String)
    case candidate(phaseID: String, candidateID: String)
    case dependency(phaseID: String, dependencyID: String)
    case dispute

    public var identifierComponent: String {
        switch self {
        case let .opinion(phaseID, opinionID): "opinion.\(phaseID).\(opinionID)"
        case let .evidence(phaseID, evidenceID): "evidence.\(phaseID).\(evidenceID)"
        case let .candidate(phaseID, candidateID): "candidate.\(phaseID).\(candidateID)"
        case let .dependency(phaseID, dependencyID): "dependency.\(phaseID).\(dependencyID)"
        case .dispute: "dispute"
        }
    }
}

/// Rendered, read-only detail payload.
public struct EvolutionReviewDetail: Equatable, Sendable {
    public struct Field: Equatable, Sendable, Identifiable {
        public let id: String
        public let name: String
        public let value: String

        public init(name: String, value: String) {
            self.id = name
            self.name = name
            self.value = value
        }
    }

    public let title: String
    public let kind: String
    public let fields: [Field]
    public let body: String
    public let fictionalMarker: String

    public init(title: String, kind: String, fields: [Field], body: String,
                fictionalMarker: String = "虚构内嵌样例 · 只读") {
        self.title = title
        self.kind = kind
        self.fields = fields
        self.body = body
        self.fictionalMarker = fictionalMarker
    }
}

// MARK: - Demo actions

/// The finite set of local, in-memory demo transitions (execution package §3:
/// "可以用有限的本地状态转换演示上述路径，不要写通用调度器或自动依赖引擎").
public enum EvolutionReviewAction: String, CaseIterable, Identifiable, Sendable {
    case sendReflectionFeedback
    case advanceDependents
    case addCandidateReview
    case trialCandidate
    case recoverSupervision
    case reopenPhase
    case completeRemediation
    case releaseRestriction
    case trialFailureFallback

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .sendReflectionFeedback: "演示：送出反思反馈"
        case .advanceDependents: "演示：推进依赖"
        case .addCandidateReview: "演示：补入同版本必要评审与既有审批"
        case .trialCandidate: "演示：受控试用候选"
        case .recoverSupervision: "演示：载入当前版本新评审示例"
        case .reopenPhase: "演示：局部重开已关闭阶段"
        case .completeRemediation: "演示：载入新版整改与定向复审"
        case .releaseRestriction: "演示：解除对应限制"
        case .trialFailureFallback: "演示：候选故障停用并回退"
        }
    }

    public var identifierComponent: String { rawValue }
}

/// Result of a demo action: applied, or refused with a readable reason.
public enum EvolutionReviewActionResult: Equatable, Sendable {
    case applied(String)
    case refused(String)
}
#endif
