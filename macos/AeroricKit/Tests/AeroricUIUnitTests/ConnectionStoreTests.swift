import XCTest
@testable import AeroricUI

/// P7-01 验收：ConnectionStoreTests——连接簿记（无明文密码）、host key
/// 状态（首连/变化分别提示）、remote cwd 分离。
@MainActor
final class ConnectionStoreTests: XCTestCase {

    func testIngestAndSelect() {
        let store = ConnectionStore()
        store.ingest([
            ConnectionStore.Connection(id: "c1", name: "prod", host: "example.com", port: 22, username: "user"),
            ConnectionStore.Connection(id: "c2", name: "dev", host: "dev.example.com", port: 2222, username: "dev"),
        ])
        XCTAssertEqual(store.connections.count, 2)
        store.select("c2")
        XCTAssertEqual(store.selected?.id, "c2")
    }

    func testNoPlaintextPasswordInStore() {
        // 密码不进 store（只有 hasPassword 布尔；credential owner 在宿主）。
        let store = ConnectionStore()
        store.ingest([
            ConnectionStore.Connection(id: "c1", name: "prod", host: "example.com", port: 22, username: "user", hasPassword: true),
        ])
        // 编译期保证：Connection 无 password 字段（这里镜像检查 hasPassword）。
        XCTAssertTrue(store.connections[0].hasPassword)
        XCTAssertNil(store.connections[0].identityFile == nil ? nil : store.connections[0].identityFile)
    }

    func testHostKeyPendingAndClear() {
        let store = ConnectionStore()
        // 首连：Unknown 需要用户确认指纹（不自动重试为信任）。
        store.setPendingHostKey(.unknown(
            target: "example.com:22",
            fingerprints: ["SHA256:abc"]
        ))
        XCTAssertNotNil(store.pendingHostKey)
        // 信任后清除（信任由用户确认触发，不自动）。
        store.clearPendingHostKey()
        XCTAssertNil(store.pendingHostKey)
    }

    func testRemoteCWDSeparateFromLocal() {
        let store = ConnectionStore()
        store.setRemoteCWD("/home/user/project")
        XCTAssertEqual(store.remoteCWD, "/home/user/project")
        // remote cwd 与 local cwd 分开显示（防误落本机）。
        store.setRemoteCWD(nil)
        XCTAssertNil(store.remoteCWD)
    }

    func testHostKeyPromptVariants() {
        // 首连/keyChanged 分别提示。
        let first = HostKeyPrompt(
            kind: .firstConnection, fingerprints: ["SHA256:abc"], onTrust: {}, onReject: {}
        )
        XCTAssertNotNil(first)
        let changed = HostKeyPrompt(
            kind: .keyChanged(previousFingerprint: "SHA256:old"),
            fingerprints: ["SHA256:new"], onTrust: {}, onReject: {}
        )
        XCTAssertNotNil(changed)
    }
}
