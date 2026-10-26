import SwiftUI
import AppKit

// Shared selection primitives for adjacent mutually-exclusive controls
// (Task P4-03 Action 5; ADR-001 §4; AGENTS.md native-selection rule).
//
// Rule: navigation, tabs, view-mode selectors and segmented controls use
// the system selection states — `Picker(selection:)` with
// `.pickerStyle(.segmented)` or `List(selection:)`. This file wraps their
// shared styling, keyboard behaviour and accessibility only; it must not
// draw an active background or an underline of its own. Standalone
// switches, checkboxes, disclosure buttons and tree expanders are outside
// the rule (same exemption as the React side).
//
// Leaf call sites provide accessibility identifiers; these containers leave
// them unset to avoid overriding child identifiers. The Picker retains its
// readable `groupTitle` accessibility name.

/// Segmented control over an enum-free Hashable selection. Label-less
/// segments take a title each; the whole control reads as one group to
/// VoiceOver, exactly like the system picker it wraps.
public struct NativeSegmentedPicker<Option: Hashable>: View {
    public struct Segment: Identifiable {
        public let value: Option
        public let title: LocalizedStringKey
        public var id: Option { value }

        public init(value: Option, title: LocalizedStringKey) {
            self.value = value
            self.title = title
        }
    }

    @Binding private var selection: Option
    private let segments: [Segment]
    private let groupTitle: LocalizedStringKey

    /// - Parameter groupTitle: VoiceOver group name for the whole control
    ///   (the segmented chrome itself renders no title text). The call
    ///   site knows what the group is — "视图模式", "连接方式", … — so it
    ///   provides it; this wrapper never hardcodes one.
    public init(selection: Binding<Option>, groupTitle: LocalizedStringKey, segments: [Segment]) {
        _selection = selection
        self.segments = segments
        self.groupTitle = groupTitle
    }

    public var body: some View {
        Picker(groupTitle, selection: $selection) {
            ForEach(segments) { segment in
                Text(segment.title).tag(segment.value)
            }
        }
        .pickerStyle(.segmented)
        // `labelsHidden()` is load-bearing, not cosmetic. A segmented
        // `Picker` renders its title as a **visible label** beside the
        // control; measured 2026-10-13 in the real window, the workspace's
        // VoiceOver group name "工作区视图" was painted as a leading segment
        // the user could see and read — an accessibility affordance that had
        // become product UI, and the first thing making the strip look
        // wrong. Hiding the label removes it from the layout while keeping
        // it as the control's accessibility name, which is the whole point
        // of passing a group title in.
        .labelsHidden()
    }
}

/// AppKit-backed segmented control that can disable individual segments.
/// Use this when a segment must stay visible but unavailable — the central
/// task/preview toolbar does exactly that before a preview target exists.
/// Prefer `NativeSegmentedPicker` for the ordinary all-enabled case.
public struct NativeSegmentedControl<Option: Hashable>: NSViewRepresentable {
    public struct Segment: Identifiable {
        public let value: Option
        public let title: String
        public let isEnabled: Bool

        public var id: Option { value }

        public init(value: Option, title: String, isEnabled: Bool = true) {
            self.value = value
            self.title = title
            self.isEnabled = isEnabled
        }
    }

    @Binding private var selection: Option
    private let segments: [Segment]
    private let groupTitle: String
    @Environment(\.isEnabled) private var isEnabled

    public init(selection: Binding<Option>, groupTitle: String, segments: [Segment]) {
        _selection = selection
        self.segments = segments
        self.groupTitle = groupTitle
    }

    public func makeCoordinator() -> Coordinator {
        Coordinator(selection: $selection, segments: segments)
    }

    public func makeNSView(context: Context) -> NSSegmentedControl {
        let control = NSSegmentedControl()
        control.segmentCount = segments.count
        control.trackingMode = .selectOne
        control.segmentStyle = .capsule
        control.segmentDistribution = .fit
        control.controlSize = .small
        control.target = context.coordinator
        control.action = #selector(Coordinator.select(_:))
        control.setAccessibilityLabel(groupTitle)
        for (index, segment) in segments.enumerated() {
            control.setLabel(segment.title, forSegment: index)
            control.setEnabled(segment.isEnabled, forSegment: index)
        }
        return control
    }

    public func updateNSView(_ control: NSSegmentedControl, context: Context) {
        context.coordinator.selection = $selection
        context.coordinator.segments = segments
        control.setAccessibilityLabel(groupTitle)
        for (index, segment) in segments.enumerated() {
            control.setLabel(segment.title, forSegment: index)
            control.setEnabled(segment.isEnabled, forSegment: index)
            control.setWidth(0, forSegment: index)
        }
        control.selectedSegment = segments.firstIndex { $0.value == selection } ?? -1
        control.isEnabled = isEnabled
        control.invalidateIntrinsicContentSize()
    }

