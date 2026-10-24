#if DEBUG
import Foundation
import Observation

/// A single window's finite rehearsal. No clients, persistence, timers,
/// subprocesses or callbacks. External facts only enter explicit sample methods.
@MainActor @Observable
final class TaskCommunicationReviewStore {
    private(set) var scenario: TCReviewScenario
    private(set) var messages: [TCReviewMessage]
    private(set) var questions: [TCReviewQuestion]
    private(set) var conditions: [TCReviewMember: TCReviewConditions]
    private(set) var alignments: [TCReviewAlignment] = []
    private(set) var records: [TCReviewRecord] = []
    private(set) var draft: TCReviewDraft
    private(set) var feedback = "所有操作只影响本窗口的虚构内存。"
    private(set) var generation = 0
    var viewedMember: TCReviewMember = .coordinator
    var selectedMessageID: Int?
    var appearance = TCReviewAppearance()
    @ObservationIgnored private let identity: UUID
    @ObservationIgnored private var draftToken = 0

    init(scenario: TCReviewScenario = .input) {
        let fixture = TaskCommunicationReviewFixtures.make(scenario)
        let identity = UUID()
        self.identity = identity
        self.scenario = scenario
        messages = fixture.messages
        questions = fixture.questions
        conditions = fixture.conditions
        selectedMessageID = fixture.messages.first?.id
        draft = TCReviewDraft(owner: identity, token: 0)
    }

    func message(_ id: Int) -> TCReviewMessage? { messages.first { $0.id == id } }
    func question(_ id: String) -> TCReviewQuestion? { questions.first { $0.id == id } }
    func answer(to id: String) -> TCReviewMessage? { messages.last { $0.questionID == id } }

    var effectiveRequirements: [TCReviewMessage] {
        messages.filter { $0.scope != nil && $0.supersededBy == nil && !$0.withdrawn }
    }

    var draftIsAnchored: Bool { draft.context != .compose }

    var composerTitle: String {
        switch draft.context {
        case .compose: draft.purpose.title
        case let .reply(id): "回答原问题 \(id)"
        case let .edit(id, _): "编辑仍可控的 M\(id)"
        case let .correction(id): "追加关联更正 M\(id)"
        }
    }

    var composerBoundary: String {
        switch draft.context {
        case .compose:
            draft.purpose == .nextRound
                ? "发送即后续交办；到合法轮次边界且条件满足后自动接续，不是只存草稿。"
                : "只记录待投递意图，等待执行器支持的安全边界；不强行打断工具，不承诺即时注入。"
        case let .reply(id):
            "始终回到 \(question(id)?.asker.title ?? "原提问者") 的原问题；查看其他成员不改投。普通问答不代批。"
        case .edit:
            "取消不应用；提交 / 撤回时重新核对可控性，不能用旧的排队显示承诺成功。"
        case .correction:
            "保留原文 / 来源 / 对象 / 交付与操作事实，只追加显式关联；不自动回滚或证明采纳。"
        }
    }

    var canConvertEditToCorrection: Bool {
        guard case let .edit(id, _) = draft.context, let original = message(id) else { return false }
        return !original.canEditOrWithdraw && original.canCorrect
    }

    func setDraftText(_ text: String) { draft.text = text }

    @discardableResult
    func setPurpose(_ purpose: TCReviewPurpose) -> Bool {
        guard !draftIsAnchored, purpose != .answer else {
            return reject("回答必须从原问题进入；已有答复 / 编辑 / 更正请先明确取消，不能静默脱锚。")
        }
        draft.purpose = purpose
        return true
    }

    @discardableResult
    func setRecipient(_ recipient: TCReviewMember) -> Bool {
        guard !draftIsAnchored else { return reject("当前收件人与原记录绑定；查看成员不会改投，请先明确取消当前动作。") }
        draft.recipient = recipient
        return true
    }

    func cancelComposer() {
        replaceDraft()
        feedback = "草稿 / 编辑 / 答复已取消；没有应用内容，也没有回答原问题。"
    }

    @discardableResult
    func beginReply(_ questionID: String) -> Bool {
        guard canBeginContext(), let question = question(questionID) else { return false }
        guard answer(to: questionID) == nil else { return reject("此问题已有答复意图；样例不重发或实现问题修订协议。") }
        replaceDraft(context: .reply(questionID), purpose: .answer, recipient: question.asker)
        feedback = "答复已锚定 \(questionID)，推荐方案未被自动选择。"
        return true
    }

