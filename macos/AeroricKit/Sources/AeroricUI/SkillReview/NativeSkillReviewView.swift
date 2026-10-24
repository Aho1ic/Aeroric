#if DEBUG
import AppKit
import SwiftUI

/// An actual native root; all state belongs to this window, with no product services.
@MainActor
public struct NativeSkillReviewView: View {
    @State private var store: SkillReviewStore
    @State private var measurement = SkillReviewWindowMeasurement()
    @State private var relatedExpanded = true
    @Environment(\.accessibilityReduceTransparency) private var systemTransparency
    @Environment(\.colorSchemeContrast) private var systemContrast
    @Environment(\.accessibilityReduceMotion) private var systemMotion

    public init() { _store = State(initialValue: SkillReviewStore()) }
    init(store: SkillReviewStore) { _store = State(initialValue: store) }

    private var transparency: Bool { systemTransparency || store.preview.reduceTransparency }
    private var contrast: Bool { systemContrast == .increased || store.preview.increaseContrast }
    private var motion: Bool { systemMotion || store.preview.reduceMotion }
    private var surface: SurfacePreference { store.preview.glass ? .glass : .classic }
    private var effectiveSurface: SurfacePreference {
        SurfaceChoice.resolve(preference: surface, reduceTransparency: transparency)
    }
    private var colorScheme: ColorScheme? {
        switch store.preview.appearance {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }
    private var sample: NativePanelGeometry.ReviewSample {
        NativePanelGeometry.reviewSample(id: store.preview.sizeID)
    }

    public var body: some View {
        @Bindable var state = store
        GeometryReader { geometry in
            VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                header
                    .padding(.horizontal, NativePanelGeometry.panelInset)
                ScrollView {
                    VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                        readout("scenarioSummary", "场景说明", store.scenario.summary, tone: .accent)
                        if store.preview.interaction == .advanced {
                            readout("interaction", "advanced 呈现（仅示例字段）",
                                    "场景=\(store.scenario.rawValue)；当前项目=\(store.project.title)；执行者=\(store.executor.title)。仅增加字段摘要，无 PTY、Agent 或业务状态变更。")
                        }
                        if geometry.size.width >= 850 {
                            HStack(alignment: .top, spacing: NativePanelGeometry.panelGap) {
                                panel { itemList(compact: false) }.frame(width: 290)
                                panel { detail }
                            }
                        } else {
                            panel { itemList(compact: true) }
                            panel { detail }
                        }
                        panel { workContent }
                        readout("feedback", "本次操作反馈（示例）", store.feedback, tone: .accent)
                        readout("consumption", "真实加载/消费", SkillReviewFixtures.noConsumption)
                        readout("geometry", "本窗口实测外框与内容", measurement.summary)
                        readout("process.pid", "本样例进程", String(ProcessInfo.processInfo.processIdentifier))
                        readout("process.launchID", "本次测试启动标识（非授权）",
                                NativeSkillReviewEntry.testLaunchID(environment: ProcessInfo.processInfo.environment))
                        readout("memory.instance", "窗口内存实例", store.instanceID.uuidString)
                    }
                    .padding(NativePanelGeometry.panelInset)
                }
            }
        }
        .font(.system(size: DSTypography.body))
        .foregroundStyle(Color.dsText)
        .frame(minWidth: 360, minHeight: 420)
        .background {
            NativePanelFoundation(preference: surface, reduceTransparency: transparency).ignoresSafeArea()
        }
        .background {
            SkillReviewWindowProbe(
                requested: CGSize(width: sample.totalWidth, height: sample.totalHeight)
            ) { if measurement != $0 { measurement = $0 } }
        }
        .preferredColorScheme(colorScheme)
        .sheet(item: $state.sheet) { target in
            let size = measurement.sheetSize(target: CGSize(width: sample.totalWidth, height: sample.totalHeight))
            sheetContent(target)
                .frame(width: size.width, height: size.height)
                .preferredColorScheme(colorScheme)
        }
        .transaction { transaction in
            let policy = SkillReviewMotionPolicy.transaction(reduceMotion: motion)
            transaction.animation = policy.animation
            transaction.disablesAnimations = policy.disablesAnimations
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("离线技能审阅 · 全部为示例")
                .font(.headline)
                .foregroundStyle(Color.dsAccent)
                .accessibilityIdentifier("skillReview.notice")
            ViewThatFits(in: .horizontal) {
                HStack { projectPicker; headerActions }
                VStack(alignment: .leading, spacing: 4) {
                    projectPicker
                    scenarioPicker
                    HStack {
                        action("全部重置", "reset") { store.reset() }
                        appearanceButton
                    }
                }
            }
        }
        .padding(.top, NativePanelGeometry.panelInset)
    }

    private var projectPicker: some View {
        Picker("虚构项目", selection: Binding(get: { store.project }, set: store.setProject)) {
            ForEach(SkillReviewProject.allCases) { Text($0.title).tag($0) }
        }
        .pickerStyle(.menu)
        .accessibilityIdentifier("skillReview.project")
    }

    private var headerActions: some View {
        HStack(spacing: 8) {
            scenarioPicker
            action("全部重置", "reset") { store.reset() }
            appearanceButton
        }
    }

    private var scenarioPicker: some View {
        Picker("离线技能场景", selection: Binding(get: { store.scenario }, set: store.load)) {
            ForEach(SkillReviewScenario.allCases) { Text($0.menuTitle).tag($0) }
        }
        .pickerStyle(.menu)
        .accessibilityIdentifier("skillReview.scenario")
    }

    private var appearanceButton: some View {
        NativeIconButton(systemImage: "paintpalette", label: "外观与辅助预览（示例）",
                         identifier: "skillReview.appearance.open") { store.sheet = .appearance }
    }

    @ViewBuilder private func itemList(compact: Bool) -> some View {
        Picker("审阅栏目", selection: Binding(get: { store.section }, set: store.setSection)) {
            ForEach(SkillReviewSection.allCases) { Text($0.title).tag($0) }
        }
        .pickerStyle(.menu)
        .accessibilityIdentifier("skillReview.section")
        if store.visibleItems.isEmpty {
            readout("items.empty", "当前栏目", "此场景没有该栏目示例，请切回有内容的栏目。")
        } else if compact {
            Picker("方法详情（示例）", selection: $store.selectedItemID) {
                ForEach(store.visibleItems) { item in
                    Text("\(item.proposal.title) · 示例").tag(Optional(item.id))
                }
            }
            .pickerStyle(.menu)
            .accessibilityIdentifier("skillReview.itemPicker")
        } else {
            NativeSelectionList(selection: $store.selectedItemID) {
                ForEach(store.visibleItems) { item in
                    VStack(alignment: .leading, spacing: 4) {
                        Text(item.proposal.title + "（示例）").fontWeight(.medium)
                        Text(item.contentState == .discarded ? item.contentState.title : item.consentTitle)
                            .font(.caption).foregroundStyle(Color.dsTextSecondary)
                    }
                    .tag(item.id)
                    .accessibilityElement(children: .combine)
                    .accessibilityLabel(item.proposal.title + "（示例）")
                    .accessibilityValue(Text(item.contentState.title + "；" + item.consentTitle))
                    .accessibilityIdentifier("skillReview.item.\(item.id.rawValue)")
                }
            }
            .frame(height: 320)
        }
        Text("统一管理不等于所有项目启用；本窗口不管理真实技能。")
            .font(.caption).foregroundStyle(Color.dsTextSecondary)
    }

    @ViewBuilder private var detail: some View {
        if let item = store.selectedItem {
            Text(item.proposal.title + "（示例）").font(.title3.bold())
                .accessibilityIdentifier("skillReview.detail.title")
            readout("detail.contentState", "内容状态", item.contentState.title)
            readout("detail.consent", "人工安装同意", item.consentTitle)
            readout("detail.target", "展示对象与拟同意范围（示例）", item.target.summary, tone: .accent)
            readout("detail.install", "独立的演示结果", item.installOutcome.title)
            readout("detail.enabled", "项目版本启用",
                    store.project == .harbor
                        ? item.enabledTarget.map { "星港排程启用 v\($0.version.number) · \($0.version.title)（示例），不是所有任务消费。" }
                            ?? "项目未启用（示例）"
                        : "潮汐笔记未获准、未启用（示例），不自动扩围。")
            availability(item.proposal, suffix: "detail")
            readout("detail.usability", "所展示版本的当前任务使用条件（示例）",
                    store.proposalUsageBlock(for: item.id)
                        ?? "相关、获准且启用、预设情景可用；仍无真实加载/消费证据。",
                    tone: store.proposalUsageBlock(for: item.id) == nil ? .success : .warning)
            readout("detail.purpose", "拟用途（示例）", item.proposal.intendedUse)
            DisclosureGroup("正文、前提与验证要求（示例）") {
                reading(item.proposal.content, identifier: "detail.content", label: "方法正文（示例）")
                readout("detail.conditions", "适用前提（示例）", item.proposal.conditions)
                readout("detail.validation", "验证要求（示例）", item.proposal.validation)
            }
            DisclosureGroup("能力、依赖与范围说明（示例）") {
                readout("detail.capabilities", "能力（示例）", item.proposal.capabilities.joined(separator: "；"))
                readout("detail.dependencies", "依赖（预置，未检测）", item.proposal.dependencies.joined(separator: "；"))
                readout("detail.scope", "范围说明（示例）", item.proposal.scopeNote)
            }
            action("查看内嵌来源（示例）", "source.open") { store.sheet = .source(item.proposal) }
            action("修改草稿/展示对象（示例）", "edit.open",
                   enabled: store.canMutateProject && item.contentState != .discarded) { store.beginEdit() }
            action("确认内容与项目范围（非安装同意）", "content.confirm",
                   enabled: store.canMutateProject && item.contentState != .discarded) { store.confirmContent() }
            action("丢弃未安装草稿（示例）", "content.discard",
                   enabled: store.canMutateProject && item.installedHistory.isEmpty && item.contentState != .discarded) {
                store.discardDraft()
            }
            consentActions(item)
            if store.scenario == .scope {
                action("查看拟扩围说明（仅说明）", "scope.open") { store.sheet = .scope }
            }
            if store.scenario == .updates || store.scenario == .inFlight { updateContent(item) }
        } else {
            Text("请选择一份示例内容；没有真实技能或数据。")
                .accessibilityIdentifier("skillReview.detail.empty")
        }
    }

    @ViewBuilder private func consentActions(_ item: SkillReviewItem) -> some View {
        Divider()
        Text("人工答复（示例）：一次同意不涵盖未来版本或扩大范围。").font(.caption)
        permissionPicker
        action("暂不答（示例）", "consent.defer",
               enabled: store.canMutateProject && item.contentState != .discarded) { store.respond(.unanswered) }
        action("拒绝此申请（示例）", "consent.refuse",
               enabled: store.canMutateProject && item.contentState != .discarded) { store.respond(.refused) }
        action("同意所展示内容、版本与项目范围（示例）", "consent.agree",
               enabled: store.canMutateProject && item.contentState != .discarded) { store.respond(.agreed) }
        action("载入演示安装失败记录", "install.failure",
               enabled: store.canMutateProject && item.hasMatchingConsent) { store.loadInstall(.failure) }
        action("载入演示安装成功记录", "install.success",
               enabled: store.canMutateProject && item.hasMatchingConsent) { store.loadInstall(.success) }
        action("演示项目版本启用（不选任务）", "version.enable",
               enabled: store.canMutateProject && item.canEnable) { store.enableVersion() }
    }

    private var permissionPicker: some View {
        Picker("任务权限情景（不是真实提权）", selection: $store.preview.fullAccess) {
            Text("默认权限（示例）").tag(false)
            Text("Full Access（示例，不代替同意）").tag(true)
        }
        .pickerStyle(.menu)
        .accessibilityIdentifier("skillReview.permission")
    }

    @ViewBuilder private func updateContent(_ item: SkillReviewItem) -> some View {
        Divider()
        action("展示 v1.1 更新候选（不覆盖旧版）", "update.stage",
               enabled: store.canMutateProject && item.id == .boundary) { store.stageUpdate() }
        if let old = item.updateComparisonTarget {
            DisclosureGroup("旧版与更新差异（示例）") {
                readout("update.versions", "版本比较（示例）", "旧版 v\(old.version.number) → 候选 v\(item.proposal.number)")
                readout("update.oldTarget", "本次比较的旧项目对象（非在途历史）", old.summary)
                readout("update.oldSource", "旧来源", old.version.source?.title ?? "无来源")
                readout("update.newSource", "候选来源", item.proposal.source?.title ?? "无来源，不当可用")
                reading(old.version.content, identifier: "update.oldContent", label: "旧正文（示例）")
                reading(item.proposal.content, identifier: "update.newContent", label: "候选正文（示例）")
                readout("update.capabilities", "能力变化（示例）",
                        old.version.capabilities.joined(separator: "；") + "\n→\n" + item.proposal.capabilities.joined(separator: "；"))
                readout("update.dependencies", "依赖变化（示例）",
                        old.version.dependencies.joined(separator: "；") + "\n→\n" + item.proposal.dependencies.joined(separator: "；"))
                readout("update.scope", "范围变化与本次对象（示例）", item.proposal.scopeNote)
            }
        }
        if store.scenario == .inFlight {
            ForEach(SkillReviewInFlightCondition.allCases) { condition in
                action(condition.title, "inFlight.load.\(condition.rawValue)", enabled: store.canMutateProject) {
                    store.loadInFlight(condition)
                }
            }
            readout("inFlight.condition", "当前有限安全记录（示例）",
                    store.inFlightReason ?? "旧版仍获准、安全且宿主允许（预置），不是永久豁免。")
        }
    }

    private var workContent: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("本任务按需选用（全为示例）").font(.headline)
            Picker("虚构任务/工作包", selection: Binding(get: { store.selectedWorkID }, set: store.setWork)) {
                ForEach(store.works) { Text($0.title).tag($0.id) }
            }
            .pickerStyle(.menu)
            .accessibilityIdentifier("skillReview.work")
            Picker("当前执行者（预设，非实际兼容结论）", selection: $store.executor) {
                ForEach(SkillReviewExecutor.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.menu)
            .accessibilityIdentifier("skillReview.executor")
            readout("work.requirement", "工作需求（示例）", store.selectedWork?.requirement ?? "无示例任务")
            if let work = store.selectedWork {
                ForEach(work.references) { reference in
                    readout("reference.\(reference.id.rawValue)", "已选版本与依据（示例）",
                            "\(reference.target.summary)；\(reference.origin.title)；" +
                            (store.referenceBlock(reference, work: work)
                                ?? "旧版/当前版本条件满足（预置），仅可在该情景继续使用；无消费证据。"),
                            tone: store.referenceBlock(reference, work: work) == nil ? .success : .danger)
                    action("查看所选版本来源（示例）", "reference.\(reference.id.rawValue).source") {
                        store.sheet = .source(reference.target.version)
                    }
                    action("取消此任务选用（示例）", "reference.\(reference.id.rawValue).remove",
                           enabled: store.canMutateProject && !work.isInFlight) { store.deselect(reference.id) }
                }
                if work.references.isEmpty { readout("work.empty", "本任务选用", "尚未选用（示例），不是已加载整库。") }
            }
            DisclosureGroup("相关技能与不能选用的原因（示例）", isExpanded: $relatedExpanded) {
                ForEach(store.relatedItems) { item in
                    let block = store.selectionBlock(for: item.id)
                    let version = item.taskSelectionVersion
                    readout("candidate.\(item.id.rawValue)", "\(version.title) v\(version.number)（示例）",
                            "任务拟选对象：\(version.title) · v\(version.number)；" +
                            (block ?? "相关、获准且启用、预设情景可用（示例）；不需要重复使用批准。"),
                            tone: block == nil ? .success : .warning)
                    action("手动选用 \(version.title) v\(version.number)（示例）", "select.\(item.id.rawValue)",
                           enabled: block == nil) { store.select(item.id) }
                }
            }
            action("演示 Agent 按需选用（非整库加载）", "work.agent",
                   enabled: store.canMutateProject && !store.stopRequested && store.selectedWork?.isInFlight == false) {
                store.agentSelect()
            }
            action("Stop：停止当前工作意图（示例）", "work.stop", enabled: store.canMutateProject) { store.requestStop() }
            readout("work.stopState", "Stop 状态（示例）", store.stopRequested ? "已要求停止；不得再继续选用。" : "未请求 Stop。")
            Text("技能后续使用受阻不等于任务完成/失败；不自动 fallback、重启或回放，不承诺清除已消费上下文。")
                .font(.caption).foregroundStyle(Color.dsTextSecondary)
        }
    }

    private func availability(_ version: SkillReviewVersion, suffix: String) -> some View {
        let value = version.availability(for: store.executor)
        return readout("\(suffix).availability", "兼容性预设（不是已安装可用）：\(value.state.title)",
                       "\(store.executor.title) · v\(version.number)；\(value.reason)",
                       tone: value.state == .available ? .success : .warning, symbol: value.state.symbol)
    }

    @ViewBuilder private func sheetContent(_ target: SkillReviewSheet) -> some View {
        switch target {
        case let .edit(draft):
            SkillReviewEditSheet(draft: draft, store: store, surface: effectiveSurface)
        case let .source(version):
            sheetFrame("内嵌虚构来源（示例）") {
                readout("source.version", "来源对应对象", "\(version.title) · v\(version.number)（示例）")
                reading(version.source?.excerpt ?? "没有来源（示例）；不能推断已可用或已验证。",
                        identifier: "source.excerpt", label: "内嵌来源摘录（示例）")
                readout("source.limitation", "来源限制（示例）", version.source?.limitation ?? "无来源。")
                readout("source.conditions", "适用条件（示例）", version.conditions)
                readout("source.validation", "验证要求（示例）", version.validation)
            }
        case .scope:
            sheetFrame("拟扩围说明（示例，不能执行扩围）") {
                readout("scope.explanation", "当前范围与拟变化",
                        "仅星港排程获准；潮汐笔记没有同意记录。统一管理不是全局启用；此页仅供查看，取消不改变范围。Full Access 不扩技能范围，Q30 任务操作权限默认仍仅当前任务。")
            }
        case .appearance:
            appearanceSheet
        }
    }

    private var appearanceSheet: some View {
        @Bindable var state = store
        return sheetFrame("窗口内外观与辅助预览（全为示例）",
                          closeTitle: "关闭（保留当前内存预览）") {
            Picker("浅深色预览", selection: $state.preview.appearance) {
                ForEach(SkillReviewAppearance.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.menu).accessibilityIdentifier("skillReview.appearance.color")
            NativeSegmentedPicker(selection: $state.preview.glass, groupTitle: "面板外观（示例）", segments: [
                .init(value: true, title: "glass"), .init(value: false, title: "classic"),
            ])
            .accessibilityIdentifier("skillReview.appearance.surface")
            Picker("交互呈现（独立示例）", selection: $state.preview.interaction) {
                ForEach(SkillReviewInteraction.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.menu).accessibilityIdentifier("skillReview.appearance.interaction")
            permissionPicker
            Toggle("演示加强：减少透明度", isOn: $state.preview.reduceTransparency)
                .accessibilityIdentifier("skillReview.appearance.transparency")
            Toggle("演示加强：增强对比", isOn: $state.preview.increaseContrast)
                .accessibilityIdentifier("skillReview.appearance.contrast")
            Toggle("演示加强：减少动效", isOn: $state.preview.reduceMotion)
                .accessibilityIdentifier("skillReview.appearance.motion")
            readout("appearance.system", "只读系统值",
                    "减少透明度=\(systemTransparency)；增强对比=\(systemContrast == .increased)；减少动效=\(systemMotion)")
            readout("appearance.effective", "实际 View 消费的有效值",
                    "材质=\(effectiveSurface.rawValue)；减少透明度=\(transparency)；增强对比=\(contrast)；减少动效=\(motion)")
            Picker("目标窗口外框（非内容尺寸）", selection: $state.preview.sizeID) {
                ForEach(NativePanelGeometry.reviewSamples, id: \.id) { Text($0.title).tag($0.id) }
            }
            .pickerStyle(.menu).accessibilityIdentifier("skillReview.appearance.size")
            readout("appearance.geometry", "当前实测", measurement.summary)
            Text("本页即时预览，关闭/Escape 保留当前窗口内存值；全部重置可恢复默认。不写真实偏好。三种选择独立；辅助注入只能加强系统值。")
        }
    }

    private func sheetFrame<Content: View>(
        _ title: String, closeTitle: String = "关闭/取消（不应用）",
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title).font(.headline).accessibilityIdentifier("skillReview.sheet.title")
            ScrollView { VStack(alignment: .leading, spacing: 12, content: content).frame(maxWidth: .infinity, alignment: .leading) }
            action(closeTitle, "sheet.cancel") { store.cancelSheet() }
                .keyboardShortcut(.cancelAction)
        }
        .padding(16)
        .background(NativePanelFoundation(preference: surface, reduceTransparency: transparency))
    }

    private func panel<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        GlassPanel(preference: surface, reduceTransparency: transparency, increaseContrast: contrast) {
            VStack(alignment: .leading, spacing: 8, content: content)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(NativePanelGeometry.panelInset)
        }
    }

    private func reading(_ text: String, identifier: String, label: String) -> some View {
        ScrollView {
            Text(text).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityLabel(Text(label)).accessibilityValue(Text(text))
                .accessibilityIdentifier("skillReview.\(identifier)")
                .padding(8)
        }
        .frame(minHeight: 120, maxHeight: 240)
        .background(Color.dsTextBackground, in: RoundedRectangle(cornerRadius: DSRadius.sm))
    }

    private func readout(_ id: String, _ label: String, _ value: String,
                         tone: SkillReviewTone = .normal, symbol: String? = nil) -> some View {
        Label {
            Text(label + "：" + value).fixedSize(horizontal: false, vertical: true)
        } icon: { Image(systemName: symbol ?? tone.symbol) }
            .font(.system(size: DSTypography.meta))
            .foregroundStyle(tone.color)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(Text(label)).accessibilityValue(Text(value))
            .accessibilityIdentifier("skillReview.\(id)")
    }

    private func action(_ title: String, _ id: String, enabled: Bool = true,
                        _ perform: @escaping () -> Void) -> some View {
        SkillReviewAction(title: title, identifier: "skillReview.\(id)",
                          glass: effectiveSurface == .glass, enabled: enabled, action: perform)
    }
}

