#if DEBUG
import Foundation
import Observation

/// Window-local owner for the fictional deliverables specimen. No production
/// client, persistence, clock, process, network, or business writer is used.
@MainActor @Observable
final class TaskDeliverablesReviewStore {
    private(set) var scenario: TaskDeliverablesReviewScenario
    private(set) var deliverables: [TaskDeliverable]
    private(set) var feedbacks: [TaskDeliverablesReviewFeedback]
    private(set) var evidence: [TaskDeliverablesReviewEvidence]
    private(set) var operations: [TaskDeliverablesReviewOperationIntent]
    private(set) var rounds: [TaskDeliverablesReviewRound]
    private(set) var conditions: TaskDeliverablesReviewConditions
    private(set) var records: [TaskDeliverablesReviewRecord] = []
    private(set) var feedback = "所有动作只影响本窗口的虚构内存。"
    private(set) var generation = 0

    var selectedDeliverableID: TaskDeliverableID?
    var selectedVersionID: String?
    var selectedFeedbackID: String?
    private(set) var organizationEditID: TaskDeliverableID?
    private(set) var organizationTitleDraft = ""
    private(set) var organizationDescriptionDraft = ""
    private(set) var draft: TaskDeliverablesReviewDraft

    @ObservationIgnored private let identity: UUID
    @ObservationIgnored private var nextRecordID = 1
    @ObservationIgnored private var nextFeedbackID = 2
    @ObservationIgnored private var nextOperationID = 1
    @ObservationIgnored private var draftToken = 0

    init(scenario: TaskDeliverablesReviewScenario = .reset) {
        let identity = UUID()
        self.identity = identity
        self.scenario = scenario
        let fixture = TaskDeliverablesReviewFixtures.make(scenario)
        deliverables = fixture.deliverables
        feedbacks = fixture.feedbacks
        evidence = fixture.evidence
        operations = fixture.operations
        rounds = fixture.rounds
        conditions = fixture.conditions
        selectedDeliverableID = fixture.selectedDeliverableID
        selectedVersionID = fixture.selectedVersionID
        selectedFeedbackID = fixture.feedbacks.first?.id
        draft = TaskDeliverablesReviewDraft(owner: identity, token: 0, correctionOf: nil)
        refreshRounds()
    }

    var taskTitle: String {
        TaskDeliverablesReviewFixtures.taskTitle(for: scenario)
    }

    var scenarioSummary: String {
        "\(scenario.instruction)\n\(TaskDeliverablesReviewFixtures.versionBoundary)"
    }

    var visibleDeliverables: [TaskDeliverable] {
        deliverables.filter(\.isVisibleInCollection)
    }

    var temporaryDeliverables: [TaskDeliverable] {
        deliverables.filter(\.isTemporary)
    }

    var selectedDeliverable: TaskDeliverable? {
        guard let selectedDeliverableID else { return nil }
        return deliverable(selectedDeliverableID)
    }

    var selectedVersion: TaskDeliverableVersion? {
        guard let selectedDeliverable, let selectedVersionID else { return nil }
        return selectedDeliverable.versions.first { $0.id == selectedVersionID }
    }

    var selectedFeedback: TaskDeliverablesReviewFeedback? {
        guard let selectedFeedbackID else { return nil }
        return feedback(id: selectedFeedbackID)
    }

    var selectedEvidence: [TaskDeliverablesReviewEvidence] {
        guard let selectedFeedbackID else { return evidence }
        return evidence.filter { $0.feedbackID == selectedFeedbackID || $0.feedbackID == nil }
    }

    var canSendDraft: Bool {
        !draft.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && draft.deliverableID != nil
            && draft.versionID != nil
    }

    var canBeginFeedback: Bool {
        draft.deliverableID == nil
            && draft.versionID == nil
            && selectedDeliverable?.isVisibleInCollection == true
            && selectedVersion != nil
    }

    var draftSummary: String {
        guard let deliverableID = draft.deliverableID,
            let versionID = draft.versionID
        else {
            return "未锚定交付版本；发送前必须明确成果和版本。"
        }
        return
            "锚定：\(deliverable(deliverableID)?.title ?? "未知成果") / \(versionID) / \(draft.location.title)"
    }

