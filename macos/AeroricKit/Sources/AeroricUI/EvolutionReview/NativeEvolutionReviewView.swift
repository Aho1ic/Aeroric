#if DEBUG
import AppKit
import SwiftUI

// Isolated task-evolution review window (Q26; execution package
// `docs/native/task-evolution-review-stage-01.md`).
//
// This is a review surface, not product UI and not the production workspace.
// It reuses the shared design components (`NativePanelFoundation`,
// `GlassPanel`, `NativePanelGeometry`, `NativeIconButton`,
// `NativeSegmentedPicker`, `NativeSelectionList`, `Color.ds*`) instead of
// introducing a second visual system, and it renders only the in-memory
// fixtures from `EvolutionReviewFixtures` — no `AeroricClient`, no host, no
// task, no PTY, no session, no statistics, no network, no timer.
//
// The whole file is compiled out of ReleaseLocal, so a release binary cannot
// reach it even through the environment.
//
// ## Accessibility preferences
//
// macOS has no per-app override for "reduce transparency", "increase
// contrast" or "reduce motion" (verified by compiler error on the read-only
// environment keys, 2026-10-14 — see `NativeVisualFoundationReviewView`). The
// sample therefore reads the real system values and reports them
// (`evolutionReview.system.*`), and its injection switches can only
// *strengthen* accessibility (`evolutionReview.effective.*`). No system
// preference is changed, and no injection can defeat a real system setting.

// MARK: - Panel measurement

private struct EvolutionReviewPanelFramesKey: PreferenceKey {
    static let defaultValue: [String: CGRect] = [:]

    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue(), uniquingKeysWith: { _, latest in latest })
    }
}

private struct EvolutionReviewWindowGeometry: Equatable {
    var frame: CGSize = .zero
    var content: CGSize = .zero
    var layout: CGSize = .zero
}

/// Keep the whole window on one display that can fit the requested sample.
/// A sample that does not fit any visible frame is still applied, and the
/// measured geometry then reports what actually happened instead of pretending
/// the requested size was reached.
private enum EvolutionReviewWindowPlacement {
    static func frame(size: CGSize, visibleFrames: [CGRect], preferred: CGRect?) -> CGRect? {
        func fits(_ frame: CGRect) -> Bool {
            frame.width >= size.width && frame.height >= size.height
        }
        let screen = preferred.flatMap { fits($0) ? $0 : nil } ?? visibleFrames.first(where: fits)
        guard let screen else { return nil }
        return CGRect(x: screen.midX - size.width / 2,
                      y: screen.midY - size.height / 2,
                      width: size.width,
                      height: size.height)
    }
}

/// Applies the selected review sample to the hosting window and reports the
/// real geometry. A fresh scene window is laid out by SwiftUI at its own
/// default size after the content first appears, so the size is applied when
/// the view attaches to its window, whenever the sample changes, and once more
/// on the next runloop turn. It never runs on plain layout passes, so a manual
/// resize is not fought. This is review plumbing for the sample's own window —
/// it is not a product window policy.
private struct EvolutionReviewWindowSizer: NSViewRepresentable {
    let sample: NativePanelGeometry.ReviewSample
    let onMeasurement: (EvolutionReviewWindowGeometry) -> Void

    @MainActor
    final class Coordinator: NSObject {
        var appliedSampleID: String?
        weak var window: NSWindow?
        var report: (EvolutionReviewWindowGeometry) -> Void = { _ in }

        deinit { NotificationCenter.default.removeObserver(self) }

        func detach() {
            NotificationCenter.default.removeObserver(self)
            window = nil
            appliedSampleID = nil
            report = { _ in }
        }

        func attach(_ window: NSWindow,
                    sample: NativePanelGeometry.ReviewSample,
                    report: @escaping (EvolutionReviewWindowGeometry) -> Void) {
            self.report = report
            if self.window !== window {
                NotificationCenter.default.removeObserver(self)
                self.window = window
                appliedSampleID = nil
                NotificationCenter.default.addObserver(
                    self, selector: #selector(windowDidResize(_:)),
                    name: NSWindow.didResizeNotification, object: window
                )
            }
            guard appliedSampleID != sample.id else {
                measure(window)
                return
            }
            appliedSampleID = sample.id
            EvolutionReviewWindowSizer.apply(sample, to: window)
            measure(window)
            DispatchQueue.main.async { [weak self, weak window] in
                guard let self, let window, self.window === window,
                      self.appliedSampleID == sample.id else { return }
                EvolutionReviewWindowSizer.apply(sample, to: window)
                self.measure(window)
            }
        }

        @objc private func windowDidResize(_ notification: Notification) {
            if let window { measure(window) }
        }

        func measure(_ window: NSWindow) {
            let geometry = EvolutionReviewWindowGeometry(
                frame: window.frame.size,
                content: window.contentView?.bounds.size
                    ?? window.contentRect(forFrameRect: window.frame).size,
                layout: window.contentLayoutRect.size
            )
            // Never publish SwiftUI state from an NSView layout pass.
            DispatchQueue.main.async { [weak self, weak window] in
                guard let self, let window, self.window === window else { return }
                self.report(geometry)
            }
        }
    }

    final class SizerView: NSView {
        var onWindow: (NSWindow?) -> Void = { _ in }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            onWindow(window)
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> SizerView {
        let view = SizerView(frame: .zero)
        view.onWindow = { window in
            guard let window else {
                context.coordinator.detach()
                return
            }
            context.coordinator.attach(window, sample: sample, report: onMeasurement)
        }
        return view
    }

    func updateNSView(_ view: SizerView, context: Context) {
        view.onWindow = { window in
            guard let window else {
                context.coordinator.detach()
                return
            }
            context.coordinator.attach(window, sample: sample, report: onMeasurement)
        }
        guard let window = view.window else { return }
        context.coordinator.attach(window, sample: sample, report: onMeasurement)
    }

    static func dismantleNSView(_ view: SizerView, coordinator: Coordinator) {
        view.onWindow = { _ in }
        coordinator.detach()
    }

