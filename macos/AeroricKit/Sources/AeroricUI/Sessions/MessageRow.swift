import SwiftUI
import MarkdownUI

/// Redesign 01 message row. User turns sit in a right-aligned bubble (Orca
/// `NativeChatMessageList`); assistant turns are unboxed. Tool calls are flat
/// `▸ 工具名 · 摘要` rows that expand in place (Q16-a), never cards.
public struct MessageRow: View {
    let message: SessionMessage

    public init(message: SessionMessage) {
        self.message = message
    }

    private var isUser: Bool { message.role == "user" }

    public var body: some View {
        HStack(alignment: .top, spacing: 0) {
            if isUser { Spacer(minLength: 80) }
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(message.content.enumerated()), id: \.offset) { _, block in
                    ContentBlock(block: block)
                }
            }
            .padding(.horizontal, isUser ? 14 : 0)
            .padding(.vertical, isUser ? 10 : 2)
            .background {
                if isUser {
                    RoundedRectangle(cornerRadius: ShellRadius.composer, style: .continuous)
                        .fill(SessionPalette.bubble)
                }
            }
            if !isUser { Spacer(minLength: 0) }
        }
        .frame(maxWidth: .infinity, alignment: isUser ? .trailing : .leading)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(isUser ? "用户" : "Assistant")
        .accessibilityIdentifier("session.message.\(message.messageId ?? message.role)")
    }
}

/// One content block. Markdown is rendered by MarkdownUI with images and
/// raw HTML disabled: no remote fetches, no script execution. Link taps go
/// through the scheme allow-list below.
public struct ContentBlock: View {
    let block: SessionContent
    @State private var expanded = false

    public init(block: SessionContent) {
        self.block = block
    }

    public var body: some View {
        switch block {
        case let .text(text):
            Markdown(text)
                .markdownTheme(.aeroric)
                .markdownImageProvider(NoImageProvider())
                .markdownInlineImageProvider(NoInlineImageProvider())
                .textSelection(.enabled)
                .environment(\.openURL, OpenURLAction { url in
                    Self.isAllowedLinkScheme(url) ? .systemAction : .discarded
                })
                .accessibilityIdentifier("session.content.text")
        case let .thinking(thinking):
            FlatDisclosureRow(symbol: "brain", title: "思考", summary: Self.summary(thinking),
                              expanded: $expanded, identifier: "session.content.thinking-toggle") {
                Text(thinking)
                    .font(AeroricFont.ui(AeroricFont.meta))
                    .foregroundStyle(SessionPalette.secondary)
                    .textSelection(.enabled)
            }
        case let .toolUse(id, name, input):
            FlatDisclosureRow(symbol: "chevron.right", title: name, summary: Self.summary(input),
                              expanded: $expanded, identifier: "session.tool.\(id)") {
                CodeText(input)
            }
        case let .toolResult(id, output):
            ToolCallRow(id: id, output: output)
        case let .attachment(name, mediaType, _):
            Label("\(name)（\(mediaType)）", systemImage: "paperclip")
                .font(AeroricFont.ui(AeroricFont.meta))
                .foregroundStyle(SessionPalette.secondary)
                .accessibilityIdentifier("session.content.attachment")
        }
    }

    /// First non-empty line, trimmed to one row.
    static func summary(_ text: String) -> String {
        let line = text.split(whereSeparator: \.isNewline).first.map(String.init) ?? ""
        return String(line.trimmingCharacters(in: .whitespaces).prefix(120))
    }

    /// Only http/https/mailto open; javascript:/file:/custom schemes never do.
    public static func isAllowedLinkScheme(_ url: URL) -> Bool {
        switch url.scheme?.lowercased() {
        case "http", "https", "mailto":
            return true
        default:
            return false
        }
    }
}

/// Tool result row. Diff-shaped output is coloured by line; everything is
/// shown in full when expanded (the host already deduplicates tool blocks).
public struct ToolCallRow: View {
    let id: String
    let output: String
    @State private var expanded = false

    public init(id: String, output: String) {
        self.id = id
        self.output = output
    }

    public var body: some View {
        FlatDisclosureRow(symbol: "arrow.turn.down.right", title: "结果",
                          summary: ContentBlock.summary(output), expanded: $expanded,
                          identifier: "session.tool-result.\(id)") {
            if DiffText.looksLikeDiff(output) { DiffText(output) } else { CodeText(output) }
        }
    }
}

/// `▸ title · summary` — the shared flat row (Orca `NativeChatToolRun`).
struct FlatDisclosureRow<Detail: View>: View {
    let symbol: String
    let title: String
    let summary: String
    @Binding var expanded: Bool
    let identifier: String
    @ViewBuilder let detail: () -> Detail
    @State private var hovered = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Button { expanded.toggle() } label: {
                HStack(spacing: 6) {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 9, weight: .semibold))
                        .rotationEffect(.degrees(expanded ? 90 : 0))
                        .frame(width: 10)
                    if symbol != "chevron.right" {
                        Image(systemName: symbol).font(.system(size: 11))
                    }
                    Text(title).font(AeroricFont.ui(AeroricFont.meta, weight: .medium))
                        .foregroundStyle(SessionPalette.text)
                    if !summary.isEmpty {
                        Text(summary).font(AeroricFont.mono(AeroricFont.meta))
                            .lineLimit(1).truncationMode(.tail)
                    }
                    Spacer(minLength: 0)
                }
                .foregroundStyle(hovered ? SessionPalette.text : SessionPalette.secondary)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .onHover { hovered = $0 }
            .accessibilityLabel("\(title)，\(expanded ? "已展开" : "已收起")")
            .accessibilityIdentifier(identifier)
            if expanded {
                detail().padding(.leading, 16)
            }
        }
    }
}

