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
    @AppStorage(ConversationCJKFont.storageKey) private var cjkChoice = ConversationCJKFont.pingFang.rawValue
    @AppStorage(ConversationCJKFont.customKey) private var cjkCustom = ""

    private var cjkFamily: String? {
        _ = FontDownloads.shared.revision // a download just registered its fonts
        return ConversationCJKFont.resolve(ConversationCJKFont(rawValue: cjkChoice) ?? .pingFang, custom: cjkCustom)
    }

    func makeNSView(context: Context) -> NSTextView {
        // TextKit 1: code-block backgrounds are `NSTextBlock`s; our own
        // layout manager rounds the inline-code backgrounds (2026-10-29).
        let layout = MarkdownLayoutManager()
        let container = NSTextContainer(size: NSSize(width: 0, height: CGFloat.greatestFiniteMagnitude))
        context.coordinator.storage.addLayoutManager(layout)
        layout.addTextContainer(container)
        let view = NSTextView(frame: .zero, textContainer: container)
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
        let family = cjkFamily
        let key = "\(size)|\(colorScheme)|\(family ?? "")|\(FontDownloads.shared.revision)|\(text)"
        guard context.coordinator.renderedKey != key else { return }
        context.coordinator.renderedKey = key
        view.textStorage?.setAttributedString(MarkdownAttributed.build(text, size: size, cjkFamily: family))
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView view: NSTextView, context: Context) -> CGSize? {
        // Hug the text (2026-10-29): a short user message keeps a short
        // bubble; only text longer than the offered width wraps to it.
        let offered = proposal.width.flatMap { $0.isFinite && $0 > 0 ? $0 : nil } ?? 10_000
        guard let storage = view.textStorage else { return nil }
        let coordinator = context.coordinator
        let key = "\(size)|\(offered)|\(cjkChoice)|\(cjkCustom)|\(text)"
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
        /// The text view does not own its storage; this keeps it alive.
        let storage = NSTextStorage()
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
    static func build(_ markdown: String, size: CGFloat, cjkFamily: String? = nil) -> NSAttributedString {
        let fonts = Fonts(cjkFamily: cjkFamily)
        let options = AttributedString.MarkdownParsingOptions(
            allowsExtendedAttributes: false, interpretedSyntax: .full,
            failurePolicy: .returnPartiallyParsedIfPossible)
        guard let parsed = try? AttributedString(markdown: markdown, options: options) else {
            return NSAttributedString(string: markdown, attributes: bodyAttributes(size, fonts))
        }
        let out = NSMutableAttributedString()
        // 2026-10-29 layout (the owner's reference): blocks are separated by
        // one line break and spaced by their paragraph style — no blank lines
        // between list items; list text hangs past its marker.
        var blocks: [(start: Int, shape: BlockShape)] = []
        var lastBlock: Int?
        var numbered = Set<Int>()
        for run in parsed.runs {
            let intent = run.presentationIntent
            let block = intent?.components.first?.identity
            let isCode = intent?.components.contains { if case .codeBlock = $0.kind { return true }; return false } ?? false
            if block != lastBlock {
                if lastBlock != nil {
                    out.append(NSAttributedString(string: "\n", attributes: bodyAttributes(size, fonts)))
                }
                var shape = BlockShape.paragraph
                if isCode {
                    shape = .code
                } else if let item = listItem(intent) {
                    let first = numbered.insert(item.identity).inserted
                    shape = .list(depth: item.depth, first: first)
                    if first {
                        var marker = bodyAttributes(size, fonts)
                        marker[.foregroundColor] = NSColor(AeroricColor.textSecondary)
                        out.append(NSAttributedString(string: item.marker + "\t", attributes: marker))
                    }
                } else if intent?.components.contains(where: { if case .header = $0.kind { return true }; return false }) == true {
                    shape = .header
                }
                blocks.append((out.length - (shape.isFirstListLine ? (listItem(intent)?.marker.utf16.count ?? 0) + 1 : 0), shape))
            }
            lastBlock = block
            var text = String(parsed[run.range].characters)
            if isCode, text.hasSuffix("\n") { text.removeLast() }
            out.append(NSAttributedString(string: text, attributes: attributes(for: run, intent: intent,
                                                                                isCode: isCode, size: size, fonts: fonts)))
        }
        padInlineCode(out)
        for (index, block) in blocks.enumerated() where block.shape != .code {
            let end = index + 1 < blocks.count ? blocks[index + 1].start : out.length
            guard end > block.start else { continue }
            let next = index + 1 < blocks.count ? blocks[index + 1].shape : nil
            out.addAttribute(.paragraphStyle, value: paragraphStyle(block.shape, next: next, size: size),
                             range: NSRange(location: block.start, length: end - block.start))
        }
        return out
    }

    /// Room around each inline-code chip (2026-10-29): kern after the
    /// character before it and after its own last character, so the
    /// rounded background `MarkdownLayoutManager` draws never touches text.
    static func padInlineCode(_ out: NSMutableAttributedString) {
        let whole = NSRange(location: 0, length: out.length)
        out.enumerateAttribute(.inlineCode, in: whole) { value, range, _ in
            guard value != nil, range.length > 0 else { return }
            out.addAttribute(.kern, value: MarkdownLayoutManager.chipKern,
                             range: NSRange(location: NSMaxRange(range) - 1, length: 1))
            if range.location > 0 {
                out.addAttribute(.kern, value: MarkdownLayoutManager.chipKern,
                                 range: NSRange(location: range.location - 1, length: 1))
            }
        }
    }

    enum BlockShape: Equatable {
        case paragraph
        case header
        case code
        /// `first` is the item's own line, the one carrying its marker.
        case list(depth: Int, first: Bool)

        var isFirstListLine: Bool { if case .list(_, true) = self { return true }; return false }
        var isList: Bool { if case .list = self { return true }; return false }
    }

    /// Line height about 1.75; half a line between paragraphs, less between
    /// list items; list text hangs at a fixed indent past its marker.
    static func paragraphStyle(_ shape: BlockShape, next: BlockShape?, size: CGFloat) -> NSParagraphStyle {
        let style = NSMutableParagraphStyle()
        style.lineSpacing = size * 0.35
        style.paragraphSpacing = size * 0.7
        switch shape {
        case .paragraph, .code:
            break
        case .header:
            style.paragraphSpacingBefore = size * 0.4
            style.paragraphSpacing = size * 0.35
        case let .list(depth, first):
            let base = CGFloat(max(0, depth - 1)) * size * 1.6
            let hang = base + size * 1.6
            style.headIndent = hang
            style.firstLineHeadIndent = first ? base : hang
            style.tabStops = [NSTextTab(textAlignment: .left, location: hang)]
            style.defaultTabInterval = size * 1.6
            // Tight inside a list; a normal gap after its last item.
            style.paragraphSpacing = (next?.isList ?? false) ? size * 0.4 : size * 0.7
        }
        return style
    }

    static func bodyAttributes(_ size: CGFloat, _ fonts: Fonts = Fonts()) -> [NSAttributedString.Key: Any] {
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineSpacing = size * 0.35
        return [.font: fonts.text(size, .regular),
                .foregroundColor: NSColor(AeroricColor.text), .paragraphStyle: paragraph]
    }

    /// The innermost list item a block sits in: its identity, nesting depth
    /// and `1.` / `•` marker.
    private static func listItem(_ intent: PresentationIntent?) -> (identity: Int, depth: Int, marker: String)? {
        guard let components = intent?.components else { return nil }
        for (index, component) in components.enumerated() {
            guard case let .listItem(ordinal) = component.kind else { continue }
            let depth = components[index...].filter {
                if case .listItem = $0.kind { return true }; return false
            }.count
            // The list directly holding this item decides its style.
            let list = components.dropFirst(index + 1).first {
                switch $0.kind { case .orderedList, .unorderedList: return true; default: return false }
            }
            let ordered = list.map { if case .orderedList = $0.kind { return true }; return false } ?? false
            return (component.identity, depth, ordered ? "\(ordinal)." : "•")
        }
        return nil
    }

    private static func attributes(for run: AttributedString.Runs.Run, intent: PresentationIntent?,
                                   isCode: Bool, size: CGFloat, fonts: Fonts) -> [NSAttributedString.Key: Any] {
        var attributes = bodyAttributes(size, fonts)
        let inline = run.inlinePresentationIntent ?? []
        if isCode {
            let paragraph = NSMutableParagraphStyle()
            let box = RoundedCodeBlock()
            box.setContentWidth(100, type: .percentageValueType)
            box.setWidth(16, type: .absoluteValueType, for: .padding, edge: .minX)
            box.setWidth(16, type: .absoluteValueType, for: .padding, edge: .maxX)
            box.setWidth(RoundedCodeBlock.inset, type: .absoluteValueType, for: .padding, edge: .minY)
            box.setWidth(RoundedCodeBlock.inset, type: .absoluteValueType, for: .padding, edge: .maxY)
            box.setWidth(4, type: .absoluteValueType, for: .margin, edge: .minY)
            box.setWidth(8, type: .absoluteValueType, for: .margin, edge: .maxY)
            paragraph.textBlocks = [box]
            paragraph.lineSpacing = RoundedCodeBlock.lineSpacing
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
            attributes[.font] = fonts.text(size * scale, .semibold)
        }
        if intent?.components.contains(where: { if case .blockQuote = $0.kind { return true }; return false }) == true {
            attributes[.foregroundColor] = NSColor(AeroricColor.textSecondary)
        }
        if inline.contains(.stronglyEmphasized) {
            attributes[.font] = fonts.text(size, .medium)
        }
        if inline.contains(.emphasized), let font = attributes[.font] as? NSFont {
            attributes[.font] = NSFontManager.shared.convert(font, toHaveTrait: .italicFontMask)
        }
        if inline.contains(.strikethrough) {
            attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
        }
        if inline.contains(.code) {
            attributes[.font] = monospaced(size * 0.92)
            attributes[.inlineCode] = true
        }
        if let link = run.link {
            attributes[.link] = link
        }
        return attributes
    }

    /// Message text fonts: the system font, Chinese in the chosen family.
    struct Fonts {
        var cjkFamily: String?
        func text(_ size: CGFloat, _ weight: NSFont.Weight) -> NSFont {
            ConversationCJKFont.font(size: size, weight: weight, family: cjkFamily)
        }
    }

    /// JetBrains Mono when the app registered it, else the system mono.
    private static func monospaced(_ size: CGFloat) -> NSFont {
        NSFont(name: "JetBrainsMono-Regular", size: size) ?? .monospacedSystemFont(ofSize: size, weight: .regular)
    }
}

