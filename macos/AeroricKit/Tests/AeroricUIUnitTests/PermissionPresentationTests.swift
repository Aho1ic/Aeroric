import XCTest

@testable import AeroricUI

/// P8-04 刀五验收：系统权限面的呈现策略。
///
/// 这个套件要钉住的是**界面的诚实性**，不是系统的状态——后者随机器变，前者不该变：
///
/// 1. **ad-hoc / linker-signed 之后不许显示成永久授权**（计划 Acceptance 的第一条）。
///    重签一次再肉眼看一遍不是回归测试；这里用 `stableAcrossUpdates: false` 断言
///    headline/explanation 都在讲"升级后会失效"，并且**仍然给得出**可执行的
///    Re-authorize 计划（说清会清什么）。
/// 2. **`notBundled` 优先于签名不稳定**，且此时不提供 Re-authorize——授权根本不在
///    本应用名下，清本应用的服务只会清掉别人的记录。
/// 3. **没有确认就没有动作**：`ReauthorizeSequence.steps(for:confirmedByUser:)` 在
///    未确认时返回空表；`canReauthorize` 的三条否决逐条有用例。
/// 4. **`unknown` 不画成已授权**（这是反例清单里指名的一条）。
@MainActor
final class PermissionPresentationTests: XCTestCase {

    // MARK: - 身份呈现

    func testAdhocSignatureNeverReadsAsPermamentlyAuthorized() {
        let plan = PermissionPresentation.plan(
            platform: "macos",
            bundleIdentifier: "com.aeroric.native.dev",
            signature: "adhoc",
            stableAcrossUpdates: false,
            resettableServices: ["ScreenCapture", "Microphone"],
            reRequest: ["screen-recording", "microphone"]
        )
        XCTAssertTrue(plan.headline.contains("失效"), plan.headline)
        XCTAssertTrue(plan.explanation.contains("ad-hoc") == false, "签名种类按参数显示")
        XCTAssertTrue(plan.explanation.contains("adhoc"), plan.explanation)
        XCTAssertTrue(plan.explanation.contains("重新授权"), plan.explanation)
        // 仍然给得出可执行的计划：说清会清哪些服务。
        XCTAssertEqual(plan.reauthorize?.services, ["ScreenCapture", "Microphone"])
        XCTAssertEqual(plan.reauthorize?.bundleIdentifier, "com.aeroric.native.dev")

        // linker-signed 与 adhoc 同类：都是"这次构建专属"的签名。
        let linker = PermissionPresentation.plan(
            platform: "macos", bundleIdentifier: "com.aeroric.native.dev",
            signature: "linker-signed", stableAcrossUpdates: false,
            resettableServices: [], reRequest: []
        )
        XCTAssertTrue(linker.headline.contains("失效"), linker.headline)
    }

    func testNotBundledWinsOverUnstableSignature() {
        let plan = PermissionPresentation.plan(
            platform: "macos",
            bundleIdentifier: nil,
            signature: "linker-signed",
            stableAcrossUpdates: false,
            resettableServices: ["ScreenCapture"],
            reRequest: ["screen-recording"]
        )
        XCTAssertTrue(plan.headline.contains("终端"), plan.headline)
        // 没有本应用的记录可清 ⇒ 不给一个点了没用的按钮。
        XCTAssertNil(plan.reauthorize)
        // 主体名不是空白字符串（界面要有个东西可显示）。
        XCTAssertFalse(plan.bundleIdentifier.isEmpty)
    }

    func testStableSignaturesStillOfferReauthorization() {
        let plan = PermissionPresentation.plan(
            platform: "macos",
            bundleIdentifier: "com.aeroric.desktop",
            signature: "developer-id",
            stableAcrossUpdates: true,
            resettableServices: ["ScreenCapture"],
            reRequest: ["screen-recording"]
        )
        XCTAssertTrue(plan.headline.contains("签名稳定"), plan.headline)
        XCTAssertEqual(plan.reauthorize?.services, ["ScreenCapture"])
    }

