import AeroricProtocol
@testable import AeroricUI
import XCTest

/// B6: the 交付物 section — what each row says, a version against another,
/// and the store driving the host's `artifacts.*`.
final class ArtifactPanelTests: XCTestCase {
    private func version(_ n: Int, checks: [LiveArtifactCheck] = [], withdrawn: Bool = false,
                         superseded: Bool = false) -> LiveArtifactVersion {
        LiveArtifactVersion(artifactId: "a", version: n, turnId: "t\(n)", withdrawn: withdrawn,
                            superseded: superseded, checks: checks)
    }

    func testARowSaysWhatTheLastCheckOfTheStandingVersionFound() {
        var artifact = LiveArtifact(id: "a", path: "src/report.md")
        XCTAssertEqual(ArtifactText.state(artifact), "已撤回", "no version stands")
        artifact.current = version(2)
        XCTAssertEqual(ArtifactText.state(artifact), "未检查")
        artifact.current = version(2, checks: [LiveArtifactCheck(turnId: "t2", passed: true),
                                                LiveArtifactCheck(turnId: "t3", passed: false, summary: "测试 1 项失败")])
        XCTAssertEqual(ArtifactText.state(artifact), "检查未过")
        artifact.current = version(2, checks: [LiveArtifactCheck(turnId: "t2", passed: true, incomplete: true)])
        XCTAssertEqual(ArtifactText.state(artifact), "检查通过（可能不完整）")
        artifact.removed = true
        XCTAssertEqual(ArtifactText.state(artifact), "已删除")
        XCTAssertEqual(artifact.name, "report.md")
        XCTAssertEqual(artifact.folder, "src")

        let failed = version(1, checks: [LiveArtifactCheck(turnId: "t1", passed: false, summary: "测试 1 项失败")],
                             superseded: true)
        XCTAssertEqual(ArtifactText.versionLine(failed), "第 1 版 · 检查未过（测试 1 项失败） · 已有新版")
        XCTAssertEqual(ArtifactText.versionLine(version(3, withdrawn: true)), "第 3 版 · 已撤回")
    }

    func testTwoVersionsCompareLineByLineAndLongSameRunsFold() {
        let same = (1...20).map { "line \($0)" }
        let older = (same + ["old tail"]).joined(separator: "\n")
        let newer = (["new head"] + same + ["new tail"]).joined(separator: "\n")
        let lines = ArtifactText.compare(older, newer)
        XCTAssertEqual(lines.first?.kind, .added)
        XCTAssertEqual(lines.first?.text, "new head")
        XCTAssertEqual(lines.filter { $0.kind == .removed }.map(\.text), ["old tail"])
        XCTAssertEqual(lines.filter { $0.kind == .added }.map(\.text), ["new head", "new tail"])
        XCTAssertEqual(lines.filter { $0.kind == .folded }.map(\.text), ["… 省略 14 行相同内容 …"])
        XCTAssertEqual(Set(lines.map(\.id)).count, lines.count)
    }

    func testThePageDecodesWhatTheHostSends() {
        let page = LiveArtifactPage.decode(.object([
            "artifacts": .array([.object([
                "id": .string("art-1"), "taskId": .string("t"), "path": .string("out/data.csv"),
                "kind": .string("data"), "createdMs": .number("5"), "latestVersion": .number("2"),
                "removed": .bool(true),
                "current": .object([
                    "artifactId": .string("art-1"), "version": .number("1"), "turnId": .string("u1"),
                    "blob": .string("abc"), "size": .number("12"), "atMs": .number("9"),
                    "checks": .array([.object([
                        "evidence": .object(["kind": .string("autoCheck"), "detail": .string("u1")]),
                        "verdict": .string("passed"), "incomplete": .bool(true),
                    ])]),
                ]),
            ])]),
            "nextCursor": .string("100"), "dropped": .number("3"),
        ]))
        XCTAssertEqual(page.nextCursor, "100")
        XCTAssertEqual(page.dropped, 3)
        let artifact = try! XCTUnwrap(page.artifacts.first)
        XCTAssertEqual(artifact.kind, "data")
        XCTAssertEqual(artifact.latestVersion, 2)
        XCTAssertTrue(artifact.removed)
        XCTAssertEqual(artifact.current?.checks, [LiveArtifactCheck(turnId: "u1", passed: true, incomplete: true)])
    }