    var continuationSummary: String {
        guard let round = rounds.last else {
            return "尚未发起返工；查看旧成果或写草稿不会重开任务。"
        }
        let blockers = conditions.blockers
        if blockers.isEmpty {
            return "\(round.state.title)；已交办意图不会重复发送。"
        }
        return "\(round.state.title)；阻塞：\(blockers.joined(separator: "、"))。"
    }

    func deliverable(_ id: TaskDeliverableID) -> TaskDeliverable? {
        deliverables.first { $0.id == id }
    }

    func version(_ id: String) -> TaskDeliverableVersion? {
        deliverables.lazy.compactMap { deliverable in
            deliverable.versions.first { $0.id == id }
        }.first
    }

    func feedback(id: String) -> TaskDeliverablesReviewFeedback? {
        feedbacks.first { $0.id == id }
    }

    func events(for feedbackID: String?) -> [TaskDeliverablesReviewEvidence] {
        evidence.filter { $0.feedbackID == feedbackID }
    }

    func loadScenario(_ scenario: TaskDeliverablesReviewScenario) {
        reset(to: scenario)
    }

    func reset() {
        reset(to: scenario)
    }

    private func reset(to scenario: TaskDeliverablesReviewScenario) {
        let fixture = TaskDeliverablesReviewFixtures.make(scenario)
        self.scenario = scenario
        deliverables = fixture.deliverables
        feedbacks = fixture.feedbacks
        evidence = fixture.evidence
        operations = fixture.operations
        rounds = fixture.rounds
        conditions = fixture.conditions
        selectedDeliverableID = fixture.selectedDeliverableID
        selectedVersionID = fixture.selectedVersionID
        selectedFeedbackID = fixture.feedbacks.first?.id
        organizationEditID = nil
        organizationTitleDraft = ""
        organizationDescriptionDraft = ""
        replaceDraft()
        records = []
        feedback = "已重置为固定虚构场景；没有读取或写入真实成果。"
        generation += 1
        refreshRounds()
    }

    @discardableResult
    func selectDeliverable(_ id: TaskDeliverableID) -> Bool {
        guard let deliverable = deliverable(id), deliverable.isVisibleInCollection else {
            return reject("未收录或临时条目不能被静默当成当前成果。")
        }
        selectedDeliverableID = id
        selectedVersionID = deliverable.versions.first?.id
        return true
    }

    @discardableResult
    func selectVersion(_ id: String) -> Bool {
        guard let selectedDeliverable,
            selectedDeliverable.versions.contains(where: { $0.id == id })
        else {
            return reject("版本不属于当前选定成果；没有静默换成当前文件或其他版本。")
        }
        selectedVersionID = id
        return true
    }

    @discardableResult
    func beginOrganizationEdit(_ id: TaskDeliverableID) -> Bool {
        guard let item = deliverable(id) else { return false }
        organizationEditID = id
        organizationTitleDraft = item.title
        organizationDescriptionDraft = item.description
        feedback = "正在编辑展示元信息；取消不会改变原提交或内容。"
        return true
    }

    func setOrganizationTitleDraft(_ title: String) {
        organizationTitleDraft = title
    }

    func setOrganizationDescriptionDraft(_ description: String) {
        organizationDescriptionDraft = description
    }

    func cancelOrganizationEdit() {
        organizationEditID = nil
        organizationTitleDraft = ""
        organizationDescriptionDraft = ""
        feedback = "展示元信息编辑已取消；原提交、内容和检查事实未改变。"
    }

    @discardableResult
    func commitOrganizationEdit() -> Bool {
        guard let id = organizationEditID else {
            return reject("当前没有展示元信息草稿；没有应用编辑。")
        }
        guard let index = deliverables.firstIndex(where: { $0.id == id }) else {
            return reject("未找到成果条目；没有应用展示编辑。")
        }
        let title = organizationTitleDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        let description = organizationDescriptionDraft.trimmingCharacters(
            in: .whitespacesAndNewlines)
        guard !title.isEmpty, !description.isEmpty else {
            return reject("标题和说明不能为空；原条目保持不变。")
        }
        deliverables[index].title = title
        deliverables[index].description = description
        organizationEditID = nil
        organizationTitleDraft = ""
        organizationDescriptionDraft = ""
        record("本地整理：只更新 \(id.rawValue) 的展示标题 / 说明；原提交内容保留。")
        feedback = "展示元信息已应用；不代表用户验收或技术核验通过。"
        return true
    }

