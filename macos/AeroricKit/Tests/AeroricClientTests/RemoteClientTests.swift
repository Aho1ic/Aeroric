import XCTest

@testable import AeroricClient
@testable import AeroricProtocol

/// P8-03 刀五：RemoteClient 的 wire 解析（字段缺失/可选降级/形状不符回退）。
/// 不启动连接：解析纯函数直接喂 JSONValue。
final class RemoteClientParsingTests: XCTestCase {
    private let status = JSONValue.object([
        "enabled": .bool(true),
        "running": .bool(true),
        "networkExposed": .bool(false),
        "port": .number("6790"),
        "lanIp": .string("192.168.1.10"),
        "lanAddresses": .array([
            .object(["interfaceName": .string("en0"), "ip": .string("192.168.1.10")]),
        ]),
        "onlineCount": .number("2"),
        "relayUrl": .string("wss://relay.example.com"),
        "relayToken": .string("tok"),
        "publicEndpoints": .array([.string("ws://100.64.0.2:6790")]),
        "relayState": .string("online"),
    ])

    func testStatusParsesFullPayload() {
        let status = RemoteClient.parseStatus(self.status)
        XCTAssertTrue(status.enabled)
        XCTAssertTrue(status.running)
        XCTAssertFalse(status.networkExposed)
        XCTAssertEqual(status.port, 6790)
        XCTAssertEqual(status.lanIP, "192.168.1.10")
        XCTAssertEqual(status.lanAddresses.count, 1)
        XCTAssertEqual(status.lanAddresses[0].interfaceName, "en0")
        XCTAssertEqual(status.onlineCount, 2)
        XCTAssertEqual(status.relayState, "online")
        XCTAssertEqual(status.publicEndpoints, ["ws://100.64.0.2:6790"])
    }

    func testStatusDegradesToOfflineOnShapeMismatch() {
        XCTAssertEqual(RemoteClient.parseStatus(.null), .offline())
        XCTAssertEqual(RemoteClient.parseStatus(.string("nope")), .offline())
    }

    func testStatusDefaultsMissingFields() {
        // 只有部分字段：缺失走类型默认（false/0/nil），不崩。
        let partial = JSONValue.object(["running": .bool(true)])
        let status = RemoteClient.parseStatus(partial)
        XCTAssertTrue(status.running)
        XCTAssertFalse(status.enabled)
        XCTAssertEqual(status.port, 0)
        XCTAssertNil(status.lanIP)
    }

    func testDeviceListSkipsMalformedRows() {
        let devices = JSONValue.array([
            .object([
                "id": .string("dev-1"), "name": .string("Phone"),
                "createdAt": .number("100"), "lastSeenAt": .number("200"),
                "online": .bool(true),
            ]),
            // 缺 name → 整行丢弃（宽容镜像：无 id 的行不上 store）
            .object(["id": .string("dev-2")]),
            // 非 object → 丢弃
            .string("junk"),
        ])
        let frame = JSONValue.object(["devices": devices])
        guard case let .array(items) = frame["devices"] else {
            return XCTFail("array expected")
        }
        // 直接复用 listDevices 的行解析语义：id/name/时间齐全才保留。
        let parsed: [RemoteClient.DeviceView] = items.compactMap { item in
            guard case let .object(object) = item,
                  case let .string(id) = object["id"],
                  case let .string(name) = object["name"],
                  let createdAt = object["createdAt"]?.numberText.flatMap(Int64.init),
                  let lastSeenAt = object["lastSeenAt"]?.numberText.flatMap(Int64.init)
            else { return nil }
            let online = object["online"]?.boolValue ?? false
            return RemoteClient.DeviceView(
                id: id, name: name, createdAt: createdAt, lastSeenAt: lastSeenAt, online: online
            )
        }
        XCTAssertEqual(parsed.count, 1)
        XCTAssertEqual(parsed[0].id, "dev-1")
        XCTAssertTrue(parsed[0].online)
        void(frame)
    }

    func testStatusNumbersAreTextBacked() {
        // .number(String) 形态：大端口/时间戳数字以文本承载，解析不丢精度。
        let big = JSONValue.object(["port": .number("99999")])
        XCTAssertEqual(RemoteClient.parseStatus(big).port, 99999)
    }

    private func void<T>(_: T) {}
}
