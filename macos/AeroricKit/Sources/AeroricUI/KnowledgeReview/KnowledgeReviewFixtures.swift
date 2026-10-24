#if DEBUG
import Foundation

/// Embedded fictional text and a finite set of outcomes. No extraction,
/// retrieval, source-file access, random event stream, or real check is used.
enum KnowledgeReviewFixtures {
    static let project = KnowledgeReviewProject.harbor
    static let sampleNotice = "离线知识审阅 · 非真实资料"
    static let closingAutomaticAdmissionNote =
        "关闭自动入库对既有知识及在途任务的传播规则未在本样例演示；不代表停用、撤回或清除上下文。"
    static let referenceBoundaryNote =
        "虚构参考，不是系统指令；不替代当前要求、权限、审批或当前验证，也不证明模型已消费。"
    static let retirementNote =
        "停用仅移出后续新上下文推荐；保留虚构历史及捕获的旧引用，不是删除记忆或抹除已消费上下文。"
    static let processNewNote =
        "每次重置仅载入一批固定虚构新候选；再次点击不重复、不追溯处理旧候选。"

    static let works: [KnowledgeReviewWork] = [
        .init(
            id: .storageTask, kind: .task, title: "虚构任务 · 存储选型",
            request: "为海港项目的新服务比较存储适用条件，不照搬旧原型。",
            relatedItemIDs: [.databaseChoice]
        ),
        .init(
            id: .apiTask, kind: .task, title: "虚构任务 · API 错误接口联调",
            request: "只核对错误接口的字段和契约，不扩大执行权限。",
            relatedItemIDs: [.interfaceContract]
        ),
        .init(
            id: .clientPackage, kind: .workPackage, title: "虚构工作包 · 客户端错误呈现",
            request: "按当前用户确认的接口约定核对呈现，不重放外部操作。",
            relatedItemIDs: [.interfaceContract]
        ),
        .init(
            id: .docsPackage, kind: .workPackage, title: "虚构工作包 · 独立术语文档",
            request: "整理独立术语和目录；不依赖错误接口，不修改实现。",
            relatedItemIDs: [.docsGuidance]
        ),
    ]

    static let conciseReplies = candidate(
        .conciseReplies, title: "虚构偏好 · 简短结论",
        kind: .preference, content: "先给简短结论，再列必要依据。",
        source: .init(
            id: "fictional.user.concise", kind: .userStatement,
            title: "虚构用户陈述 · 审阅记录 A",
            excerpt: """
            虚构来源首段：这是海港项目的内嵌样例摘录，不是用户真实历史。用户提出：先给简短结论，再列必要依据。

            第 02 段：这条虚构偏好用于项目内的结果说明。简短不意味着省略失败、风险或尚未验证的内容。

            第 03 段：如果当前问题需要用户决定，应把问题写清楚。不能用旧偏好替用户作出新的授权决定。

            第 04 段：同一段内容同时保留来源、适用条件和限制。摘录离开来源后，不能冒充新的可靠事实。

            第 05 段：本例只讨论海港这个虚构项目。山谷等其他项目有自己的边界，不能自动复制本条内容。

            第 06 段：原型时期的选择可能只适合离线、单机和低并发条件。新的服务任务需要查看当前要求。

            第 07 段：历史 SQLite 记录可以解释过去的设计背景。但用户当前明确要求 Postgres 时，旧记录不能覆盖它。

            第 08 段：一次引用只说明参考材料被选出。它不证明内容正确，也不证明任何模型已经实际消费它。

            第 09 段：虚构来源始终内嵌在样例中。展开此段不会打开外部文件、网页、个人历史或任何网络服务。

            第 10 段：如果来源不可用，界面应明确显示缺失。不能为补齐故事而编造消息、文件位置或核验过程。

            第 11 段：Agent 自报“应该好了”不是验证记录。审阅确认、改变外观或切换授权示例都不能制造证据。

            第 12 段：标为 PRELOADED 的检查是预置虚构记录，不是本次执行。它只适用于记录中明确限定的内容和版本。

            第 13 段：候选仍可编辑或丢弃。编辑前的检查不能自动证明改写后的结论；已改内容不沿用旧核验标签。

            第 14 段：批量勾选与详情焦点是两件事。查看某条来源，不意味着它已经被勾选，更不意味着已确认整批候选。

            第 15 段：如果批次里混入冲突或依据不足的候选，应说明原因并保留整批未确认状态，而不是悄悄强行收纳。

            第 16 段：自动入库预览默认关闭。只有本窗口里已经应用的固定预设，才参与一次新候选批次的演示。

            第 17 段：预设仅限定虚构类型、来源和项目。这里没有表达式、自动采集器、定时任务或可执行技能。

            第 18 段：Full Access 只是单独的任务授权示例。它不打开自动入库，也不扩大允许引用的项目范围。

            第 19 段：关闭自动入库对既有知识和在途工作的传播规则没有在本样例中设计；不能推断关闭等于清除上下文。

            第 20 段：用户明确纠正后可形成有依据的示例 v2。获准保留的 v1 来源和替代关系仍能回看。

            第 21 段：新的 Agent 猜测不替代用户确认。它仍是待处理候选，不能借旧来源标签写成用户纠正。

            第 22 段：停用只使知识退出后续推荐。已经捕获的旧版本引用仍保留，样例不声称抹掉模型已读过的内容。

            第 23 段：两条接口工作分别核对各自受影响的产物。一条适用记录不能顺带把另一条工作也标为已核对。

            第 24 段：通知或 ACK 只展示送达与收到的样例变化。它们不证明纠正完成，也不证明外部副作用已经撤销。

            第 25 段：与接口无关的独立术语文档可以在原安全范围继续。这个按钮仍然只改变内存，不会启动真实任务。

            虚构来源末段：以上 26 段仅用于长文滚动与来源关联的离线演示；重置或重建窗口不保留草稿、配置与版本改动。
            """
        ),
        applicability: "海港项目的结果说明与审阅摘要。",
        limits: "不省略风险或未验证项；不适用于其他项目。"
    )

