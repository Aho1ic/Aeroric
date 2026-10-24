#if DEBUG
import AppKit
import Observation
import SwiftUI

/// A DEBUG-only, window-owned fixture. This view has no product client,
/// services, defaults, source-file reader, scheduler or agent connection.
public struct NativeKnowledgeReviewView: View {
    @State private var store: KnowledgeReviewStore
    @State private var appearance: ReviewAppearanceOverride = .system
    @State private var surface: SurfacePreference = .glass
    @State private var injectTransparency = false
    @State private var injectContrast = false
    @State private var injectMotion = false
    @State private var sizeID = NativePanelGeometry.defaultReviewSample.id
    @State private var measured = KnowledgeReviewWindowMeasurement()
    @State private var panelFrames: [String: CGRect] = [:]
    @State private var sheet: KnowledgeReviewSheet?
    @State private var draftFailure = ""
    @State private var referencesExpanded = false
    @State private var impactsExpanded = false
    @State private var versionNumber: Int?
    @State private var nativeInputEvidence = KnowledgeReviewNativeInputEvidence()
    @FocusState private var nativeListHasFocus: Bool
    @Environment(\.accessibilityReduceTransparency) private var systemTransparency
    @Environment(\.colorSchemeContrast) private var systemContrast
    @Environment(\.accessibilityReduceMotion) private var systemMotion

    public init() {
        _store = State(initialValue: KnowledgeReviewStore())
    }

    // The same strengthening preview switches used by the real sheet can
    // initialize a hosting test. System accessibility values remain read-only
    // and always win; no private environment key or global setting is changed.
    init(store: KnowledgeReviewStore,
         reduceTransparencyPreview: Bool = false,
         increaseContrastPreview: Bool = false,
         reduceMotionPreview: Bool = false) {
        _store = State(initialValue: store)
        _injectTransparency = State(initialValue: reduceTransparencyPreview)
        _injectContrast = State(initialValue: increaseContrastPreview)
        _injectMotion = State(initialValue: reduceMotionPreview)
        _referencesExpanded = State(initialValue: [.references, .retirement].contains(store.scenario))
        _impactsExpanded = State(initialValue: store.scenario == .impact)
    }

    private var reduceTransparency: Bool { systemTransparency || injectTransparency }
    private var increaseContrast: Bool { systemContrast == .increased || injectContrast }
    private var reduceMotion: Bool { systemMotion || injectMotion }
    private var effectiveSurface: SurfacePreference {
        SurfaceChoice.resolve(preference: surface, reduceTransparency: reduceTransparency)
    }
    private var sample: NativePanelGeometry.ReviewSample { NativePanelGeometry.reviewSample(id: sizeID) }
    private var displayedVersion: KnowledgeReviewVersion? {
        guard let item = store.selectedItem else { return nil }
        if let versionNumber, let value = item.versions.first(where: { $0.number == versionNumber }) {
            return value
        }
        return store.section == .history ? (item.history.first ?? item.current) : item.current
    }

    public var body: some View {
        GeometryReader { geometry in
            VStack(spacing: NativePanelGeometry.panelGap) {
                header
                ScrollView {
                    VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                        overview
                        if geometry.size.width >= 860 {
                            HStack(alignment: .top, spacing: NativePanelGeometry.panelGap) {
                                panel("list") { reviewList(compact: false) }
                                    .frame(width: 300)
                                panel("detail") { detail }
                            }
                        } else {
                            panel("list") { reviewList(compact: true) }
                            panel("detail") { detail }
                        }
                        referenceDisclosure
                        impactDisclosure
                        Text("所有操作仅改变本窗口内存；重置或重开恢复虚构样例。用户 Stop 与宿主硬限制始终优先。")
                            .font(.system(size: AeroricType.secondary))
                            .foregroundStyle(AeroricColor.textSecondary)
                        knowledgeReadout("process.pid", "本样例进程", String(ProcessInfo.processInfo.processIdentifier))
                        knowledgeReadout("memory.instance", "窗口内存实例", store.instanceID.uuidString)
                        KnowledgeReviewNativeInputReadout(evidence: nativeInputEvidence)
                    }
                    .padding(.horizontal, NativePanelGeometry.panelInset)
                    .padding(.bottom, NativePanelGeometry.panelInset)
                }
            }
            .coordinateSpace(name: "knowledgeReview.panels")
            .onPreferenceChange(KnowledgeReviewPanelFrameKey.self) { panelFrames = $0 }
        }
        .font(.system(size: AeroricType.body))
        .foregroundStyle(AeroricColor.text)
        .frame(minWidth: 380, minHeight: 360)
        .background {
            NativePanelFoundation(preference: surface, reduceTransparency: reduceTransparency)
                .ignoresSafeArea()
        }
        .background {
            KnowledgeReviewWindowProbe(
                requested: CGSize(width: sample.totalWidth, height: sample.totalHeight),
                onMeasure: { if measured != $0 { measured = $0 } },
                onClose: rebuildWindowMemory,
                inputEvidence: nativeInputEvidence,
                inputState: {
                    ["section": store.section.rawValue,
                     "selectedID": store.selectedItemID?.rawValue ?? "<none>",
                     "selectedTitle": store.selectedItem?.title ?? "<none>",
                     "visibleIDs": store.visibleItems.map { $0.id.rawValue },
                     "nativeListHasFocus": nativeListHasFocus,
                     "batchCount": store.selectedCandidateIDs.count]
                }
            )
        }
        .preferredColorScheme(appearance.colorScheme)
        .sheet(item: $sheet, onDismiss: {
            // A late dismissal must not cancel a subsequently opened sheet.
            if sheet == nil { closeSheet() }
        }) { target in
            sheetContent(target)
                .modifier(KnowledgeReviewSheetFrame(measurement: $measured))
                .preferredColorScheme(appearance.colorScheme)
        }
        .transaction { transaction in
            if reduceMotion {
                transaction.animation = nil
                transaction.disablesAnimations = true
            }
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(KnowledgeReviewFixtures.project.title)
                .font(.system(size: AeroricType.title, weight: .semibold))
                .accessibilityIdentifier("knowledgeReview.project")
            Text(KnowledgeReviewFixtures.sampleNotice)
                .font(.system(size: AeroricType.secondary, weight: .semibold))
                .foregroundStyle(AeroricColor.warning)
                .accessibilityIdentifier("knowledgeReview.notice")
            HStack(spacing: NativePanelGeometry.panelGap) {
                Menu {
                    Picker("离线知识场景", selection: Binding(
                        get: { store.scenario }, set: chooseScenario
                    )) {
                        ForEach(KnowledgeReviewScenario.allCases) { scenario in
                            Text(scenario.menuTitle).tag(scenario)
                        }
                    }
                } label: {
                    Label(store.scenario.menuTitle, systemImage: "books.vertical")
                }
                .accessibilityLabel("离线知识场景")
                .accessibilityValue(Text(store.scenario.menuTitle))
                .accessibilityIdentifier("knowledgeReview.scenario")
                Spacer(minLength: 0)
                NativeIconButton(systemImage: "arrow.counterclockwise", label: "重置当前内存样例",
                                 identifier: "knowledgeReview.reset", action: resetScenario)
                NativeIconButton(systemImage: "paintpalette", label: "窗口内外观预览",
                                 identifier: "knowledgeReview.appearance") { sheet = .appearance }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, NativePanelGeometry.panelInset * 2)
        .padding(.top, NativePanelGeometry.panelInset)
    }

