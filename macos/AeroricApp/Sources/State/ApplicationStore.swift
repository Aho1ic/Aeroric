import Foundation
import Observation

/// Environment identity for the shell (P4-04): which environment tag this
/// launch carries and where its data directory lives. Populated from the
/// injected configuration — never probed.
@MainActor
@Observable
final class ApplicationStore {
    let environmentTag: String
    let dataDirPath: String?
    let injectedExplicitly: Bool

    init(environmentTag: String, dataDirPath: String?, injectedExplicitly: Bool) {
        self.environmentTag = environmentTag
        self.dataDirPath = dataDirPath
        self.injectedExplicitly = injectedExplicitly
    }
}
