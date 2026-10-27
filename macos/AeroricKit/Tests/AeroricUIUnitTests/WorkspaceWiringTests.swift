@testable import AeroricUI
import SwiftUI
import XCTest

@MainActor
final class WorkspaceWiringTests: XCTestCase {
    func testTaskListViewSelectionBindingAndStoreIntegration() {
        let store = TaskStore()
        store.upsertProject(id: "p1", path: "/tmp/p1", name: "p1")
        store.upsertTask(TaskStore.TaskEntry(id: "t1", projectID: "p1", title: "First Task", status: "running", agent: "claude"))
        store.upsertTask(TaskStore.TaskEntry(id: "t2", projectID: "p1", title: "Second Task", status: "done", agent: "omp"))

        var selectedID: String? = "t1"
        let view = TaskListView(
            visibleTasks: .constant(store.visibleTasks),
            filterText: .constant(""),
            selectedTaskID: Binding(get: { selectedID }, set: { selectedID = $0 })
        )
        XCTAssertNotNil(view.body)
        XCTAssertEqual(selectedID, "t1")
        XCTAssertEqual(store.visibleTasks.count, 2)
    }

    func testTaskDraftOptionsSupportAllFourFamiliesAndValidate() {
        let options = TaskDraft.Options(
            agents: ["claude", "codex", "dsh", "omp"],
            models: ["claude-sonnet-4-5", "gpt-5", "deepseek-chat"],
            efforts: ["off", "minimal", "low", "medium", "high", "xhigh", "max"],
            permissionModes: ["ask", "auto_edit", "full_access"]
        )
        for family in ["claude", "codex", "dsh", "omp"] {
            let validDraft = TaskDraft(
                agent: family,
                model: "claude-sonnet-4-5",
                effort: "high",
                permissionMode: "ask",
                prompt: "Implement feature"
            )
            XCTAssertTrue(validDraft.validate(against: options).isEmpty, "Family \(family) must validate cleanly")
        }

        let invalidDraft = TaskDraft(
            agent: "unknown-agent",
            permissionMode: "invalid-mode",
            prompt: ""
        )
        let errors = invalidDraft.validate(against: options)
        XCTAssertNotNil(errors["prompt"])
        XCTAssertNotNil(errors["agent"])
        XCTAssertNotNil(errors["permissionMode"])
    }

    func testSurfaceChoiceAndGlassPanelRespectAccessibilityReduceTransparency() {
        XCTAssertEqual(SurfaceChoice.resolve(preference: .glass, reduceTransparency: false), .glass)
        XCTAssertEqual(SurfaceChoice.resolve(preference: .glass, reduceTransparency: true), .classic)
        XCTAssertEqual(SurfaceChoice.resolve(preference: .classic, reduceTransparency: false), .classic)

        let glassPanel = GlassPanel(preference: .glass, reduceTransparency: false) {
            Text("Glass Chrome")
        }
        let a11yPanel = GlassPanel(preference: .glass, reduceTransparency: true) {
            Text("Accessible Opaque Chrome")
        }
        XCTAssertNotNil(glassPanel.body)
        XCTAssertNotNil(a11yPanel.body)
    }

    private final class StubSettingsState: SettingsStateProviding {
        var environmentLabel: String { "test" }
        var protocolVersionText: String { "v1" }
        var buildIdentifier: String { "1.0.0-debug+1" }
        var phaseText: String { "ready" }
        var applicationTag: String { "test" }
    }

    func testSettingsRootViewWiresAllSections() {
        let state = StubSettingsState()
        let root = SettingsRootView(state: state)
        XCTAssertNotNil(root.body)
        XCTAssertEqual(SettingsSection.allCases, [.application, .agents, .about])
    }
}
