import XCTest
@testable import AeroricUI

/// P7-03 验收：InfrastructureStoreTests——容器/镜像/端口簿记、Docker 不可用
/// 原因、Conda 选择、长任务状态。
@MainActor
final class InfrastructureStoreTests: XCTestCase {

    func testIngestContainersAndSelection() {
        let store = InfrastructureStore()
        store.ingest(containers: [
            InfrastructureStore.Container(id: "abc", image: "nginx", names: "web", state: "running", status: "Up 2 days", ports: "80/tcp"),
        ])
        XCTAssertEqual(store.containers.count, 1)
        store.selectContainer("abc")
        XCTAssertEqual(store.selectedContainer?.names, "web")
        store.selectContainer(nil)
        XCTAssertNil(store.selectedContainer)
    }

    func testDaemonUnavailableReason() {
        let store = InfrastructureStore()
        // 无 Docker daemon 时显示不可用，不弹 sudo。
        store.setDaemonUnavailable("Docker daemon 不可用")
        XCTAssertEqual(store.daemonUnavailableReason, "Docker daemon 不可用")
        store.setDaemonUnavailable(nil)
        XCTAssertNil(store.daemonUnavailableReason)
    }

    func testCondaSelection() {
        let store = InfrastructureStore()
        store.ingest(condaEnvironments: [
            InfrastructureStore.CondaEnv(name: "dev", path: "/opt/miniconda3/envs/dev"),
        ])
        XCTAssertEqual(store.condaEnvironments.count, 1)
        store.selectCondaEnv(path: "/opt/miniconda3/envs/dev")
        XCTAssertEqual(store.selectedCondaEnvPath, "/opt/miniconda3/envs/dev")
    }

    func testOperationStatuses() {
        let store = InfrastructureStore()
        store.setOperationStatus("op-1", "inProgress")
        XCTAssertEqual(store.operationStatuses["op-1"], "inProgress")
        store.setOperationStatus("op-1", "cancelled")
        XCTAssertEqual(store.operationStatuses["op-1"], "cancelled")
    }

    func testPortsIngest() {
        let store = InfrastructureStore()
        store.ingest(ports: [
            InfrastructureStore.Port(port: 8080, pid: 1234, processName: "nginx"),
        ])
        XCTAssertEqual(store.ports.count, 1)
        XCTAssertEqual(store.ports[0].processName, "nginx")
    }
}
