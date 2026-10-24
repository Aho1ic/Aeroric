#if DEBUG
import SwiftUI

/// The real SwiftUI consumer for the finite task-deliverables specimen.
/// Every displayed fact and action comes from the window-local Store.
@MainActor
public struct NativeTaskDeliverablesReviewView: View {
    @State private var store: TaskDeliverablesReviewStore
    @State private var surface: SurfacePreference = .glass
    @State private var appearance: ReviewAppearanceOverride = .system
    @State private var injectReduceTransparency = false
    @State private var injectIncreaseContrast = false
    @State private var injectReduceMotion = false

    @Environment(\.accessibilityReduceTransparency) private var systemTransparency
    @Environment(\.colorSchemeContrast) private var systemContrast
    @Environment(\.accessibilityReduceMotion) private var systemMotion

    public init() {
        _store = State(initialValue: TaskDeliverablesReviewStore())
    }

    private var reduceTransparency: Bool {
        systemTransparency || injectReduceTransparency
    }

    private var increaseContrast: Bool {
        systemContrast == .increased || injectIncreaseContrast
    }

    private var reduceMotion: Bool {
        systemMotion || injectReduceMotion
    }

    private var effectiveSurface: SurfacePreference {
        SurfaceChoice.resolve(preference: surface, reduceTransparency: reduceTransparency)
    }

