@testable import AeroricClient
@testable import AeroricProtocol
import XCTest

/// AP7:`adapters.capabilities` / `session.control` 的线上形状。
///
/// 用真 UDS 假宿主 + 真 codec:客户端**不**对接口自己复述的策略做二次裁决,
/// 宿主说什么就是什么;宿主拒绝时错误原样抛出,不做静默降级。
final class AdapterClientTests: XCTestCase {
    private func makeClient(
        capabilities: JSONValue,
        control: JSONValue
    ) async throws -> (AdapterClient, ScriptedServer) {
        let server = ScriptedServer()
        addTeardownBlock { server.stop() }
        server.start { line, sender in
            do {
                guard case let .request(request) = try ProtocolCodec.parse(line).get() else {
                    XCTFail("adapter client sent a non-request frame")
                    sender.close()
                    return
                }
                let result: JSONValue
                switch request.method {
                case "handshake":
                    result = .object(["clientId": .string("adapter-test-client")])
                case "adapters.capabilities":
                    result = capabilities
                case "session.control":
                    result = control
                default:
                    XCTFail("unexpected adapter client method: \(request.method)")
                    sender.close()
                    return
                }
                let reply = try ProtocolCodec.encodeLine(.success(SuccessFrame(
                    id: request.id, result: result, additional: [:]
                )))
                sender.full(reply.trimmingCharacters(in: .newlines))
            } catch {
                XCTFail("fake host codec failure: \(error)")
                sender.close()
            }
        }
        let connection = HostConnection(socketPath: server.path, tokenProvider: { "adapter-test-token" })
        addTeardownBlock { await connection.close() }
        try await connection.connect()
        return (AdapterClient(connection: connection), server)
    }

    private func lastControlRequest(from server: ScriptedServer) throws -> RequestFrame {
        let requests = server.recordedLines().compactMap { line -> RequestFrame? in
            guard case let .request(request) = try? ProtocolCodec.parse(line).get() else { return nil }
            return request.method == "session.control" ? request : nil
        }
        return try XCTUnwrap(requests.last, "no session.control request reached the fake host")
    }

    private static let capabilityReport: JSONValue = .object([
        "adapter": .string("dsh"),
        "version": .string("0.1.5-rc.2"),
        "reachableAdapters": .number("1"),
        "agent": .object([
            "nativePrompt": .bool(true),
            "terminalInput": .bool(false),
            "fork": .bool(true),
            "revert": .bool(true),
            "share": .bool(true),
            "planMode": .bool(false),
            "compact": .bool(false),
            "extra": .object([
                "adapter": .string("dsh"),
                "adapterVersion": .string("0.1.5-rc.2"),
                "unsupported": .array([
                    .string("session.plan"),
                    .string("session.compact"),
                ]),
            ]),
        ]),
        "host": .array([]),
    ])

    // MARK: - 能力如实读取

    func testCapabilitiesAreReadVerbatimFromTheHost() async throws {
        let (client, _) = try await makeClient(
            capabilities: Self.capabilityReport,
            control: .object([:])
        )
        let report = try await client.capabilities()
        XCTAssertEqual(report.adapter, "dsh")
        XCTAssertEqual(report.version, "0.1.5-rc.2")
        XCTAssertTrue(report.fork)
        XCTAssertTrue(report.revert)
        XCTAssertTrue(report.share)
        // 探不到的项必须是 false,而且能解释自己。
        XCTAssertFalse(report.planMode)
        XCTAssertFalse(report.compact)
        XCTAssertEqual(report.unsupported, ["session.plan", "session.compact"])
    }

    func testTheReportDrivesWhichControlsTheUiMayShow() async throws {
        let (client, _) = try await makeClient(
            capabilities: Self.capabilityReport,
            control: .object([:])
        )
        let report = try await client.capabilities()
        XCTAssertTrue(report.advertises(.fork))
        XCTAssertFalse(report.advertises(.plan))
        XCTAssertFalse(report.advertises(.compact))
    }

    func testAnAbsentReportOffersNothingInsteadOfGuessing() async throws {
        // 老宿主/无适配器面:全 false,UI 不显示按钮。
        let (client, _) = try await makeClient(
            capabilities: .object(["adapter": .string("unknown"), "agent": .object([:])]),
            control: .object([:])
        )
        let report = try await client.capabilities()
        XCTAssertFalse(AdapterClient.CompanionControlName.allCases.contains { report.advertises($0) })
    }

    // MARK: - 控制请求的线上形状

    func testAPlainForkSendsOnlyCapabilityAndSession() async throws {
        let (client, server) = try await makeClient(
            capabilities: Self.capabilityReport,
            control: .object([
                "capability": .string("fork"),
                "method": .string("session.fork"),
            ])
        )
        let outcome = try await client.control(
            AdapterClient.ControlRequest(capability: .fork, sessionID: "t-1")
        )
        XCTAssertEqual(outcome.capability, "fork")
        XCTAssertEqual(outcome.method, "session.fork")

        let request = try lastControlRequest(from: server)
        let params = request.params
        XCTAssertEqual(params["capability"], .string("fork"))
        XCTAssertEqual(params["sessionId"], .string("t-1"))
        // 没有要求的选项不得出现在线上(缺席 = nil,不是被写成 null)。
        XCTAssertNil(params["planEnabled"])
        XCTAssertNil(params["revertTarget"])
        XCTAssertNil(params["shareDestination"])
        XCTAssertNil(params["confirmed"])
        XCTAssertNil(params["currentPermissionMode"])
    }

