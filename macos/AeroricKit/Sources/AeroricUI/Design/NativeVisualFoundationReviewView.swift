#if DEBUG
import AppKit
import SwiftUI

// Isolated native visual foundation review window (stage 01, DEBUG only).
//
// This is a review surface, not product UI: it renders the shared components
// (`GlassPanel`, `NativePanelGeometry`, `NativeIconButtonStyle`,
// `NativeSelection`) at the approved sample sizes so the shape, material and
// accessibility behaviour can be judged in a real window. It uses local
// fixtures only — no `AeroricClient`, no wire parsing, no host, no task, no
// PTY (INV-02). The whole file is compiled out of release builds, so a
// ReleaseLocal binary cannot reach it even through the environment.
//
// ## Why the accessibility preferences are pinned instead of injected
//
// macOS exposes no per-app override for "reduce transparency", "increase
// contrast" or "reduce motion", and SwiftUI's `\.accessibilityReduceTransparency`,
// `\.colorSchemeContrast` and `\.accessibilityReduceMotion` are read-only
// environment keys (`.environment(_:_:)` does not accept them — verified by
// compiler error, 2026-10-14). The stage-01 rule is also explicit that the
// user's global system settings must not be flipped to make a test pass.
//
// So the window does both halves honestly:
//   * the real system values are read and reported (`review.system.*`), and
//   * the sample pins the values it wants to demonstrate through the shared
//     components' documented test/preview parameters (`review.effective.*`).
// Screenshots show the pinned values' actual effect; the system readout shows
// what the machine really had.

/// Entry policy for the review window.
///
/// Active only when **all** of these hold:
///
/// 1. `AERORIC_NATIVE_VISUAL_REVIEW=1` (explicit opt-in),
/// 2. `AERORIC_ENV` is `ui-test` (the label `native:ui-test` injects),
/// 3. `AERORIC_DATA_DIR` is set and lives under `~/Library/Caches`
///    (the isolated scratch location the UI runner uses — never a
///    production data directory).
///
/// The type itself only exists in DEBUG builds; the app additionally guards
/// the call site with `#if DEBUG`, so a release build has no review entry to
/// enable.
public enum NativeVisualReviewEntry {
    public static let flagKey = "AERORIC_NATIVE_VISUAL_REVIEW"
    public static let environmentKey = "AERORIC_ENV"
    public static let dataDirectoryKey = "AERORIC_DATA_DIR"
    public static let requiredEnvironment = "ui-test"
    public static let requiredFlagValue = "1"

    public static func isActive(environment: [String: String]) -> Bool {
        guard environment[flagKey] == requiredFlagValue else { return false }
        guard environment[environmentKey] == requiredEnvironment else { return false }
        guard let directory = environment[dataDirectoryKey],
              !directory.isEmpty else { return false }
        return isIsolatedDataDirectory(directory)
    }

    /// The review window must not run against a production data directory.
    /// `~/Library/Caches` is where the UI runner keeps its scratch (and the
    /// repo itself lives under TCC-protected `~/Downloads`, so a scratch
    /// inside the repo would raise a consent sheet instead of running).
    public static func isIsolatedDataDirectory(_ path: String, home: String = NSHomeDirectory()) -> Bool {
        guard path.hasPrefix("/"),
              path == path.trimmingCharacters(in: .whitespacesAndNewlines),
              !path.split(separator: "/").contains(where: { $0 == "." || $0 == ".." })
        else { return false }
        let cachesURL = URL(fileURLWithPath: home, isDirectory: true)
            .appendingPathComponent("Library/Caches", isDirectory: true)
            .standardizedFileURL
        let candidateURL = URL(fileURLWithPath: path, isDirectory: true)
            .standardizedFileURL
        guard candidateURL.path.hasPrefix(cachesURL.path + "/") else { return false }
        // Foundation leaves a symlink unresolved when its final child does
        // not exist. Explicitly reject symlinked descendants/ancestors,
        // including dangling links, before checking the canonical boundary.
        var ancestor = candidateURL
        var isDirectory: ObjCBool = false
        while ancestor.path != cachesURL.path {
            if (try? FileManager.default.destinationOfSymbolicLink(atPath: ancestor.path)) != nil {
                return false
            }
            if FileManager.default.fileExists(atPath: ancestor.path, isDirectory: &isDirectory),
               !isDirectory.boolValue {
                return false
            }
            ancestor.deleteLastPathComponent()
        }
        let caches = cachesURL.resolvingSymlinksInPath().path
        let candidate = candidateURL.resolvingSymlinksInPath().path
        guard candidate.hasPrefix(caches + "/") else { return false }
        return true
    }
}