extension NSAttributedString.Key {
    /// Marks inline code; drawn as a rounded chip, not a flat highlight.
    static let inlineCode = NSAttributedString.Key("aeroric.inlineCode")
}

/// Code blocks with large rounded corners and a hairline edge (2026-10-29).
/// TextKit adds line spacing under a block's last line too, so the drawn
/// box stops that much short of the block: the text sits in the middle.
final class RoundedCodeBlock: NSTextBlock {
    static let lineSpacing: CGFloat = 3
    static let inset: CGFloat = 12
    static let radius: CGFloat = 14

    override func drawBackground(withFrame frameRect: NSRect, in controlView: NSView?,
                                 characterRange charRange: NSRange, layoutManager: NSLayoutManager) {
        // The whole block's frame, so a partial redraw keeps its corners.
        var frame = frameRect
        if let storage = layoutManager.textStorage {
            let blockRange = storage.range(of: self, at: charRange.location)
            if blockRange.length > 0 {
                let glyphs = layoutManager.glyphRange(forCharacterRange: blockRange, actualCharacterRange: nil)
                frame = layoutManager.boundsRect(for: self, glyphRange: glyphs)
            }
        }
        frame.size.height -= Self.lineSpacing
        let box = NSBezierPath(roundedRect: frame.insetBy(dx: 0.5, dy: 0.5),
                               xRadius: Self.radius, yRadius: Self.radius)
        NSGraphicsContext.saveGraphicsState()
        NSBezierPath(rect: frameRect).addClip()
        NSColor(AeroricColor.layer1).setFill()
        box.fill()
        NSColor(AeroricColor.separator2).setStroke()
        box.lineWidth = 1
        box.stroke()
        NSGraphicsContext.restoreGraphicsState()
    }
}

