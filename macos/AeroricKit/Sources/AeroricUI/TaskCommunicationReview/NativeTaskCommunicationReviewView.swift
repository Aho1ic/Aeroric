#if DEBUG
import SwiftUI

/// One offline window owns one Store. No previews, product environment objects,
/// persistence or automatic events are installed by this view.
@MainActor
public struct NativeTaskCommunicationReviewView: View {
    @State private var store: TaskCommunicationReviewStore
    @State private var pendingWithdrawal: Int?
    @Environment(\.accessibilityReduceTransparency) private var systemTransparency
    @Environment(\.colorSchemeContrast) private var systemContrast
    @Environment(\.accessibilityReduceMotion) private var systemMotion

    public init() {
        _store = State(initialValue: TaskCommunicationReviewStore())
    }

    private var accessibility: TCReviewAccessibility {
        TCReviewAccessibility(
            appearance: store.appearance, systemTransparency: systemTransparency,
            systemContrast: systemContrast == .increased, systemMotion: systemMotion
        )
    }

    private var colorScheme: ColorScheme? {
        switch store.appearance.colorScheme {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }

    public var body: some View {
        GeometryReader { geometry in
            VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                header
                ScrollView(.vertical) {
                    VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                        scenarioControls
                        if geometry.size.width >= 920 {
                            HStack(alignment: .top, spacing: NativePanelGeometry.panelGap) {
                                conversation.frame(maxWidth: .infinity)
                                context.frame(width: min(380, geometry.size.width * 0.34))
                            }
                        } else {
                            conversation
                            context
                        }
                        appearanceControls
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.bottom, NativePanelGeometry.panelInset)
                }
            }
            .padding(NativePanelGeometry.panelInset)
            .foregroundStyle(AeroricColor.text)
            .background {
                NativePanelFoundation(
                    preference: accessibility.surface,
                    reduceTransparency: accessibility.reduceTransparency
                )
            }
            .animation(
                accessibility.reduceMotion ? nil : .easeInOut(duration: AeroricMotion.quick),
                value: store.selectedMessageID
            )
            .transaction { transaction in
                if accessibility.reduceMotion {
                    transaction.animation = nil
                    transaction.disablesAnimations = true
                }
            }
            .onChange(of: store.generation) { _, _ in pendingWithdrawal = nil }
            .onChange(of: store.selectedMessageID) { _, _ in pendingWithdrawal = nil }
        }
        .preferredColorScheme(colorScheme)
    }

    private var header: some View {
        TCReviewPanel("星图项目 · 执行中沟通", id: "header", accessibility: accessibility) {
            TCReviewText(TaskCommunicationReviewFixtures.offlineNotice, id: "offline", color: AeroricColor.accent)
            TCReviewText("未启动 / 未经 UI 验收 · 本窗口仅供后续获准的离线审阅",
                         id: "unaccepted", color: AeroricColor.textSecondary)
        }
    }

    private var scenarioControls: some View {
        TCReviewPanel("有限场景", id: "scenario", accessibility: accessibility) {
            HStack {
                Picker(selection: Binding(
                    get: { store.scenario },
                    set: { store.loadScenario($0) }
                )) {
                    ForEach(TCReviewScenario.allCases) { scenario in
                        Text("\(scenario.rawValue) · \(scenario.title)")
                            .tag(scenario)
                            .accessibilityIdentifier("taskCommunicationReview.scenario.\(scenario.id)")
                    }
                } label: {
                    Text("载入初始场景")
                        .accessibilityIdentifier("taskCommunicationReview.scenario.menu")
                }
                .pickerStyle(.menu)
                .frame(maxWidth: .infinity, alignment: .leading)
                NativeIconButton(
                    systemImage: "arrow.counterclockwise", label: "重置本场景",
                    identifier: "taskCommunicationReview.reset",
                    help: "丢弃本窗口草稿和示例事件，不影响真实业务或其他窗口"
                ) { store.reset() }
            }
            TCReviewText(store.scenario.instruction, id: "scenario.instruction")
        }
    }

    private var conversation: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            composer
            messageHistory
            decisions
        }
    }

    private var composer: some View {
        TCReviewPanel("统一输入 · \(store.composerTitle)", id: "composer", accessibility: accessibility) {
            if store.draftIsAnchored {
                TCReviewText("用途：\(store.draft.purpose.title) · 收件人：\(store.draft.recipient.title)",
                             id: "composer.anchor", color: AeroricColor.accent)
            } else {
                NativeSegmentedPicker(
                    selection: Binding(get: { store.draft.purpose }, set: { _ = store.setPurpose($0) }),
                    groupTitle: "输入用途（答复从原问题进入）",
                    segments: [
                        .init(value: TCReviewPurpose.current, title: "补充当前工作"),
                        .init(value: .nextRound, title: "留到下一轮"),
                    ]
                )
                Picker(selection: Binding(
                    get: { store.draft.recipient }, set: { _ = store.setRecipient($0) }
                )) {
                    ForEach(TCReviewMember.allCases) { member in
                        Text(member.title).tag(member)
                            .accessibilityIdentifier("taskCommunicationReview.recipient.\(member.id)")
                    }
                } label: {
                    Text("明确收件人")
                        .accessibilityIdentifier("taskCommunicationReview.recipient.menu")
                }
                .pickerStyle(.menu)
            }
            TCReviewText(store.composerBoundary, id: "composer.boundary", color: AeroricColor.textSecondary)
            TextEditor(text: Binding(get: { store.draft.text }, set: { store.setDraftText($0) }))
                .font(.body)
                .scrollContentBackground(.hidden)
                .padding(NativePanelGeometry.panelInset)
                .frame(minHeight: 100, maxHeight: 160)
                .background(AeroricColor.layer1, in: RoundedRectangle(cornerRadius: AeroricRadius.s))
                .overlay {
                    RoundedRectangle(cornerRadius: AeroricRadius.s)
                        .strokeBorder(AeroricColor.separator2, lineWidth: accessibility.increaseContrast ? 1 : 0.5)
                        .allowsHitTesting(false)
                }
                .accessibilityLabel("消息草稿，仅为本窗口示例")
                .accessibilityIdentifier("taskCommunicationReview.composer.text")
            ViewThatFits(in: .horizontal) {
                HStack { composerButtons }.fixedSize(horizontal: true, vertical: false)
                VStack(alignment: .leading) { composerButtons }
            }
            .modifier(TCReviewShortActions(accessibility: accessibility))
            if store.canConvertEditToCorrection {
                Button("保留文字，改为追加更正（示例）") { store.convertEditToCorrection() }
                    .modifier(TCReviewShortActions(accessibility: accessibility))
                    .accessibilityIdentifier("taskCommunicationReview.composer.convertCorrection")
            }
            TCReviewReading(accessibility: accessibility) {
                TCReviewText(store.feedback, id: "feedback")
            }
        }
    }

    @ViewBuilder private var composerButtons: some View {
        Button(submitTitle) { store.submitDraft() }
            .disabled(store.draft.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            .keyboardShortcut(.return, modifiers: .command)
            .accessibilityIdentifier("taskCommunicationReview.composer.submit")
        Button(pendingWithdrawal == nil ? "取消当前草稿" : "取消撤回确认") {
            // Escape cancels the innermost pending interaction first. It must
            // not discard an unrelated composer while leaving confirmation open.
            if pendingWithdrawal != nil {
                pendingWithdrawal = nil
            } else {
                store.cancelComposer()
            }
        }
            .keyboardShortcut(.cancelAction)
            .accessibilityIdentifier("taskCommunicationReview.composer.cancel")
    }

    private var submitTitle: String {
        switch store.draft.context {
        case .compose:
            store.draft.purpose == .nextRound ? "排到下一轮（示例）" : "发送补充（示例）"
        case .reply: "发送答复（示例）"
        case .edit: "确认编辑（示例）"
        case .correction: "提交更正（示例）"
        }
    }

    private var messageHistory: some View {
        @Bindable var state = store
        return TCReviewPanel("工作记录 · 保留原文与事实", id: "history", accessibility: accessibility) {
            if store.messages.isEmpty {
                TCReviewText("尚无发送意图。空白草稿不会生成记录。", id: "history.empty")
            } else {
                NativeSelectionList(selection: $state.selectedMessageID) {
                    ForEach(store.messages) { message in
                        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                            Text("M\(message.id) · \(message.recipient.shortTitle) · \(message.purpose.title)")
                                .accessibilityIdentifier("taskCommunicationReview.message.\(message.id).row")
                            Text(message.withdrawn ? "已撤回（示例，历史保留）" : message.delivery.title)
                                .font(.caption)
                                .foregroundStyle(AeroricColor.textSecondary)
                                .accessibilityIdentifier("taskCommunicationReview.message.\(message.id).rowFact")
                        }
                        .tag(message.id)
                    }
                }
                .frame(height: min(180, CGFloat(store.messages.count) * 56 + 16))
                if let id = store.selectedMessageID, let message = store.message(id) {
                    messageDetails(message)
                }
            }
            DisclosureGroup("尚未明确替代的局部要求 · 冲突仍需澄清") {
                TCReviewReading(accessibility: accessibility) {
                    ForEach(store.effectiveRequirements) { message in
                        TCReviewText("M\(message.id) · \(message.scope?.title ?? "")：\(message.text)",
                                     id: "requirement.\(message.id)")
                    }
                    TCReviewText("此列表不证明采纳或完成；普通补充不覆盖其他要求。",
                                 id: "requirements.boundary", color: AeroricColor.textSecondary)
                }
            }
            DisclosureGroup("本窗口意图 / 示例事件依据（不是真实回执）") {
                TCReviewReading(accessibility: accessibility) {
                    ForEach(store.records) { record in
                        TCReviewText("\(record.isExampleEvent ? "示例事实" : "本地记录") · \(record.text)",
                                     id: "record.\(record.id)")
                    }
                }
            }
        }
    }

    private func messageDetails(_ message: TCReviewMessage) -> some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            TCReviewReading(accessibility: accessibility) {
                TCReviewText("M\(message.id)：\(message.text)", id: "message.\(message.id).text")
                TCReviewText("收件人：\(message.recipient.title) / \(message.recipient.work)\n用途：\(message.purpose.title)",
                             id: "message.\(message.id).target")
                TCReviewText(message.intent.title, id: "message.\(message.id).intent")
                TCReviewText(message.delivery.title + "；" + message.consumptionSummary,
                             id: "message.\(message.id).delivery",
                             color: message.delivery == .unknown ? AeroricColor.warning : AeroricColor.text)
                TCReviewText(message.controlSummary, id: "message.\(message.id).control")
                TCReviewText(message.relationSummary, id: "message.\(message.id).relation")
                DisclosureGroup("原来源与操作 / 事件依据") {
                    TCReviewText(message.source.title + "\n" + message.sourceDetail,
                                 id: "message.\(message.id).source")
                    TCReviewText(message.operation.title, id: "message.\(message.id).operation")
                    ForEach(Array(message.evidence.enumerated()), id: \.offset) { index, text in
                        TCReviewText(text, id: "message.\(message.id).evidence.\(index)")
                    }
                }
            }
            ViewThatFits(in: .horizontal) {
                HStack { messageButtons(message) }.fixedSize(horizontal: true, vertical: false)
                VStack(alignment: .leading) { messageButtons(message) }
            }
            .modifier(TCReviewShortActions(accessibility: accessibility))
            if pendingWithdrawal == message.id {
                TCReviewReading(accessibility: accessibility) {
                    TCReviewText("确认撤回 M\(message.id)？确认时将重新核对可控性；历史不会删除。",
                                 id: "withdraw.explanation")
                    Button("确认撤回（示例）", role: .destructive) {
                        store.withdraw(message.id)
                        pendingWithdrawal = nil
                    }
                    .accessibilityIdentifier("taskCommunicationReview.withdraw.confirm")
                    Button("保留原消息") { pendingWithdrawal = nil }
                        .accessibilityIdentifier("taskCommunicationReview.withdraw.cancel")
                }
                .modifier(TCReviewShortActions(accessibility: accessibility))
            }
            Menu {
                ForEach(TCReviewMessageEvent.allCases) { event in
                    Button(event.title) { store.loadMessageEvent(event, id: message.id) }
                        .accessibilityIdentifier("taskCommunicationReview.message.\(message.id).event.\(event.id)")
                }
            } label: {
                Text("载入 M\(message.id) 示例事件")
                    .accessibilityIdentifier("taskCommunicationReview.message.\(message.id).events")
            }
            .disabled(message.withdrawn)
            .modifier(TCReviewShortActions(accessibility: accessibility))
        }
    }

    @ViewBuilder private func messageButtons(_ message: TCReviewMessage) -> some View {
        Button("编辑草稿") { store.beginEdit(message.id) }
            .disabled(!message.canEditOrWithdraw)
            .accessibilityIdentifier("taskCommunicationReview.message.\(message.id).edit")
        Button("追加更正") { store.beginCorrection(message.id) }
            .disabled(!message.canCorrect)
            .accessibilityIdentifier("taskCommunicationReview.message.\(message.id).correct")
        Button("撤回…") { pendingWithdrawal = message.id }
            .disabled(!message.canEditOrWithdraw)
            .accessibilityIdentifier("taskCommunicationReview.message.\(message.id).withdraw")
    }

    private var decisions: some View {
        TCReviewPanel("待你决定 · 仅约束相关工作", id: "decisions", accessibility: accessibility) {
            ForEach(store.questions) { question in
                TCReviewReading(accessibility: accessibility) {
                    TCReviewText(question.important ? "⚠︎ 重要问题：只约束相关部分" : "普通问题：集中查看",
                                 id: "question.\(question.id).importance",
                                 color: question.important ? AeroricColor.warning : AeroricColor.textSecondary)
                    TCReviewText(question.text, id: "question.\(question.id).text")
                    TCReviewText("原提问者：\(question.asker.title) · \(question.work)",
                                 id: "question.\(question.id).asker")
                    TCReviewText(
                        question.availableEvidence == nil
                            ? question.impact
                            : "答案依赖已解除（示例），相关工作仍需核对；不是已整改、审批或验收，安全独立工作继续。",
                        id: "question.\(question.id).impact"
                    )
                    TCReviewText(store.answerSummary(question.id), id: "question.\(question.id).answer")
                    DisclosureGroup("依据与推荐 · 未回答不自动接受") {
                        TCReviewText(question.basis, id: "question.\(question.id).basis")
                        TCReviewText(question.recommendation + "\n理由：" + question.recommendationReason,
                                     id: "question.\(question.id).recommendation")
                        TCReviewText("原问题未答时的影响：" + question.impact,
                                     id: "question.\(question.id).originalImpact")
                        if let evidence = question.availableEvidence {
                            TCReviewText(evidence, id: "question.\(question.id).availableEvidence")
                        }
                        ForEach(question.conflictingMessages, id: \.self) { id in
                            Button("查看冲突原文 M\(id)") { store.selectedMessageID = id }
                                .accessibilityIdentifier("taskCommunicationReview.question.\(question.id).original.\(id)")
                        }
                    }
                    ViewThatFits(in: .horizontal) {
                        HStack { questionButtons(question) }.fixedSize(horizontal: true, vertical: false)
                        VStack(alignment: .leading) { questionButtons(question) }
                    }
                    .modifier(TCReviewShortActions(accessibility: accessibility))
                    if let answer = store.answer(to: question.id) {
                        TCReviewText("关联答复 M\(answer.id)：\(answer.text)", id: "question.\(question.id).replyText")
                        Menu {
                            Button("示例答复已交出") { store.loadMessageEvent(.handedOff, id: answer.id) }
                                .accessibilityIdentifier("taskCommunicationReview.question.\(question.id).handoff")
                            Button("示例答复已消费") { store.loadMessageEvent(.consumed, id: answer.id) }
                                .accessibilityIdentifier("taskCommunicationReview.question.\(question.id).consume")
                            Button("示例答案可用，仅释放答案依赖") { store.loadAnswerAvailable(question.id) }
                                .accessibilityIdentifier("taskCommunicationReview.question.\(question.id).available")
                        } label: {
                            Text("载入关联答复示例事件")
                                .accessibilityIdentifier("taskCommunicationReview.question.\(question.id).events")
                        }
                    }
                }
            }
            TCReviewText(TaskCommunicationReviewFixtures.permissionBoundary, id: "decisions.permission")
        }
    }

    @ViewBuilder private func questionButtons(_ question: TCReviewQuestion) -> some View {
        Button("回答原问题") { store.beginReply(question.id) }
            .disabled(store.answer(to: question.id) != nil)
            .accessibilityIdentifier("taskCommunicationReview.question.\(question.id).reply")
        Button("稍后查看") { store.deferQuestion(question.id) }
            .accessibilityIdentifier("taskCommunicationReview.question.\(question.id).defer")
    }

    private var context: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            memberContext
            localAlignment
            TCReviewPanel("边界说明", id: "boundaries", accessibility: accessibility) {
                TCReviewText(TaskCommunicationReviewFixtures.bindings, id: "bindings")
                TCReviewText(TaskCommunicationReviewFixtures.acceptanceBoundary, id: "acceptance")
                if store.appearance.advanced {
                    TCReviewText("advanced 只展开本地示例依据，不是另一个终端。没有会话、权限、模型、cwd 或 PTY generation 可被切换。",
                                 id: "advanced.boundary")
                }
            }
        }
    }

    private var memberContext: some View {
        @Bindable var state = store
        let member = store.viewedMember
        let conditions = store.conditions[member] ?? TCReviewConditions()
        return TCReviewPanel("查看成员 · 不改变输入收件人", id: "member", accessibility: accessibility) {
            Picker(selection: $state.viewedMember) {
                ForEach(TCReviewMember.allCases) { item in
                    Text(item.title).tag(item)
                        .accessibilityIdentifier("taskCommunicationReview.viewedMember.\(item.id)")
                }
            } label: {
                Text("仅查看上下文")
                    .accessibilityIdentifier("taskCommunicationReview.viewedMember.menu")
            }
            .pickerStyle(.menu)
            TCReviewText(member.roleBoundary, id: "member.role")
            TCReviewReading(accessibility: accessibility) {
                TCReviewText(store.workSummary(member), id: "member.work")
                TCReviewText("当前输入仍发给：\(store.draft.recipient.title)", id: "member.notRecipient")
                TCReviewText(
                    "授权条件：\(flag(conditions.authorized)) · 审批证据：\(flag(conditions.approvalSatisfied))\n"
                    + "监督证据：\(flag(conditions.supervisionSatisfied)) · 资源：\(flag(conditions.resourceAvailable))\n"
                    + "可接收能力已知：\(flag(conditions.capabilityKnown)) · 新边界：\(flag(conditions.atBoundary))",
                    id: "member.conditions"
                )
                ForEach(Array(store.continuationBlockers(member).enumerated()), id: \.offset) { index, blocker in
                    TCReviewText("等待：\(blocker)", id: "member.blocker.\(index)", color: AeroricColor.warning)
                }
                if store.appearance.advanced {
                    TCReviewText("本地边界序号 \(conditions.boundary) / 已用于接续 \(conditions.usedBoundary)。序号不是授权或生产回执。",
                                 id: "member.boundaryEvidence")
                }
            }
            TCReviewText("下一轮顺序（已交办，不是待再次发送的草稿）", id: "queue.title")
            if store.queue(member).isEmpty {
                TCReviewText("此成员没有有效的下一轮记录。", id: "queue.empty")
            }
            ForEach(store.queue(member)) { message in
                TCReviewText("M\(message.id) · \(message.text)\n\(message.intent.title)；\(message.delivery.title)",
                             id: "queue.\(message.id)")
            }
            ViewThatFits(in: .horizontal) {
                HStack { controlButtons(member) }.fixedSize(horizontal: true, vertical: false)
                VStack(alignment: .leading) { controlButtons(member) }
            }
            .modifier(TCReviewShortActions(accessibility: accessibility))
            Menu {
                ForEach(TCReviewGateEvent.allCases) { event in
                    Button(event.title) { store.loadGateEvent(event, member: member) }
                        .accessibilityIdentifier("taskCommunicationReview.gate.\(event.id)")
                }
            } label: {
                Text("载入此成员的示例条件事件")
                    .accessibilityIdentifier("taskCommunicationReview.gate.menu")
            }
            TCReviewText("这些是外部事实的虚构替身，不是生产流程新增的批准按钮。条件有效时 Store 自动评估接续；不发送信号、不重开会话。",
                         id: "gate.boundary", color: AeroricColor.textSecondary)
        }
    }

    @ViewBuilder private func controlButtons(_ member: TCReviewMember) -> some View {
        Button("暂停接续（示例）") { store.pause(member) }
            .accessibilityIdentifier("taskCommunicationReview.control.pause")
        Button("Stop 请求（示例）", role: .destructive) { store.requestStop(member) }
            .accessibilityIdentifier("taskCommunicationReview.control.stop")
    }

    private var localAlignment: some View {
        TCReviewPanel("共享影响 · 只同步实际受影响方", id: "alignment", accessibility: accessibility) {
            if store.alignments.isEmpty {
                TCReviewText("尚无共享接口更正。TC4 更正 M1 后会列出固定受影响方；不默认广播。",
                             id: "alignment.empty")
            }
            ForEach(store.alignments) { alignment in
                TCReviewReading(accessibility: accessibility) {
                    TCReviewText("M\(alignment.correctionID) → \(alignment.member.shortTitle)",
                                 id: "alignment.\(alignment.id).target")
                    TCReviewText(alignment.summary, id: "alignment.\(alignment.id).facts")
                    if store.message(alignment.correctionID)?.supersededBy != nil {
                        TCReviewText("历史同步关系：内容已被后续明确更正，保留原同步事实。",
                                     id: "alignment.\(alignment.id).historical")
                    }
                    Menu {
                        Button("示例仅向此成员交出") { store.loadAlignmentEvent(alignment.id) }
                            .accessibilityIdentifier("taskCommunicationReview.alignment.\(alignment.id).handoff")
                        Button("示例此成员已消费") { store.loadAlignmentEvent(alignment.id, consumed: true) }
                            .accessibilityIdentifier("taskCommunicationReview.alignment.\(alignment.id).consume")
                    } label: {
                        Text("载入此关系的示例事件")
                            .accessibilityIdentifier("taskCommunicationReview.alignment.\(alignment.id).events")
                    }
                }
            }
            TCReviewText(store.workSummary(.documentation), id: "alignment.independent")
        }
    }

    private var appearanceControls: some View {
        @Bindable var state = store
        return TCReviewPanel("仅本窗口外观", id: "appearance", accessibility: accessibility) {
            DisclosureGroup("材质 / 呈现 / 辅助偏好（不改变权限与绑定）") {
                VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                    NativeSegmentedPicker(
                        selection: $state.appearance.surface, groupTitle: "材质外观",
                        segments: [.init(value: SurfacePreference.glass, title: "glass"),
                                   .init(value: .classic, title: "classic")]
                    )
                    NativeSegmentedPicker(
                        selection: $state.appearance.advanced, groupTitle: "交互呈现，仅切换说明",
                        segments: [.init(value: false, title: "native"), .init(value: true, title: "advanced")]
                    )
                    Picker(selection: $state.appearance.colorScheme) {
                        ForEach(TCReviewColorScheme.allCases) { scheme in
                            Text(scheme.title).tag(scheme)
                                .accessibilityIdentifier("taskCommunicationReview.appearance.color.\(scheme.id)")
                        }
                    } label: {
                        Text("明暗演示").accessibilityIdentifier("taskCommunicationReview.appearance.color.menu")
                    }
                    .pickerStyle(.menu)
                    Toggle("演示：减少透明度", isOn: $state.appearance.reduceTransparency)
                        .toggleStyle(.checkbox)
                        .accessibilityIdentifier("taskCommunicationReview.appearance.transparency")
                    Toggle("演示：增强对比", isOn: $state.appearance.increaseContrast)
                        .toggleStyle(.checkbox)
                        .accessibilityIdentifier("taskCommunicationReview.appearance.contrast")
                    Toggle("演示：减少动效", isOn: $state.appearance.reduceMotion)
                        .toggleStyle(.checkbox)
                        .accessibilityIdentifier("taskCommunicationReview.appearance.motion")
                    TCReviewReading(accessibility: accessibility) {
                        TCReviewText(
                            "系统 / 演示 / 有效值\n"
                            + "减少透明度：\(flag(systemTransparency)) / \(flag(state.appearance.reduceTransparency)) / \(flag(accessibility.reduceTransparency))\n"
                            + "增强对比：\(flag(systemContrast == .increased)) / \(flag(state.appearance.increaseContrast)) / \(flag(accessibility.increaseContrast))\n"
                            + "减少动效：\(flag(systemMotion)) / \(flag(state.appearance.reduceMotion)) / \(flag(accessibility.reduceMotion))\n"
                            + "有效材料：\(accessibility.surface.rawValue)。演示只能加强系统偏好，不写入设置。",
                            id: "appearance.effective"
                        )
                    }
                }
                .padding(.top, NativePanelGeometry.panelInset)
            }
        }
    }

    private func flag(_ value: Bool) -> String { value ? "是" : "否" }
}