    private var overview: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            Text(store.scenario.detail)
                .accessibilityIdentifier("knowledgeReview.scenarioSummary")
            knowledgeReadout("configuration.summary", "项目自动入库",
                             "\(store.admissionConfiguration.enabled ? "开启" : "关闭") · \(store.admissionConfiguration.preset.title)")
            ViewThatFits(in: .horizontal) {
                HStack { configurationControls }
                VStack(alignment: .leading) { configurationControls }
            }
            Button("演示处理新候选") { animate { store.processNewCandidates() } }
                .disabled(store.hasProcessedNewCandidates)
                .modifier(KnowledgeReviewActionStyle(surface: effectiveSurface))
                .accessibilityIdentifier("knowledgeReview.newCandidates.process")
            Text(KnowledgeReviewFixtures.processNewNote)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
            Text(store.notice)
                .font(.system(size: AeroricType.secondary, weight: .medium))
                .foregroundStyle(AeroricColor.accent)
                .accessibilityIdentifier("knowledgeReview.feedback")
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(NativePanelGeometry.panelInset)
    }

    @ViewBuilder private var configurationControls: some View {
        Button("项目自动入库配置") {
            store.beginConfiguration()
            draftFailure = ""
            sheet = .admission
        }
        .modifier(KnowledgeReviewActionStyle(surface: effectiveSurface))
        .accessibilityIdentifier("knowledgeReview.configuration.open")
        Menu {
            Picker("任务授权示例", selection: Binding(
                get: { store.permission }, set: store.setPermission
            )) {
                ForEach(KnowledgeReviewPermission.allCases) { value in
                    Text(value.title).tag(value)
                }
            }
        } label: {
            Label(store.permission.title, systemImage: "person.badge.key")
        }
        .accessibilityLabel("任务授权示例（不是真实提权）")
        .accessibilityValue(Text(store.permission.title))
        .accessibilityIdentifier("knowledgeReview.permission")
    }

    private func reviewList(compact: Bool) -> some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            if compact {
                Menu {
                    Picker("知识栏目", selection: sectionBinding) {
                        ForEach(KnowledgeReviewSection.allCases) { section in
                            Text(section.title).tag(section)
                        }
                    }
                } label: {
                    Label(store.section.title, systemImage: "list.bullet")
                }
                .accessibilityLabel("知识栏目")
                .accessibilityValue(Text(store.section.title))
                .accessibilityIdentifier("knowledgeReview.section")
            } else {
                NativeSegmentedPicker(
                    selection: sectionBinding, groupTitle: "知识栏目",
                    segments: KnowledgeReviewSection.allCases.map {
                        .init(value: $0, title: LocalizedStringKey($0.title))
                    }
                )
            }
            if store.visibleItems.isEmpty {
                Text("此栏目暂无样例内容；可切换栏目或重置。")
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityIdentifier("knowledgeReview.items.empty")
            } else if compact {
                Menu {
                    Picker("详情内容", selection: selectionBinding) {
                        ForEach(store.visibleItems) { item in
                            Text(item.title).tag(Optional(item.id))
                        }
                    }
                } label: {
                    Text(store.selectedItem?.title ?? "选择详情内容")
                        .lineLimit(2)
                }
                .accessibilityLabel("详情内容")
                .accessibilityValue(Text(store.selectedItem?.title ?? "无当前内容"))
                .accessibilityIdentifier("knowledgeReview.itemPicker")
                if store.section == .candidates {
                    ForEach(store.visibleItems) { item in candidateCheckbox(item) }
                }
            } else {
                NativeSelectionList(selection: selectionBinding) {
                    ForEach(store.visibleItems) { item in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(item.title)
                                .font(.system(size: AeroricType.body, weight: .medium))
                                .accessibilityIdentifier("knowledgeReview.item.\(item.id.rawValue)")
                            if store.section == .candidates {
                                candidateCheckbox(item)
                            } else {
                                Text(item.state.title)
                                    .font(.system(size: AeroricType.secondary))
                                    .foregroundStyle(stateColor(item))
                            }
                        }
                        .padding(.vertical, 4)
                        .tag(item.id)
                    }
                }
                // Columns are different native collections. Do not carry a
                // long candidate List's clip offset into a shorter collection.
                // Store/detail/batch bindings and stable row IDs stay intact.
                .id(store.section)
                // This owned hosting window needs an explicit native focus
                // target. Selection and arrow handling remain system-owned.
                .focused($nativeListHasFocus)
                .defaultFocus($nativeListHasFocus, true)
                .listStyle(.inset)
                .scrollContentBackground(.hidden)
                .frame(height: 330)
            }
            if store.section == .candidates {
                knowledgeReadout("batch.summary", "批量勾选", "\(store.selectedCandidateIDs.count) 条")
                ViewThatFits(in: .horizontal) {
                    HStack { batchButtons }
                    VStack(alignment: .leading) { batchButtons }
                }
                Text("批量勾选不改变详情焦点；有待处理原因的候选不能被强行确认。")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                if !store.selectedCandidateIDs.isEmpty, let reason = store.batchConfirmationBlockedReason {
                    Text(reason)
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.warning)
                        .accessibilityIdentifier("knowledgeReview.batch.blocked")
                }
            }
        }
    }

    private var sectionBinding: Binding<KnowledgeReviewSection> {
        Binding(get: { store.section }, set: { value in
            animate { store.setSection(value); versionNumber = nil }
        })
    }

    private var selectionBinding: Binding<KnowledgeReviewItemID?> {
        Binding(get: { store.selectedItemID }, set: { value in
            if let value { animate { store.selectItem(value); versionNumber = nil } }
        })
    }

    private func candidateCheckbox(_ item: KnowledgeReviewItem) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Toggle("批选：\(item.title)", isOn: Binding(
                get: { store.selectedCandidateIDs.contains(item.id) },
                set: { store.setCandidateSelected(item.id, selected: $0) }
            ))
            .toggleStyle(.checkbox)
            .accessibilityIdentifier("knowledgeReview.batch.\(item.id.rawValue)")
            if let reason = item.confirmationBlockedReason {
                Text(reason)
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(item.issue == .sensitivePlaceholder ? AeroricColor.danger : AeroricColor.warning)
                    .accessibilityIdentifier("knowledgeReview.blocked.\(item.id.rawValue)")
            }
        }
    }

    @ViewBuilder private var batchButtons: some View {
        Button("确认所选安全候选") { animate { store.confirmSelectedCandidates() } }
            .disabled(store.batchConfirmationBlockedReason != nil)
            .modifier(KnowledgeReviewActionStyle(surface: effectiveSurface))
            .accessibilityIdentifier("knowledgeReview.batch.confirm")
        Button("丢弃所选候选", role: .destructive) { animate { store.discardSelectedCandidates() } }
            .disabled(store.batchDiscardBlockedReason != nil)
            .accessibilityIdentifier("knowledgeReview.batch.discard")
    }

    @ViewBuilder private var detail: some View {
        if let item = store.selectedItem, let version = displayedVersion {
            VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                Text(item.title)
                    .font(.headline)
                    .accessibilityIdentifier("knowledgeReview.detail.title")
                Label(item.state.title, systemImage: stateSymbol(item))
                    .foregroundStyle(stateColor(item))
                    .accessibilityIdentifier("knowledgeReview.detail.status")
                if store.section == .history && item.versions.count > 1 {
                    Menu {
                        Picker("获准历史版本", selection: Binding(
                            get: { displayedVersion?.number ?? item.current.number },
                            set: { versionNumber = $0 }
                        )) {
                            ForEach(item.versions) { value in
                                Text("\(value.title) · \(value.state.title)").tag(value.number)
                            }
                        }
                    } label: {
                        Text("回看 \(version.title) · \(version.state.title)")
                    }
                    .accessibilityLabel("获准历史版本")
                    .accessibilityValue(Text("\(version.title) · \(version.state.title)"))
                    .accessibilityIdentifier("knowledgeReview.history.version")
                }
                knowledgeReadout("detail.version", "版本", "\(version.title) · \(version.state.title)")
                Text(version.content)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
                    .padding(NativePanelGeometry.panelInset)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(AeroricColor.layer1)
                    .accessibilityLabel("知识正文")
                    .accessibilityValue(Text(version.content))
                    .accessibilityIdentifier("knowledgeReview.detail.content")
                knowledgeReadout("detail.scope", "适用范围",
                                 "\(version.applicability.project.title) · \(version.applicability.summary)")
                knowledgeReadout("detail.limitations", "限制", version.applicability.limits)
                knowledgeReadout("detail.verification", "依据标记", version.evidence.title)
                knowledgeReadout("detail.source", "来源", version.source.title)
                if let origin = item.admissionOrigin {
                    Text(origin.title)
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                }
                if let replaced = version.replaces {
                    Text("替代关联：\(replaced.title) → \(version.title)；\(version.correctionBasis ?? "")")
                        .accessibilityIdentifier("knowledgeReview.detail.replacement")
                }
                if item.state == .candidate, let reason = item.confirmationBlockedReason {
                    Text("待处理：\(reason)")
                        .foregroundStyle(AeroricColor.warning)
                        .accessibilityIdentifier("knowledgeReview.detail.blocked")
                }
                if item.state == .candidate, store.hasProcessedNewCandidates,
                   KnowledgeReviewFixtures.newBatch.contains(where: { $0.id == item.id }),
                   let reason = store.automaticAdmissionReason(for: item) {
                    knowledgeReadout("detail.automaticAdmission", "仍未自动入库", reason)
                }
                Button("查看内嵌来源摘录") { openSource(version.source, context: "\(version.title) · \(version.state.title)") }
                    .accessibilityIdentifier("knowledgeReview.detail.openSource")
                ViewThatFits(in: .horizontal) {
                    HStack { detailActions(item) }
                    VStack(alignment: .leading) { detailActions(item) }
                }
                if item.hasHistory {
                    Button("在历史中查看获准旧版本") {
                        store.setSection(.history)
                        store.selectItem(item.id)
                        versionNumber = item.history.first?.number ?? item.current.number
                    }
                    .accessibilityIdentifier("knowledgeReview.history.open")
                }
                if item.state == .retired {
                    Text(KnowledgeReviewFixtures.retirementNote)
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.warning)
                }
            }
        } else {
            Text("选择一条有内容的样例以查看详情。")
                .foregroundStyle(AeroricColor.textSecondary)
                .accessibilityIdentifier("knowledgeReview.detail.empty")
        }
    }

    @ViewBuilder private func detailActions(_ item: KnowledgeReviewItem) -> some View {
        if item.state == .candidate {
            Button("编辑候选") {
                store.beginContentEdit()
                draftFailure = ""
                if store.contentDraft != nil { sheet = .edit }
            }
            .accessibilityIdentifier("knowledgeReview.edit")
        }
        if item.state == .reusable && store.section != .history {
            Button("修订示例知识") {
                store.beginRevision()
                draftFailure = ""
                if store.revisionDraft != nil { sheet = .revision }
            }
            .disabled(store.revisionBlockedReason != nil)
            .help(store.revisionBlockedReason ?? "以用户纠正依据形成示例 v2")
            .accessibilityIdentifier("knowledgeReview.revise")
            Button("停用示例知识", role: .destructive) {
                animate {
                    store.retireSelectedItem()
                    versionNumber = item.current.number
                    referencesExpanded = true
                    impactsExpanded = true
                }
            }
            .disabled(store.retirementBlockedReason != nil)
            .accessibilityIdentifier("knowledgeReview.retire")
        }
    }

    private var referenceDisclosure: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            Button {
                animate { referencesExpanded.toggle() }
            } label: {
                Label("按需参考 · 后续新上下文", systemImage: referencesExpanded ? "chevron.down" : "chevron.right")
            }
            .accessibilityIdentifier("knowledgeReview.references.toggle")
            if referencesExpanded {
                panel("references") { referencePanel }
                    .transition(.opacity)
            }
        }
    }

    private var referencePanel: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            Menu {
                Picker("虚构任务或工作包", selection: Binding(
                    get: { store.selectedWorkID }, set: store.setWork
                )) {
                    ForEach(KnowledgeReviewFixtures.works) { work in
                        Text(work.title).tag(work.id)
                    }
                }
            } label: {
                Label(KnowledgeReviewFixtures.work(store.selectedWorkID).title, systemImage: "doc.text.magnifyingglass")
            }
            .accessibilityLabel("虚构任务或工作包")
            .accessibilityValue(Text(store.selectedWork.title))
            .accessibilityIdentifier("knowledgeReview.references.work")
            Menu {
                Picker("参考条件示例", selection: Binding(
                    get: { store.referenceScenario }, set: store.setReferenceScenario
                )) {
                    ForEach(KnowledgeReviewReferenceScenario.allCases) { value in
                        Text(value.title).tag(value)
                    }
                }
            } label: {
                Text(store.referenceScenario.title)
            }
            .accessibilityLabel("参考条件示例")
            .accessibilityValue(Text(store.referenceScenario.title))
            .accessibilityIdentifier("knowledgeReview.references.context")
            Text(store.currentRequirement)
                .font(.system(size: AeroricType.secondary))
                .accessibilityIdentifier("knowledgeReview.references.requirement")
            knowledgeReadout("references.count", "后续新上下文推荐", "\(store.recommendedReferences.count) 条")
            Text(store.referenceRows.isEmpty
                 ? "当前工作没有获准可参考项；不会复制整库或编造缺失内容。"
                 : store.referenceRows.map { "\($0.itemTitle)：\($0.condition.title)。\($0.note)" }.joined(separator: "\n"))
                .font(.system(size: AeroricType.secondary))
                .accessibilityIdentifier("knowledgeReview.references.summary")
            if store.recommendedReferences.isEmpty {
                Text("当前条件下没有可推荐项；已停用内容只在历史和旧引用中回看。")
                    .foregroundStyle(AeroricColor.warning)
            }
            ForEach(store.referenceRows) { reference in
                VStack(alignment: .leading, spacing: 4) {
                    Text("\(reference.itemTitle) · \(reference.version.title)")
                        .font(.subheadline.bold())
                    Text(reference.condition == .retired
                         ? "已停用，不提供后续新上下文推荐正文；仅可在历史中回看。"
                         : reference.summary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("knowledgeReview.reference.\(reference.itemID.rawValue).summary")
                    Text(reference.condition.title)
                        .foregroundStyle(reference.isRecommended ? AeroricColor.accent : AeroricColor.warning)
                        .accessibilityIdentifier("knowledgeReview.reference.\(reference.itemID.rawValue).status")
                    Text("\(reference.note)\n适用：\(reference.applicability.summary)\n限制：\(reference.applicability.limits)")
                        .font(.system(size: AeroricType.secondary))
                        .accessibilityIdentifier("knowledgeReview.reference.\(reference.itemID.rawValue).explanation")
                    if reference.condition == .retired {
                        Button("在历史中回看停用项") {
                            store.setSection(.history)
                            store.selectItem(reference.itemID)
                            versionNumber = reference.version.number
                        }
                        .accessibilityIdentifier("knowledgeReview.reference.\(reference.itemID.rawValue).history")
                    } else {
                        Button("来源：\(reference.source.title)") {
                            openSource(reference.source, context: "\(reference.version.title) · \(reference.condition.title)")
                        }
                        .accessibilityIdentifier("knowledgeReview.reference.\(reference.itemID.rawValue).source")
                    }
                }
                .padding(NativePanelGeometry.panelInset)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(AeroricColor.layer1)
            }
            Text(KnowledgeReviewFixtures.referenceBoundaryNote)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.warning)
        }
    }

    private var impactDisclosure: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            Button {
                animate { impactsExpanded.toggle() }
            } label: {
                Label("在途引用与局部核对", systemImage: impactsExpanded ? "chevron.down" : "chevron.right")
            }
            .accessibilityIdentifier("knowledgeReview.impacts.toggle")
            if impactsExpanded {
                panel("impacts") {
                    VStack(alignment: .leading, spacing: AeroricSpace.l) {
                        Text("通知和 ACK 仅表示演示送达；不等于纠正、撤销副作用或本次核验。")
                            .foregroundStyle(AeroricColor.warning)
                        ForEach(store.impactRows) { impact in impactRow(impact) }
                    }
                }
                .transition(.opacity)
            }
        }
    }

    private func impactRow(_ impact: KnowledgeReviewImpact) -> some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            Text(impact.title).font(.subheadline.bold())
            Text(impact.state == .pending
                 ? "\(impact.state.title) · \(impact.delivery.title)"
                 : impact.state.title)
                .foregroundStyle(impact.state == .pending ? AeroricColor.warning : AeroricColor.accent)
                .accessibilityIdentifier("knowledgeReview.impact.\(impact.id.rawValue).status")
            Text(impact.note).font(.system(size: AeroricType.secondary))
            if let captured = impact.capturedReference {
                Text("\(store.capturedReferenceNote(captured))\n局部核对关联版本：\(impact.requiredVersion?.title ?? "待复核")。")
                    .accessibilityIdentifier("knowledgeReview.impact.\(impact.id.rawValue).citation")
                Button("回看已捕获的 \(captured.version.title) 来源") {
                    openSource(captured.version.source, context: "历史旧引用 \(captured.version.title) · 不代表当前有效")
                }
                .accessibilityIdentifier("knowledgeReview.impact.\(impact.id.rawValue).source")
                ViewThatFits(in: .horizontal) {
                    HStack { impactActions(impact) }
                    VStack(alignment: .leading) { impactActions(impact) }
                }
                if let record = impact.loadedRecord {
                    Text("\(record.title)：\(record.summary)")
                        .foregroundStyle(AeroricColor.accent)
                        .accessibilityIdentifier("knowledgeReview.impact.\(impact.id.rawValue).record")
                } else if let reason = store.preloadedCheckBlockedReason(for: impact.id) {
                    Text(reason)
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .accessibilityIdentifier("knowledgeReview.impact.\(impact.id.rawValue).blocked")
                }
            } else {
                Button("演示安全独立文档继续") { store.continueIndependentWork(.docsPackage) }
                    .disabled(impact.state != .unaffected)
                    .accessibilityIdentifier("knowledgeReview.documentation.continue")
            }
            Divider()
        }
    }

    @ViewBuilder private func impactActions(_ impact: KnowledgeReviewImpact) -> some View {
        Button("演示通知") { store.notify(impact.id) }
            .disabled(impact.state != .pending || impact.delivery != .notSent)
            .accessibilityIdentifier("knowledgeReview.impact.\(impact.id.rawValue).notify")
        Button("演示 ACK") { store.acknowledge(impact.id) }
            .disabled(impact.state != .pending || impact.delivery != .notified)
            .accessibilityIdentifier("knowledgeReview.impact.\(impact.id.rawValue).ack")
        Button("载入预置局部核对") { store.loadPreloadedCheck(for: impact.id) }
            .disabled(store.preloadedCheckBlockedReason(for: impact.id) != nil)
            .accessibilityIdentifier("knowledgeReview.impact.\(impact.id.rawValue).check")
    }

    private func panel<Content: View>(_ key: String, @ViewBuilder content: () -> Content) -> some View {
        GlassPanel(preference: surface, reduceTransparency: reduceTransparency,
                   increaseContrast: increaseContrast) {
            content()
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(NativePanelGeometry.panelInset)
        }
        .background {
            GeometryReader { geometry in
                Color.clear.preference(key: KnowledgeReviewPanelFrameKey.self,
                                       value: [key: geometry.frame(in: .named("knowledgeReview.panels"))])
            }
        }
    }

    private func stateColor(_ item: KnowledgeReviewItem) -> Color {
        if item.issue == .sensitivePlaceholder { return AeroricColor.danger }
        switch item.state {
        case .candidate: return item.confirmationBlockedReason == nil ? AeroricColor.accent : AeroricColor.warning
        case .reusable: return AeroricColor.success
        case .discarded, .retired: return AeroricColor.warning
        }
    }

    private func stateSymbol(_ item: KnowledgeReviewItem) -> String {
        switch item.state {
        case .candidate: "tray"
        case .reusable: "checkmark.seal"
        case .discarded: "archivebox"
        case .retired: "pause.circle"
        }
    }

    private func animate(_ action: () -> Void) {
        withTransaction(KnowledgeReviewMotionPolicy.transaction(reduceMotion: reduceMotion), action)
    }

    private func chooseScenario(_ scenario: KnowledgeReviewScenario) {
        closeSheet()
        animate {
            store.load(scenario)
            versionNumber = nil
            referencesExpanded = [.references, .retirement].contains(scenario)
            impactsExpanded = scenario == .impact
        }
    }

    private func resetScenario() {
        chooseScenario(store.scenario)
        resetPresentation()
    }

    private func resetPresentation() {
        appearance = .system
        surface = .glass
        injectTransparency = false
        injectContrast = false
        injectMotion = false
        sizeID = NativePanelGeometry.defaultReviewSample.id
    }

    private func rebuildWindowMemory() {
        closeSheet()
        store = KnowledgeReviewStore()
        versionNumber = nil
        referencesExpanded = false
        impactsExpanded = false
        resetPresentation()
    }

    private func openSource(_ source: KnowledgeReviewSource, context: String) {
        sheet = .source(KnowledgeReviewSourcePresentation(source: source, context: context))
    }

    private func closeSheet() {
        sheet = nil
        draftFailure = ""
        // Successful apply already cleared its draft. Do not overwrite the
        // success notice with a cancellation, or cancel an unrelated state.
        if store.contentDraft != nil { store.cancelContentDraft() }
        if store.configurationDraft != nil { store.cancelConfigurationDraft() }
        if store.revisionDraft != nil { store.cancelRevisionDraft() }
    }

    private func draftValidationMessage(for target: KnowledgeReviewSheet) -> String? {
        let revision = target == .revision
        let content = revision ? store.revisionDraft?.content : store.contentDraft?.content
        let scope = revision ? store.revisionDraft?.applicability : store.contentDraft?.applicability
        guard let content, let scope else { return "草稿已关闭或失效；请重新打开。" }
        let text = content.trimmingCharacters(in: .whitespacesAndNewlines)
        let applicability = scope.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !applicability.isEmpty else { return "内容与适用说明不能为空。" }
        if revision {
            guard let draft = store.revisionDraft,
                  !draft.basis.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                return "请填写虚构用户纠正依据；未填写不能生成新版本。"
            }
            if let original = store.item(id: draft.originalVersion.itemID),
               text == original.content, applicability == original.applicability.summary {
                return "修订需明确改变内容或适用说明；不生成空变化版本。"
            }
        }
        return nil
    }

    @ViewBuilder private func sheetContent(_ target: KnowledgeReviewSheet) -> some View {
        switch target {
        case .appearance:
            KnowledgeReviewAppearancePanel(
                appearance: $appearance, surface: $surface,
                reduceTransparency: $injectTransparency, increaseContrast: $injectContrast,
                reduceMotion: $injectMotion, sizeID: $sizeID,
                systemTransparency: systemTransparency, systemContrast: systemContrast == .increased,
                systemMotion: systemMotion, measurement: $measured, panelFrames: $panelFrames,
                close: closeSheet
            )
        case .source(let presentation):
            KnowledgeReviewSourcePanel(source: presentation.source, context: presentation.context, close: closeSheet)
        case .admission:
            KnowledgeReviewConfigurationPanel(
                enabled: Binding(
                    get: { store.configurationDraft?.configuration.enabled ?? false },
                    set: { store.configurationDraft?.configuration.enabled = $0 }
                ),
                preset: Binding(
                    get: { store.configurationDraft?.configuration.preset ?? .userPreferences },
                    set: { store.configurationDraft?.configuration.preset = $0 }
                ),
                cancel: closeSheet,
                apply: { if store.applyConfigurationDraft() { closeSheet() } }
            )
        case .edit, .revision:
            KnowledgeReviewEditorPanel(
                revision: target == .revision,
                content: Binding(
                    get: { target == .revision ? (store.revisionDraft?.content ?? "") : (store.contentDraft?.content ?? "") },
                    set: {
                        draftFailure = ""
                        if target == .revision { store.revisionDraft?.content = $0 } else { store.contentDraft?.content = $0 }
                    }
                ),
                scope: Binding(
                    get: { target == .revision ? (store.revisionDraft?.applicability ?? "") : (store.contentDraft?.applicability ?? "") },
                    set: {
                        draftFailure = ""
                        if target == .revision { store.revisionDraft?.applicability = $0 } else { store.contentDraft?.applicability = $0 }
                    }
                ),
                basis: Binding(
                    get: { store.revisionDraft?.basis ?? "" },
                    set: { draftFailure = ""; store.revisionDraft?.basis = $0 }
                ),
                error: draftFailure.isEmpty ? (draftValidationMessage(for: target) ?? "") : draftFailure,
                canApply: draftValidationMessage(for: target) == nil,
                cancel: closeSheet,
                apply: {
                    let applied = target == .revision ? store.applyRevisionDraft() : store.applyContentDraft()
                    if applied { closeSheet() } else { draftFailure = store.notice }
                }
            )
        }
    }
}

