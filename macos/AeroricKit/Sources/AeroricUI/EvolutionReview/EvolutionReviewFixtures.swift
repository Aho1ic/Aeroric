#if DEBUG
import Foundation

// Task-evolution review sample — embedded fictional fixtures (Q26; execution
// package §3).
//
// Everything below is invented for the review window: fictional connections
// ("示例连接A/B"), fictional models ("示例模型1/2"), fictional phases,
// evidence, opinions and tool candidates. The sample never reads a real
// connection, key, task, session, statistic, log or file path, and it never
// runs a timer to fake streaming, network traffic, token usage or a passing
// test log. Evidence bodies are literal strings in this file.
//
// Compiled out of ReleaseLocal.

public enum EvolutionReviewFixtures {

    // MARK: - Fictional connections and models

    public static let connections: [EvolutionReviewConnection] = [
        EvolutionReviewConnection(id: "conn.a", name: "示例连接A"),
        EvolutionReviewConnection(id: "conn.b", name: "示例连接B"),
    ]

    public static let models: [EvolutionReviewModelOption] = [
        EvolutionReviewModelOption(id: "model.1", name: "示例模型1"),
        EvolutionReviewModelOption(id: "model.2", name: "示例模型2"),
    ]

    /// Read-only sample global three-role template (Q14). The sample task may
    /// inherit a row or override it per role.
    public static let roleTemplates: [EvolutionReviewRole: EvolutionReviewRoleTemplate] = [
        .mainExecutor: EvolutionReviewRoleTemplate(role: .mainExecutor, connectionID: "conn.a", modelID: "model.1"),
        .reflection: EvolutionReviewRoleTemplate(role: .reflection, connectionID: "conn.a", modelID: "model.2"),
        .governance: EvolutionReviewRoleTemplate(role: .governance, connectionID: "conn.b", modelID: "model.2"),
    ]

    public static func template(for role: EvolutionReviewRole) -> EvolutionReviewRoleTemplate {
        roleTemplates[role] ?? EvolutionReviewRoleTemplate(role: role, connectionID: "conn.a", modelID: "model.1")
    }

    public static func connectionName(_ id: String) -> String {
        connections.first { $0.id == id }?.name ?? id
    }

    public static func modelName(_ id: String) -> String {
        models.first { $0.id == id }?.name ?? id
    }

    /// Stated wherever governance appears. The product requirement is a
    /// separate process; this sample starts none, and three API labels are not
    /// three processes.
    public static let governanceProcessNote =
        "独立进程为产品设计要求；本样例未启动进程。三个角色标签不代表三个独立进程。"

    public static let feedbackDeliveryNote =
        "反馈已送出（样例）· 未确认主执行已停止或已整改；送出、入队、消费、整改、验证是不同事实。"

    // MARK: - Scenario samples

    public static func sample(for scenario: EvolutionReviewScenarioID) -> EvolutionReviewSample {
        switch scenario {
        case .s1InterfacePending: s1()
        case .s2OrdinaryPassedGovernancePending: s2()
        case .s3CandidateMissingReview: s3()
        case .s4SupervisionFailure: s4()
        case .s5LateFinding: s5()
        case .s6RemediationAndDispute: s6()
        case .s7VersionMismatchAndStyle: s7()
        case .s8ToolTrialFailure: s8()
        }
    }

    public static let taskName = "示例任务：整理原生审阅样例（虚构）"

    // MARK: S1 — interface pending reflection / remediation

