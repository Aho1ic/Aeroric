import CoreText
import Foundation

/// Process-local registration only: no font installation, network or user
/// Library writes. Xcode copies the existing OFL fonts into the app bundle:
/// JetBrains Mono for code, Fredoka for the new-session motto.
enum AppFontRegistration {
    static let bundled = ["JetBrainsMono-Variable", "Fredoka-Variable"]

    static func registerBundledFonts() {
        for name in bundled {
            guard let url = Bundle.main.url(forResource: name, withExtension: "ttf") else {
                NSLog("Aeroric: bundled font %@ is missing; system fallback remains active.", name)
                continue
            }
            var error: Unmanaged<CFError>?
            if !CTFontManagerRegisterFontsForURL(url as CFURL, .process, &error) {
                NSLog("Aeroric: registering %@ failed; system fallback remains active.", name)
            }
        }
    }
}
