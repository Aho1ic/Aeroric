import AeroricProtocol
@testable import AeroricUI
import XCTest

/// E8 (2026-11-22): the memory page says how much a new session carries.
final class MemoryLoadTests: XCTestCase {
    func testTheLoadDecodesAndSaysWhatWasLeftOut() throws {
        let load = try XCTUnwrap(LiveMemoryLoad.decode(.object([
            "text": .string("# Aeroric 记忆"), "usedChars": .number("840"), "budgetChars": .number("1200"),
            "carried": .number("8"),
            "leftOut": .array([
                .object(["id": .string("m1"), "kind": .string("fact"), "text": .string("第 5 条项目约定"),
                         "why": .string("budget")]),
                .object(["id": .string("m2"), "kind": .string("fact"), "text": .string("旧约定"),
                         "why": .string("demoted")]),
            ]),
        ])))
        XCTAssertEqual(MemoryLoadText.line(load), "新会话会带入 840 / 1200 字（8 条），另有 2 条没带上")
        XCTAssertEqual(MemoryLoadText.leftOut(load.leftOut[0]), "· 第 5 条项目约定（超出字数上限）")
        XCTAssertEqual(MemoryLoadText.leftOut(load.leftOut[1]), "· 旧约定（已移出开场，只能检索到）")
        XCTAssertNil(LiveMemoryLoad.decode(.object(["text": .null])), "an older host")
        XCTAssertEqual(MemoryLoadText.line(LiveMemoryLoad(budgetChars: 1200)), "新会话不会带入记忆")
    }
}
