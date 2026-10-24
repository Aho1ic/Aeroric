import AeroricUI
import AppKit
import Darwin
import ImageIO
import XCTest

/// Real-window acceptance for the task-evolution review sample.
///
/// Run ONLY through `pnpm native:ui-test`, after the stage-01 process ownership
/// and exclusive-desktop preconditions have been satisfied. Every case drives
/// real clicks, keys and scrolling and asserts on observed AX state — never on
/// source text or on a constant repeated by the view.
///
/// These cases never launch against a fallback or production data directory,
/// and they explicitly pin the legacy visual-review flag so an inherited value
/// cannot change which entry the app takes.
@MainActor
final class NativeEvolutionReviewUITests: XCTestCase {
    private enum ReviewTestError: Error {
        case unsafeRunnerEnvironment
        case missingElement(String)
        case reviewNotForeground
        case unexpectedEntry(String)
    }

    private static let flagKey = "AERORIC_NATIVE_EVOLUTION_REVIEW"
    private static let legacyFlagKey = "AERORIC_NATIVE_VISUAL_REVIEW"

    // MARK: - 1. Role bindings are independent and offline

    func testRoleBindingsAreIndependentAndOffline() throws {
        let (app, directory) = try launchReview()
        defer { app.terminate() }
        try assertIsolated(app, directory: directory)

        try expectValue(app, "evolutionReview.offlineBadge", "离线审阅 · 非真实任务")
        try expectValue(app, "evolutionReview.isolated", "仅本地样例 · 无宿主、Agent 或 PTY")
        try element(app, "evolutionReview.taskName")
        try expectValue(app, "evolutionReview.governance.processNote",
                        "独立进程为产品设计要求；本样例未启动进程。三个角色标签不代表三个独立进程。")

        let reflectionBefore = value(app, "evolutionReview.role.reflection.binding")
        let governanceBefore = value(app, "evolutionReview.role.governance.binding")
        XCTAssertTrue(reflectionBefore.contains("继承模板"), reflectionBefore)
        XCTAssertTrue(governanceBefore.contains("继承模板"), governanceBefore)
        XCTAssertEqual(value(app, "evolutionReview.role.mainExecutor.template"),
                       "示例连接A · 示例模型1")

        // Open the role form and override only the main executor.
        try element(app, "evolutionReview.roleConfig.open").click()
        try element(app, "evolutionReview.roleConfig.template.mainExecutor")
        try expectValue(app, "evolutionReview.roleConfig.template.governance", "示例连接B · 示例模型2")

        try click(app, "evolutionReview.roleConfig.mainExecutor.override")
        try chooseMenu(app, "evolutionReview.roleConfig.mainExecutor.connection", title: "示例连接B")
        try chooseMenu(app, "evolutionReview.roleConfig.mainExecutor.model", title: "示例模型2")

        // The other two roles' rows must be untouched while the form is open.
        try expectValue(app, "evolutionReview.role.reflection.binding", reflectionBefore)
        try expectValue(app, "evolutionReview.role.governance.binding", governanceBefore)

        try element(app, "evolutionReview.roleConfig.apply").click()
        try expectValue(app, "evolutionReview.role.mainExecutor.binding", "覆盖 · 示例连接B · 示例模型2")
        try expectValue(app, "evolutionReview.role.reflection.binding", reflectionBefore)
        try expectValue(app, "evolutionReview.role.governance.binding", governanceBefore)
        try expectValuePrefix(app, "evolutionReview.notice", "仅本次样例，未保存真实配置")

        // Cancel must apply nothing.
        try element(app, "evolutionReview.roleConfig.open").click()
        try click(app, "evolutionReview.roleConfig.governance.override")
        try chooseMenu(app, "evolutionReview.roleConfig.governance.model", title: "示例模型1")
        try element(app, "evolutionReview.roleConfig.cancel").click()
        try expectValue(app, "evolutionReview.role.governance.binding", governanceBefore)
        try expectValuePrefix(app, "evolutionReview.notice", "已取消角色配置草稿")

        // Restore inheritance for the main executor only.
        try element(app, "evolutionReview.roleConfig.open").click()
        try click(app, "evolutionReview.roleConfig.mainExecutor.restore")
        try element(app, "evolutionReview.roleConfig.apply").click()
        try expectValue(app, "evolutionReview.role.mainExecutor.binding", "继承模板 · 示例连接A · 示例模型1")
        try expectValue(app, "evolutionReview.role.reflection.binding", reflectionBefore)
        try expectValue(app, "evolutionReview.role.governance.binding", governanceBefore)

        // Reset must discard an applied override, not merely an untouched form.
        try element(app, "evolutionReview.roleConfig.open").click()
        try click(app, "evolutionReview.roleConfig.mainExecutor.override")
        try chooseMenu(app, "evolutionReview.roleConfig.mainExecutor.connection", title: "示例连接B")
        try element(app, "evolutionReview.roleConfig.apply").click()
        try expectValue(app, "evolutionReview.role.mainExecutor.binding", "覆盖 · 示例连接B · 示例模型1")
        try element(app, "evolutionReview.reset").click()
        try expectValuePrefix(app, "evolutionReview.notice", "已重置样例状态")
        try expectValue(app, "evolutionReview.role.mainExecutor.binding", "继承模板 · 示例连接A · 示例模型1")
        try expectValue(app, "evolutionReview.role.reflection.binding", reflectionBefore)
        try expectValue(app, "evolutionReview.role.governance.binding", governanceBefore)
        try assertIsolated(app, directory: directory)
    }

    // MARK: - 2. Ordinary phase gate and candidate gate differ

