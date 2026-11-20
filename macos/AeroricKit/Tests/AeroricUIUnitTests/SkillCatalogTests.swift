import AeroricProtocol
@testable import AeroricUI
import XCTest

/// C7 (2026-11-20): the catalog and 会话信息's mounted skills.
@MainActor
final class SkillCatalogTests: XCTestCase {
    private func definition(_ id: String, _ source: String, version: String = "", hash: String = "abcdef123456",
                            active: Bool = true, note: String? = nil, summary: String = "") -> JSONValue {
        var f: [String: JSONValue] = [
            "id": .string(id), "name": .string(id), "source": .string(source), "version": .string(version),
            "contentHash": .string(hash), "embedded": .bool(source == "builtin"), "summary": .string(summary),
            "harnesses": .array([.string("claude"), .string("codex")]), "active": .bool(active),
        ]
        if let note { f["note"] = .string(note) }
        if source != "builtin" { f["path"] = .string("/tmp/\(id)") }
        return .object(f)
    }

    private func mounted(_ id: String, _ source: String, _ reason: String, hash: String) -> JSONValue {
        .object(["skillId": .string(id), "source": .string(source), "version": .string(""),
                 "contentHash": .string(hash), "reason": .string(reason)])
    }

    /// The catalog decodes, groups by source (内置 first), filters by the
    /// search words, and a name in two sources stays two rows.
    func testTheCatalogGroupsBySourceAndSearches() async {
        let book = SkillCatalogBook()
        var asked: [String] = []
        book.attach { method, params in
            asked.append("\(method) \(params["projectId"]?.stringValue ?? "-")")
            return .object(["skills": .array([
                self.definition("lint", "plugin", version: "1.2.0"),
                self.definition("brainstorming", "user", summary: "我改过的头脑风暴"),
                self.definition("brainstorming", "builtin", version: "随应用内置",
                                note: "在 Claude 里被你自己的同名技能盖过"),
                self.definition("csv-bom", "learned", version: "v2", active: false, note: "被同名的内置技能盖过"),
            ])])
        }
        book.projectID = "p1"
        await book.load()
        XCTAssertEqual(asked, ["skills.catalog p1"])
        XCTAssertEqual(book.groups.map(\.source), ["builtin", "learned", "user", "plugin"])
        XCTAssertEqual(book.skills.filter { $0.skillId == "brainstorming" }.count, 2)
        XCTAssertEqual(Set(book.skills.map(\.id)).count, 4, "each row keeps its own identity")
        let builtin = book.groups[0].skills[0]
        XCTAssertTrue(builtin.embedded)
        XCTAssertNil(builtin.path)
        XCTAssertEqual(SkillText.version(builtin.version, hash: builtin.contentHash), "随应用内置")
        XCTAssertEqual(SkillText.version("1.2.0", hash: "abcdef123456"), "1.2.0 · abcdef")
        XCTAssertEqual(SkillText.version("", hash: "abcdef123456"), "abcdef")
        XCTAssertFalse(book.groups[1].skills[0].active)

        book.query = "头脑"
        XCTAssertEqual(book.groups.map(\.source), ["user"])
        book.query = "nothing-like-this"
        XCTAssertTrue(book.groups.isEmpty)

        book.attach { _, _ in throw LiveSessionError.unsupported("断开") }
        await book.load()
        XCTAssertTrue(book.errorText?.hasPrefix("读取技能目录失败：") ?? false)
        XCTAssertEqual(book.skills.count, 4, "what was read stays")
    }