/// Draws inline code as rounded chips (2026-10-29); everything else is the
/// stock TextKit 1 layout.
final class MarkdownLayoutManager: NSLayoutManager {
    /// Kern added on each side of a chip (see `padInlineCode`).
    static let chipKern: CGFloat = 10
    /// How far a chip reaches into that room; the rest is the gap to text.
    static let chipPad: CGFloat = 6

    override func drawBackground(forGlyphRange glyphsToShow: NSRange, at origin: NSPoint) {
        // Chips first, so a selection is still drawn over them.
        defer { super.drawBackground(forGlyphRange: glyphsToShow, at: origin) }
        guard let storage = textStorage, let container = textContainers.first else { return }
        let characters = characterRange(forGlyphRange: glyphsToShow, actualGlyphRange: nil)
        storage.enumerateAttribute(.inlineCode, in: characters) { value, range, _ in
            guard value != nil else { return }
            let glyphs = glyphRange(forCharacterRange: range, actualCharacterRange: nil)
            enumerateEnclosingRects(forGlyphRange: glyphs, withinSelectedGlyphRange: NSRange(location: NSNotFound, length: 0),
                                    in: container) { rect, _ in
                var chip = rect.offsetBy(dx: origin.x, dy: origin.y)
                // Left: reach into the kern before; right: give back the
                // part of the last character's kern that is the gap.
                chip.origin.x -= Self.chipPad
                chip.size.width += Self.chipPad - (Self.chipKern - Self.chipPad)
                chip = chip.insetBy(dx: 0, dy: 1)
                let path = NSBezierPath(roundedRect: chip, xRadius: 6, yRadius: 6)
                NSColor(AeroricColor.layer2).setFill()
                path.fill()
                NSColor(AeroricColor.separator1).setStroke()
                path.lineWidth = 1
                path.stroke()
            }
        }
    }
}