    public final class Coordinator: NSObject {
        var selection: Binding<Option>
        var segments: [Segment]

        init(selection: Binding<Option>, segments: [Segment]) {
            self.selection = selection
            self.segments = segments
        }

        @MainActor @objc func select(_ sender: NSSegmentedControl) {
            let index = sender.selectedSegment
            guard segments.indices.contains(index) else { return }
            let segment = segments[index]
            guard segment.isEnabled else { return }
            selection.wrappedValue = segment.value
        }
    }
}

/// The app's one capsule slider for adjacent mutually-exclusive choices
/// (2026-10-26 UI review, Q5/Q46-a). A system `NSSegmentedControl` in its
/// capsule style: on macOS 26+ AppKit draws it as Liquid Glass with a
/// sliding selection, on 14–25 as the classic capsule. Nothing here paints
/// a selection of its own (ADR-001 §2); only labels, images and size vary.
public struct CapsuleSlider<Option: Hashable>: NSViewRepresentable {
    public enum LabelMode: Sendable {
        case iconAndTitle
        case titleOnly
        /// Every segment shows its icon; only the selected one adds its title.
        case iconsWithSelectedTitle
    }

    public enum Size: Sendable {
        case regular
        case large
    }

    public struct Segment: Identifiable {
        public let value: Option
        public let title: String
        public let systemImage: String?
        /// An asset-catalog image; wins over `systemImage`.
        public let asset: String?
        public let isEnabled: Bool
        /// Tooltip; the title when nil.
        public let help: String?

        public var id: Option { value }

        public init(value: Option, title: String, systemImage: String? = nil, asset: String? = nil,
                    isEnabled: Bool = true, help: String? = nil) {
            self.value = value
            self.title = title
            self.systemImage = systemImage
            self.asset = asset
            self.isEnabled = isEnabled
            self.help = help
        }
    }

    /// What one segment shows.
    public struct Face: Equatable, Sendable {
        public let title: String
        public let showsImage: Bool
    }

    @Binding private var selection: Option
    private let segments: [Segment]
    private let groupTitle: String
    private let mode: LabelMode
    private let size: Size
    private let identifier: String?
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    public init(selection: Binding<Option>, groupTitle: String, mode: LabelMode = .iconAndTitle,
                size: Size = .regular, identifier: String? = nil, segments: [Segment]) {
        _selection = selection
        self.segments = segments
        self.groupTitle = groupTitle
        self.mode = mode
        self.size = size
        self.identifier = identifier
    }

    /// Pure: each segment's face for `selection` under `mode`. A segment
    /// without an image always keeps its title so it is never blank.
    public static func faces(_ segments: [Segment], selection: Option, mode: LabelMode) -> [Face] {
        segments.map { segment in
            let hasImage = segment.asset != nil || segment.systemImage != nil
            switch mode {
            case .titleOnly:
                return Face(title: segment.title, showsImage: false)
            case .iconAndTitle:
                return Face(title: segment.title, showsImage: hasImage)
            case .iconsWithSelectedTitle:
                let showsTitle = !hasImage || segment.value == selection
                return Face(title: showsTitle ? segment.title : "", showsImage: hasImage)
            }
        }
    }

    public func makeCoordinator() -> Coordinator { Coordinator(selection: $selection, segments: segments) }

    public func makeNSView(context: Context) -> NSSegmentedControl {
        let control = NSSegmentedControl()
        control.trackingMode = .selectOne
        control.segmentStyle = .capsule
        control.segmentDistribution = .fit
        control.target = context.coordinator
        control.action = #selector(Coordinator.select(_:))
        return control
    }

    public func updateNSView(_ control: NSSegmentedControl, context: Context) {
        context.coordinator.selection = $selection
        context.coordinator.segments = segments
        control.controlSize = size == .large ? .large : .regular
        control.setAccessibilityLabel(groupTitle)
        if let identifier { control.setAccessibilityIdentifier(identifier) }
        if control.segmentCount != segments.count { control.segmentCount = segments.count }
        let faces = Self.faces(segments, selection: selection, mode: mode)
        let side: CGFloat = size == .large ? 16 : 14
        NSAnimationContext.beginGrouping()
        if reduceMotion { NSAnimationContext.current.duration = 0 }
        for (index, segment) in segments.enumerated() {
            let face = faces[index]
            var image: NSImage?
            if face.showsImage {
                if let asset = segment.asset {
                    image = NSImage(named: asset)?.copy() as? NSImage
                    image?.size = NSSize(width: side, height: side)
                } else if let symbol = segment.systemImage {
                    image = NSImage(systemSymbolName: symbol, accessibilityDescription: segment.title)
                }
                // Public SDK contract for icon-only segments' accessible names.
                image?.accessibilityDescription = segment.title
            }
            control.setImage(image, forSegment: index)
            control.setImageScaling(.scaleProportionallyDown, forSegment: index)
            control.setLabel(face.title, forSegment: index)
            control.setToolTip(segment.help ?? segment.title, forSegment: index)
            control.setEnabled(segment.isEnabled, forSegment: index)
            control.setWidth(0, forSegment: index)
        }
        control.selectedSegment = segments.firstIndex { $0.value == selection } ?? -1
        control.isEnabled = isEnabled
        control.invalidateIntrinsicContentSize()
        NSAnimationContext.endGrouping()
    }

