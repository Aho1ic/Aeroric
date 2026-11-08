import XCTest
@testable import AeroricUI

final class PlainTypingTests: XCTestCase {
    func testSubstitutionsStartOffAndTheUsersChoiceIsKept() throws {
        let suite = "aeroric.plain-typing.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }

        PlainTyping.apply(to: defaults, domain: suite)
        XCTAssertEqual(defaults.object(forKey: "NSAutomaticQuoteSubstitutionEnabled") as? Bool, false)
        XCTAssertEqual(defaults.object(forKey: "NSAutomaticDashSubstitutionEnabled") as? Bool, false)

        defaults.set(true, forKey: "NSAutomaticQuoteSubstitutionEnabled")
        PlainTyping.apply(to: defaults, domain: suite)
        XCTAssertEqual(defaults.object(forKey: "NSAutomaticQuoteSubstitutionEnabled") as? Bool, true,
                       "turned back on in Edit ▸ Substitutions, it stays on")
    }
}
