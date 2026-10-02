import XCTest
@testable import AeroricUI

/// P6-02 验收：DocumentStoreTests——dirty/revision/冲突簿记、UTF-8 BOM/
/// CRLF 内容推进、外部修改（冲突不可静默覆盖）、关 tab 保留未保存提示。
@MainActor
final class DocumentStoreTests: XCTestCase {

    func testOpenAndDirtyTracking() {
        let store = DocumentStore()
        store.open(DocumentStore.Document(path: "/p/a.txt", diskContent: "v1", content: "v1"))
        XCTAssertFalse(store.isDirty)
        store.updateContent("v2", for: "/p/a.txt")
        XCTAssertTrue(store.isDirty, "编辑后 dirty")
        store.discardChanges(path: "/p/a.txt")
        XCTAssertFalse(store.isDirty, "放弃编辑回基线")
    }

    func testSaveAdvancesBaselineAndRevision() {
        let store = DocumentStore()
        store.open(DocumentStore.Document(path: "/p/a.txt", diskContent: "v1", content: "v2"))
        XCTAssertEqual(store.selected?.revision, 0)
        store.markSaved(path: "/p/a.txt", diskContent: "v2")
        XCTAssertEqual(store.selected?.diskContent, "v2", "基线推进")
        XCTAssertEqual(store.selected?.revision, 1, "保存递增 revision")
        XCTAssertFalse(store.isDirty)
        // 重复 save：revision 仍递增（幂等语义由磁盘内容保证）。
        store.markSaved(path: "/p/a.txt", diskContent: "v2")
        XCTAssertEqual(store.selected?.revision, 2, "重复 save 也推进 revision")
    }

    func testConflictIsRecordedNotSilentlyOverwritten() {
        let store = DocumentStore()
        store.open(DocumentStore.Document(path: "/p/a.txt", diskContent: "v1", content: "v2"))
        // 宿主拒绝覆盖（磁盘内容与打开基线不符）→ recordConflict，不可静默覆盖。
        store.recordConflict(path: "/p/a.txt")
        XCTAssertTrue(store.conflictedPaths.contains("/p/a.txt"))
        // markSaved 清除冲突（覆盖后基线推进）。
        store.markSaved(path: "/p/a.txt", diskContent: "v2")
        XCTAssertFalse(store.conflictedPaths.contains("/p/a.txt"))
    }

    func testUTF8BOMAndCRLFContentRoundTrip() {
        let store = DocumentStore()
        // UTF-8 BOM 保留（编辑器层不剥）。
        let bom = "\u{FEFF}hello"
        store.open(DocumentStore.Document(path: "/p/bom.txt", diskContent: bom, content: bom))
        store.updateContent(bom, for: "/p/bom.txt")
        XCTAssertEqual(store.selected?.content, bom, "UTF-8 BOM 保留")
        // CRLF 保留（换行不归一）。
        let crlf = "line1\r\nline2\r\n"
        store.open(DocumentStore.Document(path: "/p/crlf.txt", diskContent: crlf, content: crlf))
        XCTAssertEqual(store.selected?.content, crlf, "CRLF 保留")
    }

    func testCloseReportsDirtyAndKeepsPromptDecision() {
        let store = DocumentStore()
        store.open(DocumentStore.Document(path: "/p/a.txt", diskContent: "v1", content: "v1"))
        store.updateContent("v2", for: "/p/a.txt")
        let dirty = store.close(path: "/p/a.txt")
        XCTAssertTrue(dirty, "关未保存的 tab 返回 dirty（调用方提示 save/discard/cancel）")
        XCTAssertTrue(store.documents.isEmpty)
        // 未 dirty 的 tab 直接关。
        store.open(DocumentStore.Document(path: "/p/b.txt", diskContent: "x", content: "x"))
        let notDirty = store.close(path: "/p/b.txt")
        XCTAssertFalse(notDirty)
    }

    func testSaveConflictOutcomeRouting() {
        let coordinator = EditorCoordinator(store: DocumentStore())
        coordinator.store.open(DocumentStore.Document(path: "/p/a.txt", diskContent: "v1", content: "v2"))
        // 成功 → saved。
        XCTAssertEqual(coordinator.handleSaveResult(path: "/p/a.txt", result: .success(()), savedContent: "v2"), .saved)
        // 冲突 → conflict（宿主拒绝覆盖）。
        struct Conflict: Error {}
        XCTAssertEqual(coordinator.handleSaveResult(path: "/p/a.txt", result: .failure(Conflict()), savedContent: nil), .conflict)
        // 其他失败 → rejected。
        struct Boom: Error {}
        if case let .rejected(reason) = coordinator.handleSaveResult(path: "/p/a.txt", result: .failure(Boom()), savedContent: nil) {
            XCTAssertFalse(reason.isEmpty)
        } else {
            XCTFail("非冲突失败应为 rejected")
        }
    }

    func testMarkdownLinkSchemeGuard() {
        XCTAssertTrue(MarkdownPreview.isAllowedLinkScheme("https://example.com"))
        XCTAssertTrue(MarkdownPreview.isAllowedLinkScheme("mailto:a@b.c"))
        XCTAssertFalse(MarkdownPreview.isAllowedLinkScheme("javascript:alert(1)"))
        XCTAssertFalse(MarkdownPreview.isAllowedLinkScheme("data:text/html,<script>"))
        XCTAssertFalse(MarkdownPreview.isAllowedLinkScheme("file:///etc/passwd"))
    }
}
