#if DEBUG
import AeroricUI
import AppKit
import CryptoKit
import Darwin
import ImageIO
import XCTest

/// Real-window tests for Q45's *offline fixture*, not a knowledge engine.
///
/// Execute only through the existing native:ui-test/accessibility gate AFTER
/// main has authorized its isolated account/VM and exclusive desktop. That
/// external authorization cannot be inferred from AERORIC_ENV or a Caches
/// path. In particular, the full runner also prewarms the ordinary product.
///
/// The authorized VM controller prepares and collects one nonsensitive key
/// per test attempt in the actual isolated account's dev-app domain. XCTest
/// only reads it: its sandbox, the target domain and the assertions stay intact.
/// It is not safe to run this suite in an everyday account.
@MainActor
final class NativeKnowledgeReviewUITests: XCTestCase {
    private static let bundleID = "com.aeroric.native.dev"
    private static let sampleTitle = "项目知识审阅样例"
    private static let refusalTitle = "知识审阅未启动"
    private static let prefix = "knowledgeReview."
    private static let reviewKeys = [
        "AERORIC_NATIVE_KNOWLEDGE_REVIEW",
        "AERORIC_NATIVE_EVOLUTION_REVIEW",
        "AERORIC_NATIVE_VISUAL_REVIEW",
        "AERORIC_NATIVE_COLLABORATION_REVIEW",
    ]

    private struct ReviewFailure: Error, CustomStringConvertible {
        let description: String
    }

    /// knowledge=1 is always present. Missing/malformed knowledge flags and
    /// unsafe directories belong to entry UNIT tests, not executable launches.
    private struct Request {
        var name = "valid"
        var environment = "ui-test"
        var evolution = "0"
        var visual = "0"
        var collaboration = "0"
        var refusalReasons: [String] = []

        var flagValues: [String] { ["1", evolution, visual, collaboration] }
        var isRefused: Bool { !refusalReasons.isEmpty }
    }

    private struct DirectoryWitness {
        let accountHome: String
        let parent: URL
        let child: URL
        let beforeCount: Int
        let beforeDigest: String
    }

    private struct PreferenceWitness {
        // Absolute domain path: never the sandboxed runner's UserDefaults.
        let domain: String
        let key: String
        let token: String
    }

    private struct PreferenceDescriptor: Decodable {
        let version: Int
        let accountUID: UInt32
        let accountHome: String
        let bundleID: String
        let method: String
        let attemptID: String
        let key: String
        let token: String
    }

    @MainActor
    private final class Launch {
        let app: XCUIApplication
        let request: Request
        let directory: DirectoryWitness
        let preference: PreferenceWitness
        let process: NSRunningApplication
        let pid: pid_t
        let started: Date?
        var observedChildren: [pid_t: String] = [:]

        init(app: XCUIApplication, request: Request, directory: DirectoryWitness,
             preference: PreferenceWitness, process: NSRunningApplication) {
            self.app = app
            self.request = request
            self.directory = directory
            self.preference = preference
            self.process = process
            pid = process.processIdentifier
            started = process.launchDate
        }

        var window: XCUIElement {
            let title = request.isRefused
                ? NativeKnowledgeReviewUITests.refusalTitle
                : NativeKnowledgeReviewUITests.sampleTitle
            return app.windows.matching(
                NSPredicate(format: "label == %@ OR title == %@", title, title)
            ).firstMatch
        }
    }

    // MARK: - 1. Candidate detail, batch review and edit drafts are separate

    func testCandidatesBatchReviewAndDraftStayOffline() throws {
        try withLaunch { launch in
            let app = launch.app
            try chooseScenario(launch, "K1 集中审阅")
            try expect(app, "detail.title", "虚构偏好 · 简短结论")
            try expect(app, "detail.content", "先给简短结论，再列必要依据。")
            try expect(app, "batch.summary", "0 条")
            let originalScope = value(app, "detail.scope")
            let originalLimits = value(app, "detail.limitations")
            let originalSource = value(app, "detail.source")
            let originalEvidence = value(app, "detail.verification")
            try require(originalScope.contains("虚构项目 · 海港"), "Initial scope must be the fictional project")

            try click(launch, "detail.openSource")
            try expect(app, "source.title", originalSource)
            try expectContains(app, "source.first", "虚构来源首段")
            try expectContains(app, "source.warning", "不打开文件、URL 或真实历史")
            try capture(launch, name: "K1-embedded-source")
            try dismiss(launch, button: "source.close", content: "source.title")

            try click(launch, "edit")
            try replaceText(launch, "draft.content", with: "Cancelled offline candidate text")
            try replaceText(launch, "draft.scope", with: "Cancelled Harbor scope")
            try dismiss(launch, button: "draft.cancel", content: "draft.content")
            try expect(app, "detail.content", "先给简短结论，再列必要依据。")
            try expect(app, "detail.scope", originalScope)

            let edited = "Offline review: a concise conclusion must still include risks."
            let scope = "Fictional Harbor review summaries only."
            try click(launch, "edit")
            try replaceText(launch, "draft.content", with: edited)
            try replaceText(launch, "draft.scope", with: scope)
            try dismiss(launch, button: "draft.apply", content: "draft.content")
            try expect(app, "detail.content", edited)
            try expectContains(app, "detail.scope", scope)
            try expectContains(app, "detail.scope", "虚构项目 · 海港")
            try expect(app, "detail.limitations", originalLimits)
            try expect(app, "detail.source", originalSource)
            try expect(app, "detail.verification", originalEvidence)
            try capture(launch, name: "K1-draft-applied-only-in-memory")

            // A checkbox click must not move the independent detail focus.
            try setToggle(launch, "batch.conciseReplies", to: true)
            try setToggle(launch, "batch.verifiedCheck", to: true)
            try expect(app, "batch.summary", "2 条")
            try expect(app, "detail.title", "虚构偏好 · 简短结论")
            try click(launch, "batch.confirm")
            try expectContains(app, "feedback", "确认 2 条")
            try expect(app, "batch.summary", "0 条")
            try require(!query(app, "batch.conciseReplies").exists && !query(app, "batch.verifiedCheck").exists,
                        "Both confirmed rows must leave candidates, not just change a toast")

            try chooseSection(launch, "可复用内容")
            try selectItem(launch, "conciseReplies")
            try expect(app, "detail.status", "样例内可复用 · 不代表当前已核验")
            try expect(app, "detail.content", edited)
            try expect(app, "detail.source", originalSource)
            try expectContains(app, "detail.scope", scope)
            try expect(app, "detail.limitations", originalLimits)
            try expect(app, "detail.verification", originalEvidence)
            try selectItem(launch, "verifiedCheck")
            try expectContains(app, "detail.verification", "PRELOADED")
            try expectContains(app, "detail.verification", "非本次实测")
            try capture(launch, name: "K1-two-confirmed-with-original-evidence")

            try chooseSection(launch, "候选")
            try selectItem(launch, "discardable")
            let discardedSource = value(app, "detail.source")
            try setToggle(launch, "batch.discardable", to: true)
            try click(launch, "batch.discard")
            try expectContains(app, "feedback", "丢弃 1 条")
            try require(!query(app, "batch.discardable").exists, "Discarded item stayed in candidates")
            try chooseSection(launch, "历史")
            try selectItem(launch, "discardable")
            try expect(app, "detail.status", "已丢弃示例 · 仅历史")
            try expect(app, "detail.source", discardedSource)

            try chooseSection(launch, "候选")
            try selectItem(launch, "selfReport")
            try expect(app, "detail.verification", "Agent 自报示例 · 未核验")
            try expectContains(app, "detail.blocked", "不能作为已核验成功经验")
            try setToggle(launch, "batch.selfReport", to: true)
            try require(!(try element(app, "batch.confirm")).isEnabled,
                        "A selected self-report must not acquire verification through review")
            try expect(app, "detail.status", "候选 · 待审阅")
            try capture(launch, name: "K1-self-report-not-verified")
            try assertIsolation(launch, stage: "K1-complete")
        }
    }

    // MARK: - 2. Auto admission and Full Access are independent, including exceptions

    func testAutoAdmissionAndFullAccessRemainIndependent() throws {
        try withLaunch { launch in
            let app = launch.app
            try chooseScenario(launch, "K2 自动入库与授权独立")
            try expect(app, "configuration.summary", "关闭 · 本项目 · 用户偏好")
            try expect(app, "permission", "默认授权示例")
            try click(launch, "configuration.open")
            try setToggle(launch, "configuration.enabled", to: true)
            try chooseMenu(launch, "configuration.preset", title: "本项目 · 预置核验解法")
            try expectContains(app, "configuration.scope", "虚构预置检查记录")
            try expectContains(app, "configuration.propagation", "传播规则未在本样例演示")
            try capture(launch, name: "K2-unapplied-configuration")
            try dismiss(launch, button: "configuration.cancel", content: "configuration.enabled")
            try expect(app, "configuration.summary", "关闭 · 本项目 · 用户偏好")
            try expect(app, "permission", "默认授权示例")

            // Each combination starts from a NEW fixed batch. Changing the
            // setting after one batch would not exercise the other combination.
            for enabled in [false, true] {
                for permission in ["默认授权示例", "Full Access 示例"] {
                    // Reselecting the same native menu item need not call its
                    // binding setter. Use the real reset action for every batch.
                    try click(launch, "reset")
                    try expect(app, "scenario", "K2 自动入库与授权独立")
                    try expect(app, "configuration.summary", "关闭 · 本项目 · 用户偏好")
                    try expect(app, "permission", "默认授权示例")
                    try expect(app, "batch.summary", "0 条")
                    try require((try element(app, "newCandidates.process")).isEnabled,
                                "Reset must restore the one-shot batch before each combination")
                    try chooseMenu(launch, "permission", title: permission)
                    try expect(app, "permission", permission)
                    try expect(app, "configuration.summary", "关闭 · 本项目 · 用户偏好")
                    try configureAdmission(launch, enabled: enabled, preset: "本项目 · 用户偏好")
                    try expect(app, "permission", permission)
                    try click(launch, "newCandidates.process")
                    try expectContains(app, "feedback", "新增 6 条，其中 \(enabled ? 1 : 0) 条按预设免逐条确认")
                    try require(!(try element(app, "newCandidates.process")).isEnabled,
                                "The fixed fixture batch must not duplicate indefinitely")
                    try expect(app, "batch.summary", "0 条")
                    try chooseSection(launch, enabled ? "可复用内容" : "候选")
                    try selectItem(launch, "newPreference")
                    try expect(app, "detail.status", enabled
                               ? "样例内可复用 · 不代表当前已核验" : "候选 · 待审阅")
                    try expect(app, "detail.source", "虚构新用户陈述 · 固定批次")
                    try expectContains(app, "detail.scope", "虚构项目 · 海港")
                    try expectContains(app, "detail.limitations", "不跨项目")
                    // A different type/source stays a candidate even when on.
                    try chooseSection(launch, "候选")
                    try selectItem(launch, "newSolution")
                    try expect(app, "detail.status", "候选 · 待审阅")
                    try assertExceptionsRemainCandidates(launch)
                    try expect(app, "permission", permission)
                    try expect(app, "configuration.summary", "\(enabled ? "开启" : "关闭") · 本项目 · 用户偏好")
                    let name = "K2-auto-\(enabled)-\(permission == "默认授权示例" ? "default" : "full-access")"
                    try capture(launch, name: name)
                    try assertIsolation(launch, stage: name)
                }
            }

            // K3 also exercises the second *fixed* type/source/project preset.
            try chooseScenario(launch, "K3 例外仍留候选")
            try chooseMenu(launch, "permission", title: "Full Access 示例")
            try configureAdmission(launch, enabled: true, preset: "本项目 · 预置核验解法")
            try click(launch, "newCandidates.process")
            try expectContains(app, "feedback", "新增 2 条，其中 1 条按预设免逐条确认")
            try assertExceptionsRemainCandidates(launch)
            try selectItem(launch, "newPreference")
            try expect(app, "detail.status", "候选 · 待审阅")
            try chooseSection(launch, "可复用内容")
            try selectItem(launch, "newSolution")
            try expect(app, "detail.status", "样例内可复用 · 不代表当前已核验")
            try expectContains(app, "detail.verification", "PRELOADED")
            try expectContains(app, "detail.verification", "非本次实测")
            try expectContains(app, "detail.source", "虚构固定批次检查")
            try configureAdmission(launch, enabled: false, preset: "本项目 · 预置核验解法")
            try expect(app, "permission", "Full Access 示例")
            // This is only the fixture's observable retention. The UI must
            // explicitly leave production propagation semantics undecided.
            try expect(app, "detail.status", "样例内可复用 · 不代表当前已核验")
            try click(launch, "configuration.open")
            try expectContains(app, "configuration.propagation", "不代表停用、撤回或清除上下文")
            try dismiss(launch, button: "configuration.cancel", content: "configuration.enabled")
            try capture(launch, name: "K3-exceptions-and-preloaded-not-live-verification")
            try assertIsolation(launch, stage: "K2-K3-complete")
        }
    }

