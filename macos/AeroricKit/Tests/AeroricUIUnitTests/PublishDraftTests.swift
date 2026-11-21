import AeroricProtocol
@testable import AeroricUI
import XCTest

private let writeJSON: JSONValue = .object([
    "id": .string("ghw-1"), "projectId": .string("p1"), "reviewId": .string("prrev-1"),
    "repo": .object(["owner": .string("acme"), "name": .string("widget")]),
    "number": .number("7"), "headSha": .string("abcdef1234567890"), "state": .string("unknown"),
    "items": .array([
        .object(["id": .string("summary"), "body": .string("审查了提交 abcdef1：2 条意见。"), "included": .bool(true),
                 "state": .string("sent"), "url": .string("https://github.com/acme/widget/pull/7#c1")]),
        .object(["id": .string("c1"), "path": .string("src/a.rs"), "line": .number("2"),
                 "body": .string("**严重**：b 没有测试\n\n依据：看到了"), "included": .bool(true),
                 "state": .string("unknown"), "reason": .string("发出后没有收到回答")]),
        .object(["id": .string("c2"), "path": .string("src/b.rs"), "line": .number("5"), "body": .string("命名"),
                 "included": .bool(true), "state": .string("failed"), "reason": .string("GitHub 拒绝了（422）")]),
        .object(["id": .string("c3"), "path": .string("src/c.rs"), "line": .number("1"), "body": .string("不发"),
                 "included": .bool(false), "state": .string("draft")]),
    ]),
])

/// D2 step 3 (2026-11-21): the publishing draft under a review.
@MainActor
final class PublishDraftTests: XCTestCase {
    func testADraftReadsAsTheSheetShowsIt() throws {
        let w = try XCTUnwrap(LiveWriteIntent.decode(writeJSON))
        XCTAssertEqual(w.items.map(PublishText.place), ["总评", "src/a.rs:2", "src/b.rs:5", "src/c.rs:1"])
        XCTAssertEqual(w.items.map(PublishText.item), [
            "已发布", "结果不明：发出后没有收到回答，去 GitHub 核对", "失败：GitHub 拒绝了（422）", "不发布",
        ])
        XCTAssertEqual(w.items.map(\.editable), [false, false, true, true])
        XCTAssertEqual(PublishText.state(w), "1 条结果不明：先去 GitHub 核对")
        // A publish now sends only the refused one; resending adds the unknown one.
        XCTAssertEqual(PublishText.due(w).map(\.id), ["c2"])
        XCTAssertEqual(PublishText.due(w, resendUnknown: true).map(\.id), ["c1", "c2"])
        XCTAssertEqual(PublishText.question(w), "发布 1 条评论到 acme/widget#7？")
        let detail = PublishText.detail(w, resendUnknown: true)
        XCTAssertTrue(detail.hasPrefix("针对提交 abcdef1。"), detail)
        XCTAssertTrue(detail.contains("在 GitHub 上没有找到，会再发一次"))
        XCTAssertTrue(detail.contains("· src/a.rs:2：**严重**：b 没有测试 依据：看到了"), detail)
        var moved = w
        moved.state = "targetMoved"
        moved.reason = "PR 在审查之后有了新提交（现在是 bbbbbbb），没有发布；重新审查后再发"
        XCTAssertTrue(PublishText.state(moved).contains("bbbbbbb"))
        moved.state = "partial"
        XCTAssertEqual(PublishText.state(moved), "部分发布：失败的可以再发")
    }

    /// Preparing, editing and publishing each go to the host for the shown
    /// project and read the drafts again; a refusal is said.
    func testTheBookPreparesEditsAndPublishes() async throws {
        let book = PullReviewBook()
        let sent = Sent()
        book.attach(PullReviewBook.Calls(
            pull: { _, _ in nil },
            snapshots: { _ in [] },
            reviewStart: { _, _, _ in },
            reviews: { _ in [LivePullReview(id: "prrev-1", snapshotId: "s", state: "done")] },
            writeDraft: { project, review in await sent.add("draft \(project) \(review)") },
            writes: { _ in [LiveWriteIntent.decode(writeJSON)!] },
            writeEdit: { project, id, item, body, included in
                if body == "坏" { throw Failure() }
                await sent.add("edit \(project) \(id) \(item) \(body ?? "-") \(included.map { "\($0)" } ?? "-")")
            },
            publish: { project, id, resend in await sent.add("publish \(project) \(id) \(resend)") }))
        await book.show(projectID: "p1")
        let review = book.reviews[0]
        let write = try XCTUnwrap(book.write(for: review))
        XCTAssertEqual(write.id, "ghw-1")
        await book.prepare(review)
        await book.edit(write, item: "c3", included: true)
        await book.edit(write, item: "c2", body: "改过的")
        await book.publish(write)
        await book.publish(write, resendUnknown: true)
        let lines = await sent.lines
        XCTAssertEqual(lines, [
            "draft p1 prrev-1", "edit p1 ghw-1 c3 - true", "edit p1 ghw-1 c2 改过的 -",
            "publish p1 ghw-1 false", "publish p1 ghw-1 true",
        ])
        let saved = await book.edit(write, item: "c2", body: "坏")
        XCTAssertFalse(saved)
        XCTAssertTrue(book.errorText?.hasPrefix("保存失败") ?? false)
    }

    private struct Failure: Error {}

    private actor Sent {
        var lines: [String] = []
        func add(_ line: String) { lines.append(line) }
    }
}
