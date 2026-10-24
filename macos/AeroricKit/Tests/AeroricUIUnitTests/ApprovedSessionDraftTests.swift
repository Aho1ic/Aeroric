import XCTest
@testable import AeroricUI

/// In-memory values only. No NSApplication/hosting, defaults, filesystem,
/// network, Agent, setup or teardown.
final class ApprovedSessionDraftTests: XCTestCase {
    func testProjectsAndGlobalDraftAreIsolatedIncludingConfigurationOnlyEdits() {
        var book = NewSessionDraftBook()
        book.edit(.project("a")) { $0.task.permissionMode = "fixture-permission" }
        book.edit(.project("b")) { $0.task.prompt = "B" }
        book.open(.global)
        book.open(.project("a"))
        XCTAssertEqual(book[.project("a")]?.task.permissionMode, "fixture-permission")
        XCTAssertEqual(book[.project("a")]?.task.prompt, "")
        XCTAssertEqual(book[.project("b")]?.task.prompt, "B")
        XCTAssertEqual(book[.global]?.task.agent, "")
        XCTAssertEqual(book[.global]?.capabilities, SessionCapabilities())
    }

    func testFirstCapabilitiesOffAndSuccessPreferencesStayWithinProject() throws {
        var book = NewSessionDraftBook()
        book.open(.project("a"))
        XCTAssertEqual(book[.project("a")]?.capabilities.enabledNames, [])
        book.edit(.project("a")) {
            $0.task.prompt = "sent"
            $0.task.attachmentPaths = ["/fixture/reference"]
            $0.task.worktree = "/fixture/previous-run-only"
            $0.recipeID = "fixture-recipe"
            $0.task.permissionMode = "fixture-permission"
            $0.capabilities.team = true
            $0.capabilities.evolution = true
        }
        let sent = try XCTUnwrap(book.begin(.project("a")))
        XCTAssertTrue(book.acknowledge(.project("a"), submissionID: sent.submissionID))
        book.open(.project("a"))
        book.open(.project("b"))
        let next = try XCTUnwrap(book[.project("a")])
        XCTAssertEqual(next.task.prompt, "")
        XCTAssertEqual(next.task.attachmentPaths, [])
        XCTAssertNil(next.task.worktree)
        XCTAssertEqual(next.recipeID, "fixture-recipe")
        XCTAssertEqual(next.capabilities.enabledNames, ["团队协作", "任务工具进化"])
        XCTAssertEqual(next.task.permissionMode, "fixture-permission")
        XCTAssertNotEqual(next.submissionID, sent.submissionID)
        XCTAssertNil(book[.project("b")]?.recipeID)
        XCTAssertEqual(book[.project("b")]?.capabilities.enabledNames, [])
    }

    func testUnknownRetainsInputIdentityAndCannotResendOrEdit() throws {
        var book = NewSessionDraftBook()
        book.edit(.project("a")) { $0.task.prompt = "keep" }
        let sent = try XCTUnwrap(book.begin(.project("a")))
        XCTAssertNil(book.begin(.project("a")))
        book.fail(.project("a"), submissionID: sent.submissionID, reason: "connection lost", acceptanceUnknown: true)
        book.edit(.project("a")) { $0.task.prompt = "overwrite" }
        book.open(.project("a"))
        XCTAssertNil(book.begin(.project("a")))
        XCTAssertEqual(book[.project("a")]?.task.prompt, "keep")
        XCTAssertEqual(book[.project("a")]?.submissionID, sent.submissionID)
        XCTAssertFalse(book.acknowledge(.project("a"), submissionID: sent.submissionID))
    }

    func testKnownFailurePreservesDraftAndStaleReceiptCannotClearIt() throws {
        var book = NewSessionDraftBook()
        book.edit(.project("a")) { $0.task.prompt = "keep" }
        let sent = try XCTUnwrap(book.begin(.project("a")))
        book.fail(.project("a"), submissionID: sent.submissionID, reason: "rejected", acceptanceUnknown: false)
        XCTAssertEqual(book[.project("a")]?.task.prompt, "keep")
        XCTAssertFalse(book.acknowledge(.project("a"), submissionID: "another-id"))
        XCTAssertEqual(book.begin(.project("a"))?.submissionID, sent.submissionID)
    }