private enum SkillReviewTone {
    case normal, accent, success, warning, danger
    var color: Color {
        switch self {
        case .normal: .dsTextSecondary
        case .accent: .dsAccent
        case .success: .dsSuccess
        case .warning: .dsWarning
        case .danger: .dsDanger
        }
    }
    var symbol: String {
        switch self {
        case .normal: "info.circle"
        case .accent: "doc.text.magnifyingglass"
        case .success: "checkmark.circle"
        case .warning: "exclamationmark.triangle"
        case .danger: "xmark.octagon"
        }
    }
}

private struct SkillReviewAction: View {
    let title: String
    let identifier: String
    let glass: Bool
    var enabled = true
    let action: () -> Void
    var body: some View {
        Group {
            if #available(macOS 26.0, *), glass {
                Button(action: action) { Text(title).fixedSize(horizontal: false, vertical: true) }
                    .buttonStyle(.glass).buttonBorderShape(.capsule)
            } else {
                Button(action: action) { Text(title).fixedSize(horizontal: false, vertical: true) }
                    .buttonStyle(.bordered).buttonBorderShape(.capsule)
            }
        }
        .disabled(!enabled).accessibilityIdentifier(identifier)
    }
}

@MainActor
private struct SkillReviewEditSheet: View {
    @State var draft: SkillReviewEditDraft
    let store: SkillReviewStore
    let surface: SurfacePreference
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("修改展示对象（离线示例）").font(.headline)
                .accessibilityIdentifier("skillReview.edit.title")
            Text("应用只改变内存；内容、版本或范围不匹配时旧同意失效。无真实加载/消费。")
            ScrollView {
                VStack(alignment: .leading, spacing: 8) {
                    TextField("方法名（示例）", text: $draft.version.title)
                        .textFieldStyle(.roundedBorder).accessibilityIdentifier("skillReview.edit.name")
                    TextEditor(text: $draft.version.content)
                        .frame(minHeight: 180)
                        .background(Color.dsTextBackground)
                        .accessibilityLabel("方法正文（示例）")
                        .accessibilityIdentifier("skillReview.edit.content")
                    TextField("适用前提（示例）", text: $draft.version.conditions)
                        .textFieldStyle(.roundedBorder).accessibilityIdentifier("skillReview.edit.conditions")
                    TextField("验证要求（示例）", text: $draft.version.validation)
                        .textFieldStyle(.roundedBorder).accessibilityIdentifier("skillReview.edit.validation")
                }
            }
            SkillReviewAction(title: "取消（不应用）", identifier: "skillReview.edit.cancel",
                              glass: surface == .glass) { store.cancelSheet() }.keyboardShortcut(.cancelAction)
            SkillReviewAction(title: "应用到内存（示例）", identifier: "skillReview.edit.apply",
                              glass: surface == .glass) { store.apply(draft) }.keyboardShortcut(.defaultAction)
        }
        .padding(16)
        .background(Color.dsTextBackground)
    }
}

