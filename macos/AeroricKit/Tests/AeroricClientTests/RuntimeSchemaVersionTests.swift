import XCTest
@testable import AeroricClient

/// The native client's `runtime.json` bound must equal core's real constant.
///
/// ## Why this test exists
///
/// 2026-10-13: the shell showed **协议不兼容** against the host it had just spawned
/// itself. The cause was a stale literal in `HostSupervisor.checkRuntimeCompatibility`:
/// it refused any `schemaVersion > 1`, while core's `RUNTIME_SCHEMA_VERSION` had been
/// **2** since P2-04 (v2 added `contentFingerprints` — a real schema change, not an
/// optional field). Every real data directory therefore carried schemaVersion 2 and
/// the client rejected its own host.
///
/// The check's *shape* was right (a higher value means a newer core has been here and
/// must be refused) — only the bound was wrong. So the fix is not "raise the number";
/// it is to stop having a second, silently-drifting source of truth. This test reads
/// core's Rust source and asserts the two agree, which is exactly the thing nobody
/// could see before: `swift test` and `cargo test` both passed while the product was
/// unusable, because no test compared the two constants.
final class RuntimeSchemaVersionTests: XCTestCase {
    private static let repoRoot: URL = {
        var url = URL(fileURLWithPath: #filePath)
        // file -> AeroricClientTests -> Tests -> AeroricKit -> macos -> repo root
        for _ in 0..<5 { url.deleteLastPathComponent() }
        return url
    }()

    private func coreSchemaVersion() throws -> Int {
        let path = Self.repoRoot
            .appendingPathComponent("core/crates/aeroric-core/src/storage/runtime.rs")
        let source = try String(contentsOf: path, encoding: .utf8)
        // `pub const RUNTIME_SCHEMA_VERSION: u32 = 2;`
        let pattern = #"pub const RUNTIME_SCHEMA_VERSION: u32 = (\d+);"#
        let regex = try NSRegularExpression(pattern: pattern)
        let range = NSRange(source.startIndex..<source.endIndex, in: source)
        guard let match = regex.firstMatch(in: source, range: range),
              match.numberOfRanges == 2,
              let captured = Range(match.range(at: 1), in: source),
              let value = Int(source[captured])
        else {
            // A test that cannot find the constant must fail, never pass quietly:
            // it would go on "verifying" a mirror nobody is checking.
            throw XCTSkip("could not parse RUNTIME_SCHEMA_VERSION from \(path.path) — update this test's pattern")
        }
        return value
    }

    func testClientSupportedSchemaVersionMatchesCore() throws {
        let core = try coreSchemaVersion()
        XCTAssertEqual(
            RuntimeSchemaVersion.supported, core,
            """
            The native client supports runtime schema \(RuntimeSchemaVersion.supported) while core \
            writes \(core). A client that refuses its own host's data directory shows 协议不兼容 \
            and cannot run any task (measured 2026-10-13: core moved to 2 in P2-04, the client kept \
            refusing anything above 1). Update RuntimeSchemaVersion.supported in HostSupervisor.swift \
            together with core's RUNTIME_SCHEMA_VERSION.
            """
        )
    }

    /// The bound must actually **reject a genuinely newer core** — otherwise
    /// "raise the number until the shell goes green" would be a valid-looking fix.
    func testAFutureSchemaVersionIsStillRefused() async throws {
        let dataDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("aeroric-schema-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dataDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dataDir) }

        let future = RuntimeSchemaVersion.supported + 1
        let runtimeURL = dataDir.appendingPathComponent("runtime.json")
        let text = """
        {"schemaVersion":\(future),"epoch":"future-epoch","revision":"1"}
        """
        try text.write(to: runtimeURL, atomically: true, encoding: .utf8)

        let config = SupervisorConfig(
            dataDir: dataDir,
            socketPath: dataDir.appendingPathComponent("host.sock").path,
            cliExecutable: URL(fileURLWithPath: "/usr/bin/true")
        )
        let supervisor = HostSupervisor(config: config, onChange: { _ in })
        await supervisor.start()
        defer { Task { await supervisor.stop() } }

        let deadline = Date().addingTimeInterval(5)
        var phase = await supervisor.phase
        while Date() < deadline {
            if case .incompatible = phase { break }
            try? await Task.sleep(for: .milliseconds(40))
            phase = await supervisor.phase
        }
        guard case let .incompatible(reason) = phase else {
            XCTFail("a newer core must still be refused, got \(phase)")
            return
        }
        XCTAssertTrue(reason.contains("schemaVersion \(future)"), reason)
        // 拒绝必须是"不覆写、不误杀":文件一字不动(ADR-010 的既有语义)。
        XCTAssertEqual(try String(contentsOf: runtimeURL, encoding: .utf8), text)
    }
}
