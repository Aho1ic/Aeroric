import AppKit
import SwiftUI

/// A message's Markdown as ONE native text view (2026-10-28): selection runs
/// across paragraphs, lists and code blocks, and ⌘A / ⌘C / drag-select work
/// like any Mac text. MarkdownUI drew each block as its own view, so a
/// selection stopped at the end of a paragraph.
///
/// Same rules as before: images and raw HTML are never loaded or run (images
/// become their alt text), and only http / https / mailto links open.
struct SelectableMarkdown: NSViewRepresentable {
    let text: String
    let size: CGFloat
    @Environment(\.colorScheme) private var colorScheme

    func makeNSView(context: Context) -> NSTextView {
        // TextKit 1: code-block backgrounds are `NSTextBlock`s.
        let view = NSTextView(usingTextLayoutManager: false)
        view.isEditable = false
        view.isSelectable = true
        view.drawsBackground = false
        view.textContainerInset = .zero
        view.textContainer?.lineFragmentPadding = 0
        view.textContainer?.widthTracksTextView = true
        view.isVerticallyResizable = false
        view.isHorizontallyResizable = false
        view.linkTextAttributes = [.foregroundColor: NSColor(AeroricColor.accent), .cursor: NSCursor.pointingHand]
        view.delegate = context.coordinator
        view.setAccessibilityIdentifier("session.content.text")
        return view
    }

    func updateNSView(_ view: NSTextView, context: Context) {
        let key = "\(size)|\(colorScheme)|\(text)"
        guard context.coordinator.renderedKey != key else { return }
        context.coordinator.renderedKey = key
        view.textStorage?.setAttributedString(MarkdownAttributed.build(text, size: size))
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView view: NSTextView, context: Context) -> CGSize? {
        // Hug the text (2026-10-29): a short user message keeps a short
        // bubble; only text longer than the offered width wraps to it.
        let offered = proposal.width.flatMap { $0.isFinite && $0 > 0 ? $0 : nil } ?? 10_000
        guard let storage = view.textStorage else { return nil }
        let coordinator = context.coordinator
        let key = "\(size)|\(offered)|\(text)"
        if let cached = coordinator.measured, cached.key == key { return cached.size }
        let used = Self.measure(storage, width: offered)
        let size = CGSize(width: min(offered, max(ceil(used.width), 1)), height: max(ceil(used.height), 1))
        coordinator.measured = (key, size)
        return size
    }

    /// A fresh layout per measurement: the view's own container keeps the
    /// width it was last given, and its `usedRect` then reports that width
    /// instead of the text's (measured 2026-10-29: 74 pt text read as 300).
    static func measure(_ text: NSAttributedString, width: CGFloat) -> CGSize {
        let storage = NSTextStorage(attributedString: text)
        let layout = NSLayoutManager()
        let container = NSTextContainer(size: NSSize(width: width, height: .greatestFiniteMagnitude))
        container.lineFragmentPadding = 0
        layout.addTextContainer(container)
        storage.addLayoutManager(layout)
        layout.ensureLayout(for: container)
        return layout.usedRect(for: container).size
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var renderedKey = ""
        var measured: (key: String, size: CGSize)?

        func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
            let url = (link as? URL) ?? (link as? String).flatMap(URL.init(string:))
            if let url, ContentBlock.isAllowedLinkScheme(url) { NSWorkspace.shared.open(url) }
            return true // handled either way: a refused scheme does nothing
        }
    }
}