    func testUnsupportedPlatformsSaySoInsteadOfShowingAnEmptyTable() {
        let plan = PermissionPresentation.plan(
            platform: "windows", bundleIdentifier: "com.aeroric.desktop",
            signature: "not-applicable", stableAcrossUpdates: true,
            resettableServices: [], reRequest: []
        )
        XCTAssertTrue(plan.explanation.contains("windows"), plan.explanation)
        XCTAssertNil(plan.reauthorize)
        XCTAssertFalse(PermissionPresentation.supports(platform: "windows"))
        XCTAssertFalse(PermissionPresentation.supports(platform: "linux"))
        XCTAssertTrue(PermissionPresentation.supports(platform: "macos"))
    }

    // MARK: - 确认门控

    func testReauthorizeNeedsConfirmationServicesAndAPlan() {
        let plan = PermissionPresentation.plan(
            platform: "macos", bundleIdentifier: "com.aeroric.native.dev",
            signature: "adhoc", stableAcrossUpdates: false,
            resettableServices: ["ScreenCapture"], reRequest: ["screen-recording"]
        )
        XCTAssertTrue(PermissionPresentation.canReauthorize(plan, confirmedByUser: true))
        XCTAssertFalse(
            PermissionPresentation.canReauthorize(plan, confirmedByUser: false),
            "没有确认就不能动系统权限"
        )
        XCTAssertFalse(PermissionPresentation.canReauthorize(nil, confirmedByUser: true))

        // 没有可清的服务 ⇒ 不给按钮。
        let nothingToReset = PermissionPresentation.plan(
            platform: "macos", bundleIdentifier: "com.aeroric.native.dev",
            signature: "adhoc", stableAcrossUpdates: false,
            resettableServices: [], reRequest: ["screen-recording"]
        )
        XCTAssertFalse(PermissionPresentation.canReauthorize(nothingToReset, confirmedByUser: true))
    }

    func testReauthorizeStepsRequireConfirmation() {
        let plan = ReauthorizePlan(
            summary: "clear and re-request",
            services: ["ScreenCapture", "Microphone"],
            reRequest: ["screen-recording", "microphone"],
            bundleIdentifier: "com.aeroric.native.dev"
        )
        XCTAssertTrue(ReauthorizeSequence.steps(for: plan, confirmedByUser: false).isEmpty)
        XCTAssertTrue(ReauthorizeSequence.steps(for: nil, confirmedByUser: true).isEmpty)

        let steps = ReauthorizeSequence.steps(for: plan, confirmedByUser: true)
        // 先 reset 后 request：反过来系统认为"已授权"，弹窗不会出现。
        XCTAssertEqual(steps.map(\.kind), [.reset, .reset, .request, .request])
        XCTAssertEqual(steps[0].service, "ScreenCapture")
        XCTAssertEqual(steps[0].permissionID, "screen-recording")
        XCTAssertEqual(steps[2].permissionID, "screen-recording")
        XCTAssertNil(steps[2].service)
    }

    func testReauthorizeStepsIgnoreServicesOutsideTheReRequestList() {
        // 多清一项＝多毁一项用户手工给出的授权：不在 reRequest 里的服务不 reset。
        let plan = ReauthorizePlan(
            summary: "x", services: ["SystemPolicyAllFiles", "ScreenCapture"],
            reRequest: ["screen-recording"], bundleIdentifier: "com.aeroric.native.dev"
        )
        let steps = ReauthorizeSequence.steps(for: plan, confirmedByUser: true)
        XCTAssertEqual(steps.filter { $0.kind == .reset }.map(\.service), ["ScreenCapture"])
    }

    // MARK: - 项级判定

    func testUnknownIsNotDrawnAsGranted() {
        XCTAssertFalse(PermissionStatus.unknown.isGranted)
        XCTAssertFalse(PermissionStatus.notGranted.isGranted)
        XCTAssertTrue(PermissionStatus.granted.isGranted)
        // 文案两回事："探测不到" 与 "系统说没给" 要给用户不同的动作。
        XCTAssertNotEqual(PermissionStatus.unknown.label, PermissionStatus.notGranted.label)
    }

