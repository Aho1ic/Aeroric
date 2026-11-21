import AeroricProtocol
@testable import AeroricUI
import XCTest

private let head = "abcdef1234567890abcdef1234567890abcdef12"

private let snapshotJSON: JSONValue = .object([
    "id": .string("42-7-abcdef123456"), "projectId": .string("p1"),
    "repo": .object(["host": .string("github.com"), "owner": .string("acme"), "name": .string("widget"),
                     "repoId": .number("42")]),
    "number": .number("7"), "title": .string("解析器支持注释"), "author": .string("dev"),
    "state": .string("open"), "url": .string("https://github.com/acme/widget/pull/7"),
    "headRef": .string("comments"), "baseRef": .string("main"), "headSha": .string(head),
    "baseSha": .string("b"), "fetchedMs": .number("1763700000000"),
    "files": .array([
        .object(["path": .string("src/a.rs"), "status": .string("modified"), "additions": .number("3"),
                 "deletions": .number("1")]),
        .object(["path": .string("src/b.rs"), "status": .string("added"), "additions": .number("10"),
                 "deletions": .number("0")]),
    ]),
    "checks": .array([
        .object(["name": .string("ci"), "status": .string("completed"), "conclusion": .string("failure")]),
        .object(["name": .string("lint"), "status": .string("in_progress")]),
        .object(["name": .string("docs"), "status": .string("completed"), "conclusion": .string("success")]),
    ]),
    "comments": .array([
        .object(["author": .string("lead"), "body": .string("整体可以")]),
        .object(["author": .string("lead"), "body": .string("旧意见"), "outdated": .bool(true),
                 "anchor": .object(["path": .string("src/a.rs"), "side": .string("RIGHT"), "line": .number("9"),
                                    "commitSha": .string("old")])]),
    ]),
    "truncated": .bool(true), "superseded": .bool(false),
])

private let reviewJSON: JSONValue = .object([
    "id": .string("prrev-1"), "projectId": .string("p1"), "snapshotId": .string("42-7-abcdef123456"),
    "headSha": .string(head), "taskId": .string("task-9"), "state": .string("done"),
    "findings": .array([
        .object(["path": .string("src/a.rs"), "line": .number("2"), "severity": .string("high"),
                 "body": .string("b 没有测试"), "basis": .string("src/a.rs:2 新增")]),
        .object(["path": .string("src/b.rs"), "severity": .string("low"), "body": .string("命名")]),
    ]),
    "dropped": .number("1"), "createdMs": .number("5"), "outdated": .bool(true),
])

/// D1 step 3 (2026-11-21): Git 侧栏 → PR 审查.
@MainActor
final class PullReviewTests: XCTestCase {
    func testASnapshotAndAReviewReadAsTheSectionShowsThem() throws {
        let s = try XCTUnwrap(LivePullSnapshot.decode(snapshotJSON))
        XCTAssertEqual(PullText.title(s), "#7 解析器支持注释")
        XCTAssertEqual(PullText.branches(s), "acme/widget · comments → main")
        XCTAssertTrue(PullText.pinned(s).hasSuffix("对应提交 abcdef1"))
        XCTAssertEqual(PullText.files(s), "2 个文件 · +13 −1 · 列表不完整")
        XCTAssertEqual(PullText.checks(s), "检查：1 失败 · 1 进行中 · 1 通过")
        XCTAssertEqual(s.checks.map(PullText.check), ["ci：失败", "lint：进行中", "docs：通过"])
        XCTAssertEqual(s.comments.map(PullText.comment), ["整个 PR · @lead", "src/a.rs:9 · @lead（针对旧版本）"])
        XCTAssertNil(PullText.checks(LivePullSnapshot(id: "x", number: 1, title: "", headSha: "h")))

        let r = try XCTUnwrap(LivePullReview.decode(reviewJSON))
        XCTAssertEqual(PullText.review(r), "2 条结论 · 另有 1 条不在这个 PR 里，没收 · 针对旧版本")
        XCTAssertEqual(r.findings.map(PullText.place), ["src/a.rs:2", "src/b.rs"])
        XCTAssertEqual(r.findings.map { PullText.severity($0.severity) }, ["高", "低"])
        XCTAssertEqual(PullText.checkoutPath(r.findings[0], snapshotID: s.id),
                       ".aeroric/pr-reviews/42-7-abcdef123456/src/a.rs")
        XCTAssertEqual(PullText.review(LivePullReview(id: "a", snapshotId: "x", state: "running")), "审查中…")
        XCTAssertEqual(PullText.review(LivePullReview(id: "a", snapshotId: "x", state: "done")), "没有发现问题")
        XCTAssertEqual(PullText.review(LivePullReview(id: "a", snapshotId: "x", state: "failed", reason: "审查会话没有回答")),
                       "没有结论：审查会话没有回答")
    }

    /// Pinning and starting a review go to the host for the shown project
    /// and read the list again; a refusal says what failed and keeps the list.
    func testTheBookPinsReviewsAndSaysWhatFailed() async throws {
        let book = PullReviewBook()
        let sent = Sent()
        book.attach(PullReviewBook.Calls(
            pull: { project, link in
                if link.contains("bad") { throw Failure() }
                await sent.add("pull \(project) \(link)")
                return LivePullSnapshot.decode(snapshotJSON)
            },
            snapshots: { project in
                await sent.add("snapshots \(project)")
                return [LivePullSnapshot.decode(snapshotJSON)!]
            },
            reviewStart: { project, snapshot, agent in await sent.add("review \(project) \(snapshot) \(agent)") },
            reviews: { _ in [LivePullReview.decode(reviewJSON)!] }))
        let pinnedBeforeShow = await book.pin("acme/widget#7")
        XCTAssertFalse(pinnedBeforeShow, "no project shown yet")
        await book.show(projectID: "p1")
        XCTAssertEqual(book.snapshots.count, 1)
        let pinned = await book.pin("  https://github.com/acme/widget/pull/7  ")
        XCTAssertTrue(pinned)
        let snapshot = book.snapshots[0]
        let started = await book.startReview(snapshot, agent: "codex")
        XCTAssertTrue(started)
        XCTAssertEqual(book.reviews(of: snapshot).count, 1)
        let lines = await sent.lines
        XCTAssertEqual(lines, ["snapshots p1", "pull p1 https://github.com/acme/widget/pull/7", "snapshots p1",
                               "review p1 42-7-abcdef123456 codex", "snapshots p1"])
        let failed = await book.pin("bad link")
        XCTAssertFalse(failed)
        XCTAssertTrue(book.errorText?.hasPrefix("固定快照失败") ?? false)
        XCTAssertEqual(book.snapshots.count, 1)
        XCTAssertNil(book.working)
    }

    private struct Failure: Error {}

    private actor Sent {
        var lines: [String] = []
        func add(_ line: String) { lines.append(line) }
    }
}