    @discardableResult
    func beginEdit(_ id: Int) -> Bool {
        guard canBeginContext(), let original = message(id) else { return false }
        guard original.canEditOrWithdraw else { return reject("不再确认可控，或不是本样例支持编辑的普通消息；请追加关联更正。") }
        replaceDraft(context: .edit(id, revision: original.revision), text: original.text,
                     purpose: original.purpose, recipient: original.recipient)
        return true
    }

    @discardableResult
    func beginCorrection(_ id: Int) -> Bool {
        guard canBeginContext(), let original = message(id) else { return false }
        guard original.canCorrect else { return reject("此记录已撤回、已更正或属于原问题答复；不能制造含糊的重复替代。") }
        replaceDraft(context: .correction(id), purpose: original.purpose, recipient: original.recipient)
        feedback = "更正只关联 M\(id) 的对应范围，不覆盖其他要求。"
        return true
    }

    @discardableResult
    func convertEditToCorrection() -> Bool {
        guard canConvertEditToCorrection, case let .edit(id, _) = draft.context,
              let original = message(id) else { return reject("没有可转换的失去控制权编辑。") }
        let text = draft.text
        replaceDraft(context: .correction(id), text: text, purpose: original.purpose, recipient: original.recipient)
        feedback = "已明确改为追加关联更正；编辑文字保留，原消息不会覆盖。"
        return true
    }

    /// The optional captured value tests cancelled/reset/stale UI drafts.
    /// Tokens are only local lifetime guards, not an atomic delivery protocol.
    @discardableResult
    func submitDraft(_ captured: TCReviewDraft? = nil) -> Bool {
        let submitted = captured ?? draft
        guard submitted.owner == identity, submitted == draft else {
            return reject("此草稿已变化、取消、重置或来自另一窗口；没有提交。")
        }
        let text = submitted.text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return reject("空白草稿未生成消息。") }

