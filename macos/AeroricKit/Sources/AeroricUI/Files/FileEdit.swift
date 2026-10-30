import Foundation

/// A change the file tree asks the App to make (2026-10-30). Removing goes
/// to the Trash; the tree offers no permanent delete.
public enum FileEdit: Equatable, Sendable {
    case create(parent: String, name: String, directory: Bool)
    case rename(path: String, newName: String)
    case trash(path: String)

    /// The folder whose listing changes.
    public var parent: String {
        switch self {
        case let .create(parent, _, _): parent
        case let .rename(path, _), let .trash(path): FileEditText.parent(of: path)
        }
    }
}

/// The file tree's paths, kept out of the view so they can be tested.
public enum FileEditText {
    public static func child(of parent: String, name: String) -> String {
        parent.isEmpty ? name : "\(parent)/\(name)"
    }

    public static func parent(of path: String) -> String {
        guard let slash = path.lastIndex(of: "/") else { return "" }
        return String(path[..<slash])
    }

    /// Linked folders are read-only here: they belong to other folders on the Mac.
    public static func writable(_ path: String) -> Bool {
        !path.hasPrefix(LinkedFolderPath.prefix)
    }

    public static func name(of path: String) -> String {
        guard let slash = path.lastIndex(of: "/") else { return path }
        return String(path[path.index(after: slash)...])
    }
}