    @discardableResult
    func includeExampleTemporaryRecord() -> Bool {
        guard let index = deliverables.firstIndex(where: { $0.id == .temporaryLog }),
            deliverables[index].collectionState == .temporary
        else {
            return reject("临时示例已经被整理过，不能重复加入。")
        }
        deliverables[index].collectionState = .submitted
        record("用户补充整理：临时日志被明确加入示例清单；没有扫描或复制磁盘文件。")
        feedback = "已加入一条明确标记的示例成果；这是内存整理，不改变原日志事实。"
        return true
    }

    @discardableResult
    func removeFromCollection(_ id: TaskDeliverableID) -> Bool {
        guard let index = deliverables.firstIndex(where: { $0.id == id }),
            deliverables[index].collectionState != .hidden
        else {
            return reject("条目已不在清单中；没有删除原记录。")
        }
        deliverables[index].collectionState = .hidden
        if selectedDeliverableID == id {
            let next = visibleDeliverables.first
            selectedDeliverableID = next?.id
            selectedVersionID = next?.versions.first?.id
        }
        record("本地整理：\(id.rawValue) 移出展示清单；原提交、内容、失败依据仍保留。")
        feedback = "已移出展示清单；不等于删除原文件、撤销提交或用户验收。"
        return true
    }

    func beginFeedback(location: TaskDeliverablesReviewLocation? = nil) {
        guard canBeginFeedback,
            let selectedDeliverableID,
            let selectedVersionID,
            let selectedVersion
        else {
            _ = reject("当前草稿已锚定或没有可用交付版本；没有静默更换反馈依据。")
            return
        }

        let requestedLocation = location ?? draft.location
        let anchoredLocation: TaskDeliverablesReviewLocation
        if case .known(let value) = requestedLocation,
            selectedVersion.knownLocation != value
        {
            anchoredLocation = .unknown(
                selectedVersion.limitation ?? "所选位置不能可靠映射到该版本。"
            )
        } else {
            anchoredLocation = requestedLocation
        }
        let currentDraft = draft
        replaceDraft(
            deliverableID: selectedDeliverableID,
            versionID: selectedVersionID,
            location: anchoredLocation,
            recipient: currentDraft.recipient,
            text: currentDraft.text
        )
        feedback = "草稿输入与收件人已保留，并锚定当前成果和版本；查看其他版本不会静默改写依据。"
    }

    @discardableResult
    func beginCorrection(_ feedbackID: String) -> Bool {
        guard let original = feedback(id: feedbackID) else {
            return reject("原意见不存在；没有创建无来源的更正。")
        }
        replaceDraft(
            deliverableID: original.deliverableID,
            versionID: original.versionID,
            location: original.location,
            recipient: original.recipient,
            correctionOf: original.id
        )
        selectedFeedbackID = original.id
        feedback = "更正草稿沿用原版本、位置和收件人；提交后追加，不重写原意见。"
        return true
    }

    func setDraftText(_ text: String) {
        draft.text = text
    }

    func setDraftLocation(_ location: TaskDeliverablesReviewLocation) {
        draft.location = location
    }

    func setDraftLocationChoice(_ choice: TaskDeliverablesReviewLocationChoice) {
        let anchorVersion = draft.versionID.flatMap { version($0) } ?? selectedVersion
        switch choice {
        case .whole:
            draft.location = .whole
        case .known:
            guard let location = anchorVersion?.knownLocation else {
                draft.location = .unknown(
                    anchorVersion?.limitation ?? "当前版本没有可靠位置"
                )
                return
            }
            draft.location = .known(location)
        case .unknown:
            draft.location = .unknown(
                anchorVersion?.limitation ?? "当前版本位置无法可靠映射"
            )
        }
    }

    func setDraftRecipient(_ recipient: TaskDeliverablesReviewRecipient) {
        draft.recipient = recipient
    }

    func cancelFeedback() {
        replaceDraft()
        feedback = "草稿已取消；没有发送修改要求，也没有创建交办意图。"
    }