    static func apply(_ sample: NativePanelGeometry.ReviewSample, to window: NSWindow) {
        let size = CGSize(width: sample.totalWidth, height: sample.totalHeight)
        if let frame = EvolutionReviewWindowPlacement.frame(
            size: size,
            visibleFrames: NSScreen.screens.map(\.visibleFrame),
            preferred: window.screen?.visibleFrame
        ) {
            window.setFrame(frame, display: true)
            return
        }
        var frame = window.frame
        frame.size = size
        window.setFrame(frame, display: true)
    }
}

// MARK: - Main sample

/// The same policy is used by the actual detail/inspector transitions, not
/// just the accessibility readout. It never weakens the system preference.
enum EvolutionReviewMotionPolicy {
    static func transaction(reduceMotion: Bool) -> Transaction {
        var transaction = Transaction(animation: reduceMotion ? nil : .easeInOut(
            duration: AeroricMotion.duration(AeroricMotion.standard, reduceMotion: false)
        ))
        transaction.disablesAnimations = reduceMotion
        return transaction
    }
}

public struct NativeEvolutionReviewView: View {
    @State private var store: EvolutionReviewStore
    @State private var appearance: ReviewAppearanceOverride = .system
    @State private var surface: SurfacePreference = .glass
    @State private var injectReduceTransparency = false
    @State private var injectIncreaseContrast = false
    @State private var injectReduceMotion = false
    @State private var sizeID = NativePanelGeometry.defaultReviewSample.id
    @State private var measured = EvolutionReviewWindowGeometry()
    @State private var panelFrames: [String: CGRect] = [:]
    @State private var bandWidth: CGFloat = 0
    @State private var roleSheetPresented = false
    @State private var detailSheetPresented = false
    @FocusState private var focusedControl: String?

    @Environment(\.accessibilityReduceTransparency) private var systemReduceTransparency
    @Environment(\.colorSchemeContrast) private var systemContrast
    @Environment(\.accessibilityReduceMotion) private var systemReduceMotion
    @Environment(\.colorScheme) private var systemColorScheme

    public init(store: EvolutionReviewStore = EvolutionReviewStore()) {
        _store = State(initialValue: store)
    }

    private var sample: NativePanelGeometry.ReviewSample {
        NativePanelGeometry.reviewSample(id: sizeID)
    }

    private var effectiveReduceTransparency: Bool {
        systemReduceTransparency || injectReduceTransparency
    }

    private var effectiveIncreaseContrast: Bool {
        systemContrast == .increased || injectIncreaseContrast
    }

    private var effectiveReduceMotion: Bool {
        systemReduceMotion || injectReduceMotion
    }

    private var effectiveSurface: SurfacePreference {
        SurfaceChoice.resolve(preference: surface, reduceTransparency: effectiveReduceTransparency)
    }

    private var motionDuration: TimeInterval {
        AeroricMotion.duration(AeroricMotion.standard, reduceMotion: effectiveReduceMotion)
    }

    /// Narrow windows go vertical and open the phase selector / details on
    /// demand instead of reserving an always-empty right column.
    private var isCompact: Bool { bandWidth > 0 && bandWidth < 1100 }

    private var sheetSize: CGSize {
        CGSize(
            width: min(560, max(320, (measured.content.width > 0 ? measured.content.width : 552) - 32)),
            height: min(620, max(320, (measured.layout.height > 0 ? measured.layout.height : 572) - 32))
        )
    }

    public var body: some View {
        VStack(spacing: 0) {
            header
            roleSummary
            reviewControls
            band
            readout
        }
        .font(.system(size: AeroricType.body))
        .foregroundStyle(AeroricColor.text)
        .frame(minWidth: 380)
        .background {
            NativePanelFoundation(preference: surface,
                                  reduceTransparency: injectReduceTransparency ? true : nil)
                .ignoresSafeArea()
        }
        .background(EvolutionReviewWindowSizer(sample: sample) { geometry in
            if measured != geometry { measured = geometry }
        })
        .preferredColorScheme(appearance.colorScheme)
        .sheet(isPresented: $roleSheetPresented, onDismiss: { store.cancelDraft() }) {
            EvolutionReviewRoleSheet(store: store, size: sheetSize,
                                     onClose: { roleSheetPresented = false })
        }
        .sheet(isPresented: $detailSheetPresented, onDismiss: closeDetail) {
            EvolutionReviewDetailSheet(store: store, size: sheetSize, onClose: closeDetail)
        }
        .onChange(of: isCompact) { _, _ in
            // A resize must not resurrect a dismissed sheet as an inspector.
            closeDetail()
        }
        .onChange(of: store.detail) { _, target in
            if target == nil { detailSheetPresented = false }
        }
    }

    // MARK: Header

