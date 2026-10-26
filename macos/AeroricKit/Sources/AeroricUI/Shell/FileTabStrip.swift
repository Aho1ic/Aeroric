import SwiftUI

/// The centre's document tabs (2026-10-26 C): a fixed "任务" tab, then one
/// rounded tab per opened file, left to right. Shown only while at least one
/// file tab is open. These are closable documents like Xcode's or Safari's
/// tabs, not an adjacent mutually-exclusive choice, so ADR-001's segmented
/// rule does not apply.
public struct FileTabStrip: View {
    let tabs: [ResourcePreviewTarget]
    /// The file tab in front; nil when the task is.
    let active: ResourcePreviewTarget?
    let isDirty: (ResourcePreviewTarget) -> Bool
    let onShowTask: () -> Void
    let onSelect: (ResourcePreviewTarget) -> Void
    let onClose: (ResourcePreviewTarget) -> Void

    public init(tabs: [ResourcePreviewTarget], active: ResourcePreviewTarget?,
                isDirty: @escaping (ResourcePreviewTarget) -> Bool,
                onShowTask: @escaping () -> Void, onSelect: @escaping (ResourcePreviewTarget) -> Void,
                onClose: @escaping (ResourcePreviewTarget) -> Void) {
        self.tabs = tabs; self.active = active; self.isDirty = isDirty
        self.onShowTask = onShowTask; self.onSelect = onSelect; self.onClose = onClose
    }

    public var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: AeroricSpace.xs) {
                    FileTab(title: "任务", systemImage: "bubble.left.and.text.bubble.right", selected: active == nil,
                            dirty: false, closable: false, onSelect: onShowTask, onClose: {})
                        .accessibilityIdentifier("file-tab.task")
                    ForEach(tabs, id: \.self) { target in
                        FileTab(title: (target.path as NSString).lastPathComponent,
                                systemImage: target.kind == .file ? "doc.text" : "plusminus",
                                selected: active == target, dirty: isDirty(target), closable: true,
                                onSelect: { onSelect(target) }, onClose: { onClose(target) })
                            .help(target.path)
                            .id(target)
                            .accessibilityIdentifier("file-tab.\(target.path)")
                    }
                }
                .padding(.horizontal, AeroricSpace.m)
                .padding(.vertical, AeroricSpace.xs)
            }
            .onChange(of: active) { _, target in
                guard let target else { return }
                withAnimation { proxy.scrollTo(target) }
            }
        }
        .frame(height: 38)
        .background(AeroricColor.canvas)
        .overlay(alignment: .bottom) { HairlineDivider() }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("打开的文件")
        .accessibilityIdentifier("file-tabs")
    }
}

/// One rounded tab: icon, name, and a close mark that shows on hover or
/// while selected; an unsaved tab shows a dot in its place.
private struct FileTab: View {
    let title: String
    let systemImage: String
    let selected: Bool
    let dirty: Bool
    let closable: Bool
    let onSelect: () -> Void
    let onClose: () -> Void
    @State private var hovering = false

    var body: some View {
        HStack(spacing: AeroricSpace.xs) {
            Image(systemName: systemImage)
                .font(.system(size: 11))
                .foregroundStyle(selected ? AeroricColor.accent : AeroricColor.textTertiary)
            Text(title)
                .font(.system(size: AeroricType.secondary, weight: selected ? .medium : .regular))
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: 180)
            if closable {
                ZStack {
                    if dirty && !hovering {
                        Circle().fill(AeroricColor.textSecondary).frame(width: 7, height: 7)
                            .accessibilityLabel("未保存")
                    } else if hovering || selected {
                        Button(action: onClose) {
                            Image(systemName: "xmark").font(.system(size: 9, weight: .semibold))
                        }
                        .buttonStyle(.borderless)
                        .help("关闭（⌘W）")
                        .accessibilityLabel("关闭 \(title)")
                    }
                }
                .frame(width: 14, height: 14)
            }
        }
        .foregroundStyle(selected ? AeroricColor.text : AeroricColor.textSecondary)
        .padding(.leading, AeroricSpace.m)
        .padding(.trailing, closable ? AeroricSpace.s : AeroricSpace.m)
        .frame(height: 28)
        .background {
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .fill(selected ? AeroricColor.layer1 : (hovering ? AeroricColor.layer1.opacity(0.5) : .clear))
        }
        .overlay {
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .strokeBorder(selected ? AeroricColor.separator1 : .clear, lineWidth: 1)
        }
        .contentShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .onTapGesture(perform: onSelect)
        .onHover { hovering = $0 }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
        .accessibilityAction(named: "关闭") { if closable { onClose() } }
    }
}
