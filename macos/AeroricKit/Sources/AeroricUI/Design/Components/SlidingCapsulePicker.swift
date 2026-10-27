import SwiftUI

/// A capsule picker whose selection is a pill that slides from the old
/// choice to the new one (2026-10-27, user request; codeg `agent-selector`'s
/// "sliding droplet"). Optionally only the selected choice shows its title
/// and the others collapse to their icon, the title opening as the pill
/// arrives.
///
/// ADR-001 asks adjacent exclusive selectors to use the system segmented
/// control; the user explicitly asked for this motion, which
/// `NSSegmentedControl` cannot draw. Semantics stay those of a segmented
/// control: one accessibility group, each choice a button with the selected
/// trait, ←/→ move the selection while it is focused, Reduce Motion skips the
/// slide.
public struct SlidingCapsulePicker<Option: Hashable>: View {
    public enum TitleMode: Sendable {
        case always
        /// Icons only, except the selected choice (the harness slider).
        case selectedOnly
    }

    public enum Size: Sendable {
        case regular
        /// The new-session scenario tabs.
        case large
    }

    public struct Segment: Identifiable {
        public let value: Option
        public let title: String
        public let systemImage: String?
        /// An asset-catalog image; wins over `systemImage`.
        public let asset: String?
        public let isEnabled: Bool
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

    @Binding private var selection: Option
    private let segments: [Segment]
    private let groupTitle: String
    private let titleMode: TitleMode
    private let size: Size
    private let identifier: String?
    @Namespace private var thumb
    @FocusState private var focused: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.isEnabled) private var isEnabled

    /// Slide timing: a soft spring (≈ codeg's 300 ms ease-in-out).
    static var slide: Animation { .spring(response: 0.34, dampingFraction: 0.82) }

    public init(selection: Binding<Option>, groupTitle: String, titleMode: TitleMode = .always,
                size: Size = .regular, identifier: String? = nil, segments: [Segment]) {
        _selection = selection
        self.segments = segments
        self.groupTitle = groupTitle
        self.titleMode = titleMode
        self.size = size
        self.identifier = identifier
    }

    // Regular is the approved harness slider (2026-10-27); large is a touch bigger.
    private var height: CGFloat { size == .large ? 32 : 28 }
    private var iconSide: CGFloat { size == .large ? 17 : 16 }
    private var fontSize: CGFloat { size == .large ? AeroricType.body + 1 : AeroricType.body }

    /// Pure: whether `segment` shows its title for `selection`. A segment
    /// without an image always keeps its title so it is never blank.
    static func showsTitle(_ segment: Segment, selection: Option, mode: TitleMode) -> Bool {
        let hasImage = segment.asset != nil || segment.systemImage != nil
        return mode == .always || !hasImage || segment.value == selection
    }

    /// Pure: the enabled neighbour `step` places away (←/→), or nil.
    static func neighbour(of selection: Option, step: Int, in segments: [Segment]) -> Option? {
        guard let start = segments.firstIndex(where: { $0.value == selection }) else { return nil }
        var index = start + step
        while segments.indices.contains(index) {
            if segments[index].isEnabled { return segments[index].value }
            index += step
        }
        return nil
    }

    public var body: some View {
        HStack(spacing: 2) {
            ForEach(segments) { segment in
                button(segment)
            }
        }
        .padding(3)
        .background(AeroricColor.layer2.opacity(0.7), in: Capsule(style: .continuous))
        .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator1, lineWidth: 1) }
        .focusable()
        .focused($focused)
        .focusEffectDisabled()
        .onKeyPress(.leftArrow) { move(-1) }
        .onKeyPress(.rightArrow) { move(1) }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(groupTitle)
        .accessibilityIdentifier(identifier ?? "")
        .opacity(isEnabled ? 1 : 0.5)
    }

    private func move(_ step: Int) -> KeyPress.Result {
        guard let next = Self.neighbour(of: selection, step: step, in: segments) else { return .ignored }
        choose(next)
        return .handled
    }

    private func choose(_ value: Option) {
        guard value != selection else { return }
        if reduceMotion {
            selection = value
        } else {
            withAnimation(Self.slide) { selection = value }
        }
    }

    private func button(_ segment: Segment) -> some View {
        let selected = segment.value == selection
        let titled = Self.showsTitle(segment, selection: selection, mode: titleMode)
        return Button { choose(segment.value) } label: {
            HStack(spacing: AeroricSpace.s) {
                if let asset = segment.asset {
                    Image(asset).resizable().scaledToFit().frame(width: iconSide, height: iconSide)
                } else if let symbol = segment.systemImage {
                    Image(systemName: symbol).font(.system(size: iconSide - 3, weight: .medium))
                }
                if titled {
                    Text(segment.title)
                        .font(.system(size: fontSize, weight: selected ? .semibold : .medium))
                        .lineLimit(1)
                        .fixedSize()
                        .transition(.opacity.combined(with: .scale(scale: 0.85, anchor: .leading)))
                }
            }
            .foregroundStyle(selected ? AeroricColor.text : AeroricColor.textSecondary)
            .padding(.horizontal, titled ? AeroricSpace.l : AeroricSpace.m + 1)
            .frame(height: height)
            .background {
                if selected {
                    Capsule(style: .continuous)
                        .fill(AeroricColor.canvas)
                        .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator2, lineWidth: 1) }
                        .shadow(color: AeroricColor.shadow, radius: 3, y: 1)
                        .matchedGeometryEffect(id: "thumb", in: thumb)
                }
            }
            .contentShape(Capsule(style: .continuous))
        }
        .buttonStyle(.plain)
        .disabled(!segment.isEnabled)
        .opacity(segment.isEnabled ? 1 : 0.45)
        .help(segment.help ?? segment.title)
        .accessibilityLabel(segment.title)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}
