import SwiftUI

/// Send button state. `.running` turns it into Stop (ends the task: D3).
public enum ComposerSendState: Equatable, Sendable {
    case ready
    case blocked(String)
    case running
}

/// Shared input card for the new-session home and the session page:
/// auto-growing editor (up to 12 lines), attachment references, a footer of
/// caller-supplied capsules and the send / stop button. ⌘↩ sends.
public struct ComposerCard<Footer: View>: View {
    @Binding var text: String
    @Binding var attachments: [String]
    let placeholder: String
    let minLines: Int
    let isEditable: Bool
    let acceptsAttachments: Bool
    let sendState: ComposerSendState
    let identifierPrefix: String
    let focusRequest: Int
    let onSend: () -> Void
    let onStop: () -> Void
    let footer: () -> Footer
    @FocusState private var focused: Bool
    @Environment(\.colorSchemeContrast) private var contrast

    public static var maxLines: Int { 12 }

    public init(text: Binding<String>, attachments: Binding<[String]> = .constant([]), placeholder: String,
                minLines: Int = 1, isEditable: Bool = true, acceptsAttachments: Bool = false,
                sendState: ComposerSendState, identifierPrefix: String, focusRequest: Int = 0,
                onSend: @escaping () -> Void, onStop: @escaping () -> Void = {},
                @ViewBuilder footer: @escaping () -> Footer) {
        _text = text
        _attachments = attachments
        self.placeholder = placeholder
        self.minLines = minLines
        self.isEditable = isEditable
        self.acceptsAttachments = acceptsAttachments
        self.sendState = sendState
        self.identifierPrefix = identifierPrefix
        self.focusRequest = focusRequest
        self.onSend = onSend
        self.onStop = onStop
        self.footer = footer
    }

    private var font: Font { .system(size: 14) }

    public var body: some View {
        let shape = AeroricRadius.shape(AeroricRadius.xl)
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            editor
            if !attachments.isEmpty { attachmentChips }
            HStack(alignment: .center, spacing: AeroricSpace.xs) {
                footer()
                Spacer(minLength: AeroricSpace.s)
                sendButton
            }
        }
        .padding(.horizontal, AeroricSpace.l)
        .padding(.top, AeroricSpace.l)
        // 2026-10-27: the Agent / 模型 / 权限 row sits close to the bottom edge.
        .padding(.bottom, AeroricSpace.xs)
        .glassSurface(.floatingCard, in: shape)
        .overlay {
            shape.strokeBorder(focused ? AeroricColor.accent.opacity(0.55) : .clear,
                               lineWidth: contrast == .increased ? 1.5 : 1)
                .allowsHitTesting(false)
        }
        .aeroricElevation(.floating, in: shape)
        .dropDestination(for: URL.self) { urls, _ in
            guard acceptsAttachments, isEditable else { return false }
            return SessionAttachmentReferences.append(urls, to: &attachments)
        }
        .onChange(of: focusRequest) { _, _ in focused = true }
    }

    /// The hidden Text sizes the card (wrapping, 12-line cap); the editor
    /// overlays it and scrolls once the text is longer than that.
    private var editor: some View {
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false).count
        let padding = String(repeating: "\n", count: max(0, minLines - lines))
        return Text(text.isEmpty ? " " + padding : text + padding + " ")
            .font(font)
            .lineLimit(Self.maxLines)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 5)
            .opacity(0)
            .accessibilityHidden(true)
            .overlay(alignment: .topLeading) {
                ZStack(alignment: .topLeading) {
                    if text.isEmpty {
                        Text(placeholder)
                            .font(font)
                            .foregroundStyle(AeroricColor.textTertiary)
                            .padding(.horizontal, 5)
                            .allowsHitTesting(false)
                            .accessibilityHidden(true)
                    }
                    TextEditor(text: $text)
                        .font(font)
                        .scrollContentBackground(.hidden)
                        .scrollIndicators(.never)
                        .focused($focused)
                        .disabled(!isEditable)
                        .accessibilityLabel(placeholder)
                        .accessibilityIdentifier("\(identifierPrefix).prompt")
                }
            }
    }

    private var attachmentChips: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: AeroricSpace.s) {
                ForEach(attachments, id: \.self) { path in
                    HStack(spacing: AeroricSpace.xs) {
                        Image(systemName: "doc")
                        Text(URL(fileURLWithPath: path).lastPathComponent).lineLimit(1)
                        Button {
                            attachments.removeAll { $0 == path }
                        } label: {
                            Image(systemName: "xmark").font(.system(size: 9, weight: .semibold))
                        }
                        .buttonStyle(.plain)
                        .disabled(!isEditable)
                        .accessibilityLabel("移除资料引用")
                        .accessibilityIdentifier("\(identifierPrefix).remove-reference.\(path)")
                    }
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .padding(.horizontal, AeroricSpace.m)
                    .frame(height: 24)
                    .background(AeroricColor.layer2, in: Capsule(style: .continuous))
                }
            }
        }
    }

    @ViewBuilder private var sendButton: some View {
        switch sendState {
        case .running:
            Button(action: onStop) {
                Image(systemName: "stop.fill")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(AeroricColor.onPrimary)
                    .frame(width: 30, height: 30)
                    .background(AeroricColor.primary, in: Circle())
            }
            .buttonStyle(.plain)
            .help("停止任务（结束当前运行）")
            .accessibilityLabel("停止任务")
            .accessibilityIdentifier("\(identifierPrefix).stop")
        case .ready, .blocked:
            let ready = sendState == .ready && isEditable
                && !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            Button(action: onSend) {
                PaperPlane()
                    .stroke(ready ? AeroricColor.onPrimary : AeroricColor.textTertiary,
                            style: StrokeStyle(lineWidth: 1.6, lineCap: .round, lineJoin: .round))
                    .frame(width: 14, height: 14)
                    .offset(x: -0.5, y: 0.5)
                    .frame(width: 30, height: 30)
                    .background(ready ? AeroricColor.primary : AeroricColor.layer3, in: Circle())
            }
            .buttonStyle(.plain)
            .keyboardShortcut(.return, modifiers: .command)
            .disabled(!ready)
            .help(blockedReason ?? "发送 ⌘↩")
            .accessibilityLabel("发送")
            .accessibilityIdentifier("\(identifierPrefix).submit")
        }
    }

    private var blockedReason: String? {
        if case let .blocked(reason) = sendState { return reason }
        return nil
    }
}

/// The send glyph (2026-10-28, user's reference): an outlined paper plane
/// pointing up-right with a notch at its tail, drawn on a 24-unit grid
/// (3,11 → 22,2 → 13,21 → 11,13 → back).
struct PaperPlane: Shape {
    func path(in rect: CGRect) -> Path {
        let unit = min(rect.width, rect.height) / 24
        let point = { (x: CGFloat, y: CGFloat) in CGPoint(x: rect.minX + x * unit, y: rect.minY + y * unit) }
        var path = Path()
        path.move(to: point(3, 11))
        path.addLine(to: point(22, 2))
        path.addLine(to: point(13, 21))
        path.addLine(to: point(11, 13))
        path.closeSubpath()
        return path
    }
}

