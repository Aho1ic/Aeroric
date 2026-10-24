@testable import AeroricClient
@testable import AeroricUI
import SwiftUI
import XCTest

/// AP7:Agent 工具面的 UI 接线。
///
/// 断言的是「用户看到什么」与「什么会发出去」之间的关系:未广告的控件不出现,
/// 危险动作在缺证据时构造不出请求,语音草稿不自动变成 prompt。
@MainActor
final class AgentToolsWiringTests: XCTestCase {
    private func report(
        plan: Bool = false,
        compact: Bool = false,
        fork: Bool = true,
        revert: Bool = true,
        share: Bool = true
    ) -> AdapterClient.AgentCapabilityReport {
        AdapterClient.AgentCapabilityReport(
            adapter: "dsh",
            version: "0.1.5-rc.2",
            nativePrompt: true,
            terminalInput: false,
            planMode: plan,
            compact: compact,
            fork: fork,
            revert: revert,
            share: share,
            usage: false,
            attachments: true,
            unsupported: plan ? [] : ["session.plan"],
            note: nil
        )
    }

    // MARK: - 1. 未探测 → 什么都不显示

    func testBeforeProbingNoControlsAreOffered() {
        let store = AgentToolsStore()
        XCTAssertNil(store.capabilities)
        XCTAssertTrue(store.offeredControls.isEmpty)
        XCTAssertEqual(store.reason(for: .fork), "尚未探测到适配器能力")
        // 也不构造请求。
        XCTAssertNil(store.makeRequest(control: .fork, sessionID: "t-1"))
    }

    // MARK: - 2. 只渲染被广告的控制

    func testOnlyAdvertisedControlsAreRendered() {
        let store = AgentToolsStore()
        store.attach(report())
        XCTAssertEqual(store.offeredControls, [.fork, .revert, .share])
        XCTAssertFalse(store.isOffered(.plan))
        XCTAssertFalse(store.isOffered(.compact))
        XCTAssertTrue(store.isOffered(.fork))
    }

    func testAReasonIsAvailableForEveryMissingControl() {
        let store = AgentToolsStore()
        store.attach(report())
        XCTAssertNil(store.reason(for: .fork), "an advertised control needs no excuse")
        let plan = store.reason(for: .plan)
        XCTAssertEqual(plan, "适配器 dsh 未实现 session.plan")
        // compact 不在 unsupported 清单里 → 说法换成「未广告」。
        XCTAssertEqual(store.reason(for: .compact), "适配器 dsh 未广告 session.compact")
    }

    // MARK: - 3. 危险动作缺证据就构造不出请求

    func testAWorkspaceRevertWithoutAConfirmedPreviewCannotBeBuilt() {
        let store = AgentToolsStore()
        store.attach(report())
        XCTAssertNil(
            store.makeRequest(
                control: .revert,
                sessionID: "t-1",
                workspacePaths: ["Sources/App.swift"]
            ),
            "a workspace revert must not be constructible before the user confirmed the diff"
        )
        let confirmed = store.makeRequest(
            control: .revert,
            sessionID: "t-1",
            workspacePaths: ["Sources/App.swift"],
            previewConfirmed: true
        )
        XCTAssertEqual(confirmed?.revertTarget, .workspace(affectedPaths: ["Sources/App.swift"]))
        // 没有工作区改动 → 会话回退,不需要预览。
        let session = store.makeRequest(control: .revert, sessionID: "t-1")
        XCTAssertEqual(session?.revertTarget, .session)
    }

