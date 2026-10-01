import SwiftUI

/// Bottom strip: connection phase (indicator + the store-formatted
/// `phaseText`) plus the environment tag. Hung via `safeAreaInset(.bottom)`
/// so the sidebar/detail backgrounds extend to the window bottom behind it
/// — a plain VStack row would cut the material off with its own bar (the
/// primuse lesson, MacContentView :256).
public struct StatusBar: View {
    let phase: ConnectionPhaseSnapshot
    let phaseText: String
    let environmentLabel: String

    public init(phase: ConnectionPhaseSnapshot, phaseText: String, environmentLabel: String) {
        self.phase = phase
        self.phaseText = phaseText
        self.environmentLabel = environmentLabel
    }

    public var body: some View {
        HStack(spacing: 12) {
            Circle()
                .fill(phaseIndicatorColor)
                .frame(width: 6, height: 6)
                .accessibilityHidden(true)
            Text(phaseText)
                .lineLimit(1)
                .accessibilityIdentifier("statusbar.phase")
            Spacer()
            Text(environmentLabel)
                .foregroundStyle(Color.dsTextMuted)
                .accessibilityIdentifier("statusbar.environment")
        }
        .font(.caption)
        .padding(.horizontal, 12)
        .padding(.vertical, 4)
        .frame(height: 22)
        .background(Color.dsSurface.opacity(0.55))
    }

    private var phaseIndicatorColor: Color {
        switch DetailState.resolve(phase) {
        case .loading: Color.dsWarning
        case .empty: Color.dsSuccess
        case .disconnected: Color.dsDanger
        case .incompatible: Color.dsDanger
        }
    }
}
