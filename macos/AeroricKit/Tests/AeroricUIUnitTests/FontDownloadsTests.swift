@testable import AeroricUI
import AppKit
import XCTest

/// In-app Chinese font downloads (2026-10-29): the archive is checked before
/// anything is extracted, and an installed font is usable at once.
final class FontDownloadsTests: XCTestCase {
    private func scratch() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("fonts-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    func testAnArchiveWithTheWrongChecksumIsRefused() throws {
        let root = try scratch()
        let archive = root.appendingPathComponent("bad.zip")
        try Data("not the font".utf8).write(to: archive)
        let package = try XCTUnwrap(FontPackage.of(.maple))
        XCTAssertThrowsError(try FontDownloads.install(package, archive: archive, root: root)) { error in
            guard case FontDownloadError.checksum = error else { return XCTFail("\(error)") }
        }
        XCTAssertTrue(FontDownloads.installedFonts(package, root: root).isEmpty)
    }

    /// Uses the real release archive when it is on this Mac (fetched once by
    /// hand into /tmp/aer-fonts); skipped otherwise — tests never download.
    func testTheMapleArchiveInstallsItsWeightsAndRegistersTheFamily() throws {
        let cached = URL(fileURLWithPath: "/tmp/aer-fonts/maple.zip")
        try XCTSkipUnless(FileManager.default.fileExists(atPath: cached.path))
        let root = try scratch()
        let archive = root.appendingPathComponent("maple.zip")
        try FileManager.default.copyItem(at: cached, to: archive)
        let package = try XCTUnwrap(FontPackage.of(.maple))
        try FontDownloads.install(package, archive: archive, root: root)
        XCTAssertEqual(FontDownloads.installedFonts(package, root: root).count, 4)
        XCTAssertEqual(ConversationCJKFont.resolve(.maple, custom: ""), "Maple Mono CN")
        XCTAssertFalse(FileManager.default.fileExists(atPath: archive.path), "the archive is removed")
    }

    func testSizesReadInMegabytes() {
        XCTAssertEqual(FontPackage.megabytes(49_862_338), "约 50 MB")
        XCTAssertEqual(FontPackage.megabytes(140_216_987), "约 140 MB")
    }
}