    private var header: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: AeroricSpace.l) { headerIdentity; Spacer(minLength: 8); headerControls }
            VStack(alignment: .leading, spacing: AeroricSpace.m) {
                headerIdentity
                headerControls
            }
        }
        .padding(AeroricSpace.m)
    }

    private var headerIdentity: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(store.sample.taskName)
                .font(.system(size: AeroricType.title, weight: .semibold))
                .lineLimit(1)
                .accessibilityIdentifier("evolutionReview.taskName")
            HStack(spacing: AeroricSpace.m) {
                Text("离线审阅 · 非真实任务")
                    .font(.system(size: AeroricType.caption, weight: .semibold))
                    .foregroundStyle(AeroricColor.warning)
                    .padding(.horizontal, AeroricSpace.m)
                    .padding(.vertical, 2)
                    .background(Capsule().fill(AeroricColor.warning.opacity(0.14)))
                    .overlay(Capsule().strokeBorder(AeroricColor.warning.opacity(0.5), lineWidth: 0.5))
                    .accessibilityLabel("离线审阅 · 非真实任务")
                    .accessibilityValue(Text("离线审阅 · 非真实任务"))
                    .accessibilityIdentifier("evolutionReview.offlineBadge")
                Text("\(store.scenario.code) \(store.scenario.title)")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(1)
                    .accessibilityLabel("当前场景")
                    .accessibilityValue(Text("\(store.scenario.code) \(store.scenario.title)"))
                    .accessibilityIdentifier("evolutionReview.scenario.current")
            }
        }
    }

    private var headerControls: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: AeroricSpace.m) { scenarioControls; appearanceControls }
            VStack(alignment: .leading, spacing: AeroricSpace.m) {
                scenarioControls
                appearanceControls
            }
        }
    }

    private var scenarioControls: some View {
        HStack(spacing: AeroricSpace.m) {
            Picker("场景", selection: Binding(get: { store.scenario }, set: { store.load($0) })) {
                ForEach(EvolutionReviewScenarioID.allCases) { scenario in
                    Text(scenario.menuTitle).tag(scenario)
                }
            }
            .pickerStyle(.menu)
            .labelsHidden()
            .fixedSize()
            .accessibilityLabel("场景菜单")
            .accessibilityIdentifier("evolutionReview.scenario.menu")

            NativeIconButton(systemImage: "arrow.counterclockwise",
                             label: "重置样例",
                             identifier: "evolutionReview.reset",
                             help: "重置样例状态（仅内存）") { store.reset() }
                .focused($focusedControl, equals: "reset")
        }
    }

    private var appearanceControls: some View {
        HStack(spacing: AeroricSpace.m) {
            NativeSegmentedPicker(
                selection: $appearance,
                groupTitle: "外观",
                segments: ReviewAppearanceOverride.allCases.map {
                    .init(value: $0, title: LocalizedStringKey($0.title))
                }
            )
            .fixedSize()

            NativeSegmentedPicker(
                selection: $surface,
                groupTitle: "材质",
                segments: [.init(value: SurfacePreference.glass, title: "glass"),
                           .init(value: SurfacePreference.classic, title: "classic")]
            )
            .fixedSize()
        }
    }

    // MARK: Role summary

    private var roleSummary: some View {
        GlassPanel(preference: surface,
                   reduceTransparency: injectReduceTransparency ? true : nil,
                   increaseContrast: injectIncreaseContrast ? true : nil) {
            VStack(alignment: .leading, spacing: AeroricSpace.m) {
                HStack(spacing: AeroricSpace.m) {
                    Text("角色摘要（示例）")
                        .font(.system(size: AeroricType.caption, weight: .semibold))
                        .foregroundStyle(AeroricColor.textTertiary)
                    Spacer(minLength: 4)
                    Button("配置角色") {
                        store.beginRoleConfiguration()
                        roleSheetPresented = true
                    }
                    .buttonStyle(.bordered)
                    .buttonBorderShape(.capsule)
                    .accessibilityIdentifier("evolutionReview.roleConfig.open")
                }
                ForEach(EvolutionReviewRole.allCases) { role in
                    HStack(spacing: AeroricSpace.m) {
                        Text(role.title)
                            .font(.system(size: AeroricType.secondary, weight: .semibold))
                            .frame(width: 48, alignment: .leading)
                        Text(store.bindingSummary(for: role))
                            .font(.system(size: AeroricType.secondary))
                            .lineLimit(1)
                            .accessibilityLabel(Text("\(role.title)绑定"))
                            .accessibilityValue(Text(store.bindingSummary(for: role)))
                            .accessibilityIdentifier("evolutionReview.role.\(role.identifierComponent).summary")
                        Spacer(minLength: 4)
                        if !isCompact {
                            Text(role.authorityNote)
                                .font(.system(size: AeroricType.caption))
                                .foregroundStyle(AeroricColor.textTertiary)
                                .lineLimit(1)
                        }
                    }
                }
                Text(store.governanceProcessNote)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityLabel("治理进程说明")
                    .accessibilityValue(Text(store.governanceProcessNote))
                    .accessibilityIdentifier("evolutionReview.governance.processNote")
            }
            .padding(NativePanelGeometry.panelInset)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, NativePanelGeometry.panelInset)
    }

    // MARK: Review controls (test injection, never product UI)

    private var reviewControls: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: AeroricSpace.l) { reviewControlLabel; sizePicker; injectionControls }
            VStack(alignment: .leading, spacing: AeroricSpace.m) {
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: AeroricSpace.l) { reviewControlLabel; sizePicker }
                    VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                        reviewControlLabel
                        sizePicker
                    }
                }
                injectionControls
            }
        }
        .padding(AeroricSpace.m)
    }

    private var reviewControlLabel: some View {
        Text("审阅控制（样例注入）")
            .font(.system(size: AeroricType.caption, weight: .semibold))
            .foregroundStyle(AeroricColor.textTertiary)
    }

    private var sizePicker: some View {
        NativeSegmentedPicker(
            selection: $sizeID,
            groupTitle: "样例窗口尺寸",
            segments: NativePanelGeometry.reviewSamples.map {
                .init(value: $0.id, title: LocalizedStringKey($0.title))
            }
        )
        .fixedSize()
    }

    private var injectionControls: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: AeroricSpace.m) { injectionToggles }
            VStack(alignment: .leading, spacing: AeroricSpace.xs) { injectionToggles }
        }
        .font(.system(size: AeroricType.caption))
    }

    @ViewBuilder private var injectionToggles: some View {
        Toggle("注入减少透明度", isOn: $injectReduceTransparency)
            .toggleStyle(.checkbox)
            .accessibilityIdentifier("evolutionReview.inject.reduceTransparency")
        Toggle("注入增强对比", isOn: $injectIncreaseContrast)
            .toggleStyle(.checkbox)
            .accessibilityIdentifier("evolutionReview.inject.increaseContrast")
        Toggle("注入减少动效", isOn: $injectReduceMotion)
            .toggleStyle(.checkbox)
            .accessibilityIdentifier("evolutionReview.inject.reduceMotion")
    }

    // MARK: Band

    private var band: some View {
        GeometryReader { proxy in
            Group {
                if isCompact {
                    compactBand
                } else {
                    regularBand(totalWidth: proxy.size.width)
                }
            }
            .padding(NativePanelGeometry.panelInset)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(panelMeasurement("band"))
            .background {
                GeometryReader { inner in
                    Color.clear
                        .onAppear { bandWidth = inner.size.width }
                        .onChange(of: inner.size.width) { _, newValue in bandWidth = newValue }
                }
            }
        }
        .frame(maxHeight: .infinity)
        .coordinateSpace(name: "evolutionReview.band")
        .onPreferenceChange(EvolutionReviewPanelFramesKey.self) { frames in
            if panelFrames != frames { panelFrames = frames }
        }
    }

    private func regularBand(totalWidth: CGFloat) -> some View {
        let budget = NativePanelGeometry.budget(
            totalWidth: totalWidth,
            sidebarWidth: 320,
            inspectorWidth: 320,
            inspectorVisible: store.inspectorVisible && store.detail != nil
        )
        return HStack(spacing: NativePanelGeometry.panelGap) {
            if budget.sidebarWidth > 0 {
                panel { EvolutionReviewPhaseListPanel(store: store) }
                    .background(panelMeasurement("sidebar"))
                    .frame(width: budget.sidebarWidth)
            }
            panel { EvolutionReviewPhaseDetailPanel(store: store) { target in openDetail(target) } }
                .background(panelMeasurement("workspace"))
                .frame(width: budget.workspaceWidth)
            if budget.inspectorVisible, budget.inspectorWidth > 0 {
                panel { EvolutionReviewInspectorPanel(store: store, onClose: closeDetail) }
                    .background(panelMeasurement("inspector"))
                    .frame(width: budget.inspectorWidth)
                    .transition(.opacity)
            }
        }
    }

    private var compactBand: some View {
        VStack(spacing: NativePanelGeometry.panelGap) {
            panel(fillsHeight: false) { EvolutionReviewCompactSelectorPanel(store: store) }
                .background(panelMeasurement("compactSelector"))
            panel { EvolutionReviewPhaseDetailPanel(store: store) { target in openDetail(target) } }
                .background(panelMeasurement("workspace"))
        }
    }

    private func panel<Content: View>(fillsHeight: Bool = true,
                                     @ViewBuilder content: () -> Content) -> some View {
        GlassPanel(preference: surface,
                   reduceTransparency: injectReduceTransparency ? true : nil,
                   increaseContrast: injectIncreaseContrast ? true : nil) {
            content()
                .frame(maxWidth: .infinity, maxHeight: fillsHeight ? .infinity : nil, alignment: .topLeading)
        }
    }

    /// Probe the real panel band. Non-interactive, adds no AX container.
    private func panelMeasurement(_ name: String) -> some View {
        GeometryReader { proxy in
            Color.clear.preference(
                key: EvolutionReviewPanelFramesKey.self,
                value: [name: proxy.frame(in: .named("evolutionReview.band"))]
            )
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    // MARK: Readout

    private var readout: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 10) {
                leaf("evolutionReview.isolated", "隔离审阅", "仅本地样例 · 无宿主、Agent 或 PTY")
                leaf("evolutionReview.layout.compact", "窄窗纵向布局", isCompact ? "是" : "否")
                Divider().frame(height: 12)
                leaf("evolutionReview.phase.selected", "所选阶段", store.selectedPhase?.title ?? "无")
                leaf("evolutionReview.phase.state", "阶段状态", store.selectedPhase?.state.title ?? "无")
                leaf("evolutionReview.phase.governance", "治理状态",
                     store.selectedPhase?.governance.title ?? "无")
                leaf("evolutionReview.phase.version", "产物版本", store.selectedPhase?.productVersion ?? "无")
                leaf("evolutionReview.supervision", "监督通道", store.sample.supervision.title)
                leaf("evolutionReview.candidate.trial", "候选试用",
                     store.selectedPhase?.candidates.first?.trial.title ?? "无候选")
                leaf("evolutionReview.candidate.gate", "候选门槛",
                     store.selectedPhase?.candidates.first.flatMap { store.gateReason(for: $0) } ?? "可试用或无候选")
                leaf("evolutionReview.detail.visible", "已打开详情", store.detail == nil ? "无" : "有")
                leaf("evolutionReview.dispute.available", "争议裁决示例",
                     store.sample.disputeNote == nil ? "无" : "有")
                Divider().frame(height: 12)
                ForEach(EvolutionReviewRole.allCases) { role in
                    leaf("evolutionReview.role.\(role.identifierComponent).binding",
                         "\(role.title)绑定", store.bindingSummary(for: role))
                    leaf("evolutionReview.role.\(role.identifierComponent).template",
                         "\(role.title)模板", store.templateSummary(for: role))
                }
                Divider().frame(height: 12)
                leaf("evolutionReview.notice", "最近提示", store.notice)
                leaf("evolutionReview.lastAction", "最后操作", store.lastAction)
                leaf("evolutionReview.focus.control", "键盘焦点", focusedControl ?? "无")
                Divider().frame(height: 12)
                leaf("evolutionReview.effective.surface", "有效表面", effectiveSurface.rawValue)
                leaf("evolutionReview.effective.appearance", "有效外观",
                     (appearance.colorScheme ?? systemColorScheme) == .dark ? "dark" : "light")
                leaf("evolutionReview.effective.reduceTransparency", "有效减少透明度",
                     effectiveReduceTransparency ? "on" : "off")
                leaf("evolutionReview.effective.increaseContrast", "有效增强对比",
                     effectiveIncreaseContrast ? "on" : "off")
                leaf("evolutionReview.effective.reduceMotion", "有效减少动效",
                     effectiveReduceMotion ? "on" : "off")
                leaf("evolutionReview.effective.motionDuration", "样例动效时长",
                     String(format: "%.3f", motionDuration))
                leaf("evolutionReview.effective.outline", "面板边界",
                     String(format: "%.1f",
                            NativePanelGeometry.outline(increasedContrast: effectiveIncreaseContrast).width))
                Divider().frame(height: 12)
                leaf("evolutionReview.system.reduceTransparency", "系统减少透明度",
                     systemReduceTransparency ? "on" : "off")
                leaf("evolutionReview.system.increaseContrast", "系统增强对比",
                     systemContrast == .increased ? "on" : "off")
                leaf("evolutionReview.system.reduceMotion", "系统减少动效",
                     systemReduceMotion ? "on" : "off")
                Divider().frame(height: 12)
                leaf("evolutionReview.sample.budget", "目标样本预算", sample.budget.summary)
                leaf("evolutionReview.measured.frame", "实测窗口外框", sizeReadout(measured.frame))
                leaf("evolutionReview.measured.content", "实测窗口内容", sizeReadout(measured.content))
                leaf("evolutionReview.measured.contentLayout", "实测可用内容区", sizeReadout(measured.layout))
                ForEach(["band", "sidebar", "workspace", "inspector", "compactSelector"], id: \.self) { name in
                    leaf("evolutionReview.measured.\(name)", "实测 \(name)（x,y,w,h）",
                         frameReadout(panelFrames[name]))
                }
                leaf("evolutionReview.process.pid", "审阅进程",
                     String(ProcessInfo.processInfo.processIdentifier))
            }
            .padding(.horizontal, AeroricSpace.m)
        }
        .frame(height: NativePanelGeometry.statusBarHeight)
    }

    private func sizeReadout(_ size: CGSize) -> String {
        String(format: "%.1f×%.1f", size.width, size.height)
    }

    private func frameReadout(_ frame: CGRect?) -> String {
        guard let frame else { return "absent" }
        return String(format: "%.1f,%.1f,%.1f,%.1f", frame.minX, frame.minY, frame.width, frame.height)
    }

    private func leaf(_ identifier: String, _ title: String, _ value: String) -> some View {
        Text("\(title)：\(value)")
            .font(.system(size: AeroricType.caption))
            .monospacedDigit()
            .foregroundStyle(AeroricColor.textSecondary)
            .accessibilityLabel(Text(title))
            .accessibilityValue(Text(value))
            .accessibilityIdentifier(identifier)
    }

    // MARK: Detail routing

    /// Wide window: the right inspector opens. Narrow window: a native sheet
    /// opens instead of squeezing a column into the layout.
    private func openDetail(_ target: EvolutionReviewDetailTarget) {
        withTransaction(EvolutionReviewMotionPolicy.transaction(reduceMotion: effectiveReduceMotion)) {
            store.openDetail(target)
            if isCompact {
                detailSheetPresented = true
            } else {
                store.inspectorVisible = true
            }
        }
    }

    private func closeDetail() {
        withTransaction(EvolutionReviewMotionPolicy.transaction(reduceMotion: effectiveReduceMotion)) {
            detailSheetPresented = false
            store.closeDetail()
        }
    }
}

