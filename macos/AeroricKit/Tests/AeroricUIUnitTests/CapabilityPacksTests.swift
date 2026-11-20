import AeroricProtocol
@testable import AeroricUI
import XCTest

private let packsListed: JSONValue = .object([
    "packs": .array([.object([
        "pack": .object([
            "id": .string("github"), "name": .string("github"), "confirmed": .bool(true), "enabled": .bool(true),
            "source": .object(["kind": .string("local"), "origin": .string("aeroric")]),
            "mcpServers": .array([.object([
                "name": .string("github"), "command": .string("gh-mcp"), "args": .array([.string("--stdio")]),
                "env": .object(["GH_TOKEN": .string(LivePackServer.mask), "GH_HOST": .string("github.com")]),
            ])]),
        ]),
        "trusted": .bool(true),
        "places": .array([
            .object(["place": .string("Claude"), "items": .array([.object([
                "kind": .string("mcpServer"), "name": .string("github"), "ready": .bool(true),
                "basis": .string("已写进本次启动的 MCP 配置（--mcp-config）"),
            ])])]),
            .object(["place": .string("DSH"), "items": .array([.object([
                "kind": .string("hook"), "name": .string("sessionStart"), "ready": .bool(false),
                "basis": .string("DSH 没有事件处理的入口"),
            ])])]),
        ]),
    ])]),
    "broken": .array([.object(["folder": .string("bad"), "reason": .string("pack.json 不是有效的能力包清单")])]),
])

/// H6 step 3 (2026-11-21): the 能力包 page.
@MainActor
final class CapabilityPacksTests: XCTestCase {
    func testTheListReadsAsThePageShowsIt() {
        let overview = LivePackOverview.decode(packsListed)
        XCTAssertEqual(overview.packs.count, 1)
        let view = overview.packs[0]
        XCTAssertTrue(view.trusted)
        XCTAssertTrue(view.pack.isOwnServer)
        XCTAssertEqual(view.pack.servers[0].args, ["--stdio"])
        XCTAssertEqual(PackText.contents(view.pack), "1 个 MCP 服务器")
        XCTAssertEqual(PackText.source(view.pack), "你在 Aeroric 里添加的")
        XCTAssertEqual(view.places.map(PackText.place), ["Claude：1/1 项可用", "DSH：0/1 项可用"])
        XCTAssertEqual(PackText.item(view.places[1].items[0]), "事件处理 sessionStart：DSH 没有事件处理的入口")
        XCTAssertEqual(overview.broken.first?.folder, "bad")

        let imported = LivePack(id: "team", name: "团队包", sourceKind: "local", sourceOrigin: "/Users/me/team",
                                revision: "r1", confirmed: false, skills: ["a", "b"], commands: ["c"])
        XCTAssertEqual(PackText.source(imported), "从 /Users/me/team 导入 · 版本 r1")
        XCTAssertEqual(PackText.contents(imported), "2 个技能 · 1 个命令")
        XCTAssertFalse(imported.isOwnServer)
        XCTAssertEqual(PackText.contents(LivePack(id: "e", name: "e")), "空包")
    }

    /// The form reads args by spaces and env by lines; an edit keeps masked values as they are.
    func testTheServerFormMakesAServer() {
        var draft = PackServerDraft()
        XCTAssertFalse(draft.ready)
        draft.name = " github "
        draft.command = " npx "
        draft.args = "-y  @modelcontextprotocol/server-github"
        draft.env = "GH_TOKEN=abc=def\n\nnot a pair\n GH_HOST = github.com"
        XCTAssertTrue(draft.ready)
        let server = draft.server
        XCTAssertEqual(server.name, "github")
        XCTAssertEqual(server.command, "npx")
        XCTAssertEqual(server.args, ["-y", "@modelcontextprotocol/server-github"])
        XCTAssertEqual(server.env, ["GH_TOKEN": "abc=def", "GH_HOST": " github.com"])

        let editing = PackServerDraft(LivePackOverview.decode(packsListed).packs[0].pack.servers[0])
        XCTAssertTrue(editing.editing)
        XCTAssertEqual(editing.env, "GH_HOST=github.com\nGH_TOKEN=\(LivePackServer.mask)")
        XCTAssertEqual(editing.server.env["GH_TOKEN"], LivePackServer.mask, "sent back masked, kept by the host")
        XCTAssertEqual(server.json["args"]?.arrayValue?.count, 2)
    }

    /// A change reads the packs again; a failure says what failed and keeps the page.
    func testTheBookReadsAgainAfterAChangeAndSaysWhatFailed() async {
        let book = PackBook()
        let calls = Counter()
        book.attach(PackBook.Calls(
            list: { await calls.bump(); return LivePackOverview.decode(packsListed) },
            saveServer: { _ in },
            importFolder: { _ in throw Failure() },
            setEnabled: { _, _ in }, setConfirmed: { _, _ in }, remove: { _ in }))
        await book.refresh()
        XCTAssertTrue(book.loaded)
        let saved = await book.save(LivePackServer(name: "x", command: "y"))
        XCTAssertTrue(saved)
        let lists = await calls.count
        XCTAssertEqual(lists, 2)
        let imported = await book.importFolder("/tmp/x")
        XCTAssertFalse(imported)
        XCTAssertTrue(book.errorText?.hasPrefix("导入失败") ?? false)
        XCTAssertEqual(book.overview.packs.count, 1)
    }

    private struct Failure: Error {}

    private actor Counter {
        var count = 0
        func bump() { count += 1 }
    }
}
