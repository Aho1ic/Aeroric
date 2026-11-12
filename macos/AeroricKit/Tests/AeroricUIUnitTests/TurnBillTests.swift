@testable import AeroricUI
import AeroricProtocol
import XCTest

/// R3-1: the bill under each turn, decoded from `session.history`'s turn stats.
final class TurnBillTests: XCTestCase {
    private func decodedTurn() throws -> SessionTranscriptStats.TurnStat {
        let stats = SessionTranscriptStats.decode(.object([
            "turns": .array([.object([
                "startedAt": .number("1000"), "endedAt": .number("11000"), "tokens": .number("12345"),
                "inputTokens": .number("12000"), "cachedInputTokens": .number("9000"),
                "outputTokens": .number("345"), "model": .string("gpt-5.4"), "costUsd": .number("0.0123"),
                "moreTools": .number("2"),
                "tools": .array([
                    .object(["name": .string("exec_command"), "detail": .string("cargo test"),
                             "startedAt": .number("2000"), "endedAt": .number("7000"), "failed": .bool(true)]),
                    .object(["name": .string("Read"), "startedAt": .number("8000"), "endedAt": .number("8200")]),
                    .object(["name": .string("wait"), "startedAt": .number("9000")]),
                ]),
            ])]),
        ]))
        return try XCTUnwrap(stats?.turns.first)
    }

    func testTheTurnsBillDecodesWithItsToolRows() throws {
        let turn = try decodedTurn()
        XCTAssertEqual(turn.inputTokens, 12000)
        XCTAssertEqual(turn.cachedInputTokens, 9000)
        XCTAssertEqual(turn.outputTokens, 345)
        XCTAssertEqual(turn.model, "gpt-5.4")
        XCTAssertEqual(turn.costUsd, 0.0123)
        XCTAssertEqual(turn.moreTools, 2)
        XCTAssertEqual(turn.tools.map(\.name), ["exec_command", "Read", "wait"])
        XCTAssertEqual(turn.tools[0].detail, "cargo test")
        XCTAssertTrue(turn.tools[0].failed)
        XCTAssertNil(turn.tools[2].endedAt)
    }

    func testChipAndCardText() throws {
        var turn = try decodedTurn()
        XCTAssertTrue(TurnBillText.hasBill(turn))
        XCTAssertEqual(TurnBillText.chip(turn), "12.3K tok · ≈$0.012")
        XCTAssertEqual(TurnBillText.hitRate(turn), "75%")
        XCTAssertEqual(TurnBillText.toolsSummary(turn), "5 次 · 1 次失败 · 共 5.2 秒")
        turn.costUsd = nil
        XCTAssertEqual(TurnBillText.chip(turn), "12.3K tok")
        XCTAssertEqual(TurnBillText.money(0.0004), "<$0.001")
        XCTAssertEqual(TurnBillText.money(1.5), "$1.50")
        XCTAssertFalse(TurnBillText.hasBill(SessionTranscriptStats.TurnStat(startedAt: 0, endedAt: 1, tokens: 0)))
    }

    func testWaterfallColoursAndPlacesEachCall() throws {
        let turn = try decodedTurn()
        XCTAssertEqual(TurnBillText.speed(turn.tools[0]), .failed)
        XCTAssertEqual(TurnBillText.speed(turn.tools[1]), .fast)
        XCTAssertEqual(TurnBillText.speed(turn.tools[2]), .unfinished)
        XCTAssertEqual(TurnBillText.speed(.init(name: "x", startedAt: 0, endedAt: 3000)), .medium)
        XCTAssertEqual(TurnBillText.speed(.init(name: "x", startedAt: 0, endedAt: 12000)), .slow)
        let first = TurnBillText.bar(turn.tools[0], turnStart: 1000, turnEnd: 11000)
        XCTAssertEqual(first.start, 0.1, accuracy: 1e-9)
        XCTAssertEqual(first.width, 0.5, accuracy: 1e-9)
        // No result: the bar runs to the turn's end.
        let open = TurnBillText.bar(turn.tools[2], turnStart: 1000, turnEnd: 11000)
        XCTAssertEqual(open.start + open.width, 1, accuracy: 1e-9)
    }

    /// R5-1: a turn that did not run, one the user undid, and one that ran on
    /// after API errors, each told apart; a plain completed turn says nothing.
    func testTheTurnsGradeIsDecodedAndWorded() throws {
        let stats = try XCTUnwrap(SessionTranscriptStats.decode(.object([
            "turns": .array([
                .object(["startedAt": .number("1000"), "endedAt": .number("3700"), "tokens": .number("0"),
                         "failed": .bool(true), "apiErrors": .number("1"),
                         "notRun": .object(["reason": .string("rejected"), "status": .number("400"),
                                            "message": .string("requires reasoning.context")])]),
                .object(["startedAt": .number("5000"), "tokens": .number("10"),
                         "productFailure": .object(["undone": .bool(true)])]),
                .object(["startedAt": .number("9000"), "tokens": .number("10"),
                         "productFailure": .object(["check": .string("测试 3 项失败")])]),
                .object(["startedAt": .number("12000"), "tokens": .number("10"), "apiErrors": .number("2")]),
                .object(["startedAt": .number("15000"), "tokens": .number("10")]),
            ]),
        ])))
        let turns = stats.turns
        XCTAssertEqual(turns[0].notRun, .init(reason: "rejected", status: 400, message: "requires reasoning.context"))
        XCTAssertEqual(TranscriptTurns.header(turns[0]), "没跑成（请求被拒），用时 2.7 秒")
        let notRun = try XCTUnwrap(TurnBillText.outcome(turns[0]))
        XCTAssertEqual(notRun.text, "没跑成 · 请求被拒")
        XCTAssertEqual(notRun.tone, .danger)
        XCTAssertTrue(notRun.help.hasSuffix("HTTP 400 · requires reasoning.context"))
        XCTAssertEqual(TurnBillText.outcome(turns[1])?.text, "产品失败 · 已撤销")
        XCTAssertEqual(TurnBillText.outcome(turns[2])?.text, "产品失败 · 检查没过")
        XCTAssertEqual(TurnBillText.outcome(turns[2])?.help, "这一轮之后，项目检查没过：测试 3 项失败")
        XCTAssertEqual(TurnBillText.outcome(turns[3])?.text, "途中 2 次 API 错误")
        XCTAssertNil(TurnBillText.outcome(turns[4]))
        XCTAssertEqual(TranscriptTurns.header(turns[4]), "已完成")
    }
}