// MARK: - Phase list (regular layout)

private struct EvolutionReviewPhaseListPanel: View {
    @Bindable var store: EvolutionReviewStore

    var body: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelInset) {
            NativeSegmentedPicker(
                selection: Binding(get: { store.track }, set: { store.setTrack($0) }),
                groupTitle: "交付轨道",
                segments: EvolutionReviewTrack.allCases.map {
                    .init(value: $0, title: LocalizedStringKey($0.title))
                }
            )
            .fixedSize()
            Text(store.track.note)
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            NativeSelectionList(selection: Binding(
                get: { store.selectedPhaseID },
                set: { id in
                    if let id {
                        store.selectPhase(id)
                    } else {
                        store.selectedPhaseID = nil
                        store.closeDetail()
                    }
                }
            )) {
                ForEach(store.phases) { phase in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(phase.title)
                            .font(.system(size: AeroricType.body))
                            .lineLimit(1)
                            .accessibilityIdentifier("evolutionReview.phase.\(phase.id)")
                        Text("\(phase.state.title) · \(phase.governance.title)")
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textSecondary)
                            .lineLimit(1)
                    }
                    .frame(height: NativePanelGeometry.navigationRowHeight, alignment: .leading)
                    .tag(phase.id)
                }
            }
            .listStyle(.sidebar)
            .scrollContentBackground(.hidden)
        }
        .padding(NativePanelGeometry.panelInset)
    }
}