// Local composition only: existing geometry, semantic reading surfaces,
// native material and native buttons, not a second theme/selection system.
private struct TCReviewPanel<Content: View>: View {
    let title: String
    let id: String
    let accessibility: TCReviewAccessibility
    let content: Content

    init(_ title: String, id: String, accessibility: TCReviewAccessibility, @ViewBuilder content: () -> Content) {
        self.title = title
        self.id = id
        self.accessibility = accessibility
        self.content = content()
    }

    var body: some View {
        GlassPanel(preference: accessibility.surface, reduceTransparency: accessibility.reduceTransparency,
                   increaseContrast: accessibility.increaseContrast) {
            VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                Text(title)
                    .font(.headline)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("taskCommunicationReview.\(id).title")
                content
            }
            .padding(NativePanelGeometry.panelInset * 2)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

private struct TCReviewReading<Content: View>: View {
    let accessibility: TCReviewAccessibility
    let content: Content

    init(accessibility: TCReviewAccessibility, @ViewBuilder content: () -> Content) {
        self.accessibility = accessibility
        self.content = content()
    }

    var body: some View {
        let outline = NativePanelGeometry.outline(increasedContrast: accessibility.increaseContrast)
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) { content }
            .padding(NativePanelGeometry.panelInset)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(AeroricColor.layer1, in: RoundedRectangle(cornerRadius: AeroricRadius.s))
            .overlay {
                RoundedRectangle(cornerRadius: AeroricRadius.s)
                    .strokeBorder(AeroricColor.separator2.opacity(outline.opacity), lineWidth: outline.width)
                    .allowsHitTesting(false)
            }
    }
}

private struct TCReviewText: View {
    let text: String
    let id: String
    let color: Color

    init(_ text: String, id: String, color: Color = AeroricColor.text) {
        self.text = text
        self.id = id
        self.color = color
    }

    var body: some View {
        Text(text)
            .foregroundStyle(color)
            .fixedSize(horizontal: false, vertical: true)
            .textSelection(.enabled)
            .accessibilityIdentifier("taskCommunicationReview.\(id)")
    }
}

private struct TCReviewShortActions: ViewModifier {
    let accessibility: TCReviewAccessibility

    @ViewBuilder func body(content: Content) -> some View {
        if #available(macOS 26.0, *), accessibility.surface == .glass, !accessibility.reduceTransparency {
            content.buttonStyle(.glass).buttonBorderShape(.capsule)
        } else {
            content.buttonStyle(.bordered).buttonBorderShape(.capsule)
        }
    }
}
#endif