/// Markdown → attributed text in the session's look (pure; tested).
enum MarkdownAttributed {
    static func build(_ markdown: String, size: CGFloat) -> NSAttributedString {
        let options = AttributedString.MarkdownParsingOptions(
            allowsExtendedAttributes: false, interpretedSyntax: .full,
            failurePolicy: .returnPartiallyParsedIfPossible)
        guard let parsed = try? AttributedString(markdown: markdown, options: options) else {
            return NSAttributedString(string: markdown, attributes: bodyAttributes(size))
        }
        let out = NSMutableAttributedString()
        var lastBlock: Int?
        var lastList: Int?
        var lastWasCode = false
        for run in parsed.runs {
            let intent = run.presentationIntent
            let block = intent?.components.first?.identity
            let isCode = intent?.components.contains { if case .codeBlock = $0.kind { return true }; return false } ?? false
            if let lastBlock, block != lastBlock {
                // A new block: one line break, or a blank line between paragraphs.
                out.append(NSAttributedString(string: lastWasCode || isCode ? "\n" : "\n\n",
                                              attributes: bodyAttributes(size)))
            }
            if block != lastBlock, let marker = listMarker(intent), lastList != intent?.components.first?.identity {
                out.append(NSAttributedString(string: marker, attributes: bodyAttributes(size)))
                lastList = intent?.components.first?.identity
            }
            lastBlock = block
            lastWasCode = isCode
            var text = String(parsed[run.range].characters)
            if isCode, text.hasSuffix("\n") { text.removeLast() }
            out.append(NSAttributedString(string: text, attributes: attributes(for: run, intent: intent,
                                                                                isCode: isCode, size: size)))
        }
        return out
    }

    static func bodyAttributes(_ size: CGFloat) -> [NSAttributedString.Key: Any] {
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineSpacing = size * 0.3
        return [.font: NSFont.systemFont(ofSize: size, weight: .regular),
                .foregroundColor: NSColor(AeroricColor.text), .paragraphStyle: paragraph]
    }

    private static func listMarker(_ intent: PresentationIntent?) -> String? {
        guard let components = intent?.components else { return nil }
        for (index, component) in components.enumerated() {
            guard case let .listItem(ordinal) = component.kind else { continue }
            let depth = components[index...].filter {
                if case .listItem = $0.kind { return true }; return false
            }.count
            let indent = String(repeating: "    ", count: max(0, depth - 1))
            let ordered = components.dropFirst(index).contains { if case .orderedList = $0.kind { return true }; return false }
            return indent + (ordered ? "\(ordinal). " : "• ")
        }
        return nil
    }

    private static func attributes(for run: AttributedString.Runs.Run, intent: PresentationIntent?,
                                   isCode: Bool, size: CGFloat) -> [NSAttributedString.Key: Any] {
        var attributes = bodyAttributes(size)
        let inline = run.inlinePresentationIntent ?? []
        if isCode {
            let paragraph = NSMutableParagraphStyle()
            let box = NSTextBlock()
            box.backgroundColor = NSColor(AeroricColor.layer1)
            box.setContentWidth(100, type: .percentageValueType)
            box.setWidth(10, type: .absoluteValueType, for: .padding)
            paragraph.textBlocks = [box]
            paragraph.lineSpacing = 2
            attributes[.paragraphStyle] = paragraph
            attributes[.font] = monospaced(size - 1)
            attributes[.foregroundColor] = NSColor(AeroricColor.textSecondary)
            return attributes
        }
        if let header = intent?.components.compactMap({ component -> Int? in
            if case let .header(level) = component.kind { return level }
            return nil
        }).first {
            let scale: CGFloat = header == 1 ? 1.3 : header == 2 ? 1.15 : 1
            attributes[.font] = NSFont.systemFont(ofSize: size * scale, weight: .semibold)
        }
        if intent?.components.contains(where: { if case .blockQuote = $0.kind { return true }; return false }) == true {
            attributes[.foregroundColor] = NSColor(AeroricColor.textSecondary)
        }
        if inline.contains(.stronglyEmphasized) {
            attributes[.font] = NSFont.systemFont(ofSize: size, weight: .medium)
        }
        if inline.contains(.emphasized), let font = attributes[.font] as? NSFont {
            attributes[.font] = NSFontManager.shared.convert(font, toHaveTrait: .italicFontMask)
        }
        if inline.contains(.strikethrough) {
            attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
        }
        if inline.contains(.code) {
            attributes[.font] = monospaced(size * 0.92)
            attributes[.backgroundColor] = NSColor(AeroricColor.layer2)
        }
        if let link = run.link {
            attributes[.link] = link
        }
        return attributes
    }

    /// JetBrains Mono when the app registered it, else the system mono.
    private static func monospaced(_ size: CGFloat) -> NSFont {
        NSFont(name: "JetBrainsMono-Regular", size: size) ?? .monospacedSystemFont(ofSize: size, weight: .regular)
    }
}