    func testReportOnlyEntriesGetNoActionButton() {
        // 只报告的能力（示例：Linux 用户组类）点了没反应，所以不给按钮。
        XCTAssertFalse(PermissionPresentation.showsRequestButton(
            reportOnly: true, canRequestInApp: true, canOpenSettings: true
        ))
        XCTAssertTrue(PermissionPresentation.showsRequestButton(
            reportOnly: false, canRequestInApp: true, canOpenSettings: false
        ))
        // 两者都不可（未知平台）⇒ 不给按钮。
        XCTAssertFalse(PermissionPresentation.showsRequestButton(
            reportOnly: false, canRequestInApp: false, canOpenSettings: false
        ))
    }

    func testRestartHintOnlyWhenTheSystemGrantedAndThisProcessDidNot() {
        XCTAssertTrue(PermissionPresentation.needsRestart(
            systemStatus: .granted, processStatus: .notGranted
        ))
        XCTAssertFalse(PermissionPresentation.needsRestart(
            systemStatus: .granted, processStatus: .granted
        ))
        // 系统还没给 ⇒ 不是"重启就好"，是"还没授权"。
        XCTAssertFalse(PermissionPresentation.needsRestart(
            systemStatus: .notGranted, processStatus: .notGranted
        ))
        XCTAssertFalse(PermissionPresentation.needsRestart(
            systemStatus: .unknown, processStatus: .granted
        ))
    }

    // MARK: - 表驱动映射（与宿主同表）

    func testTccServiceNamesMatchTheHostTable() {
        // 两处都要给 `tccutil` 传同一个字符串，对不上＝"点了重新授权但什么都没清"。
        XCTAssertEqual(PermissionPresentation.tccService(for: "screen-recording"), "ScreenCapture")
        XCTAssertEqual(PermissionPresentation.tccService(for: "accessibility"), "Accessibility")
        XCTAssertEqual(PermissionPresentation.tccService(for: "input-monitoring"), "ListenEvent")
        XCTAssertEqual(PermissionPresentation.tccService(for: "automation"), "AppleEvents")
        XCTAssertEqual(PermissionPresentation.tccService(for: "microphone"), "Microphone")
        XCTAssertEqual(PermissionPresentation.tccService(for: "camera"), "Camera")
        XCTAssertEqual(
            PermissionPresentation.tccService(for: "full-disk-access"), "SystemPolicyAllFiles"
        )
        XCTAssertNil(PermissionPresentation.tccService(for: "nope"))
    }

    func testSettingsAnchorsCoverEveryResettableService() {
        for id in ["screen-recording", "accessibility", "input-monitoring", "automation",
                   "microphone", "camera", "full-disk-access"] {
            let anchor = PermissionPresentation.settingsAnchor(for: id)
            XCTAssertNotNil(anchor, "\(id) 没有设置锚点")
            XCTAssertTrue(anchor?.hasPrefix("Privacy_") ?? false, "\(anchor ?? "")")
        }
        XCTAssertNil(PermissionPresentation.settingsAnchor(for: "nope"))
    }

    // MARK: - 报告装配

