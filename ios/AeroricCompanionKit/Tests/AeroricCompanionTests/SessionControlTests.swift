@testable import AeroricMobileTransport
import XCTest

/// AP7 / Q8:iOS 伴生端的会话控制面。
///
/// 断言的是**宿主契约**(`session.control` 的 params 形状与 `session.*` 字面量),
/// 不是本地日志:客户端不预判宿主的裁决,但必须在自己的能力表之外拒绝,
/// 并且绝不假装控制已经被上游执行。
@MainActor
final class SessionControlTests: XCTestCase {
    private func onlineStore(
        _ capabilities: CompanionAdapterCapabilities
    ) throws -> CompanionStore {
        let store = CompanionStore()
        try store.pairWithMac(
            hostName: "mac",
            staticPubB64: Base64UrlCodec.encode(Data(repeating: 7, count: 32)),
            initialEpoch: "e1",
            mode: .directLAN
        )
        store.updateAdapterCapabilities(capabilities)
        return store
    }

    private func dshCapabilities() -> CompanionAdapterCapabilities {
        CompanionAdapterCapabilities(
            adapter: "dsh",
            version: "0.1.5-rc.2",
            plan: true,
            compact: false,
            fork: true,
            revert: true,
            share: true
        )
    }

    // MARK: - 1. 能力广告决定 UI 面

    func testOnlyAdvertisedControlsAreOffered() throws {
        let store = try onlineStore(dshCapabilities())
        XCTAssertEqual(store.adapterCapabilities.offeredControls, [.plan, .fork, .revert, .share])
        XCTAssertFalse(store.adapterCapabilities.advertises(.compact))

        // 没探测到 = 什么都不给,而不是给一个注定失败的按钮。
        let unknown = try onlineStore(.none)
        XCTAssertTrue(unknown.adapterCapabilities.offeredControls.isEmpty)
    }

    /// 跨语言契约:这些字面量就是宿主 `SessionCapability::method()` 的值。
    func testControlLiteralsMatchTheHostVocabulary() {
        XCTAssertEqual(CompanionSessionControl.plan.rawValue, "session.plan")
        XCTAssertEqual(CompanionSessionControl.compact.rawValue, "session.compact")
        XCTAssertEqual(CompanionSessionControl.fork.rawValue, "session.fork")
        XCTAssertEqual(CompanionSessionControl.revert.rawValue, "session.revert")
        XCTAssertEqual(CompanionSessionControl.share.rawValue, "session.share")
        // capability 字段只带短名(`plan`),方法名带前缀。
        XCTAssertEqual(CompanionSessionControl.revert.capabilityName, "revert")
    }

    // MARK: - 2. 已广告的控制走通

    func testAnAdvertisedControlIsSentAndRecorded() throws {
        let store = try onlineStore(dshCapabilities())
        let outcome = try store.triggerSessionControl(.fork, taskId: "t-1")
        XCTAssertEqual(outcome.capability, "fork")
        XCTAssertEqual(outcome.method, "session.fork")
        XCTAssertEqual(store.controlOutcomes.count, 1)
        XCTAssertEqual(store.sentFramesLog, ["session.fork:t-1"])
    }

    func testTheControlFrameCarriesOnlyWhatTheUserAskedFor() {
        var request = CompanionControlRequest(control: .plan, taskID: "t-1")
        XCTAssertNil(request.frame["planEnabled"], "an unset option stays off the wire")

        request.planEnabled = false
        XCTAssertEqual(request.frame["planEnabled"] as? Bool, false)
        XCTAssertEqual(request.frame["capability"] as? String, "plan")
        XCTAssertEqual(request.frame["sessionId"] as? String, "t-1")
        XCTAssertNil(request.frame["confirmed"])
        XCTAssertNil(request.frame["revertTarget"])
    }

    // MARK: - 3. 未广告 → 拒绝且不发

    func testAnUnadvertisedControlIsRefusedWithoutSendingAnything() throws {
        let store = try onlineStore(dshCapabilities())
        XCTAssertThrowsError(try store.triggerSessionControl(.compact, taskId: "t-1")) { error in
            XCTAssertEqual(error as? CompanionControlRefusal, .notAdvertised)
        }
        XCTAssertTrue(store.sentFramesLog.isEmpty, "a refused control must not reach the wire")
        XCTAssertTrue(store.controlOutcomes.isEmpty)
    }

    func testAnUnknownAdapterOffersAndSendsNothing() throws {
        let store = try onlineStore(.none)
        for control in CompanionSessionControl.allCases {
            XCTAssertThrowsError(try store.triggerSessionControl(control, taskId: "t-1")) { error in
                XCTAssertEqual(error as? CompanionControlRefusal, .notAdvertised)
            }
        }
        XCTAssertTrue(store.sentFramesLog.isEmpty)
    }