enum SkillReviewMotionPolicy {
    static func transaction(reduceMotion: Bool) -> Transaction {
        var value = Transaction(animation: reduceMotion ? nil : .easeInOut(duration: DSMotion.duration(DSMotion.fast, reduceMotion: false)))
        value.disablesAnimations = reduceMotion
        return value
    }
}

struct SkillReviewWindowMeasurement: Equatable {
    var frame = CGRect.zero
    var content = CGSize.zero
    var layout = CGSize(width: 400, height: 600)
    var fitsDisplay = false
    func sheetSize(target: CGSize) -> CGSize {
        // Shrink with the requested window BEFORE a resize notification: an
        // attached wide sheet must not prevent its parent becoming compact.
        CGSize(
            width: max(300, min(650, min(layout.width, target.width) - 32)),
            height: max(300, min(650, min(layout.height, target.height) - 32))
        )
    }
    var summary: String {
        "外框 \(Int(frame.width))×\(Int(frame.height))pt；内容 \(Int(content.width))×\(Int(content.height))pt；" +
        "布局 \(Int(layout.width))×\(Int(layout.height))pt；屏幕容纳目标=\(fitsDisplay)（实测，不缩放截图）。"
    }
}

/// Standard public AppKit geometry only, scoped to our own identified window.
private struct SkillReviewWindowProbe: NSViewRepresentable {
    let requested: CGSize
    let report: (SkillReviewWindowMeasurement) -> Void
    func makeNSView(context: Context) -> Probe { Probe() }
    func updateNSView(_ view: Probe, context: Context) {
        view.requested = requested
        view.report = report
        view.synchronize()
    }
    static func dismantleNSView(_ view: Probe, coordinator: ()) { view.detach(); view.report = { _ in } }

