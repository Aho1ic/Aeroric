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
}