/// Used by the actual sample's selection/detail transitions, not only a label.
enum KnowledgeReviewMotionPolicy {
    static func transaction(reduceMotion: Bool) -> Transaction {
        var value = Transaction(animation: reduceMotion ? nil : .easeInOut(
            duration: AeroricMotion.duration(AeroricMotion.standard, reduceMotion: false)
        ))
        value.disablesAnimations = reduceMotion
        return value
    }
}

struct KnowledgeReviewWindowMeasurement: Equatable {
    var frame: CGRect = .zero
    var content: CGSize = .zero
    var layout: CGSize = .zero
    var requestedFitsDisplay = false

    var sheetSize: CGSize {
        CGSize(width: min(620, max(320, content.width - 32)),
               height: min(680, max(320, layout.height - 32)))
    }
}

/// Read the live binding in the presented view's own body, not a captured
/// root-view value. Resizing must update a sheet even when its item is unchanged.
private struct KnowledgeReviewSheetFrame: ViewModifier {
    @Binding var measurement: KnowledgeReviewWindowMeasurement

    func body(content: Content) -> some View {
        let size = measurement.sheetSize
        content.frame(width: size.width, height: size.height)
    }
}

/// A probe attached to this view's window only. It neither enumerates other
/// instances nor activates, closes, restores, or cleans preferences for them.
/// The close callback replaces window memory even if SwiftUI retains a Scene.
private struct KnowledgeReviewWindowProbe: NSViewRepresentable {
    let requested: CGSize
    let onMeasure: (KnowledgeReviewWindowMeasurement) -> Void
    let onClose: () -> Void
    let inputEvidence: KnowledgeReviewNativeInputEvidence
    let inputState: () -> [String: Any]

