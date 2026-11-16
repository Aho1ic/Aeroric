import AeroricClient
@testable import AeroricUI
import XCTest

/// Q-R4 (2026-11-13): PDF and image preview, excerpts back to the composer.
@MainActor
final class PDFExcerptTests: XCTestCase {
    private actor NoReader: ProjectResourceReading {
        var calls = 0
        func directory(projectID: String, path: String) async throws -> [FilesClient.FileEntry] { calls += 1; return [] }
        func changes(projectPath: String) async throws -> [ResourceGitChange] { calls += 1; return [] }
        func branches(projectPath: String) async throws -> [ResourceGitBranch] { calls += 1; return [] }
        func diff(projectPath: String, path: String, staged: Bool) async throws -> String { calls += 1; return "" }
        func text(projectID: String, path: String) async throws -> String { calls += 1; return "" }
        func write(projectID: String, path: String, content: String) async throws { calls += 1 }
    }

    func testSoftLineBreaksAreJoinedTheWayTheTextReads() {
        XCTAssertEqual(PDFExcerpt.clean("Task-aligned assign-\nment picks the top\nk anchors."),
                       "Task-aligned assignment picks the top k anchors.")
        XCTAssertEqual(PDFExcerpt.clean("标签分配按对齐度\n量选出前 k 个"), "标签分配按对齐度量选出前 k 个")
        XCTAssertEqual(PDFExcerpt.clean("First para.\n\n\nSecond   para."), "First para.\n\nSecond para.")
        XCTAssertEqual(PDFExcerpt.clean("Mosaic-\nBased"), "Mosaic- Based", "a capital after the hyphen is a compound")
        let long = PDFExcerpt.clean(String(repeating: "a", count: PDFExcerpt.limit + 10))
        XCTAssertTrue(long.hasSuffix("…（摘录过长，只带了前 \(PDFExcerpt.limit) 字）"))
    }

    func testPagesSayThePositionAndAPrintedLabelThatDiffers() {
        XCTAssertEqual(PDFExcerpt.pages(first: 2, last: 2, label: "3"), "第 3 页")
        XCTAssertEqual(PDFExcerpt.pages(first: 2, last: 2, label: nil), "第 3 页")
        XCTAssertEqual(PDFExcerpt.pages(first: 13, last: 13, label: "12"), "第 14 页（页码 12）")
        XCTAssertEqual(PDFExcerpt.pages(first: 2, last: 3, label: "3"), "第 3–4 页")
    }

    func testAnExcerptIsAQuoteWithItsSourceThenTheNote() {
        let text = PDFExcerpt.markdown(text: "line one\n\nline two", source: "papers/tal.pdf",
                                       pages: "第 3 页", note: "  这里和 mmyolo 的 topk 不一样？ ")
        XCTAssertEqual(text, """
        > line one
        >
        > line two
        >
        > —— `papers/tal.pdf` 第 3 页

        这里和 mmyolo 的 topk 不一样？
        """)
        XCTAssertFalse(PDFExcerpt.markdown(text: "x", source: "a.pdf", pages: "第 1 页", note: " ").contains("\n\n"))
    }

    func testTheInboxHandsExcerptsToOneSessionOnceAndAppendsThem() {
        let inbox = ComposerInbox()
        inbox.post("> a", to: "t1")
        inbox.post("  ", to: "t1")
        inbox.post("> b", to: "t1")
        inbox.post("> c", to: "t2")
        XCTAssertEqual(inbox.count(for: "t1"), 2)
        let pieces = inbox.take("t1")
        XCTAssertEqual(pieces, ["> a", "> b"])
        XCTAssertEqual(inbox.count(for: "t1"), 0)
        XCTAssertEqual(inbox.count(for: "t2"), 1)
        XCTAssertEqual(ComposerInbox.appending(pieces, to: "看看这两段\n"), "看看这两段\n\n> a\n\n> b")
        XCTAssertEqual(ComposerInbox.appending(pieces, to: "  "), "> a\n\n> b")
    }

    func testALocalPDFOrImageOpensFromDiskWithoutTheHost() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("pdf-preview-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        try Data("%PDF-1.4\n".utf8).write(to: root.appendingPathComponent("paper.pdf"))
        try Data([0x89, 0x50, 0x4E, 0x47]).write(to: root.appendingPathComponent("shot.PNG"))
        let book = ProjectResourceBook(), reader = NoReader()

        let pdf = ResourcePreviewTarget(projectID: "p", path: "paper.pdf", kind: .file)
        book.select(pdf)
        await book.loadPreview(pdf, projectPath: root.path, reader: reader)
        XCTAssertEqual(book.scene("p").previewState, .loaded)
        XCTAssertEqual(book.scene("p").previewMedia?.kind, .pdf)
        XCTAssertEqual(book.scene("p").previewMedia?.url.lastPathComponent, "paper.pdf")
        XCTAssertNil(book.scene("p").previewText)

        let image = ResourcePreviewTarget(projectID: "p", path: "shot.PNG", kind: .file)
        book.select(image)
        XCTAssertNil(book.scene("p").previewMedia, "switching tabs clears the old viewer")
        await book.loadPreview(image, projectPath: root.path, reader: nil)
        XCTAssertEqual(book.scene("p").previewMedia?.kind, .image, "no host is needed for a local file")

        let missing = ResourcePreviewTarget(projectID: "p", path: "gone.pdf", kind: .file)
        book.select(missing)
        await book.loadPreview(missing, projectPath: root.path, reader: reader)
        XCTAssertEqual(book.scene("p").previewState, .failed("文件不在了，或没有读取权限"))

        let remote = ResourcePreviewTarget(projectID: "r", path: "paper.pdf", kind: .file)
        book.select(remote)
        await book.loadPreview(remote, projectPath: "/no/such/remote/project", reader: reader)
        guard case .unsupported = book.scene("r").previewState else { return XCTFail("remote stays unsupported") }

        let calls = await reader.calls
        XCTAssertEqual(calls, 0)
    }

    func testALinkedFolderPDFResolvesToItsOwnFolder() {
        let path = LinkedFolderPath.node(folder: "/Users/me/papers", relative: "yolo/tal.pdf")
        XCTAssertEqual(ResourceMedia.localURL(path: path, projectPath: "/remote/x")?.path, "/Users/me/papers/yolo/tal.pdf")
        XCTAssertEqual(ResourceMedia.kind(forExtension: "PDF"), .pdf)
        XCTAssertEqual(ResourceMedia.kind(forExtension: "heic"), .image)
        XCTAssertNil(ResourceMedia.kind(forExtension: "zip"))
    }
}