    // MARK: - 4. revert:工作区要预览,session 不要

    func testWorkspaceRevertNeedsAConfirmedPreview() throws {
        let store = try onlineStore(dshCapabilities())
        let unconfirmed = CompanionControlRequest(
            control: .revert,
            taskID: "t-1",
            workspacePaths: ["Sources/App.swift"]
        )
        XCTAssertEqual(
            unconfirmed.refusal(advertised: store.adapterCapabilities),
            .previewRequired
        )
        XCTAssertThrowsError(try store.triggerSessionControl(unconfirmed))

        let confirmed = CompanionControlRequest(
            control: .revert,
            taskID: "t-1",
            workspacePaths: ["Sources/App.swift"],
            previewConfirmed: true
        )
        XCTAssertNil(confirmed.refusal(advertised: store.adapterCapabilities))
        _ = try store.triggerSessionControl(confirmed)
        XCTAssertEqual(
            (confirmed.frame["revertTarget"] as? [String: Any])?["target"] as? String,
            "workspace"
        )
        XCTAssertEqual(
            (confirmed.frame["revertTarget"] as? [String: Any])?["affectedPaths"] as? [String],
            ["Sources/App.swift"]
        )
    }

    func testSessionRevertNeedsNoPreview() throws {
        let store = try onlineStore(dshCapabilities())
        let request = CompanionControlRequest(control: .revert, taskID: "t-1")
        XCTAssertNil(request.refusal(advertised: store.adapterCapabilities))
        _ = try store.triggerSessionControl(request)
        XCTAssertEqual(
            (request.frame["revertTarget"] as? [String: Any])?["target"] as? String,
            "session"
        )
    }

    // MARK: - 5. share:默认本地,公开要 endpoint + 确认

    func testLocalShareIsTheDefaultAndNeedsNoConfirmation() throws {
        let store = try onlineStore(dshCapabilities())
        let request = CompanionControlRequest(control: .share, taskID: "t-1")
        XCTAssertNil(request.refusal(advertised: store.adapterCapabilities))
        _ = try store.triggerSessionControl(request)
        XCTAssertEqual(
            (request.frame["shareDestination"] as? [String: Any])?["target"] as? String,
            "local"
        )
    }

    func testPublicShareNeedsAnEndpointAndConfirmation() throws {
        let store = try onlineStore(dshCapabilities())
        let noEndpoint = CompanionControlRequest(
            control: .share,
            taskID: "t-1",
            shareIsPublic: true,
            confirmed: true
        )
        XCTAssertEqual(
            noEndpoint.refusal(advertised: store.adapterCapabilities),
            .confirmationRequired
        )

        let unconfirmed = CompanionControlRequest(
            control: .share,
            taskID: "t-1",
            shareIsPublic: true,
            shareEndpoint: "https://example.com/upload"
        )
        XCTAssertEqual(
            unconfirmed.refusal(advertised: store.adapterCapabilities),
            .confirmationRequired
        )
        XCTAssertThrowsError(try store.triggerSessionControl(unconfirmed))

        let ok = CompanionControlRequest(
            control: .share,
            taskID: "t-1",
            shareIsPublic: true,
            shareEndpoint: "https://example.com/upload",
            confirmed: true
        )
        _ = try store.triggerSessionControl(ok)
        let destination = ok.frame["shareDestination"] as? [String: Any]
        XCTAssertEqual(destination?["target"] as? String, "public")
        XCTAssertEqual(destination?["endpoint"] as? String, "https://example.com/upload")
    }

    // MARK: - 6. 离线不发

    func testAnOfflineMacRefusesBeforeAnythingElse() throws {
        let store = try onlineStore(dshCapabilities())
        store.markMacOffline(relayStillConnected: false)
        XCTAssertThrowsError(try store.triggerSessionControl(.fork, taskId: "t-1"))
        XCTAssertTrue(store.sentFramesLog.isEmpty)
    }

    // MARK: - 7. 探针结果由宿主覆盖

    func testTheHostsProbeReplacesThePreviousCapabilitySet() throws {
        let store = try onlineStore(dshCapabilities())
        XCTAssertTrue(store.adapterCapabilities.advertises(.fork))

        // 换了适配器/降级版本:能力表整体替换,不留旧值。
        store.updateAdapterCapabilities(
            CompanionAdapterCapabilities(adapter: "omp", version: "1.0")
        )
        XCTAssertFalse(store.adapterCapabilities.advertises(.fork))
        XCTAssertThrowsError(try store.triggerSessionControl(.fork, taskId: "t-1"))
    }
}