    @MainActor
    final class ProbeView: NSView {
        var requested = CGSize.zero
        var report: (KnowledgeReviewWindowMeasurement) -> Void = { _ in }
        var closeMemory: () -> Void = {}
        var inputEvidence: KnowledgeReviewNativeInputEvidence?
        var inputState: () -> [String: Any] = { [:] }
        private weak var observed: NSWindow?
        private weak var lastHitOutline: NSOutlineView?
        private var applied: CGSize?
        private var closing = false

        deinit { NotificationCenter.default.removeObserver(self) }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            synchronize()
        }

        func synchronize() {
            guard let window else { detach(); return }
            if closing {
                guard window.isVisible else { return }
                closing = false
            }
            if observed !== window {
                NotificationCenter.default.removeObserver(self)
                observed = window
                (window as? KnowledgeReviewInputWindow)?.observeNativeInput = { [weak self] stage, event in
                    self?.recordInput(stage: stage, event: event)
                }
                applied = nil
                window.isRestorable = false
                for name in [NSWindow.didResizeNotification, NSWindow.didMoveNotification,
                             NSWindow.didChangeScreenNotification] {
                    NotificationCenter.default.addObserver(
                        self, selector: #selector(measure), name: name, object: window
                    )
                }
                NotificationCenter.default.addObserver(
                    self, selector: #selector(willClose),
                    name: NSWindow.willCloseNotification, object: window
                )
            }
            if requested != applied {
                applied = requested
                applyRequestedSize()
                // SwiftUI may set its default content size after attachment.
                // Reapply once, not on every layout/manual window resize.
                let request = requested
                DispatchQueue.main.async { [weak self, weak window] in
                    guard let self, let window, self.window === window,
                          !self.closing, self.requested == request else { return }
                    self.applyRequestedSize()
                    self.measure()
                }
            }
            measure()
        }