    private static func s1() -> EvolutionReviewSample {
        let interface = EvolutionReviewPhase(
            id: "phase.interface",
            title: "接口契约联调",
            track: .projectDelivery,
            state: .pendingReflection,
            governance: .notRequired,
            productVersion: "草案 v0.3",
            requirements: [
                EvolutionReviewRequirement(id: "req.interface.contract", title: "接口契约核对",
                                           satisfied: false, note: "反思核对未完成"),
                EvolutionReviewRequirement(id: "req.interface.verify", title: "既定验证",
                                           satisfied: true, note: "样例记录 v0.3"),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.interface.draft", title: "接口草案",
                                        version: "v0.3", sourceRole: .mainExecutor, kind: .artifact,
                                        body: """
                                        虚构接口草案 v0.3：字段 exportMode、pageSize、cursor，仅供离线审阅。
                                        01 范围：本草案描述虚构的批量导出接口，不代表真实服务。
                                        02 来源：全部文字内嵌于样例，不来自任务、会话或本机文件。
                                        03 请求：示例请求由导出模式、分页大小和游标三个字段组成。
                                        04 导出模式：草案字段写作 exportMode，用于展示待核对的命名差异。
                                        05 既定契约：样例要求使用 export_mode，不以草案自动更改验收标准。
                                        06 分页大小：pageSize 表示每页的示例记录数，不发出实际请求。
                                        07 分页边界：空页与最后一页需要分别说明，不能仅凭模型结论省略。
                                        08 游标：cursor 是虚构的下一页标记，不含任何真实身份或凭据。
                                        09 首次分页：未提供游标时的解释只属于本份接口说明。
                                        10 后续分页：联调需要按同一版本契约核对游标和字段命名。
                                        11 响应结构：示例响应包含记录列表和下一页标记，不是执行结果。
                                        12 空结果：空列表与接口失败是不同情况，不混用状态说明。
                                        13 错误说明：字段不匹配应记录为契约问题，而不是风格偏好。
                                        14 版本绑定：v0.3 的草案内容不能自动作为未来版本的验证证据。
                                        15 反思核对：需对照既定字段名称与当前草案，不按模型票数判断。
                                        16 意见来源：机器来源示例不是用户授权，也未真实投递。
                                        17 依赖范围：依赖 export_mode 的接口联调等待契约核对结论。
                                        18 独立工作：不依赖该字段、无工作区冲突且获准的文档可继续。
                                        19 历史保留：后续整改不能抹去原草案及其版本记录。
                                        20 工具边界：阅读这份说明不会创建候选副本、启动进程或执行命令。
                                        21 滚动审阅：长正文用于检查证据详情的真实滚动，不是流式输出。
                                        22 验证边界：本说明不声称联网、测试通过、工具运行或项目已整改。
                                        内嵌证据末段：本页全部为虚构接口说明，未读取本机文件，未执行工具。
                                        """),
                EvolutionReviewEvidence(id: "ev.interface.verify", title: "既定验证记录",
                                        version: "v0.3", sourceRole: .mainExecutor, kind: .establishedVerification,
                                        body: "虚构验证记录：契约字段与联调用例 12/12 通过。\n（样例文本，未运行任何测试）"),
            ],
            opinions: [
                EvolutionReviewOpinion(
                    id: "op.interface.reflection",
                    title: "字段命名与既定契约不一致",
                    sourceRole: .reflection,
                    sourceKind: .machineFeedback,
                    severity: .blocking,
                    body: "虚构反思意见：exportMode 与既定契约的 export_mode 不一致，依赖该字段的联调会失败。",
                    requirementIDs: ["req.interface.contract"],
                    impactScope: "仅接口联调相关依赖",
                    deliveryState: "机器来源示例 · 未真实投递"
                ),
            ],
            candidates: [],
            dependencies: [
                EvolutionReviewDependency(id: "dep.interface.integration", title: "接口联调联测",
                                          state: .waiting, reason: "依赖该接口契约的核对结论"),
                EvolutionReviewDependency(id: "dep.interface.docs", title: "独立文档整理",
                                          state: .continuing,
                                          reason: "不依赖该接口、无工作区冲突且已获准，可继续"),
            ],
            summary: "接口契约待反思核对；依赖该接口的联调等待，独立文档可继续。"
        )

