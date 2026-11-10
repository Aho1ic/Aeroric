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
}