    /// B6 step 2: a delegation card names what the child delivered.
    func testADelegationCardSaysWhatTheChildDelivered() throws {
        let card = try XCTUnwrap(LiveDelegationCard.decode(.object([
            "delegationId": .string("dg-1"), "state": .string("reported"),
            "artifacts": .array([
                .object(["artifactId": .string("art-1"), "version": .number("2"), "path": .string("docs/report.md")]),
            ]),
        ])))
        XCTAssertEqual(card.artifacts, [LiveArtifactRef(artifactId: "art-1", version: 2, path: "docs/report.md")])
        XCTAssertEqual(DelegationText.deliverables(card), "交付 report.md（第 2 版）")
        XCTAssertTrue(DelegationText.footnote(card)?.contains("交付 report.md（第 2 版）") == true)
        var many = card
        many.artifacts += [LiveArtifactRef(artifactId: "art-2", version: 1, path: "a.rs"),
                           LiveArtifactRef(artifactId: "art-3", version: 1, path: "b.rs")]
        XCTAssertEqual(DelegationText.deliverables(many), "交付 3 个文件：report.md、a.rs 等")
    }

    @MainActor
    func testTheStoreListsExcludesOpensAndCompares() async {
        let host = FakeArtifactHost()
        let store = ArtifactStore(taskID: "t", actions: host)
        await store.load()
        XCTAssertEqual(store.counted.map(\.path), ["src/a.rs", "logo.png"])

        await store.setExcluded("art-png", true)
        XCTAssertEqual(store.counted.map(\.path), ["src/a.rs"])
        XCTAssertEqual(store.excluded.map(\.path), ["logo.png"])
        XCTAssertEqual(host.excludedCalls(), ["art-png=true"])

        let code = store.artifacts[0]
        await store.toggle(code.id)
        XCTAssertEqual(store.versions[code.id]?.map(\.version), [2, 1])

        await store.open(code, version: 1)
        XCTAssertEqual(store.viewing?.title, "a.rs · 第 1 版")
        XCTAssertEqual(store.viewing?.body, .text("fn a() {}\n"))

        await store.compare(code, older: 1, newer: 2)
        guard case let .lines(lines)? = store.viewing?.body else { return XCTFail("no comparison") }
        XCTAssertEqual(lines.map(\.kind), [.removed, .added, .same])

        await store.open(store.artifacts[1], version: nil)
        guard case let .note(text)? = store.viewing?.body else { return XCTFail("binary shows a note") }
        XCTAssertTrue(text.contains("二进制文件"))

        await store.open(code, version: 9)
        XCTAssertEqual(store.viewing?.body, .note("这个版本的内容已不可用"))
    }
}

private final class FakeArtifactHost: ArtifactActions, @unchecked Sendable {
    private let lock = NSLock()
    private var excluded: [String] = []

    func excludedCalls() -> [String] { lock.withLock { excluded } }

    func artifacts(taskId: String, cursor: String?) async throws -> LiveArtifactPage {
        LiveArtifactPage(artifacts: [
            LiveArtifact(id: "art-rs", path: "src/a.rs", kind: "code", latestVersion: 2,
                         current: LiveArtifactVersion(artifactId: "art-rs", version: 2)),
            LiveArtifact(id: "art-png", path: "logo.png", kind: "image", latestVersion: 1,
                         current: LiveArtifactVersion(artifactId: "art-png", version: 1)),
        ])
    }

    func artifactVersions(taskId: String, artifactId: String) async throws -> [LiveArtifactVersion] {
        [LiveArtifactVersion(artifactId: artifactId, version: 2), LiveArtifactVersion(artifactId: artifactId, version: 1)]
    }

    func readArtifact(taskId: String, artifactId: String, version: Int?, offset: UInt64,
                      limit: UInt64?) async throws -> LiveArtifactContent {
        struct Gone: Error, CustomStringConvertible { var description: String { "这个版本的内容已不可用" } }
        switch (artifactId, version) {
        case ("art-rs", 1): return LiveArtifactContent(version: 1, size: 10, content: "fn a() {}\n")
        case ("art-rs", 2): return LiveArtifactContent(version: 2, size: 11, content: "fn a() { 2 }\n")
        case ("art-png", _): return LiveArtifactContent(version: 1, size: 4, encoding: "base64", content: "iVBO")
        default: throw Gone()
        }
    }

    func excludeArtifact(taskId: String, artifactId: String, excluded flag: Bool) async throws {
        lock.withLock { excluded.append("\(artifactId)=\(flag)") }
    }
}