        private func recordInput(stage: String, event: NSEvent) {
            guard !closing, let window = observed, window === self.window,
                  event.windowNumber == window.windowNumber, let content = window.contentView else { return }
            func identity(_ responder: NSResponder?) -> String {
                guard let responder else { return "<none>" }
                return "\(type(of: responder)):\(ObjectIdentifier(responder))"
            }
            func chain(_ start: NSResponder?) -> [String] {
                var current = start
                var seen = Set<ObjectIdentifier>()
                var result: [String] = []
                while let responder = current, result.count < 16,
                      seen.insert(ObjectIdentifier(responder)).inserted {
                    result.append(identity(responder))
                    current = responder.nextResponder
                }
                return result
            }
            let point = content.superview?.convert(event.locationInWindow, from: nil) ?? event.locationInWindow
            let hit = event.type == .keyDown ? nil : content.hitTest(point)
            func outline(for responder: NSResponder?) -> NSOutlineView? {
                if let outline = responder as? NSOutlineView { return outline }
                return (responder as? NSView)?.enclosingScrollView?.documentView as? NSOutlineView
            }
            if let actual = outline(for: hit), actual.window === window {
                lastHitOutline = actual
            }
            // Public responder/event-hit and NSScrollView document ownership
            // only. Never enumerate or inspect SwiftUI's private view subtree.
            var identities = Set<ObjectIdentifier>()
            let outlines = [outline(for: window.firstResponder), outline(for: hit), lastHitOutline]
                .compactMap { $0 }.filter {
                    $0.window === window && identities.insert(ObjectIdentifier($0)).inserted
                }
            let native = outlines.prefix(8).map { outline -> [String: Any] in
                let scroll = outline.enclosingScrollView
                return [
                    "identity": identity(outline),
                    "acceptsFirstResponder": outline.acceptsFirstResponder,
                    "rows": outline.numberOfRows,
                    "selectedRows": Array(outline.selectedRowIndexes),
                    "visibleRect": NSStringFromRect(outline.visibleRect),
                    "windowFrame": NSStringFromRect(outline.convert(outline.bounds, to: nil)),
                    "clipBounds": scroll.map { NSStringFromRect($0.contentView.bounds) } ?? "<none>",
                    "documentFrame": scroll?.documentView.map { NSStringFromRect($0.frame) } ?? "<none>",
                    "selectedRowRects": outline.selectedRowIndexes.prefix(3).map {
                        NSStringFromRect(outline.convert(outline.rect(ofRow: $0), to: nil))
                    },
                ]
            }
            var snapshot: [String: Any] = [
                "stage": stage, "eventType": event.type.rawValue,
                "eventTimestamp": event.timestamp,
                "windowNumber": window.windowNumber, "keyWindow": window.isKeyWindow,
                "mainWindow": window.isMainWindow, "hasSheet": window.attachedSheet != nil,
                "firstResponder": identity(window.firstResponder),
                "responderChain": chain(window.firstResponder), "hitChain": chain(hit),
                "pointInWindow": NSStringFromPoint(event.locationInWindow),
                "outlineCandidateCount": outlines.count,
                "outlineDiscovery": "Public responder/event hit/enclosingScrollView.documentView/last own hit; not exhaustive enumeration",
                "outlines": native, "store": inputState(),
                "gutterFocusTransfer": (window as? KnowledgeReviewInputWindow)?.gutterFocusTransfer ?? [:],
            ]
            if event.type == .keyDown { snapshot["keyCode"] = event.keyCode }
            if event.type == .scrollWheel { snapshot["wheelY"] = event.scrollingDeltaY }
            inputEvidence?.append(snapshot)
        }