// MARK: - Compact selector (narrow layout)

private struct EvolutionReviewCompactSelectorPanel: View {
    @Bindable var store: EvolutionReviewStore

    var body: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelInset) {
            HStack(spacing: AeroricSpace.m) {
                Text("阶段")
                    .font(.system(size: AeroricType.caption, weight: .semibold))
                    .foregroundStyle(AeroricColor.textTertiary)
                Picker("阶段", selection: Binding(
                    get: { store.selectedPhaseID ?? store.phases.first?.id ?? "" },
                    set: { store.selectPhase($0) }
                )) {
                    ForEach(store.phases) { phase in
                        Text("\(phase.title) · \(phase.state.title)").tag(phase.id)
                    }
                }
                .pickerStyle(.menu)
                .labelsHidden()
                .fixedSize()
                .accessibilityLabel("阶段选择")
                .accessibilityIdentifier("evolutionReview.phase.menu")
                Spacer(minLength: 4)
            }
            NativeSegmentedPicker(
                selection: Binding(get: { store.track }, set: { store.setTrack($0) }),
                groupTitle: "交付轨道",
                segments: EvolutionReviewTrack.allCases.map {
                    .init(value: $0, title: LocalizedStringKey($0.title))
                }
            )
            .fixedSize()
        }
        .padding(NativePanelGeometry.panelInset)
    }
}

// MARK: - Phase detail

