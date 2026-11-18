import Foundation
import XCTest

/// A5 (2026-11-17): every RPC method the Swift clients name must be a name the
/// Rust side spells somewhere. The host is the authority on method names; the
/// notebook client had called `notebook.rag.search`, `notebook.sync.run`, … for
/// months while the host registered `notebook.rag_search`, `notebook.sync_run`,
/// so each call came back "method not found". `contracts/native/v1` and the
/// command map list only part of the methods, so the check reads the Rust
/// sources instead: a name must appear there as a string literal.
final class ClientMethodNamesTests: XCTestCase {
    private static let repoRoot: URL = {
        var url = URL(fileURLWithPath: #filePath)
        // file -> AeroricClientTests -> Tests -> AeroricKit -> macos -> repo root
        for _ in 0..<5 { url.deleteLastPathComponent() }
        return url
    }()

    /// Files under `relative` (recursively) with the extension.
    private func files(_ relative: String, ext: String) -> [URL] {
        let root = Self.repoRoot.appendingPathComponent(relative)
        guard let walker = FileManager.default.enumerator(at: root, includingPropertiesForKeys: nil) else {
            return []
        }
        return walker.compactMap { $0 as? URL }.filter { $0.pathExtension == ext }
    }

    private func matches(_ pattern: String, in text: String) -> [String] {
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return [] }
        let range = NSRange(text.startIndex..., in: text)
        return regex.matches(in: text, range: range).compactMap { match in
            Range(match.range(at: 1), in: text).map { String(text[$0]) }
        }
    }

    /// Dotted string literals in the Rust crates: registered names, `*_METHOD`
    /// constants, `service_ctx!` names.
    private func rustNames() throws -> Set<String> {
        var names = Set<String>()
        let crates = Self.repoRoot.appendingPathComponent("core/crates")
        for crate in try FileManager.default.contentsOfDirectory(atPath: crates.path) {
            for file in files("core/crates/\(crate)/src", ext: "rs") {
                let text = try String(contentsOf: file, encoding: .utf8)
                names.formUnion(matches(#""([A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z0-9_]+)+)""#, in: text))
            }
        }
        return names
    }

    /// Method literals the Swift clients send: `request("…"`, `call("…"`.
    /// Interpolated names (`"discussion.\(action)"`) are skipped.
    private func swiftCalls() throws -> [(name: String, file: String)] {
        var calls: [(String, String)] = []
        for root in ["macos/AeroricKit/Sources", "ios/AeroricCompanionKit/Sources"] {
            for file in files(root, ext: "swift") {
                let text = try String(contentsOf: file, encoding: .utf8)
                for name in matches(#"\b(?:request|call|requestRaw|stream|subscribe)\(\s*"([^"\\]+)""#, in: text)
                where name.contains(".") {
                    calls.append((name, file.lastPathComponent))
                }
            }
        }
        return calls
    }

    func testEveryMethodTheClientsCallIsSpelledByTheHost() throws {
        let rust = try rustNames()
        let calls = try swiftCalls()
        // The scan itself found something: a moved directory must not pass silently.
        XCTAssertGreaterThan(rust.count, 500, "Rust sources not found under \(Self.repoRoot.path)")
        XCTAssertGreaterThan(calls.count, 250, "Swift client calls not found")
        XCTAssertTrue(rust.contains("notebook.rag_search"))

        let unknown = calls.filter { !rust.contains($0.name) }
        let lines = Set(unknown.map { "\($0.name)（\($0.file)）" }).sorted()
        XCTAssertTrue(lines.isEmpty, "宿主没有这些方法名：\n" + lines.joined(separator: "\n"))
    }
}
