import Foundation

/// C 批 (2026-11-08): what people type to an agent goes as typed. macOS
/// turns `'` and `"` into curly quotes and `--` into a dash in every text
/// view, which breaks commands and code in a prompt (CU: `user's` became
/// `user’s`); Aeroric starts with both off. A choice the user made in
/// Edit ▸ Substitutions lives in the same keys and is kept. Quotes typed
/// with an input method (“”) are not substitutions and are untouched.
public enum PlainTyping {
    public static let keys = ["NSAutomaticQuoteSubstitutionEnabled", "NSAutomaticDashSubstitutionEnabled"]

    /// Turns each substitution off unless `domain` already has a choice.
    public static func apply(to defaults: UserDefaults, domain: String) {
        let own = defaults.persistentDomain(forName: domain) ?? [:]
        for key in keys where own[key] == nil {
            defaults.set(false, forKey: key)
        }
    }
}