private struct EvolutionReviewPhaseDetailPanel: View {
    @Bindable var store: EvolutionReviewStore
    let onOpenDetail: (EvolutionReviewDetailTarget) -> Void

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AeroricSpace.l) {
                if let phase = store.selectedPhase {
                    phaseHeader(phase)
                    requirementsSection(phase)
                    opinionsSection(phase)
                    evidenceSection(phase)
                    dependenciesSection(phase)
                    candidatesSection(phase)
                    actionsSection(phase)
                    notesSection
                } else {
                    Text("未选择阶段")
                        .font(.system(size: AeroricType.body))
                        .foregroundStyle(AeroricColor.textSecondary)
                }
            }
            .padding(NativePanelGeometry.panelInset)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .id("\(store.scenario.id)/\(store.selectedPhaseID ?? "none")")
    }

    private func phaseHeader(_ phase: EvolutionReviewPhase) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack(spacing: AeroricSpace.m) {
                Text(phase.title)
                    .font(.system(size: AeroricType.title, weight: .semibold))
                    .lineLimit(1)
                Spacer(minLength: 4)
                chip(phase.state.title, color: stateColor(phase.state))
                chip(phase.governance.title, color: governanceColor(phase.governance))
            }
            HStack(spacing: AeroricSpace.m) {
                Text("产物版本：\(phase.productVersion)")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                Text(phase.track.title)
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            Text(phase.summary)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func requirementsSection(_ phase: EvolutionReviewPhase) -> some View {
        section("既定要求") {
            ForEach(phase.requirements) { requirement in
                HStack(alignment: .firstTextBaseline, spacing: AeroricSpace.m) {
                    Image(systemName: requirement.satisfied ? "checkmark.circle" : "exclamationmark.circle")
                        .foregroundStyle(requirement.satisfied ? AeroricColor.success : AeroricColor.warning)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(requirement.title)
                            .font(.system(size: AeroricType.secondary))
                        if let note = requirement.note {
                            Text(note)
                                .font(.system(size: AeroricType.caption))
                                .foregroundStyle(AeroricColor.textTertiary)
                        }
                    }
                    Spacer(minLength: 4)
                    Text(requirement.satisfied ? "已满足" : "未满足")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(requirement.satisfied ? AeroricColor.success : AeroricColor.warning)
                }
            }
        }
    }

    private func opinionsSection(_ phase: EvolutionReviewPhase) -> some View {
        Group {
            if !phase.opinions.isEmpty {
                section("意见 / 问题") {
                    ForEach(phase.opinions) { opinion in
                        Button {
                            onOpenDetail(.opinion(phaseID: phase.id, opinionID: opinion.id))
                        } label: {
                            HStack(alignment: .firstTextBaseline, spacing: AeroricSpace.m) {
                                chip(opinion.resolutionNote == nil ? opinion.severity.title : "历史 · 已处理",
                                     color: opinion.resolutionNote == nil
                                        ? severityColor(opinion.severity) : AeroricColor.textSecondary)
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(opinion.title)
                                        .font(.system(size: AeroricType.secondary))
                                        .lineLimit(2)
                                    Text("\(opinion.sourceRole.title) · \(opinion.sourceKind.title)")
                                        .font(.system(size: AeroricType.caption))
                                        .foregroundStyle(AeroricColor.textTertiary)
                                        .lineLimit(1)
                                    if let resolution = opinion.resolutionNote {
                                        Text(resolution)
                                            .font(.system(size: AeroricType.caption))
                                            .foregroundStyle(AeroricColor.textSecondary)
                                            .fixedSize(horizontal: false, vertical: true)
                                    }
                                }
                                Spacer(minLength: 4)
                                Image(systemName: "chevron.right")
                                    .font(.system(size: AeroricType.caption))
                                    .foregroundStyle(AeroricColor.textTertiary)
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("evolutionReview.opinion.\(opinion.id)")
                    }
                }
            }
        }
    }

    private func evidenceSection(_ phase: EvolutionReviewPhase) -> some View {
        Group {
            if !phase.evidence.isEmpty {
                section("证据（虚构内嵌）") {
                    ForEach(phase.evidence) { evidence in
                        Button {
                            onOpenDetail(.evidence(phaseID: phase.id, evidenceID: evidence.id))
                        } label: {
                            HStack(alignment: .firstTextBaseline, spacing: AeroricSpace.m) {
                                VStack(alignment: .leading, spacing: 1) {
                                    Text("\(evidence.title) · \(evidence.version)")
                                        .font(.system(size: AeroricType.secondary))
                                        .lineLimit(1)
                                    Text("\(evidence.kind.title) · \(evidence.sourceRole.title)")
                                        .font(.system(size: AeroricType.caption))
                                        .foregroundStyle(AeroricColor.textTertiary)
                                        .lineLimit(1)
                                }
                                Spacer(minLength: 4)
                                Text("虚构样例")
                                    .font(.system(size: AeroricType.caption))
                                    .foregroundStyle(AeroricColor.textTertiary)
                                Image(systemName: "chevron.right")
                                    .font(.system(size: AeroricType.caption))
                                    .foregroundStyle(AeroricColor.textTertiary)
                            }
                            .padding(AeroricSpace.m)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(AeroricColor.layer1,
                                        in: RoundedRectangle(cornerRadius: AeroricRadius.s, style: .continuous))
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("evolutionReview.evidence.\(evidence.id)")
                    }
                }
            }
        }
    }

    private func dependenciesSection(_ phase: EvolutionReviewPhase) -> some View {
        Group {
            if !phase.dependencies.isEmpty {
                section("依赖影响") {
                    ForEach(phase.dependencies) { dependency in
                        Button {
                            onOpenDetail(.dependency(phaseID: phase.id, dependencyID: dependency.id))
                        } label: {
                            HStack(alignment: .firstTextBaseline, spacing: AeroricSpace.m) {
                                chip(dependency.state.title, color: dependencyColor(dependency.state))
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(dependency.title)
                                        .font(.system(size: AeroricType.secondary))
                                    Text(dependency.reason)
                                        .font(.system(size: AeroricType.caption))
                                        .foregroundStyle(AeroricColor.textTertiary)
                                        .lineLimit(2)
                                }
                                Spacer(minLength: 4)
                                Image(systemName: "chevron.right")
                                    .font(.system(size: AeroricType.caption))
                                    .foregroundStyle(AeroricColor.textTertiary)
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("evolutionReview.dependency.\(dependency.id)")
                    }
                }
            }
        }
    }

    private func candidatesSection(_ phase: EvolutionReviewPhase) -> some View {
        Group {
            if !phase.candidates.isEmpty {
                section("工具候选（能力改进）") {
                    ForEach(phase.candidates) { candidate in
                        VStack(alignment: .leading, spacing: AeroricSpace.m) {
                            Button {
                                onOpenDetail(.candidate(phaseID: phase.id, candidateID: candidate.id))
                            } label: {
                                HStack(alignment: .firstTextBaseline, spacing: AeroricSpace.m) {
                                    Text(candidate.title)
                                        .font(.system(size: AeroricType.secondary, weight: .semibold))
                                    Spacer(minLength: 4)
                                    chip(candidate.trial.title, color: trialColor(candidate.trial))
                                    Image(systemName: "chevron.right")
                                        .font(.system(size: AeroricType.caption))
                                        .foregroundStyle(AeroricColor.textTertiary)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("evolutionReview.candidate.\(candidate.id)")

                            HStack(spacing: AeroricSpace.m) {
                                gateChip("验证", candidate.verification)
                                gateChip("必要评审", candidate.requiredReview)
                                gateChip("既有审批", candidate.approval)
                            }
                            if let reason = store.gateReason(for: candidate) {
                                Text(reason)
                                    .font(.system(size: AeroricType.caption))
                                    .foregroundStyle(AeroricColor.warning)
                                    .accessibilityIdentifier("evolutionReview.candidate.\(candidate.id).gateReason")
                            }
                            if let fallback = candidate.fallbackTool {
                                Text("回退工具：\(fallback)")
                                    .font(.system(size: AeroricType.caption))
                                    .foregroundStyle(AeroricColor.textSecondary)
                            }
                        }
                        .padding(AeroricSpace.m)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(AeroricColor.layer1,
                                    in: RoundedRectangle(cornerRadius: AeroricRadius.s, style: .continuous))
                    }
                }
            }
        }
    }

    private func actionsSection(_ phase: EvolutionReviewPhase) -> some View {
        section("演示动作（仅本地样例状态）") {
            ForEach(relevantActions(for: phase)) { action in
                let reason = store.blockedReason(for: action)
                VStack(alignment: .leading, spacing: 2) {
                    Button(action.title) { store.perform(action) }
                        .buttonStyle(.bordered)
                        .buttonBorderShape(.capsule)
                        .disabled(reason != nil)
                        .accessibilityIdentifier("evolutionReview.action.\(action.identifierComponent)")
                    if let reason {
                        Text(reason)
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityIdentifier("evolutionReview.action.\(action.identifierComponent).reason")
                    }
                }
            }
        }
    }

    private var notesSection: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(store.governanceProcessNote)
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            Text(store.sample.feedbackDeliveryNote)
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            if let preservation = store.sample.preservationNote {
                Text(preservation)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            if store.sample.disputeNote != nil {
                Button("查看 Q24 用户争议裁决示例") { onOpenDetail(.dispute) }
                    .buttonStyle(.bordered)
                    .buttonBorderShape(.capsule)
                    .accessibilityIdentifier("evolutionReview.dispute.open")
            }
        }
        .fixedSize(horizontal: false, vertical: true)
    }

    private func relevantActions(for phase: EvolutionReviewPhase) -> [EvolutionReviewAction] {
        var actions: [EvolutionReviewAction] = []
        if phase.opinions.contains(where: { $0.sourceKind == .machineFeedback }) {
            actions.append(.sendReflectionFeedback)
        }
        if !phase.dependencies.isEmpty { actions.append(.advanceDependents) }
        if phase.candidates.contains(where: { !$0.requiredReview.isSatisfied }) { actions.append(.addCandidateReview) }
        if !phase.candidates.isEmpty { actions.append(.trialCandidate) }
        if store.sample.supervision.isFailing { actions.append(.recoverSupervision) }
        if phase.state.isClosed { actions.append(.reopenPhase) }
        if phase.state == .reopened {
            actions.append(.completeRemediation)
            actions.append(.releaseRestriction)
        }
        if phase.candidates.contains(where: { $0.trial == .trialing }) { actions.append(.trialFailureFallback) }
        return actions
    }

    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            Text(title)
                .font(.system(size: AeroricType.caption, weight: .semibold))
                .foregroundStyle(AeroricColor.textTertiary)
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func chip(_ title: String, color: Color) -> some View {
        Text(title)
            .font(.system(size: AeroricType.caption, weight: .semibold))
            .foregroundStyle(color)
            .padding(.horizontal, AeroricSpace.m)
            .padding(.vertical, 1)
            .background(Capsule().fill(color.opacity(0.14)))
            .overlay(Capsule().strokeBorder(color.opacity(0.5), lineWidth: 0.5))
    }

    private func gateChip(_ name: String, _ state: EvolutionReviewGateState) -> some View {
        Text("\(name)：\(state.title)")
            .font(.system(size: AeroricType.caption))
            .foregroundStyle(state.isSatisfied ? AeroricColor.success : AeroricColor.warning)
            .padding(.horizontal, AeroricSpace.m)
            .padding(.vertical, 1)
            .background(Capsule().fill(AeroricColor.text.opacity(0.06)))
    }

    private func stateColor(_ state: EvolutionReviewPhaseState) -> Color {
        switch state {
        case .passed: AeroricColor.success
        case .pendingReflection, .pendingRemediation: AeroricColor.warning
        case .reopened: AeroricColor.danger
        }
    }

    private func governanceColor(_ state: EvolutionReviewGovernanceState) -> Color {
        switch state {
        case .notRequired: AeroricColor.textSecondary
        case .pendingReview: AeroricColor.warning
        case .reviewed: AeroricColor.success
        case .blockingCandidate: AeroricColor.danger
        }
    }

    private func severityColor(_ severity: EvolutionReviewSeverity) -> Color {
        switch severity {
        case .info: AeroricColor.textSecondary
        case .warning: AeroricColor.warning
        case .blocking: AeroricColor.danger
        }
    }

    private func dependencyColor(_ state: EvolutionReviewDependencyState) -> Color {
        switch state {
        case .waiting: AeroricColor.warning
        case .continuing: AeroricColor.textSecondary
        case .restricted: AeroricColor.danger
        case .released: AeroricColor.success
        case .executingUnsettled: AeroricColor.warning
        }
    }

    private func trialColor(_ state: EvolutionReviewTrialState) -> Color {
        switch state {
        case .blocked: AeroricColor.textSecondary
        case .enabled: AeroricColor.success
        case .trialing: AeroricColor.accent
        case .disabledAfterFailure: AeroricColor.danger
        case .fallbackActive: AeroricColor.warning
        }
    }
}

// MARK: - Inspector and detail surfaces

private struct EvolutionReviewInspectorPanel: View {
    @Bindable var store: EvolutionReviewStore
    let onClose: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelInset) {
            HStack(spacing: AeroricSpace.m) {
                Text("检查器")
                    .font(.system(size: AeroricType.title, weight: .semibold))
                Spacer(minLength: 4)
                NativeIconButton(systemImage: "xmark",
                                 label: "关闭检查器",
                                 identifier: "evolutionReview.inspector.close", action: onClose)
            }
            if let detail = store.detail.flatMap({ store.detailPayload(for: $0) }) {
                EvolutionReviewDetailContent(detail: detail)
                    .id(store.detail)
            } else {
                Text("点意见、证据或候选查看只读详情。")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
            }
            Spacer(minLength: 0)
        }
        .padding(NativePanelGeometry.panelInset)
    }
}

private struct EvolutionReviewDetailSheet: View {
    @Bindable var store: EvolutionReviewStore
    let size: CGSize
    let onClose: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            HStack {
                Text("详情（只读样例）")
                    .font(.system(size: AeroricType.title, weight: .semibold))
                Spacer()
                Button("关闭") { onClose() }
                    .keyboardShortcut(.cancelAction)
                    .buttonStyle(.bordered)
                    .buttonBorderShape(.capsule)
                    .accessibilityIdentifier("evolutionReview.detailSheet.close")
            }
            if let detail = store.detail.flatMap({ store.detailPayload(for: $0) }) {
                EvolutionReviewDetailContent(detail: detail)
                    .id(store.detail)
            } else {
                Text("未选择详情")
                    .font(.system(size: AeroricType.body))
                    .foregroundStyle(AeroricColor.textSecondary)
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: size.width, height: size.height)
    }
}