    func testWorkspaceRevertCarriesThePathsAndThePreviewFlag() async throws {
        let (client, server) = try await makeClient(
            capabilities: Self.capabilityReport,
            control: .object([
                "capability": .string("revert"),
                "method": .string("session.revert"),
            ])
        )
        var request = AdapterClient.ControlRequest(capability: .revert, sessionID: "t-1")
        request.revertTarget = .workspace(affectedPaths: ["Sources/App.swift"])
        request.previewConfirmed = true
        _ = try await client.control(request)

        let params = try lastControlRequest(from: server).params
        XCTAssertEqual(params["revertTarget"]?["target"], .string("workspace"))
        XCTAssertEqual(params["revertTarget"]?["affectedPaths"], .array([.string("Sources/App.swift")]))
        XCTAssertEqual(params["previewConfirmed"], .bool(true))
    }

    func testSessionRevertSaysSessionNotWorkspace() async throws {
        let (client, server) = try await makeClient(
            capabilities: Self.capabilityReport,
            control: .object([:])
        )
        var request = AdapterClient.ControlRequest(capability: .revert, sessionID: "t-1")
        request.revertTarget = .session
        _ = try await client.control(request)
        XCTAssertEqual(
            try lastControlRequest(from: server).params["revertTarget"]?["target"],
            .string("session")
        )
    }

    func testAPublicShareCarriesEndpointAndConfirmation() async throws {
        let (client, server) = try await makeClient(
            capabilities: Self.capabilityReport,
            control: .object([
                "capability": .string("share"),
                "method": .string("session.share"),
                "share": .object([
                    "payload": .object(["sessionId": .string("t-1")]),
                    "redacted": .array([.string("/apiKey"), .string("/cwd")]),
                    "destination": .object(["target": .string("local")]),
                ]),
            ])
        )
        var request = AdapterClient.ControlRequest(capability: .share, sessionID: "t-1")
        request.shareDestination = .public(endpoint: "https://example.com/upload")
        request.confirmed = true
        let outcome = try await client.control(request)

        let params = try lastControlRequest(from: server).params
        XCTAssertEqual(params["shareDestination"]?["target"], .string("public"))
        XCTAssertEqual(params["shareDestination"]?["endpoint"], .string("https://example.com/upload"))
        XCTAssertEqual(params["confirmed"], .bool(true))
        // 脱敏清单回到 UI,用户能看到什么被拿掉了。
        XCTAssertEqual(outcome.shareRedacted, ["/apiKey", "/cwd"])
    }

    func testPermissionModesAreForwardedAndNotResolvedOnTheClient() async throws {
        let (client, server) = try await makeClient(
            capabilities: Self.capabilityReport,
            control: .object([:])
        )
        var request = AdapterClient.ControlRequest(capability: .fork, sessionID: "t-1")
        request.currentPermissionMode = "full_access"
        request.requestedPermissionMode = "full_access"
        _ = try await client.control(request)

        // 客户端原样转发;漂移判定是宿主的责任。
        let params = try lastControlRequest(from: server).params
        XCTAssertEqual(params["currentPermissionMode"], .string("full_access"))
        XCTAssertEqual(params["requestedPermissionMode"], .string("full_access"))
    }

    // MARK: - 宿主拒绝原样抛出

    func testAHostRefusalIsSurfacedRatherThanSwallowed() async throws {
        let server = ScriptedServer()
        addTeardownBlock { server.stop() }
        server.start { line, sender in
            guard case let .request(request) = try? ProtocolCodec.parse(line).get() else {
                sender.close()
                return
            }
            if request.method == "handshake" {
                let reply = try? ProtocolCodec.encodeLine(.success(SuccessFrame(
                    id: request.id,
                    result: .object(["clientId": .string("c")]),
                    additional: [:]
                )))
                sender.full((reply ?? "").trimmingCharacters(in: .newlines))
                return
            }
            let reply = try? ProtocolCodec.encodeLine(.failure(FailureFrame(
                id: request.id,
                error: RpcError(
                    code: .unsupportedCapability,
                    message: "Unsupported { capability: Compact }",
                    retryable: false
                ),
                additional: [:]
            )))
            sender.full((reply ?? "").trimmingCharacters(in: .newlines))
        }
        let connection = HostConnection(socketPath: server.path, tokenProvider: { "t" })
        addTeardownBlock { await connection.close() }
        try await connection.connect()
        let client = AdapterClient(connection: connection)
        do {
            _ = try await client.control(
                AdapterClient.ControlRequest(capability: .compact, sessionID: "t-1")
            )
            XCTFail("a host refusal must not be turned into a success")
        } catch {
            XCTAssertTrue(
                String(describing: error).contains("Unsupported")
                    || String(describing: error).lowercased().contains("unsupported"),
                "expected the host's refusal to surface, got \(error)"
            )
        }
    }
}