    func testHarnessChangePreservesInputReferencesPermissionsAndRoles() {
        var draft = NewSessionDraft()
        draft.task.prompt = "input"
        draft.task.attachmentPaths = ["/fixture/a"]
        draft.task.permissionMode = "explicit"
        draft.task.model = "previous-model"
        draft.capabilities.review = true
        draft.selectHarness("custom-fixture")
        XCTAssertEqual(draft.task.agent, "custom-fixture")
        XCTAssertEqual(draft.task.prompt, "input")
        XCTAssertEqual(draft.task.model, "previous-model")
        XCTAssertEqual(draft.task.attachmentPaths, ["/fixture/a"])
        XCTAssertEqual(draft.task.permissionMode, "explicit")
        XCTAssertTrue(draft.capabilities.review)
    }

    func testNoScopeOrMissingCatalogBlocksWithoutFallback() {
        let draft = NewSessionDraft()
        XCTAssertEqual(draft.blockingReasons(scope: .global, configurations: [], projectExists: true).count, 2)
        XCTAssertTrue(draft.blockingReasons(scope: .project("gone"), configurations: [], projectExists: false)
            .contains { $0.contains("原项目不可用") })
        XCTAssertNil(SessionNavigation.draftScope(SessionNavigation.allTasks))
        XCTAssertEqual(SessionNavigation.draftScope(SessionNavigation.globalDraft), .global)
    }

    func testUnsupportedSelectionsAreNamedRatherThanIgnored() {
        var draft = NewSessionDraft()
        draft.capabilities.team = true
        draft.capabilities.review = true
        draft.capabilities.evolution = true
        draft.task.attachmentPaths = ["/fixture/a"]
        let reasons = draft.blockingReasons(scope: .project("a"), configurations: [], projectExists: true)
        for name in ["团队协作", "独立审查", "任务工具进化", "附件"] {
            XCTAssertTrue(reasons.contains { $0.contains(name) })
        }
    }

    func testProfileVersionAndUnsupportedModelNeverSilentlyFallback() {
        var draft = NewSessionDraft()
        draft.selectHarness("codex")
        let config = SessionConfiguration(id: "p", version: "v1", title: "fixture", agent: "codex",
            model: "m1", effort: nil,
            options: .init(agents: ["codex"], models: ["m1"], efforts: [], permissionModes: ["ask"]))
        draft.task.permissionMode = "ask"
        draft.apply(config)
        draft.task.prompt = "fixture"
        XCTAssertEqual(draft.blockingReasons(scope: .project("a"), configurations: [config], projectExists: true), [])
        draft.task.model = "unsupported"
        XCTAssertTrue(draft.blockingReasons(scope: .project("a"), configurations: [config], projectExists: true)
            .contains { $0.contains("已选模型") })
        var newer = config
        newer.version = "v2"
        XCTAssertTrue(draft.blockingReasons(scope: .project("a"), configurations: [newer], projectExists: true)
            .contains { $0.contains("版本") })
        XCTAssertEqual(draft.configurationVersion, "v1")
    }

    func testNewSessionGeometryDoesNotOverwriteSharedTokens() {
        XCTAssertEqual(NewSessionGeometry.inputRadius(compact: false), 16)
        XCTAssertEqual(NewSessionGeometry.inputRadius(compact: true), 16)
        XCTAssertEqual(NewSessionGeometry.contextGap, 6)
        XCTAssertEqual(NativePanelGeometry.panelRadius, AeroricRadius.xl)
    }

    func testEveryCreationSurfaceUsesExplicitProjectAndGlobalDestinations() {
        let items = [
            SidebarItem(id: "a", title: "A", children: [
                SidebarItem(id: SessionNavigation.draft(projectID: "a"), title: "新建会话"),
                SidebarItem(id: SessionNavigation.task("ta"), title: "Task A"),
            ]),
            SidebarItem(id: "b", title: "B"),
        ]
        for selection in ["a", SessionNavigation.task("ta"), SessionNavigation.draft(projectID: "a")] {
            XCTAssertEqual(SessionNavigation.newDraftSelection(from: selection, items: items),
                           SessionNavigation.draft(projectID: "a"))
        }
        for selection in [nil, SessionNavigation.allTasks, SessionNavigation.globalDraft, "section.git", "missing"] {
            XCTAssertEqual(SessionNavigation.newDraftSelection(from: selection, items: items),
                           SessionNavigation.globalDraft)
        }
    }

