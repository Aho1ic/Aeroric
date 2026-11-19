import AeroricProtocol
@testable import AeroricUI
import XCTest

/// C1 (2026-11-19): 查看原话 — the evidence's place decodes, the lookup's
/// outcome reads as a sentence, and the words are cut out by the host's
/// char (Unicode scalar) offsets.
@MainActor
final class MemoryEvidenceSourceTests: XCTestCase {
    func testTheEvidencePlaceAndTheLookupDecode() throws {
        let evidence = LiveMemoryEvidence.decode(.object([
            "taskId": .string("t1"), "excerpt": .string("以后回复都用简体中文"), "hash": .string("h"),
            "atMs": .number("5"),
            "location": .object(["messageIndex": .number("2"), "role": .string("user"), "start": .number("0"),
                                 "end": .number("10"), "messageHash": .string("m")]),
        ]))
        XCTAssertEqual(evidence?.location, LiveEvidenceLocation(messageIndex: 2, role: "user"))
        XCTAssertNil(LiveMemoryEvidence.decode(.object(["excerpt": .string("旧的")]))?.location)

        let lookup = LiveEvidenceLookup.decode(.object([
            "status": .string("moved"), "taskId": .string("t1"), "excerpt": .string("跑 check"),
            "message": .object(["messageIndex": .number("1"), "role": .string("user"), "text": .string("先跑 check"),
                                "start": .number("1"), "end": .number("8"), "cutBefore": .bool(true)]),
        ]))
        XCTAssertEqual(lookup.status, "moved")
        XCTAssertEqual(lookup.message?.start, 1)
        XCTAssertEqual(lookup.message?.cutBefore, true)
        XCTAssertEqual(lookup.message?.cutAfter, false)
        XCTAssertEqual(MemoryText.lookupStatus(lookup), "来源会话之后改过，原话现在在第 2 条消息里（你说的）")
    }

    func testEachOutcomeSaysWhatBecameOfTheSource() {
        let message = LiveEvidenceMessage(messageIndex: 0, role: "assistant", text: "好的", start: 0, end: 2)
        let say = { (status: String, message: LiveEvidenceMessage?) in
            MemoryText.lookupStatus(LiveEvidenceLookup(status: status, excerpt: "x", message: message))
        }
        XCTAssertEqual(say("located", message), "原话在来源会话的第 1 条消息里（Agent 说的）")
        XCTAssertTrue(say("changed", nil).hasPrefix("原文之后改过，原话已经不在了"))
        XCTAssertTrue(say("gone", nil).hasPrefix("原文已不在"))
        XCTAssertTrue(say("unrecorded", nil).hasPrefix("这条记忆的来源位置没有记录"))
    }

    func testTheWordsAreCutByScalarOffsets() {
        // "👍" is one scalar, as the host counts it; "é" written as e + ◌́
        // is two.
        let text = "好👍，e\u{301}先跑 native:check 再发布"
        let start = Array("好👍，e\u{301}".unicodeScalars).count
        let end = start + Array("先跑 native:check".unicodeScalars).count
        let message = LiveEvidenceMessage(messageIndex: 0, role: "user", text: text, start: start, end: end,
                                          cutAfter: true)
        let parts = MemoryText.split(message)
        XCTAssertEqual(parts.before, "好👍，e\u{301}")
        XCTAssertEqual(parts.words, "先跑 native:check")
        XCTAssertEqual(parts.after, " 再发布…", "the host cut the message after")

        let long = LiveEvidenceMessage(messageIndex: 0, role: "user",
                                       text: String(repeating: "前", count: 900) + "原话" + String(repeating: "后", count: 50),
                                       start: 900, end: 902)
        let shown = MemoryText.split(long, lead: 400, trail: 10)
        XCTAssertEqual(shown.before, "…" + String(repeating: "前", count: 400), "kept near the words")
        XCTAssertEqual(shown.words, "原话")
        XCTAssertEqual(shown.after, String(repeating: "后", count: 10) + "…")

        let wild = LiveEvidenceMessage(messageIndex: 0, role: "user", text: "短", start: 5, end: 9)
        XCTAssertEqual(MemoryText.split(wild).words, "", "offsets past the end do not crash")
    }

    func testOnlyEvidenceFromASessionCanBeLookedUp() {
        let placed = LiveMemoryEvidence(taskId: "t2", excerpt: "b", location: LiveEvidenceLocation(messageIndex: 0, role: "user"))
        let item = LiveMemoryItem(id: "m", text: "x", evidence: [
            LiveMemoryEvidence(excerpt: "/记住 x"),
            LiveMemoryEvidence(taskId: "t1", excerpt: "a"),
            placed,
        ])
        XCTAssertEqual(MemoryText.sourceIndex(item), 2, "the placed one first")
        XCTAssertEqual(MemoryText.sourceIndex(LiveMemoryItem(id: "m", text: "x", evidence: [
            LiveMemoryEvidence(excerpt: "/记住 x"), LiveMemoryEvidence(taskId: "t1", excerpt: "a"),
        ])), 1, "else the first from a session (it says it was not recorded)")
        XCTAssertNil(MemoryText.sourceIndex(LiveMemoryItem(id: "m", text: "x", evidence: [
            LiveMemoryEvidence(excerpt: "/记住 x"),
        ])))
    }
}
