import SwiftUI

/// Top strip. The window chrome itself is system-owned (`WindowAppearance`
/// applied at the app level: transparent title bar, standard traffic
/// lights); this view only renders content inside the top safe area and
/// never draws its own chrome.
public struct TitleBar: View {
    let title: String

    public init(title: String) {
        self.title = title
    }

    public var body: some View {
        HStack(spacing: 8) {
            Text(title)
                .font(.headline)
                .lineLimit(1)
                .padding(.leading, 4)
            Spacer()
        }
        .frame(height: 24)
        .padding(.horizontal, 10)
        .accessibilityIdentifier("titlebar")
    }
}