    public final class Coordinator: NSObject {
        var selection: Binding<Option>
        var segments: [Segment]

        init(selection: Binding<Option>, segments: [Segment]) {
            self.selection = selection
            self.segments = segments
        }

        @MainActor @objc func select(_ sender: NSSegmentedControl) {
            guard segments.indices.contains(sender.selectedSegment) else { return }
            let segment = segments[sender.selectedSegment]
            guard segment.isEnabled else { return }
            selection.wrappedValue = segment.value
        }
    }
}

/// Selection-enabled list wrapper. The binding owns the selection state;
/// rows declare their value with `.tag` and the system paints the
/// selection, never this wrapper.
public struct NativeSelectionList<SelectionValue: Hashable, Content: View>: View {
    @Binding private var selection: SelectionValue?
    private let content: () -> Content

    public init(selection: Binding<SelectionValue?>, @ViewBuilder content: @escaping () -> Content) {
        _selection = selection
        self.content = content
    }

    public var body: some View {
        List(selection: $selection, content: content)
    }
}

/// V6's four fixed harness identities. AppKit owns selection, focus and
/// drawing. No custom cell, private subviews, thumb or selection overlay.
public struct NativeHarnessPicker: NSViewRepresentable {
    public struct Harness: Identifiable, Sendable {
        public let id: String
        public let title: String
        public let asset: String
    }
    public static let harnesses = [
        Harness(id: "codex", title: "Codex", asset: "HarnessCodex"),
        Harness(id: "claude", title: "Claude Code", asset: "HarnessClaude"),
        Harness(id: "omp", title: "oh-my-pi", asset: "HarnessOMP"),
        Harness(id: "dsh", title: "DSH", asset: "HarnessDSH"),
    ]

    @Binding private var selection: String
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    public init(selection: Binding<String>) { _selection = selection }

    public func makeCoordinator() -> Coordinator { Coordinator(selection: $selection) }

    public func makeNSView(context: Context) -> NSSegmentedControl {
        let control = NSSegmentedControl()
        control.segmentCount = Self.harnesses.count
        control.trackingMode = .selectOne
        control.segmentStyle = .capsule
        control.segmentDistribution = .fit
        control.controlSize = .small
        control.target = context.coordinator
        control.action = #selector(Coordinator.select(_:))
        control.setAccessibilityLabel("Harness")
        for (index, harness) in Self.harnesses.enumerated() {
            let image = NSImage(named: harness.asset)?.copy() as? NSImage
            image?.size = NSSize(width: NewSessionGeometry.harnessIconSize,
                                 height: NewSessionGeometry.harnessIconSize)
            // Public SDK contract for icon-only segments' accessible names.
            image?.accessibilityDescription = harness.title
            control.setImage(image, forSegment: index)
            control.setImageScaling(.scaleProportionallyDown, forSegment: index)
            control.setToolTip(harness.title, forSegment: index)
        }
        return control
    }

    public func updateNSView(_ control: NSSegmentedControl, context: Context) {
        context.coordinator.selection = $selection
        NSAnimationContext.beginGrouping()
        if reduceMotion { NSAnimationContext.current.duration = 0 }
        for (index, harness) in Self.harnesses.enumerated() {
            control.setLabel(harness.id == selection ? harness.title : "", forSegment: index)
            control.setWidth(0, forSegment: index)
        }
        control.selectedSegment = Self.harnesses.firstIndex { $0.id == selection } ?? -1
        control.isEnabled = isEnabled
        control.invalidateIntrinsicContentSize()
        NSAnimationContext.endGrouping()
    }

    public final class Coordinator: NSObject {
        var selection: Binding<String>
        init(selection: Binding<String>) { self.selection = selection }
        @MainActor @objc func select(_ sender: NSSegmentedControl) {
            guard NativeHarnessPicker.harnesses.indices.contains(sender.selectedSegment) else { return }
            selection.wrappedValue = NativeHarnessPicker.harnesses[sender.selectedSegment].id
        }
    }
}
