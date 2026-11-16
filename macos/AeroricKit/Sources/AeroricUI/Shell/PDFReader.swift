import AppKit
import PDFKit
import SwiftUI

/// Q-R4 (2026-11-13): where a PDF excerpt goes — the session in front.
public struct PDFExcerptTarget {
    public let title: String
    public let post: (String) -> Void
    public init(title: String, post: @escaping (String) -> Void) {
        self.title = title; self.post = post
    }
}

/// How a PDF selection becomes text in the composer: a quote with where it
/// came from, then the note. Nothing is written into the PDF.
public enum PDFExcerpt {
    public static let limit = 4000

    /// PDF text breaks lines inside sentences. Joins them back: no space
    /// between CJK characters, hyphenated words rejoined, blank lines kept as
    /// paragraph breaks; overlong text is cut and says so.
    public static func clean(_ raw: String) -> String {
        let lines = raw.replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
            .components(separatedBy: "\n")
            .map { $0.trimmingCharacters(in: .whitespaces) }
        var out = ""
        for line in lines {
            if line.isEmpty {
                if !out.isEmpty, !out.hasSuffix("\n\n") { out += "\n\n" }
                continue
            }
            if out.isEmpty || out.hasSuffix("\n\n") {
                out += line
            } else if out.hasSuffix("-"), out.dropLast().last?.isLetter == true,
                      line.first?.isLowercase == true {
                out.removeLast()
                out += line
            } else if isCJK(out.last) || isCJK(line.first) {
                out += line
            } else {
                out += " " + line
            }
        }
        out = out.replacingOccurrences(of: #"[ \t]{2,}"#, with: " ", options: .regularExpression)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard out.count > limit else { return out }
        return String(out.prefix(limit)) + "…（摘录过长，只带了前 \(limit) 字）"
    }

    /// 0-based page indices to words. A printed page label that differs from
    /// the position (iv, 12 on the 14th sheet) is added in brackets.
    public static func pages(first: Int, last: Int, label: String?) -> String {
        if last > first { return "第 \(first + 1)–\(last + 1) 页" }
        if let label, !label.isEmpty, label != String(first + 1) {
            return "第 \(first + 1) 页（页码 \(label)）"
        }
        return "第 \(first + 1) 页"
    }

    public static func markdown(text: String, source: String, pages: String, note: String) -> String {
        let quote = clean(text).components(separatedBy: "\n")
            .map { $0.isEmpty ? ">" : "> \($0)" }
            .joined(separator: "\n")
        var out = "\(quote)\n>\n> —— `\(source)` \(pages)"
        let note = note.trimmingCharacters(in: .whitespacesAndNewlines)
        if !note.isEmpty { out += "\n\n\(note)" }
        return out
    }

    private static func isCJK(_ character: Character?) -> Bool {
        guard let scalar = character?.unicodeScalars.first else { return false }
        switch scalar.value {
        case 0x3000...0x303F, 0x3040...0x30FF, 0x3400...0x4DBF, 0x4E00...0x9FFF, 0xF900...0xFAFF, 0xFF00...0xFFEF:
            return true
        default:
            return false
        }
    }
}

/// The reader's state outside the PDFView: page, selection, search hits.
@MainActor @Observable
final class PDFReaderModel {
    var pageCount = 0
    var pageIndex = 0
    var selectionText = ""
    var selectionPages = ""
    var matches: [PDFSelection] = []
    var matchIndex = 0
    var searched = false
    @ObservationIgnored weak var view: PDFView?

    func attach(_ view: PDFView) {
        self.view = view
        pageCount = view.document?.pageCount ?? 0
        pageChanged()
    }

    func pageChanged() {
        guard let view, let page = view.currentPage, let document = view.document else { return }
        pageIndex = document.index(for: page)
    }

    func selectionChanged() {
        guard let view, let document = view.document, let selection = view.currentSelection,
              let text = selection.string?.trimmingCharacters(in: .whitespacesAndNewlines), !text.isEmpty else {
            selectionText = ""; selectionPages = ""
            return
        }
        let indices = selection.pages.map { document.index(for: $0) }.sorted()
        selectionText = text
        selectionPages = PDFExcerpt.pages(first: indices.first ?? 0, last: indices.last ?? 0,
                                          label: selection.pages.first?.label)
    }

    func search(_ query: String) {
        let query = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let view, let document = view.document, !query.isEmpty else {
            matches = []; searched = false; view?.highlightedSelections = nil
            return
        }
        matches = document.findString(query, withOptions: [.caseInsensitive])
        searched = true
        matchIndex = 0
        view.highlightedSelections = matches.isEmpty ? nil : matches
        show()
    }

    func next() {
        guard !matches.isEmpty else { return }
        matchIndex = (matchIndex + 1) % matches.count
        show()
    }

    func clearSelection() { view?.clearSelection() }

    private func show() {
        guard matches.indices.contains(matchIndex), let view else { return }
        view.setCurrentSelection(matches[matchIndex], animate: true)
        view.go(to: matches[matchIndex])
    }
}

private struct PDFKitView: NSViewRepresentable {
    let document: PDFDocument
    let model: PDFReaderModel

    @MainActor final class Coordinator: NSObject {
        let model: PDFReaderModel
        init(model: PDFReaderModel) { self.model = model }
        @objc func selectionChanged(_ note: Notification) { model.selectionChanged() }
        @objc func pageChanged(_ note: Notification) { model.pageChanged() }
    }

    func makeCoordinator() -> Coordinator { Coordinator(model: model) }