private struct EvolutionReviewDetailContent: View {
    let detail: EvolutionReviewDetail

    private var paragraphs: [String] {
        detail.body.split(separator: "\n").map(String.init)
    }

    var body: some View {
        // One native scroll owner for metadata and body, in both inspector and
        // sheet. Identifiers belong to its readable leaves, never the container.
        ScrollView {
            VStack(alignment: .leading, spacing: AeroricSpace.m) {
                Text(detail.title)
                    .font(.system(size: AeroricType.body, weight: .semibold))
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityLabel("详情标题")
                    .accessibilityValue(Text(detail.title))
                    .accessibilityIdentifier("evolutionReview.detail.title")
                Text(detail.kind)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                ForEach(detail.fields) { field in
                    HStack(alignment: .firstTextBaseline, spacing: AeroricSpace.m) {
                        Text(field.name)
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textSecondary)
                        Spacer(minLength: 4)
                        Text(field.value)
                            .font(.system(size: AeroricType.caption))
                            .multilineTextAlignment(.trailing)
                            .accessibilityLabel(Text("详情\(field.name)"))
                            .accessibilityValue(Text(field.value))
                            .accessibilityIdentifier("evolutionReview.detail.field.\(field.id)")
                    }
                }
                Text("详情正文（\(paragraphs.count) 段）")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityLabel("详情正文摘要")
                    .accessibilityValue(Text("\(paragraphs.count) 段内嵌虚构内容"))
                    .accessibilityIdentifier("evolutionReview.detail.body")
                VStack(alignment: .leading, spacing: AeroricSpace.m) {
                    ForEach(Array(paragraphs.enumerated()), id: \.offset) { index, paragraph in
                        Text(paragraph)
                            .font(.system(size: AeroricType.secondary))
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .accessibilityIdentifier(paragraphIdentifier(index))
                    }
                }
                .padding(AeroricSpace.m)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(AeroricColor.layer1,
                            in: RoundedRectangle(cornerRadius: AeroricRadius.s, style: .continuous))
                Text(detail.fictionalMarker)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func paragraphIdentifier(_ index: Int) -> String {
        if index == 0 { return "evolutionReview.detail.body.first" }
        if index == paragraphs.count - 1 { return "evolutionReview.detail.body.last" }
        return "evolutionReview.detail.body.paragraph.\(index)"
    }
}

// MARK: - Role configuration sheet

private struct EvolutionReviewRoleSheet: View {
    @Bindable var store: EvolutionReviewStore
    let size: CGSize
    let onClose: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text("角色配置（样例）")
                .font(.system(size: AeroricType.title, weight: .semibold))
            Text("示例连接A/B、示例模型1/2 均为虚构；无密钥或 URL 输入，不联网测试，不推断真实模型能力。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)

            ScrollView {
                VStack(alignment: .leading, spacing: AeroricSpace.l) {
                    templateSection
                    ForEach(EvolutionReviewRole.allCases) { role in
                        roleForm(role)
                    }
                    Text(store.governanceProcessNote)
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .frame(maxHeight: .infinity)

            HStack(spacing: AeroricSpace.m) {
                Spacer()
                Button("取消") {
                    store.cancelDraft()
                    onClose()
                }
                .keyboardShortcut(.cancelAction)
                .buttonStyle(.bordered)
                .buttonBorderShape(.capsule)
                .accessibilityIdentifier("evolutionReview.roleConfig.cancel")
                Button("应用") {
                    store.applyDraft()
                    onClose()
                }
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .buttonBorderShape(.capsule)
                .accessibilityIdentifier("evolutionReview.roleConfig.apply")
            }
            Text("应用只更新当前样例内存：\(EvolutionReviewStore.sampleOnlyNotice)")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
        }
        .padding(AeroricSpace.xl)
        .frame(width: size.width, height: size.height)
    }

    private var templateSection: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            Text("示例全局三角色模板（只读）")
                .font(.system(size: AeroricType.caption, weight: .semibold))
                .foregroundStyle(AeroricColor.textTertiary)
            ForEach(EvolutionReviewRole.allCases) { role in
                HStack(spacing: AeroricSpace.m) {
                    Text(role.title)
                        .font(.system(size: AeroricType.secondary, weight: .semibold))
                        .frame(width: 48, alignment: .leading)
                    Text(store.templateSummary(for: role))
                        .font(.system(size: AeroricType.secondary))
                        .accessibilityLabel(Text("\(role.title)模板"))
                        .accessibilityValue(Text(store.templateSummary(for: role)))
                        .accessibilityIdentifier("evolutionReview.roleConfig.template.\(role.identifierComponent)")
                    Spacer(minLength: 4)
                }
            }
        }
        .padding(AeroricSpace.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(AeroricColor.layer1,
                    in: RoundedRectangle(cornerRadius: AeroricRadius.s, style: .continuous))
    }

    private func roleForm(_ role: EvolutionReviewRole) -> some View {
        let binding = store.draftBinding(for: role)
        return VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack(spacing: AeroricSpace.m) {
                Text(role.title)
                    .font(.system(size: AeroricType.body, weight: .semibold))
                Spacer(minLength: 4)
                Text(store.bindingSummary(for: role))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(1)
            }
            Toggle("覆盖模板", isOn: Binding(
                get: { binding.mode == .overriding },
                set: { store.setDraftMode($0 ? .overriding : .inheriting, for: role) }
            ))
            .toggleStyle(.switch)
            .accessibilityLabel(Text("\(role.title)覆盖模板"))
            .accessibilityIdentifier("evolutionReview.roleConfig.\(role.identifierComponent).override")

            if binding.mode == .overriding {
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: AeroricSpace.m) { overrideControls(role, binding: binding) }
                    VStack(alignment: .leading, spacing: AeroricSpace.m) {
                        overrideControls(role, binding: binding)
                    }
                }
            }
        }
        .padding(AeroricSpace.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(AeroricColor.layer1,
                    in: RoundedRectangle(cornerRadius: AeroricRadius.s, style: .continuous))
    }

