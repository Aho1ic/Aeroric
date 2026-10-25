import AeroricProtocol
import XCTest
@testable import AeroricClient

/// A13: `diagnostics.export_report` decodes strictly.
final class DiagnosticsClientDecodingTests: XCTestCase {
    func testDecodesTheReport() throws {
        let report = try DiagnosticsClient.decode(.object([
            "fileName": .string("aeroric-diagnostics-1.md"), "text": .string("# 报告"),
        ]))
        XCTAssertEqual(report, DiagnosticsClient.Report(fileName: "aeroric-diagnostics-1.md", text: "# 报告"))
    }

    func testRejectsAReplyWithoutAFileName() {
        XCTAssertThrowsError(try DiagnosticsClient.decode(.object(["text": .string("x")])))
    }
}
