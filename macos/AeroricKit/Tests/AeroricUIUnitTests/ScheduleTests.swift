import AeroricProtocol
@testable import AeroricUI
import XCTest

@MainActor
private final class FakeScheduleService: ScheduleService {
    var rules: [LiveScheduleRule] = []
    var calls: [String] = []
    var refuse: String?

    func list() async throws -> [LiveScheduleRule] { rules }
    func save(_ rule: LiveScheduleRule) async throws -> LiveScheduleRule {
        if let refuse { throw NSError(domain: "x", code: 1, userInfo: [NSLocalizedDescriptionKey: refuse]) }
        calls.append("save \(rule.name) \(rule.enabled)")
        var saved = rule
        if saved.id.isEmpty { saved.id = "sched-1" }
        rules.removeAll { $0.id == saved.id }
        rules.append(saved)
        return saved
    }
    func delete(id: String) async throws {
        calls.append("delete \(id)")
        rules.removeAll { $0.id == id }
    }
    func runNow(id: String) async throws -> LiveScheduleRun {
        calls.append("runNow \(id)")
        return LiveScheduleRun(id: "r1", ruleId: id, outcome: "checkFailed", manual: true)
    }
    func runs(id: String) async throws -> [LiveScheduleRun] {
        [LiveScheduleRun(id: "r1", ruleId: id, outcome: "checkFailed", detail: "退出码 1", manual: true)]
    }
}

@MainActor
final class ScheduleTests: XCTestCase {
    /// A12 (2026-11-14): a rule round-trips the wire, every kind of repeat.
    func testARuleRoundTripsWithEveryKindOfRepeat() {
        let repeats: [LiveScheduleRepeat] = [
            .once(atMs: 1_800_000_000_000), .daily(minute: 540),
            .weekly(weekdays: [1, 3], minute: 1110), .every(hours: 6, fromMs: 42),
        ]
        for repeatRule in repeats {
            let rule = LiveScheduleRule(id: "s", name: "早报", projectId: "p", agent: "codex", model: "gpt",
                                        prompt: "看看", repeatRule: repeatRule, check: "git log", enabled: true)
            XCTAssertEqual(LiveScheduleRule.decode(rule.json), rule)
        }
        let blank = LiveScheduleRule(check: "  ").json
        XCTAssertNil(blank.objectValue?["check"], "a blank check is no check")
        let run = LiveScheduleRun.decode(.object([
            "id": .string("r"), "ruleId": .string("s"), "dueMs": .number("1"), "atMs": .number("2"),
            "outcome": .string("busy"), "taskId": .string("task-1"), "catchUp": .bool(true),
        ]))
        XCTAssertEqual(run?.outcome, "busy")
        XCTAssertEqual(run?.catchUp, true)
    }

    /// The page's words: repeat, next time and what a run did.
    func testTheWordsSayWhenAndWhatHappened() {
        XCTAssertEqual(ScheduleText.repeatText(.daily(minute: 9 * 60)), "每天 09:00")
        XCTAssertEqual(ScheduleText.repeatText(.weekly(weekdays: [3, 1], minute: 18 * 60 + 30)), "每周一、三 18:30")
        XCTAssertEqual(ScheduleText.repeatText(.every(hours: 6, fromMs: 0)), "每 6 小时")
        XCTAssertEqual(ScheduleText.repeatText(.every(hours: 24, fromMs: 0)), "每 1 天")
        XCTAssertEqual(ScheduleText.next(LiveScheduleRule(enabled: false)), "已停用")
        XCTAssertEqual(ScheduleText.next(LiveScheduleRule(enabled: true)), "不会再运行")
        XCTAssertEqual(ScheduleText.outcome(LiveScheduleRun(id: "r", outcome: "checkFailed")), "检查没过，没唤醒模型")
        XCTAssertEqual(ScheduleText.outcome(LiveScheduleRun(id: "r", outcome: "busy", catchUp: true)), "补跑 · 上一次还在跑，跳过")
        XCTAssertEqual(ScheduleText.outcome(LiveScheduleRun(id: "r", outcome: "started", manual: true)), "立即运行 · 已开会话")
    }