        private func applyRequestedSize() {
            guard !closing, let window = observed, requested.width > 0,
                  let visible = window.screen?.visibleFrame,
                  visible.width >= requested.width, visible.height >= requested.height else { return }
            let frame = CGRect(
                x: visible.midX - requested.width / 2,
                y: visible.midY - requested.height / 2,
                width: requested.width, height: requested.height
            )
            window.setFrame(frame, display: true)
        }

        @objc private func measure() {
            guard !closing, let window = observed else { return }
            let visible = window.screen?.visibleFrame ?? .zero
            let value = KnowledgeReviewWindowMeasurement(
                frame: window.frame,
                content: window.contentView?.bounds.size
                    ?? window.contentRect(forFrameRect: window.frame).size,
                layout: window.contentLayoutRect.size,
                requestedFitsDisplay: visible.width >= requested.width && visible.height >= requested.height
            )
            DispatchQueue.main.async { [weak self, weak window] in
                guard let self, let window, !self.closing, self.observed === window else { return }
                self.report(value)
            }
        }

        @objc private func willClose() {
            closing = true
            applied = nil
            // No defaults write and no product lifecycle callback.
            let reset = closeMemory
            DispatchQueue.main.async { reset() }
        }

        func detach() {
            NotificationCenter.default.removeObserver(self)
            (observed as? KnowledgeReviewInputWindow)?.observeNativeInput = nil
            observed = nil
            lastHitOutline = nil
            applied = nil
        }
    }

    func makeNSView(context: Context) -> ProbeView { ProbeView(frame: .zero) }

    func updateNSView(_ view: ProbeView, context: Context) {
        view.requested = requested
        view.report = onMeasure
        view.closeMemory = onClose
        view.inputEvidence = inputEvidence
        view.inputState = inputState
        view.synchronize()
    }

    static func dismantleNSView(_ view: ProbeView, coordinator: ()) {
        view.detach()
        view.report = { _ in }
        view.closeMemory = {}
        view.inputEvidence = nil
        view.inputState = { [:] }
    }
}

/// Bounded scalar snapshots only: no event characters, files, defaults, global
/// monitor, responder assignment or business-state mutation.
@MainActor
@Observable
final class KnowledgeReviewNativeInputEvidence {
    private(set) var snapshots: [String] = []
    private var nextSequence = 1

    func append(_ snapshot: [String: Any]) {
        var snapshot = snapshot
        snapshot["snapshotSequence"] = nextSequence
        guard JSONSerialization.isValidJSONObject(snapshot),
              let data = try? JSONSerialization.data(withJSONObject: snapshot, options: [.sortedKeys]),
              let text = String(data: data, encoding: .utf8) else { return }
        nextSequence += 1
        snapshots.append(text)
        if snapshots.count > 18 { snapshots.removeFirst(snapshots.count - 18) }
    }

    var value: String { "[" + snapshots.joined(separator: ",") + "]" }
}

private struct KnowledgeReviewNativeInputReadout: View {
    let evidence: KnowledgeReviewNativeInputEvidence

    var body: some View {
        Text("原生输入诊断：\(evidence.snapshots.count) 个只读快照")
            .font(.system(size: AeroricType.secondary))
            .foregroundStyle(AeroricColor.textSecondary)
            .accessibilityLabel("本窗口原生输入诊断")
            .accessibilityValue(Text(evidence.value))
            .accessibilityIdentifier("knowledgeReview.input.diagnostics")
    }
}