    public var body: some View {
        GeometryReader { geometry in
            VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                header
                ScrollView(.vertical) {
                    VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                        scenarioControls
                        if geometry.size.width >= 940 {
                            HStack(alignment: .top, spacing: NativePanelGeometry.panelGap) {
                                collectionPanel
                                    .frame(width: min(330, geometry.size.width * 0.27))
                                detailColumn
                                    .frame(maxWidth: .infinity)
                            }
                        } else {
                            collectionPanel
                            detailColumn
                        }
                        continuationPanel
                        appearanceControls
                    }
                    .padding(.bottom, NativePanelGeometry.panelInset)
                }
            }
            .padding(NativePanelGeometry.panelInset)
            .foregroundStyle(AeroricColor.text)
            .background {
                NativePanelFoundation(
                    preference: surface,
                    reduceTransparency: reduceTransparency
                )
            }
            .animation(
                reduceMotion ? nil : .easeInOut(duration: AeroricMotion.quick),
                value: store.selectedVersionID
            )
            .transaction { transaction in
                if reduceMotion {
                    transaction.animation = nil
                    transaction.disablesAnimations = true
                }
            }
        }
        .preferredColorScheme(appearance.colorScheme)
    }

    private var header: some View {
        reviewPanel("header") {
            VStack(alignment: .leading, spacing: 4) {
                Text(store.taskTitle)
                    .font(.title2.bold())
                    .accessibilityIdentifier("taskDeliverablesReview.task")
                Text(TaskDeliverablesReviewFixtures.offlineNotice)
                    .font(.system(size: AeroricType.secondary, weight: .semibold))
                    .foregroundStyle(AeroricColor.warning)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("taskDeliverablesReview.offlineNotice")
                Text(TaskDeliverablesReviewFixtures.acceptanceBoundary)
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("taskDeliverablesReview.acceptanceBoundary")
            }
        }
    }

    private var scenarioControls: some View {
        reviewPanel("scenario") {
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .top, spacing: NativePanelGeometry.panelGap) {
                    scenarioPicker
                    resetButton
                }
                VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                    scenarioPicker
                    resetButton
                }
            }
            Text(store.scenarioSummary)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("taskDeliverablesReview.scenario.instruction")
        }
    }

    private var scenarioPicker: some View {
        Picker(
            "离线成果场景",
            selection: Binding(
                get: { store.scenario },
                set: { store.loadScenario($0) }
            )
        ) {
            ForEach(TaskDeliverablesReviewScenario.allCases) { scenario in
                Text("\(scenario.rawValue) · \(scenario.title)")
                    .tag(scenario)
                    .accessibilityIdentifier("taskDeliverablesReview.scenario.\(scenario.id)")
            }
        }
        .pickerStyle(.menu)
        .accessibilityIdentifier("taskDeliverablesReview.scenario")
    }

    private var resetButton: some View {
        NativeIconButton(
            systemImage: "arrow.counterclockwise",
            label: "重置离线成果样例",
            identifier: "taskDeliverablesReview.reset",
            help: "丢弃本窗口内存变化并恢复固定虚构数据"
        ) {
            store.reset()
        }
    }

    private var collectionPanel: some View {
        reviewPanel("collection") {
            Text("成果清单")
                .font(.headline)
                .accessibilityIdentifier("taskDeliverablesReview.collection.title")
            if store.visibleDeliverables.isEmpty {
                Text("暂无已提交成果；不会编造安装包或成果。")
                    .foregroundStyle(AeroricColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("taskDeliverablesReview.collection.empty")
            } else {
                NativeSelectionList(selection: deliverableSelection) {
                    ForEach(store.visibleDeliverables) { item in
                        VStack(alignment: .leading, spacing: 3) {
                            Text(item.title)
                                .lineLimit(2)
                            Text("\(item.kind.title) · \(item.collectionState.title)")
                                .font(.system(size: AeroricType.secondary))
                                .foregroundStyle(AeroricColor.textSecondary)
                            Text("\(item.sourcePackage) · \(item.submittedBy)")
                                .font(.system(size: AeroricType.secondary))
                                .foregroundStyle(AeroricColor.textSecondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        .tag(item.id)
                        .accessibilityIdentifier(
                            "taskDeliverablesReview.deliverable.\(item.id.rawValue)"
                        )
                    }
                }
                .listStyle(.inset)
                .scrollContentBackground(.hidden)
                .frame(minHeight: 180, maxHeight: 270)
            }

            if !store.temporaryDeliverables.isEmpty {
                DisclosureGroup("未默认收录的临时记录") {
                    ForEach(store.temporaryDeliverables) { item in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(item.title)
                            Text(item.collectionState.title)
                                .font(.system(size: AeroricType.secondary))
                                .foregroundStyle(AeroricColor.textSecondary)
                            Button("明确加入示例清单") {
                                _ = store.includeExampleTemporaryRecord()
                            }
                            .accessibilityIdentifier(
                                "taskDeliverablesReview.temporary.include"
                            )
                        }
                        .padding(.vertical, 4)
                    }
                }
            }

            if let selected = store.selectedDeliverable {
                Text(selected.description)
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("taskDeliverablesReview.collection.description")
                ViewThatFits(in: .horizontal) {
                    HStack {
                        Button("整理标题 / 说明") {
                            _ = store.beginOrganizationEdit(selected.id)
                        }
                        Button("移出清单", role: .destructive) {
                            _ = store.removeFromCollection(selected.id)
                        }
                    }
                    VStack(alignment: .leading) {
                        Button("整理标题 / 说明") {
                            _ = store.beginOrganizationEdit(selected.id)
                        }
                        Button("移出清单", role: .destructive) {
                            _ = store.removeFromCollection(selected.id)
                        }
                    }
                }
                .modifier(shortActionStyle)
            }

            if let editID = store.organizationEditID,
                store.deliverable(editID) != nil
            {
                organizationEditor
            }

            if store.deliverables.contains(where: { $0.collectionState == .hidden }) {
                DisclosureGroup("已移出清单 · 原事实保留") {
                    ForEach(store.deliverables.filter { $0.collectionState == .hidden }) { item in
                        Text("\(item.title) · \(item.collectionState.title)")
                            .font(.system(size: AeroricType.secondary))
                            .foregroundStyle(AeroricColor.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
        }
    }

    private var organizationEditor: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            Text("本地整理草稿")
                .font(.subheadline.weight(.semibold))
            TextField(
                "展示标题",
                text: Binding(
                    get: { store.organizationTitleDraft },
                    set: { store.setOrganizationTitleDraft($0) }
                )
            )
            .textFieldStyle(.roundedBorder)
            .accessibilityIdentifier("taskDeliverablesReview.organization.title")
            TextField(
                "展示说明",
                text: Binding(
                    get: { store.organizationDescriptionDraft },
                    set: { store.setOrganizationDescriptionDraft($0) }
                )
            )
            .textFieldStyle(.roundedBorder)
            .accessibilityIdentifier("taskDeliverablesReview.organization.description")
            HStack {
                Button("应用整理") {
                    _ = store.commitOrganizationEdit()
                }
                Button("取消整理") {
                    store.cancelOrganizationEdit()
                }
                .keyboardShortcut(.cancelAction)
            }
            .modifier(shortActionStyle)
        }
        .padding(.top, AeroricSpace.m)
        .accessibilityIdentifier("taskDeliverablesReview.organization.editor")
    }

    private var detailColumn: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            versionPanel
            feedbackPanel
        }
    }

    private var versionPanel: some View {
        reviewPanel("version") {
            Text("交付版本与阅读面")
                .font(.headline)
                .accessibilityIdentifier("taskDeliverablesReview.version.title")
            if let selected = store.selectedDeliverable {
                Picker("选定交付版本", selection: selectedVersionSelection) {
                    ForEach(selected.versions) { version in
                        Text("\(version.displayTitle) · \(version.source)")
                            .tag(version.id)
                    }
                }
                .pickerStyle(.menu)
                .accessibilityIdentifier("taskDeliverablesReview.version.picker")

                if let version = store.selectedVersion {
                    Text("\(version.displayTitle) · \(version.contentSummary)")
                        .font(.subheadline.weight(.semibold))
                        .accessibilityIdentifier("taskDeliverablesReview.version.selected")
                    Text("来源：\(version.source)")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                    Text(version.readback.title)
                        .font(.system(size: AeroricType.secondary, weight: .medium))
                        .foregroundStyle(
                            version.readback == .embedded ? AeroricColor.accent : AeroricColor.warning
                        )
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("taskDeliverablesReview.version.readback")

                    readingSurface(version)
                    checks(for: version)

                    if let current = selected.currentWorkFile {
                        DisclosureGroup("当前工作文件示意 · 不是交付版本") {
                            Text(current.title)
                                .font(.subheadline.weight(.semibold))
                            Text(verbatim: current.content)
                                .textSelection(.enabled)
                                .fixedSize(horizontal: false, vertical: true)
                            Text(current.note)
                                .font(.system(size: AeroricType.secondary))
                                .foregroundStyle(AeroricColor.textSecondary)
                        }
                        .accessibilityIdentifier("taskDeliverablesReview.currentWorkFile")
                    }

                    operationButtons
                    if !store.operations.isEmpty {
                        DisclosureGroup("本窗口的操作意图（均未执行）") {
                            ForEach(store.operations) { operation in
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(
                                        "\(operation.kind.title) · \(operation.versionID) · \(operation.state)"
                                    )
                                    .font(.subheadline.weight(.medium))
                                    Text(operation.detail)
                                        .font(.system(size: AeroricType.secondary))
                                        .foregroundStyle(AeroricColor.textSecondary)
                                        .fixedSize(horizontal: false, vertical: true)
                                }
                            }
                        }
                        .accessibilityIdentifier("taskDeliverablesReview.operations")
                    }
                }
            } else {
                Text("请选择一个已收录成果。")
                    .foregroundStyle(AeroricColor.textSecondary)
            }
        }
    }

    @ViewBuilder
    private func readingSurface(_ version: TaskDeliverableVersion) -> some View {
        if let content = version.content {
            Text(verbatim: content)
                .font(.system(.body, design: .monospaced))
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(NativePanelGeometry.panelInset)
                .background(AeroricColor.layer1, in: RoundedRectangle(cornerRadius: AeroricRadius.s))
                .overlay {
                    RoundedRectangle(cornerRadius: AeroricRadius.s)
                        .strokeBorder(
                            AeroricColor.separator2,
                            lineWidth: increaseContrast ? 1 : 0.5
                        )
                        .allowsHitTesting(false)
                }
                .accessibilityIdentifier("taskDeliverablesReview.reading.content")
        } else {
            VStack(alignment: .leading, spacing: AeroricSpace.m) {
                Text("无法安全被动呈现该格式")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(AeroricColor.warning)
                Text(version.limitation ?? "没有可回看的内容；不调用外部解码器。")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .accessibilityIdentifier("taskDeliverablesReview.reading.unavailable")
        }
    }

    private func checks(for version: TaskDeliverableVersion) -> some View {
        DisclosureGroup("检查适用性与限制") {
            VStack(alignment: .leading, spacing: AeroricSpace.m) {
                ForEach(version.checks) { check in
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(check.title) · \(check.result.title)")
                            .font(.subheadline.weight(.medium))
                            .foregroundStyle(checkColor(check.result))
                        Text("范围：\(check.scope)\n依据：\(check.evidence)")
                            .font(.system(size: AeroricType.secondary))
                            .foregroundStyle(AeroricColor.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                if let limitation = version.limitation {
                    Text("限制：\(limitation)")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.warning)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .accessibilityIdentifier("taskDeliverablesReview.checks")
    }

    private var operationButtons: some View {
        ViewThatFits(in: .horizontal) {
            HStack {
                ForEach(TaskDeliverablesReviewOperationKind.allCases) { kind in
                    Button("\(kind.title)（未执行）") {
                        _ = store.recordOperation(kind)
                    }
                    .accessibilityIdentifier(
                        "taskDeliverablesReview.operation.\(kind.id)"
                    )
                }
            }
            VStack(alignment: .leading) {
                ForEach(TaskDeliverablesReviewOperationKind.allCases) { kind in
                    Button("\(kind.title)（未执行）") {
                        _ = store.recordOperation(kind)
                    }
                    .accessibilityIdentifier(
                        "taskDeliverablesReview.operation.\(kind.id)"
                    )
                }
            }
        }
        .modifier(shortActionStyle)
    }

    private var feedbackPanel: some View {
        reviewPanel("feedback") {
            Text("反馈、草稿与明确发送")
                .font(.headline)
                .accessibilityIdentifier("taskDeliverablesReview.feedback.title")
            Text(store.draftSummary)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.accent)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("taskDeliverablesReview.feedback.anchor")

            Picker("反馈位置", selection: draftLocationChoice) {
                ForEach(availableDraftLocationChoices) { choice in
                    Text(choice.title).tag(choice)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .accessibilityIdentifier("taskDeliverablesReview.feedback.location")

            Picker("明确收件人", selection: draftRecipient) {
                ForEach(TaskDeliverablesReviewRecipient.allCases) { recipient in
                    Text(recipient.title).tag(recipient)
                }
            }
            .pickerStyle(.menu)
            .accessibilityIdentifier("taskDeliverablesReview.feedback.recipient")

            TextEditor(text: draftText)
                .font(.body)
                .scrollContentBackground(.hidden)
                .frame(minHeight: 90, maxHeight: 150)
                .padding(NativePanelGeometry.panelInset)
                .background(AeroricColor.layer1, in: RoundedRectangle(cornerRadius: AeroricRadius.s))
                .accessibilityLabel("成果反馈草稿")
                .accessibilityIdentifier("taskDeliverablesReview.feedback.editor")

            ViewThatFits(in: .horizontal) {
                HStack {
                    Button("开始写反馈") {
                        store.beginFeedback()
                    }
                    .disabled(!store.canBeginFeedback)
                    Button("发送修改要求（示例）") {
                        _ = store.sendFeedback()
                    }
                    .disabled(!store.canSendDraft)
                    .keyboardShortcut(.return, modifiers: .command)
                    Button("取消草稿") {
                        store.cancelFeedback()
                    }
                    .keyboardShortcut(.cancelAction)
                }
                VStack(alignment: .leading) {
                    Button("开始写反馈") {
                        store.beginFeedback()
                    }
                    .disabled(!store.canBeginFeedback)
                    Button("发送修改要求（示例）") {
                        _ = store.sendFeedback()
                    }
                    .disabled(!store.canSendDraft)
                    Button("取消草稿") {
                        store.cancelFeedback()
                    }
                }
            }
            .modifier(shortActionStyle)

            if store.feedbacks.isEmpty {
                Text("尚无发送意图；空草稿不会生成反馈。")
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityIdentifier("taskDeliverablesReview.feedback.empty")
            } else {
                NativeSelectionList(selection: feedbackSelection) {
                    ForEach(store.feedbacks) { item in
                        VStack(alignment: .leading, spacing: 3) {
                            Text("\(item.id) · \(item.recipient.title)")
                            Text("\(item.versionID) · \(item.location.title) · \(item.state.title)")
                                .font(.system(size: AeroricType.secondary))
                                .foregroundStyle(AeroricColor.textSecondary)
                        }
                        .tag(item.id)
                        .accessibilityIdentifier(
                            "taskDeliverablesReview.feedback.\(item.id)"
                        )
                    }
                }
                .listStyle(.inset)
                .scrollContentBackground(.hidden)
                .frame(minHeight: 90, maxHeight: 160)
                if let item = store.selectedFeedback {
                    feedbackDetails(item)
                }
            }
            Text(store.feedback)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("taskDeliverablesReview.feedback.status")
        }
    }

    private func feedbackDetails(_ item: TaskDeliverablesReviewFeedback) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            Text(verbatim: item.text)
                .fixedSize(horizontal: false, vertical: true)
            Text("原关联：\(item.deliverableID.rawValue) / \(item.versionID) / \(item.location.title)")
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            ViewThatFits(in: .horizontal) {
                HStack {
                    Button("追加更正") {
                        _ = store.beginCorrection(item.id)
                    }
                    Button("发起同任务返工") {
                        _ = store.beginRework(for: item.id)
                    }
                }
                VStack(alignment: .leading) {
                    Button("追加更正") {
                        _ = store.beginCorrection(item.id)
                    }
                    Button("发起同任务返工") {
                        _ = store.beginRework(for: item.id)
                    }
                }
            }
            .modifier(shortActionStyle)
            Menu("载入反馈示例事件") {
                Button("Agent 说明已修改（示例）") {
                    _ = store.loadAgentReport(for: item.id)
                }
                ForEach(store.deliverable(item.deliverableID)?.versions ?? []) { version in
                    Button("载入 \(version.displayTitle) 示例核验") {
                        _ = store.loadVerification(for: item.id, versionID: version.id)
                    }
                }
                Button("历史用户接受当前版本（示例）") {
                    _ = store.loadUserAccepted(versionID: item.versionID)
                }
            }
            .accessibilityIdentifier("taskDeliverablesReview.feedback.events")
            ForEach(
                store.evidence.filter {
                    $0.feedbackID == item.id
                        || ($0.feedbackID == nil && $0.versionID == item.versionID)
                }
            ) { event in
                Text("\(event.kind.title)：\(event.detail)")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(
                        event.kind == .verification ? AeroricColor.accent : AeroricColor.textSecondary
                    )
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("taskDeliverablesReview.evidence.\(event.id)")
            }
        }
        .padding(.top, AeroricSpace.m)
    }

    private var continuationPanel: some View {
        reviewPanel("continuation") {
            Text("返工轮次与条件事实")
                .font(.headline)
                .accessibilityIdentifier("taskDeliverablesReview.continuation.title")
            Text(store.continuationSummary)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(
                    store.conditions.blockers.isEmpty ? AeroricColor.accent : AeroricColor.warning
                )
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("taskDeliverablesReview.continuation.summary")
            if !store.rounds.isEmpty {
                ForEach(store.rounds) { round in
                    VStack(alignment: .leading, spacing: 3) {
                        Text("\(round.id) · \(round.state.title)")
                            .font(.subheadline.weight(.medium))
                        Text(round.detail)
                            .font(.system(size: AeroricType.secondary))
                            .foregroundStyle(AeroricColor.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            Menu("载入条件示例事件") {
                ForEach(TaskDeliverablesReviewConditionEvent.allCases) { event in
                    Button(event.title) {
                        _ = store.loadConditionEvent(event)
                    }
                }
            }
            .accessibilityIdentifier("taskDeliverablesReview.conditions.events")
            DisclosureGroup("当前条件明细") {
                Text(
                    store.conditions.blockers.isEmpty
                        ? "授权、资源、监督、执行核实满足；仍不启动真实执行。"
                        : store.conditions.blockers.joined(separator: "\n")
                )
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            }
            DisclosureGroup("本窗口记录与示例依据") {
                ForEach(store.records) { record in
                    Text("\(record.isExampleEvent ? "示例事实" : "本地意图")：\(record.text)")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .accessibilityIdentifier("taskDeliverablesReview.records")
        }
    }

    private var appearanceControls: some View {
        reviewPanel("appearance") {
            Text("样例外观与辅助偏好")
                .font(.headline)
            ViewThatFits(in: .horizontal) {
                HStack {
                    Picker("面板外观", selection: $surface) {
                        Text("Glass").tag(SurfacePreference.glass)
                        Text("Classic").tag(SurfacePreference.classic)
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    Picker("颜色外观", selection: $appearance) {
                        ForEach([ReviewAppearanceOverride.system, .light, .dark], id: \.rawValue) {
                            value in
                            Text(value.title).tag(value)
                        }
                    }
                    .pickerStyle(.menu)
                }
                VStack(alignment: .leading) {
                    Picker("面板外观", selection: $surface) {
                        Text("Glass").tag(SurfacePreference.glass)
                        Text("Classic").tag(SurfacePreference.classic)
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    Picker("颜色外观", selection: $appearance) {
                        ForEach([ReviewAppearanceOverride.system, .light, .dark], id: \.rawValue) {
                            value in
                            Text(value.title).tag(value)
                        }
                    }
                    .pickerStyle(.menu)
                }
            }
            Toggle("示例：减少透明度", isOn: $injectReduceTransparency)
            Toggle("示例：增强对比", isOn: $injectIncreaseContrast)
            Toggle("示例：减少动效", isOn: $injectReduceMotion)
            Text(
                "系统辅助偏好始终优先；这些开关只强化本窗口示例，不修改系统设置。当前面板：\(effectiveSurface == .glass ? "Material" : "Classic")。"
            )
            .font(.system(size: AeroricType.secondary))
            .foregroundStyle(AeroricColor.textSecondary)
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityIdentifier("taskDeliverablesReview.appearance.boundary")
        }
    }

    private var deliverableSelection: Binding<TaskDeliverableID?> {
        Binding(
            get: { store.selectedDeliverableID },
            set: { value in
                if let value { _ = store.selectDeliverable(value) }
            }
        )
    }

    private var selectedVersionSelection: Binding<String> {
        Binding(
            get: { store.selectedVersionID ?? "" },
            set: { _ = store.selectVersion($0) }
        )
    }

    private var feedbackSelection: Binding<String?> {
        Binding(
            get: { store.selectedFeedbackID },
            set: { store.selectedFeedbackID = $0 }
        )
    }

    private var draftText: Binding<String> {
        Binding(get: { store.draft.text }, set: { store.setDraftText($0) })
    }

    private var draftRecipient: Binding<TaskDeliverablesReviewRecipient> {
        Binding(
            get: { store.draft.recipient },
            set: { store.setDraftRecipient($0) }
        )
    }

    private var draftLocationChoice: Binding<TaskDeliverablesReviewLocationChoice> {
        Binding(
            get: {
                switch store.draft.location {
                case .whole: .whole
                case .known: .known
                case .unknown: .unknown
                }
            },
            set: { store.setDraftLocationChoice($0) }
        )
    }

    private var availableDraftLocationChoices: [TaskDeliverablesReviewLocationChoice] {
        let anchorVersion =
            store.draft.versionID.flatMap { store.version($0) }
            ?? store.selectedVersion
        return TaskDeliverablesReviewLocationChoice.allCases.filter { choice in
            choice != .known
                || store.draft.location.isReliable
                || anchorVersion?.knownLocation != nil
        }
    }

    private var shortActionStyle: some ViewModifier {
        TaskDeliverablesReviewShortActionStyle()
    }

    private func reviewPanel<Content: View>(
        _ id: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        GlassPanel(
            preference: surface,
            reduceTransparency: reduceTransparency ? true : nil,
            increaseContrast: increaseContrast ? true : nil
        ) {
            VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                content()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(NativePanelGeometry.panelInset)
            .accessibilityIdentifier("taskDeliverablesReview.panel.\(id)")
        }
    }

    private func checkColor(_ result: TaskCheckResult) -> Color {
        switch result {
        case .passed: AeroricColor.success
        case .pending, .unknown: AeroricColor.warning
        case .notRun: AeroricColor.textSecondary
        }
    }
}

private struct TaskDeliverablesReviewShortActionStyle: ViewModifier {
    func body(content: Content) -> some View {
        content
            .buttonStyle(.bordered)
            .controlSize(.small)
    }
}
#endif