    func testAPublicShareNeedsAnEndpointAndConfirmation() {
        let store = AgentToolsStore()
        store.attach(report())
        XCTAssertNil(
            store.makeRequest(control: .share, sessionID: "t-1", shareIsPublic: true),
            "a public share without confirmation must not be constructible"
        )
        XCTAssertNil(
            store.makeRequest(
                control: .share,
                sessionID: "t-1",
                shareIsPublic: true,
                shareEndpoint: "",
                confirmed: true
            ),
            "an empty endpoint must not be constructible"
        )
        let ok = store.makeRequest(
            control: .share,
            sessionID: "t-1",
            shareIsPublic: true,
            shareEndpoint: "https://example.com/upload",
            confirmed: true
        )
        XCTAssertEqual(ok?.shareDestination, .public(endpoint: "https://example.com/upload"))
        // 本地导出是默认,不需要确认。
        let local = store.makeRequest(control: .share, sessionID: "t-1")
        XCTAssertEqual(local?.shareDestination, .local(path: ""))
    }

    // MARK: - 4. 权限漂移必须显式呈现

    func testPermissionDriftIsVisibleAndDoesNotSilentlyChangeTheQueue() {
        let store = AgentToolsStore()
        store.attach(report())
        XCTAssertFalse(store.permissionDrift)
        store.queuedPermissionMode = "ask"
        store.currentPermissionMode = "full_access"
        XCTAssertTrue(store.permissionDrift)
        // 请求里带的是**当前**值,由宿主判漂移;store 不替用户改。
        let request = store.makeRequest(control: .fork, sessionID: "t-1")
        XCTAssertEqual(request?.currentPermissionMode, "full_access")
    }

    // MARK: - 5. 拒绝原样记录

    func testRefusalsAreRecordedVerbatimAndClearedOnSuccess() {
        struct Refused: Error, CustomStringConvertible {
            var description: String { "Unsupported { capability: Compact }" }
        }
        let store = AgentToolsStore()
        store.attach(report())
        store.recordRefusal(Refused())
        XCTAssertEqual(store.lastRefusal, "Unsupported { capability: Compact }")
        store.record(
            AdapterClient.ControlOutcome(
                capability: "fork", method: "session.fork", shareRedacted: []
            ),
            confirmed: false
        )
        XCTAssertNil(store.lastRefusal)
        XCTAssertEqual(store.outcomes.first?.method, "session.fork")
    }

    // MARK: - 6. 沙箱 fail closed

    func testASandboxThatIsRequestedButNotReadyBlocksToolExecution() {
        let ready = ExternalProtocolStore.SandboxStatus(
            requestedEnabled: true, state: .ready, backend: "seatbelt", writableRoots: ["/tmp/w"]
        )
        XCTAssertTrue(ready.allowsToolExecution)
        XCTAssertNil(ready.blockingReason)

        let notReady = ExternalProtocolStore.SandboxStatus(
            requestedEnabled: true, state: .notReady, backend: "seatbelt", note: "扩展未加载"
        )
        XCTAssertFalse(notReady.allowsToolExecution)
        XCTAssertEqual(notReady.blockingReason, "扩展未加载")

        let unsupported = ExternalProtocolStore.SandboxStatus(
            requestedEnabled: true, state: .unsupported, backend: "none"
        )
        XCTAssertFalse(unsupported.allowsToolExecution)
        XCTAssertNotNil(unsupported.blockingReason)

        // 用户没要求沙箱:不拦。
        let off = ExternalProtocolStore.SandboxStatus(
            requestedEnabled: false, state: .unsupported, backend: "none"
        )
        XCTAssertTrue(off.allowsToolExecution)
        XCTAssertNil(off.blockingReason)
    }

    // MARK: - 7. 语音：默认关闭、先草稿

    func testVoiceIsOffByDefaultAndADraftIsNotAPrompt() {
        let store = ExternalProtocolStore()
        XCTAssertFalse(store.voice.optedIn, "voice must be opt-in")
        XCTAssertFalse(store.voice.canSubmit)

        store.setVoiceOptIn(true)
        XCTAssertFalse(store.voice.canSubmit, "an empty draft cannot be submitted")

        // 转写结果先到草稿,什么都没发。
        store.voice.phase = .draft
        store.voice.transcript = "run the tests"
        XCTAssertTrue(store.voice.canSubmit)
        XCTAssertNil(store.voice.submittedPrompt)

        XCTAssertEqual(store.submitVoiceDraft(), "run the tests")
        XCTAssertEqual(store.voice.submittedPrompt, "run the tests")
        XCTAssertEqual(store.voice.phase, .sent)

        // 空白草稿提交不了。
        store.voice.phase = .draft
        store.voice.transcript = "   "
        XCTAssertFalse(store.voice.canSubmit)
        XCTAssertNil(store.submitVoiceDraft())
    }

