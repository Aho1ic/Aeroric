import CoreText
import Foundation

/// Process-local registration only: no font installation, network or user
/// Library writes. Xcode copies the existing OFL font into the app bundle.
enum AppFontRegistration {
    static func registerBundledFonts() {
        guard let url = Bundle.main.url(forResource: "JetBrainsMono-Variable", withExtension: "ttf") else {
            NSLog("Aeroric: bundled JetBrains Mono is missing; system fallback remains active.")
            return
        }
        var error: Unmanaged<CFError>?
        if !CTFontManagerRegisterFontsForURL(url as CFURL, .process, &error) {
            NSLog("Aeroric: font registration failed; system fallback remains active.")
        }
    }
}