    func testReportCollectsOnlyGrantedResettableServices() {
        let snapshot = ProbeSnapshot(
            bundleIdentifier: "com.aeroric.native.dev",
            signature: "adhoc",
            stableAcrossUpdates: false,
            warning: "unstableSignature",
            permissions: [
                ProbeSnapshot.Row(
                    descriptor: PermissionDescriptor(
                        id: "screen-recording", canRequestInApp: true, canOpenSettings: true, canReset: true
                    ),
                    status: .granted
                ),
                ProbeSnapshot.Row(
                    descriptor: PermissionDescriptor(
                        id: "microphone", canRequestInApp: true, canOpenSettings: true, canReset: true
                    ),
                    status: .notGranted
                ),
                ProbeSnapshot.Row(
                    descriptor: PermissionDescriptor(
                        id: "full-disk-access", canOpenSettings: true, canReset: true
                    ),
                    status: .unknown, detail: "只能在系统设置里手工勾选"
                ),
            ]
        )
        let report = PermissionPresentation.report(platform: "macos", probe: snapshot)
        XCTAssertEqual(report.permissions.count, 3)
        XCTAssertEqual(report.permissions.map(\.status), [.granted, .notGranted, .unknown])
        XCTAssertEqual(report.permissions[2].detail, "只能在系统设置里手工勾选")
        XCTAssertEqual(report.identity.warning, "unstableSignature")
        // 只把**已授权**且可清的项列进 reset：清一个本来就没给的记录是白费。
        XCTAssertEqual(report.identity.plan?.reauthorize?.services, ["ScreenCapture"])
        // re-request 覆盖所有非 report-only 的项。
        XCTAssertEqual(
            report.identity.plan?.reauthorize?.reRequest,
            ["screen-recording", "microphone", "full-disk-access"]
        )
    }

    func testReportTracksWhoseIdentityItIs() {
        // native 客户端必须报自己的 bundle id：拿 Tauri 的去查会查到别人的记录。
        let snapshot = ProbeSnapshot(
            bundleIdentifier: "com.aeroric.native.dev", signature: "adhoc",
            stableAcrossUpdates: false, warning: "unstableSignature", permissions: []
        )
        let report = PermissionPresentation.report(platform: "macos", probe: snapshot)
        XCTAssertEqual(report.identity.bundleIdentifier, "com.aeroric.native.dev")
        XCTAssertEqual(report.identity.plan?.bundleIdentifier, "com.aeroric.native.dev")
        // 没有任何权限项时也仍然给出身份解释（身份与清单是两件事）。
        XCTAssertTrue(report.identity.plan?.headline.contains("失效") ?? false)
    }

    // MARK: - 探针的信号分类（纯函数部分）

    func testSignatureClassificationMatchesTheHostRules() {
        // linker-signed（0x20000）优先于 adhoc：两个标志位同时存在时，前者是更准确的
        // 解释（连 Info.plist 都没覆盖）。
        let (kind, stable, _) = MacOSPermissionProbe.classify(flags: 0x20000, team: nil)
        XCTAssertEqual(kind, "linker-signed")
        XCTAssertFalse(stable)

        let (adhoc, adhocStable, _) = MacOSPermissionProbe.classify(flags: 0x2, team: nil)
        XCTAssertEqual(adhoc, "adhoc")
        XCTAssertFalse(adhocStable, "ad-hoc 的授权绑当次构建")

        let (developer, developerStable, _) = MacOSPermissionProbe.classify(flags: 0, team: "TEAM123")
        XCTAssertEqual(developer, "developer-id")
        XCTAssertTrue(developerStable)

        // 有签名但没有 team：signature=signed（宿主同判）。
        let (signed, signedStable, _) = MacOSPermissionProbe.classify(flags: 0, team: nil)
        XCTAssertEqual(signed, "signed")
        XCTAssertTrue(signedStable)
    }

    func testEveryProbedServiceIsMappedToATccName() {
        // 探针的默认清单里每一项都必须有 TCC 名或明确不可清——否则 UI 会给出一个
        // 点了会抛 `unknownService` 的按钮。
        for descriptor in MacOSPermissionProbe.defaultDescriptors {
            if descriptor.canReset {
                XCTAssertNotNil(
                    PermissionPresentation.tccService(for: descriptor.id),
                    "\(descriptor.id) 标了可清但没有 TCC 服务名"
                )
            }
            if descriptor.canOpenSettings {
                XCTAssertNotNil(
                    PermissionPresentation.settingsAnchor(for: descriptor.id),
                    "\(descriptor.id) 标了可打开设置但没有锚点"
                )
            }
        }
    }
}