        switch submitted.context {
        case .compose:
            guard submitted.purpose != .answer else { return reject("未关联原问题，不能作为回答发送。") }
            appendMessage(text, recipient: submitted.recipient, purpose: submitted.purpose)
        case let .reply(id):
            guard let question = question(id), question.asker == submitted.recipient,
                  submitted.purpose == .answer, answer(to: id) == nil else {
                return reject("原问题 / 提问者已变化，或已有答复；没有静默改投。")
            }
            appendMessage(text, recipient: question.asker, purpose: .answer, questionID: id, scope: question.scope)
        case let .edit(id, revision):
            guard let index = messages.firstIndex(where: { $0.id == id }),
                  messages[index].canEditOrWithdraw, messages[index].revision == revision,
                  submitted.recipient == messages[index].recipient,
                  submitted.purpose == messages[index].purpose else {
                return reject("事实已变化：不能覆盖原记录或宣称撤回成功。保留编辑文字，可明确转为关联更正。")
            }
            messages[index].text = text
            messages[index].revision += 1
            record("本地意图：修改仍可控的 M\(id)，没有追加另一条旧草稿。")
        case let .correction(id):
            guard let index = messages.firstIndex(where: { $0.id == id }),
                  messages[index].canCorrect, submitted.recipient == messages[index].recipient,
                  submitted.purpose == messages[index].purpose else {
                return reject("原关联对象已经变化；更正未提交，不自动选另一个对象。")
            }
            let scope = messages[index].scope
            let newID = appendMessage(text, recipient: submitted.recipient, purpose: submitted.purpose,
                                      correctionOf: id, scope: scope)
            // Only the explicit content relation changes. Never rewrite facts.
            messages[index].supersededBy = newID
            messages[index].revision += 1
            alignments += TaskCommunicationReviewFixtures.affectedMembers(for: scope).map {
                TCReviewAlignment(correctionID: newID, member: $0)
            }
        }
        // Evaluate only after the complete local mutation, especially after a
        // correction has superseded its original. An existing valid boundary
        // must not require another fake external event just to notice new work.
        advanceIfReady(submitted.recipient)
        let recipient = submitted.context == .compose ? submitted.recipient : .coordinator
        replaceDraft(recipient: recipient)
        feedback = "已记下本地示例意图；没有因此证明交付、消费、采纳或通过。"
        return true
    }

    @discardableResult
    func withdraw(_ id: Int) -> Bool {
        guard let index = messages.firstIndex(where: { $0.id == id }), messages[index].canEditOrWithdraw else {
            return reject("撤回未确认：控制权 / 交付事实不满足条件，原记录保留；可查看关联更正。")
        }
        messages[index].withdrawn = true
        messages[index].ownerConfirmed = false
        messages[index].revision += 1
        if case let .edit(editID, _) = draft.context, editID == id { replaceDraft() }
        record("本地意图：已撤回仍可控的 M\(id)（示例），保留原记录。")
        advanceIfReady(messages[index].recipient)
        feedback = "已撤回（示例）；只因确认仍可控，不能由删除列表推定成功。"
        return true
    }

    func deferQuestion(_ id: String) {
        guard let index = questions.firstIndex(where: { $0.id == id }) else { return }
        questions[index].deferred = true
        feedback = "稍后查看不等于同意推荐；真正依赖答案的部分仍等待。"
    }

    func answerSummary(_ id: String) -> String {
        guard let question = question(id) else { return "原问题未找到" }
        guard let answer = answer(to: id) else {
            return question.deferred ? "稍后处理 · 仍未回答，不视为同意" : "未回答 · 推荐方案尚未获同意"
        }
        if question.availableEvidence != nil { return "示例答案可用；关联工作仍需核对，不是审批 / 完成 / 验收" }
        return "关联答复 M\(answer.id)：\(answer.delivery.title)；\(answer.consumptionSummary)"
    }

    @discardableResult
    func loadAnswerAvailable(_ questionID: String) -> Bool {
        guard let index = questions.firstIndex(where: { $0.id == questionID }),
              let answer = answer(to: questionID),
              answer.delivery == .handedOff || answer.consumed else {
            return reject("没有已交出 / 已消费的关联答复，不能用示例事件把未回答当同意。")
        }
        let evidence = "载入示例事件：\(questionID) 的关联答案已可用；只释放答案依赖，不代批。"
        questions[index].availableEvidence = evidence
        record(evidence, example: true)
        for member in TCReviewMember.allCases { advanceIfReady(member) }
        feedback = evidence
        return true
    }

    @discardableResult
    func loadMessageEvent(_ event: TCReviewMessageEvent, id: Int) -> Bool {
        guard let index = messages.firstIndex(where: { $0.id == id }), !messages[index].withdrawn else {
            return reject("示例事件未应用：没有有效的原记录，已撤回记录不重新送出。")
        }
        switch event {
        case .handedOff:
            guard messages[index].delivery != .notHandedOff || messages[index].supersededBy == nil else {
                return reject("未交出的过时要求已有明确更正，不再投递；原历史仍保留。")
            }
            guard messages[index].intent != .waitingNextRound || messages[index].delivery != .notHandedOff else {
                return reject("下一轮尚未推进到待处理；先载入合法轮次边界，不挤入当前工具。")
            }
            messages[index].delivery = .handedOff
            messages[index].ownerConfirmed = false
        case .deliveryUnknown:
            guard messages[index].delivery != .handedOff else { return reject("已有已交出事实，不能用未知抹掉。") }
            messages[index].delivery = .unknown
            messages[index].ownerConfirmed = false
        case .consumed:
            guard messages[index].delivery != .notHandedOff else { return reject("尚未交出，不能制造消费证据。") }
            messages[index].consumed = true
            messages[index].ownerConfirmed = false
        case .operationOccurred:
            guard messages[index].delivery != .notHandedOff else { return reject("尚未交出，不能制造旧操作事实。") }
            messages[index].operation = .occurred
        }
        messages[index].revision += 1
        let evidence = "载入示例事件：M\(id) · \(event.title)"
        messages[index].evidence.append(evidence)
        record(evidence, example: true)
        if let alignment = alignments.firstIndex(where: {
            $0.correctionID == id && $0.member == messages[index].recipient
        }) {
            if event == .handedOff { alignments[alignment].handedOff = true }
            if event == .consumed { alignments[alignment].consumed = true }
        }
        advanceIfReady(messages[index].recipient)
        feedback = evidence + "；不等于采纳、整改或验收。"
        return true
    }

    @discardableResult
    func loadAlignmentEvent(_ id: String, consumed: Bool = false) -> Bool {
        guard let index = alignments.firstIndex(where: { $0.id == id }),
              let correction = message(alignments[index].correctionID) else {
            return reject("未找到该局部同步关系。")
        }
        if alignments[index].member == correction.recipient {
            return loadMessageEvent(consumed ? .consumed : .handedOff, id: correction.id)
        }
        guard alignments[index].handedOff || correction.supersededBy == nil else {
            return reject("此同步内容已被更正，不再向未交出的成员投递过时要求；历史事实保留。")
        }
        if consumed {
            guard alignments[index].handedOff else { return reject("该成员的同步尚未交出，不能宣称消费。") }
            alignments[index].consumed = true
        } else {
            alignments[index].handedOff = true
        }
        let text = "载入示例事件：M\(correction.id) 仅向 \(alignments[index].member.shortTitle)"
            + (consumed ? "确认消费；仍需核对。" : "交出同步；其他成员的消费未知。")
        record(text, example: true)
        feedback = text
        return true
    }

    @discardableResult
    func loadGateEvent(_ event: TCReviewGateEvent, member: TCReviewMember) -> Bool {
        var state = conditions[member] ?? TCReviewConditions()
        switch event {
        case .roundEnded: state.boundary += 1; state.atBoundary = true
        case .resourceBlocked: state.resourceAvailable = false
        case .resourceRestored: state.resourceAvailable = true
        case .authorizationMissing: state.authorized = false
        case .authorizationAvailable: state.authorized = true
        case .approvalMissing: state.approvalSatisfied = false
        case .approvalAvailable: state.approvalSatisfied = true
        case .supervisionMissing: state.supervisionSatisfied = false
        case .supervisionAvailable: state.supervisionSatisfied = true
        case .capabilityUnknown: state.capabilityKnown = false
        case .capabilityKnown: state.capabilityKnown = true
        case .waitForAnswer: state.answerQuestionID = questions.first(where: \.important)?.id
        case .operationUnknown: state.operationUnknown = true
        case .operationVerified: state.operationUnknown = false
        case .authorizationRevoked: state.authorizationRevoked = true; state.authorized = false
        case .queueCancelled: state.queueCancelled = true
        case .stopVerified:
            guard state.stopRequested else { return reject("没有 Stop 请求，不能把事件当成已发出的停止控制。") }
            state.stopVerified = true
        }
        conditions[member] = state
        let text = "载入示例事件：\(member.shortTitle) · \(event.title)"
        record(text, example: true)
        advanceIfReady(member)
        feedback = text + "；只更新虚构条件，不改变真实授权或会话。"
        return true
    }

    func pause(_ member: TCReviewMember) {
        conditions[member]?.paused = true
        record("本地控制意图：暂停 \(member.shortTitle) 的新增接续；不声称在途工具已停。")
        feedback = "已暂停新增接续（示例）；资源恢复 / 本轮结束不会自动唤醒。重置只重置样例。"
    }

    func requestStop(_ member: TCReviewMember) {
        conditions[member]?.stopRequested = true
        record("本地控制意图：Stop \(member.shortTitle)，尚未核实在途停止，保留队列 / 历史。")
        feedback = "Stop 请求已记下（示例）；禁止新增接续，已停需单独示例事实；无真实信号或回滚。"
    }

    func continuationBlockers(_ member: TCReviewMember, includeBoundary: Bool = true) -> [String] {
        let state = conditions[member] ?? TCReviewConditions()
        var reasons = state.controlBlockers
        if state.operationUnknown || messages.contains(where: {
            $0.recipient == member && !$0.withdrawn && ($0.delivery == .unknown || $0.operation == .unknown)
        }) { reasons.append("交付 / 原操作未知，先核实，不盲重发") }
        if messages.contains(where: {
            $0.recipient == member && $0.intent == .followUpPending && $0.delivery == .notHandedOff
                && !$0.withdrawn && $0.supersededBy == nil
        }) { reasons.append("上一条后续意图尚未确认交出，保留顺序") }
        if !state.capabilityKnown { reasons.append("执行器接续能力不明，等待可靠安全边界") }
        if !state.authorized { reasons.append("缺少授权") }
        if !state.approvalSatisfied { reasons.append("必需审批未齐，普通问答不代批") }
        if !state.supervisionSatisfied { reasons.append("必需监督证据未齐") }
        let unanswered = questions.filter {
            ($0.dependentMembers.contains(member) || $0.id == state.answerQuestionID) && $0.availableEvidence == nil
        }
        if !unanswered.isEmpty { reasons.append("等待答案可用：\(unanswered.map(\.id).joined(separator: "、"))") }
        if !state.resourceAvailable { reasons.append("必需资源不足") }
        if includeBoundary && (!state.atBoundary || state.boundary <= state.usedBoundary) {
            reasons.append("本轮尚未到新的可接收输入边界")
        }
        return reasons
    }

    func queue(_ member: TCReviewMember) -> [TCReviewMessage] {
        messages.filter {
            $0.recipient == member && $0.purpose == .nextRound && !$0.withdrawn && $0.supersededBy == nil
        }
    }

    func workSummary(_ member: TCReviewMember) -> String {
        let blockers = continuationBlockers(member, includeBoundary: false)
        if !blockers.isEmpty { return "\(member.work)：\(blockers.joined(separator: "；"))。仅约束相关部分，不等于全队停止。" }
        if member.isReadOnly { return member.roleBoundary }
        if alignments.contains(where: {
            $0.member == member && message($0.correctionID)?.supersededBy == nil
        }) { return "\(member.work)：受共享更正影响，待局部核对；同步 / 消费不等于按新要求完成。" }
        if questions.contains(where: { $0.dependentMembers.contains(member) && $0.availableEvidence != nil }) {
            return "\(member.work)：关联答案示例已可用，工作仍待核对；不代表整改 / 完成 / 验收。"
        }
        return "\(member.work)：既定授权内的安全独立工作可继续；不是完成或验收回执。"
    }

    func loadScenario(_ scenario: TCReviewScenario) {
        let fixture = TaskCommunicationReviewFixtures.make(scenario)
        self.scenario = scenario
        messages = fixture.messages
        questions = fixture.questions
        conditions = fixture.conditions
        alignments = []
        records = []
        viewedMember = .coordinator
        selectedMessageID = fixture.messages.first?.id
        generation += 1
        replaceDraft()
        feedback = "已重置为 \(scenario.rawValue) 初始虚构 fixture；所有旧草稿 / 问答 / 队列 / 事件已丢弃。"
    }

    func reset() { loadScenario(scenario) }

    // MARK: - Small, synchronous local transitions

    private func advanceIfReady(_ member: TCReviewMember) {
        guard continuationBlockers(member).isEmpty,
              let index = messages.firstIndex(where: {
                  $0.recipient == member && $0.purpose == .nextRound && $0.intent == .waitingNextRound
                      && $0.ownerConfirmed && $0.delivery == .notHandedOff && !$0.withdrawn && $0.supersededBy == nil
              }) else { return }
        messages[index].intent = .followUpPending
        var state = conditions[member] ?? TCReviewConditions()
        state.usedBoundary = state.boundary
        state.atBoundary = false
        conditions[member] = state
        record("示例规则自动接续：\(member.shortTitle) 的 M\(messages[index].id) 进入后续待处理；"
               + "无需再次批准，其余 FIFO 内容保留，交付 / 消费仍无证据。")
    }

    @discardableResult
    private func appendMessage(
        _ text: String, recipient: TCReviewMember, purpose: TCReviewPurpose,
        questionID: String? = nil, correctionOf: Int? = nil, scope: TCReviewScope? = nil
    ) -> Int {
        let id = (messages.map(\.id).max() ?? 0) + 1
        messages.append(TCReviewMessage(
            id: id, text: text, recipient: recipient, purpose: purpose, source: .human,
            sourceDetail: "用户明确发送的本地示例意图", questionID: questionID, correctionOf: correctionOf, scope: scope,
            intent: purpose == .nextRound ? .waitingNextRound : .pendingCurrent,
            delivery: .notHandedOff, ownerConfirmed: true
        ))
        selectedMessageID = id
        record("本地意图：M\(id) · \(purpose.title) → \(recipient.shortTitle)；未声明交付。")
        return id
    }

    private func replaceDraft(
        context: TCReviewDraftContext = .compose, text: String = "",
        purpose: TCReviewPurpose = .current, recipient: TCReviewMember = .coordinator
    ) {
        draftToken += 1
        draft = TCReviewDraft(owner: identity, token: draftToken, context: context,
                              text: text, purpose: purpose, recipient: recipient)
    }

    private func canBeginContext() -> Bool {
        guard draft.context == .compose, draft.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return reject("先明确发送或取消当前草稿 / 答复 / 编辑，再切换关联动作。")
        }
        return true
    }

    private func record(_ text: String, example: Bool = false) {
        records.append(TCReviewRecord(id: records.count + 1, text: text, isExampleEvent: example))
    }

    @discardableResult
    private func reject(_ reason: String) -> Bool { feedback = reason; return false }
}
#endif