    @ViewBuilder
    private func overrideControls(_ role: EvolutionReviewRole, binding: EvolutionReviewRoleBinding) -> some View {
        Picker("连接", selection: Binding(
            get: { binding.overrideConnectionID ?? store.template(for: role).connectionID },
            set: { store.setDraftConnection($0, for: role) }
        )) {
            ForEach(EvolutionReviewFixtures.connections) { connection in
                Text(connection.name).tag(connection.id)
            }
        }
        .pickerStyle(.menu)
        .fixedSize()
        .accessibilityLabel(Text("\(role.title)连接"))
        .accessibilityIdentifier("evolutionReview.roleConfig.\(role.identifierComponent).connection")

        Picker("模型", selection: Binding(
            get: { binding.overrideModelID ?? store.template(for: role).modelID },
            set: { store.setDraftModel($0, for: role) }
        )) {
            ForEach(EvolutionReviewFixtures.models) { model in
                Text(model.name).tag(model.id)
            }
        }
        .pickerStyle(.menu)
        .fixedSize()
        .accessibilityLabel(Text("\(role.title)模型"))
        .accessibilityIdentifier("evolutionReview.roleConfig.\(role.identifierComponent).model")

        Button("恢复继承") { store.restoreInheritanceInDraft(for: role) }
            .buttonStyle(.bordered)
            .buttonBorderShape(.capsule)
            .accessibilityIdentifier("evolutionReview.roleConfig.\(role.identifierComponent).restore")
    }
}
#endif
