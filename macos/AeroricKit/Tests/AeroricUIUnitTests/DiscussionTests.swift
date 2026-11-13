@testable import AeroricUI
import AeroricProtocol
import XCTest

/// 2026-11-12: the discussion read back from `discussion.get`, and its words.
final class DiscussionTests: XCTestCase {
    private func discussion() -> LiveDiscussion? {
        LiveDiscussion.decode(.object([
            "discussion": .object([
                "id": .string("task-d"), "projectId": .string("p"), "topic": .string("数据增强放哪\n细节"),
                "moderator": .number("0"), "maxRounds": .number("3"), "round": .number("2"),
                "state": .string("running"),
                "members": .array([
                    .object(["taskId": .string("a"), "label": .string("Claude"), "agent": .string("claude"),
                             "angle": .string("关注性能")]),
                    .object(["taskId": .string("b"), "label": .string("Codex"), "agent": .string("codex")]),
                ]),
                "posts": .array([
                    .object(["round": .number("1"), "member": .number("0"), "kind": .string("answer"),
                             "state": .string("done"), "text": .string("A"), "startedMs": .number("1")]),
                    .object(["round": .number("1"), "member": .number("1"), "kind": .string("answer"),
                             "state": .string("done"), "text": .string("B"), "startedMs": .number("1")]),
                    .object(["round": .number("2"), "member": .number("0"), "kind": .string("answer"),
                             "state": .string("done"), "text": .string("A2"), "stance": .string("keep"),
                             "startedMs": .number("2")]),
                    .object(["round": .number("2"), "member": .number("1"), "kind": .string("answer"),
                             "state": .string("waiting"), "waiting": .string("python bench.py"),
                             "startedMs": .number("2")]),
                ]),
            ]),
            "usage": .array([
                .object(["tokens": .number("41000"), "costUsd": .number("0.012"), "durationMs": .number("52000")]),
                .object(["tokens": .number("38000")]),
                .null,
                .null,
            ]),
            "families": .array([.string("claude"), .string("codex")]),
        ]))
    }

    func testTheDiscussionDecodesWithItsUsage() throws {
        let d = try XCTUnwrap(discussion())
        XCTAssertEqual(d.members.map(\.label), ["Claude", "Codex"])
        XCTAssertEqual(d.members[0].family, "claude")
        XCTAssertEqual(d.members[0].angle, "关注性能")
        XCTAssertEqual(d.posts.count, 4)
        XCTAssertEqual(d.posts[2].stance, "keep")
        XCTAssertEqual(d.posts[0].usage?.tokens, 41000)
        XCTAssertEqual(d.posts[0].usage?.costUsd, 0.012)
        XCTAssertNil(d.posts[2].usage)
        XCTAssertEqual(d.posts[3].waiting, "python bench.py")
        XCTAssertNil(LiveDiscussion.decode(.object([:])))
    }

    func testTheWordsOfTheDiscussionPage() throws {
        let d = try XCTUnwrap(discussion())
        XCTAssertEqual(DiscussionText.state(d), "第 2 / 3 轮 · 有成员在等你确认")
        XCTAssertEqual(DiscussionText.rounds(d), [1, 2])
        var retried = d
        retried.posts.append(LiveDiscussion.Post(index: 4, round: 2, member: 1, state: "speaking"))
        XCTAssertEqual(DiscussionText.answers(retried, round: 2).map(\.index), [2, 4])
        XCTAssertEqual(DiscussionText.stance("keep"), "维持")
        XCTAssertEqual(DiscussionText.stance("changed"), "已修改")
        XCTAssertNil(DiscussionText.stance(nil))
        XCTAssertEqual(DiscussionText.usage(d.posts[0].usage), "41K tok · 52 秒 · ≈$0.012")
        XCTAssertEqual(DiscussionText.total(d)?.hasPrefix("合计 79K tok"), true)
        var done = d
        done.state = "done"
        XCTAssertEqual(DiscussionText.state(done), "已总结")
        XCTAssertFalse(DiscussionText.active(done))
    }

    func testADiscussionNeedsAProjectATopicAndTwoToFiveMembers() {
        let one = [LiveDiscussionChoice(configurationId: "c")]
        let two = one + [LiveDiscussionChoice(configurationId: "d", model: "m")]
        XCTAssertEqual(DiscussionText.blocked(topic: "x", projectID: nil, members: two), "先选一个项目")
        XCTAssertEqual(DiscussionText.blocked(topic: "x", projectID: "p", members: one), "讨论至少要 2 个成员")
        XCTAssertNotNil(DiscussionText.blocked(topic: "  ", projectID: "p", members: two))
        XCTAssertNil(DiscussionText.blocked(topic: "x", projectID: "p", members: two))
        let targets = [RerunTarget(id: "d", title: "Codex 登录", agent: "codex", models: ["m"])]
        XCTAssertEqual(DiscussionText.member(two[1], targets: targets), "Codex 登录 · m")
    }

    /// The add panel: harness, then that harness's configurations, then models.
    func testTheAddPanelGoesFromHarnessToConfigurationToModel() {
        let targets = [
            RerunTarget(id: "c1", title: "官方订阅", agent: "codex", models: ["gpt-6.1-sol", "gpt-6.1"]),
            RerunTarget(id: "c2", title: "第三方", agent: "codex", models: []),
            RerunTarget(id: "k1", title: "官方订阅", agent: "claude", models: ["sonnet"]),
        ]
        XCTAssertEqual(DiscussionText.harnesses(targets),
                       ["codex": true, "claude": true, "dsh": false, "omp": false])
        XCTAssertEqual(DiscussionText.startHarness(preferred: "claude", targets: targets), "claude")
        XCTAssertEqual(DiscussionText.startHarness(preferred: "dsh", targets: targets), "codex")
        XCTAssertEqual(DiscussionText.configurations(targets, harness: "codex").map(\.id), ["c1", "c2"])
        XCTAssertEqual(DiscussionText.models(targets[0]), ["gpt-6.1-sol", "gpt-6.1"])
        XCTAssertEqual(DiscussionText.models(targets[1]), [nil])
        let members = [LiveDiscussionChoice(configurationId: "c1", model: "gpt-6.1"),
                       LiveDiscussionChoice(configurationId: "c1", model: "gpt-6.1"),
                       LiveDiscussionChoice(configurationId: "c2")]
        XCTAssertEqual(DiscussionText.added(members, configurationID: "c1", model: "gpt-6.1"), 2)
        XCTAssertEqual(DiscussionText.added(members, configurationID: "c2", model: nil), 1)
        XCTAssertEqual(DiscussionText.added(members, configurationID: "c1", model: "gpt-6.1-sol"), 0)
    }
}