/// DEBUG-only owner of the Visual sample's standard window. The entry policy
/// remains unchanged; even direct construction with an inactive request is inert.
/// Runtime route/environment and real AX still need the isolated UI run.
@MainActor
public final class NativeVisualReviewWindowLifecycle: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private let active: Bool
    private var controller: NSWindowController?

    public override init() {
        active = NativeVisualReviewEntry.isActive(environment: ProcessInfo.processInfo.environment)
        super.init()
    }

    init(environment: [String: String]) {
        active = NativeVisualReviewEntry.isActive(environment: environment)
        super.init()
    }

    public var windowTitle: String { active ? "原生视觉基础审阅" : "" }
    var windowIdentifier: NSUserInterfaceItemIdentifier? {
        active ? NSUserInterfaceItemIdentifier("native-visual-review") : nil
    }
    var isWindowCreated: Bool { controller?.window != nil }

    public func applicationDidFinishLaunching(_ notification: Notification) {
        showReviewWindow()
    }

    public func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        showReviewWindow()
        return false
    }

    public func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    public func showReviewWindow() {
        guard let identifier = windowIdentifier else { return }
        if let window = controller?.window {
            if window.isMiniaturized { window.deminiaturize(nil) }
            window.makeKeyAndOrderFront(nil)
            NSApplication.shared.activate(ignoringOtherApps: true)
            return
        }
        let window = NSWindow(
            contentRect: CGRect(x: 0, y: 0, width: 1260, height: 860),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered, defer: false
        )
        window.title = windowTitle
        window.identifier = identifier
        window.isRestorable = false
        window.isReleasedWhenClosed = false
        window.delegate = self
        window.contentViewController = NSHostingController(rootView: NativeVisualFoundationReviewView())
        window.center()
        controller = NSWindowController(window: window)
        window.makeKeyAndOrderFront(nil)
        NSApplication.shared.activate(ignoringOtherApps: true)
    }

    public func windowWillClose(_ notification: Notification) {
        guard let closing = notification.object as? NSWindow,
              controller?.window === closing else { return }
        controller = nil // Fresh View/@State next open; never touch another window.
    }
}

/// Appearance override for the review window. `system` follows the user's
/// real setting; the other two pin the window so light/dark can be compared
/// without touching any global system preference.
public enum ReviewAppearanceOverride: String, CaseIterable, Sendable {
    case system
    case light
    case dark

    public var title: String {
        switch self {
        case .system: "跟随系统"
        case .light: "浅色"
        case .dark: "深色"
        }
    }

    var colorScheme: ColorScheme? {
        switch self {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }
}

public struct NativeVisualFoundationReviewView: View {
    public init() {}

    @State private var sampleID = NativePanelGeometry.defaultReviewSample.id
    @State private var surface: SurfacePreference = .glass
    @State private var appearance: ReviewAppearanceOverride = .system
    @State private var injectReduceTransparency = false
    @State private var injectIncreaseContrast = false
    @State private var injectReduceMotion = false
    @State private var selectedRow: String? = "review.row.aeroric"
    @State private var filterText = ""
    @State private var draftText = "多行输入样本：\n第二行用于检查行高与光标。"
    @State private var lastAction = "无"
    @State private var pulseExpanded = false
    @State private var measuredFrames: [String: CGRect] = [:]
    @State private var windowGeometry = ReviewWindowGeometry()
    @FocusState private var refreshHasFocus: Bool