private struct KnowledgeReviewPanelFrameKey: PreferenceKey {
    static let defaultValue: [String: CGRect] = [:]

    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue(), uniquingKeysWith: { _, new in new })
    }
}

/// A source presentation owns its payload. Reopening the same source ID with
/// a missing excerpt or a historical version is a new, immutable presentation.
struct KnowledgeReviewSourcePresentation: Identifiable, Equatable, Sendable {
    let id = UUID()
    let source: KnowledgeReviewSource
    let context: String
}

private enum KnowledgeReviewSheet: Identifiable, Equatable {
    case edit, admission, revision, appearance
    case source(KnowledgeReviewSourcePresentation)

    var id: String {
        switch self {
        case .edit: "edit"
        case .admission: "admission"
        case .revision: "revision"
        case .appearance: "appearance"
        case .source(let presentation): "source-\(presentation.id.uuidString)"
        }
    }
}

@MainActor
private func knowledgeReadout(_ id: String, _ title: String, _ value: String) -> some View {
    Text("\(title)：\(value)")
        .font(.system(size: AeroricType.secondary))
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityLabel(Text(title))
        .accessibilityValue(Text(value))
        .accessibilityIdentifier("knowledgeReview.\(id)")
}

private func knowledgeSize(_ size: CGSize) -> String {
    String(format: "%.1f×%.1f", size.width, size.height)
}

private func knowledgeRect(_ rect: CGRect?) -> String {
    guard let rect else { return "absent" }
    return String(format: "%.1f,%.1f,%.1f,%.1f", rect.minX, rect.minY, rect.width, rect.height)
}

/// A real system glass style on text-bearing actions only. Standalone icons
/// retain NativeIconButton's bare style, and content panes never use glassEffect.
private struct KnowledgeReviewActionStyle: ViewModifier {
    let surface: SurfacePreference

    @ViewBuilder
    func body(content: Content) -> some View {
        if #available(macOS 26.0, *), surface == .glass {
            content.buttonStyle(.glass)
        } else {
            content.buttonStyle(.bordered)
        }
    }
}

private struct KnowledgeReviewAppearancePanel: View {
    @Binding var appearance: ReviewAppearanceOverride
    @Binding var surface: SurfacePreference
    @Binding var reduceTransparency: Bool
    @Binding var increaseContrast: Bool
    @Binding var reduceMotion: Bool
    @Binding var sizeID: String
    let systemTransparency: Bool
    let systemContrast: Bool
    let systemMotion: Bool
    @Binding var measurement: KnowledgeReviewWindowMeasurement
    @Binding var panelFrames: [String: CGRect]
    let close: () -> Void

    private var effectiveTransparency: Bool { systemTransparency || reduceTransparency }
    private var effectiveContrast: Bool { systemContrast || increaseContrast }
    private var effectiveMotion: Bool { systemMotion || reduceMotion }

    var body: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            Text("窗口内外观与辅助偏好预览")
                .font(.headline)
                .accessibilityIdentifier("knowledgeReview.appearance.sheet")
            ScrollView {
                VStack(alignment: .leading, spacing: AeroricSpace.l) {
                    Text("只影响本窗口；不会改变系统设置、interactionMode 或任务授权。")
                        .foregroundStyle(AeroricColor.textSecondary)
                    Text("配色预览").font(.subheadline.bold())
                    NativeSegmentedPicker(
                        selection: $appearance, groupTitle: "配色预览",
                        segments: ReviewAppearanceOverride.allCases.map {
                            .init(value: $0, title: LocalizedStringKey($0.title))
                        }
                    )
                    Text("面板材质").font(.subheadline.bold())
                    NativeSegmentedPicker(
                        selection: $surface, groupTitle: "面板材质",
                        segments: SurfacePreference.allCases.map {
                            .init(value: $0, title: LocalizedStringKey($0.rawValue))
                        }
                    )
                    Toggle("演示减少透明度（只能加强）", isOn: $reduceTransparency)
                        .accessibilityIdentifier("knowledgeReview.preview.reduceTransparency")
                    Toggle("演示增强对比（只能加强）", isOn: $increaseContrast)
                        .accessibilityIdentifier("knowledgeReview.preview.increaseContrast")
                    Toggle("演示减少动效（只能加强）", isOn: $reduceMotion)
                        .accessibilityIdentifier("knowledgeReview.preview.reduceMotion")
                    Divider()
                    knowledgeReadout("system.reduceTransparency", "系统减少透明度", systemTransparency ? "on" : "off")
                    knowledgeReadout("system.increaseContrast", "系统增强对比", systemContrast ? "on" : "off")
                    knowledgeReadout("system.reduceMotion", "系统减少动效", systemMotion ? "on" : "off")
                    knowledgeReadout("effective.reduceTransparency", "有效减少透明度", effectiveTransparency ? "on" : "off")
                    knowledgeReadout("effective.increaseContrast", "有效增强对比", effectiveContrast ? "on" : "off")
                    knowledgeReadout("effective.reduceMotion", "有效减少动效", effectiveMotion ? "on" : "off")
                    knowledgeReadout("effective.surface", "实际面板基底",
                                     SurfaceChoice.resolve(preference: surface,
                                                           reduceTransparency: effectiveTransparency).rawValue)
                    knowledgeReadout("effective.appearance", "配色预览值", appearance.rawValue)
                    knowledgeReadout("effective.motion", "实际转换时长",
                                     String(format: "%.3f", AeroricMotion.duration(AeroricMotion.standard, reduceMotion: effectiveMotion)))
                    knowledgeReadout("effective.outline", "实际面板边界宽度",
                                     String(format: "%.1f", NativePanelGeometry.outline(increasedContrast: effectiveContrast).width))
                    Divider()
                    Menu {
                        Picker("窗口尺寸样例", selection: $sizeID) {
                            ForEach(NativePanelGeometry.reviewSamples, id: \.id) { sample in
                                Text(sample.title).tag(sample.id)
                            }
                        }
                    } label: {
                        Label("尺寸：\(NativePanelGeometry.reviewSample(id: sizeID).title)", systemImage: "macwindow")
                    }
                    .accessibilityLabel("窗口尺寸样例")
                    .accessibilityValue(Text(NativePanelGeometry.reviewSample(id: sizeID).title))
                    .accessibilityIdentifier("knowledgeReview.geometry.sample")
                    knowledgeReadout("geometry.request", "请求外框", NativePanelGeometry.reviewSample(id: sizeID).title)
                    knowledgeReadout("geometry.frame", "实测窗口外框", knowledgeSize(measurement.frame.size))
                    knowledgeReadout("geometry.content", "实测内容尺寸", knowledgeSize(measurement.content))
                    knowledgeReadout("geometry.layout", "实测可用内容区", knowledgeSize(measurement.layout))
                    knowledgeReadout("geometry.fits", "当前显示器可容纳请求", measurement.requestedFitsDisplay ? "yes" : "no")
                    if !measurement.requestedFitsDisplay {
                        Text("当前显示器不足以容纳请求，保留实际窗口尺寸；不能用缩放截图代替目标尺寸证据。")
                            .foregroundStyle(AeroricColor.warning)
                    }
                    ForEach(["list", "detail", "references", "impacts"], id: \.self) { key in
                        knowledgeReadout("geometry.panel.\(key)", "实测面板 \(key)", knowledgeRect(panelFrames[key]))
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(NativePanelGeometry.panelInset)
            }
            HStack {
                Spacer()
                Button("关闭预览", action: close)
                    .keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier("knowledgeReview.preview.close")
            }
        }
        .padding(NativePanelGeometry.panelInset * 2)
        .foregroundStyle(AeroricColor.text)
        .background(NativePanelFoundation(preference: surface, reduceTransparency: effectiveTransparency))
        .preferredColorScheme(appearance.colorScheme)
    }
}