        let docs = EvolutionReviewPhase(
            id: "phase.docs",
            title: "独立文档整理",
            track: .projectDelivery,
            state: .passed,
            governance: .notRequired,
            productVersion: "v1",
            requirements: [
                EvolutionReviewRequirement(id: "req.docs.verify", title: "既定验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.docs.reflection", title: "反思核对", satisfied: true),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.docs.verify", title: "文档核对记录",
                                        version: "v1", sourceRole: .reflection, kind: .reflection,
                                        body: "虚构核对记录：目录与链接自检通过。\n（样例文本）"),
            ],
            opinions: [],
            candidates: [],
            dependencies: [],
            summary: "与接口无依赖的独立文档已通过。"
        )

        let tool = EvolutionReviewPhase(
            id: "phase.tool",
            title: "示例工具候选",
            track: .capabilityImprovement,
            state: .passed,
            governance: .reviewed,
            productVersion: "候选 v1",
            requirements: [
                EvolutionReviewRequirement(id: "req.tool.verify", title: "候选受保护验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.tool.review", title: "同版本必要评审", satisfied: true),
                EvolutionReviewRequirement(id: "req.tool.approval", title: "既有审批", satisfied: true),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.tool.verify.v1", title: "候选验证记录",
                                        version: "v1", sourceRole: .mainExecutor, kind: .establishedVerification,
                                        body: "虚构验证记录：示例工具「批量导出」v1 在样例夹具上通过。\n（样例文本）"),
                EvolutionReviewEvidence(id: "ev.tool.review.v1", title: "必要评审记录",
                                        version: "v1", sourceRole: .governance, kind: .governance,
                                        body: "虚构评审记录：v1 的验证范围与回退路径已确认。\n（样例文本）"),
            ],
            opinions: [],
            candidates: [candidateExportV1()],
            dependencies: [],
            summary: "v1 候选验证、评审与审批齐备，可受控试用。"
        )

        return EvolutionReviewSample(
            scenario: .s1InterfacePending,
            taskName: taskName,
            phases: [interface, docs, tool],
            supervision: .healthy,
            governanceProcessNote: governanceProcessNote,
            feedbackDeliveryNote: feedbackDeliveryNote
        )
    }

    // MARK: S2 — ordinary phase passed, governance pending

    private static func s2() -> EvolutionReviewSample {
        let verify = EvolutionReviewPhase(
            id: "phase.verify",
            title: "既定验证与阶段收束",
            track: .projectDelivery,
            state: .passed,
            governance: .pendingReview,
            productVersion: "v1.4",
            requirements: [
                EvolutionReviewRequirement(id: "req.verify.established", title: "既定验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.verify.reflection", title: "反思核对", satisfied: true),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.verify.v14", title: "既定验证记录",
                                        version: "v1.4", sourceRole: .mainExecutor, kind: .establishedVerification,
                                        body: "虚构验证记录：v1.4 的既定验证用例 24/24 通过。\n（样例文本，未运行任何测试）"),
                EvolutionReviewEvidence(id: "ev.reflection.v14", title: "反思核对记录",
                                        version: "v1.4", sourceRole: .reflection, kind: .reflection,
                                        body: "虚构反思记录：声称完成与证据一致，无未解决验收问题。\n（样例文本）"),
            ],
            opinions: [
                EvolutionReviewOpinion(
                    id: "op.verify.governance",
                    title: "治理复审已入队，尚未完成",
                    sourceRole: .governance,
                    sourceKind: .machineFeedback,
                    severity: .info,
                    body: "虚构治理意见：关键事件已通知独立治理侧，异步及时复审中；待审不等于通过或免审。",
                    requirementIDs: ["req.verify.established"],
                    impactScope: "候选启用仍需治理评审；普通阶段不等待签字",
                    deliveryState: "已入队（样例）· 尚未消费，未产生复审结论"
                ),
            ],
            candidates: [],
            dependencies: [
                EvolutionReviewDependency(id: "dep.verify.downstream", title: "依赖该阶段的后续工作",
                                          state: .waiting,
                                          reason: "既定验证与反思已满足，可关闭阶段并推进依赖"),
            ],
            summary: "普通阶段已满足既定验证与反思，可关闭并推进依赖；治理仍待审。"
        )

        let trial = EvolutionReviewPhase(
            id: "phase.trial",
            title: "候选受控试用",
            track: .capabilityImprovement,
            state: .passed,
            governance: .reviewed,
            productVersion: "候选 v1",
            requirements: [
                EvolutionReviewRequirement(id: "req.trial.verify", title: "候选受保护验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.trial.review", title: "同版本必要评审", satisfied: true),
                EvolutionReviewRequirement(id: "req.trial.approval", title: "既有审批", satisfied: true),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.trial.v1", title: "试用记录",
                                        version: "v1", sourceRole: .mainExecutor, kind: .establishedVerification,
                                        body: "虚构试用记录：v1 试用 3 次，无失败。\n（样例文本）"),
            ],
            opinions: [],
            candidates: [candidateExportV1()],
            dependencies: [],
            summary: "v1 已获准试用；阶段通过与候选启用是两件事。"
        )

        return EvolutionReviewSample(
            scenario: .s2OrdinaryPassedGovernancePending,
            taskName: taskName,
            phases: [verify, trial],
            supervision: .healthy,
            governanceProcessNote: governanceProcessNote,
            feedbackDeliveryNote: feedbackDeliveryNote
        )
    }

    // MARK: S3 — candidate missing required review

    private static func s3() -> EvolutionReviewSample {
        let verify = EvolutionReviewPhase(
            id: "phase.verify",
            title: "既定验证与阶段收束",
            track: .projectDelivery,
            state: .passed,
            governance: .pendingReview,
            productVersion: "v1.5",
            requirements: [
                EvolutionReviewRequirement(id: "req.verify.established", title: "既定验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.verify.reflection", title: "反思核对", satisfied: true),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.verify.v15", title: "既定验证记录",
                                        version: "v1.5", sourceRole: .mainExecutor, kind: .establishedVerification,
                                        body: "虚构验证记录：v1.5 既定验证通过。\n（样例文本）"),
            ],
            opinions: [],
            candidates: [],
            dependencies: [],
            summary: "普通阶段已通过——这不使缺评审的候选可试用。"
        )

        let tool = EvolutionReviewPhase(
            id: "phase.tool",
            title: "示例工具候选 v2",
            track: .capabilityImprovement,
            state: .pendingRemediation,
            governance: .blockingCandidate,
            productVersion: "候选 v2",
            requirements: [
                EvolutionReviewRequirement(id: "req.tool.verify", title: "候选受保护验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.tool.review", title: "同版本必要评审",
                                           satisfied: false, note: "v2 缺同版本评审"),
                EvolutionReviewRequirement(id: "req.tool.approval", title: "既有审批",
                                           satisfied: false, note: "尚未满足"),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.tool.verify.v2", title: "候选验证记录",
                                        version: "v2", sourceRole: .mainExecutor, kind: .establishedVerification,
                                        body: "虚构验证记录：示例工具「批量导出」v2 在样例夹具上通过。\n（样例文本）"),
            ],
            opinions: [
                EvolutionReviewOpinion(
                    id: "op.tool.review.missing",
                    title: "候选缺少同版本必要评审",
                    sourceRole: .governance,
                    sourceKind: .machineFeedback,
                    severity: .blocking,
                    body: "虚构治理意见：v2 的验证通过，但没有针对 v2 的必要评审记录；阶段通过不能代替评审。",
                    requirementIDs: ["req.tool.review"],
                    impactScope: "仅阻断该候选启用",
                    deliveryState: "已入队（样例）· 未产生复审结论"
                ),
            ],
            candidates: [
                EvolutionReviewCandidate(
                    id: "cand.export.v2",
                    title: "示例工具「批量导出」v2",
                    toolName: "示例工具「批量导出」",
                    version: "v2",
                    verification: .satisfied,
                    requiredReview: .missing,
                    approval: .missing,
                    trial: .blocked,
                    notes: ["候选与证据版本绑定：v2 需要 v2 的评审。", "缺评审时保持未启用。"]
                ),
            ],
            dependencies: [],
            summary: "候选验证通过、普通阶段通过，仍因缺同版本必要评审而不能试用。"
        )

        return EvolutionReviewSample(
            scenario: .s3CandidateMissingReview,
            taskName: taskName,
            phases: [verify, tool],
            supervision: .healthy,
            governanceProcessNote: governanceProcessNote,
            feedbackDeliveryNote: feedbackDeliveryNote
        )
    }

    // MARK: S4 — supervision API failure / timeout

    private static func s4() -> EvolutionReviewSample {
        let tool = EvolutionReviewPhase(
            id: "phase.tool",
            title: "示例工具候选 v2",
            track: .capabilityImprovement,
            state: .pendingRemediation,
            governance: .blockingCandidate,
            productVersion: "候选 v2",
            requirements: [
                EvolutionReviewRequirement(id: "req.tool.verify", title: "候选受保护验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.tool.review", title: "同版本必要评审",
                                           satisfied: false, note: "现有评审针对 v1"),
                EvolutionReviewRequirement(id: "req.tool.approval", title: "既有审批", satisfied: true),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.tool.verify.v2", title: "候选验证记录",
                                        version: "v2", sourceRole: .mainExecutor, kind: .establishedVerification,
                                        body: "虚构验证记录：v2 在样例夹具上通过。\n（样例文本）"),
                EvolutionReviewEvidence(id: "ev.tool.review.v1", title: "必要评审记录（旧版本）",
                                        version: "v1", sourceRole: .governance, kind: .governance,
                                        body: "虚构评审记录：针对 v1 的评审，不能批准 v2。\n（样例文本）"),
            ],
            opinions: [
                EvolutionReviewOpinion(
                    id: "op.supervision.failed",
                    title: "监督 API 调用失败 / 超时",
                    sourceRole: .governance,
                    sourceKind: .machineFeedback,
                    severity: .warning,
                    body: "虚构监督记录：监督 API 调用失败（超时）。记为失败/待审，不算通过，也不构成模型否决；不静默换模型、不以主执行者自审替代、不回退有效工具。",
                    requirementIDs: ["req.tool.review"],
                    impactScope: "候选启用保持阻断；既有安全工具不受影响",
                    deliveryState: "记为失败/待审（样例）· 未产生复审结论"
                ),
            ],
            candidates: [
                EvolutionReviewCandidate(
                    id: "cand.export.v2",
                    title: "示例工具「批量导出」v2",
                    toolName: "示例工具「批量导出」",
                    version: "v2",
                    verification: .satisfied,
                    requiredReview: .staleVersion,
                    requiredReviewVersion: "v1",
                    approval: .satisfied,
                    trial: .blocked,
                    notes: ["监督通道失败期间不启用缺审候选。", "恢复只载入当前版本的新评审示例，不自动通过。"]
                ),
            ],
            dependencies: [],
            summary: "监督 API 失败/超时：记为失败或待审，缺审候选不启用，既有安全工具不回退。"
        )

        return EvolutionReviewSample(
            scenario: .s4SupervisionFailure,
            taskName: taskName,
            phases: [tool],
            supervision: .failed,
            governanceProcessNote: governanceProcessNote,
            feedbackDeliveryNote: feedbackDeliveryNote
        )
    }

    // MARK: S5 — late substantive finding

    private static func s5() -> EvolutionReviewSample {
        let delivery = EvolutionReviewPhase(
            id: "phase.delivery",
            title: "交付收束（原已通过）",
            track: .projectDelivery,
            state: .passed,
            governance: .reviewed,
            productVersion: "v2.0",
            requirements: [
                EvolutionReviewRequirement(id: "req.delivery.established", title: "既定验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.delivery.reflection", title: "反思核对", satisfied: true),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.delivery.pass.v20", title: "原通过记录",
                                        version: "v2.0", sourceRole: .mainExecutor, kind: .establishedVerification,
                                        body: "虚构原通过记录：v2.0 既定验证通过，阶段已关闭。\n（样例文本）保留为历史，不因漏检被删除。"),
                EvolutionReviewEvidence(id: "ev.delivery.downstream", title: "下游成果记录",
                                        version: "v2.0", sourceRole: .mainExecutor, kind: .artifact,
                                        body: "虚构下游成果：已产出的交付物与文档保留，不重做无关安全工作。\n（样例文本）"),
            ],
            opinions: [
                EvolutionReviewOpinion(
                    id: "op.late.finding",
                    title: "迟到实质漏检：导出字段仍与既定要求不符",
                    sourceRole: .governance,
                    sourceKind: .machineFeedback,
                    severity: .blocking,
                    body: "虚构治理意见：有据的验收问题，仍适用当前产物 v2.0；按 Q25 局部重开并复核实际受影响依赖链。",
                    requirementIDs: ["req.delivery.established"],
                    impactScope: "仅实际受影响的导出字段依赖链",
                    deliveryState: "已入队（样例）· 未确认主执行已停止或已整改"
                ),
            ],
            candidates: [],
            dependencies: [
                EvolutionReviewDependency(id: "dep.export.integration", title: "导出字段联调",
                                          state: .restricted, reason: "实际受影响的依赖链，需按新前提补验"),
                EvolutionReviewDependency(id: "dep.export.docs", title: "独立文档整理",
                                          state: .continuing, reason: "安全独立路径保留，不受该漏检影响"),
                EvolutionReviewDependency(id: "dep.export.worker", title: "已开工的批量任务",
                                          state: .executingUnsettled,
                                          reason: "未观察到暂停证据，仍在执行 / 待收束；不显示为已停止"),
            ],
            summary: "保留原通过记录与下游成果，局部重开并只限制实际受影响依赖。"
        )

        let tool = EvolutionReviewPhase(
            id: "phase.tool",
            title: "示例工具候选",
            track: .capabilityImprovement,
            state: .passed,
            governance: .reviewed,
            productVersion: "候选 v1",
            requirements: [
                EvolutionReviewRequirement(id: "req.tool.verify", title: "候选受保护验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.tool.review", title: "同版本必要评审", satisfied: true),
                EvolutionReviewRequirement(id: "req.tool.approval", title: "既有审批", satisfied: true),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.tool.verify.v1", title: "候选验证记录",
                                        version: "v1", sourceRole: .mainExecutor, kind: .establishedVerification,
                                        body: "虚构验证记录：v1 通过。\n（样例文本）"),
            ],
            opinions: [],
            candidates: [candidateExportV1()],
            dependencies: [],
            summary: "旧工具安全、宿主允许时可继续使用。"
        )

        return EvolutionReviewSample(
            scenario: .s5LateFinding,
            taskName: taskName,
            phases: [delivery, tool],
            supervision: .healthy,
            governanceProcessNote: governanceProcessNote,
            feedbackDeliveryNote: feedbackDeliveryNote,
            preservationNote: "原通过记录、版本与下游成果保留；只解除对应限制，不重做无关安全工作。"
        )
    }

    // MARK: S6 — remediation and dispute

    // Fixed S5 → S6 sample transition, not a general dependency/review engine.
    static let remediationVersion = "v2.1"
    static let targetedRemediationReviewID = "ev.remediation.targeted.review"
    static let pendingRemediationRequirements: [EvolutionReviewRequirement] = [
        EvolutionReviewRequirement(id: "req.remediation.patch", title: "新版整改",
                                   satisfied: false, note: "待载入 v2.1 整改记录"),
        EvolutionReviewRequirement(id: "req.remediation.reflection", title: "反思核对",
                                   satisfied: false, note: "待核对 v2.1 整改"),
        EvolutionReviewRequirement(id: "req.remediation.verify", title: "既定验证",
                                   satisfied: false, note: "待载入 v2.1 既定验证"),
        EvolutionReviewRequirement(id: "req.remediation.targeted", title: "原问题定向复审",
                                   satisfied: false, note: "尚未针对原漏检问题复审"),
    ]
    static let remediationRequirementIDs = Set(pendingRemediationRequirements.map(\.id))
    static let completedRemediationEvidence: [EvolutionReviewEvidence] = [
        EvolutionReviewEvidence(id: "ev.remediation.v21", title: "新版整改记录",
                                version: remediationVersion, sourceRole: .mainExecutor, kind: .artifact,
                                body: "虚构整改记录：exportMode 已按既定契约改为 export_mode。\n（样例文本）"),
        EvolutionReviewEvidence(id: "ev.remediation.reflection.v21", title: "反思核对记录",
                                version: remediationVersion, sourceRole: .reflection, kind: .reflection,
                                body: "虚构反思记录：整改与声称一致，无未解决验收问题。\n（样例文本）"),
        EvolutionReviewEvidence(id: "ev.remediation.verify.v21", title: "既定验证记录",
                                version: remediationVersion, sourceRole: .mainExecutor, kind: .establishedVerification,
                                body: "虚构验证记录：v2.1 既定验证通过。\n（样例文本）"),
        EvolutionReviewEvidence(id: targetedRemediationReviewID, title: "原问题定向复审记录",
                                version: remediationVersion, sourceRole: .governance, kind: .governance,
                                body: "虚构复审记录：针对原漏检问题的 v2.1 定向复审通过，可解除导出字段联调的对应限制。\n（样例文本）"),
    ]

    private static func s6() -> EvolutionReviewSample {
        let delivery = EvolutionReviewPhase(
            id: "phase.delivery",
            title: "漏检整改与定向复审",
            track: .projectDelivery,
            state: .reopened,
            governance: .reviewed,
            productVersion: remediationVersion,
            requirements: [
                EvolutionReviewRequirement(id: "req.remediation.patch", title: "新版整改", satisfied: true),
                EvolutionReviewRequirement(id: "req.remediation.reflection", title: "反思核对", satisfied: true),
                EvolutionReviewRequirement(id: "req.remediation.verify", title: "既定验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.remediation.targeted", title: "原问题定向复审",
                                           satisfied: false, note: "尚未针对原漏检问题复审"),
            ],
            evidence: completedRemediationEvidence.filter { $0.id != targetedRemediationReviewID },
            opinions: [
                EvolutionReviewOpinion(
                    id: "op.remediation.targeted",
                    title: "原问题定向复审未完成",
                    sourceRole: .governance,
                    sourceKind: .machineFeedback,
                    severity: .warning,
                    body: "虚构治理意见：整改、反思与既定验证已满足，但原漏检问题的定向复审尚未完成，限制保持。",
                    requirementIDs: ["req.remediation.targeted"],
                    impactScope: "仅解除对应限制，不自动放宽其他依赖",
                    deliveryState: "已入队（样例）· 未产生复审结论"
                ),
            ],
            candidates: [],
            dependencies: [
                EvolutionReviewDependency(id: "dep.export.integration", title: "导出字段联调",
                                          state: .restricted,
                                          reason: "待原问题定向复审满足后解除"),
            ],
            summary: "展示新版整改、反思、既定验证与原问题定向复审满足后再解对应限制。"
        )

        let tool = EvolutionReviewPhase(
            id: "phase.tool",
            title: "示例工具候选 v2",
            track: .capabilityImprovement,
            state: .pendingRemediation,
            governance: .blockingCandidate,
            productVersion: "候选 v2",
            requirements: [
                EvolutionReviewRequirement(id: "req.tool.verify", title: "候选受保护验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.tool.review", title: "同版本必要评审",
                                           satisfied: false, note: "用户裁决不能替代候选必需评审"),
                EvolutionReviewRequirement(id: "req.tool.approval", title: "既有审批", satisfied: true),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.tool.verify.v2", title: "候选验证记录",
                                        version: "v2", sourceRole: .mainExecutor, kind: .establishedVerification,
                                        body: "虚构验证记录：v2 通过。\n（样例文本）"),
            ],
            opinions: [],
            candidates: [
                EvolutionReviewCandidate(
                    id: "cand.export.v2",
                    title: "示例工具「批量导出」v2",
                    toolName: "示例工具「批量导出」",
                    version: "v2",
                    verification: .satisfied,
                    requiredReview: .missing,
                    approval: .satisfied,
                    trial: .blocked,
                    notes: ["Q24 用户争议裁决不替代候选必需评审。"]
                ),
            ],
            dependencies: [],
            summary: "争议裁决示例可查看，但不能让缺评审候选启用。"
        )

        return EvolutionReviewSample(
            scenario: .s6RemediationAndDispute,
            taskName: taskName,
            phases: [delivery, tool],
            supervision: .healthy,
            governanceProcessNote: governanceProcessNote,
            feedbackDeliveryNote: feedbackDeliveryNote,
            disputeNote: "Q24 用户争议裁决示例（只读）：维持既定验收标准；该裁决不替代工具候选必需评审或既定验证。",
            preservationNote: "整改期间保留下游成果；仅解除对应限制。"
        )
    }

    // MARK: S7 — version mismatch and style suggestion

    private static func s7() -> EvolutionReviewSample {
        let verify = EvolutionReviewPhase(
            id: "phase.verify",
            title: "既定验证与阶段收束",
            track: .projectDelivery,
            state: .passed,
            governance: .notRequired,
            productVersion: "v1.6",
            requirements: [
                EvolutionReviewRequirement(id: "req.verify.established", title: "既定验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.verify.reflection", title: "反思核对", satisfied: true),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.verify.v16", title: "既定验证记录",
                                        version: "v1.6", sourceRole: .mainExecutor, kind: .establishedVerification,
                                        body: "虚构验证记录：v1.6 既定验证通过。\n（样例文本）"),
            ],
            opinions: [
                EvolutionReviewOpinion(
                    id: "op.style.rounded",
                    title: "建议统一按钮圆角为胶囊",
                    sourceRole: .governance,
                    sourceKind: .machineFeedback,
                    severity: .info,
                    body: "虚构风格建议：与既定要求无关的样式偏好。",
                    requirementIDs: [],
                    impactScope: "无",
                    deliveryState: "纯风格建议 · 不自动阻断阶段",
                    isStylePreference: true,
                    hasEvidence: false
                ),
                EvolutionReviewOpinion(
                    id: "op.vote.consensus",
                    title: "两个模型都给出通过结论",
                    sourceRole: .reflection,
                    sourceKind: .modelReflection,
                    severity: .info,
                    body: "虚构记录：模型头衔与投票不决定事实；事实由既定要求与当前证据决定。",
                    requirementIDs: [],
                    impactScope: "无",
                    deliveryState: "无证据的模型结论（样例）",
                    isStylePreference: false,
                    hasEvidence: false
                ),
            ],
            candidates: [],
            dependencies: [],
            summary: "风格建议与无证据的模型结论都不阻断已满足的阶段。"
        )

        let tool = EvolutionReviewPhase(
            id: "phase.tool",
            title: "示例工具候选 v3",
            track: .capabilityImprovement,
            state: .pendingRemediation,
            governance: .blockingCandidate,
            productVersion: "候选 v3",
            requirements: [
                EvolutionReviewRequirement(id: "req.tool.verify", title: "候选受保护验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.tool.review", title: "同版本必要评审",
                                           satisfied: false, note: "现有评审针对 v2"),
                EvolutionReviewRequirement(id: "req.tool.approval", title: "既有审批", satisfied: true),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.tool.verify.v3", title: "候选验证记录",
                                        version: "v3", sourceRole: .mainExecutor, kind: .establishedVerification,
                                        body: "虚构验证记录：v3 通过。\n（样例文本）"),
                EvolutionReviewEvidence(id: "ev.tool.review.v2", title: "必要评审记录（旧版本）",
                                        version: "v2", sourceRole: .governance, kind: .governance,
                                        body: "虚构评审记录：针对 v2 的评审结论，不自动套用到 v3。\n（样例文本）"),
            ],
            opinions: [
                EvolutionReviewOpinion(
                    id: "op.version.mismatch",
                    title: "旧版证据不能批准新版候选",
                    sourceRole: .governance,
                    sourceKind: .machineFeedback,
                    severity: .blocking,
                    body: "虚构治理意见：v2 的评审与验证结论不能用于批准 v3；先核对当前适用性。",
                    requirementIDs: ["req.tool.review"],
                    impactScope: "仅阻断该候选启用",
                    deliveryState: "已入队（样例）· 未产生复审结论"
                ),
            ],
            candidates: [
                EvolutionReviewCandidate(
                    id: "cand.export.v3",
                    title: "示例工具「批量导出」v3",
                    toolName: "示例工具「批量导出」",
                    version: "v3",
                    verification: .satisfied,
                    requiredReview: .staleVersion,
                    requiredReviewVersion: "v2",
                    approval: .satisfied,
                    trial: .blocked,
                    notes: ["候选与证据版本绑定，不用旧结论批准新版本。"]
                ),
            ],
            dependencies: [],
            summary: "旧版证据不能批准新版候选；纯风格建议不阻断阶段。"
        )

        return EvolutionReviewSample(
            scenario: .s7VersionMismatchAndStyle,
            taskName: taskName,
            phases: [verify, tool],
            supervision: .healthy,
            governanceProcessNote: governanceProcessNote,
            feedbackDeliveryNote: feedbackDeliveryNote
        )
    }

    // MARK: S8 — tool trial runtime failure

    private static func s8() -> EvolutionReviewSample {
        let trial = EvolutionReviewPhase(
            id: "phase.trial",
            title: "候选受控试用",
            track: .capabilityImprovement,
            state: .passed,
            governance: .reviewed,
            productVersion: "候选 v2",
            requirements: [
                EvolutionReviewRequirement(id: "req.trial.verify", title: "候选受保护验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.trial.review", title: "同版本必要评审", satisfied: true),
                EvolutionReviewRequirement(id: "req.trial.approval", title: "既有审批", satisfied: true),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.trial.verify.v2", title: "试用前受保护验证记录",
                                        version: "v2", sourceRole: .mainExecutor, kind: .establishedVerification,
                                        body: "虚构验证记录：v2 试用前的受保护验证已通过，保留为历史记录。\n（样例文本，未运行任何测试）"),
                EvolutionReviewEvidence(id: "ev.trial.v2", title: "试用开始记录",
                                        version: "v2", sourceRole: .mainExecutor, kind: .artifact,
                                        body: "虚构试用开始记录：v2 进入受控试用；该记录时点尚未观察到运行故障。\n（样例文本，未执行任何工具）"),
            ],
            opinions: [],
            candidates: [
                EvolutionReviewCandidate(
                    id: "cand.export.v2",
                    title: "示例工具「批量导出」v2",
                    toolName: "示例工具「批量导出」",
                    version: "v2",
                    verification: .satisfied,
                    requiredReview: .satisfied,
                    requiredReviewVersion: "v2",
                    approval: .satisfied,
                    trial: .trialing,
                    notes: ["试用中（样例）：尚未观察到运行故障。"],
                    fallbackTool: "示例工具「批量导出」v1（获准安全工具）"
                ),
            ],
            dependencies: [],
            summary: "候选 v2 受控试用中，尚未观察到运行故障；可演示故障停用并回退获准安全工具 v1。"
        )

        let delivery = EvolutionReviewPhase(
            id: "phase.delivery",
            title: "项目交付（成果保留）",
            track: .projectDelivery,
            state: .passed,
            governance: .reviewed,
            productVersion: "v2.2",
            requirements: [
                EvolutionReviewRequirement(id: "req.delivery.established", title: "既定验证", satisfied: true),
                EvolutionReviewRequirement(id: "req.delivery.reflection", title: "反思核对", satisfied: true),
            ],
            evidence: [
                EvolutionReviewEvidence(id: "ev.delivery.v22", title: "项目成果记录",
                                        version: "v2.2", sourceRole: .mainExecutor, kind: .artifact,
                                        body: "虚构成果记录：项目交付物保留，工具回退不撤销项目代码或外部副作用。\n（样例文本）"),
            ],
            opinions: [],
            candidates: [],
            dependencies: [],
            summary: "工具故障回退不声称自动撤销项目代码与外部副作用。"
        )

        return EvolutionReviewSample(
            scenario: .s8ToolTrialFailure,
            taskName: taskName,
            phases: [trial, delivery],
            supervision: .healthy,
            governanceProcessNote: governanceProcessNote,
            feedbackDeliveryNote: feedbackDeliveryNote,
            preservationNote: "项目成果保留；回退只针对工具版本，不显示项目/外部副作用已撤销。"
        )
    }

    // MARK: - Shared candidate

    private static func candidateExportV1() -> EvolutionReviewCandidate {
        EvolutionReviewCandidate(
            id: "cand.export.v1",
            title: "示例工具「批量导出」v1",
            toolName: "示例工具「批量导出」",
            version: "v1",
            verification: .satisfied,
            requiredReview: .satisfied,
            requiredReviewVersion: "v1",
            approval: .satisfied,
            trial: .enabled,
            notes: ["获准安全工具；v1 的验证、评审与审批均为同版本。"]
        )
    }
}
#endif
