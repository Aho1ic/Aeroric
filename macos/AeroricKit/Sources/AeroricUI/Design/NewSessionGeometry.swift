import SwiftUI

/// V6-local geometry, intentionally separate from the v10 shared panel tokens.
public enum NewSessionGeometry {
    public static let pageMaximumWidth: CGFloat = 850
    public static func pageSidePadding(compact: Bool) -> CGFloat { compact ? 17 : 34 }
    public static func pageTopPadding(compact: Bool) -> CGFloat { compact ? 24 : 32 }
    public static func pageBottomPadding(compact: Bool) -> CGFloat { compact ? 24 : 35 }
    public static let contextGap: CGFloat = 6
    public static let harnessHeight: CGFloat = 34
    public static let harnessItemHeight: CGFloat = 28
    public static let harnessIconSize: CGFloat = 16
    public static func inputRadius(compact: Bool) -> CGFloat { 16 }
    public static func promptHeight(compact: Bool) -> CGFloat { compact ? 104 : 108 }
}
