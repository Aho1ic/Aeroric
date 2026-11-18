@testable import AeroricClient
@testable import AeroricProtocol
import XCTest

/// 目检 2026-11-18 第四轮: an error written into a sentence reads as the
/// host's words, not the enum's debug dump.
final class ConnectionErrorWordsTests: XCTestCase {
    func testAnInterpolatedRefusalIsTheHostsMessage() {
        let refused = ConnectionError.refused(FailureFrame(
            id: "r1",
            error: RpcError(code: .invalidArgument, message: "只读会话（讨论成员、代码审查、只读委派）不能改权限",
                            retryable: false),
            additional: [:]))
        let error: Error = refused
        XCTAssertEqual("修改失败：\(error)", "修改失败：只读会话（讨论成员、代码审查、只读委派）不能改权限")
        XCTAssertEqual(String(describing: error), "只读会话（讨论成员、代码审查、只读委派）不能改权限")
        XCTAssertEqual("\(ConnectionError.timedOut)", "宿主没有及时回应")
    }
}