    func testOrdinaryPhaseAndCandidateGatesDiffer() throws {
        let (app, directory) = try launchReview()
        defer { app.terminate() }
        try assertIsolated(app, directory: directory)

        // S2: the ordinary phase closes without a governance signature.
        try chooseScenario(app, code: "S2", title: "普通阶段通过、治理待审")
        try expectValue(app, "evolutionReview.phase.state", "已通过")
        try expectValue(app, "evolutionReview.phase.governance", "治理待审")
        try expectDependencyState(app, id: "dep.verify.downstream", state: "等待该结论")
        try click(app, "evolutionReview.action.advanceDependents")
        try expectValuePrefix(app, "evolutionReview.notice", "依赖已推进（样例）")
        try expectDependencyState(app, id: "dep.verify.downstream", state: "已解除限制")
        try expectValue(app, "evolutionReview.phase.governance", "治理待审")

        // S3: a passed ordinary phase does not make a review-less candidate triallable.
        try chooseScenario(app, code: "S3", title: "工具候选缺必要评审")
        try expectValue(app, "evolutionReview.phase.state", "已通过")
        try chooseTrack(app, title: "能力改进")
        try element(app, "evolutionReview.action.addCandidateReview")
        try expectValue(app, "evolutionReview.candidate.trial", "未启用")
        try expectValue(app, "evolutionReview.candidate.gate", "缺少同版本必要评审或既有审批，不能试用")
        XCTAssertFalse(try element(app, "evolutionReview.action.trialCandidate").isEnabled)
        try element(app, "evolutionReview.action.trialCandidate.reason")
        try expectValuePrefix(app, "evolutionReview.candidate.cand.export.v2.gateReason",
                              "缺少同版本必要评审或既有审批")

        try click(app, "evolutionReview.action.addCandidateReview")
        try expectValuePrefix(app, "evolutionReview.notice", "已补入 v2 的同版本必要评审与既有审批")
        try expectValue(app, "evolutionReview.candidate.gate", "可试用或无候选")
        try expectValue(app, "evolutionReview.phase.governance", "治理已复审")
        try click(app, "evolutionReview.opinion.op.tool.review.missing")
        try expectValue(app, "evolutionReview.detail.field.状态", "历史意见（已解决）")
        try closeDetail(app)
        try click(app, "evolutionReview.action.trialCandidate")
        try expectValuePrefix(app, "evolutionReview.notice", "候选 v2 进入受控试用（样例）")
        try expectValue(app, "evolutionReview.candidate.trial", "试用中（样例）")

        // S7: style preferences do not block an ordinary phase; the same
        // scenario still refuses to approve v3 using the old v2 review.
        try chooseScenario(app, code: "S7", title: "版本不匹配 / 风格建议")
        try chooseTrack(app, title: "项目交付")
        try expectValue(app, "evolutionReview.phase.state", "已通过")
        XCTAssertFalse(try element(app, "evolutionReview.action.reopenPhase").isEnabled)
        try click(app, "evolutionReview.opinion.op.style.rounded")
        try expectValue(app, "evolutionReview.detail.field.风格偏好", "是（不自动阻断阶段）")
        try closeDetail(app)
        try expectValue(app, "evolutionReview.phase.state", "已通过")
        try chooseTrack(app, title: "能力改进")
        try expectValue(app, "evolutionReview.candidate.gate", "现有评审针对 v2，不能批准当前版本 v3")
        XCTAssertFalse(try element(app, "evolutionReview.action.trialCandidate").isEnabled)
        try click(app, "evolutionReview.evidence.ev.tool.review.v2")
        try expectValue(app, "evolutionReview.detail.field.版本", "v2")
        try closeDetail(app)
        try click(app, "evolutionReview.action.addCandidateReview")
        try expectValuePrefix(app, "evolutionReview.notice", "已补入 v3 的同版本必要评审与既有审批")
        try click(app, "evolutionReview.action.trialCandidate")
        try expectValue(app, "evolutionReview.candidate.trial", "试用中（样例）")
        try assertIsolated(app, directory: directory)
    }

    // MARK: - 3. Late finding keeps history and scopes dependencies

    func testLateFindingKeepsHistoryAndScopesDependencies() throws {
        let (app, directory) = try launchReview()
        defer { app.terminate() }
        try assertIsolated(app, directory: directory)

        // S1: the interface-dependent path waits while the independent path
        // continues. These are observed state leaves, not explanatory prose.
        try expectDependencyState(app, id: "dep.interface.integration", state: "等待该结论")
        try expectDependencyState(app, id: "dep.interface.docs", state: "可继续")

        try chooseScenario(app, code: "S5", title: "迟到实质漏检")
        try expectValue(app, "evolutionReview.phase.state", "已通过")
        try element(app, "evolutionReview.evidence.ev.delivery.pass.v20")
        try element(app, "evolutionReview.evidence.ev.delivery.downstream")

        try click(app, "evolutionReview.action.reopenPhase")
        try expectValue(app, "evolutionReview.phase.state", "局部重开")
        // The original pass record and the downstream results are still there.
        try element(app, "evolutionReview.evidence.ev.delivery.pass.v20")
        try element(app, "evolutionReview.evidence.ev.delivery.downstream")
        try element(app, "evolutionReview.opinion.op.late.finding")
        XCTAssertFalse(try element(app, "evolutionReview.action.releaseRestriction").isEnabled)
        try expectValuePrefix(app, "evolutionReview.action.releaseRestriction.reason", "仍有未满足的整改要求")
        try expectValue(app, "evolutionReview.phase.version", "v2.0")

        // Only the actually affected dependency is restricted.
        try expectDependencyState(app, id: "dep.export.integration", state: "受限")
        try expectDependencyState(app, id: "dep.export.docs", state: "可继续")

        // Without pause evidence the sample shows still-executing, never stopped.
        try expectDependencyState(app, id: "dep.export.worker", state: "仍在执行 / 待收束")
        try click(app, "evolutionReview.action.completeRemediation")
        try expectValue(app, "evolutionReview.phase.version", "v2.1")
        try click(app, "evolutionReview.evidence.ev.remediation.targeted.review")
        try expectValue(app, "evolutionReview.detail.field.版本", "v2.1")
        try closeDetail(app)
        try click(app, "evolutionReview.action.releaseRestriction")
        try expectValue(app, "evolutionReview.phase.state", "已通过")
        try expectDependencyState(app, id: "dep.export.integration", state: "已解除限制")
        try expectDependencyState(app, id: "dep.export.docs", state: "可继续")
        try expectDependencyState(app, id: "dep.export.worker", state: "仍在执行 / 待收束")
        try element(app, "evolutionReview.evidence.ev.delivery.pass.v20")
        try element(app, "evolutionReview.evidence.ev.delivery.downstream")
        try click(app, "evolutionReview.opinion.op.late.finding")
        try expectValue(app, "evolutionReview.detail.field.状态", "历史意见（已解决）")
        try closeDetail(app)

        // S6: the incomplete targeted review still holds the restriction.
        // Looking at a user dispute cannot replace a candidate's review.
        try chooseScenario(app, code: "S6", title: "漏检整改 / 争议")
        try expectValue(app, "evolutionReview.phase.version", "v2.1")
        XCTAssertFalse(try element(app, "evolutionReview.action.releaseRestriction").isEnabled)
        try expectDependencyState(app, id: "dep.export.integration", state: "受限")
        try chooseTrack(app, title: "能力改进")
        XCTAssertFalse(try element(app, "evolutionReview.action.trialCandidate").isEnabled)
        try click(app, "evolutionReview.dispute.open")
        try expectValue(app, "evolutionReview.detail.title", "Q24 用户争议裁决示例")
        try expectValue(app, "evolutionReview.detail.field.约束", "不替代候选必需评审或既定验证")
        try closeDetail(app)
        XCTAssertFalse(try element(app, "evolutionReview.action.trialCandidate").isEnabled)
        try expectValue(app, "evolutionReview.candidate.trial", "未启用")
        try chooseTrack(app, title: "项目交付")
        try click(app, "evolutionReview.action.completeRemediation")
        try click(app, "evolutionReview.action.releaseRestriction")
        try expectDependencyState(app, id: "dep.export.integration", state: "已解除限制")
        try assertIsolated(app, directory: directory)
    }