    func makeNSView(context: Context) -> PDFView {
        let view = PDFView()
        view.document = document
        view.autoScales = true
        view.displayMode = .singlePageContinuous
        view.backgroundColor = .clear
        // Selector observers go away with the coordinator.
        NotificationCenter.default.addObserver(context.coordinator, selector: #selector(Coordinator.selectionChanged(_:)),
                                               name: .PDFViewSelectionChanged, object: view)
        NotificationCenter.default.addObserver(context.coordinator, selector: #selector(Coordinator.pageChanged(_:)),
                                               name: .PDFViewPageChanged, object: view)
        model.attach(view)
        return view
    }

    func updateNSView(_ view: PDFView, context: Context) {}
}

/// Centre preview for a local PDF: read, search, and take a selected passage
/// with a note back to the session's composer.
struct PDFReaderView: View {
    let url: URL
    /// What the excerpt names as its source: project-relative, or absolute
    /// for a linked folder.
    let source: String
    let target: PDFExcerptTarget?
    @State private var model = PDFReaderModel()
    @State private var document: PDFDocument?
    @State private var failure: String?
    @State private var query = ""
    @State private var note = ""
    @State private var added = 0

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            Divider()
            if let document {
                PDFKitView(document: document, model: model)
                    .accessibilityIdentifier("preview.pdf")
            } else if let failure {
                PaneEmptyState(systemImage: "doc.questionmark", title: "打不开这个 PDF", hint: failure)
            } else {
                ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            if document != nil { excerptBar }
        }
        .task(id: url) { open() }
    }

    private func open() {
        guard let document = PDFDocument(url: url) else {
            failure = "文件不是有效的 PDF，或已经损坏。"
            return
        }
        guard !document.isLocked else {
            failure = "这个 PDF 有密码，暂时不支持。"
            return
        }
        self.document = document
    }

    private var toolbar: some View {
        HStack(spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: "magnifyingglass").foregroundStyle(AeroricColor.textTertiary)
                TextField("在 PDF 里搜索", text: $query)
                    .textFieldStyle(.plain)
                    .onSubmit { model.matches.isEmpty || !model.searched ? model.search(query) : model.next() }
                    .onChange(of: query) { _, _ in model.search("") }
                    .accessibilityIdentifier("preview.pdf.search")
            }
            .padding(.horizontal, AeroricSpace.l)
            .padding(.vertical, AeroricSpace.xs + 1)
            .background(AeroricColor.layer2, in: Capsule(style: .continuous))
            .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator1, lineWidth: 1) }
            .frame(maxWidth: 280)
            if model.searched {
                Text(model.matches.isEmpty ? "没有找到" : "\(model.matchIndex + 1) / \(model.matches.count)")
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityIdentifier("preview.pdf.matches")
                if model.matches.count > 1 {
                    Button("下一个") { model.next() }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                }
            }
            Spacer()
            if model.pageCount > 0 {
                Text("第 \(model.pageIndex + 1) / \(model.pageCount) 页")
                    .foregroundStyle(AeroricColor.textSecondary)
                    .accessibilityIdentifier("preview.pdf.page")
            }
        }
        .font(AeroricType.mono(AeroricType.caption))
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    @ViewBuilder private var excerptBar: some View {
        if !model.selectionText.isEmpty || added > 0 {
            Divider()
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                if !model.selectionText.isEmpty {
                    Text("选中 · \(model.selectionPages)：\(model.selectionText)")
                        .lineLimit(2)
                        .foregroundStyle(AeroricColor.textSecondary)
                    if let target {
                        HStack(spacing: AeroricSpace.s) {
                            TextField("批注（可选）", text: $note)
                                .textFieldStyle(.plain)
                                .onSubmit { add(to: target) }
                                .padding(.horizontal, AeroricSpace.l)
                                .padding(.vertical, AeroricSpace.xs + 1)
                                .background(AeroricColor.layer2, in: Capsule(style: .continuous))
                                .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator1, lineWidth: 1) }
                                .accessibilityIdentifier("preview.pdf.note")
                            Button("加到输入框") { add(to: target) }
                                .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                                .fixedSize()
                                .help("追加到「\(target.title)」的输入框，不会发送")
                                .accessibilityIdentifier("preview.pdf.excerpt")
                        }
                    } else {
                        Text("先打开一个会话，摘录才能带回它的输入框。")
                            .foregroundStyle(AeroricColor.textTertiary)
                    }
                }
                if added > 0, let target {
                    Text("已加 \(added) 段到「\(target.title)」的输入框，回到会话可以编辑后再发送。")
                        .foregroundStyle(AeroricColor.textTertiary)
                        .accessibilityIdentifier("preview.pdf.added")
                }
            }
            .font(AeroricType.mono(AeroricType.caption))
            .padding(12)
        }
    }

    private func add(to target: PDFExcerptTarget) {
        guard !model.selectionText.isEmpty else { return }
        target.post(PDFExcerpt.markdown(text: model.selectionText, source: source,
                                        pages: model.selectionPages, note: note))
        added += 1
        note = ""
        model.clearSelection()
    }
}

/// Centre preview for a local image (Q-R4): view only.
struct ImageFileView: View {
    let url: URL
    @State private var image: NSImage?
    @State private var loaded = false

    var body: some View {
        Group {
            if let image {
                ImagePreview(source: .image(image)).padding(12)
            } else if loaded {
                ImagePreview(source: .readOnly(reason: "图片读不出来，格式可能不受支持。"))
            } else {
                ProgressView().controlSize(.small)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .task(id: url) {
            image = NSImage(contentsOf: url)
            loaded = true
        }
    }
}
