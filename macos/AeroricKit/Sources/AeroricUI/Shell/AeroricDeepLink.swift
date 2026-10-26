import Foundation

/// A16: `aeroric://` links (notifications, other apps). Unknown links are
/// ignored rather than guessed at.
public enum AeroricDeepLink {
    public enum Target: Equatable, Sendable {
        case task(String)
        case attention
        /// A sidebar surface by its raw name, e.g. `agentConfigs` (the App
        /// decides whether it exists).
        case section(String)
    }

    public static let scheme = "aeroric"

    /// `aeroric://task/<id>`, `aeroric://attention` or `aeroric://section/<name>`.
    public static func target(_ url: URL) -> Target? {
        guard url.scheme?.lowercased() == scheme, let host = url.host?.lowercased() else { return nil }
        switch host {
        case "attention":
            return .attention
        case "section":
            let name = url.pathComponents.filter { $0 != "/" }.first ?? ""
            guard !name.isEmpty, name.count <= 64, name.unicodeScalars.allSatisfy(CharacterSet.alphanumerics.contains) else {
                return nil
            }
            return .section(name)
        case "task":
            let id = url.pathComponents.filter { $0 != "/" }.first ?? ""
            let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_"))
            guard !id.isEmpty, id.count <= 128, id.unicodeScalars.allSatisfy(allowed.contains) else { return nil }
            return .task(id)
        default:
            return nil
        }
    }

    public static func url(for target: Target) -> URL {
        switch target {
        case let .task(id): URL(string: "\(scheme)://task/\(id)")!
        case .attention: URL(string: "\(scheme)://attention")!
        case let .section(name): URL(string: "\(scheme)://section/\(name)")!
        }
    }
}