    static let verifiedCheck = candidate(
        .verifiedCheck, title: "虚构解法 · 空字段提示", kind: .solution,
        content: "空字段应返回可读错误，避免把空值当成成功结果。",
        source: .init(
            id: "fictional.check.empty-field", kind: .preloadedCheck,
            title: "虚构预置检查 · PRELOADED · 非本次实测",
            excerpt: "【虚构 PRELOADED 记录】示例 v1 的空字段输入得到可读错误；仅此固定输入，不证明当前产物。"
        ),
        applicability: "海港项目的虚构表单空字段示例。",
        limits: "仅预置空字段用例，不覆盖并发、网络或当前代码；仍需当前验证。",
        evidence: .preloadedVerified
    )

    static let discardable = candidate(
        .discardable, title: "虚构提议 · 临时术语", kind: .decision,
        content: "早期提议使用临时术语“航标”，可在集中审阅中丢弃。",
        source: .init(
            id: "fictional.user.term", kind: .userStatement,
            title: "虚构用户讨论 · 临时术语",
            excerpt: "【虚构摘录】“航标”只是讨论中的备选术语，不是正式命名要求。"
        ),
        applicability: "海港项目的早期术语讨论。",
        limits: "不应把备选项作为后续任务的强制名称。"
    )

    static let selfReport = candidate(
        .selfReport, title: "虚构自报 · 重试已修好？", kind: .solution,
        content: "Agent 自报已修复重试，但未提供可核对的检查记录。",
        source: .init(
            id: "fictional.agent.retry", kind: .agentReport,
            title: "虚构 Agent 自报 · 无核验记录",
            excerpt: "【虚构自报】“我觉得重试已经好了。”没有附当前产物或检查依据。"
        ),
        applicability: "海港项目的重试讨论，尚非可复用解法。",
        limits: "自报保持未核验，确认或 Full Access 不能制造验证证据。",
        evidence: .unverifiedAgentReport, issue: .insufficientEvidence
    )

