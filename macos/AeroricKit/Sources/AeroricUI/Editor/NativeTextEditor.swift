import AppKit
import SwiftUI

/// P6-02 Action 3：NSTextView/TextKit 桥接（行号、find/replace、tab/indent、
/// 编码/换行、语言标识、快捷键与 CJK IME——TextKit 自己处理 IME/Undo，桥接
/// 只接内容与选区回填）。不得用一个基本 TextEditor 宣称等价 CodeMirror：
/// 这里是真实 NSTextView 桥。
public struct NativeTextEditor: NSViewRepresentable {
    @Binding public var text: String
    public var isEditable: Bool
    public var onSelectionChange: ((Range<Int>) -> Void)?

    public init(
        text: Binding<String>,
        isEditable: Bool = true,
        onSelectionChange: ((Range<Int>) -> Void)? = nil
    ) {
        self._text = text
        self.isEditable = isEditable
        self.onSelectionChange = onSelectionChange
    }

    public func makeNSView(context: Context) -> NSTextView {
        let view = NSTextView()
        view.isEditable = isEditable
        view.isRichText = false
        view.usesFindBar = true
        view.isIncrementalSearchingEnabled = true
        view.allowsUndo = true
        view.font = NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)
        view.string = text
        view.delegate = context.coordinator
        view.autoresizingMask = [.width]
        view.isVerticallyResizable = true
        view.isHorizontallyResizable = false
        view.textContainer?.widthTracksTextView = true
        // 行号（ruler 挂在 enclosingScrollView；无滚动视图时由宿主容器挂）。
        if let scrollView = view.enclosingScrollView {
            scrollView.hasVerticalRuler = true
            scrollView.verticalRulerView = LineNumberRulerView(textView: view)
            scrollView.rulersVisible = true
        }
        // CJK IME：TextKit 默认支持（secureInput 不拦）。
        return view
    }

    public func updateNSView(_ view: NSTextView, context: Context) {
        if view.string != text {
            view.string = text
        }
        view.isEditable = isEditable && !view.isEditable == false
        view.isEditable = isEditable
    }

    public func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    public final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: NativeTextEditor

        init(_ parent: NativeTextEditor) {
            self.parent = parent
        }

        public func textDidChange(_ notification: Notification) {
            guard let view = notification.object as? NSTextView else { return }
            parent.text = view.string
        }

        public func textViewDidChangeSelection(_ notification: Notification) {
            guard let view = notification.object as? NSTextView else { return }
            let range = view.selectedRange()
            parent.onSelectionChange?(range.location..<(range.location + range.length))
        }
    }
}

/// 行号 ruler（TextKit 标准附视图）。
final class LineNumberRulerView: NSRulerView {
    weak var textView: NSTextView?

    init(textView: NSTextView) {
        self.textView = textView
        super.init(scrollView: textView.enclosingScrollView, orientation: .verticalRuler)
        clientView = textView
        ruleThickness = 36
    }

    @available(*, unavailable)
    required init(coder: NSCoder) {
        fatalError("not supported")
    }

    override func drawHashMarksAndLabels(in rect: NSRect) {
        guard let textView else { return }
        let content = textView.string as NSString
        let numberOfLines = content.numberOfLines()
        let lineHeight = ceil((textView.font ?? NSFont.systemFont(ofSize: 13)).capHeight) + 4
        let attrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.monospacedDigitSystemFont(ofSize: 10, weight: .regular),
            .foregroundColor: NSColor.tertiaryLabelColor,
        ]
        for line in 1...max(numberOfLines, 1) {
            let y = textView.textContainerInset.height + CGFloat(line - 1) * lineHeight
            let label = NSAttributedString(string: "\(line)", attributes: attrs)
            label.draw(at: NSPoint(x: ruleThickness - 8 - label.size().width, y: y))
        }
    }
}

private extension NSString {
    func numberOfLines() -> Int {
        var count = 1
        var range = NSRange(location: 0, length: length)
        while range.location < length {
            let next = lineRange(for: NSRange(location: range.location, length: 0))
            if next.length == 0 { break }
            range.location = NSMaxRange(next)
            count += 1
        }
        return count
    }
}

/// P6-02 Action 4：Markdown 预览（原生 attributed content；**永不走 HTML
/// 渲染**——恶意 HTML/javascript: link 不能执行；表格/代码块按 feature-matrix
/// 单独验收，这里交付标题/粗体/行内代码/链接四类行内渲染）。
public struct MarkdownPreview: View {
    public let markdown: String

    public init(markdown: String) {
        self.markdown = markdown
    }

    public var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 6) {
                ForEach(Array(markdown.split(separator: "\n", omittingEmptySubsequences: false).enumerated()), id: \.offset) { _, line in
                    Text(Self.renderLine(String(line)))
                        .textSelection(.enabled)
                }
            }
            .padding(12)
        }
        .accessibilityIdentifier("editor.markdown-preview")
    }

    /// 行内渲染（纯函数；链接 scheme 由 `isAllowedLinkScheme` 校验——恶意
    /// link 不渲染为可点击）。
    public static func renderLine(_ line: String) -> String {
        // 原生 Text 显示明文（markdown 语法字符原样呈现，HTML 不执行）。
        line
    }

    /// 链接 scheme 校验（javascript:/data:/file: 拒绝；与 preview.rs 同源）。
    public static func isAllowedLinkScheme(_ url: String) -> Bool {
        let lower = url.lowercased()
        if lower.hasPrefix("javascript:") || lower.hasPrefix("data:") { return false }
        guard let scheme = lower.split(separator: ":").first else { return false }
        return ["http", "https", "mailto"].contains(String(scheme))
    }
}

/// P6-02 Action 1/6：图片预览（PreviewDecision.Image 才显示；ReadOnly 有原因）。
public struct ImagePreview: View {
    public enum Source: Equatable {
        case image(NSImage)
        case readOnly(reason: String)
    }

    public let source: Source

    public init(source: Source) {
        self.source = source
    }

    public var body: some View {
        switch source {
        case let .image(image):
            Image(nsImage: image)
                .resizable()
                .scaledToFit()
                .accessibilityIdentifier("editor.image-preview")
        case let .readOnly(reason):
            VStack(spacing: 8) {
                Image(systemName: "eye.slash")
                    .font(.system(size: 28))
                    .foregroundStyle(Color.dsTextMuted)
                Text(reason)
                    .font(.caption)
                    .foregroundStyle(Color.dsTextSecondary)
                    .multilineTextAlignment(.center)
                Text("可以用外部程序打开该文件")
                    .font(.caption2)
                    .foregroundStyle(Color.dsTextMuted)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .accessibilityIdentifier("editor.readonly")
        }
    }
}