    /// 会话信息 says how many skills the last launch had, what changed from
    /// the launch before, and 未记录 for a session from before C7.
    func testASessionsLaunchesSayWhatTheyHad() async {
        let book = SkillCatalogBook()
        book.attach { _, params in
            guard params["taskId"]?.stringValue == "t1" else { return .object(["launches": .array([])]) }
            return .object(["launches": .array([
                .object(["atMs": .number("1000"), "harness": .string("claude"), "skills": .array([
                    self.mounted("brainstorming", "builtin", "default", hash: "aaaaaa"),
                    self.mounted("csv-bom", "learned", "accepted", hash: "111111"),
                    self.mounted("old-one", "user", "userFolder", hash: "999999"),
                ])]),
                .object(["atMs": .number("2000"), "harness": .string("claude"), "skills": .array([
                    self.mounted("brainstorming", "builtin", "default", hash: "aaaaaa"),
                    self.mounted("csv-bom", "learned", "accepted", hash: "222222"),
                    self.mounted("tools:lint", "plugin", "plugin", hash: "333333"),
                ])]),
            ])])
        }
        XCTAssertEqual(SkillText.launchSummary(nil), "读取中…")
        await book.loadMounts(taskID: "t1")
        await book.loadMounts(taskID: "t0")
        let launches = book.mounts["t1"]!
        XCTAssertEqual(launches.count, 2)
        XCTAssertEqual(launches[1].skills[1].reason, "accepted")
        XCTAssertTrue(SkillText.launchSummary(launches).hasPrefix("3 个 · "))
        XCTAssertTrue(SkillText.launchSummary(launches).hasSuffix(" 启动 · 共启动 2 次"))
        XCTAssertEqual(SkillText.launchSummary(book.mounts["t0"]), "未记录（这个会话在记录技能之前启动）")
        XCTAssertEqual(SkillText.changes(from: launches[0], to: launches[1]),
                       ["csv-bom 换了版本（111111 → 222222）", "新增 tools:lint", "去掉 old-one"])
        XCTAssertEqual(SkillText.mounted(launches[0].skills[0]), "brainstorming · 默认挂载 · aaaaaa")
        XCTAssertEqual(SkillText.reason("userFolder"), "你自己的目录")
        XCTAssertEqual(SkillText.source("plugin"), "Claude Code 插件")
        XCTAssertEqual(SkillText.harness("dsh"), "DSH")
    }

    /// C8 (2026-11-20): a launch's tools decode from `session.capabilities`;
    /// 会话信息 says how many of Aeroric's are offered, which are not and
    /// why, or that none were injected and why; an old session says 未记录.
    func testALaunchsToolsSayWhatIsOfferedAndWhy() async {
        func tool(_ name: String, _ owner: String, _ group: String, _ scope: String, ready: Bool, basis: String) -> JSONValue {
            .object(["name": .string(name), "owner": .string(owner), "group": .string(group),
                     "scope": .string(scope), "ready": .bool(ready), "basis": .string(basis)])
        }
        let book = SkillCatalogBook()
        var asked: [String] = []
        book.attach { method, params in
            asked.append("\(method) \(params["family"]?.stringValue ?? "-") \(params["taskId"]?.stringValue ?? "-")")
            switch params["taskId"]?.stringValue {
            case "t1":
                return .object(["capabilities": .object([:]), "tools": .object([
                    "atMs": .number("5"), "harness": .string("claude"), "injected": .bool(true),
                    "tools": .array([
                        tool("delegate", "aeroric", "base", "writes", ready: true, basis: "Aeroric 工具，所有接入的会话都有"),
                        tool("memory_recall", "aeroric", "memory", "readOnly", ready: false, basis: "这个项目没有开启记忆"),
                        tool("memory_propose", "aeroric", "memory", "writes", ready: false, basis: "这个项目没有开启记忆"),
                        tool("ask_expert", "aeroric", "expert", "sendsOut", ready: false, basis: "还没有设置专家模型"),
                        tool("Bash", "harness", "harness", "writes", ready: true, basis: "Claude 自带"),
                    ]),
                ])])
            case "t2":
                return .object(["tools": .object([
                    "harness": .string("omp"), "injected": .bool(false),
                    "reason": .string("OMP 还不能在启动时接入 Aeroric 工具"),
                    "tools": .array([tool("delegate", "aeroric", "base", "writes", ready: false,
                                          basis: "OMP 还不能在启动时接入 Aeroric 工具")]),
                ])])
            default:
                return .object(["capabilities": .object([:])])
            }
        }
        XCTAssertEqual(ToolText.summary(nil), "读取中…")
        await book.loadTools(taskID: "t1", family: "claude")
        await book.loadTools(taskID: "t2", family: "omp")
        await book.loadTools(taskID: "t0", family: "codex")
        XCTAssertEqual(asked, ["session.capabilities claude t1", "session.capabilities omp t2",
                               "session.capabilities codex t0"])
        let first = book.tools["t1"]!!
        XCTAssertEqual(first.tools.count, 5)
        XCTAssertEqual(ToolText.summary(book.tools["t1"]), "Aeroric 工具 1/4 · Claude 自带 1 个")
        XCTAssertEqual(ToolText.missing(first),
                       ["memory_recall、memory_propose：这个项目没有开启记忆", "ask_expert：还没有设置专家模型"])
        XCTAssertEqual(ToolText.line(first.tools[0]), "delegate · 会写")
        XCTAssertEqual(ToolText.scope("sendsOut"), "会对外发送")
        XCTAssertEqual(ToolText.summary(book.tools["t2"]), "没有 Aeroric 工具：OMP 还不能在启动时接入 Aeroric 工具")
        XCTAssertTrue(ToolText.missing(book.tools["t2"]!!).isEmpty, "the reason says it once")
        XCTAssertEqual(ToolText.summary(book.tools["t0"]), "未记录（这个会话在记录工具之前启动）")
    }

