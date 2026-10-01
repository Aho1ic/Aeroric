import SwiftUI

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
        .accessibilityIdentifier("native.segmented")
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
            .accessibilityIdentifier("native.selectionList")
    }
}