    // MARK: - 3. Related references keep their source and obey current requirements

    func testScopedReferencesRespectSourcesAndCurrentRequirements() throws {
        try withLaunch { launch in
            let app = launch.app
            try chooseScenario(launch, "K4 按需参考")
            try ensureReferences(launch)
            try chooseMenu(launch, "references.work", title: "虚构任务 · 存储选型")
            try chooseMenu(launch, "references.context", title: "带来源参考")
            try expect(app, "references.count", "1 条")
            try expectContains(app, "reference.databaseChoice.summary", "SQLite")
            try expect(app, "reference.databaseChoice.status", "带条件的虚构参考")
            try expectContains(app, "reference.databaseChoice.explanation", "单机原型")
            try assertOnlyReference(launch, id: "databaseChoice")
            try click(launch, "reference.databaseChoice.source")
            try expect(app, "source.title", "虚构用户决策 · 离线原型")
            try expectContains(app, "source.first", "旧原型单机、离线、低并发")
            try capture(launch, name: "K4-sqlite-embedded-history")
            try dismiss(launch, button: "source.close", content: "source.title")

            try chooseMenu(launch, "references.context", title: "当前明确要求优先")
            try expect(app, "references.count", "0 条")
            try expect(app, "reference.databaseChoice.status", "当前明确要求优先 · 旧记录不强制")
            try expectContains(app, "references.summary", "Postgres")
            try expectContains(app, "references.requirement", "使用 Postgres")
            try expectContains(app, "reference.databaseChoice.explanation", "不作为推荐方案")
            try capture(launch, name: "K4-current-Postgres-requirement-wins")

            try chooseMenu(launch, "references.context", title: "来源缺失")
            try expect(app, "references.count", "0 条")
            try expect(app, "reference.databaseChoice.status", "来源缺失 · 不作为当前事实")
            try expectContains(app, "reference.databaseChoice.explanation", "不去查真实历史")
            try click(launch, "reference.databaseChoice.source")
            try expectContains(app, "source.missing", "来源缺失；不编造回源")
            try require(!query(app, "source.first").exists && !query(app, "source.last").exists,
                        "A missing source must not fabricate an excerpt")
            try capture(launch, name: "K4-missing-source-explicit")
            try dismiss(launch, button: "source.close", content: "source.title")

            try chooseMenu(launch, "references.context", title: "过时／冲突待复核")
            try expect(app, "references.count", "0 条")
            try expect(app, "reference.databaseChoice.status", "过时／冲突 · 待复核")
            try expectContains(app, "reference.databaseChoice.explanation", "不作为当前确定结论推荐")
            try capture(launch, name: "K4-outdated-is-not-a-current-fact")

            try chooseMenu(launch, "references.context", title: "带来源参考")
            for title in ["虚构任务 · API 错误接口联调", "虚构工作包 · 客户端错误呈现"] {
                try chooseMenu(launch, "references.work", title: title)
                try expect(app, "references.count", "1 条")
                try assertOnlyReference(launch, id: "interfaceContract")
                try expectContains(app, "reference.interfaceContract.summary", "message")
                try expectContains(app, "reference.interfaceContract.explanation", "不是系统指令")
                try capture(launch, name: "K4-\(title)")
            }
            try chooseMenu(launch, "references.work", title: "虚构工作包 · 独立术语文档")
            try expect(app, "references.count", "1 条")
            try assertOnlyReference(launch, id: "docsGuidance")
            try expectContains(app, "reference.docsGuidance.summary", "不修改接口契约")
            try click(launch, "reference.docsGuidance.source")
            try expect(app, "source.title", "虚构用户说明 · 独立文档范围")
            try expectContains(app, "source.first", "不依赖接口字段变更")
            try dismiss(launch, button: "source.close", content: "source.title")
            try click(launch, "references.toggle")
            try require(wait { !self.query(app, "references.count").exists }, "Disclosure must actually collapse")
            try click(launch, "references.toggle")
            try assertOnlyReference(launch, id: "docsGuidance")
            try capture(launch, name: "K4-related-docs-not-whole-library")
            try assertIsolation(launch, stage: "K4-complete")
        }
    }

    // MARK: - 4. User revisions, retirement, old citations and local checks

    func testRevisionsRetirementAndAffectedTasksStayLocal() throws {
        try withLaunch { launch in
            let app = launch.app
            try chooseScenario(launch, "K5 纠正与版本")
            try expect(app, "detail.version", "v1 · 当前样例版本")
            let oldContent = value(app, "detail.content")
            let oldSource = value(app, "detail.source")
            let oldScope = value(app, "detail.scope")
            try require(!query(app, "history.open").exists, "There should be no generated v2 before a correction")
            try chooseSection(launch, "候选")
            try selectItem(launch, "agentGuess")
            try expect(app, "detail.verification", "Agent 自报示例 · 未核验")
            try setToggle(launch, "batch.agentGuess", to: true)
            try require(!(try element(app, "batch.confirm")).isEnabled, "An Agent guess cannot replace user knowledge")
            try expectContains(app, "detail.blocked", "不得覆盖用户确认")
            try chooseSection(launch, "可复用内容")
            try selectItem(launch, "interfaceContract")
            try expect(app, "detail.content", oldContent)

            try click(launch, "revise")
            try require(!(try element(app, "draft.apply")).isEnabled, "Revision requires a user correction basis")
            try replaceText(launch, "draft.content", with: "Cancelled v2 content")
            try replaceText(launch, "draft.scope", with: "Cancelled v2 scope")
            try replaceText(launch, "draft.basis", with: "Cancelled fictional basis")
            try dismiss(launch, button: "draft.cancel", content: "draft.content")
            try expect(app, "detail.version", "v1 · 当前样例版本")
            try expect(app, "detail.content", oldContent)
            try expect(app, "detail.scope", oldScope)
            try require(!query(app, "history.open").exists, "Cancel must not manufacture a historical version")

            let correction = "Fictional user correction: preserve message and add code for the Harbor API."
            let basis = "Offline user correction UI witness; never a live check."
            try click(launch, "revise")
            try replaceText(launch, "draft.content", with: correction)
            try replaceText(launch, "draft.basis", with: basis)
            try dismiss(launch, button: "draft.apply", content: "draft.content")
            try expect(app, "detail.version", "v2 · 当前样例版本")
            try expect(app, "detail.content", correction)
            try expectContains(app, "detail.replacement", "v1 → v2")
            try expectContains(app, "detail.replacement", basis)
            try expect(app, "detail.verification", "用户陈述示例 · 不代表核验")
            try click(launch, "history.open")
            try expect(app, "detail.version", "v1 · 已被替代的历史版本")
            try expect(app, "detail.content", oldContent)
            try expect(app, "detail.source", oldSource)
            try expect(app, "detail.scope", oldScope)
            try click(launch, "detail.openSource")
            try expectContains(app, "source.context", "v1 · 已被替代的历史版本")
            try expect(app, "source.title", oldSource)
            try capture(launch, name: "K5-preserved-v1-source")
            try dismiss(launch, button: "source.close", content: "source.title")
            try chooseMenu(launch, "history.version", title: "v2 · 当前样例版本")
            try expect(app, "detail.content", correction)
            try expectContains(app, "detail.replacement", basis)
            try ensureImpacts(launch)
            try expectContains(app, "impact.apiTask.citation", "旧引用 v1")
            try expectContains(app, "impact.apiTask.citation", "当前为 v2")
            try require(!(try element(app, "impact.apiTask.check")).isEnabled,
                        "A custom v2 cannot borrow the fixed preloaded check merely by matching its number")
            try expectContains(app, "impact.apiTask.blocked", "当前 v2 内容或适用条件不同于预置记录")
            try assertIsolation(launch, stage: "K5-custom-revision")

            // K6 intentionally loads a clearly preloaded v2, not the preceding
            // custom revision, then tests actual retirement and old citations.
            try chooseScenario(launch, "K6 停用与后续引用")
            try expect(app, "detail.version", "v2 · 当前样例版本")
            try ensureReferences(launch)
            try expect(app, "references.count", "1 条")
            try click(launch, "retire")
            try expect(app, "detail.status", "已停用示例 · 不再推荐")
            try expect(app, "references.count", "0 条")
            try expect(app, "reference.interfaceContract.status", "已停用 · 仅历史回看")
            try expectContains(app, "references.summary", "不是删除记忆或抹除已消费上下文")
            try expectContains(app, "reference.interfaceContract.summary", "不提供后续新上下文推荐正文")
            try require(!query(app, "reference.interfaceContract.source").exists,
                        "A retired reference must route to history, not inject its body as new context")
            try expect(app, "detail.version", "v2 · 停用版本 · 仅历史")
            try click(launch, "reference.interfaceContract.history")
            try expect(app, "detail.version", "v2 · 停用版本 · 仅历史")
            try click(launch, "history.open")
            try expect(app, "detail.version", "v1 · 已被替代的历史版本")
            try chooseMenu(launch, "history.version", title: "v2 · 停用版本 · 仅历史")
            try expect(app, "detail.version", "v2 · 停用版本 · 仅历史")
            try ensureImpacts(launch)
            try expectContains(app, "impact.apiTask.status", "待局部核对")
            try require(!(try element(app, "impact.apiTask.check")).isEnabled,
                        "Retired knowledge cannot be validated with an old preloaded check")
            try expectContains(app, "impact.apiTask.citation", "已停用")
            try click(launch, "impact.apiTask.source")
            try expectContains(app, "source.context", "历史旧引用 v1")
            try expect(app, "source.title", oldSource)
            try expectContains(app, "source.first", "第一版使用 message")
            try capture(launch, name: "K6-retired-but-old-citation-preserved")
            try dismiss(launch, button: "source.close", content: "source.title")
            try assertIsolation(launch, stage: "K6-retirement")

            try chooseScenario(launch, "K7 在途局部核对")
            try ensureImpacts(launch)
            for work in ["apiTask", "clientPackage"] {
                try expect(app, "impact.\(work).status", "受影响 · 待局部核对 · 尚未演示通知")
            }
            try expectContains(app, "impact.docsPackage.status", "安全独立文档 · 可继续")
            let oldCitation = value(app, "impact.apiTask.citation")
            try require(!(try element(app, "impact.apiTask.ack")).isEnabled, "ACK must follow a notification")
            try require(!(try element(app, "impact.clientPackage.check")).isEnabled,
                        "The API record is not applicable to the client work package")
            try click(launch, "impact.apiTask.notify")
            try expect(app, "impact.apiTask.status", "受影响 · 待局部核对 · 已演示通知 · 不代表纠正")
            try require(!query(app, "impact.apiTask.record").exists, "Notification cannot create a check record")
            try capture(launch, name: "K7-notification-is-not-correction")
            try click(launch, "impact.apiTask.ack")
            try expect(app, "impact.apiTask.status", "受影响 · 待局部核对 · 已演示 ACK · 不代表核验")
            try require(!query(app, "impact.apiTask.record").exists, "ACK cannot create a check record")
            try expect(app, "impact.apiTask.citation", oldCitation)
            try click(launch, "documentation.continue")
            try expectContains(app, "impact.docsPackage.status", "独立文档已演示继续 · 仅内存")
            let documents = value(app, "impact.docsPackage.status")
            try capture(launch, name: "K7-ACK-pending-and-safe-docs-continue")

            try click(launch, "impact.apiTask.check")
            try expect(app, "impact.apiTask.status", "预置局部核对示例 · PRELOADED · 非本次实测")
            try expectContains(app, "impact.apiTask.record", "PRELOADED · 非本次实测")
            try expectContains(app, "impact.apiTask.record", "客户端工作包未核对")
            try expect(app, "impact.clientPackage.status", "受影响 · 待局部核对 · 尚未演示通知")
            try require(!query(app, "impact.clientPackage.record").exists, "The other affected item must remain unchecked")
            try expect(app, "impact.docsPackage.status", documents)
            try expect(app, "impact.apiTask.citation", oldCitation)
            try require(!(try element(app, "impact.apiTask.check")).isEnabled, "A preloaded record must not be replayed")
            try capture(launch, name: "K7-only-applicable-local-record-loaded")
            try assertIsolation(launch, stage: "K5-K6-K7-complete")
        }
    }