/// Monospaced, selectable block on a subtle card.
struct CodeText: View {
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        ScrollView(.horizontal) {
            Text(text)
                .font(AeroricFont.mono(AeroricFont.code))
                .foregroundStyle(SessionPalette.secondary)
                .textSelection(.enabled)
                .fixedSize(horizontal: true, vertical: true)
                .padding(10)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(SessionPalette.card, in: RoundedRectangle(cornerRadius: ShellRadius.chip, style: .continuous))
    }
}

/// Unified diff with additions/deletions coloured; no syntax highlighting.
struct DiffText: View {
    let lines: [Substring]
    init(_ text: String) { lines = text.split(separator: "\n", omittingEmptySubsequences: false) }

    static func looksLikeDiff(_ text: String) -> Bool {
        text.hasPrefix("diff --git") || text.hasPrefix("--- ") || text.contains("\n@@ ")
    }

    var body: some View {
        ScrollView(.horizontal) {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                    Text(line.isEmpty ? " " : String(line))
                        .font(AeroricFont.mono(AeroricFont.code))
                        .foregroundStyle(color(line))
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(background(line))
                }
            }
            .textSelection(.enabled)
            .fixedSize(horizontal: true, vertical: true)
            .padding(.vertical, 8)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(SessionPalette.card, in: RoundedRectangle(cornerRadius: ShellRadius.chip, style: .continuous))
    }

    private func color(_ line: Substring) -> Color {
        if line.hasPrefix("+"), !line.hasPrefix("+++") { return SessionPalette.teal }
        if line.hasPrefix("-"), !line.hasPrefix("---") { return Color.dsDanger }
        if line.hasPrefix("@@") { return SessionPalette.accent }
        return SessionPalette.secondary
    }

    private func background(_ line: Substring) -> Color {
        if line.hasPrefix("+"), !line.hasPrefix("+++") { return SessionPalette.tintTeal }
        if line.hasPrefix("-"), !line.hasPrefix("---") { return Color.dsDanger.opacity(0.08) }
        return .clear
    }
}

// MARK: - MarkdownUI configuration

/// Images never load: no remote fetches from agent output.
struct NoImageProvider: ImageProvider {
    func makeImage(url: URL?) -> some View {
        Label(url?.lastPathComponent ?? "图片", systemImage: "photo")
            .font(AeroricFont.ui(AeroricFont.meta))
            .foregroundStyle(SessionPalette.muted)
    }
}

struct NoInlineImageProvider: InlineImageProvider {
    struct Refused: Error {}
    func image(with url: URL, label: String) async throws -> Image { throw Refused() }
}

extension Theme {
    /// System text at 13pt, JetBrains Mono for code, DSH colour roles.
    @MainActor static let aeroric = Theme()
        .text {
            ForegroundColor(SessionPalette.text)
            FontSize(13)
        }
        .code {
            FontFamily(.custom("JetBrainsMono-Regular"))
            FontSize(.em(0.92))
            BackgroundColor(SessionPalette.chip)
        }
        .link { ForegroundColor(SessionPalette.accent) }
        .heading1 { configuration in
            configuration.label
                .markdownTextStyle { FontWeight(.semibold); FontSize(.em(1.3)) }
                .markdownMargin(top: 12, bottom: 6)
        }
        .heading2 { configuration in
            configuration.label
                .markdownTextStyle { FontWeight(.semibold); FontSize(.em(1.15)) }
                .markdownMargin(top: 10, bottom: 4)
        }
        .heading3 { configuration in
            configuration.label
                .markdownTextStyle { FontWeight(.semibold) }
                .markdownMargin(top: 8, bottom: 4)
        }
        .paragraph { configuration in
            configuration.label
                .relativeLineSpacing(.em(0.3))
                .markdownMargin(top: 0, bottom: 8)
        }
        .codeBlock { configuration in
            ScrollView(.horizontal) {
                configuration.label
                    .markdownTextStyle {
                        FontFamily(.custom("JetBrainsMono-Regular"))
                        FontSize(12)
                        ForegroundColor(SessionPalette.secondary)
                    }
                    .padding(10)
            }
            .background(SessionPalette.card, in: RoundedRectangle(cornerRadius: ShellRadius.chip, style: .continuous))
            .markdownMargin(top: 0, bottom: 8)
        }
        .blockquote { configuration in
            configuration.label
                .markdownTextStyle { ForegroundColor(SessionPalette.secondary) }
                .padding(.leading, 10)
                .overlay(alignment: .leading) {
                    Rectangle().fill(SessionPalette.edge).frame(width: 2)
                }
        }
}