    // The real system preferences, read by this view. Reported separately so
    // "the sample was pinned" can never be confused with "the user's system
    // setting is on".
    @Environment(\.accessibilityReduceTransparency) private var systemReduceTransparency
    @Environment(\.colorSchemeContrast) private var systemContrast
    @Environment(\.accessibilityReduceMotion) private var systemReduceMotion
    @Environment(\.colorScheme) private var systemColorScheme

    private var sample: NativePanelGeometry.ReviewSample {
        NativePanelGeometry.reviewSample(id: sampleID)
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

    private var motionDuration: TimeInterval {
        AeroricMotion.duration(AeroricMotion.standard, reduceMotion: effectiveReduceMotion)
    }

    private var effectiveSurface: SurfacePreference {
        SurfaceChoice.resolve(preference: surface, reduceTransparency: effectiveReduceTransparency)
    }

    private var textActionMaterial: String {
        if #available(macOS 26.0, *), effectiveSurface == .glass { return "Liquid Glass" }
        return "系统按钮"
    }

    public var body: some View {
        VStack(spacing: 0) {
            harnessControls
            harnessReadout
            sampleArea
        }
        .font(.system(size: AeroricType.body))
        .foregroundStyle(AeroricColor.text)
        // Fixture dimensions describe the standard window's OUTER frame.
        // A 600pt content minimum would add the native titlebar on top.
        .frame(minWidth: 400)
        .background {
            NativePanelFoundation(preference: surface,
                                  reduceTransparency: injectReduceTransparency ? true : nil)
                .ignoresSafeArea()
        }
        .background(ReviewWindowSizer(sample: sample) { geometry in
            if windowGeometry != geometry { windowGeometry = geometry }
        })
        .preferredColorScheme(appearance.colorScheme)
    }

    // MARK: - Sample (the part that carries the reviewed geometry)

    private var sampleArea: some View {
        GeometryReader { proxy in
            let budget = NativePanelGeometry.budget(
                totalWidth: proxy.size.width,
                sidebarWidth: sample.sidebarWidth,
                inspectorWidth: sample.inspectorWidth,
                inspectorVisible: sample.inspectorVisible
            )
            HStack(spacing: NativePanelGeometry.panelGap) {
                if budget.sidebarWidth > 0 {
                    panel { sidebarSample }
                        .background(panelMeasurement("sidebar"))
                        .frame(width: budget.sidebarWidth)
                }
                panel { workspaceSample }
                    .background(panelMeasurement("workspace"))
                    .frame(width: budget.workspaceWidth)
                if budget.inspectorVisible, budget.inspectorWidth > 0 {
                    panel { inspectorSample }
                        .background(panelMeasurement("inspector"))
                        .frame(width: budget.inspectorWidth)
                }
            }
            .padding(NativePanelGeometry.panelInset)
            // Account for the fixture's 1pt outer allowance on both sides,
            // without promising control over NSWindow's corner geometry.
            .padding(.horizontal, NativePanelGeometry.outlineInset)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(panelMeasurement("band"))
        }
        .coordinateSpace(name: "review.panelBand")
        .onPreferenceChange(ReviewPanelFramesKey.self) { frames in
            if measuredFrames != frames { measuredFrames = frames }
        }
    }

    /// Probe the actual shared panel, before the outside column frame.
    /// These noninteractive backgrounds add no AX container identifiers.
    private func panelMeasurement(_ name: String) -> some View {
        GeometryReader { proxy in
            Color.clear.preference(
                key: ReviewPanelFramesKey.self,
                value: [name: proxy.frame(in: .named("review.panelBand"))]
            )
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    /// One shared panel. The accessibility preferences are pinned only while
    /// the corresponding injection is on; otherwise the panel reads the real
    /// system values through its environment.
    private func panel<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        GlassPanel(
            preference: surface,
            reduceTransparency: injectReduceTransparency ? true : nil,
            increaseContrast: injectIncreaseContrast ? true : nil
        ) {
            content()
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
    }

    private var sidebarSample: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelInset) {
            HStack(spacing: 4) {
                Text("项目")
                    .font(.system(size: AeroricType.caption, weight: .semibold))
                    .foregroundStyle(AeroricColor.textTertiary)
                Spacer(minLength: 4)
                NativeIconButton(systemImage: "plus",
                                 label: "新建任务样本",
                                 identifier: "review.sidebar.new",
                                 help: "新建任务（样本）") { lastAction = "新建任务" }
                NativeIconButton(systemImage: "magnifyingglass",
                                 label: "搜索样本",
                                 identifier: "review.sidebar.search") { lastAction = "搜索" }
            }
            // System-painted selection (ADR-001 §4): no hand-drawn active
            // background anywhere in this sample.
            NativeSelectionList(selection: $selectedRow) {
                ForEach(Self.sidebarRows, id: \.id) { row in
                    Text(row.title)
                        .font(.system(size: AeroricType.body))
                        .lineLimit(1)
                        .frame(height: NativePanelGeometry.navigationRowHeight, alignment: .leading)
                        .tag(row.id)
                        .accessibilityIdentifier(row.identifier)
                }
            }
            .listStyle(.sidebar)
            .scrollContentBackground(.hidden)
        }
        .padding(NativePanelGeometry.panelInset)
    }

    private var workspaceSample: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelInset) {
            HStack(spacing: 8) {
                Text("工作区样本")
                    .font(.system(size: AeroricType.title, weight: .semibold))
                    .lineLimit(1)
                Spacer(minLength: 4)
                NativeIconButton(systemImage: "arrow.clockwise",
                                 label: "刷新工作区样本",
                                 identifier: "review.workspace.refresh") { lastAction = "刷新工作区" }
                    .focused($refreshHasFocus)
                NativeIconButton(systemImage: "ellipsis",
                                 label: "更多操作样本",
                                 identifier: "review.workspace.more",
                                 help: "更多操作（样本）") { lastAction = "更多" }
            }
            TextField("单行输入样本", text: $filterText)
                .textFieldStyle(.plain)
                .padding(.horizontal, AeroricSpace.l)
                .padding(.vertical, AeroricSpace.m)
                .background(AeroricColor.layer1, in: Capsule())
                .overlay {
                    Capsule().strokeBorder(AeroricColor.separator2, lineWidth: 0.5)
                        .allowsHitTesting(false)
                }
                .accessibilityLabel("单行输入样本")
                .accessibilityIdentifier("review.input.singleLine")
            // Multi-line input on an opaque reading surface — the same rule
            // the terminal/editor panes follow (no per-row translucency).
            TextEditor(text: $draftText)
                .font(.system(size: AeroricType.body))
                .scrollContentBackground(.hidden)
                .frame(minHeight: 64)
                .padding(NativePanelGeometry.panelInset)
                .background(AeroricColor.layer1, in: readingShape)
                .overlay {
                    readingShape
                        .strokeBorder(AeroricColor.separator2.opacity(0.6), lineWidth: 0.5)
                        .allowsHitTesting(false)
                }
                .accessibilityLabel("多行输入样本")
                .accessibilityIdentifier("review.input.multiLine")
            ScrollView {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(Self.scrollRows, id: \.id) { row in
                        Text(row.title)
                            .font(.system(size: AeroricType.secondary))
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .accessibilityIdentifier(row.identifier)
                    }
                }
            }
            .frame(maxHeight: .infinity)
            .padding(NativePanelGeometry.panelInset)
            .background(AeroricColor.layer1, in: readingShape)
            // No identifier on the ScrollView itself: a container identifier
            // replaces its leaves' identifiers (G51), and the rows above must
            // stay individually queryable.
            HStack(spacing: 8) {
                // Text-bearing action: the system glass button style where
                // the OS provides it, the plain system style on macOS 14.
                if #available(macOS 26.0, *), effectiveSurface == .glass {
                    Button("操作样本") { lastAction = "操作" }
                        .buttonStyle(.glass)
                        .buttonBorderShape(.capsule)
                        .accessibilityIdentifier("review.action.text")
                } else {
                    Button("操作样本") { lastAction = "操作" }
                        .buttonStyle(.bordered)
                        .buttonBorderShape(.capsule)
                        .accessibilityIdentifier("review.action.text")
                }
                Button("播放动效样本") {
                    if effectiveReduceMotion {
                        pulseExpanded.toggle()
                    } else {
                        withAnimation(.easeInOut(duration: motionDuration)) {
                            pulseExpanded.toggle()
                        }
                    }
                }
                .accessibilityIdentifier("review.motion.play")
                Capsule(style: .continuous)
                    .fill(AeroricColor.accent.opacity(0.7))
                    .frame(width: pulseExpanded ? 72 : 16, height: 6)
                    .accessibilityHidden(true)
                Text(lastAction)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityLabel("最后操作")
                    .accessibilityValue(Text(lastAction))
                    .accessibilityIdentifier("review.action.last")
            }
        }
        .padding(NativePanelGeometry.panelInset)
    }

    private var readingShape: RoundedRectangle {
        RoundedRectangle(cornerRadius: NativePanelGeometry.panelRadius, style: .continuous)
    }

    private var inspectorSample: some View {
        VStack(alignment: .leading, spacing: NativePanelGeometry.panelInset) {
            Text("检查器样本")
                .font(.system(size: AeroricType.title, weight: .semibold))
                .lineLimit(1)
            ForEach(Self.inspectorRows, id: \.key) { row in
                HStack(spacing: 8) {
                    Text(row.key)
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                    Spacer(minLength: 4)
                    Text(row.value)
                        .font(.system(size: AeroricType.secondary))
                        .lineLimit(1)
                }
            }
            Spacer(minLength: 0)
            Text("已连接 · 样本")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
        }
        .padding(NativePanelGeometry.panelInset)
    }

    // MARK: - Harness (test injection, never product UI)

    private var harnessControls: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack {
                Text("审阅控制（测试注入）")
                    .font(.system(size: AeroricType.caption, weight: .semibold))
                    .foregroundStyle(AeroricColor.textTertiary)
                Spacer()
                NativeIconButton(systemImage: "questionmark",
                                 label: "帮助样本",
                                 identifier: "review.icon.help",
                                 help: "带 help 的裸图标按钮") { lastAction = "帮助" }
                NativeIconButton(systemImage: "xmark",
                                 label: "禁用样本",
                                 identifier: "review.icon.disabled",
                                 help: "禁用状态样本") { lastAction = "不应触发" }
                    .disabled(true)
            }
            ViewThatFits(in: .horizontal) {
                HStack(spacing: AeroricSpace.l) {
                    samplePicker
                    surfacePicker
                    appearancePicker
                }
                VStack(alignment: .leading, spacing: AeroricSpace.m) {
                    samplePicker
                    HStack(spacing: AeroricSpace.m) {
                        surfacePicker
                        appearancePicker
                    }
                }
            }
            ViewThatFits(in: .horizontal) {
                HStack(spacing: AeroricSpace.l) { injectionToggles }
                VStack(alignment: .leading, spacing: AeroricSpace.xs) { injectionToggles }
            }
        }
        .padding(AeroricSpace.m)
    }

    // Every adjacent choice reuses the shared system picker. Tests address
    // actual named segments; no container identifier can replace leaf IDs.
    private var samplePicker: some View {
        NativeSegmentedPicker(
            selection: $sampleID,
            groupTitle: "样本尺寸",
            segments: NativePanelGeometry.reviewSamples.map {
                .init(value: $0.id, title: LocalizedStringKey($0.title))
            }
        ).fixedSize()
    }

    private var surfacePicker: some View {
        NativeSegmentedPicker(
            selection: $surface, groupTitle: "表面",
            segments: [.init(value: .glass, title: "glass"),
                       .init(value: .classic, title: "classic")]
        ).fixedSize()
    }

    private var appearancePicker: some View {
        NativeSegmentedPicker(
            selection: $appearance, groupTitle: "外观",
            segments: ReviewAppearanceOverride.allCases.map {
                .init(value: $0, title: LocalizedStringKey($0.title))
            }
        ).fixedSize()
    }

    @ViewBuilder private var injectionToggles: some View {
        Toggle("注入减少透明度", isOn: $injectReduceTransparency)
            .toggleStyle(.checkbox)
            .accessibilityIdentifier("review.inject.reduceTransparency")
        Toggle("注入增强对比", isOn: $injectIncreaseContrast)
            .toggleStyle(.checkbox)
            .accessibilityIdentifier("review.inject.increaseContrast")
        Toggle("注入减少动效", isOn: $injectReduceMotion)
            .toggleStyle(.checkbox)
            .accessibilityIdentifier("review.inject.reduceMotion")
    }

    /// Configuration and system truth stay separate, readable AX leaves.
    private var harnessReadout: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 10) {
                leaf("review.isolated", "隔离审阅", "仅本地样本 · 无宿主、Agent 或 PTY")
                leaf("review.effective.surface", "有效表面", effectiveSurface.rawValue)
                leaf("review.effective.controlMaterial", "操作控件材质", textActionMaterial)
                leaf("review.effective.appearance", "有效外观",
                     (appearance.colorScheme ?? systemColorScheme) == .dark ? "dark" : "light")
                leaf("review.effective.reduceTransparency", "有效减少透明度",
                     effectiveReduceTransparency ? "on" : "off")
                leaf("review.effective.increaseContrast", "有效增强对比",
                     effectiveIncreaseContrast ? "on" : "off")
                leaf("review.effective.reduceMotion", "有效减少动效",
                     effectiveReduceMotion ? "on" : "off")
                leaf("review.effective.motionDuration", "样本动效时长",
                     String(format: "%.3f", motionDuration))
                leaf("review.effective.outline", "面板边界",
                     String(format: "%.1f", NativePanelGeometry.outline(
                        increasedContrast: effectiveIncreaseContrast
                     ).width))
                Divider().frame(height: 12)
                leaf("review.system.reduceTransparency", "系统减少透明度",
                     systemReduceTransparency ? "on" : "off")
                leaf("review.system.increaseContrast", "系统增强对比",
                     systemContrast == .increased ? "on" : "off")
                leaf("review.system.reduceMotion", "系统减少动效", systemReduceMotion ? "on" : "off")
                Divider().frame(height: 12)
                leaf("review.sample.budget", "目标样本预算", sample.budget.summary)
                leaf("review.window.frame", "实测窗口外框", sizeReadout(windowGeometry.frame))
                leaf("review.window.content", "实测窗口内容", sizeReadout(windowGeometry.content))
                leaf("review.window.contentLayout", "实测可用内容区", sizeReadout(windowGeometry.layout))
                ForEach(["band", "sidebar", "workspace", "inspector"], id: \.self) { name in
                    leaf("review.measured.\(name)", "实测 \(name)（x,y,w,h）",
                         frameReadout(measuredFrames[name]))
                }
                leaf("review.selection", "系统列表选择", selectedRow ?? "无")
                leaf("review.focus.icon", "刷新按钮键盘焦点", refreshHasFocus ? "on" : "off")
                leaf("review.process.pid", "审阅进程", String(ProcessInfo.processInfo.processIdentifier))
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
        return String(format: "%.1f,%.1f,%.1f,%.1f",
                      frame.minX, frame.minY, frame.width, frame.height)
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

    // MARK: - Fixtures (local, inert)

    private struct Row {
        let id: String
        let title: String
        let identifier: String
    }

    private static let sidebarRows: [Row] = [
        Row(id: "review.row.all", title: "全部任务", identifier: "review.row.all"),
        Row(id: "review.row.aeroric", title: "Aeroric", identifier: "review.row.aeroric"),
        Row(id: "review.row.task", title: "整理原生界面", identifier: "review.row.task"),
        Row(id: "review.row.sample", title: "示例项目", identifier: "review.row.sample"),
    ]

    private static let scrollRows: [Row] = (1...20).map { index in
        Row(id: "review.scroll.\(index)",
            title: "滚动样本行 \(index)",
            identifier: "review.scroll.row.\(index)")
    }

    private static let inspectorRows: [(key: String, value: String)] = [
        ("分支", "main"),
        ("状态", "待输入"),
        ("Agent", "claude"),
        ("模型", "claude-sonnet-4-5"),
    ]
}