    // MARK: - 4. Service failure and tool failure stay distinct

    func testServiceAndToolFailuresRemainDistinct() throws {
        let (app, directory) = try launchReview()
        defer { app.terminate() }
        try assertIsolated(app, directory: directory)

        // S4: supervision failure is recorded as failed/pending; nothing is
        // silently substituted and the review-less candidate stays disabled.
        try chooseScenario(app, code: "S4", title: "监督 API 失败 / 超时")
        try expectValuePrefix(app, "evolutionReview.supervision", "监督 API 调用失败")
        try expectValue(app, "evolutionReview.candidate.trial", "未启用")
        XCTAssertFalse(try element(app, "evolutionReview.action.trialCandidate").isEnabled)
        try chooseTrack(app, title: "项目交付")
        try expectValue(app, "evolutionReview.phase.selected", "无")
        try expectValue(app, "evolutionReview.candidate.trial", "无候选")
        XCTAssertFalse(app.buttons["evolutionReview.action.recoverSupervision"].exists)
        try chooseTrack(app, title: "能力改进")
        try click(app, "evolutionReview.action.recoverSupervision")
        try expectValuePrefix(app, "evolutionReview.notice", "已载入 v2 的新评审示例（样例）")
        try expectValue(app, "evolutionReview.candidate.trial", "未启用")

        // S8: a simulated tool runtime failure (not an API failure) disables the
        // candidate and falls back; project results are preserved.
        try chooseScenario(app, code: "S8", title: "工具试用运行故障")
        try expectValuePrefix(app, "evolutionReview.supervision", "监督通道正常")
        try expectValue(app, "evolutionReview.candidate.trial", "试用中（样例）")
        XCTAssertFalse(app.buttons["evolutionReview.evidence.ev.trial.failure.v2"].exists)
        XCTAssertFalse(app.buttons["evolutionReview.opinion.op.trial.failure"].exists)
        try click(app, "evolutionReview.action.trialFailureFallback")
        try expectValuePrefix(app, "evolutionReview.notice", "候选 v2 已停用并回退（样例）")
        try expectValue(app, "evolutionReview.candidate.trial", "已回退到获准安全工具")
        try element(app, "evolutionReview.evidence.ev.trial.failure.v2")
        try element(app, "evolutionReview.opinion.op.trial.failure")
        try click(app, "evolutionReview.candidate.cand.export.v2")
        try expectValue(app, "evolutionReview.detail.field.试用状态", "已回退到获准安全工具")
        try expectValuePrefix(app, "evolutionReview.detail.body.first", "候选 v2 运行故障，已停用")
        try closeDetail(app)
        XCTAssertFalse(value(app, "evolutionReview.notice").contains("已撤销"))
        // The project delivery track still carries its results.
        try chooseTrack(app, title: "项目交付")
        try expectValue(app, "evolutionReview.phase.state", "已通过")
        try element(app, "evolutionReview.evidence.ev.delivery.v22")
        try chooseTrack(app, title: "能力改进")
        try assertIsolated(app, directory: directory)
    }

    // MARK: - 5. Native appearance, keyboard and compact layout