    /// H1 (2026-11-20): the same `session.capabilities` reply carries the
    /// launch's surfaces; 会话信息's 入口 counts the connected ones and
    /// gives each surface its state and why; an old session says 未记录.
    func testALaunchsSurfacesSayWhichAreConnectedAndWhy() async {
        func entry(_ feature: String, _ state: String, _ note: String?) -> JSONValue {
            var f: [String: JSONValue] = ["feature": .string(feature), "state": .string(state)]
            if let note { f["note"] = .string(note) }
            return .object(f)
        }
        let book = SkillCatalogBook()
        var asked = 0
        book.attach { _, params in
            asked += 1
            guard params["taskId"]?.stringValue == "t1" else { return .object(["capabilities": .object([:])]) }
            return .object(["surfaces": .object([
                "atMs": .number("5"), "harness": .string("omp"), "place": .string("local"),
                "surfaces": .array([
                    entry("surfaceInstructions", "no", "这家没有系统层入口：交接简报放在了第一条消息前，记忆档案没给"),
                    entry("contextTools", "no", "OMP 还不能在启动时接入 Aeroric 工具"),
                    entry("surfaceSkills", "yes", "这次挂了 3 个 Aeroric 技能"),
                    entry("surfaceEvents", "no", "还没接：OMP 可以按次传 --hook，Aeroric 没传（H4）"),
                    entry("surfaceSubagents", "yes", "启动时订阅子代理事件"),
                    entry("surfaceCli", "no", "命令行入口（aeroric-cli ctx）还没做（H2）"),
                ]),
            ])])
        }
        XCTAssertEqual(SurfaceText.summary(nil), "读取中…")
        await book.loadTools(taskID: "t1", family: "omp")
        await book.loadTools(taskID: "t0", family: "codex")
        XCTAssertEqual(asked, 2, "one reply carries tools and surfaces")
        let launch = book.surfaces["t1"]!!
        XCTAssertEqual(launch.harness, "omp")
        XCTAssertEqual(launch.surfaces.map(\.feature), ["surfaceInstructions", "contextTools", "surfaceSkills",
                                                        "surfaceEvents", "surfaceSubagents", "surfaceCli"])
        XCTAssertEqual(SurfaceText.summary(book.surfaces["t1"]), "接通 2/6：技能、子代理")
        XCTAssertEqual(SurfaceText.line(launch.surfaces[2]), "技能 · 已接通 · 这次挂了 3 个 Aeroric 技能")
        XCTAssertEqual(SurfaceText.line(launch.surfaces[1]), "工具 · 没有 · OMP 还不能在启动时接入 Aeroric 工具")
        XCTAssertEqual(SurfaceText.line(LiveCapabilityEntry(feature: "surfaceEvents", state: "unknown")), "事件 · 还没核对")
        XCTAssertEqual(SurfaceText.summary(book.surfaces["t0"]), "未记录（这个会话在记录入口之前启动）")
        XCTAssertEqual(SurfaceText.summary(.some(LiveSurfaceLaunch(harness: "codex", surfaces: [
            LiveCapabilityEntry(feature: "surfaceCli", state: "no", note: "x"),
        ]))), "接通 0/1")
    }
}