    func testNativeAttachmentURLsRemainReferencesAndRejectOtherSources() {
        var paths = ["/fixture/already-selected"]
        XCTAssertFalse(SessionAttachmentReferences.append([
            URL(string: "https://example.invalid/file")!,
            URL(string: "file://untrusted.invalid/path")!,
            URL(fileURLWithPath: "/"),
        ], to: &paths))
        XCTAssertEqual(paths, ["/fixture/already-selected"])
        XCTAssertTrue(SessionAttachmentReferences.append([
            URL(fileURLWithPath: "/fixture/user-selected"),
            URL(fileURLWithPath: "/fixture/user-selected"),
        ], to: &paths))
        XCTAssertEqual(paths, ["/fixture/already-selected", "/fixture/user-selected"])
        // These fixture paths need not exist: the projection must never probe them.
    }

    func testCustomProfileUsesHarnessButPreservesAgentAndRequiresExplicitPermissionAndModel() {
        let config = SessionConfiguration(
            id: "custom", version: "v1", title: "Fixture", agent: "custom-agent",
            model: nil, effort: nil,
            options: .init(agents: ["custom-agent"], models: ["m"], efforts: [], permissionModes: ["ask"]),
            harness: "codex", requiresModel: true, requiresPermissionIdentity: true
        )
        var draft = NewSessionDraft()
        draft.selectHarness("codex")
        draft.task.prompt = "fixture"
        draft.apply(config)
        XCTAssertEqual(draft.selectedHarness, "codex")
        XCTAssertEqual(draft.task.agent, "custom-agent")
        XCTAssertNil(draft.task.model, "never select the first model")
        XCTAssertFalse(draft.blockingReasons(scope: .project("p"), configurations: [config], projectExists: true).isEmpty)
        draft.task.model = "m"
        draft.selectPermission("ask")
        XCTAssertEqual(draft.blockingReasons(scope: .project("p"), configurations: [config], projectExists: true), [])
        var other = config
        other.id = "other"; other.agent = "other-agent"; other.options.agents = ["other-agent"]
        draft.apply(other)
        draft.task.model = "m"
        XCTAssertEqual(draft.task.permissionMode, "ask")
        XCTAssertTrue(draft.blockingReasons(scope: .project("p"), configurations: [other], projectExists: true)
            .contains { $0.contains("当前 Agent 明确选择权限") })
    }

    func testReconciledRejectionAllowsEditingWithNewIdentityAndRetainsOriginalPrompt() throws {
        var book = NewSessionDraftBook()
        book.edit(.project("p")) { $0.task.prompt = "keep" }
        let sent = try XCTUnwrap(book.begin(.project("p")))
        book.fail(.project("p"), submissionID: sent.submissionID, reason: "lost reply", acceptanceUnknown: true)
        book.reconcileRejected(.project("p"), submissionID: sent.submissionID, reason: "verified refusal")
        XCTAssertEqual(book[.project("p")]?.task.prompt, "keep")
        book.edit(.project("p")) { $0.task.prompt += " revised" }
        XCTAssertNotEqual(book[.project("p")]?.submissionID, sent.submissionID)
        XCTAssertFalse(book.reconcileAccepted(.project("p"), submissionID: sent.submissionID))
        XCTAssertEqual(book[.project("p")]?.task.prompt, "keep revised")
    }

    func testVerifiedAcceptedLookupClearsOnlyMatchingUnknownDraft() throws {
        var book = NewSessionDraftBook()
        book.edit(.project("p")) { $0.task.prompt = "sent" }
        book.edit(.project("other")) { $0.task.prompt = "other" }
        let sent = try XCTUnwrap(book.begin(.project("p")))
        book.fail(.project("p"), submissionID: sent.submissionID, reason: "lost reply", acceptanceUnknown: true)
        XCTAssertFalse(book.reconcileAccepted(.project("p"), submissionID: "wrong"))
        XCTAssertTrue(book.reconcileAccepted(.project("p"), submissionID: sent.submissionID))
        XCTAssertNil(book[.project("p")])
        XCTAssertEqual(book[.project("other")]?.task.prompt, "other")
    }
}
