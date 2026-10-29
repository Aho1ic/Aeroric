@testable import AeroricUI
import XCTest

/// Cooldown labels and the dropped Codex notice (2026-10-29).
final class GatewayStatusTests: XCTestCase {
    func testACoolingConnectionSaysWhyAndUntilWhenInOneLine() {
        XCTAssertEqual(AgentConfigText.coolingTag(until: "17:45", reason: "Network"), "冷却到 17:45 · 网络错误")
        XCTAssertEqual(AgentConfigText.coolingTag(until: "18:00", reason: "RateLimited"), "冷却到 18:00 · 限流")
    }

    func testCodexFallbackMetadataNoticeIsRecognised() {
        let notice = "Model metadata for `deepseek-v4.1-flash` not found. Defaulting to fallback metadata; this can degrade performance and cause issues."
        XCTAssertEqual(LiveNoticeRow.fallbackModel(in: notice), "deepseek-v4.1-flash")
        XCTAssertNil(LiveNoticeRow.fallbackModel(in: "Reconnecting…"))
    }
}