    /// E16 (2026-11-21): the structure round-trips; a run says how its
    /// result came out, and a result that did not fit counts as failed.
    func testAResultStructureRoundTripsAndRunsSayHowTheResultCameOut() throws {
        let schema = try XCTUnwrap(try? ScheduleText.schema(from: "{\"type\": \"object\", \"required\": [\"ok\"]}").get())
        var rule = LiveScheduleRule(id: "s", name: "巡检", projectId: "p", agent: "codex", prompt: "查")
        rule.resultSchema = schema
        XCTAssertEqual(LiveScheduleRule.decode(rule.json)?.resultSchema, schema)
        XCTAssertNil(LiveScheduleRule(id: "s").json.objectValue?["resultSchema"])
        XCTAssertEqual(try ScheduleText.schema(from: "  ").get(), nil)
        XCTAssertThrowsError(try ScheduleText.schema(from: "[1]").get())
        XCTAssertThrowsError(try ScheduleText.schema(from: "{type: object}").get())
        XCTAssertTrue(ScheduleText.pretty(schema).contains("\n"))

        let decoded = try XCTUnwrap(LiveScheduleRun.decode(.object([
            "id": .string("r"), "outcome": .string("started"), "resultExpected": .bool(true),
            "result": .object(["ok": .bool(true)]),
        ])))
        XCTAssertEqual(ScheduleText.outcome(decoded), "已开会话 · 结果合格")
        XCTAssertEqual(ScheduleText.detail(decoded), "{\"ok\":true}")
        XCTAssertFalse(ScheduleText.failed(decoded))
        var bad = decoded
        bad.result = nil
        bad.resultError = "结果不符合结构：$ 缺少字段 ok"
        XCTAssertEqual(ScheduleText.outcome(bad), "已开会话 · 结果不合格")
        XCTAssertEqual(ScheduleText.detail(bad), "结果不符合结构：$ 缺少字段 ok")
        XCTAssertTrue(ScheduleText.failed(bad))
        bad.resultError = nil
        XCTAssertEqual(ScheduleText.outcome(bad), "已开会话 · 等结果")
    }

    /// 第二批 (2026-11-14): when busy and the hours round-trip; a rule held
    /// by them says so; the runs say what happened.
    func testWhenBusyAndActiveHoursRoundTripAndAreShown() {
        var rule = LiveScheduleRule(id: "sched-1", name: "x", projectId: "p", agent: "codex", prompt: "y")
        rule.whenBusy = "after"
        rule.activeHours = LiveActiveHours(start: 22 * 60, end: 6 * 60)
        let json = rule.json
        XCTAssertEqual(json.objectValue?["whenBusy"]?.stringValue, "after")
        let back = LiveScheduleRule.decode(json)
        XCTAssertEqual(back?.whenBusy, "after")
        XCTAssertEqual(back?.activeHours, LiveActiveHours(start: 22 * 60, end: 6 * 60))
        XCTAssertNil(LiveScheduleRule(id: "s", prompt: "y").json.objectValue?["whenBusy"], "skip is the default")
        XCTAssertEqual(ScheduleText.hours(LiveActiveHours(start: 9 * 60, end: 18 * 60)), "09:00–18:00")

        var held = LiveScheduleRule(enabled: true, nextMs: 5)
        held.busyPending = true
        XCTAssertEqual(ScheduleText.next(held), "上一次还在跑 · 结束后补跑")
        held.busyPending = false
        held.hoursRetryMs = 1_800_000_060_000
        XCTAssertEqual(ScheduleText.next(held), "不在活跃时段 · \(ScheduleText.date(1_800_000_060_000)) 补跑")
        XCTAssertEqual(ScheduleText.outcome(LiveScheduleRun(id: "r", outcome: "queued")), "上一次还在跑，结束后补跑")
        XCTAssertEqual(ScheduleText.outcome(LiveScheduleRun(id: "r", outcome: "outsideHours", catchUp: false)),
                       "不在活跃时段，推迟")
        let decoded = LiveScheduleRule.decode(.object([
            "id": .string("s"), "repeat": .object(["kind": .string("daily"), "minute": .number("540")]),
            "enabled": .bool(true), "busyPending": .bool(true), "hoursRetryMs": .number("7"),
        ]))
        XCTAssertEqual(decoded?.busyPending, true)
        XCTAssertEqual(decoded?.hoursRetryMs, 7)
    }