private struct KnowledgeReviewSourcePanel: View {
    let source: KnowledgeReviewSource
    let context: String
    let close: () -> Void

    private var paragraphs: [String] {
        (source.excerpt ?? "").components(separatedBy: .newlines)
            .filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            Text(source.title)
                .font(.headline)
                .accessibilityIdentifier("knowledgeReview.source.title")
            Text(context)
                .foregroundStyle(AeroricColor.textSecondary)
                .accessibilityIdentifier("knowledgeReview.source.context")
            Text("仅展开内嵌虚构摘录；不打开文件、URL 或真实历史。")
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.warning)
                .accessibilityIdentifier("knowledgeReview.source.warning")
            ScrollView {
                VStack(alignment: .leading, spacing: AeroricSpace.l) {
                    if paragraphs.isEmpty {
                        Text("来源缺失；不编造回源，不作为当前已核验事实。")
                            .accessibilityIdentifier("knowledgeReview.source.missing")
                    } else {
                        ForEach(Array(paragraphs.enumerated()), id: \.offset) { index, paragraph in
                            Text(paragraph)
                                .fixedSize(horizontal: false, vertical: true)
                                .textSelection(.enabled)
                                .accessibilityElement(children: .combine)
                                .accessibilityIdentifier(
                                    index == 0 ? "knowledgeReview.source.first"
                                    : index == paragraphs.count - 1 ? "knowledgeReview.source.last"
                                    : "knowledgeReview.source.paragraph.\(index)"
                                )
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(NativePanelGeometry.panelInset)
            }
            .background(AeroricColor.layer1)
            HStack {
                Spacer()
                Button("关闭来源", action: close)
                    .keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier("knowledgeReview.source.close")
            }
        }
        .padding(NativePanelGeometry.panelInset * 2)
        .foregroundStyle(AeroricColor.text)
    }
}

private struct KnowledgeReviewEditorPanel: View {
    let revision: Bool
    @Binding var content: String
    @Binding var scope: String
    @Binding var basis: String
    let error: String
    let canApply: Bool
    let cancel: () -> Void
    let apply: () -> Void
    @FocusState private var contentFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            Text(revision ? "修订已确认知识 · 内存草稿" : "编辑候选 · 内存草稿")
                .font(.headline)
                .accessibilityIdentifier("knowledgeReview.sheet.title")
            ScrollView {
                VStack(alignment: .leading, spacing: AeroricSpace.l) {
                    Text("取消不应用；应用只改变本窗口的虚构内容，保留来源和限制。")
                        .foregroundStyle(AeroricColor.textSecondary)
                    Text("示例内容")
                    TextEditor(text: $content)
                        .font(.body)
                        .scrollContentBackground(.hidden)
                        .background(AeroricColor.layer1)
                        .frame(minHeight: 120, idealHeight: 150, maxHeight: 180)
                        .focused($contentFocused)
                        .accessibilityLabel("示例内容")
                        .accessibilityIdentifier("knowledgeReview.draft.content")
                    TextField("适用说明", text: $scope, axis: .vertical)
                        .textFieldStyle(.roundedBorder)
                        .lineLimit(2...4)
                        .accessibilityIdentifier("knowledgeReview.draft.scope")
                    if revision {
                        TextField("纠正依据（虚构示例）", text: $basis, axis: .vertical)
                            .textFieldStyle(.roundedBorder)
                            .lineLimit(2...4)
                            .accessibilityIdentifier("knowledgeReview.draft.basis")
                        Text("应用会保留旧版本和替代关联；Agent 猜测不自动改写用户确认。")
                            .foregroundStyle(AeroricColor.warning)
                    }
                    if !error.isEmpty {
                        Text(error)
                            .foregroundStyle(AeroricColor.danger)
                            .accessibilityIdentifier("knowledgeReview.draft.error")
                    }
                }
                .padding(NativePanelGeometry.panelInset)
            }
            HStack {
                Button("取消", action: cancel)
                    .keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier("knowledgeReview.draft.cancel")
                Spacer()
                Button("应用到样例", action: apply)
                    .disabled(!canApply)
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("knowledgeReview.draft.apply")
            }
        }
        .padding(NativePanelGeometry.panelInset * 2)
        .foregroundStyle(AeroricColor.text)
        .onAppear { contentFocused = true }
    }
}

private struct KnowledgeReviewConfigurationPanel: View {
    @Binding var enabled: Bool
    @Binding var preset: KnowledgeReviewAdmissionPreset
    let cancel: () -> Void
    let apply: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
            Text("项目自动入库 · 固定预设预览")
                .font(.headline)
                .accessibilityIdentifier("knowledgeReview.sheet.title")
            ScrollView {
                VStack(alignment: .leading, spacing: AeroricSpace.l) {
                    Text("取消不应用，应用只改本窗口内存；不会启用真实自动化。")
                        .foregroundStyle(AeroricColor.textSecondary)
                    Toggle("演示自动入库", isOn: $enabled)
                        .accessibilityIdentifier("knowledgeReview.configuration.enabled")
                    Menu {
                        Picker("类型与来源预设", selection: $preset) {
                            ForEach(KnowledgeReviewAdmissionPreset.allCases) { option in
                                Text(option.title).tag(option)
                            }
                        }
                    } label: {
                        Label(preset.title, systemImage: "line.3.horizontal.decrease.circle")
                    }
                    .accessibilityLabel("类型与来源预设")
                    .accessibilityValue(Text(preset.title))
                    .accessibilityIdentifier("knowledgeReview.configuration.preset")
                    Text(preset.detail)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("knowledgeReview.configuration.scope")
                    Text("任务授权另以“默认／Full Access”示例切换；本配置不扩大权限、项目或资料范围。")
                    Text("冲突、敏感占位、依据不足和范围外内容仍留候选，不强行入库。")
                        .foregroundStyle(AeroricColor.warning)
                    Text(KnowledgeReviewFixtures.closingAutomaticAdmissionNote)
                        .font(.system(size: AeroricType.secondary))
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("knowledgeReview.configuration.propagation")
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(NativePanelGeometry.panelInset)
            }
            HStack {
                Button("取消", action: cancel)
                    .keyboardShortcut(.cancelAction)
                    .accessibilityIdentifier("knowledgeReview.configuration.cancel")
                Spacer()
                Button("应用配置到样例", action: apply)
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("knowledgeReview.configuration.apply")
            }
        }
        .padding(NativePanelGeometry.panelInset * 2)
        .foregroundStyle(AeroricColor.text)
    }
}
#endif
