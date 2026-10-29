import AppKit

/// The font for Chinese in conversation messages (2026-10-29). Only the
/// Chinese glyphs change: the system font stays first and the chosen family
/// sits in its cascade list, so Latin text and code keep their fonts. Nothing
/// is bundled; a preset that is not installed falls back to the system font
/// (PingFang) and settings says so with its download page.
public enum ConversationCJKFont: String, CaseIterable, Sendable {
    case pingFang = "pingfang"
    case mono
    case maple
    case custom

    public static let storageKey = "aeroric.conversation.cjkFont"
    public static let customKey = "aeroric.conversation.cjkFont.custom"

    public var title: String {
        switch self {
        case .pingFang: "苹方"
        case .mono: "Mono"
        case .maple: "Maple Mono"
        case .custom: "自定义"
        }
    }

    /// Families a preset accepts, in order (the first installed one wins).
    public var candidates: [String] {
        switch self {
        case .pingFang, .custom: []
        case .mono: ["Sarasa Mono SC", "Sarasa Term SC", "Sarasa Fixed SC"]
        case .maple: ["Maple Mono CN", "Maple Mono NF CN", "Maple Mono Normal CN", "Maple Mono Normal NF CN"]
        }
    }

    public var downloadURL: URL? {
        switch self {
        case .mono: URL(string: "https://github.com/be5invis/Sarasa-Gothic/releases")
        case .maple: URL(string: "https://github.com/subframe7536/maple-font/releases")
        case .pingFang, .custom: nil
        }
    }

    /// The installed family to use, or nil for the system font. Pure given
    /// `installed` (tested); `custom` accepts a family or a font name.
    public static func resolve(_ choice: ConversationCJKFont, custom: String,
                               installed: (String) -> Bool = ConversationCJKFont.isInstalled) -> String? {
        switch choice {
        case .pingFang:
            return nil
        case .custom:
            let name = custom.trimmingCharacters(in: .whitespacesAndNewlines)
            return !name.isEmpty && installed(name) ? name : nil
        case .mono, .maple:
            return choice.candidates.first(where: installed)
        }
    }

    /// The stored choice, resolved.
    public static func storedFamily(_ defaults: UserDefaults = .standard) -> String? {
        let choice = defaults.string(forKey: storageKey).flatMap(ConversationCJKFont.init(rawValue:)) ?? .pingFang
        return resolve(choice, custom: defaults.string(forKey: customKey) ?? "")
    }

    /// A family (`NSFontManager`) or a PostScript / full font name.
    public static func isInstalled(_ name: String) -> Bool {
        NSFontManager.shared.availableMembers(ofFontFamily: name) != nil || NSFont(name: name, size: 12) != nil
    }

    /// Installed families that draw Chinese, for the custom picker.
    public static func installedChineseFamilies() -> [String] {
        NSFontManager.shared.availableFontFamilies.filter { family in
            guard !family.hasPrefix("."), let font = NSFont(name: family, size: 12)
                ?? NSFontManager.shared.font(withFamily: family, traits: [], weight: 5, size: 12) else { return false }
            return font.coveredCharacterSet.contains(Unicode.Scalar(0x4E2D)!) // 中
        }
    }

    /// The system font at `weight`, with `family` (when set) drawing what the
    /// system font cannot — in practice the Chinese.
    public static func font(size: CGFloat, weight: NSFont.Weight, family: String?) -> NSFont {
        let base = NSFont.systemFont(ofSize: size, weight: weight)
        guard let family else { return base }
        let isFamily = NSFontManager.shared.availableMembers(ofFontFamily: family) != nil
        let cjk = NSFontDescriptor(fontAttributes: isFamily
            ? [.family: family, .traits: [NSFontDescriptor.TraitKey.weight: weight]]
            : [.name: family])
        let descriptor = base.fontDescriptor.addingAttributes([.cascadeList: [cjk]])
        return NSFont(descriptor: descriptor, size: size) ?? base
    }
}