    func testNativeAppearanceKeyboardAndCompactLayout() throws {
        let (app, directory) = try launchReview()
        defer { app.terminate() }
        try assertIsolated(app, directory: directory)

        // The real system values are reported and the effective values match
        // them before any injection.
        for key in ["reduceTransparency", "increaseContrast", "reduceMotion"] {
            let actual = value(app, "evolutionReview.system.\(key)")
            XCTAssertTrue(["on", "off"].contains(actual), key)
            try expectValue(app, "evolutionReview.effective.\(key)", actual)
        }
        let systemReducedTransparency = value(app, "evolutionReview.system.reduceTransparency") == "on"
        try capture(app, name: "system-default")

        // Light and dark, glass and classic, at each review sample size.
        for (appearance, expected) in [("浅色", "light"), ("深色", "dark")] {
            try chooseSegment(app, title: appearance)
            try expectValue(app, "evolutionReview.effective.appearance", expected)
            for surface in ["glass", "classic"] {
                try chooseSegment(app, title: surface)
                try expectValue(app, "evolutionReview.effective.surface",
                                systemReducedTransparency ? "classic" : surface)
                for (title, budget, width, height) in [
                    ("1260×860", "1260 → 320/586/320", 1260.0, 860.0),
                    ("1000×640", "1000 → 284/409/273", 1000.0, 640.0),
                    ("400×600", "400 → 382", 400.0, 600.0),
                ] {
                    try chooseSegment(app, title: title)
                    try expectValue(app, "evolutionReview.sample.budget", budget)
                    try expectValue(app, "evolutionReview.detail.visible", "无")
                    try assertMeasuredGeometry(app, width: width, height: height, inspectorVisible: false)
                    try capture(app, name: "\(expected)-\(surface)-\(Int(width))")
                }
            }
        }

        // Narrow windows go vertical and open details on demand.
        try chooseSegment(app, title: "400×600")
        try expectValue(app, "evolutionReview.layout.compact", "是")
        try expectValue(app, "evolutionReview.measured.sidebar", "absent")
        try expectValue(app, "evolutionReview.measured.inspector", "absent")
        try element(app, "evolutionReview.phase.menu")
        // S1's interface phase has feedback, not a tool candidate.
        try click(app, "evolutionReview.action.sendReflectionFeedback")
        try expectValuePrefix(app, "evolutionReview.notice", "反思反馈已送出（样例）")
        try click(app, "evolutionReview.evidence.ev.interface.draft")
        try expectValue(app, "evolutionReview.detail.visible", "有")
        XCTAssertTrue(try element(app, "evolutionReview.detailSheet.close").isHittable)
        try expectValuePrefix(app, "evolutionReview.detail.body.first", "虚构接口草案 v0.3")
        try capture(app, name: "compact-detail-open")
        try closeDetail(app)

        // The narrow role sheet must keep the native form and its footer
        // reachable without widening the 400pt parent.
        try click(app, "evolutionReview.roleConfig.open")
        try click(app, "evolutionReview.roleConfig.mainExecutor.override")
        try chooseMenu(app, "evolutionReview.roleConfig.mainExecutor.connection", title: "示例连接B")
        try chooseMenu(app, "evolutionReview.roleConfig.mainExecutor.model", title: "示例模型2")
        XCTAssertTrue(try element(app, "evolutionReview.roleConfig.cancel").isHittable)
        XCTAssertTrue(try element(app, "evolutionReview.roleConfig.apply").isHittable)
        try capture(app, name: "compact-role-form")
        try click(app, "evolutionReview.roleConfig.apply")
        try expectValue(app, "evolutionReview.role.mainExecutor.binding", "覆盖 · 示例连接B · 示例模型2")
        try assertMeasuredGeometry(app, width: 400, height: 600, inspectorVisible: false)
        try click(app, "evolutionReview.reset")

        try chooseSegment(app, title: "1260×860")
        try expectValue(app, "evolutionReview.layout.compact", "否")
        try expectValue(app, "evolutionReview.detail.visible", "无")
        try assertMeasuredGeometry(app, width: 1260, height: 860, inspectorVisible: false)
        // Verify the correct opt-in inspector geometry separately from the
        // default closed layout. This exercises the real transition policy.
        try exerciseInspectorTransition(app)

        // classic is an opaque surface; the system preference always wins.
        try chooseSegment(app, title: "classic")
        try expectValue(app, "evolutionReview.effective.surface", "classic")
        try capture(app, name: "classic-opaque")
        try chooseSegment(app, title: "glass")
        try expectValue(app, "evolutionReview.effective.surface",
                        systemReducedTransparency ? "classic" : "glass")

        // Injection strengthens accessibility and never changes the system value.
        try element(app, "evolutionReview.inject.reduceTransparency").click()
        try expectValue(app, "evolutionReview.effective.surface", "classic")
        try element(app, "evolutionReview.inject.increaseContrast").click()
        try expectValue(app, "evolutionReview.effective.outline", "1.0")
        try element(app, "evolutionReview.inject.reduceMotion").click()
        try expectValue(app, "evolutionReview.effective.motionDuration", "0.000")
        try exerciseInspectorTransition(app)
        try capture(app, name: "injected-accessibility")
        XCTAssertEqual(value(app, "evolutionReview.system.reduceTransparency"),
                       systemReducedTransparency ? "on" : "off")
        try element(app, "evolutionReview.inject.reduceTransparency").click()
        try element(app, "evolutionReview.inject.increaseContrast").click()
        try element(app, "evolutionReview.inject.reduceMotion").click()

        // Keyboard: Tab reaches the reset control, the phase list moves with
        // the arrow keys, and Escape cancels the role form.
        try activateReview(app)
        for _ in 0..<40 {
            if value(app, "evolutionReview.focus.control") == "reset" { break }
            app.typeKey(.tab, modifierFlags: [])
        }
        try expectValue(app, "evolutionReview.focus.control", "reset")

        try element(app, "evolutionReview.phase.phase.interface")
        let row = app.outlineRows.containing(.staticText, identifier: "evolutionReview.phase.phase.interface")
            .firstMatch
        XCTAssertTrue(row.exists)
        try activateReview(app)
        row.click()
        try activateReview(app)
        try expectValue(app, "evolutionReview.phase.selected", "接口契约联调")
        app.typeKey(.downArrow, modifierFlags: [])
        try expectValue(app, "evolutionReview.phase.selected", "独立文档整理")

        try element(app, "evolutionReview.roleConfig.open").click()
        try element(app, "evolutionReview.roleConfig.apply")
        app.typeKey(.escape, modifierFlags: [])
        try expectValuePrefix(app, "evolutionReview.notice", "已取消角色配置草稿")

        // Open genuine embedded long evidence and scroll its own native
        // container. Existence or a summary readout is not visibility proof.
        try activateReview(app)
        row.click()
        try expectValue(app, "evolutionReview.phase.selected", "接口契约联调")
        try click(app, "evolutionReview.evidence.ev.interface.draft")
        let firstParagraph = try element(app, "evolutionReview.detail.body.first")
        let lastParagraph = try element(app, "evolutionReview.detail.body.last")
        let scroll = try owningScrollView(app, identifier: "evolutionReview.detail.body.last")
        XCTAssertTrue(isVisible(firstParagraph, in: scroll), "the embedded evidence must start at its first paragraph")
        XCTAssertFalse(isVisible(lastParagraph, in: scroll),
                       "the long evidence's last paragraph must initially be outside the viewport")
        try capture(app, name: "long-evidence-before-scroll")
        for _ in 0..<40 {
            if isVisible(lastParagraph, in: scroll) { break }
            try activateReview(app)
            scroll.scroll(byDeltaX: 0, deltaY: -220)
        }
        XCTAssertTrue(isVisible(lastParagraph, in: scroll),
                      "the real last paragraph must enter the evidence viewport by scrolling")
        try expectValue(app, "evolutionReview.detail.body.last",
                        "内嵌证据末段：本页全部为虚构接口说明，未读取本机文件，未执行工具。")
        XCTAssertFalse(isVisible(firstParagraph, in: scroll))
        try capture(app, name: "scrolled-evidence")
        try closeDetail(app)
        try assertIsolated(app, directory: directory)
    }

    // MARK: - 6. An invalid review request never falls into the product

    func testInvalidReviewRequestNeverFallsIntoProduct() throws {
        let environment = ProcessInfo.processInfo.environment
        guard environment["AERORIC_ENV"] == "ui-test",
              let directory = environment["AERORIC_DATA_DIR"] else {
            throw ReviewTestError.unsafeRunnerEnvironment
        }
        let accountHome = try accountHomeDirectory()
        guard NativeVisualReviewEntry.isIsolatedDataDirectory(directory, home: accountHome) else {
            throw ReviewTestError.unsafeRunnerEnvironment
        }
        let root = URL(fileURLWithPath: directory).standardizedFileURL.resolvingSymlinksInPath()
        let scratch = root.appendingPathComponent("evolution-refusal-\(UUID().uuidString)", isDirectory: true)
        try assertDirectoryAbsent(scratch)

        // Wrong environment: refused, not the product.
        let wrongEnvironment = try launch(
            dataDirectory: scratch,
            environment: "default",
            evolutionFlag: "1",
            legacyFlag: "0"
        )
        defer { wrongEnvironment.terminate() }
        try assertRefused(wrongEnvironment, reason: EvolutionReviewRefusalReason.environmentNotUITest.rawValue)
        try assertNoProductSurface(wrongEnvironment)
        wrongEnvironment.terminate()
        try assertDirectoryAbsent(scratch)

        // Both review flags at once: refused explicitly, not resolved by order.
        let conflicting = try launch(
            dataDirectory: scratch,
            environment: "ui-test",
            evolutionFlag: "1",
            legacyFlag: "1"
        )
        defer { conflicting.terminate() }
        try assertRefused(conflicting, reason: EvolutionReviewRefusalReason.legacyVisualReviewAlsoRequested.rawValue)
        try assertNoProductSurface(conflicting)
        conflicting.terminate()

        // Unsafe directory shapes are covered by the pure entry-policy tests.
        // Never launch the app with one: a regression in refusal handling must
        // not expose account data before a later AX assertion can fail.
        try assertDirectoryAbsent(scratch)
    }

