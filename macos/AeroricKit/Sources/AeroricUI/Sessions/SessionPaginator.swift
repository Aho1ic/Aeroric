import SwiftUI

/// P5-04 Action 4：分页器。稳定 ID diff 更新、分页不跳动；加载更早页时
/// 保持当前锚点（消息数增量插入顶部，不整表替换）。游标守卫由
/// `SessionHistoryLogic.isValidCursorAdvance` 保证。
@MainActor
@Observable
public final class SessionPaginator {
    public private(set) var messages: [SessionMessage] = []
    public private(set) var cursor: UInt64?
    public private(set) var hasMore: Bool = false
    public private(set) var isCodex: Bool = false
    /// 加载中标记（防重复触发）。
    public private(set) var isLoadingPage = false
    /// 会话身份（切换会话时整表重置）。
    public private(set) var sessionKey: String?

    public init() {}

    /// 切换会话：整表重置（稳定 ID diff 不跨会话）。
    public func reset(sessionKey: String, isCodex: Bool) {
        self.sessionKey = sessionKey
        self.isCodex = isCodex
        messages = []
        cursor = nil
        hasMore = false
        isLoadingPage = false
    }

    /// 首页/更早页到达：合并（跨页去重）＋去重 tool 块。同会话内是增量
    /// 插入（分页不跳动）；hasMore=false 或坏游标停止。
    @discardableResult
    public func ingest(page: SessionMessagePage) -> Bool {
        guard !isLoadingPage else { return false }
        guard SessionHistoryLogic.isValidCursorAdvance(cursor: cursor, next: page.nextCursor) else {
            hasMore = false
            return false
        }
        if page.messages.isEmpty, !page.hasMore {
            hasMore = false
            return false
        }
        let merged = SessionHistoryLogic.mergePages(page.messages, messages, isCodex: isCodex)
            .reversed()
        messages = SessionHistoryLogic.deduplicateToolBlocks(Array(merged))
        cursor = page.nextCursor
        hasMore = page.hasMore
        isLoadingPage = false
        return true
    }

    public func setLoadingPage(_ loading: Bool) {
        isLoadingPage = loading
    }

    /// 是否还有更早页可加载。
    public var canLoadMore: Bool {
        hasMore && !isLoadingPage
    }
}