    func testCancellingVoiceLeavesNothingToSend() {
        let store = ExternalProtocolStore()
        store.setVoiceOptIn(true)
        store.voice.phase = .draft
        store.voice.transcript = "half a thought"
        store.cancelVoice()
        XCTAssertEqual(store.voice.phase, .cancelled)
        XCTAssertNil(store.voice.submittedPrompt)
        XCTAssertNil(store.submitVoiceDraft())

        // 关闭语音输入也清掉草稿。
        store.voice.phase = .draft
        store.voice.transcript = "again"
        store.setVoiceOptIn(false)
        XCTAssertEqual(store.voice.phase, .cancelled)
        XCTAssertNil(store.voice.submittedPrompt)
    }

    // MARK: - 8. OAuth 只有状态

    func testAuthStatusExposesNoTokenOnlyAnOwner() {
        let configured = ExternalProtocolStore.AuthStatus(
            state: .configured(owner: "keychain", expiresAt: 1_700_000_000, generation: 3)
        )
        XCTAssertTrue(configured.isUsable)
        XCTAssertEqual(configured.owner, "keychain")

        let expired = ExternalProtocolStore.AuthStatus(
            state: .expired(owner: "keychain", generation: 3)
        )
        XCTAssertFalse(expired.isUsable)
        XCTAssertEqual(expired.owner, "keychain")

        XCTAssertNil(ExternalProtocolStore.AuthStatus(state: .notConfigured).owner)
    }

    // MARK: - 9. 权限交接只指路

    func testPermissionHandoffPointsAtSystemSettingsAndNeverClaimsToGrant() {
        let store = ExternalProtocolStore(
            activities: [
                ExternalProtocolStore.ToolActivity(
                    tool: "browser.open",
                    summary: "open a page",
                    executor: "external agent",
                    permissions: [
                        "screenRecording": "denied",
                        "accessibility": "unstableSignature",
                        "camera": "granted",
                    ]
                )
            ]
        )
        XCTAssertTrue(store.permissionHandoff(for: "camera").contains("已授权"))
        XCTAssertTrue(store.permissionHandoff(for: "accessibility").contains("重新授权"))
        XCTAssertTrue(store.permissionHandoff(for: "screenRecording").contains("系统设置"))
        XCTAssertTrue(store.permissionHandoff(for: "microphone").contains("系统设置"))
    }

    // MARK: - 10. 视图真的渲染

    func testThePanelAndBarRenderForBothEmptyAndPopulatedStores() {
        let empty = AgentToolsStore()
        let panel = AgentToolsPanel(store: ExternalProtocolStore(), controls: empty)
        XCTAssertNotNil(panel.body)
        XCTAssertNotNil(SessionControlBar(store: empty).body)

        let populated = AgentToolsStore()
        populated.attach(report())
        let populatedPanel = AgentToolsPanel(
            store: ExternalProtocolStore(
                sandbox: ExternalProtocolStore.SandboxStatus(
                    requestedEnabled: true, state: .notReady, backend: "seatbelt", note: "x"
                ),
                voice: ExternalProtocolStore.VoiceState(optedIn: true, phase: .draft, transcript: "hi")
            ),
            controls: populated
        )
        XCTAssertNotNil(populatedPanel.body)
        let bar = SessionControlBar(
            store: populated,
            workspaceDiff: ["- old", "+ new"]
        )
        XCTAssertNotNil(bar.body)
    }
}