    // MARK: - Refusal assertions

    private func assertRefused(_ app: XCUIApplication, reason: String) throws {
        try expectValue(app, "evolutionReview.refusal.title", "审阅未启动")
        try expectValue(app, "evolutionReview.refusal.explanation",
                        "未显示样例，未回落到产品工作区，未启动宿主")
        try element(app, "evolutionReview.refusal.reason.\(reason)")
        try expectValue(app, "evolutionReview.refusal.isolation", "只读 · 不启动宿主、不写真实配置")
        XCTAssertNotNil(Int32(value(app, "evolutionReview.process.pid")))
        // The refusal page must not claim the sample started.
        XCTAssertFalse(app.descendants(matching: .any)["evolutionReview.offlineBadge"].exists)
        XCTAssertFalse(app.descendants(matching: .any)["evolutionReview.taskName"].exists)
    }

    /// The product shell and the host must never appear. Absence of these
    /// identifiers is corroborating evidence only — the source-level branch and
    /// the process/directory checks in the report carry the real proof.
    private func assertNoProductSurface(_ app: XCUIApplication) throws {
        XCTAssertFalse(app.descendants(matching: .any)["bootstrap.environment"].exists)
        XCTAssertFalse(app.descendants(matching: .any)["sidebar.list"].exists)
        XCTAssertFalse(app.descendants(matching: .any)["statusbar.phase"].exists)
    }

    // MARK: - Launch helpers

    private func launchReview() throws -> (XCUIApplication, URL) {
        let environment = ProcessInfo.processInfo.environment
        guard environment["AERORIC_ENV"] == "ui-test",
              let directory = environment["AERORIC_DATA_DIR"] else {
            throw ReviewTestError.unsafeRunnerEnvironment
        }
        // XCTest's runner is sandboxed: NSHomeDirectory() is its container, not
        // the account home used by the unsandboxed app. Resolve the account
        // independently and reuse the app's own path policy.
        let accountHome = try accountHomeDirectory()
        guard NativeVisualReviewEntry.isIsolatedDataDirectory(directory, home: accountHome) else {
            throw ReviewTestError.unsafeRunnerEnvironment
        }
        print("EVOLUTION-REVIEW-RUNNER-PATHS runnerHome=\(NSHomeDirectory()) accountHome=\(accountHome) dataDirectory=\(directory)")
        let root = URL(fileURLWithPath: directory).standardizedFileURL.resolvingSymlinksInPath()
        let scratch = root.appendingPathComponent("evolution-\(UUID().uuidString)", isDirectory: true)
        // The sandboxed runner may read but must not gain write access to the
        // account's Caches. This fixture needs no directory at all, so the
        // unique child must stay absent before and after use.
        try assertDirectoryAbsent(scratch)

        let app = try launch(dataDirectory: scratch, environment: "ui-test",
                             evolutionFlag: "1", legacyFlag: "0")
        do {
            _ = try element(app, "evolutionReview.offlineBadge")
            try activateReview(app)
        } catch {
            try? capture(app, name: "launch-failure")
            app.terminate()
            throw error
        }
        return (app, scratch)
    }

    private func launch(dataDirectory: URL,
                        environment: String,
                        evolutionFlag: String,
                        legacyFlag: String) throws -> XCUIApplication {
        let accountHome = try accountHomeDirectory()
        guard NativeVisualReviewEntry.isIsolatedDataDirectory(dataDirectory.path, home: accountHome) else {
            throw ReviewTestError.unsafeRunnerEnvironment
        }
        try assertDirectoryAbsent(dataDirectory)
        let app = XCUIApplication()
        // Pin every entry-relevant variable explicitly so an inherited value
        // cannot decide which branch the app takes.
        app.launchEnvironment["AERORIC_ENV"] = environment
        app.launchEnvironment["AERORIC_DATA_DIR"] = dataDirectory.path
        app.launchEnvironment[Self.flagKey] = evolutionFlag
        app.launchEnvironment[Self.legacyFlagKey] = legacyFlag
        app.launch()
        app.activate()
        return app
    }

    private func accountHomeDirectory() throws -> String {
        // Reentrant lookup: do not retain getpwuid's shared static storage.
        // Lookup failure (including an unexpectedly large record) fails closed.
        var record = passwd()
        var result: UnsafeMutablePointer<passwd>?
        var buffer = [CChar](repeating: 0, count: 16_384)
        return try buffer.withUnsafeMutableBufferPointer { bytes in
            guard let base = bytes.baseAddress,
                  getpwuid_r(getuid(), &record, base, bytes.count, &result) == 0,
                  result != nil, let home = record.pw_dir else {
                throw ReviewTestError.unsafeRunnerEnvironment
            }
            return String(cString: home)
        }
    }

    private func assertIsolated(_ app: XCUIApplication, directory: URL) throws {
        XCTAssertFalse(app.descendants(matching: .any)["bootstrap.environment"].exists)
        XCTAssertFalse(app.descendants(matching: .any)["sidebar.list"].exists)
        try assertDirectoryAbsent(directory)
    }

    private func assertDirectoryAbsent(_ directory: URL) throws {
        // Read the existing parent instead of treating fileExists == false as
        // proof: permission errors and dangling links must not look green.
        let entries = try FileManager.default.contentsOfDirectory(
            atPath: directory.deletingLastPathComponent().path
        )
        guard !entries.contains(directory.lastPathComponent) else {
            XCTFail("the review sample must not create even its data directory, let alone runtime/host/task/session artifacts")
            throw ReviewTestError.unsafeRunnerEnvironment
        }
    }

    // MARK: - Element helpers

    @discardableResult
    private func element(_ app: XCUIApplication, _ identifier: String) throws -> XCUIElement {
        let element = app.descendants(matching: .any).matching(identifier: identifier).firstMatch
        guard element.waitForExistence(timeout: 15) else {
            XCTFail("Missing AX leaf: \(identifier)")
            throw ReviewTestError.missingElement(identifier)
        }
        return element
    }

    /// Real click, scrolling the owning scroll view first when the target sits
    /// below the fold. Long evidence and long phase content must be reachable
    /// by native scrolling, and a test that only clicks what happens to be
    /// visible would not prove that.
    private func click(_ app: XCUIApplication,
                       _ identifier: String,
                       file: StaticString = #filePath,
                       line: UInt = #line) throws {
        let target = try element(app, identifier)
        try reveal(app, target: target, identifier: identifier, file: file, line: line)
        try activateReview(app)
        target.click()
    }

