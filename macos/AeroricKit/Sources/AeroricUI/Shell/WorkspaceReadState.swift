import Foundation

/// A UI projection of explicit read outcomes; empty arrays are not evidence
/// that a repository/device list was successfully read.
public enum WorkspaceReadState: Equatable, Sendable {
    case unavailable
    case notLoaded
    case loading
    case loaded
    case failed(String)

    public var notice: String? {
        switch self {
        case .unavailable: "本页尚未接入宿主能力"
        case .notLoaded: "尚未加载"
        case .loading: "正在加载…"
        case .loaded: nil
        case let .failed(reason): "读取失败；保留内容仅供参考：\(reason)"
        }
    }
}