    /// 常用模板: eight, each named with a prompt; a pre-check where it helps.
    func testTheTemplatesAreComplete() {
        XCTAssertEqual(ScheduleTemplate.all.count, 8)
        XCTAssertEqual(Set(ScheduleTemplate.all.map(\.id)).count, 8)
        for template in ScheduleTemplate.all {
            XCTAssertFalse(template.name.isEmpty)
            XCTAssertGreaterThan(template.prompt.count, 20, template.id)
        }
        XCTAssertNotNil(ScheduleTemplate.all.first { $0.id == "review" }?.check)
    }

    /// 配额感知调度 (2026-11-14): a rule put off by the quota says so instead
    /// of its next time, and the run says what happened.
    func testARulePutOffByTheQuotaSaysSo() {
        let record: JSONValue = .object([
            "id": .string("sched-1"), "repeat": .object(["kind": .string("daily"), "minute": .number("540")]),
            "enabled": .bool(true), "nextMs": .number("1800000060000"), "quotaRetryMs": .number("1800000060000"),
        ])
        let rule = LiveScheduleRule.decode(record)
        XCTAssertEqual(rule?.quotaRetryMs, 1_800_000_060_000)
        XCTAssertEqual(rule.map(ScheduleText.next), "额度用完 · \(ScheduleText.date(1_800_000_060_000)) 后补跑")
        XCTAssertNil(rule?.json.objectValue?["quotaRetryMs"], "a save never sends it back")
        XCTAssertEqual(ScheduleText.outcome(LiveScheduleRun(id: "r", outcome: "quotaOut")), "Codex 额度用完，重置后补跑")
        XCTAssertEqual(ScheduleText.outcome(LiveScheduleRun(id: "r", outcome: "quotaOut", manual: true)),
                       "立即运行 · Codex 额度用完，没开会话")
    }

    /// The store saves, switches off, runs now (and shows the run), and
    /// deletes; a refused save keeps the reason.
    func testTheStoreSendsEachActionAndKeepsTheReasonForARefusal() async {
        let service = FakeScheduleService()
        let store = ScheduleStore(service: service)
        let saved = await store.save(LiveScheduleRule(name: "早报", projectId: "p", agent: "codex", prompt: "看看"))
        XCTAssertTrue(saved)
        XCTAssertEqual(store.rules.map(\.id), ["sched-1"])
        await store.setEnabled(store.rules[0], false)
        XCTAssertEqual(store.rules[0].enabled, false)
        await store.runNow(store.rules[0])
        XCTAssertEqual(store.runs["sched-1"]?.first?.detail, "退出码 1")
        XCTAssertNil(store.running)
        await store.delete(store.rules[0])
        XCTAssertTrue(store.rules.isEmpty)
        XCTAssertEqual(service.calls, ["save 早报 true", "save 早报 false", "runNow sched-1", "delete sched-1"])

        service.refuse = "给定时任务起个名字"
        let refused = await store.save(LiveScheduleRule())
        XCTAssertFalse(refused)
        XCTAssertTrue(store.errorText?.contains("给定时任务起个名字") == true, store.errorText ?? "")
    }
}