    private func reveal(_ app: XCUIApplication, target: XCUIElement, identifier: String,
                        file: StaticString = #filePath, line: UInt = #line) throws {
        let owners = app.scrollViews.containing(.any, identifier: identifier).allElementsBoundByIndex
            .filter { !$0.frame.isEmpty }
        guard let owner = owners.min(by: {
            $0.frame.width * $0.frame.height < $1.frame.width * $1.frame.height
        }) else {
            if target.isHittable { return }
            XCTFail("No owning scroll view for \(identifier)", file: file, line: line)
            throw ReviewTestError.missingElement(identifier)
        }
        for attempt in 0..<30 {
            if isVisible(target, in: owner) { break }
            let frame = target.frame
            let viewport = owner.frame
            // Reopening a detail or changing a phase may leave a target
            // above the viewport. Never scroll an unrelated global view.
            let upwards = frame.isEmpty ? attempt >= 15 : frame.minY < viewport.minY
            let delta = min(80, max(16, viewport.height * 0.45))
            try activateReview(app)
            owner.scroll(byDeltaX: 0, deltaY: upwards ? delta : -delta)
        }
        guard isVisible(target, in: owner) && target.isHittable else {
            XCTFail("\(identifier) must be reachable by scrolling", file: file, line: line)
            throw ReviewTestError.missingElement(identifier)
        }
    }

    private func owningScrollView(_ app: XCUIApplication, identifier: String) throws -> XCUIElement {
        // The smallest enclosing viewport is the nearest native scroll owner.
        // This remains scoped even if AppKit exposes additional ancestors.
        let owners = app.scrollViews.containing(.any, identifier: identifier).allElementsBoundByIndex
            .filter { !$0.frame.isEmpty }
        guard let owner = owners.min(by: {
            $0.frame.width * $0.frame.height < $1.frame.width * $1.frame.height
        }) else {
            XCTFail("No owning scroll view for \(identifier)")
            throw ReviewTestError.missingElement(identifier)
        }
        return owner
    }

    private func isVisible(_ target: XCUIElement, in scroll: XCUIElement) -> Bool {
        let frame = target.frame
        let viewport = scroll.frame
        return target.isHittable && !frame.isEmpty
            && frame.minY >= viewport.minY - 1 && frame.maxY <= viewport.maxY + 1
            && frame.minX >= viewport.minX - 1 && frame.maxX <= viewport.maxX + 1
    }

    private func value(_ app: XCUIApplication, _ identifier: String) -> String {
        let element = app.descendants(matching: .any).matching(identifier: identifier).firstMatch
        return (element.value as? String) ?? element.label
    }

    private func expectValue(_ app: XCUIApplication, _ identifier: String, _ expected: String) throws {
        let element = try element(app, identifier)
        XCTAssertFalse(element.label.isEmpty, "\(identifier) must have a readable name")
        let predicate = NSPredicate(format: "value == %@ OR label == %@", expected, expected)
        XCTAssertEqual(XCTWaiter.wait(
            for: [XCTNSPredicateExpectation(predicate: predicate, object: element)], timeout: 5
        ), .completed, "\(identifier) expected \(expected), got \(value(app, identifier))")
    }

    private func expectValuePrefix(_ app: XCUIApplication, _ identifier: String, _ prefix: String) throws {
        _ = try element(app, identifier)
        let predicate = NSPredicate { [self] _, _ in
            value(app, identifier).hasPrefix(prefix)
        }
        XCTAssertEqual(XCTWaiter.wait(
            for: [XCTNSPredicateExpectation(predicate: predicate, object: app)], timeout: 5
        ), .completed, "\(identifier) expected prefix \(prefix), got \(value(app, identifier))")
    }

    /// System segmented pickers expose their segments as named radio buttons.
    private func chooseSegment(_ app: XCUIApplication, title: String) throws {
        try activateReview(app)
        let predicate = NSPredicate(format: "label == %@ OR title == %@", title, title)
        let segment = app.radioButtons.matching(predicate).firstMatch
        guard segment.waitForExistence(timeout: 5) else {
            XCTFail("System segment is not independently addressable: \(title)")
            throw ReviewTestError.missingElement(title)
        }
        segment.click()
    }

    /// Project delivery and capability improvement are separate tracks; the
    /// sample switches between them through the shared segmented picker.
    private func chooseTrack(_ app: XCUIApplication, title: String) throws {
        try chooseSegment(app, title: title)
    }

    /// System menu pickers expose a pop-up button plus a menu of items. The
    /// control is looked up across the element types SwiftUI may use so the
    /// test does not depend on one rendering.
    private func chooseScenario(_ app: XCUIApplication, code: String, title: String) throws {
        try activateReview(app)
        let control = try menuControl(app, "evolutionReview.scenario.menu")
        control.click()
        let item = app.menuItems.matching(
            NSPredicate(format: "label BEGINSWITH %@ OR title BEGINSWITH %@", code, code)
        ).firstMatch
        guard item.waitForExistence(timeout: 5) else {
            XCTFail("Scenario menu item not addressable: \(code)")
            throw ReviewTestError.missingElement(code)
        }
        item.click()
        try expectValuePrefix(app, "evolutionReview.scenario.current", "\(code) \(title)")
    }

    private func chooseMenu(_ app: XCUIApplication, _ identifier: String, title: String) throws {
        try activateReview(app)
        let control = try menuControl(app, identifier)
        try reveal(app, target: control, identifier: identifier)
        control.click()
        let item = app.menuItems.matching(
            NSPredicate(format: "label == %@ OR title == %@", title, title)
        ).firstMatch
        guard item.waitForExistence(timeout: 5) else {
            XCTFail("Menu item not addressable: \(identifier) → \(title)")
            throw ReviewTestError.missingElement(title)
        }
        item.click()
    }

    private func menuControl(_ app: XCUIApplication, _ identifier: String) throws -> XCUIElement {
        for query in [app.popUpButtons, app.menuButtons, app.buttons] {
            let candidate = query.matching(identifier: identifier).firstMatch
            if candidate.waitForExistence(timeout: 3) { return candidate }
        }
        XCTFail("Menu control not addressable: \(identifier)")
        throw ReviewTestError.missingElement(identifier)
    }

    private func closeDetail(_ app: XCUIApplication) throws {
        // Wide window: the inspector closes. Narrow window: the sheet closes.
        let inspectorClose = app.buttons.matching(identifier: "evolutionReview.inspector.close").firstMatch
        if inspectorClose.exists {
            try click(app, "evolutionReview.inspector.close")
        } else {
            try click(app, "evolutionReview.detailSheet.close")
        }
        try expectValue(app, "evolutionReview.detail.visible", "无")
    }

