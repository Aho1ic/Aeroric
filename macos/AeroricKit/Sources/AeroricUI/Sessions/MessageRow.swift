import SwiftUI
import MarkdownUI

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
            // 2026-10-28: one native text view per message, so a selection
            // can span paragraphs and code blocks (SelectableMarkdown).
            SelectableMarkdown(text: text, size: AeroricType.conversation)
        case let .thinking(thinking):
            FlatDisclosureRow(symbol: "brain", title: "思考", summary: Self.summary(thinking),
                              expanded: $expanded, identifier: "session.content.thinking-toggle") {
                Text(thinking)
                    .font(Font.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .textSelection(.enabled)
            }
        case let .toolUse(id, name, input):
            FlatDisclosureRow(symbol: "chevron.right", title: name, summary: Self.summary(input),
                              expanded: $expanded, identifier: "session.tool.\(id)") {
                CodeText(input)
            }
        case let .toolResult(id, output):
            // The transcript pairs results with their calls; this is the fallback.
            FlatDisclosureRow(symbol: "arrow.turn.down.right", title: "结果", summary: Self.summary(output),
                              expanded: $expanded, identifier: "session.tool-result.\(id)") {
                if DiffText.looksLikeDiff(output) { DiffText(output) } else { CodeText(output) }
            }
        case let .attachment(name, mediaType, _):
            Label("\(name)（\(mediaType)）", systemImage: "paperclip")
                .font(Font.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
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
                    Text(title).font(Font.system(size: AeroricType.caption, weight: .medium))
                        .foregroundStyle(AeroricColor.text)
                    if !summary.isEmpty {
                        Text(summary).font(AeroricType.mono(AeroricType.caption))
                            .lineLimit(1).truncationMode(.tail)
                    }
                    Spacer(minLength: 0)
                }
                .foregroundStyle(hovered ? AeroricColor.text : AeroricColor.textSecondary)
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
                .font(AeroricType.mono(AeroricType.code))
                .foregroundStyle(AeroricColor.textSecondary)
                .textSelection(.enabled)
                .fixedSize(horizontal: true, vertical: true)
                .padding(10)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(AeroricColor.layer1, in: RoundedRectangle(cornerRadius: AeroricRadius.m, style: .continuous))
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
                        .font(AeroricType.mono(AeroricType.code))
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
        .background(AeroricColor.layer1, in: RoundedRectangle(cornerRadius: AeroricRadius.m, style: .continuous))
    }

    private func color(_ line: Substring) -> Color {
        if line.hasPrefix("+"), !line.hasPrefix("+++") { return AeroricColor.teal }
        if line.hasPrefix("-"), !line.hasPrefix("---") { return AeroricColor.danger }
        if line.hasPrefix("@@") { return AeroricColor.accent }
        return AeroricColor.textSecondary
    }

    private func background(_ line: Substring) -> Color {
        if line.hasPrefix("+"), !line.hasPrefix("+++") { return AeroricColor.teal.opacity(0.12) }
        if line.hasPrefix("-"), !line.hasPrefix("---") { return AeroricColor.danger.opacity(0.08) }
        return .clear
    }
}

// MARK: - MarkdownUI configuration

/// Images never load: no remote fetches from agent output.
struct NoImageProvider: ImageProvider {
    func makeImage(url: URL?) -> some View {
        Label(url?.lastPathComponent ?? "图片", systemImage: "photo")
            .font(Font.system(size: AeroricType.caption))
            .foregroundStyle(AeroricColor.textTertiary)
    }
}

struct NoInlineImageProvider: InlineImageProvider {
    struct Refused: Error {}
    func image(with url: URL, label: String) async throws -> Image { throw Refused() }
}

extension Theme {
    /// System text at the session's own size (设置 → 外观 → 会话), JetBrains
    /// Mono for code, DSH colour roles.
    @MainActor static func aeroric(_ size: CGFloat) -> Theme {
        Theme()
        .text {
            ForegroundColor(AeroricColor.text)
            FontSize(size)
            // 2026-10-28: body text is regular — Chinese renders as PingFang SC
            // Regular (the system face's own CJK fallback), never bold.
            FontWeight(.regular)
        }
        // **重点** one step above the body instead of semibold.
        .strong { FontWeight(.medium) }
        .code {
            FontFamily(.custom("JetBrainsMono-Regular"))
            FontSize(.em(0.92))
            BackgroundColor(AeroricColor.layer2)
        }
        .link { ForegroundColor(AeroricColor.accent) }
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
                        FontSize(size - 1)
                        ForegroundColor(AeroricColor.textSecondary)
                    }
                    .padding(10)
            }
            .background(AeroricColor.layer1, in: RoundedRectangle(cornerRadius: AeroricRadius.m, style: .continuous))
            .markdownMargin(top: 0, bottom: 8)
        }
        .blockquote { configuration in
            configuration.label
                .markdownTextStyle { ForegroundColor(AeroricColor.textSecondary) }
                .padding(.leading, 10)
                .overlay(alignment: .leading) {
                    Rectangle().fill(AeroricColor.separator2).frame(width: 2)
                }
        }
    }
}