private struct ReviewPanelFramesKey: PreferenceKey {
    static let defaultValue: [String: CGRect] = [:]

    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue(), uniquingKeysWith: { _, latest in latest })
    }
}

private struct ReviewWindowGeometry: Equatable {
    var frame: CGSize = .zero
    var content: CGSize = .zero
    var layout: CGSize = .zero
}

/// Keep the whole standard window on one display that can fit the sample.
/// In particular, `center()` can constrain an 860pt sample to an 842pt visible
/// frame on a laptop even when a larger external display is available.
enum ReviewWindowPlacement {
    static func frame(
        size: CGSize, visibleFrames: [CGRect], preferred: CGRect?
    ) -> CGRect? {
        func fits(_ frame: CGRect) -> Bool {
            frame.width >= size.width && frame.height >= size.height
        }
        let screen = preferred.flatMap { fits($0) ? $0 : nil }
            ?? visibleFrames.first(where: fits)
        guard let screen else { return nil }
        return CGRect(
            x: screen.midX - size.width / 2,
            y: screen.midY - size.height / 2,
            width: size.width, height: size.height
        )
    }
}

/// Sizes the hosting window to the selected review sample.
///
/// A fresh scene window is laid out by SwiftUI at its own default size
/// *after* the content first appears, so sizing from `onAppear` loses that
/// race (measured 2026-10-14: the window stayed at SwiftUI's 1000×672 default
/// while the sample asked for 1260×860). This applies the size when the view
/// attaches to its window and whenever the sample changes, and re-asserts once
/// on the next runloop turn. It never runs on plain layout passes, so a manual
/// resize is not fought.
private struct ReviewWindowSizer: NSViewRepresentable {
    let sample: NativePanelGeometry.ReviewSample
    let onMeasurement: (ReviewWindowGeometry) -> Void

