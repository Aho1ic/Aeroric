import SwiftUI

/// Composer footer capsule that opens a popover list (icon + name + grey
/// description + check). Unavailable rows show their reason and can't be
/// picked; an empty list shows the menu's note instead of a fake default.
public struct ComposerPicker: View {
    let title: String
    let systemImage: String?
    let menu: PickerMenu
    let identifier: String
    let onSelect: (String) -> Void
    @State private var presented = false
    @State private var hovered = false
    @Environment(\.isEnabled) private var isEnabled

    public init(title: String, systemImage: String? = nil, menu: PickerMenu, identifier: String,
                onSelect: @escaping (String) -> Void) {
        self.title = title
        self.systemImage = systemImage
        self.menu = menu
        self.identifier = identifier
        self.onSelect = onSelect
    }

    public var body: some View {
        Button { presented.toggle() } label: {
            HStack(spacing: AeroricSpace.s) {
                if let systemImage { Image(systemName: systemImage).imageScale(.small) }
                Text(title).lineLimit(1).truncationMode(.middle).frame(maxWidth: 160, alignment: .leading)
                Image(systemName: "chevron.down")
                    .font(.system(size: 8, weight: .semibold))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            .font(.system(size: AeroricType.secondary))
            .foregroundStyle(isEnabled ? AeroricColor.textSecondary : AeroricColor.textTertiary)
            .padding(.horizontal, AeroricSpace.m)
            .frame(height: 26)
            .background(Capsule(style: .continuous).fill(hovered || presented ? AeroricColor.hover : .clear))
            .contentShape(Capsule(style: .continuous))
            .fixedSize()
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 && isEnabled }
        .popover(isPresented: $presented, arrowEdge: .bottom) {
            PickerMenuList(menu: menu, identifier: identifier) { id in
                onSelect(id)
                presented = false
            }
        }
        .accessibilityLabel(title)
        .accessibilityIdentifier(identifier)
    }
}

/// Popover body. ↑/↓ move between enabled rows, ↩ picks, Esc closes (system).
struct PickerMenuList: View {
    let menu: PickerMenu
    let identifier: String
    let onSelect: (String) -> Void
    @State private var highlighted: String?
    @FocusState private var focused: Bool

    private var enabledIDs: [String] {
        menu.sections.flatMap(\.options).filter(\.isEnabled).map(\.id)
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(menu.sections) { section in
                    if let title = section.title, !section.options.isEmpty {
                        Text(title)
                            .font(AeroricType.sectionLabel())
                            .foregroundStyle(AeroricColor.textTertiary)
                            .padding(.horizontal, AeroricSpace.m)
                            .padding(.top, AeroricSpace.s)
                            .padding(.bottom, 2)
                    }
                    ForEach(section.options) { option in row(option) }
                }
                if let note = menu.note {
                    Label(note, systemImage: "info.circle")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .padding(AeroricSpace.l)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .padding(AeroricSpace.xs)
        }
        .frame(width: 280)
        .frame(maxHeight: 360)
        .fixedSize(horizontal: false, vertical: true)
        .focusable()
        .focusEffectDisabled()
        .focused($focused)
        .onAppear {
            highlighted = menu.sections.flatMap(\.options).first(where: \.selected)?.id ?? enabledIDs.first
            focused = true
        }
        .onKeyPress(.downArrow) { move(1) }
        .onKeyPress(.upArrow) { move(-1) }
        .onKeyPress(.return) {
            guard let highlighted else { return .ignored }
            onSelect(highlighted)
            return .handled
        }
        .accessibilityIdentifier("\(identifier).menu")
    }

    private func move(_ step: Int) -> KeyPress.Result {
        let ids = enabledIDs
        guard !ids.isEmpty else { return .ignored }
        let index = highlighted.flatMap { ids.firstIndex(of: $0) } ?? (step > 0 ? -1 : ids.count)
        highlighted = ids[(index + step + ids.count) % ids.count]
        return .handled
    }

    private func row(_ option: PickerOption) -> some View {
        // Flatter rows (2026-10-26 D): 24pt, or 36pt with a description line.
        HoverCapsuleRow(selected: highlighted == option.id && option.isEnabled,
                        height: option.detail == nil && option.disabledReason == nil ? 24 : 36,
                        horizontalPadding: AeroricSpace.m) {
            onSelect(option.id)
        } label: {
            HStack(spacing: AeroricSpace.s) {
                if let symbol = option.systemImage {
                    Image(systemName: symbol)
                        .font(.system(size: 12))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .frame(width: 16)
                }
                VStack(alignment: .leading, spacing: 0) {
                    Text(option.title)
                        .font(.system(size: AeroricType.secondary, weight: .medium))
                        .foregroundStyle(AeroricColor.text)
                        .lineLimit(1)
                    if let detail = option.disabledReason ?? option.detail {
                        Text(detail)
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(option.isEnabled ? AeroricColor.textTertiary : AeroricColor.warning)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                }
                Spacer(minLength: AeroricSpace.s)
                if option.selected {
                    Image(systemName: "checkmark")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(AeroricColor.accent)
                }
            }
            .opacity(option.isEnabled ? 1 : 0.6)
        }
        .disabled(!option.isEnabled)
        .onHover { if $0 && option.isEnabled { highlighted = option.id } }
        .accessibilityValue(option.selected ? "当前" : "")
        .accessibilityHint(option.disabledReason ?? "")
        .accessibilityIdentifier("\(identifier).option.\(option.id)")
    }
}