    private func expectDependencyState(_ app: XCUIApplication, id: String, state: String) throws {
        try click(app, "evolutionReview.dependency.\(id)")
        try expectValue(app, "evolutionReview.detail.field.状态", state)
        try closeDetail(app)
    }

    private func exerciseInspectorTransition(_ app: XCUIApplication) throws {
        try click(app, "evolutionReview.evidence.ev.interface.draft")
        try expectValue(app, "evolutionReview.detail.visible", "有")
        try assertMeasuredGeometry(app, width: 1260, height: 860, inspectorVisible: true)
        try closeDetail(app)
        try assertMeasuredGeometry(app, width: 1260, height: 860, inspectorVisible: false)
        // These observations prove the interaction and final geometry, not
        // animation frame timing. The transaction policy has a pure unit test.
    }

    // MARK: - Geometry

    /// Assert the measured layout of the real panels, not a fixture string
    /// repeated by the view.
    private func assertMeasuredGeometry(_ app: XCUIApplication, width: Double, height: Double,
                                        inspectorVisible: Bool) throws {
        let window = app.windows.firstMatch
        let settled = NSPredicate { _, _ in
            guard window.exists else { return false }
            let frame = window.frame
            return abs(frame.width - width) <= 1 && abs(frame.height - height) <= 1
        }
        XCTAssertEqual(XCTWaiter.wait(
            for: [XCTNSPredicateExpectation(predicate: settled, object: window)], timeout: 5
        ), .completed, "the sample size must describe the entire standard NSWindow")
        try expectValue(app, "evolutionReview.measured.frame", String(format: "%.1f×%.1f", width, height))

        let compact = width < 1100
        try expectValue(app, "evolutionReview.layout.compact", compact ? "是" : "否")
        XCTAssertFalse(compact && inspectorVisible, "compact detail must use a sheet, not an inspector column")
        let names = compact ? ["compactSelector", "workspace"]
            : (inspectorVisible ? ["sidebar", "workspace", "inspector"] : ["sidebar", "workspace"])
        let panelsSettled = NSPredicate { [self] _, _ in
            guard let band = frameValue(app, name: "band"),
                  abs(band.width - width) <= 1, band.height > 100 else { return false }
            if !inspectorVisible && value(app, "evolutionReview.measured.inspector") != "absent" { return false }
            var nextY = 8.0
            var nextX = 8.0
            for name in names {
                guard let frame = frameValue(app, name: name),
                      frame.width > 0, frame.height > 0 else { return false }
                if compact {
                    guard abs(frame.minX - 8) <= 1, abs(frame.minY - nextY) <= 1,
                          abs(frame.width - (band.width - 16)) <= 1 else { return false }
                    nextY += frame.height + 8
                } else {
                    guard abs(frame.minX - nextX) <= 1, abs(frame.minY - 8) <= 1,
                          abs(frame.height - (band.height - 16)) <= 1 else { return false }
                    nextX += frame.width + 8
                }
            }
            return compact ? abs(nextY - band.height) <= 2 : abs(nextX - band.width) <= 2
        }
        XCTAssertEqual(XCTWaiter.wait(
            for: [XCTNSPredicateExpectation(predicate: panelsSettled, object: app)], timeout: 10
        ), .completed, "the real panel layout did not settle to the current sample")

        let band = try measuredFrame(app, name: "band")
        XCTAssertEqual(band.width, width, accuracy: 1)
        if compact {
            var nextY = 8.0
            for name in names {
                let frame = try measuredFrame(app, name: name)
                XCTAssertEqual(frame.minX, 8, accuracy: 1, name)
                XCTAssertEqual(frame.minY, nextY, accuracy: 1, name)
                XCTAssertEqual(frame.width, band.width - 16, accuracy: 1, name)
                nextY += frame.height + 8
            }
            XCTAssertEqual(nextY, band.height, accuracy: 2,
                           "compact panels must fit vertically inside the real band")
            try expectValue(app, "evolutionReview.measured.sidebar", "absent")
            try expectValue(app, "evolutionReview.measured.inspector", "absent")
        } else {
            var nextX = 8.0
            for name in names {
                let frame = try measuredFrame(app, name: name)
                XCTAssertEqual(frame.minX, nextX, accuracy: 1, name)
                XCTAssertEqual(frame.minY, 8, accuracy: 1, name)
                XCTAssertEqual(frame.height, band.height - 16, accuracy: 1, name)
                nextX += frame.width + 8
            }
            XCTAssertEqual(nextX, band.width, accuracy: 2,
                           "all widths, gaps and insets must be conserved")
            if !inspectorVisible {
                try expectValue(app, "evolutionReview.measured.inspector", "absent")
            }
        }
        let sizes = value(app, "evolutionReview.measured.content")
            .split(separator: "×").compactMap { Double($0) }
        XCTAssertEqual(sizes.count, 2)
        if sizes.count == 2 {
            XCTAssertEqual(sizes[0], width, accuracy: 1)
            XCTAssertLessThanOrEqual(sizes[1], height + 1)
        }
    }

    private func frameValue(_ app: XCUIApplication, name: String) -> CGRect? {
        let values = value(app, "evolutionReview.measured.\(name)")
            .split(separator: ",").compactMap { Double($0) }
        guard values.count == 4 else { return nil }
        return CGRect(x: values[0], y: values[1], width: values[2], height: values[3])
    }

    private func measuredFrame(_ app: XCUIApplication, name: String) throws -> CGRect {
        let identifier = "evolutionReview.measured.\(name)"
        let leaf = try element(app, identifier)
        let predicate = NSPredicate { [self] _, _ in
            value(app, identifier).split(separator: ",").compactMap { Double($0) }.count == 4
        }
        XCTAssertEqual(XCTWaiter.wait(
            for: [XCTNSPredicateExpectation(predicate: predicate, object: leaf)], timeout: 5
        ), .completed, "missing real layout measurement for \(name)")
        guard let frame = frameValue(app, name: name) else {
            throw ReviewTestError.missingElement(identifier)
        }
        return frame
    }

    // MARK: - Foreground and capture

