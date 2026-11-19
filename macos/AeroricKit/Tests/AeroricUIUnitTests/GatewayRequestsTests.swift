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
}
