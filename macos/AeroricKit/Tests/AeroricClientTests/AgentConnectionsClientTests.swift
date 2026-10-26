@testable import AeroricClient
@testable import AeroricProtocol
import XCTest

/// G: the `agentConnections.*` wire shapes, as the core serializes them.
final class AgentConnectionsClientTests: XCTestCase {
    private func json(_ text: String) throws -> JSONValue {
        JSONValue.from(try JSONSerialization.jsonObject(with: Data(text.utf8)))
    }

    private let snapshotJSON = """
    {"document": {"version": 1,
      "connections": [{"id": "ds", "label": "DeepSeek", "kind": "api_key", "vendor": "other",
        "base_url": "https://api.deepseek.com/v1", "protocols": ["chat_completions"],
        "use_global_proxy": false, "catalog_revision": 3, "enabled": true, "rest_on_limit": false,
        "models": [{"upstream_id": "deepseek-chat", "mapped_name": "deepseek-v4.1-flash", "display_name": "",
          "source": "fetched", "context_window": 128000, "max_output": null, "inputs": ["text"],
          "reasoning_tiers": [{"label": "高", "value": "high"}], "protocols": ["messages"], "enabled": true}]}],
      "pools": [{"id": "p", "label": "池", "strategy": "weighted_round_robin",
        "members": [{"connection_id": "ds", "priority": 1, "weight": 5, "free": true}],
        "sticky_sessions": true, "sticky_ttl_seconds": 1800}],
      "configs": [{"id": "c", "label": "DeepSeek 配置", "harness": "codex",
        "target": {"kind": "route", "pool_id": "p"}, "default_model": "deepseek-v4.1-flash", "default_reasoning": ""}]},
     "credentials": {"ds": "stored"}}
    """

    func testASnapshotDecodesAndEncodesBackWithoutLosingFields() throws {
        let snapshot = try AgentConnectionsClient.decodeSnapshot(try json(snapshotJSON))
        let connection = try XCTUnwrap(snapshot.document.connections.first)
        XCTAssertEqual(connection.baseUrl, "https://api.deepseek.com/v1")
        XCTAssertFalse(connection.restOnLimit)
        XCTAssertEqual(connection.catalogRevision, 3)
        XCTAssertEqual(connection.models.first?.routingName, "deepseek-v4.1-flash")
        XCTAssertEqual(connection.models.first?.protocols, ["messages"])
        XCTAssertEqual(snapshot.document.pools.first?.members.first?.weight, 5)
        XCTAssertEqual(snapshot.document.configs.first?.target, .init(kind: "route", poolId: "p"))
        XCTAssertEqual(snapshot.credentials["ds"], "stored")

        let wire = try AgentConnectionsClient.encode(connection)
        XCTAssertEqual(wire["base_url"]?.stringValue, "https://api.deepseek.com/v1")
        XCTAssertEqual(wire["rest_on_limit"]?.boolValue, false)
        XCTAssertEqual(wire["models"]?.arrayValue?.first?["upstream_id"]?.stringValue, "deepseek-chat")
        XCTAssertEqual(wire["models"]?.arrayValue?.first?["reasoning_tiers"]?.arrayValue?.count, 1)
        XCTAssertNil(wire["baseUrl"])
        let config = try AgentConnectionsClient.encode(snapshot.document.configs[0])
        XCTAssertEqual(config["target"]?["pool_id"]?.stringValue, "p")
        XCTAssertNil(config["target"]?["connection_id"], "nil stays absent")
    }

    func testCandidatesNameHowEachChannelIsReached() throws {
        let value = try json("""
        {"models": ["deepseek-v4.1-flash"], "model": "deepseek-v4.1-flash",
         "candidates": [
           {"connection_id": "ds", "upstream_model": "deepseek-chat", "reach": {"via_gateway": {"from": "responses", "to": "chat_completions"}}, "priority": 0, "weight": 1, "free": false},
           {"connection_id": "oa", "upstream_model": "gpt-x", "reach": "direct", "priority": 0, "weight": 1, "free": false}],
         "excluded": [{"id": "off", "reason": {"code": "connection_disabled"}}]}
        """)
        let candidates = try AgentConnectionsClient.decodeCandidates(value)
        XCTAssertEqual(candidates.channels.map(\.reach), ["经网关 Responses → Chat Completions", "直连"])
        XCTAssertEqual(candidates.excluded.first?.reason, "connection_disabled")
    }

    func testStatusAndOfficialLoginsDecodeCamelCase() throws {
        let cooldowns = try AgentConnectionsClient.decode(
            [AgentConnectionsClient.Cooldown].self,
            try json(#"[{"connectionId": "ds", "model": "m", "untilEpochS": 1800000000, "reason": "NotFound"}]"#),
            snake: false)
        XCTAssertEqual(cooldowns.first?.reason, "NotFound")
        let official = try AgentConnectionsClient.decode(
            AgentConnectionsClient.OfficialStatus.self,
            try json(#"{"codex": {"installed": true, "signedIn": true, "mode": "chatgpt", "account": "me@example.com", "sources": ["/x"]}, "claude": {"installed": false, "signedIn": false, "mode": "none", "sources": []}}"#),
            snake: false)
        XCTAssertEqual(official.codex.account, "me@example.com")
        XCTAssertNil(official.claude.account)
    }
}
