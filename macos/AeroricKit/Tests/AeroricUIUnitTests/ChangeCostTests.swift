import XCTest

@testable import AeroricClient
import AeroricProtocol
@testable import AeroricUI

final class ChangeCostTests: XCTestCase {
    private typealias Row = SessionsClient.ChangeCostRow
    private typealias Bucket = SessionsClient.ChangeCostBucket

    func testDecodesTheHostRows() throws {
        let json = """
        {"rows":[{"agent":"codex","configId":"via-codex-x","config":"CPA","model":"gpt-6.1-sol",
          "accepted":{"turns":4,"tokens":400000,"costUsd":0.4},"undone":{"turns":1,"tokens":90000},
          "pending":{"turns":0,"tokens":0},"noChange":{"turns":2,"tokens":5000}}]}
        """
        let value = JSONValue.from(try JSONSerialization.jsonObject(with: Data(json.utf8)))
        let row = try XCTUnwrap(SessionsClient.changeCostRows(value).first)
        XCTAssertEqual(row.config, "CPA")
        XCTAssertEqual(row.accepted, Bucket(turns: 4, tokens: 400_000, costUsd: 0.4))
        XCTAssertNil(row.undone.costUsd)
    }

    func testWordsARow() {
        let row = Row(agent: "codex", config: "CPA", model: "gpt-6.1-sol",
                      accepted: Bucket(turns: 4, tokens: 400_000, costUsd: 0.4),
                      undone: Bucket(turns: 1, tokens: 90_000),
                      noChange: Bucket(turns: 2, tokens: 5_000))
        XCTAssertEqual(ChangeCostText.title(row), "CPA · gpt-6.1-sol")
        XCTAssertEqual(ChangeCostText.acceptRate(row), "80% · 4/5 轮")
        XCTAssertEqual(ChangeCostText.perAccepted(row), "≈100K tok · ≈$0.100")
        XCTAssertEqual(ChangeCostText.total(row.undone), "1 轮 · 90K tok")
        XCTAssertNil(ChangeCostText.total(row.pending))
    }

    func testARowWithNothingDecidedHasNoRate() {
        let row = Row(agent: "claude", model: "", noChange: Bucket(turns: 3, tokens: 30))
        XCTAssertEqual(ChangeCostText.title(row), "claude（未选配置） · 未记模型")
        XCTAssertNil(ChangeCostText.acceptRate(row))
        XCTAssertNil(ChangeCostText.perAccepted(row))
    }

    // MARK: - R3-3b quota pace

    private typealias Pace = MaintenanceClient.QuotaWindowPace

    private var shanghai: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "Asia/Shanghai")!
        return calendar
    }

    /// 2026-11-10 10:00 in Shanghai.
    private let now = Date(timeIntervalSince1970: 1_794_276_000)

    func testDecodesTheQuotaReply() throws {
        let json = """
        {"status":"available","planType":"plus","windows":[{"window":"secondary","usedPercent":89,
          "resetAt":1794471089,"windowMinutes":10080,"elapsedPercent":68,"recent":false,
          "verdict":"fast","exhaustAt":1794379200}]}
        """
        let pace = try JSONDecoder().decode(MaintenanceClient.QuotaPace.self, from: Data(json.utf8))
        XCTAssertEqual(pace.planType, "plus")
        XCTAssertEqual(pace.windows?.first?.exhaustAt, 1_794_379_200)
        XCTAssertNil(pace.windows?.first?.leftAtReset)
    }

    func testWordsAFastWeek() {
        // Out at 11-11 14:40, reset 11-12 16:11: a day and an hour and a half early.
        let pace = Pace(window: "secondary", usedPercent: 89, resetAt: 1_794_471_089, windowMinutes: 10_080,
                        elapsedPercent: 68, recent: false, verdict: "fast", exhaustAt: 1_794_379_200)
        XCTAssertEqual(QuotaText.windowLabel(pace), "每周")
        XCTAssertEqual(QuotaText.verdict(pace), "偏快")
        XCTAssertEqual(QuotaText.usage(pace, now: now, calendar: shanghai), "已用 89% · 窗口过了 68% · 11月12日 16:11 重置")
        XCTAssertEqual(QuotaText.detail(pace, now: now, calendar: shanghai),
                       "按现在的速度约 11月11日 14:40 用完，比重置早 1 天 1 小时")
        XCTAssertEqual(QuotaText.basis(pace), "按这个窗口的平均速度推算")
    }

    func testWordsAnOkFiveHoursAndAFreshOne() {
        let ok = Pace(window: "primary", usedPercent: 16, resetAt: 1_794_286_142, windowMinutes: 300,
                      elapsedPercent: 44, recent: true, verdict: "ok", leftAtReset: 64)
        XCTAssertEqual(QuotaText.windowLabel(ok), "5 小时")
        XCTAssertEqual(QuotaText.usage(ok, now: now, calendar: shanghai), "已用 16% · 窗口过了 44% · 12:49 重置")
        XCTAssertEqual(QuotaText.detail(ok, now: now, calendar: shanghai), "按现在的速度，重置时约剩 64%")
        XCTAssertEqual(QuotaText.basis(ok), "按最近一小时的速度推算")
        var fresh = ok
        fresh.verdict = "unknown"
        XCTAssertEqual(QuotaText.verdict(fresh), "读数不够")
        var spent = ok
        (spent.verdict, spent.usedPercent) = ("fast", 100)
        XCTAssertEqual(QuotaText.detail(spent, now: now, calendar: shanghai), "已用完，12:49 重置")
        XCTAssertEqual(QuotaText.span(seconds: 125 * 60), "2 小时 5 分钟")
    }
}
