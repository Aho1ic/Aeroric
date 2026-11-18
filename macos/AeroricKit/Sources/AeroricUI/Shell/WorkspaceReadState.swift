import AeroricClient
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

    /// B2-1: what a read that threw leaves behind. A cancelled read (the
    /// view moved on and started another) is no failure: it leaves what was
    /// there — loaded when there is something read before, else not loaded.
    public static func after(_ error: Error, hasEarlierRead: Bool, cancelled: Bool = Task.isCancelled) -> WorkspaceReadState {
        let cancellation = cancelled || error is CancellationError
            || (error as? ConnectionError) == .cancelled
            || (error as? RequestResolution) == .cancelled
        if cancellation {
            return hasEarlierRead ? .loaded : .notLoaded
        }
        return .failed(String(describing: error))
    }
}
