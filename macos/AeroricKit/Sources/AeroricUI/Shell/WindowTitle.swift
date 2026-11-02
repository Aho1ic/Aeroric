import SwiftUI

/// 2026-11-02 (owner): a page's title, its one-line status and — for a
/// session — its state, drawn in the window toolbar right after the sidebar
/// toggle instead of in a header row of its own. Filters and search stay in
/// the page.
///
/// A preference, not the page's own `.toolbar(.navigation)`: SwiftUI does
/// not promise the order of navigation items from different views, and the
/// title must sit after the toggle. Page buttons (停止, 置顶, 更多) stay in
/// the page's own `.toolbar` so they keep its environment and dialogs.
public struct WindowTitle: Equatable, Sendable {
    public var title: String
    public var subtitle: String?
    public var systemImage: String?
    /// A session's wire status, drawn with `SessionStatusDot`.
    public var status: String?
    public var subtitleTint: SubtitleTint

    public enum SubtitleTint: Equatable, Sendable { case quiet, warning }

    public init(_ title: String, subtitle: String? = nil, systemImage: String? = nil,
                status: String? = nil, subtitleTint: SubtitleTint = .quiet) {
        self.title = title
        self.subtitle = subtitle
        self.systemImage = systemImage
        self.status = status
        self.subtitleTint = subtitleTint
    }
}

struct WindowTitleKey: PreferenceKey {
    static let defaultValue: WindowTitle? = nil
    /// The innermost page wins: a scaffold inside a page reports after it.
    static func reduce(value: inout WindowTitle?, nextValue: () -> WindowTitle?) {
        if let next = nextValue() { value = next }
    }
}

public extension View {
    /// Report this page's title to the window toolbar. `nil` reports
    /// nothing — for a page kept mounted while hidden.
    func windowTitle(_ title: WindowTitle?) -> some View {
        preference(key: WindowTitleKey.self, value: title)
    }
}

/// What the toolbar draws beside the sidebar toggle.
struct WindowTitleView: View {
    let title: WindowTitle

    var body: some View {
        HStack(spacing: AeroricSpace.s) {
            if let status = title.status {
                SessionStatusDot(status: status)
            } else if let symbol = title.systemImage {
                Image(systemName: symbol)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(AeroricColor.textSecondary)
            }
            Text(title.title)
                .font(.system(size: AeroricType.body, weight: .semibold))
                .foregroundStyle(AeroricColor.text)
                .lineLimit(1)
                .truncationMode(.tail)
                .layoutPriority(1)
                .accessibilityIdentifier("window.title")
            if let subtitle = title.subtitle, !subtitle.isEmpty {
                Text(subtitle)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(title.subtitleTint == .warning ? AeroricColor.warning : AeroricColor.textTertiary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .accessibilityIdentifier("window.subtitle")
            }
        }
        .accessibilityElement(children: .combine)
    }
}