    final class Probe: NSView {
        var requested = CGSize.zero
        var report: (SkillReviewWindowMeasurement) -> Void = { _ in }
        private weak var observed: NSWindow?
        private var applied: CGSize?
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); synchronize() }
        func synchronize() {
            guard let window, window.identifier?.rawValue == "native-skill-review" else { return }
            if observed !== window {
                detach()
                observed = window
                for event in [NSWindow.didResizeNotification, NSWindow.didMoveNotification, NSWindow.didChangeScreenNotification] {
                    NotificationCenter.default.addObserver(self, selector: #selector(measure), name: event, object: window)
                }
            }
            if requested != .zero, requested != applied {
                applied = requested
                var frame = window.frame
                frame.origin.y += frame.height - requested.height
                frame.size = requested
                window.setFrame(frame, display: true)
            }
            measure()
        }
        @objc private func measure() {
            guard let window = observed else { return }
            let visible = window.screen?.visibleFrame.size ?? .zero
            let value = SkillReviewWindowMeasurement(
                frame: window.frame, content: window.contentView?.bounds.size ?? .zero,
                layout: window.contentLayoutRect.size,
                fitsDisplay: visible.width >= requested.width && visible.height >= requested.height
            )
            DispatchQueue.main.async { [weak self, weak window] in
                guard let self, let window, self.observed === window else { return }
                self.report(value)
            }
        }
        func detach() {
            NotificationCenter.default.removeObserver(self)
            observed = nil
            applied = nil
        }
    }
}
#endif