    static let databaseChoice = reusable(
        .databaseChoice, title: "虚构历史决策 · SQLite 原型", kind: .decision,
        content: "历史离线单机原型选择 SQLite；这不是新服务必须继续使用的存储要求。",
        source: .init(
            id: "fictional.user.sqlite", kind: .userStatement,
            title: "虚构用户决策 · 离线原型",
            excerpt: "【虚构历史】旧原型单机、离线、低并发，因此当时选择 SQLite。新任务需重新依据当前要求选型。"
        ),
        applicability: "仅海港项目过去的单机离线原型。",
        limits: "不覆盖新的多用户服务；当前明确 Postgres 要求优先；历史选择不证明当前正确。"
    )

    static let interfaceV1 = KnowledgeReviewVersion(
        itemID: .interfaceContract, number: 1,
        content: "旧接口示例：错误响应只有 message 字符串；此为获准保留的历史约定。",
        source: .init(
            id: "fictional.user.interface.v1", kind: .userStatement,
            title: "虚构用户确认 · 接口 v1",
            excerpt: "【虚构 v1 摘录】第一版使用 message 展示错误；还没有稳定的错误分类字段。"
        ),
        applicability: .init(
            project: .harbor, summary: "海港项目的旧版错误接口与已捕获的 v1 引用。",
            limits: "不以文案推断权限或工具审批；不能代替当前接口验证。"
        ),
        evidence: .userStatement, state: .current, correctionBasis: nil, replaces: nil
    )

    static let correctionContent =
        "接口示例 v2：保留 message，并增加稳定字段 code；客户端按 code 区分错误类型。"
    static let correctionApplicability =
        "海港项目下一版错误接口及相关 API／客户端联调。"
    static let correctionBasis =
        "虚构用户纠正 #2：需要区分可重试与不可重试错误，不能根据 message 文案判断。"

    static let interfaceV2 = KnowledgeReviewVersion(
        itemID: .interfaceContract, number: 2, content: correctionContent,
        source: .init(
            id: "fictional.user.interface.v2.preloaded", kind: .userCorrection,
            title: "虚构用户纠正 · 预置 v2（非本次操作）",
            excerpt: correctionBasis
        ),
        applicability: .init(
            project: .harbor, summary: correctionApplicability,
            limits: interfaceV1.applicability.limits
        ),
        evidence: .userStatement, state: .current, correctionBasis: correctionBasis,
        replaces: interfaceV1.id
    )

    static let docsGuidance = reusable(
        .docsGuidance, title: "虚构文档约定 · 独立术语", kind: .decision,
        content: "独立文档仅整理术语与目录，不修改接口契约。",
        source: .init(
            id: "fictional.user.docs", kind: .userStatement,
            title: "虚构用户说明 · 独立文档范围",
            excerpt: "【虚构摘录】术语整理不依赖接口字段变更，可在原有安全范围继续。"
        ),
        applicability: "海港项目的独立术语与目录工作包。",
        limits: "不自动放行接口联调；不重启 Agent 或重放外部操作。"
    )

    static let agentGuess = candidate(
        .agentGuess, title: "虚构新猜测 · 删除 message？", kind: .decision,
        content: "Agent 猜测删除 message 可能更简洁；没有用户纠正依据。",
        source: .init(
            id: "fictional.agent.interface-guess", kind: .agentReport,
            title: "虚构 Agent 猜测 · 非用户纠正",
            excerpt: "【虚构猜测】“也许可以直接删除 message。”这不是用户确认或当前验证。"
        ),
        applicability: "海港项目的接口讨论候选。",
        limits: "新猜测不覆盖用户确认，不生成替代版本。",
        evidence: .unverifiedAgentReport, issue: .agentGuess
    )

    static let newPreference = candidate(
        .newPreference, title: "虚构新偏好 · 先写适用条件", kind: .preference,
        content: "说明项目经验时，先写适用条件，再给示例。",
        source: .init(
            id: "fictional.new.user.preference", kind: .userStatement,
            title: "虚构新用户陈述 · 固定批次",
            excerpt: "【虚构固定批次】海港项目的经验说明请先写适用条件。"
        ),
        applicability: "仅海港项目的新经验说明。",
        limits: "不扩大权限、不跨项目；不省略当前验证。"
    )