    // MARK: - 5. Reset, real window recreation, keyboard, appearance and geometry

    func testNativeAppearanceKeyboardAndCompactLayout() throws {
        var unavailableSizes: [String] = []
        var terminatedInstance = ""
        var terminatedPID: pid_t = 0
        var terminatedStart: Date?

        try withLaunch { launch in
            let app = launch.app
            try chooseScenario(launch, "K8 无持久化与重置")
            try dirtyWindowFixture(launch, marker: "reset-witness")
            try capture(launch, name: "K8-mutated-before-reset")
            try click(launch, "reset")
            try assertPristineFixture(launch, code: "K8")
            try capture(launch, name: "K8-after-real-reset")

            try dirtyWindowFixture(launch, marker: "scenario-witness")
            try chooseScenario(launch, "K4 按需参考")
            try chooseScenario(launch, "K8 无持久化与重置")
            try assertPristineFixture(launch, code: "K8")

            // Minimizing is not closing: restoring the still-owned window must
            // preserve the same Store and its in-memory state.
            try dirtyWindowFixture(launch, marker: "minimize-window-witness")
            let beforeMinimize = try memoryInstance(launch)
            let minimize = launch.window.buttons[XCUIIdentifierMinimizeWindow]
            try require(minimize.exists && minimize.isHittable,
                        "The own standard window minimize button must be reachable")
            try activate(launch)
            minimize.click()
            try require(wait { !minimize.exists || !minimize.isHittable },
                        "The sample window did not actually minimize")
            try assertOwnership(launch)
            try reopenSampleWindow(launch)
            try require(try memoryInstance(launch) == beforeMinimize,
                        "Restoring the minimized window replaced its Store")
            try expect(app, "detail.content", "Offline applied content: minimize-window-witness")
            try expect(app, "batch.summary", "1 条")
            try expect(app, "configuration.summary", "开启 · 本项目 · 用户偏好")
            try expect(app, "permission", "Full Access 示例")
            try capture(launch, name: "K8-minimized-and-restored-same-memory")
            try click(launch, "reset")
            try assertPristineFixture(launch, code: "K8")

            // Exercise the real close/reopen path, not merely reset or a unit
            // test constructing a second Store. The process must stay the same
            // while its window-local Store UUID changes.
            try dirtyWindowFixture(launch, marker: "close-window-witness")
            let beforeClose = try memoryInstance(launch)
            try capture(launch, name: "K8-before-window-close")
            let close = launch.window.buttons[XCUIIdentifierCloseWindow]
            try require(close.exists && close.isHittable, "The own standard window close button must be reachable")
            try activate(launch)
            close.click()
            try require(wait { !launch.window.exists }, "The sample window did not actually close")
            try assertOwnership(launch)
            attachText("same owned PID=\(launch.pid); window absent=\(!launch.window.exists)\n\(app.debugDescription)",
                       name: "K8-actual-closed-window")
            try reopenSampleWindow(launch)
            try require(wait(timeout: 10) {
                let leaf = self.query(app, "memory.instance")
                guard leaf.exists else { return false }
                let actual = self.text(leaf)
                return UUID(uuidString: actual) != nil && actual != beforeClose
            }, "The reopened window did not publish a fresh Store UUID")
            try require(try memoryInstance(launch) != beforeClose, "Reopening reused the closed window's Store")
            try assertPristineFixture(launch, code: "K1", inspectDrafts: true)
            try capture(launch, name: "K8-standard-Window-menu-reopened")

            let presentationInstance = try memoryInstance(launch)
            try openAppearance(launch)
            var system: [String: String] = [:]
            for key in ["reduceTransparency", "increaseContrast", "reduceMotion"] {
                let actual = text(try element(app, "system.\(key)"))
                try require(["on", "off"].contains(actual), "Unreadable actual system preference: \(key)")
                system[key] = actual
                try expect(app, "effective.\(key)", actual)
                try require(!(try booleanValue(element(app, "preview.\(key)"))), "Preview injection should start disabled")
            }
            try expect(app, "effective.appearance", "system")
            try expect(app, "effective.surface", system["reduceTransparency"] == "on" ? "classic" : "glass")
            try capture(launch, name: "K8-system-values-without-injection")
            try dismiss(launch, button: "preview.close", content: "appearance.sheet")

            for (appearance, raw) in [("浅色", "light"), ("深色", "dark")] {
                for surface in ["glass", "classic"] {
                    for sample in Self.sizeSamples {
                        try openAppearance(launch)
                        try chooseSegment(launch, group: "配色预览", title: appearance)
                        try chooseSegment(launch, group: "面板材质", title: surface)
                        try expect(app, "effective.appearance", raw)
                        try expect(app, "effective.surface",
                                   system["reduceTransparency"] == "on" ? "classic" : surface)
                        let fits = try requestGeometry(launch, sample, unavailable: &unavailableSizes)
                        let frame = launch.window.frame
                        let name = "\(raw)-\(surface)-\(sample.title)"
                        try capture(launch, name: "K8-\(name)-measured-preview\(fits ? "" : "-NOT-VERIFIED")")
                        try dismiss(launch, button: "preview.close", content: "appearance.sheet")
                        try require(launch.window.frame == frame, "Closing preview changed the measured window")
                        // Show the real sample's material, seams, semantic text
                        // and bare icons, not just the settings sheet.
                        try reveal(launch, target: element(app, "configuration.summary"))
                        try capture(launch, name: "K8-\(name)-actual-sample\(fits ? "" : "-NOT-VERIFIED")")
                    }
                }
            }

            try openAppearance(launch)
            try chooseSegment(launch, group: "面板材质", title: "glass")
            for key in ["reduceTransparency", "increaseContrast", "reduceMotion"] {
                try setToggle(launch, "preview.\(key)", to: true)
                try expect(app, "effective.\(key)", "on")
                try expect(app, "system.\(key)", try XCTUnwrap(system[key]))
            }
            try expect(app, "effective.surface", "classic")
            try expect(app, "effective.outline", "1.0")
            try expect(app, "effective.motion", "0.000")
            try capture(launch, name: "K8-strengthened-accessibility-readouts")
            try dismiss(launch, button: "preview.close", content: "appearance.sheet")
            try ensureReferences(launch)
            try ensureImpacts(launch)
            try capture(launch, name: "K8-effective-opaque-panels-and-reduced-motion-transition")
            // These screenshots and final frames demonstrate real views and
            // transitions, not measured animation timing or automatic visual
            // approval. Main also reviews the transaction/component tests.
            try openAppearance(launch)
            for key in ["reduceTransparency", "increaseContrast", "reduceMotion"] {
                try setToggle(launch, "preview.\(key)", to: false)
                try expect(app, "effective.\(key)", try XCTUnwrap(system[key]))
                try expect(app, "system.\(key)", try XCTUnwrap(system[key]))
            }
            try expect(app, "effective.surface", system["reduceTransparency"] == "on" ? "classic" : "glass")
            try expect(app, "effective.outline", system["increaseContrast"] == "on" ? "1.0" : "0.5")
            let motion = try XCTUnwrap(Double(value(app, "effective.motion")))
            try require(system["reduceMotion"] == "on" ? motion == 0 : motion > 0,
                        "Disabling injection must return to, never weaken, the system motion policy")
            try dismiss(launch, button: "preview.close", content: "appearance.sheet")
            try require(try memoryInstance(launch) == presentationInstance, "Appearance changes replaced business memory")
            try expect(app, "permission", "默认授权示例")
            try expect(app, "configuration.summary", "关闭 · 本项目 · 用户偏好")

            // A real native List selection responds to arrows without selecting
            // a batch. The 1000pt specimen uses the wide system-list layout.
            try openAppearance(launch)
            let wideFits = try requestGeometry(launch, Self.sizeSamples[1], unavailable: &unavailableSizes)
            try dismiss(launch, button: "preview.close", content: "appearance.sheet")
            if wideFits {
                try chooseScenario(launch, "K8 无持久化与重置")
                try selectItem(launch, "conciseReplies")
                try focusNativeListRow(launch, "conciseReplies")
                var reachedUp = false
                defer {
                    if !reachedUp { recordNativeInput(launch, checkpoint: "Up-not-reached") }
                }
                recordNativeInput(launch, checkpoint: "before-Down", attachment: "K8-native-before-Down")
                do {
                    defer { recordNativeInput(launch, checkpoint: "after-Down-check") }
                    app.typeKey(.downArrow, modifierFlags: [])
                    recordNativeInput(launch, checkpoint: "after-Down", attachment: "K8-native-after-Down")
                    try expect(app, "detail.title", "虚构解法 · 空字段提示")
                }
                reachedUp = true
                recordNativeInput(launch, checkpoint: "before-Up")
                do {
                    defer { recordNativeInput(launch, checkpoint: "after-Up-check") }
                    app.typeKey(.upArrow, modifierFlags: [])
                    recordNativeInput(launch, checkpoint: "after-Up", attachment: "K8-native-after-Up")
                    try expect(app, "detail.title", "虚构偏好 · 简短结论")
                }
                try expect(app, "batch.summary", "0 条")
                try capture(launch, name: "K8-native-list-arrow-selection")
            } else {
                attachText("Native wide-list arrow selection NOT VERIFIED: the measured display cannot fit 1000×640. Compact interactions and relaunch checks continue below.",
                           name: "K8-wide-keyboard-display-limit-NOT-VERIFIED")
            }

            try openAppearance(launch)
            let compactFits = try requestGeometry(launch, Self.sizeSamples[2], unavailable: &unavailableSizes)
            try dismiss(launch, button: "preview.close", content: "appearance.sheet")
            // Continue all semantic interactions even on a constrained screen;
            // only actual 400×600 geometry is reported verified when it fits.
            try chooseScenario(launch, "K8 无持久化与重置")
            if compactFits {
                _ = try element(app, "section")
                _ = try element(app, "itemPicker")
            }
            try selectItem(launch, "conciseReplies")
            try click(launch, "edit")
            try replaceText(launch, "draft.content", with: "Escape must discard this compact draft")
            try replaceText(launch, "draft.scope", with: "Discard this compact scope")
            try require((try element(app, "draft.cancel")).isHittable && (try element(app, "draft.apply")).isHittable,
                        "Both compact draft footer actions must remain reachable")
            try capture(launch, name: "K8-compact-draft-before-Escape")
            try escapeSheet(launch, content: "draft.content")
            try expect(app, "detail.content", "先给简短结论，再列必要依据。")

            try click(launch, "configuration.open")
            try setToggle(launch, "configuration.enabled", to: true)
            // Tab from the native toggle must reach the next menu without
            // altering global keyboard preferences. Space opens that menu;
            // the first Escape closes it, the second cancels the sheet.
            try activate(launch)
            app.typeKey(.tab, modifierFlags: [])
            app.typeKey(" ", modifierFlags: [])
            let presetItem = app.menuItems.matching(
                NSPredicate(format: "label == %@ OR title == %@", "本项目 · 用户偏好", "本项目 · 用户偏好")
            ).firstMatch
            try require(presetItem.waitForExistence(timeout: 5), "Tab did not reach the native configuration preset menu")
            app.typeKey(.escape, modifierFlags: [])
            try require(wait { !presetItem.exists || !presetItem.isHittable },
                        "Escape must close the native preset menu")
            _ = try element(app, "configuration.enabled")
            try capture(launch, name: "K8-compact-configuration-keyboard")
            try escapeSheet(launch, content: "configuration.enabled")
            try expect(app, "configuration.summary", "关闭 · 本项目 · 用户偏好")
            try click(launch, "configuration.open")
            try require(!(try booleanValue(element(app, "configuration.enabled"))), "Escape applied a configuration draft")
            try require((try element(app, "configuration.cancel")).isHittable
                        && (try element(app, "configuration.apply")).isHittable,
                        "Compact configuration footer must remain reachable")
            try dismiss(launch, button: "configuration.cancel", content: "configuration.enabled")

            // Compact batch actions are real and independent from its detail menu.
            try setToggle(launch, "batch.conciseReplies", to: true)
            try setToggle(launch, "batch.verifiedCheck", to: true)
            try click(launch, "batch.confirm")
            try expectContains(app, "feedback", "确认 2 条")
            try setToggle(launch, "batch.discardable", to: true)
            try click(launch, "batch.discard")
            try chooseSection(launch, "历史")
            try selectItem(launch, "discardable")
            try expect(app, "detail.status", "已丢弃示例 · 仅历史")

            // Tab between actual native editable fields has an observable
            // result: text goes to basis, not back into scope or the editor.
            try chooseScenario(launch, "K5 纠正与版本")
            try click(launch, "revise")
            let scope = "Fictional Harbor compact keyboard scope."
            try replaceText(launch, "draft.scope", with: scope)
            app.typeKey(.tab, modifierFlags: [])
            app.typeText("Keyboard-only fictional correction basis.")
            try expect(app, "draft.basis", "Keyboard-only fictional correction basis.")
            try expect(app, "draft.scope", scope)
            try capture(launch, name: "K8-compact-revision-Tab-field-focus")
            try dismiss(launch, button: "draft.apply", content: "draft.content")
            try click(launch, "history.open")
            try expect(app, "detail.version", "v1 · 已被替代的历史版本")
            try chooseMenu(launch, "history.version", title: "v2 · 当前样例版本")
            try expectContains(app, "detail.replacement", "Keyboard-only fictional correction basis.")

            try chooseScenario(launch, "K8 无持久化与重置")
            try selectItem(launch, "conciseReplies")
            try exerciseLongSource(launch, compactVerified: compactFits)
            try assertIsolation(launch, stage: "K8-appearance-keyboard-geometry")

            // Leave both applied changes AND an unsaved editor draft before
            // terminating. Relaunch below must restore neither of them.
            try dirtyWindowFixture(launch, marker: "terminate-witness")
            terminatedInstance = try memoryInstance(launch)
            terminatedPID = launch.pid
            terminatedStart = launch.started
            try click(launch, "edit")
            try replaceText(launch, "draft.content", with: "Unsaved terminate-witness draft")
            try capture(launch, name: "K8-before-owned-terminate-with-unsaved-draft")
        }

        // withLaunch has actually terminated the first owned App and checked
        // its directory and the controller-owned preference witness before
        // starting this new App.
        try withLaunch { relaunched in
            try require(relaunched.pid != terminatedPID || relaunched.started != terminatedStart,
                        "Expected a new process incarnation, not merely a reopened window")
            try require(try memoryInstance(relaunched) != terminatedInstance, "Relaunch reused the old Store instance")
            try assertPristineFixture(relaunched, code: "K1", inspectDrafts: true)
            try capture(relaunched, name: "K8-terminated-and-relaunched-pristine")
            try assertIsolation(relaunched, stage: "K8-after-relaunch")
        }
        if !unavailableSizes.isEmpty {
            // Only independently measured display limits can take this path.
            // Happy paths, missing AX, short sources and bad layout FAIL above.
            throw XCTSkip("NOT VERIFIED: display cannot fit \(unavailableSizes.joined(separator: ", ")); actual unscaled window/AX/geometry attached. Semantic and relaunch checks were still exercised.")
        }
    }

