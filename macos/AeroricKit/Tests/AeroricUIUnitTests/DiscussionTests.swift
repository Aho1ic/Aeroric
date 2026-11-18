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

    /// A4: a harness whose table says it cannot hold itself read-only is greyed
    /// out with the reason; no table (older host) leaves the choice to the host.
    func testAHarnessThatCannotStayReadOnlyIsGreyedOut() {
        let targets = [
            RerunTarget(id: "c1", title: "官方订阅", agent: "codex", models: []),
            RerunTarget(id: "o1", title: "omp", agent: "omp", models: []),
        ]
        let why = "omp 没有能在启动时固定的只读档"
        let tables = [
            "codex": LiveCapabilities([LiveCapabilityEntry(feature: "strictReadOnly", state: "yes",
                                                           note: "Codex 只读沙盒强制")]),
            "omp": LiveCapabilities([LiveCapabilityEntry(feature: "strictReadOnly", state: "no", note: why)]),
        ]
        XCTAssertEqual(DiscussionText.harnesses(targets, capabilities: tables),
                       ["codex": true, "claude": false, "dsh": false, "omp": false])
        XCTAssertEqual(DiscussionText.harnesses(targets)["omp"], true)
        XCTAssertEqual(DiscussionText.startHarness(preferred: "omp", targets: targets, capabilities: tables), "codex")
        XCTAssertEqual(DiscussionText.harnessHelp("omp", targets: targets, capabilities: tables),
                       "\(AgentConfigText.harnessTitle("omp"))：不能参加讨论（\(why)）")
        XCTAssertEqual(DiscussionText.harnessHelp("claude", targets: targets, capabilities: tables),
                       "\(AgentConfigText.harnessTitle("claude"))：没有能启动的配置")
        XCTAssertNil(DiscussionText.readOnlyBlock(tables["codex"]))
        XCTAssertNil(DiscussionText.readOnlyBlock(nil))
        XCTAssertEqual(DiscussionText.readOnlyBlock(
            LiveCapabilities([LiveCapabilityEntry(feature: "strictReadOnly", state: "unknown")])), "不能保证只读")
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

    /// 2026-11-13: the person speaks, asks by name, another round, anonymity.
    func testWhatThePersonSaidAndTheAnswersSitInTheirRound() throws {
        let d = try XCTUnwrap(LiveDiscussion.decode(.object([
            "discussion": .object([
                "id": .string("task-d"), "moderator": .number("0"), "maxRounds": .number("2"),
                "round": .number("2"), "state": .string("done"), "anonymous": .bool(true),
                "members": .array([
                    .object(["taskId": .string("a"), "label": .string("Codex · a")]),
                    .object(["taskId": .string("b"), "label": .string("Codex · ab")]),
                ]),
                "posts": .array([
                    .object(["round": .number("1"), "member": .number("0"), "kind": .string("answer"),
                             "state": .string("done"), "text": .string("A")]),
                    .object(["round": .number("1"), "member": .number("0"), "kind": .string("user"),
                             "state": .string("done"), "text": .string("@Codex · ab 为什么"),
                             "mentions": .array([.number("1")])]),
                    .object(["round": .number("1"), "member": .number("1"), "kind": .string("reply"),
                             "state": .string("done"), "text": .string("因为")]),
                    .object(["round": .number("1"), "member": .number("0"), "kind": .string("summary"),
                             "state": .string("failed")]),
                    .object(["round": .number("1"), "member": .number("0"), "kind": .string("summary"),
                             "state": .string("done"), "text": .string("总结一")]),
                    .object(["round": .number("2"), "member": .number("0"), "kind": .string("user"),
                             "state": .string("done"), "text": .string("再想想")]),
                    .object(["round": .number("2"), "member": .number("1"), "kind": .string("answer"),
                             "state": .string("done"), "text": .string("B2")]),
                ]),
            ]),
        ])))
        XCTAssertTrue(d.anonymous)
        XCTAssertEqual(d.posts[1].mentions, [1])
        XCTAssertEqual(DiscussionText.rounds(d), [1, 2])
        XCTAssertEqual(DiscussionText.exchanges(d, round: 1).map(\.index), [1, 2])
        XCTAssertEqual(DiscussionText.summary(d, round: 1)?.text, "总结一")
        XCTAssertNil(DiscussionText.summary(d, round: 2))
        XCTAssertEqual(DiscussionText.alias(0), "成员 A")
        XCTAssertEqual(DiscussionText.alias(1), "成员 B")

        // The longer name wins; an alias works when anonymous.
        XCTAssertEqual(DiscussionText.mentions(in: "@Codex · ab 说说", d), [1])
        XCTAssertEqual(DiscussionText.mentions(in: "@Codex · a 和 @成员 B", d), [0, 1])
        XCTAssertEqual(DiscussionText.mentions(in: "大家怎么看", d), [])

        XCTAssertTrue(DiscussionText.canSay(d))
        XCTAssertTrue(DiscussionText.canAgain(d))
        XCTAssertEqual(DiscussionText.state(d), "已总结")
        XCTAssertTrue(DiscussionText.sayPlaceholder(d).hasPrefix("说点什么会再开一轮"))

        var replying = d
        replying.posts.append(LiveDiscussion.Post(index: 7, round: 2, member: 0, kind: "reply", state: "speaking"))
        XCTAssertEqual(DiscussionText.state(replying), "已总结 · 有成员在回答你")
        XCTAssertFalse(DiscussionText.canAgain(replying))

        var summing = d
        summing.state = "summarizing"
        XCTAssertFalse(DiscussionText.canSay(summing))
        XCTAssertFalse(DiscussionText.canAgain(summing))
        var running = d
        running.state = "running"
        XCTAssertTrue(DiscussionText.sayPlaceholder(running).hasPrefix("说点什么，下一轮"))
        XCTAssertFalse(DiscussionText.canAgain(running))
    }

    /// 2026-11-13: draft review — the format rule, progress and the final.
    func testADraftReviewReadsAsVersionsReviewsAndAFinal() throws {
        func post(_ round: Int, _ member: Int, kind: String = "answer", stage: String, version: Int,
                  verdict: String? = nil) -> JSONValue {
            var fields: [String: JSONValue] = [
                "round": .number(String(round)), "member": .number(String(member)), "kind": .string(kind),
                "state": .string("done"), "text": .string("v\(version)"), "stage": .string(stage),
                "version": .number(String(version)),
            ]
            if let verdict { fields["verdict"] = .string(verdict) }
            return .object(fields)
        }
        let d = try XCTUnwrap(LiveDiscussion.decode(.object([
            "discussion": .object([
                "id": .string("task-r"), "projectId": .string("p"), "moderator": .number("0"),
                "maxRounds": .number("3"), "round": .number("4"), "state": .string("done"),
                "format": .string("review"), "maxRevisions": .number("2"),
                "members": .array([
                    .object(["taskId": .string("a"), "label": .string("Claude"), "configurationId": .string("cfg-a"),
                             "model": .string("sonnet")]),
                    .object(["taskId": .string("b"), "label": .string("Codex"), "configurationId": .string("cfg-b"),
                             "angle": .string("挑刺")]),
                    .object(["taskId": .string("c"), "label": .string("Kimi"), "configurationId": .string("cfg-c")]),
                ]),
                "posts": .array([
                    post(1, 0, stage: "draft", version: 1),
                    post(2, 1, stage: "review", version: 1, verdict: "pass"),
                    post(2, 2, stage: "review", version: 1, verdict: "revise"),
                    post(3, 0, stage: "revise", version: 2),
                    post(4, 1, stage: "review", version: 2, verdict: "pass"),
                    post(4, 2, stage: "review", version: 2, verdict: "pass"),
                    post(4, 0, kind: "summary", stage: "final", version: 2, verdict: "pass"),
                ]),
            ]),
        ])))
        XCTAssertTrue(DiscussionText.isReview(d))
        XCTAssertEqual(d.maxRevisions, 2)
        XCTAssertEqual(d.posts[2].verdict, "revise")
        XCTAssertEqual(d.posts[6].stage, "final")
        XCTAssertEqual(DiscussionText.state(d), "已定稿")
        XCTAssertEqual(DiscussionText.reviewSteps(d).map(\.title),
                       ["第 1 版 起草", "评审 1/2 通过", "第 2 版 修订", "评审 2/2 通过", "定稿"])
        XCTAssertEqual(DiscussionText.finalTitle(d.posts[6]), "定稿 · 第 2 版 · 全部通过 · 修订 1 次")
        XCTAssertEqual(DiscussionText.roundTitle(d, 1), "第 1 版 · 作者起草")
        XCTAssertEqual(DiscussionText.roundTitle(d, 2), "第 1 版 · 评审（各自独立，互相看不到）")
        XCTAssertEqual(DiscussionText.roundTitle(d, 3), "第 2 版 · 作者修订")
        XCTAssertEqual(DiscussionText.verdict("revise"), "需修改")
        XCTAssertEqual(DiscussionText.choices(d).map(\.configurationId), ["cfg-a", "cfg-b", "cfg-c"])
        XCTAssertEqual(DiscussionText.choices(d)[0].model, "sonnet")
        XCTAssertEqual(DiscussionText.choices(d)[1].angle, "挑刺")

        var reviewing = d
        reviewing.state = "running"
        XCTAssertEqual(DiscussionText.state(reviewing), "第 2 版 · 评审中")
        XCTAssertFalse(DiscussionText.canMention(reviewing))
        reviewing.round = 3
        XCTAssertEqual(DiscussionText.state(reviewing), "第 2 版 · 作者修订中")
        XCTAssertTrue(DiscussionText.canMention(reviewing))
        reviewing.state = "summarizing"
        XCTAssertEqual(DiscussionText.state(reviewing), "写完这一步就定稿")
    }

    func testAutoPicksDraftReviewForWritingTopics() {
        XCTAssertEqual(DiscussionText.recommendFormat("帮我起草一份数据增强规范").format, "review")
        XCTAssertTrue(DiscussionText.recommendFormat("帮我起草一份数据增强规范").reason.contains("「起草」"))
        XCTAssertEqual(DiscussionText.recommendFormat("写一份 README").format, "review")
        XCTAssertEqual(DiscussionText.recommendFormat("Mosaic 该不该关").format, "roundtable")
        XCTAssertEqual(DiscussionText.format(.auto, topic: "改一下提案"), "review")
        XCTAssertEqual(DiscussionText.format(.roundtable, topic: "改一下提案"), "roundtable")
        XCTAssertEqual(DiscussionText.format(.review, topic: "为什么慢"), "review")
    }

    /// 2026-11-13: research — parts, progress, report, only `@` after it.
    func testAResearchReadsAsSplitPartsAndAReport() throws {
        func post(_ round: Int, _ member: Int, kind: String = "answer", stage: String?, state: String = "done") -> JSONValue {
            var fields: [String: JSONValue] = [
                "round": .number(String(round)), "member": .number(String(member)), "kind": .string(kind),
                "state": .string(state), "text": .string("t"),
            ]
            if let stage { fields["stage"] = .string(stage) }
            return .object(fields)
        }
        let d = try XCTUnwrap(LiveDiscussion.decode(.object([
            "discussion": .object([
                "id": .string("task-x"), "moderator": .number("0"), "maxRounds": .number("3"),
                "round": .number("2"), "state": .string("running"), "format": .string("research"),
                "requirements": .string(""),
                "assignments": .array([.string(""), .string("查数据加载"), .string("查 GPU")]),
                "members": .array([
                    .object(["taskId": .string("a"), "label": .string("Claude")]),
                    .object(["taskId": .string("b"), "label": .string("Codex")]),
                    .object(["taskId": .string("c"), "label": .string("Kimi")]),
                ]),
                "posts": .array([
                    post(1, 0, stage: "split"),
                    post(2, 1, stage: "research"),
                    post(2, 2, stage: "research", state: "speaking"),
                ]),
            ]),
        ])))
        XCTAssertTrue(DiscussionText.isResearch(d))
        XCTAssertEqual(d.requirements, "")
        XCTAssertEqual(DiscussionText.assignment(d, member: 1), "查数据加载")
        XCTAssertNil(DiscussionText.assignment(d, member: 0))
        XCTAssertEqual(DiscussionText.state(d), "成员各自调研中")
        XCTAssertEqual(DiscussionText.researchSteps(d).map(\.title), ["拆题", "调研 1/2 查完"])
        XCTAssertEqual(DiscussionText.roundTitle(d, 1), "主持人拆题")
        XCTAssertFalse(DiscussionText.canMention(d))
        XCTAssertFalse(DiscussionText.canAgain(d))

        var reported = d
        reported.posts[2].state = "done"
        reported.posts.append(LiveDiscussion.Post(index: 3, round: 2, member: 0, kind: "summary", text: "报告"))
        reported.state = "done"
        XCTAssertEqual(DiscussionText.state(reported), "已出报告")
        XCTAssertEqual(DiscussionText.researchSteps(reported).map(\.title), ["拆题", "调研 2/2 查完", "报告"])
        XCTAssertTrue(DiscussionText.canMention(reported))
        XCTAssertTrue(DiscussionText.needsMention(reported))
        XCTAssertFalse(DiscussionText.canAgain(reported))
        XCTAssertTrue(DiscussionText.sayPlaceholder(reported).contains("只能 @成员 追问"))
    }

    func testTheFirstKindOfWordInTheTopicPicksTheFormat() {
        XCTAssertEqual(DiscussionText.recommendFormat("为什么训练这么慢").format, "research")
        XCTAssertTrue(DiscussionText.recommendFormat("排查显存泄漏").reason.contains("「排查」"))
        XCTAssertEqual(DiscussionText.recommendFormat("调研之后起草一份方案").format, "research")
        XCTAssertEqual(DiscussionText.recommendFormat("起草一份调研计划").format, "review")
        // 2026-11-16: English topics are read too, as whole words.
        let draft = DiscussionText.recommendFormat("Draft a short paragraph introducing scheduled tasks.")
        XCTAssertEqual(draft.format, "review")
        XCTAssertTrue(draft.reason.contains("「draft」"))
        XCTAssertEqual(DiscussionText.recommendFormat("Research why host tests time out.").format, "research")
        XCTAssertEqual(DiscussionText.recommendFormat("Tabs or spaces for Python code?").format, "roundtable")
        XCTAssertEqual(DiscussionText.recommendFormat("Is it redrafted anywhere?").format, "roundtable")
        XCTAssertEqual(DiscussionText.formatTitle("research"), "分工调研")
        XCTAssertEqual(DiscussionText.requirementPresets.map(\.title), ["简洁讨论", "方案对比", "深入分析"])
        XCTAssertEqual(DiscussionText.defaultRequirements, "观点要具体、说明理由，尽量在 600 字以内。")
    }

    /// B2-5: a relay counts turns, not rounds: never `第 6 / 2 轮`.
    func testARelaySaysWhichLapAndWhichTurn() {
        let members = (1...3).map { LiveDiscussion.Member(taskId: "m\($0)", label: "M\($0)") }
        let relay = LiveDiscussion(id: "d", members: members, maxRounds: 2, round: 5, format: "relay")
        XCTAssertEqual(DiscussionText.formatTitle("relay"), "接力")
        XCTAssertEqual(DiscussionText.state(relay), "第 2 / 2 圈 · 第 2 位 · 讨论中")
        XCTAssertEqual(DiscussionText.roundTitle(relay, 1), "第 1 / 2 圈 · 第 1 位 · 开题")
    }

    /// B2-5: a derived or custom configuration is grouped under the harness it
    /// runs on, not the agent it names.
    func testConfigurationsAreGroupedByTheirHarness() {
        let targets = [
            RerunTarget(id: "claude_gpt55", title: "GPT 5.5 · Claude", agent: "claude_gpt55", models: [],
                        harness: "codex"),
            RerunTarget(id: "claude", title: "Claude Code", agent: "claude", models: []),
        ]
        XCTAssertEqual(DiscussionText.configurations(targets, harness: "codex").map(\.id), ["claude_gpt55"])
        XCTAssertEqual(DiscussionText.harnesses(targets)["codex"], true)
    }

}
