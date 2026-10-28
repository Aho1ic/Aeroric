import AeroricClient
import AeroricProtocol
@testable import AeroricUI
import XCTest

/// The sidebar's 更多 menus (2026-10-28).
@MainActor
final class SidebarMenuTests: XCTestCase {
    func testProjectsAreGroupedUngroupedFirstThenInGroupOrder() {
        let projects = [
            SidebarItem(id: "a", title: "A", projectGroup: "工作"),
            SidebarItem(id: "b", title: "B"),
            SidebarItem(id: "c", title: "C", projectGroup: "个人"),
            SidebarItem(id: "d", title: "D", projectGroup: "工作"),
        ]
        let sections = SidebarItem.groupedProjects(projects, order: ["个人", "工作"])
        XCTAssertEqual(sections.map(\.name), ["", "个人", "工作"])
        XCTAssertEqual(sections[2].projects.map(\.id), ["a", "d"])
        XCTAssertEqual(SidebarItem.groupedProjects(projects, order: []).map(\.name), ["", "工作", "个人"],
                       "a group the host did not list still shows, in first-seen order")
    }

    func testOnlyClaudeAndCodexSessionsWithAConversationCanFork() {
        XCTAssertNil(SidebarItem.forkUnavailableReason(
            SidebarItem(id: "task:1", title: "t", harness: "claude", sessionID: "s1")))
        XCTAssertNotNil(SidebarItem.forkUnavailableReason(
            SidebarItem(id: "task:2", title: "t", harness: "omp", sessionID: "s1")))
        XCTAssertNotNil(SidebarItem.forkUnavailableReason(
            SidebarItem(id: "task:3", title: "t", harness: "codex", sessionID: "fork:s1")),
            "a fork that has not started yet has nothing of its own to fork")
        XCTAssertNotNil(SidebarItem.forkUnavailableReason(
            SidebarItem(id: "imported:1", title: "t", agent: "claude", imported: true, harness: "claude",
                        sessionID: "s1")))
    }

    func testSessionRowsUseTheHarnessLogo() {
        XCTAssertEqual(HarnessLogo.asset(harness: "claude", agent: "claude_gpt55"), "HarnessClaude")
        XCTAssertEqual(HarnessLogo.asset(harness: nil, agent: "codex"), "HarnessCodex")
        XCTAssertEqual(HarnessLogo.asset(harness: "omp", agent: "my-omp"), "HarnessOMP")
        XCTAssertNil(HarnessLogo.asset(harness: nil, agent: "custom"))
        XCTAssertNotNil(ProjectColor.color("teal"))
        XCTAssertNil(ProjectColor.color(nil))
    }

    func testImportedSessionIDsComeFromTheFileName() {
        XCTAssertEqual(ImportsClient.sessionID(fromSource: "/h/.claude/projects/-w/0b1c.jsonl"), "0b1c")
        XCTAssertEqual(
            ImportsClient.sessionID(
                fromSource: "/h/.codex/sessions/2026/rollout-2026-10-02T10-00-00-0199a6b2-1c2d-7e3f-8a9b-0c1d2e3f4a5b.jsonl"),
            "0199a6b2-1c2d-7e3f-8a9b-0c1d2e3f4a5b")
        XCTAssertNil(ImportsClient.sessionID(fromSource: "/h/.codex/sessions/rollout-x.jsonl"))
    }
}