    // MARK: - 6. Refusals must not fall through to any product/review App

    func testInvalidKnowledgeRequestsNeverReachProduct() throws {
        // Every launch gets its own originally absent, validated Caches child
        // AND the preserved controller-owned marker. No unsafe or missing data path is
        // passed to the app, even when testing the refusal boundary.
        var requests = [
            Request(name: "wrong-environment", environment: "default",
                    refusalReasons: ["invalidEnvironment"]),
            Request(name: "empty-environment", environment: "",
                    refusalReasons: ["invalidEnvironment"]),
        ]
        // Singles, every pair, and all three old/reserved flags. They are
        // conflicts, not a priority order which may launch another sample.
        for mask in 1...7 {
            requests.append(Request(
                name: "conflict-\(mask)",
                evolution: mask & 1 == 0 ? "0" : "1",
                visual: mask & 2 == 0 ? "0" : "1",
                collaboration: mask & 4 == 0 ? "0" : "1",
                refusalReasons: ["conflictingReview"]
            ))
        }
        requests.append(Request(
            name: "combined-invalid-request", environment: "default",
            evolution: "1", visual: "1", collaboration: "1",
            refusalReasons: ["invalidEnvironment", "conflictingReview"]
        ))

        for request in requests {
            try withLaunch(request) { launch in
                let app = launch.app
                try expect(app, "refusal.title", Self.refusalTitle)
                try expect(app, "refusal.notice", "只读拒绝页 · 未进入知识样例或产品工作区")
                for reason in ["invalidFlag", "invalidEnvironment", "invalidDirectory", "conflictingReview"] {
                    let leaf = query(app, "refusal.reason.\(reason)")
                    if request.refusalReasons.contains(reason) {
                        let actual = try element(app, "refusal.reason.\(reason)")
                        try require(!text(actual).trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                                    "Refusal needs readable reason text: \(reason)")
                        try require(!text(actual).contains(launch.directory.child.path),
                                    "The refusal must not echo its data path")
                    } else {
                        try require(!leaf.exists, "Unexpected refusal reason: \(reason)")
                    }
                }
                for id in ["notice", "project", "scenario", "configuration.open"] {
                    try require(!query(app, id).exists, "A refusal must not claim the sample started: \(id)")
                }
                // Search only this app's AX. No account path or environment
                // dump is included in the refusal's user-facing content.
                let ax = launch.window.debugDescription
                try require(!ax.contains(launch.directory.child.path), "Refusal leaked the data directory")
                try assertIsolation(launch, stage: "refused-\(request.name)")
                try capture(launch, name: "refused-\(request.name)")
            }
        }
    }

    // MARK: - Launch ownership, environment, directory and preference witnesses

    private func withLaunch(_ request: Request = Request(),
                            _ body: (Launch) throws -> Void) throws {
        let directory = try makeDirectoryWitness()
        // Do not let XCUIApplication.launch() terminate an unrelated existing
        // instance. The gate owns exclusivity; this is an additional fail-closed
        // check, not a substitute for that handoff.
        try require(NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID).isEmpty,
                    "An Aeroric instance already exists; stop and return ownership to main, never kill it")
        let app = XCUIApplication(bundleIdentifier: Self.bundleID)
        try require(app.state == .notRunning, "The target must be stopped before this test-owned launch")
        let preference = try loadPreferenceWitness(accountHome: directory.accountHome)
        var launch: Launch?
        defer {
            if let launch {
                finishLaunch(launch)
            } else {
                // Before ownership is established, never terminate a process
                // found by name. The VM controller alone collects the marker.
                checkPreferenceWitness(preference, stage: "launch-not-established")
                checkDirectoryAfter(directory, stage: "launch-not-established")
            }
        }

        var overrides = app.launchEnvironment
        let explicit = Dictionary(uniqueKeysWithValues: zip(Self.reviewKeys, request.flagValues))
            .merging(["AERORIC_ENV": request.environment, "AERORIC_DATA_DIR": directory.child.path]) {
                _, new in new
            }
        for (key, value) in explicit {
            // Removing a dictionary entry alone does NOT neutralize inherited
            // XCTest environment. Override both spellings, including "0"/"".
            overrides[key] = value
            overrides["TEST_RUNNER_\(key)"] = value
        }
        app.launchArguments = []
        app.launchEnvironment = overrides
        attachText("""
        request=\(request.name)
        direct-and-TEST_RUNNER flags=\(zip(Self.reviewKeys, request.flagValues).map { "\($0.0)=\($0.1)" }.joined(separator: ", "))
        AERORIC_ENV=\(request.environment.debugDescription)
        data child=\(directory.child.lastPathComponent)
        parent enumerated; count=\(directory.beforeCount), SHA256=\(directory.beforeDigest)
        actual account home resolved by getpwuid_r; no child mkdir
        External isolated-account/desktop authorization is a precondition, not proved by this environment.
        """, name: "launch-\(request.name)")