    @discardableResult
    func sendFeedback(_ captured: TaskDeliverablesReviewDraft? = nil) -> Bool {
        let submitted = captured ?? draft
        guard submitted.owner == identity, submitted == draft else {
            return reject("草稿已变化、取消、重置或来自另一窗口；没有发送。")
        }
        let text = submitted.text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return reject("空意见不会形成交办。") }
        guard let deliverableID = submitted.deliverableID,
            let versionID = submitted.versionID,
            deliverable(deliverableID) != nil,
            version(versionID) != nil
        else {
            return reject("发送前必须保留可核对的成果和版本关联。")
        }
        let id = "feedback-\(nextFeedbackID)"
        nextFeedbackID += 1
        let state: TaskDeliverablesReviewFeedbackState =
            submitted.correctionOf == nil ? .sent : .correctionSent
        let item = TaskDeliverablesReviewFeedback(
            id: id,
            taskID: "task-harbor-deliverables",
            deliverableID: deliverableID,
            versionID: versionID,
            location: submitted.location,
            text: text,
            recipient: submitted.recipient,
            state: state,
            correctionOf: submitted.correctionOf
        )
        feedbacks.append(item)
        selectedFeedbackID = id
        record(
            submitted.correctionOf == nil
                ? "明确发送修改要求：\(id) · \(submitted.recipient.title)"
                : "追加关联更正：\(id) → \(submitted.correctionOf ?? "未知原意见")"
        )
        replaceDraft(recipient: .coordinator)
        feedback = "已形成一次本地交办意图；不等于已交付、已消费、已修复或已验收。"
        refreshRounds()
        return true
    }

    @discardableResult
    func recordOperation(_ kind: TaskDeliverablesReviewOperationKind) -> Bool {
        guard let versionID = selectedVersionID, version(versionID) != nil else {
            return reject("没有选定可查看的交付版本；未记录操作意图。")
        }
        let id = "operation-\(nextOperationID)"
        nextOperationID += 1
        operations.append(
            TaskDeliverablesReviewOperationIntent(
                id: id,
                versionID: versionID,
                kind: kind,
                state: "未执行（示例）",
                detail: "\(kind.title) 仅被记录为明确意图；没有运行、安装、外部打开、联网或上传。"
            )
        )
        record("本地操作意图：\(kind.title) \(versionID)，未执行。")
        feedback = "\(kind.title) 意图已记录；没有显示真实成功回执。"
        return true
    }

    @discardableResult
    func loadAgentReport(for feedbackID: String) -> Bool {
        guard let item = feedback(id: feedbackID) else {
            return reject("未找到原意见；没有载入 Agent 自报。")
        }
        guard
            !evidence.contains(where: {
                $0.feedbackID == feedbackID && $0.kind == .agentReport
            })
        else {
            return reject("该示例自报已载入；不重复制造处理说明。")
        }
        appendEvidence(
            feedbackID: feedbackID,
            versionID: item.versionID,
            kind: .agentReport,
            detail: "示例 Agent 说明：已按意见修改；这是自报，不是核验或用户接受。"
        )
        feedback = "已载入 Agent 自报；它没有自动产生核验通过。"
        return true
    }

    @discardableResult
    func loadVerification(for feedbackID: String, versionID: String) -> Bool {
        guard let item = feedback(id: feedbackID),
            deliverable(item.deliverableID)?.versions.contains(where: { $0.id == versionID })
                == true
        else {
            return reject("反馈或同一成果的新版本不存在；没有载入核验结果。")
        }
        guard
            !evidence.contains(where: {
                $0.feedbackID == feedbackID && $0.versionID == versionID && $0.kind == .verification
            })
        else {
            return reject("该版本的示例核验已存在；不重复制造结果。")
        }
        appendEvidence(
            feedbackID: item.id,
            versionID: versionID,
            kind: .verification,
            detail: "示例核对结果：仅核对 \(versionID) 的固定范围；不代表全部意见结案。"
        )
        feedback = "已载入 \(versionID) 示例核验；自报、核验和用户接受仍分开。"
        return true
    }

    @discardableResult
    func loadUserAccepted(versionID: String) -> Bool {
        guard version(versionID) != nil else {
            return reject("版本不存在；没有制造历史用户接受事实。")
        }
        guard
            !evidence.contains(where: {
                $0.versionID == versionID && $0.kind == .userAccepted
            })
        else {
            return reject("该版本的历史接受事实已存在；不重复添加。")
        }
        appendEvidence(
            feedbackID: nil,
            versionID: versionID,
            kind: .userAccepted,
            detail: "预置历史事实：用户接受 \(versionID) 当时范围；不延伸到其他版本。"
        )
        feedback = "已载入明确标注的历史接受事实；它不覆盖新版本。"
        return true
    }

    @discardableResult
    func beginRework(for feedbackID: String) -> Bool {
        guard let item = feedback(id: feedbackID) else {
            return reject("返工必须关联已有意见；没有创建新任务。")
        }
        guard !rounds.contains(where: { $0.feedbackID == feedbackID }) else {
            return reject("该意见已有同任务返工轮次；不重复发送或开工。")
        }
        let state: TaskDeliverablesReviewRoundState =
            conditions.canContinue ? .readyToContinue : .blocked
        rounds.append(
            TaskDeliverablesReviewRound(
                id: "round-rework-\(rounds.count + 1)",
                taskID: item.taskID,
                feedbackID: item.id,
                sourceVersionID: item.versionID,
                state: state,
                detail: "同任务返工：关联 \(item.id) 和 \(item.versionID)；不创建真实任务、不恢复旧会话。"
            )
        )
        selectedFeedbackID = feedbackID
        record("明确发起同任务返工：\(feedbackID)；修改要求只发送一次。")
        feedback = "返工已形成同任务本地交办；当前状态由固定条件决定。"
        return true
    }

    @discardableResult
    func loadConditionEvent(_ event: TaskDeliverablesReviewConditionEvent) -> Bool {
        switch event {
        case .resourcesRestored: conditions.resourcesAvailable = true
        case .resourcesBlocked: conditions.resourcesAvailable = false
        case .authorizationRestored: conditions.authorized = true
        case .authorizationRevoked: conditions.authorized = false
        case .supervisionRestored: conditions.supervisionAvailable = true
        case .supervisionMissing: conditions.supervisionAvailable = false
        case .pause: conditions.paused = true
        case .resume: conditions.paused = false
        case .stopRequested: conditions.stopRequested = true
        case .stopVerified: conditions.stopVerified = true
        case .executionVerified: conditions.originalExecutionUnknown = false
        }
        record("载入示例条件事件：\(event.title)")
        refreshRounds()
        feedback = "已载入 \(event.title)；不启动进程、不发送重复交办。"
        return true
    }

    private func appendEvidence(
        feedbackID: String?,
        versionID: String,
        kind: TaskDeliverablesReviewEvidenceKind,
        detail: String
    ) {
        let id = "evidence-\(evidence.count + 1)"
        evidence.append(
            TaskDeliverablesReviewEvidence(
                id: id,
                feedbackID: feedbackID,
                versionID: versionID,
                kind: kind,
                detail: detail,
                isExampleEvent: true
            )
        )
        record("载入示例事件：\(kind.title) · \(versionID)", example: true)
    }

    private func refreshRounds() {
        for index in rounds.indices where rounds[index].state != .completed {
            rounds[index].state = conditions.canContinue ? .readyToContinue : .blocked
        }
    }

    private func replaceDraft(
        deliverableID: TaskDeliverableID? = nil,
        versionID: String? = nil,
        location: TaskDeliverablesReviewLocation = .whole,
        recipient: TaskDeliverablesReviewRecipient = .coordinator,
        correctionOf: String? = nil,
        text: String = ""
    ) {
        draftToken += 1
        draft = TaskDeliverablesReviewDraft(
            owner: identity,
            token: draftToken,
            deliverableID: deliverableID,
            versionID: versionID,
            location: location,
            recipient: recipient,
            correctionOf: correctionOf,
            text: text
        )
    }

    private func record(_ text: String, example: Bool = false) {
        records.append(
            TaskDeliverablesReviewRecord(id: nextRecordID, text: text, isExampleEvent: example)
        )
        nextRecordID += 1
    }

    @discardableResult
    private func reject(_ text: String) -> Bool {
        feedback = text
        return false
    }
}
#endif
