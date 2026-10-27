import AeroricClient
import AeroricProtocol
@testable import AeroricUI
import XCTest

@MainActor
final class SshHostsTests: XCTestCase {
    private typealias H = SshClient.Host

    func testHostsGroupByNameWithUngroupedLastAndSearch() {
        let hosts = [
            H(id: "1", name: "GPU", group: "公司", host: "gpu.lan", username: "me"),
            H(id: "2", name: "开发机", group: "公司", host: "dev.example", username: "me"),
            H(id: "3", name: "NAS", group: " ", host: "nas.local", username: "me"),
            H(id: "4", name: "Pi", group: "个人", host: "pi.local", username: "pi"),
        ]
        let groups = SshHostsStore.grouped(hosts)
        XCTAssertEqual(groups.map(\.group), ["个人", "公司", "未分组"])
        XCTAssertEqual(Set(groups[1].hosts.map(\.name)), ["GPU", "开发机"], "both hosts of 公司, in the user's locale order")
        XCTAssertEqual(SshHostsStore.grouped(hosts, query: "nas").flatMap { $0.hosts.map(\.id) }, ["3"])
        XCTAssertEqual(SshHostsStore.grouped(hosts, query: "公司").count, 1, "the group name is searchable")
    }

    func testEmptyCreatedGroupsAreListedButNotWhileSearching() {
        let hosts = [H(id: "1", name: "开发机", group: "公司", host: "dev", username: "me")]
        let groups = SshHostsStore.grouped(hosts, groups: ["测试环境", "公司"])
        XCTAssertEqual(Set(groups.map(\.group)), ["测试环境", "公司"])
        XCTAssertEqual(groups.first { $0.group == "测试环境" }?.hosts.count, 0)
        XCTAssertEqual(SshHostsStore.grouped(hosts, groups: ["测试环境"], query: "dev").map(\.group), ["公司"])
        let store = SshHostsStore()
        XCTAssertEqual(store.groupProblem("  "), "填写分组名")
        XCTAssertEqual(store.groupProblem("未分组"), "“未分组”是保留名")
        XCTAssertNil(store.groupProblem("测试环境"))
    }

    func testTheDraftNeedsHostUserPortAndAPasswordWhenChosen() {
        let store = SshHostsStore()
        store.newHost()
        XCTAssertEqual(store.draftProblem, "填写主机")
        store.draft?.host = "dev.example"
        store.draft?.port = 70000
        XCTAssertEqual(store.draftProblem, "端口应在 1–65535")
        store.draft?.port = 22
        store.authMode = .password
        XCTAssertEqual(store.draftProblem, "填写密码")
        store.passwordDraft = "secret"
        XCTAssertNil(store.draftProblem)
    }

    func testTestResultsDecode() {
        let ok = SshClient.decodeTest(.object(["state": .string("ok"), "agents": .array([
            .object(["agent": .string("claude"), "version": .string("2.1.311 (Claude Code)")]),
            .object(["agent": .string("omp"), "version": .null]),
        ])]))
        XCTAssertEqual(ok, .ok([.init(agent: "claude", version: "2.1.311 (Claude Code)"), .init(agent: "omp", version: nil)]))
        let untrusted = SshClient.decodeTest(.object(["state": .string("untrusted"), "keys": .array([
            .object(["keyType": .string("ED25519"), "fingerprint": .string("SHA256:abc"),
                     "knownHostsLine": .string("h ssh-ed25519 AAAA")]),
        ])]))
        XCTAssertEqual(untrusted, .untrusted([.init(keyType: "ED25519", fingerprint: "SHA256:abc",
                                                    knownHostsLine: "h ssh-ed25519 AAAA")]))
        XCTAssertEqual(SshClient.decodeTest(.object(["state": .string("failed"), "message": .string("拒绝")])),
                       .failed("拒绝"))
        XCTAssertEqual(SshHostsPage.shortVersion("2.1.311 (Claude Code)"), "2.1.311")
        XCTAssertEqual(SshHostsPage.shortVersion("codex-cli 0.164.0"), "0.164.0")
    }
}