        app.launch()
        try require(wait(timeout: 10) {
            NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID).count == 1
        }, "Cannot identify exactly one newly launched dev-app instance")
        let running = NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID)
        let process = try XCTUnwrap(running.first)
        let owned = Launch(app: app, request: request, directory: directory,
                           preference: preference, process: process)
        launch = owned
        do {
            // Like the existing native visual suite, activate our identified
            // process before waiting for its window's accessibility surface.
            // A background launch can otherwise expose only the menu bar.
            try assertOwnership(owned)
            owned.app.activate()
            try activate(owned)
            try require(owned.window.waitForExistence(timeout: 15),
                        "Expected \(request.isRefused ? "refusal" : "knowledge sample") window did not appear")
            if !request.isRefused {
                try expect(app, "notice", "离线知识审阅 · 非真实资料")
                _ = try memoryInstance(owned)
            }
            try assertIsolation(owned, stage: "after-launch")
            try capture(owned, name: "\(request.name)-initial")
            try body(owned)
        } catch {
            attachText(owned.app.debugDescription, name: "\(request.name)-failure-AX")
            if owned.window.exists {
                do { try capture(owned, name: "\(request.name)-failure") }
                catch { attachText(String(describing: error), name: "failure-capture-not-verified") }
            }
            throw error
        }
    }

    private func makeDirectoryWitness() throws -> DirectoryWitness {
        let environment = ProcessInfo.processInfo.environment
        // These are existing runner variables, not new opt-ins. Some XCTest
        // versions retain the forwarding names as well as their stripped form.
        func runnerValue(_ key: String) throws -> String {
            let direct = environment[key]
            let forwarded = environment["TEST_RUNNER_\(key)"]
            if let direct, let forwarded {
                try require(direct == forwarded, "Runner has conflicting \(key) inheritance")
            }
            return try XCTUnwrap(direct ?? forwarded, "Missing native UI runner \(key)")
        }
        try require(try runnerValue("AERORIC_ENV") == "ui-test", "Use only the authorized native UI runner")
        let accountHome = try accountHomeDirectory()
        let rootPath = try runnerValue("AERORIC_DATA_DIR")
        try require(NativeVisualReviewEntry.isIsolatedDataDirectory(rootPath, home: accountHome),
                    "Runner data root must satisfy the existing real-account Caches guard")
        let parent = URL(fileURLWithPath: rootPath, isDirectory: true)
        // Enumeration must succeed. fileExists == false can hide permission
        // failures or dangling links and is not an absence witness.
        let entries = try FileManager.default.contentsOfDirectory(atPath: parent.path)
        let child = parent.appendingPathComponent("knowledge-review-\(UUID().uuidString)", isDirectory: true)
        try require(!entries.contains(child.lastPathComponent), "The unique sample directory already exists")
        try require(NativeVisualReviewEntry.isIsolatedDataDirectory(child.path, home: accountHome),
                    "The generated data child failed the shared guard")
        return DirectoryWitness(accountHome: accountHome, parent: parent, child: child,
                                beforeCount: entries.count, beforeDigest: digest(entries.sorted().joined(separator: "\n")))
    }

    private func accountHomeDirectory() throws -> String {
        try require(getuid() == geteuid(), "Do not run UI preference witnesses with a changed effective account")
        var capacity = 16_384
        while capacity <= 1_048_576 {
            var record = passwd()
            var result: UnsafeMutablePointer<passwd>?
            var buffer = [CChar](repeating: 0, count: capacity)
            var home: String?
            let code = buffer.withUnsafeMutableBufferPointer { bytes in
                guard let base = bytes.baseAddress else { return Int32(ERANGE) }
                let status = getpwuid_r(getuid(), &record, base, bytes.count, &result)
                if status == 0, result != nil, let path = record.pw_dir {
                    home = String(cString: path)
                }
                return status
            }
            if code == ERANGE { capacity *= 2; continue }
            try require(code == 0 && result != nil, "getpwuid_r could not resolve the real account")
            let actual = try XCTUnwrap(home)
            try require(actual.hasPrefix("/") && actual != "/", "Invalid account home")
            return actual
        }
        throw ReviewFailure(description: "getpwuid_r exceeded its bounded lookup buffer")
    }

    private func assertDirectoryAbsent(_ witness: DirectoryWitness, stage: String) throws {
        try require(NativeVisualReviewEntry.isIsolatedDataDirectory(
            witness.child.path, home: witness.accountHome
        ), "Directory boundary changed during \(stage)")
        let entries = try FileManager.default.contentsOfDirectory(atPath: witness.parent.path)
        let absent = !entries.contains(witness.child.lastPathComponent)
        attachText("""
        stage=\(stage); child=\(witness.child.lastPathComponent)
        before parent count=\(witness.beforeCount); SHA256=\(witness.beforeDigest)
        after parent count=\(entries.count); SHA256=\(digest(entries.sorted().joined(separator: "\n")))
        childAbsent=\(absent)
        Enumerated an existing parent; this test never creates/removes the data child.
        """, name: "directory-\(stage)")
        try require(absent, "The offline review created its data directory; preserve it for main's investigation")
    }

    private func checkDirectoryAfter(_ witness: DirectoryWitness, stage: String) {
        do { try assertDirectoryAbsent(witness, stage: stage) }
        catch { XCTFail("Directory witness failed: \(error)") }
    }

    private struct UtilityResult {
        let status: Int32
        let output: String
    }

    /// Only fixed system utilities, for this test's processes/marker. No shell,
    /// network, host, agent, global process matching or real preference dump.
    private func utility(_ executable: String, _ arguments: [String]) throws -> UtilityResult {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        process.standardOutput = pipe
        process.standardError = pipe
        try process.run()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return UtilityResult(status: process.terminationStatus,
                             output: String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines))
    }

    private func loadPreferenceWitness(accountHome: String) throws -> PreferenceWitness {
        let environment = ProcessInfo.processInfo.environment
        let variable = "AERORIC_KNOWLEDGE_PREFERENCE_WITNESS"
        let direct = environment[variable]
        let forwarded = environment["TEST_RUNNER_\(variable)"]
        if let direct, let forwarded {
            try require(direct == forwarded, "Conflicting VM preference-witness descriptors")
        }
        let encoded = try XCTUnwrap(direct ?? forwarded, "Missing authorized VM preference-witness descriptor")
        let descriptor = try JSONDecoder().decode(PreferenceDescriptor.self, from: Data(encoded.utf8))
        try require(descriptor.version == 1 && descriptor.accountUID == getuid(),
                    "The controller witness belongs to a different protocol or account")
        try require(descriptor.accountHome == accountHome && descriptor.bundleID == Self.bundleID,
                    "The witness must use the actual account's dev-app domain")
        let methodPrefix = "AeroricNativeUITests/NativeKnowledgeReviewUITests/"
        try require(descriptor.method.hasPrefix(methodPrefix),
                    "The witness is not for this knowledge suite")
        let method = String(descriptor.method.dropFirst(methodPrefix.count))
        try require(!method.isEmpty && !method.contains("/") && name.hasSuffix(" \(method)]"),
                    "The controller witness belongs to a different test method")
        try require(UUID(uuidString: descriptor.attemptID) != nil
                    && descriptor.key == "knowledgeReviewUITestMarker_\(descriptor.attemptID)",
                    "Invalid per-attempt controller marker identity")
        let tokenPrefix = "nonsensitive-offline-witness-"
        try require(descriptor.token.hasPrefix(tokenPrefix)
                    && UUID(uuidString: String(descriptor.token.dropFirst(tokenPrefix.count))) != nil,
                    "Invalid nonsensitive controller token")
        let domain = URL(fileURLWithPath: accountHome, isDirectory: true)
            .appendingPathComponent("Library/Preferences", isDirectory: true)
            .appendingPathComponent(Self.bundleID).path
        let witness = PreferenceWitness(
            domain: domain, key: descriptor.key, token: descriptor.token
        )
        attachText("controller method=\(descriptor.method); attempt=\(descriptor.attemptID); XCTest read-only",
                   name: "preference-controller")
        try assertPreferencePreserved(witness, stage: "before-launch")
        return witness
    }

    private func assertPreferencePreserved(_ witness: PreferenceWitness, stage: String) throws {
        let read = try utility("/usr/bin/defaults", ["read", witness.domain, witness.key])
        let preserved = read.status == 0 && read.output == witness.token
        attachText("""
        stage=\(stage); marker=\(witness.key); preserved=\(preserved); readExit=\(read.status)
        Target: actual isolated account's \(Self.bundleID), not runner-container defaults.
        Only this nonsensitive key was read. Its survival detects domain clearing,
        not all possible writes. AppKit/SwiftUI may persist transient window state;
        this is NOT a claim of zero OS-level preference changes.
        """, name: "preference-\(stage)")
        try require(preserved, "The sample/refusal cleared or changed the test-owned preference marker")
    }

    private func checkPreferenceWitness(_ witness: PreferenceWitness, stage: String) {
        do {
            try assertPreferencePreserved(witness, stage: stage)
        } catch {
            XCTFail("Isolated preference witness: \(error)")
        }
    }

    private func assertOwnership(_ launch: Launch) throws {
        let matching = NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID)
        try require(matching.count == 1 && matching.first?.processIdentifier == launch.pid,
                    "The dev-app process identity changed; never manipulate a replacement instance")
        try require(matching.first?.launchDate == launch.started,
                    "The test-owned app is no longer the same live process")
    }

    private func childPIDs(of pid: pid_t) throws -> [pid_t] {
        try require(pid > 0, "Process observation must be rooted at a known positive PID")
        var buffer = [pid_t](repeating: 0, count: 256)
        let capacity = buffer.count * MemoryLayout<pid_t>.stride
        let bytes = buffer.withUnsafeMutableBytes {
            // Use the byte-count API directly, not a convenience wrapper
            // whose result is a PID count. PROC_PPID_ONLY is scoped to this
            // already-owned parent, never a global process-name search.
            proc_listpids(UInt32(PROC_PPID_ONLY), UInt32(pid), $0.baseAddress, Int32($0.count))
        }
        try require(bytes >= 0 && Int(bytes) < capacity && Int(bytes) % MemoryLayout<pid_t>.stride == 0,
                    "Cannot obtain a complete own-process child list for PID \(pid)")
        return Array(buffer.prefix(Int(bytes) / MemoryLayout<pid_t>.stride)).filter { $0 > 0 }
    }

    private func executablePath(of pid: pid_t) -> String? {
        var buffer = [CChar](repeating: 0, count: 4096)
        let count = buffer.withUnsafeMutableBytes {
            proc_pidpath(pid, $0.baseAddress, UInt32($0.count))
        }
        guard count > 0 else { return nil }
        return buffer.withUnsafeBufferPointer { bytes in
            bytes.baseAddress.map { String(cString: $0) }
        }
    }

    private func observeProcesses(_ launch: Launch, stage: String) throws {
        try assertOwnership(launch)
        var pending = [launch.pid]
        var visited = Set<pid_t>()
        while let parent = pending.popLast() {
            try require(visited.count < 256, "Unexpectedly large process tree; preserve evidence and stop")
            if !visited.insert(parent).inserted { continue }
            for child in try childPIDs(of: parent) {
                launch.observedChildren[child] = executablePath(of: child) ?? "<exited-or-unreadable>"
                pending.append(child)
            }
        }
        // libproc above supplies the child-list assertion. Read the owned
        // application's identity through AppKit rather than spawning /bin/ps,
        // which this actual XCTest sandbox rejects with EPERM.
        let current = try XCTUnwrap(
            NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID)
                .first { $0.processIdentifier == launch.pid }
        )
        try require(current.launchDate == launch.started && current.executableURL != nil,
                    "Cannot read the owned review process identity")
        attachText("""
        stage=\(stage); owned root PID=\(launch.pid); actual account UID=\(getuid())
        launchDate=\(String(describing: current.launchDate))
        executable=\(current.executableURL?.path ?? "<unreadable>")
        Observed descendants (libproc, rooted ONLY at our launched PID):
        \(launch.observedChildren.sorted { $0.key < $1.key }.map { "\($0.key) \($0.value)" }.joined(separator: "\n"))
        Point-in-time observations cannot rule out an arbitrarily short-lived or
        already-reparented process. Correlate with main's entry/delegate review
        and directory/preference evidence; never substitute a static host=0 label.
        """, name: "process-\(stage)")
        try require(launch.observedChildren.isEmpty, "The offline sample/refusal spawned a child process")
    }

    private func assertIsolation(_ launch: Launch, stage: String) throws {
        for id in ["bootstrap.environment", "sidebar.list", "statusbar.phase"] {
            try require(!launch.app.descendants(matching: .any).matching(identifier: id).firstMatch.exists,
                        "Product AX unexpectedly appeared: \(id)")
        }
        for id in ["evolutionReview.offlineBadge", "review.workspace.refresh"] {
            try require(!launch.app.descendants(matching: .any).matching(identifier: id).firstMatch.exists,
                        "Another review entry unexpectedly appeared: \(id)")
        }
        try observeProcesses(launch, stage: stage)
        try assertDirectoryAbsent(launch.directory, stage: stage)
        try assertPreferencePreserved(launch.preference, stage: stage)
    }

    private func finishLaunch(_ launch: Launch) {
        // Checks are independent: a failed witness must not bypass termination
        // of our still-owned App or the other witnesses. Marker collection is
        // outside this sandbox, after the complete xcodebuild attempt exits.
        do {
            if launch.app.state != .notRunning { try observeProcesses(launch, stage: "before-terminate") }
            try assertPreferencePreserved(launch.preference, stage: "before-terminate")
        } catch { XCTFail("Before termination: \(error)") }
        if launch.app.state != .notRunning {
            do {
                try assertOwnership(launch)
                launch.app.terminate() // Only the XCUIApplication this helper launched.
            } catch { XCTFail("Owned App cleanup: \(error); no global kill attempted") }
        }
        // The already-notRunning fast path needs the same independent exit
        // witness; do not infer process exit from XCTest state alone.
        do {
            try require(wait(timeout: 10) {
                launch.app.state == .notRunning
                    && !NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID)
                        .contains { $0.processIdentifier == launch.pid && $0.launchDate == launch.started }
            }, "Owned App did not terminate")
        } catch { XCTFail("Owned App exit witness: \(error)") }
        for (pid, path) in launch.observedChildren {
            if let stillRunning = executablePath(of: pid), stillRunning == path {
                XCTFail("Observed child \(pid) survived App termination (\(path)); not killed, return to main")
            }
        }
        checkDirectoryAfter(launch.directory, stage: "after-terminate")
        checkPreferenceWitness(launch.preference, stage: "after-terminate")
        attachText("owned PID=\(launch.pid); XCTest state=\(launch.app.state.rawValue)",
                   name: "owned-process-after-terminate")
    }

    // MARK: - Real AX, controls and evidence

    private func query(_ app: XCUIApplication, _ suffix: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: Self.prefix + suffix).firstMatch
    }

    @discardableResult
    private func element(_ app: XCUIApplication, _ suffix: String) throws -> XCUIElement {
        let found = query(app, suffix)
        try require(found.exists || found.waitForExistence(timeout: 10), "Missing AX leaf \(Self.prefix)\(suffix)")
        try require(hasReadableName(found),
                    "AX leaf needs a readable name: \(suffix)")
        return found
    }

    private func hasReadableName(_ element: XCUIElement) -> Bool {
        func readable(_ value: String?) -> Bool {
            guard let value else { return false }
            let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
            return !trimmed.isEmpty && trimmed != element.identifier
        }
        if readable(element.label) || readable(element.title) { return true }
        // AppKit exposes SwiftUI Text through AXValue. A native control must
        // still have a semantic name; an identifier or Boolean is not one.
        if element.elementType == .staticText { return readable(element.value as? String) }
        if [.textField, .textView, .secureTextField].contains(element.elementType) {
            return readable(element.placeholderValue)
        }
        return false
    }

    private func text(_ element: XCUIElement) -> String {
        if let value = element.value as? String, !value.isEmpty { return value }
        if !element.label.isEmpty { return element.label }
        return element.title
    }

    private func value(_ app: XCUIApplication, _ suffix: String) -> String {
        text(query(app, suffix))
    }

    private func expect(_ app: XCUIApplication, _ suffix: String, _ expected: String) throws {
        _ = try element(app, suffix)
        try require(wait { self.value(app, suffix) == expected },
                    "\(suffix): expected \(expected.debugDescription), observed \(value(app, suffix).debugDescription)")
    }

    private func expectContains(_ app: XCUIApplication, _ suffix: String, _ expected: String) throws {
        _ = try element(app, suffix)
        try require(wait { self.value(app, suffix).contains(expected) },
                    "\(suffix): expected to contain \(expected.debugDescription), observed \(value(app, suffix).debugDescription)")
    }

    private func click(_ launch: Launch, _ suffix: String) throws {
        let target = try element(launch.app, suffix)
        try reveal(launch, target: target)
        try require(target.isEnabled, "Disabled action must not be clicked: \(suffix)")
        try activate(launch)
        target.click()
    }

    private func scrollOwner(_ launch: Launch, containing identifier: String) throws -> XCUIElement {
        let owners = launch.app.scrollViews.containing(.any, identifier: identifier)
            .allElementsBoundByIndex.filter { !$0.frame.isEmpty }
        let nearest = owners.min { lhs, rhs in
            lhs.frame.width * lhs.frame.height < rhs.frame.width * rhs.frame.height
        }
        return try XCTUnwrap(nearest, "No actual native scroll owner for \(identifier)")
    }

    private func fullyVisible(_ element: XCUIElement, in viewport: XCUIElement) -> Bool {
        element.exists && element.isHittable && !element.frame.isEmpty
            && viewport.frame.insetBy(dx: -1, dy: -1).contains(element.frame)
    }

    private func reveal(_ launch: Launch, target: XCUIElement, anchor: XCUIElement? = nil) throws {
        // A List/TextEditor may itself be outside the root ScrollView. First
        // expose its viewport, then scroll within it. Scrolling only the
        // smallest owner cannot reveal an offscreen nested control.
        let locator = anchor ?? target
        let identifier = locator.identifier.isEmpty ? locator.label : locator.identifier
        try require(!identifier.isEmpty, "A scroll target needs a readable AX locator")
        let sheets = launch.app.sheets.containing(locator.elementType, identifier: identifier)
            .allElementsBoundByIndex
        try require(sheets.count <= 1, "Ambiguous containing sheet for \(identifier)")
        let surface = sheets.first ?? launch.window
        let owners = surface.scrollViews.containing(locator.elementType, identifier: identifier)
            .allElementsBoundByIndex.filter { !$0.frame.isEmpty }
            .map { (element: $0, descendants: $0.scrollViews.count) }
            // All contain the same unique target, so the ancestor has more
            // descendant scroll views than its child, regardless of frame size.
            .sorted { $0.descendants > $1.descendants }.map { $0.element }
        func viewport() -> CGRect {
            owners.reduce(surface.frame.intersection(launch.window.frame)) {
                $0.intersection($1.frame)
            }
        }
        let before = target.frame
        let beforeViewport = viewport()
        var scrolled = false
        func reached() -> Bool {
            let visible = viewport()
            guard target.exists, target.isHittable, !target.frame.isEmpty,
                  !visible.isNull, visible.insetBy(dx: -1, dy: -1).contains(target.frame) else {
                return false
            }
            if scrolled {
                attachText("""
                target=\(target.identifier); native containing scroll ancestors=\(owners.count)
                before target=\(before); effective viewport=\(beforeViewport)
                after target=\(target.frame); effective viewport=\(visible)
                fullyVisible=true; hittable=true
                Hittability alone did not establish an unclipped interaction target.
                """, name: "native-scroll-revealed-\(identifier)")
            }
            return true
        }
        // Fixed header/footer controls legitimately have no scroll ancestors.
        // A partly clipped checkbox can be hittable yet receive no click.
        if reached() { return }
        try require(!owners.isEmpty, "No actual native scroll owner for \(identifier)")

        for index in owners.indices {
            let scroll = owners[index]
            let hasInnerOwner = index + 1 < owners.count
            let next = hasInnerOwner ? owners[index + 1] : target
            for attempt in 0..<40 {
                if reached() { return }
                let viewport = owners.prefix(index + 1).reduce(surface.frame.intersection(launch.window.frame)) {
                    $0.intersection($1.frame)
                }
                try require(!viewport.isNull && viewport.width > 4 && viewport.height > 4,
                            "An owning scroll viewport is outside the own window: \(identifier)")
                let frame = next.frame
                if hasInnerOwner, next.isHittable, !frame.isEmpty {
                    let overlap = viewport.intersection(frame)
                    if !overlap.isNull,
                       overlap.height >= min(viewport.height, frame.height) - 2 {
                        break
                    }
                }
                let upwards = frame.isEmpty ? attempt >= 20 : frame.midY < viewport.midY
                let delta = frame.isEmpty ? CGFloat(180)
                    : min(180, max(36, abs(frame.midY - viewport.midY)))
                // Aim inside this owner's visible leading gutter, not at its
                // center (which could send the wheel event to an inner List).
                let point = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
                    .withOffset(CGVector(dx: viewport.minX - scroll.frame.minX + 2,
                                         dy: viewport.midY - scroll.frame.minY))
                try activate(launch)
                point.scroll(byDeltaX: 0, deltaY: upwards ? delta : -delta)
                scrolled = true
            }
        }
        try require(reached(), "Cannot fully reveal \(target.identifier) using its native scroll ancestors")
    }

    private func chooseMenu(_ launch: Launch, _ suffix: String, title: String) throws {
        let control = try element(launch.app, suffix)
        try require([XCUIElement.ElementType.menuButton, .popUpButton, .button].contains(control.elementType),
                    "\(suffix) must be a real native menu control")
        try click(launch, suffix)
        let item = launch.app.menuItems.matching(
            NSPredicate(format: "label == %@ OR title == %@", title, title)
        ).firstMatch
        try require(item.waitForExistence(timeout: 5), "Missing real menu choice \(suffix) → \(title)")
        try require(item.isEnabled, "Menu choice is disabled: \(title)")
        item.click()
        // A closed native menu can retain AX-addressable items.
        try require(wait { !item.exists || !item.isHittable },
                    "Native menu did not dismiss after selection: \(title)")
        try expect(launch.app, suffix, title) // Explicit selected accessibilityValue.
    }

    private func chooseSegment(_ launch: Launch, group: String, title: String) throws {
        try activate(launch)
        let named = NSPredicate(format: "label == %@ OR title == %@", group, group)
        let containers = [launch.app.radioGroups, launch.app.segmentedControls]
            .map { $0.matching(named).firstMatch }
        let container = try XCTUnwrap(containers.first(where: { $0.exists }),
                                      "Missing named system segmented control: \(group)")
        let target = container.radioButtons.matching(
            NSPredicate(format: "label == %@ OR title == %@", title, title)
        ).firstMatch
        try require(target.waitForExistence(timeout: 5), "Missing system segment \(group) → \(title)")
        // Shared NativeSelection leaves its identifiers unset; use the named
        // native group solely as the ancestry locator for the same reveal path.
        try reveal(launch, target: target, anchor: container)
        try require(target.isHittable, "System segment is clipped: \(group) → \(title)")
        target.click()
        try require(wait {
            target.isSelected || (target.value as? NSNumber)?.boolValue == true
                || (target.value as? String) == "1"
        }, "The native segment did not enter its system selection state: \(title)")
    }

    private func chooseSection(_ launch: Launch, _ title: String) throws {
        if query(launch.app, "section").exists {
            try chooseMenu(launch, "section", title: title)
        } else {
            try chooseSegment(launch, group: "知识栏目", title: title)
        }
    }

    private func chooseScenario(_ launch: Launch, _ title: String) throws {
        let previous = value(launch.app, "scenario")
        try chooseMenu(launch, "scenario", title: title)
        if previous != title { try expectContains(launch.app, "feedback", title) }
        try expect(launch.app, "notice", "离线知识审阅 · 非真实资料")
    }

    private static let itemTitles: [String: String] = [
        "conciseReplies": "虚构偏好 · 简短结论",
        "verifiedCheck": "虚构解法 · 空字段提示",
        "discardable": "虚构提议 · 临时术语",
        "selfReport": "虚构自报 · 重试已修好？",
        "databaseChoice": "虚构历史决策 · SQLite 原型",
        "interfaceContract": "虚构接口约定 · 错误响应",
        "docsGuidance": "虚构文档约定 · 独立术语",
        "agentGuess": "虚构新猜测 · 删除 message？",
        "newPreference": "虚构新偏好 · 先写适用条件",
        "newSolution": "虚构新解法 · 无结果提示",
        "conflict": "虚构冲突 · 删除用户要求字段",
        "sensitive": "虚构敏感例外 · 仅脱敏占位",
        "insufficient": "虚构依据不足 · 未提供检查",
        "outOfScope": "虚构范围外 · 山谷项目偏好",
    ]

    private func selectItem(_ launch: Launch, _ id: String) throws {
        let title = try XCTUnwrap(Self.itemTitles[id], "Unknown fixture ID in test: \(id)")
        if query(launch.app, "itemPicker").exists {
            try chooseMenu(launch, "itemPicker", title: title)
        } else {
            let leaf = try element(launch.app, "item.\(id)")
            try reveal(launch, target: leaf)
            let rows = [launch.app.outlineRows, launch.app.tableRows, launch.app.cells]
                .map { $0.containing(.staticText, identifier: leaf.identifier).firstMatch }
            let row = try XCTUnwrap(rows.first(where: { $0.exists }), "Missing native selection row for \(id)")
            try require(row.isHittable, "Native list row is not reachable: \(id)")
            try activate(launch)
            // The row's center can land on its separate batch checkbox. Its
            // non-selectable title is the real row-selection click target.
            leaf.click()
        }
        try expect(launch.app, "detail.title", title)
    }

    /// A hosted row title can select without making the native outline the
    /// keyboard responder. Click its real leading gutter, outside hosted cell
    /// content and its independent checkbox; never synthesize selection state.
    private func focusNativeListRow(_ launch: Launch, _ id: String) throws {
        let leaf = try element(launch.app, "item.\(id)")
        try reveal(launch, target: leaf)
        let outline = launch.app.outlines.containing(.staticText, identifier: leaf.identifier).firstMatch
        try require(outline.exists, "Keyboard selection requires the actual native outline")
        let row = outline.outlineRows.containing(.staticText, identifier: leaf.identifier).firstMatch
        let cell = row.cells.containing(.staticText, identifier: leaf.identifier).firstMatch
        try require(row.exists && cell.exists, "Missing native row/cell geometry for keyboard focus")
        let frame = row.frame
        let gutter = CGRect(x: frame.minX, y: leaf.frame.minY,
                            width: cell.frame.minX - frame.minX, height: leaf.frame.height)
        let viewport = try scrollOwner(launch, containing: leaf.identifier)
        try require(gutter.width >= 4 && gutter.height > 0
                    && viewport.frame.intersection(launch.window.frame).contains(gutter)
                    && frame.contains(gutter),
                    "No verified visible native row gutter; do not guess a focus coordinate")
        try activate(launch)
        row.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
            .withOffset(CGVector(dx: gutter.midX - frame.minX, dy: gutter.midY - frame.minY))
            .click()
        try expect(launch.app, "detail.title", try XCTUnwrap(Self.itemTitles[id]))
        try expect(launch.app, "batch.summary", "0 条")
        attachText("""
        native outline=\(outline.frame); row=\(frame); hosted cell=\(cell.frame)
        visible leading gutter=\(gutter); actual click=(\(gutter.midX), \(gutter.midY))
        No focus claim is inferred from AX Keyboard Focused propagation.
        The following unchanged Down/Up assertions verify native keyboard routing.
        """, name: "native-list-keyboard-focus-input")
    }

    private func booleanValue(_ control: XCUIElement) throws -> Bool {
        if let value = control.value as? NSNumber { return value.boolValue }
        if let value = control.value as? String {
            if ["1", "on", "true"].contains(value) { return true }
            if ["0", "off", "false"].contains(value) { return false }
        }
        throw ReviewFailure(description: "Native toggle has no readable Boolean value: \(control.identifier)")
    }

    private func setToggle(_ launch: Launch, _ suffix: String, to enabled: Bool) throws {
        let control = try element(launch.app, suffix)
        try require([XCUIElement.ElementType.checkBox, .switch, .toggle, .button].contains(control.elementType),
                    "Expected a system checkbox/toggle: \(suffix)")
        let before = try booleanValue(control)
        if before != enabled { try click(launch, suffix) }
        try require(wait { (try? self.booleanValue(control)) == enabled }, "Toggle did not change: \(suffix)")
        attachText("target=\(control.identifier); before=\(before); after=\(try booleanValue(control)); frame=\(control.frame)",
                   name: "native-toggle-\(suffix)")
    }

    private func replaceText(_ launch: Launch, _ suffix: String, with replacement: String) throws {
        let control = try element(launch.app, suffix)
        try reveal(launch, target: control)
        try activate(launch)
        control.click()
        control.typeKey("a", modifierFlags: .command)
        if replacement.isEmpty {
            control.typeKey(.delete, modifierFlags: [])
        } else {
            control.typeText(replacement)
        }
        try expect(launch.app, suffix, replacement)
    }

    private func dismiss(_ launch: Launch, button: String, content: String) throws {
        try click(launch, button)
        try require(wait { !self.query(launch.app, content).exists }, "Sheet did not dismiss: \(content)")
    }

    private func openAppearance(_ launch: Launch) throws {
        if !query(launch.app, "appearance.sheet").exists {
            try click(launch, "appearance")
            _ = try element(launch.app, "appearance.sheet")
        }
    }

    @discardableResult
    private func memoryInstance(_ launch: Launch) throws -> String {
        try expect(launch.app, "process.pid", String(launch.pid))
        let instance = text(try element(launch.app, "memory.instance"))
        try require(UUID(uuidString: instance) != nil, "Expected the actual Store UUID, not a fixture counter")
        return instance
    }

    private func configureAdmission(_ launch: Launch, enabled: Bool, preset: String) throws {
        try click(launch, "configuration.open")
        try setToggle(launch, "configuration.enabled", to: enabled)
        try chooseMenu(launch, "configuration.preset", title: preset)
        try expectContains(launch.app, "configuration.scope", "虚构项目 · 海港")
        try expectContains(launch.app, "configuration.scope", "不扩权或生成技能")
        try dismiss(launch, button: "configuration.apply", content: "configuration.enabled")
        try expect(launch.app, "configuration.summary", "\(enabled ? "开启" : "关闭") · \(preset)")
    }

    private func assertExceptionsRemainCandidates(_ launch: Launch) throws {
        try chooseSection(launch, "候选")
        try expect(launch.app, "batch.summary", "0 条")
        let exceptions = [
            ("conflict", "与虚构用户已确认的接口约定冲突"),
            ("sensitive", "已移除的敏感内容占位"),
            ("insufficient", "不能作为已核验成功经验"),
            ("outOfScope", "属于另一虚构项目"),
        ]
        for (id, reason) in exceptions {
            try selectItem(launch, id)
            try expect(launch.app, "detail.status", "候选 · 待审阅")
            try expectContains(launch.app, "detail.blocked", reason)
            if id == "sensitive" {
                try expect(launch.app, "detail.content", "［已移除的敏感内容占位］")
                try expectContains(launch.app, "detail.source", "不含敏感原文")
            } else if id == "outOfScope" {
                try expectContains(launch.app, "detail.scope", "虚构项目 · 山谷（范围外）")
                try expectContains(launch.app, "detail.limitations", "不扩到全局、不生成技能")
            }
            try setToggle(launch, "batch.\(id)", to: true)
            try expect(launch.app, "batch.summary", "1 条")
            try require(!(try element(launch.app, "batch.confirm")).isEnabled,
                        "An exception cannot be forced through by automatic admission or Full Access: \(id)")
            // A previous blocked selection must not mask this item's gate.
            try setToggle(launch, "batch.\(id)", to: false)
            try expect(launch.app, "batch.summary", "0 条")
        }
        for (id, _) in exceptions {
            try setToggle(launch, "batch.\(id)", to: true)
        }
        try expect(launch.app, "batch.summary", "4 条")
        try require(!(try element(launch.app, "batch.confirm")).isEnabled,
                    "The complete exception batch must also remain blocked")
    }

    private func ensureReferences(_ launch: Launch) throws {
        if !query(launch.app, "references.count").exists { try click(launch, "references.toggle") }
        _ = try element(launch.app, "references.count")
    }

    private func ensureImpacts(_ launch: Launch) throws {
        if !query(launch.app, "impact.apiTask.status").exists { try click(launch, "impacts.toggle") }
        _ = try element(launch.app, "impact.apiTask.status")
    }

    private func assertOnlyReference(_ launch: Launch, id: String) throws {
        _ = try element(launch.app, "reference.\(id).summary")
        let summaries = launch.app.descendants(matching: .any).matching(NSPredicate(
            format: "identifier BEGINSWITH %@ AND identifier ENDSWITH %@",
            Self.prefix + "reference.", ".summary"
        ))
        try require(summaries.count == 1, "Only this work's concise reference should be rendered, not the whole library")
        for other in Self.itemTitles.keys where other != id {
            try require(!query(launch.app, "reference.\(other).summary").exists,
                        "Unrelated knowledge appeared in the current work's reference: \(other)")
        }
    }

    private func dirtyWindowFixture(_ launch: Launch, marker: String) throws {
        try chooseSection(launch, "候选")
        try selectItem(launch, "conciseReplies")
        try click(launch, "edit")
        try replaceText(launch, "draft.content", with: "Offline applied content: \(marker)")
        try replaceText(launch, "draft.scope", with: "Fictional Harbor scope: \(marker)")
        try dismiss(launch, button: "draft.apply", content: "draft.content")
        try setToggle(launch, "batch.verifiedCheck", to: true)
        try configureAdmission(launch, enabled: true, preset: "本项目 · 用户偏好")
        try chooseMenu(launch, "permission", title: "Full Access 示例")
        try click(launch, "newCandidates.process")
        try expectContains(launch.app, "feedback", "1 条按预设免逐条确认")
        try chooseSection(launch, "可复用内容")
        try selectItem(launch, "interfaceContract")
        try click(launch, "revise")
        try replaceText(launch, "draft.basis", with: "Fictional user correction basis: \(marker)")
        try dismiss(launch, button: "draft.apply", content: "draft.content")
        try expect(launch.app, "detail.version", "v2 · 当前样例版本")
        try chooseSection(launch, "候选")
        try selectItem(launch, "conciseReplies")
        try expect(launch.app, "detail.content", "Offline applied content: \(marker)")
        try expect(launch.app, "batch.summary", "1 条")
        try expect(launch.app, "configuration.summary", "开启 · 本项目 · 用户偏好")
        try expect(launch.app, "permission", "Full Access 示例")
    }

    private func assertPristineFixture(_ launch: Launch, code: String, inspectDrafts: Bool = false) throws {
        let app = launch.app
        try expectContains(app, "feedback", code)
        try expect(app, "detail.title", "虚构偏好 · 简短结论")
        try expect(app, "detail.content", "先给简短结论，再列必要依据。")
        try expect(app, "detail.scope", "虚构项目 · 海港 · 海港项目的结果说明与审阅摘要。")
        try expect(app, "configuration.summary", "关闭 · 本项目 · 用户偏好")
        try expect(app, "permission", "默认授权示例")
        try expect(app, "batch.summary", "0 条")
        try require((try element(app, "newCandidates.process")).isEnabled, "New-batch state leaked across reset/reopen")
        try require(!query(app, "batch.newPreference").exists && !query(app, "batch.newSolution").exists,
                    "Processed new candidates leaked into the initial fixture")
        for id in ["conciseReplies", "verifiedCheck", "discardable", "selfReport"] {
            try require(!(try booleanValue(element(app, "batch.\(id)"))), "Batch selection leaked: \(id)")
        }
        try require(!query(app, "draft.content").exists && !query(app, "configuration.enabled").exists,
                    "An old sheet survived memory reconstruction")
        try chooseSection(launch, "可复用内容")
        try selectItem(launch, "interfaceContract")
        try expect(app, "detail.version", "v1 · 当前样例版本")
        try require(!query(app, "history.open").exists, "An applied revision survived reset/reopen")
        try chooseSection(launch, "候选")
        try selectItem(launch, "conciseReplies")
        if inspectDrafts {
            try click(launch, "edit")
            try expect(app, "draft.content", "先给简短结论，再列必要依据。")
            try expect(app, "draft.scope", "海港项目的结果说明与审阅摘要。")
            try dismiss(launch, button: "draft.cancel", content: "draft.content")
            try click(launch, "configuration.open")
            try require(!(try booleanValue(element(app, "configuration.enabled"))), "Reopened config reused an old draft")
            try expectContains(app, "configuration.scope", "偏好／纠正示例")
            try dismiss(launch, button: "configuration.cancel", content: "configuration.enabled")
        }
    }

    private func escapeSheet(_ launch: Launch, content: String) throws {
        _ = try element(launch.app, content)
        try activate(launch)
        launch.app.typeKey(.escape, modifierFlags: [])
        try require(wait { !self.query(launch.app, content).exists }, "Escape did not cancel the actual sheet: \(content)")
    }

    private func reopenSampleWindow(_ launch: Launch) throws {
        try assertOwnership(launch)
        // Activation is allowed only on the still-live, known sample process.
        // Never call launch(), open URLs, or invoke the product's menu route.
        try activate(launch)
        let menu = launch.app.menuBars.menuBarItems.matching(
            NSPredicate(format: "label IN %@ OR title IN %@", ["Window", "窗口"], ["Window", "窗口"])
        ).firstMatch
        try require(menu.waitForExistence(timeout: 5), "The standard Window menu is missing")
        menu.click()
        let item = launch.app.menuItems.matching(
            NSPredicate(format: "label == %@ OR title == %@", Self.sampleTitle, Self.sampleTitle)
        ).firstMatch
        try require(item.waitForExistence(timeout: 5), "The standard Window menu cannot reopen the knowledge Scene")
        try require(item.isEnabled, "The sample Scene's standard reopen item is disabled")
        item.click()
        try require(launch.window.waitForExistence(timeout: 10), "The real sample window did not reopen")
        try require(wait { launch.window.buttons[XCUIIdentifierCloseWindow].isHittable },
                    "The restored sample window is not reachable")
        try assertOwnership(launch)
        try expect(launch.app, "notice", "离线知识审阅 · 非真实资料")
        try expect(launch.app, "process.pid", String(launch.pid))
    }

    private struct SizeSample {
        let title: String
        let width: CGFloat
        let height: CGFloat
    }

    private static let sizeSamples = [
        SizeSample(title: "1260×860", width: 1260, height: 860),
        SizeSample(title: "1000×640", width: 1000, height: 640),
        SizeSample(title: "400×600", width: 400, height: 600),
    ]

    private struct OwnDisplay {
        let frame: CGRect
        let visible: CGRect
        let backingScale: CGFloat
    }

    private func ownDisplay(for window: XCUIElement) throws -> OwnDisplay {
        let screens = NSScreen.screens
        let primary = try XCTUnwrap(screens.first, "No display geometry available for the owned window")
        // AppKit is bottom-left based; XCTest AX is top-left based.
        func axRect(_ rect: CGRect) -> CGRect {
            CGRect(x: rect.minX, y: primary.frame.maxY - rect.maxY,
                   width: rect.width, height: rect.height)
        }
        let center = CGPoint(x: window.frame.midX, y: window.frame.midY)
        let matching = screens.filter { axRect($0.frame).contains(center) }
        try require(matching.count == 1, "Cannot uniquely associate the own window with a display")
        let screen = try XCTUnwrap(matching.first)
        return OwnDisplay(frame: axRect(screen.frame), visible: axRect(screen.visibleFrame),
                          backingScale: screen.backingScaleFactor)
    }

    private func measuredSize(_ app: XCUIApplication, _ suffix: String) throws -> CGSize {
        let raw = text(try element(app, suffix))
        let numbers = raw.split(separator: "×").compactMap { Double($0) }
        try require(numbers.count == 2 && numbers.allSatisfy { $0.isFinite && $0 > 0 },
                    "Missing real positive window measurement \(suffix): \(raw)")
        return CGSize(width: numbers[0], height: numbers[1])
    }

    private func panelRect(_ app: XCUIApplication, _ name: String) -> CGRect? {
        let parts = value(app, "geometry.panel.\(name)").split(separator: ",").compactMap { Double($0) }
        guard parts.count == 4, parts.allSatisfy(\.isFinite), parts[2] > 0, parts[3] > 0 else { return nil }
        return CGRect(x: parts[0], y: parts[1], width: parts[2], height: parts[3])
    }

    /// Leave the appearance sheet open so the caller can capture all the
    /// measured leaves, then close it and capture the actual material panels.
    private func requestGeometry(_ launch: Launch, _ sample: SizeSample,
                                 unavailable: inout [String]) throws -> Bool {
        try openAppearance(launch)
        try chooseMenu(launch, "geometry.sample", title: sample.title)
        try expect(launch.app, "geometry.request", sample.title)
        let display = try ownDisplay(for: launch.window)
        let physicallyFits = display.visible.width >= sample.width && display.visible.height >= sample.height
        try expect(launch.app, "geometry.fits", physicallyFits ? "yes" : "no")
        if !physicallyFits {
            attachText("""
            NOT VERIFIED requested \(sample.title)
            actual AX window=\(launch.window.frame)
            own display frame=\(display.frame), visible=\(display.visible), backingScale=\(display.backingScale)
            App reports frame=\(value(launch.app, "geometry.frame"))
            content=\(value(launch.app, "geometry.content")); layout=\(value(launch.app, "geometry.layout"))
            Public display measurements corroborate the screen limit.
            No desktop screenshot, display resize, window-border trick or image scaling was used.
            """, name: "geometry-\(sample.title)-DISPLAY-LIMIT-NOT-VERIFIED")
            if !unavailable.contains(sample.title) { unavailable.append(sample.title) }
            return false
        }
        try require(wait(timeout: 8) {
            let actual = launch.window.frame
            return abs(actual.width - sample.width) <= 1 && abs(actual.height - sample.height) <= 1
        }, "Requested \(sample.title) is possible, but the actual standard NSWindow has the wrong size")
        let frame = launch.window.frame
        try expect(launch.app, "geometry.frame", String(format: "%.1f×%.1f", frame.width, frame.height))
        let content = try measuredSize(launch.app, "geometry.content")
        let layout = try measuredSize(launch.app, "geometry.layout")
        try require(abs(content.width - frame.width) <= 1 && content.height <= frame.height,
                    "Real content size must agree with, and fit inside, the actual NSWindow")
        try require(layout.width <= content.width + 1 && layout.height <= content.height + 1,
                    "Public contentLayoutRect cannot exceed the content view")
        try require(wait {
            guard let list = self.panelRect(launch.app, "list"),
                  let detail = self.panelRect(launch.app, "detail") else { return false }
            if layout.width >= 860 {
                return abs(list.width - 300) <= 1
                    && abs(detail.maxX - (layout.width - 8)) <= 1
                    && abs(detail.minX - list.maxX - 8) <= 1
            }
            return abs(list.width - (layout.width - 16)) <= 1
                && abs(detail.width - list.width) <= 1
        }, "Actual panel measurements did not settle for \(sample.title)")
        let list = try XCTUnwrap(panelRect(launch.app, "list"))
        let detail = try XCTUnwrap(panelRect(launch.app, "detail"))
        try require(abs(list.minX - 8) <= 1 && abs(detail.maxX - (layout.width - 8)) <= 1,
                    "Measured panel edges must preserve the native 8pt insets")
        if layout.width >= 860 {
            try require(abs(list.minY - detail.minY) <= 1 && abs(detail.minX - list.maxX - 8) <= 1,
                        "Wide list/detail panels must be adjacent with the actual 8pt gap")
        } else {
            try require(abs(detail.minX - list.minX) <= 1 && abs(detail.minY - list.maxY - 8) <= 1,
                        "Compact panels must stack with the actual 8pt gap, not overlap")
        }
        var observed = ["list=\(list)", "detail=\(detail)"]
        for name in ["references", "impacts"] {
            let raw = value(launch.app, "geometry.panel.\(name)")
            if raw == "absent" {
                observed.append("\(name)=absent (disclosure closed)")
            } else {
                let panel = try XCTUnwrap(panelRect(launch.app, name), "Malformed measured \(name) panel")
                try require(panel.minX >= -1 && panel.maxX <= layout.width + 1,
                            "\(name) exceeds the real content width")
                observed.append("\(name)=\(panel)")
            }
        }
        attachText("""
        VERIFIED requested dimensions only: \(sample.title)
        actual AX frame=\(frame); measured content=\(content); measured layout=\(layout)
        own display visible=\(display.visible); backingScale=\(display.backingScale)
        \(observed.joined(separator: "\n"))
        Panel coordinates are from actual SwiftUI geometry; vertical overflow
        belongs to the native root ScrollView, not an asserted fixed-height pane.
        This geometry check is not automatic visual/material acceptance.
        """, name: "geometry-\(sample.title)-actual")
        return true
    }

    private func exerciseLongSource(_ launch: Launch, compactVerified: Bool) throws {
        try click(launch, "detail.openSource")
        let first = try element(launch.app, "source.first")
        let last = try element(launch.app, "source.last")
        let scroll = try scrollOwner(launch, containing: last.identifier)
        try expectContains(launch.app, "source.first", "虚构来源首段")
        try expectContains(launch.app, "source.last", "以上 26 段")
        try require(fullyVisible(first, in: scroll), "The actual long source must start with its first paragraph visible")
        try require(!fullyVisible(last, in: scroll), "The long source's last paragraph should start outside its viewport")
        let firstBefore = first.frame
        let lastBefore = last.frame
        let frame = launch.window.frame
        try capture(launch, name: "K8-long-source-first-visible\(compactVerified ? "-400x600" : "-actual-size")")
        for _ in 0..<50 {
            if fullyVisible(last, in: scroll) { break }
            try activate(launch)
            scroll.scroll(byDeltaX: 0, deltaY: -220)
        }
        try require(fullyVisible(last, in: scroll), "Scrolling the source's own viewport did not reveal its last paragraph")
        try require(!fullyVisible(first, in: scroll), "The source did not really scroll away from its first paragraph")
        try require(frame == launch.window.frame, "Long-source scrolling resized the parent window")
        attachText("""
        own frame=\(frame); source viewport=\(scroll.frame)
        before first=\(firstBefore); before last=\(lastBefore)
        after first=\(first.frame); after last=\(last.frame)
        first fully visible after=\(fullyVisible(first, in: scroll))
        last fully visible after=\(fullyVisible(last, in: scroll))
        These are actual AX bounds and native scroll input, not string existence alone.
        """, name: "K8-long-source-actual-visibility")
        try capture(launch, name: "K8-long-source-last-visible")
        try require((try element(launch.app, "source.close")).isHittable, "Long source must keep its close action reachable")
        try dismiss(launch, button: "source.close", content: "source.title")
    }

    private func require(_ condition: @autoclosure () throws -> Bool, _ message: @autoclosure () -> String,
                         file: StaticString = #filePath, line: UInt = #line) throws {
        guard try condition() else {
            let detail = message()
            XCTFail(detail, file: file, line: line)
            throw ReviewFailure(description: detail)
        }
    }

    private func wait(timeout: TimeInterval = 5, _ condition: @escaping () -> Bool) -> Bool {
        if condition() { return true }
        return XCTWaiter.wait(for: [XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in condition() }, object: nil
        )], timeout: timeout) == .completed
    }

    private func activate(_ launch: Launch) throws {
        try assertOwnership(launch)
        if NSWorkspace.shared.frontmostApplication?.processIdentifier != launch.pid {
            launch.app.activate()
        }
        try require(wait {
            launch.app.state == .runningForeground
                && NSWorkspace.shared.frontmostApplication?.processIdentifier == launch.pid
        }, "Only the owned review App may receive this test's keys/clicks")
    }

    private func attachText(_ text: String, name: String) {
        let attachment = XCTAttachment(string: text)
        attachment.name = "knowledge-review-\(name)"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    /// Mirror existing, bounded AX diagnostics before attachment serialization.
    /// No activation, wait, input, assertion or focus/selection mutation is added.
    private func recordNativeInput(_ launch: Launch, checkpoint: String, attachment: String? = nil) {
        let leaf = query(launch.app, "input.diagnostics")
        let raw = leaf.exists ? (leaf.value as? String ?? "") : ""
        let decoded = raw.utf8.count <= 131_072
            ? (try? JSONSerialization.jsonObject(with: Data(raw.utf8))) as? [[String: Any]]
            : nil
        var record: [String: Any] = [
            "schema": 1, "method": name, "attemptKey": launch.preference.key,
            "ownPID": launch.pid, "checkpoint": checkpoint,
            "observedUptime": ProcessInfo.processInfo.systemUptime,
            "rawByteCount": raw.utf8.count, "rawSHA256": digest(raw),
            "readStatus": decoded != nil ? "decoded" : (raw.isEmpty ? "unavailable" : "malformed"),
        ]
        if let decoded { record["snapshots"] = decoded }
        if let data = try? JSONSerialization.data(withJSONObject: record, options: [.sortedKeys]) {
            let line = Data("KNOWLEDGE_NATIVE_INPUT ".utf8) + data + Data([0x0a])
            line.withUnsafeBytes { buffer in
                guard let base = buffer.baseAddress else { return }
                var offset = 0
                while offset < buffer.count {
                    let count = Darwin.write(STDOUT_FILENO, base.advanced(by: offset), buffer.count - offset)
                    if count < 0 && errno == EINTR { continue }
                    guard count > 0 else { break } // Transport failure is not a passing diagnostic.
                    offset += count
                }
            }
        }
        if let attachment { attachText(raw, name: attachment) }
    }

    private func digest(_ string: String) -> String {
        SHA256.hash(data: Data(string.utf8)).map { String(format: "%02x", $0) }.joined()
    }

    /// Capture the actual own window, never the desktop or a scaled render.
    /// Pixel/frame evidence is retained even when geometry is not verifiable.
    private func capture(_ launch: Launch, name: String) throws {
        try activate(launch)
        let window = launch.window
        try require(window.exists, "No owned review window to capture")
        let frame = window.frame
        let screenshot = window.screenshot()
        let png = screenshot.pngRepresentation
        let attachment = XCTAttachment(data: png, uniformTypeIdentifier: "public.png")
        attachment.name = "knowledge-review-\(name)-actual-window"
        attachment.lifetime = .keepAlways
        add(attachment)

        let source = CGImageSourceCreateWithData(png as CFData, nil)
        let image = source.flatMap { CGImageSourceCreateImageAtIndex($0, 0, nil) }
        let bundle = launch.process.bundleURL.flatMap(Bundle.init(url:))
        let sdk = bundle?.object(forInfoDictionaryKey: "DTSDKName") as? String ?? "not recorded in app bundle"
        let sdkBuild = bundle?.object(forInfoDictionaryKey: "DTSDKBuild") as? String ?? "not recorded in app bundle"
        #if arch(arm64)
        let architecture = "arm64"
        #elseif arch(x86_64)
        let architecture = "x86_64"
        #else
        let architecture = "other"
        #endif
        attachText("""
        \(ProcessInfo.processInfo.operatingSystemVersionString)
        test architecture=\(architecture); app SDK=\(sdk); SDK build=\(sdkBuild)
        owned PID=\(launch.pid); launchDate=\(String(describing: launch.started))
        AX window frame=\(frame); post-capture frame=\(window.frame)
        raw PNG pixels=\(image.map { "\($0.width)×\($0.height)" } ?? "undecodable")
        No image resizing/compositing; this is only the own-window XCTest capture.
        Target combinations not executed on this machine remain NOT VERIFIED.
        Appearance/material/occlusion still require human inspection of this PNG.

        \(window.debugDescription)
        """, name: "\(name)-AX-OS-geometry")
        try require(image != nil, "Actual screenshot could not be decoded; evidence is not verified")
        try require(frame == window.frame, "Window moved during screenshot; retain actual evidence, do not rescale it")
        try require(NSWorkspace.shared.frontmostApplication?.processIdentifier == launch.pid,
                    "Foreground ownership changed during capture")
        if let image, frame.width > 0, frame.height > 0 {
            let xScale = Double(image.width) / Double(frame.width)
            let yScale = Double(image.height) / Double(frame.height)
            try require(abs(xScale - yScale) <= 0.02 && xScale >= 1,
                        "Own-window PNG/frame mismatch; no synthetic image fallback is allowed")
        }
    }
}
#endif