    @MainActor
    final class Coordinator: NSObject {
        var appliedSampleID: String?
        weak var window: NSWindow?
        var report: (ReviewWindowGeometry) -> Void = { _ in }

        deinit {
            NotificationCenter.default.removeObserver(self)
        }

        func detach() {
            NotificationCenter.default.removeObserver(self)
            window = nil
            appliedSampleID = nil
            report = { _ in }
        }

        func attach(_ window: NSWindow, sample: NativePanelGeometry.ReviewSample,
                    report: @escaping (ReviewWindowGeometry) -> Void) {
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
            ReviewWindowSizer.apply(sample, to: window)
            measure(window)
            DispatchQueue.main.async { [weak self, weak window] in
                guard let self, let window, self.window === window,
                      self.appliedSampleID == sample.id else { return }
                ReviewWindowSizer.apply(sample, to: window)
                self.measure(window)
            }
        }

        @objc private func windowDidResize(_ notification: Notification) {
            if let window { measure(window) }
        }

        func measure(_ window: NSWindow) {
            let geometry = ReviewWindowGeometry(
                frame: window.frame.size,
                content: window.contentView?.bounds.size
                    ?? window.contentRect(forFrameRect: window.frame).size,
                layout: window.contentLayoutRect.size
            )
            // Do not publish SwiftUI state from an NSView update/layout pass.
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
        if let frame = ReviewWindowPlacement.frame(
            size: size, visibleFrames: NSScreen.screens.map(\.visibleFrame),
            preferred: window.screen?.visibleFrame
        ) {
            window.setFrame(frame, display: true)
            return
        }
        // Do not pretend a smaller frame proves the requested sample. Keep
        // reporting actual geometry; tests also require the whole window
        // to fit a display's visible frame before retaining a screenshot.
        var frame = window.frame
        frame.size = size
        window.setFrame(frame, display: true)
    }
}
#endif
