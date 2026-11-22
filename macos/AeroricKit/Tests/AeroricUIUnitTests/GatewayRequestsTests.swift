@testable import AeroricUI
import AeroricProtocol
import XCTest

/// C4: the gateway's requests as the 用量与成本 tab shows them; usage
/// nobody reported reads 用量未知, never 0.
final class GatewayRequestsTests: XCTestCase {
    func testRequestsDecodeWithEachTryAndUnknownUsageIsSaid() {
        let usage = LiveGatewayUsage.decode(.object([
            "summary": .object([
                "totalRequests": .number("12"), "failedRequests": .number("1"),
                "unknownUsageRequests": .number("2"), "inputTokens": .number("12300"),
                "outputTokens": .number("2000"), "cacheWrite5mTokens": .number("1200"),
                "cacheWrite1hTokens": .number("800"),
            ]),
            "requests": .array([
                .object([
                    "requestId": .string("r1"), "model": .string("claude-x"), "targetName": .string("B"),
                    "statusCode": .number("200"), "success": .bool(true), "inputTokens": .number("1200"),
                    "outputTokens": .number("300"), "usageSource": .string("reported"),
                    "attempts": .array([
                        .object(["seq": .number("1"), "targetName": .string("A"), "statusCode": .number("429"),
                                 "latencyMs": .number("400"), "usageSource": .string("unknown"),
                                 "errorSummary": .string("slow down")]),
                        .object(["seq": .number("2"), "targetName": .string("C"), "skipped": .bool(true)]),
                        .object(["seq": .number("3"), "targetName": .string("B"), "statusCode": .number("200"),
                                 "latencyMs": .number("1500"), "inputTokens": .number("1200"),
                                 "outputTokens": .number("300"), "usageSource": .string("reported")]),
                    ]),
                ]),
                .object([
                    "requestId": .string("r2"), "model": .string("gpt-y"), "statusCode": .number("200"),
                    "success": .bool(true), "usageSource": .string("unknown"),
                ]),
            ]),
        ]))
        XCTAssertEqual(GatewayText.summary(usage.summary),
                       "共 12 次 · 失败 1 · 2 次用量未知 · 输入 12.3K · 输出 2K · 缓存写入 5 分钟 1.2K / 1 小时 800")
        let first = usage.requests[0]
        XCTAssertEqual(first.attempts.map(\.seq), [1, 2, 3])
        XCTAssertEqual(GatewayText.request(first), "claude-x · B · 成功 · 1.2K 入 · 300 出 · 试了 2 次")
        XCTAssertEqual(GatewayText.attempt(first.attempts[0]), "第 1 次 · A · 429 · 0.4 秒 · 用量未知 · slow down")
        XCTAssertEqual(GatewayText.attempt(first.attempts[1]), "第 2 次 · C · 熔断中，跳过")
        XCTAssertEqual(GatewayText.request(usage.requests[1]), "gpt-y · 成功 · 用量未知")
    }

    /// E7 (2026-11-22): the gateway's usage by purpose; failed tries are
    /// said apart, and rows from before E7 read 未记录用途.
    func testUsageByPurposeDecodesAndSaysFailedTriesApart() {
        let usage = LiveGatewayUsage.decode(.object([
            "summary": .object([:]), "requests": .array([]),
            "purposes": .array([
                .object(["purpose": .string("review"), "requests": .number("12"), "inputTokens": .number("30000"),
                         "outputTokens": .number("4000"), "failedAttempts": .number("3"),
                         "failedAttemptInputTokens": .number("9000"), "failedAttemptOutputTokens": .number("0")]),
                .object(["purpose": .string(""), "requests": .number("2")]),
            ]),
        ]))
        XCTAssertEqual(usage.purposes.map(\.purpose), ["review", ""])
        let review = GatewayText.purpose(usage.purposes[0])
        XCTAssertTrue(review.hasPrefix("审查 · 12 次 · 输入 "), review)
        XCTAssertTrue(review.contains("失败的尝试 3 次（输入 "), review)
        XCTAssertTrue(GatewayText.purpose(usage.purposes[1]).hasPrefix("未记录用途 · 2 次"))
        XCTAssertEqual(GatewayText.purposeName("unattributed"), "不属于任何会话")
        XCTAssertEqual(LiveGatewayUsage.decode(.object([:])).purposes, [])
    }

    /// E7 step 2: Aeroric's own direct calls say they did not go through
    /// the gateway.
    func testDirectCallsSayTheyWentStraightToTheAnalysisModel() {
        let usage = LiveGatewayUsage.decode(.object([
            "purposes": .array([.object(["purpose": .string("memory"), "requests": .number("4"),
                                         "direct": .bool(true)])]),
        ]))
        XCTAssertEqual(usage.purposes.first?.direct, true)
        XCTAssertTrue(GatewayText.purpose(usage.purposes[0]).hasPrefix("记忆整理（直连分析模型） · 4 次"))
        XCTAssertEqual(GatewayText.purposeName("wiki"), "Wiki 模块说明")
    }
}
