@testable import AeroricUI
import AppKit
import CoreText
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

    /// A10 (2026-11-17): a downloaded file that does not register says which
    /// and why, instead of only a log line.
    func testAFileThatDoesNotRegisterSaysWhichAndWhy() throws {
        let root = try scratch()
        let package = try XCTUnwrap(FontPackage.of(.mono))
        let folder = root.appendingPathComponent(package.choice.rawValue, isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        try Data("not a font".utf8).write(to: folder.appendingPathComponent("SarasaMonoSC-Regular.ttf"))
        let failure = try XCTUnwrap(FontDownloads.registerAll(package, root: root))
        XCTAssertTrue(failure.hasPrefix("SarasaMonoSC-Regular.ttf："), failure)
        XCTAssertGreaterThan(failure.count, "SarasaMonoSC-Regular.ttf：".count, "a reason follows")
        XCTAssertNil(FontDownloads.registerAll(package, root: try scratch()), "nothing downloaded, nothing failed")
    }

    func testCoreTextReasonsReadInWords() {
        XCTAssertNil(FontDownloads.registrationFailure(code: CTFontManagerError.alreadyRegistered.rawValue,
                                                       described: "x"), "already in is fine")
        XCTAssertEqual(FontDownloads.registrationFailure(code: CTFontManagerError.invalidFontData.rawValue,
                                                         described: nil), "字体文件已损坏")
        XCTAssertEqual(FontDownloads.registrationFailure(code: CTFontManagerError.insufficientPermissions.rawValue,
                                                         described: nil), "没有权限读取字体文件")
        XCTAssertEqual(FontDownloads.registrationFailure(code: 9999, described: "系统的话"), "系统的话")
        XCTAssertEqual(FontDownloads.registrationFailure(code: 9999, described: nil), "原因不明（错误码 9999）")
    }

    /// A10: files on disk are never "not downloaded"; a failure says why, and
    /// a family that registered under another name says that.
    func testAPresetWithItsFilesIsNeverNotDownloaded() {
        XCTAssertEqual(FontPackage.installState(choice: .mono, family: "Sarasa Mono SC", filesPresent: true,
                                                failure: nil), .active("Sarasa Mono SC"))
        XCTAssertEqual(FontPackage.installState(choice: .mono, family: nil, filesPresent: false, failure: nil),
                       .notDownloaded)
        XCTAssertEqual(FontPackage.installState(choice: .mono, family: nil, filesPresent: true,
                                                failure: "a.ttf：字体文件已损坏"),
                       .notRegistered("没装上：a.ttf：字体文件已损坏"))
        XCTAssertEqual(FontPackage.installState(choice: .maple, family: nil, filesPresent: true, failure: nil),
                       .notRegistered("已下载，但系统里找不到「Maple Mono CN」字体"))
    }

    func testSizesReadInMegabytes() {
        XCTAssertEqual(FontPackage.megabytes(49_862_338), "约 50 MB")
        XCTAssertEqual(FontPackage.megabytes(140_216_987), "约 140 MB")
    }

    /// B2-7: a cancel that lands before the download task exists stops it
    /// from starting at all, instead of being lost.
    func testACancelBeforeTheTaskStartsIsNotLost() async {
        let watcher = DownloadWatcher(expected: 1, progress: { _ in })
        watcher.cancel()
        let session = URLSession(configuration: .ephemeral)
        defer { session.invalidateAndCancel() }
        let task = session.downloadTask(with: URL(string: "https://example.invalid/font.7z")!)
        do {
            _ = try await withCheckedThrowingContinuation { watcher.begin(task, $0) }
            XCTFail("a cancelled download must not finish")
        } catch {
            XCTAssertTrue(error is CancellationError, "\(error)")
        }
        XCTAssertEqual(task.state, .suspended, "never resumed")
    }

}
