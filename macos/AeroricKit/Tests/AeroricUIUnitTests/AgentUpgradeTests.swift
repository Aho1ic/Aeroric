import AeroricClient
import AeroricProtocol
@testable import AeroricUI
import XCTest

final class AgentUpgradeTests: XCTestCase {
    private typealias H = AgentUpgradeClient.Harness

    private func omp(_ status: AgentUpgradeClient.Status, remind: Bool = true,
                     job: AgentUpgradeClient.Job? = nil) -> H {
        H(agent: "omp", title: "omp", installed: "18.12.3", latest: "18.13.1", minimum: "18.12.0",
          status: status, remind: remind, shouldRemind: remind && status == .updateAvailable || status == .required,
          job: job)
    }

    func testReminderStatesFollowStatusAndJob() {
        XCTAssertEqual(AgentUpdateReminder.make(omp(.updateAvailable), dismissedJob: nil),
                       .available(title: "omp", latest: "18.13.1", installed: "18.12.3"))
        XCTAssertNil(AgentUpdateReminder.make(omp(.updateAvailable, remind: false), dismissedJob: nil))
        XCTAssertEqual(AgentUpdateReminder.make(omp(.required), dismissedJob: nil),
                       .required(title: "omp", installed: "18.12.3", minimum: "18.12.0"))
        XCTAssertEqual(AgentUpdateReminder.make(omp(.updateAvailable, job: .init(state: .running)), dismissedJob: nil),
                       .upgrading(title: "omp"))
        let done = AgentUpgradeClient.Job(state: .succeeded, finishedAtMs: 7)
        XCTAssertEqual(AgentUpdateReminder.make(omp(.upToDate, remind: false, job: done), dismissedJob: nil),
                       .done(title: "omp", version: "18.12.3"))
        XCTAssertNil(AgentUpdateReminder.make(omp(.upToDate, remind: false, job: done), dismissedJob: 7),
                     "a shown result stays dismissed")
        XCTAssertNil(AgentUpdateReminder.make(nil, dismissedJob: nil))
    }

    func testDecodeReadsTheHostSnapshot() {
        let value = JSONValue.object([
            "checkedAt": .number("1793000000000"),
            "agents": .array([.object([
                "agent": .string("codex"), "title": .string("Codex"), "installed": .string("0.163.0"),
                "latest": .string("0.165.1"), "minimum": .string("0.162.0"), "channel": .string("npm"),
                "status": .string("updateAvailable"), "remind": .bool(true), "shouldRemind": .bool(true),
                "job": .object(["state": .string("running"), "installing": .bool(false),
                                "log": .array([.string("added 1 package")]), "startedAt": .number("1")]),
            ])]),
        ])
        let snapshot = AgentUpgradeClient.decode(value)
        XCTAssertEqual(snapshot.checkedAtMs, 1_793_000_000_000)
        let codex = try? XCTUnwrap(snapshot.agents.first)
        XCTAssertEqual(codex?.status, .updateAvailable)
        XCTAssertEqual(codex?.job?.state, .running)
        XCTAssertEqual(codex?.job?.log, ["added 1 package"])
    }

    @MainActor
    func testBookMapsFamiliesAndListsUpgradable() {
        let book = AgentUpgradeBook()
        book.load(.init(agents: [
            H(agent: "codex", title: "Codex", installed: "0.163.0", latest: "0.165.1", status: .updateAvailable),
            H(agent: "claude", title: "Claude Code", installed: "2.1.3", latest: "2.1.3", status: .upToDate),
            omp(.required),
        ]))
        XCTAssertEqual(book.harness(forFamily: "claude_gpt55")?.agent, "codex")
        XCTAssertEqual(book.upgradable.map(\.agent), ["codex", "omp"])
        XCTAssertNil(book.harness(forFamily: nil))
    }
}