    static let newSolution = candidate(
        .newSolution, title: "虚构新解法 · 无结果提示", kind: .solution,
        content: "没有匹配项时显示明确的无结果提示，而不是成功状态。",
        source: .init(
            id: "fictional.new.check.empty-result", kind: .preloadedCheck,
            title: "虚构固定批次检查 · PRELOADED · 非本次实测",
            excerpt: "【虚构 PRELOADED 记录】固定无结果输入只显示无结果说明；不代表当前实现已执行检查。"
        ),
        applicability: "仅海港项目的无结果提示示例。",
        limits: "预置固定输入，不证明当前版本或检索完整性。",
        evidence: .preloadedVerified
    )

    static let conflict = candidate(
        .conflict, title: "虚构冲突 · 删除用户要求字段", kind: .decision,
        content: "候选建议删除 message，与用户确认保留该字段的接口约定冲突。",
        source: .init(
            id: "fictional.exception.conflict", kind: .agentReport,
            title: "虚构冲突提议 · 固定批次",
            excerpt: "【虚构冲突】提议删除用户要求保留的 message；没有用户纠正授权，在 v1 与 v2 下都不能自动替代。"
        ),
        applicability: "海港项目的错误接口讨论。",
        limits: "不得自动替代用户约定，仍待复核。",
        evidence: .unverifiedAgentReport, issue: .conflict
    )

    static let sensitive = candidate(
        .sensitive, title: "虚构敏感例外 · 仅脱敏占位", kind: .preference,
        content: "［已移除的敏感内容占位］",
        source: .init(
            id: "fictional.exception.placeholder", kind: .userStatement,
            title: "虚构脱敏占位 · 不含敏感原文",
            excerpt: "【虚构占位】没有任何凭据、个人资料或敏感原文；此项只演示拒绝收纳。"
        ),
        applicability: "海港项目的例外展示，不能入库。",
        limits: "Full Access 也不允许强行收纳；不保留敏感原文。",
        issue: .sensitivePlaceholder
    )

    static let insufficient = candidate(
        .insufficient, title: "虚构依据不足 · 未提供检查", kind: .solution,
        content: "声称新方案更可靠，但没有相应产物或检查记录。",
        source: .init(
            id: "fictional.exception.insufficient", kind: .agentReport,
            title: "虚构自报 · 依据不足",
            excerpt: "【虚构自报】仅有“应该更可靠”的猜测，没有核验依据。"
        ),
        applicability: "海港项目的方案讨论。",
        limits: "仍待提供依据，不包装成已验证解法。",
        evidence: .unverifiedAgentReport, issue: .insufficientEvidence
    )

    static let outOfScope = candidate(
        .outOfScope, title: "虚构范围外 · 山谷项目偏好", kind: .preference,
        content: "另一虚构项目希望采用其自己的说明格式。",
        source: .init(
            id: "fictional.exception.other-project", kind: .userStatement,
            title: "虚构其他项目陈述 · 仅占位展示",
            excerpt: "【虚构范围外】山谷项目自己的偏好；不是获准给海港项目使用的知识。"
        ),
        applicability: "仅虚构山谷项目，超出当前项目预设。",
        limits: "不跨项目、不扩到全局、不生成技能。",
        project: .other, issue: .outsideProject
    )

    /// All six are considered at most once per reset. Exceptions already
    /// visible in K3 keep their identity instead of being appended again.
    static let newBatch: [KnowledgeReviewItem] = [
        newPreference, newSolution, conflict, sensitive, insufficient, outOfScope,
    ]

    static let preloadedCheck = KnowledgeReviewCheckRecord(
        id: "fictional.local-check.api.interface.v2",
        workID: .apiTask, capturedVersion: interfaceV1.id, applicableVersion: interfaceV2.id,
        source: .init(
            id: "fictional.local-check.api.source", kind: .preloadedCheck,
            title: "虚构 API 局部核对 · PRELOADED · 非本次实测",
            excerpt: "【虚构 PRELOADED 记录】仅核对 API 示例从 message 到 message＋code 的字段契约；没有执行任何命令。"
        ),
        summary: "仅 API 接口行载入 v2 预置核对示例；客户端工作包未核对，外部副作用未验证。"
    )