    private func activateReview(_ app: XCUIApplication) throws {
        guard let expectedPID = Int32(value(app, "evolutionReview.process.pid")) else {
            throw ReviewTestError.missingElement("evolutionReview.process.pid")
        }
        if NSWorkspace.shared.frontmostApplication?.processIdentifier != expectedPID {
            app.activate()
        }
        let foreground = NSPredicate { _, _ in
            app.state == .runningForeground
                && NSWorkspace.shared.frontmostApplication?.processIdentifier == expectedPID
        }
        guard XCTWaiter.wait(
            for: [XCTNSPredicateExpectation(predicate: foreground, object: app)], timeout: 5
        ) == .completed else {
            XCTFail("Review is not foreground: expected \(expectedPID), observed \(String(describing: NSWorkspace.shared.frontmostApplication?.processIdentifier))")
            throw ReviewTestError.reviewNotForeground
        }
    }

    private func capture(_ app: XCUIApplication, name: String) throws {
        do {
            try activateReview(app)
        } catch {
            _ = attachAX(app, name: "\(name)-foreground-failure-DIAGNOSTIC")
            throw error
        }
        let window = app.windows.firstMatch
        let snapshotFrame = attachAX(app, name: name)
        guard window.exists else { throw ReviewTestError.missingElement("review window for \(name)") }
        let expectedPID = Int32(value(app, "evolutionReview.process.pid"))
        let png = try windowPNG(window)
        guard expectedPID != nil,
              NSWorkspace.shared.frontmostApplication?.processIdentifier == expectedPID else {
            throw ReviewTestError.reviewNotForeground
        }
        guard window.frame == snapshotFrame else {
            throw ReviewTestError.unexpectedEntry("Window moved between AX and PNG capture")
        }
        let image = XCTAttachment(data: png, uniformTypeIdentifier: "public.png")
        image.name = "evolution-review-\(name)"
        image.lifetime = .keepAlways
        add(image)
    }

    @discardableResult
    private func attachAX(_ app: XCUIApplication, name: String) -> CGRect? {
        let window = app.windows.firstMatch
        let actualFrame = window.exists ? window.frame : nil
        let frame = window.exists ? String(describing: window.frame) : "No window"
        let text = """
        \(ProcessInfo.processInfo.operatingSystemVersionString)
        Window frame: \(frame)
        Offline sample; appearance/accessibility injections are labelled in the window.
        \(app.debugDescription)
        """
        print("EVOLUTION-REVIEW-AX-BEGIN \(name)\n\(text)\nEVOLUTION-REVIEW-AX-END \(name)")
        let ax = XCTAttachment(data: Data(text.utf8), uniformTypeIdentifier: "public.plain-text")
        ax.name = "evolution-review-\(name)-AX"
        ax.lifetime = .keepAlways
        add(ax)
        return actualFrame
    }

    /// Public per-screen capture avoids the observed negative-y element
    /// screenshot error. These geometry guards cannot prove absence of
    /// occlusion or compositor transforms; every exported image still requires
    /// visual review.
    private func windowPNG(_ window: XCUIElement) throws -> Data {
        let frame = window.frame
        let displays = NSScreen.screens
        func topology(_ screens: [NSScreen]) -> [String] {
            screens.map {
                "\($0.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] ?? ""):\($0.frame):\($0.visibleFrame):\($0.backingScaleFactor)"
            }
        }
        let initialTopology = topology(displays)
        guard let primary = displays.first else {
            throw ReviewTestError.unexpectedEntry("No active display")
        }
        let bounds = displays.map {
            CGRect(x: $0.frame.minX, y: primary.frame.maxY - $0.frame.maxY,
                   width: $0.frame.width, height: $0.frame.height)
        }
        let containing = bounds.indices.filter { bounds[$0].contains(frame) }
        guard containing.count == 1, let index = containing.first else {
            throw ReviewTestError.unexpectedEntry("Window not wholly on one display: \(frame), \(bounds)")
        }
        let target = displays[index]
        let targetBounds = bounds[index]
        let visible = CGRect(
            x: target.visibleFrame.minX, y: primary.frame.maxY - target.visibleFrame.maxY,
            width: target.visibleFrame.width, height: target.visibleFrame.height
        )
        guard visible.contains(frame) else {
            throw ReviewTestError.unexpectedEntry("Window exceeds display's visible frame: \(frame), \(visible)")
        }
        func requireStableGeometry() throws {
            guard window.frame == frame, topology(NSScreen.screens) == initialTopology else {
                throw ReviewTestError.unexpectedEntry("Window/display moved during capture")
            }
        }
        if index == 0 {
            let png = window.screenshot().pngRepresentation
            try requireStableGeometry()
            return png
        }
        let expectedWidth = Int((target.frame.width * target.backingScaleFactor).rounded())
        let expectedHeight = Int((target.frame.height * target.backingScaleFactor).rounded())
        let samePixelSize = displays.dropFirst().filter {
            Int(($0.frame.width * $0.backingScaleFactor).rounded()) == expectedWidth
                && Int(($0.frame.height * $0.backingScaleFactor).rounded()) == expectedHeight
        }
        let screens = XCUIScreen.screens
        guard samePixelSize.count == 1, screens.count == displays.count else {
            throw ReviewTestError.unexpectedEntry("Cannot unambiguously match secondary display")
        }
        let matchingImages = screens.dropFirst().compactMap { screen -> CGImage? in
            let png = screen.screenshot().pngRepresentation
            guard let source = CGImageSourceCreateWithData(png as CFData, nil),
                  let pixels = CGImageSourceCreateImageAtIndex(source, 0, nil),
                  pixels.width == expectedWidth, pixels.height == expectedHeight else { return nil }
            return pixels
        }
        guard matchingImages.count == 1, let pixels = matchingImages.first else {
            throw ReviewTestError.unexpectedEntry("No unique XCTest image for secondary display")
        }
        let scaleX = CGFloat(pixels.width) / targetBounds.width
        let scaleY = CGFloat(pixels.height) / targetBounds.height
        let crop = CGRect(
            x: (frame.minX - targetBounds.minX) * scaleX,
            y: (frame.minY - targetBounds.minY) * scaleY,
            width: frame.width * scaleX, height: frame.height * scaleY
        )
        let integral = crop.integral
        guard abs(crop.minX - integral.minX) < 0.001,
              abs(crop.minY - integral.minY) < 0.001,
              abs(crop.width - integral.width) < 0.001,
              abs(crop.height - integral.height) < 0.001 else {
            throw ReviewTestError.unexpectedEntry("Window crop is not backing-pixel aligned: \(crop)")
        }
        guard CGRect(x: 0, y: 0, width: pixels.width, height: pixels.height).contains(crop),
              let result = pixels.cropping(to: crop) else {
            throw ReviewTestError.unexpectedEntry("Window crop exceeds display image: \(crop)")
        }
        try requireStableGeometry()
        guard let png = NSBitmapImageRep(cgImage: result).representation(using: .png, properties: [:]) else {
            throw ReviewTestError.unexpectedEntry("Could not encode original window pixels as PNG")
        }
        return png
    }
}
