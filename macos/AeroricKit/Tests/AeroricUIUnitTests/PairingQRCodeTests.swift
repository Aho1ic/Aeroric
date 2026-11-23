@testable import AeroricUI
import AppKit
import CoreImage
import XCTest

/// D4 step 7 (2026-11-23): the Mac's pairing page draws the link as a QR
/// code, and reading that code back gives the link exactly.
final class PairingQRCodeTests: XCTestCase {
    func testThePairingLinkReadsBackFromItsQRCode() throws {
        let link = "aeroric://pair?code=" + String(repeating: "eyJ2IjoyLCJlbmRwb2ludHMiOlsid3M6Ly8xOTIuMTY4LjEuMzM6ODc2NSJdfQ", count: 6)
        let image = try XCTUnwrap(PairingQRCode.image(for: link))
        XCTAssertGreaterThanOrEqual(image.size.width, 120, "large enough to scan")
        XCTAssertEqual(image.size.width, image.size.height)

        let cgImage = try XCTUnwrap(image.cgImage(forProposedRect: nil, context: nil, hints: nil))
        let detector = try XCTUnwrap(CIDetector(ofType: CIDetectorTypeQRCode, context: nil,
                                                options: [CIDetectorAccuracy: CIDetectorAccuracyHigh]))
        let found = detector.features(in: CIImage(cgImage: cgImage)).compactMap { ($0 as? CIQRCodeFeature)?.messageString }
        XCTAssertEqual(found, [link])
    }
}