    static func items(for scenario: KnowledgeReviewScenario) -> [KnowledgeReviewItem] {
        let candidates: [KnowledgeReviewItem]
        switch scenario {
        case .candidates, .reset:
            candidates = [conciseReplies, verifiedCheck, discardable, selfReport]
        case .admission:
            candidates = [selfReport]
        case .exceptions:
            candidates = [conflict, sensitive, insufficient, outOfScope]
        case .references:
            candidates = []
        case .revisions, .retirement:
            candidates = [agentGuess]
        case .impact:
            candidates = []
        }
        let revised = scenario == .retirement || scenario == .impact
        return candidates + [databaseChoice, interfaceItem(revised: revised), docsGuidance]
    }

    static func defaultSection(for scenario: KnowledgeReviewScenario) -> KnowledgeReviewSection {
        switch scenario {
        case .candidates, .admission, .exceptions, .reset: .candidates
        case .references, .revisions, .retirement, .impact: .reusable
        }
    }

    static func defaultSelection(for scenario: KnowledgeReviewScenario) -> KnowledgeReviewItemID {
        switch scenario {
        case .candidates, .reset: .conciseReplies
        case .admission: .selfReport
        case .exceptions: .conflict
        case .references: .databaseChoice
        case .revisions, .retirement, .impact: .interfaceContract
        }
    }

    static func impacts(for scenario: KnowledgeReviewScenario) -> [KnowledgeReviewImpact] {
        let required = (scenario == .retirement || scenario == .impact) ? interfaceV2.id : interfaceV1.id
        let affected = [KnowledgeReviewWorkID.apiTask, .clientPackage].map { workID in
            KnowledgeReviewImpact(
                id: workID, title: work(workID).title,
                capturedReference: .init(
                    workID: workID, itemTitle: "虚构接口 · 已捕获的旧 v1",
                    version: interfaceV1
                ),
                requiredVersion: required, state: .pending, delivery: .notSent, loadedRecord: nil,
                note: "虚构在途项已引用 v1；只核对相关接口产物，不把通知当作完成纠正。"
            )
        }
        return affected + [
            .init(
                id: .docsPackage, title: work(.docsPackage).title, capturedReference: nil,
                requiredVersion: nil, state: .unaffected, delivery: .notSent, loadedRecord: nil,
                note: "虚构独立术语文档不依赖接口知识，可演示在原安全范围继续。"
            ),
        ]
    }

    static func work(_ id: KnowledgeReviewWorkID) -> KnowledgeReviewWork {
        // Exhaustive fixed IDs keep this sample from silently inventing work.
        switch id {
        case .storageTask: works[0]
        case .apiTask: works[1]
        case .clientPackage: works[2]
        case .docsPackage: works[3]
        }
    }

    private static func interfaceItem(revised: Bool) -> KnowledgeReviewItem {
        var old = interfaceV1
        old.state = .superseded
        return .init(
            id: .interfaceContract, title: "虚构接口约定 · 错误响应", kind: .decision,
            state: .reusable, issue: nil, current: revised ? interfaceV2 : interfaceV1,
            history: revised ? [old] : [], admissionOrigin: .preloaded
        )
    }

    private static func candidate(
        _ id: KnowledgeReviewItemID, title: String, kind: KnowledgeReviewKind,
        content: String, source: KnowledgeReviewSource, applicability: String, limits: String,
        project: KnowledgeReviewProject = .harbor,
        evidence: KnowledgeReviewEvidence = .userStatement,
        issue: KnowledgeReviewCandidateIssue? = nil
    ) -> KnowledgeReviewItem {
        .init(
            id: id, title: title, kind: kind, state: .candidate, issue: issue,
            current: .init(
                itemID: id, number: 1, content: content, source: source,
                applicability: .init(project: project, summary: applicability, limits: limits),
                evidence: evidence, state: .current, correctionBasis: nil, replaces: nil
            ),
            history: [], admissionOrigin: nil
        )
    }

    private static func reusable(
        _ id: KnowledgeReviewItemID, title: String, kind: KnowledgeReviewKind,
        content: String, source: KnowledgeReviewSource, applicability: String, limits: String
    ) -> KnowledgeReviewItem {
        var item = candidate(
            id, title: title, kind: kind, content: content, source: source,
            applicability: applicability, limits: limits
        )
        item.state = .reusable
        item.admissionOrigin = .preloaded
        return item
    }
}
#endif
