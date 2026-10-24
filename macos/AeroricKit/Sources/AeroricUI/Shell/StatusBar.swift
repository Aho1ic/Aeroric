import SwiftUI

/// Bottom strip: connection phase (indicator + the store-formatted
/// `phaseText`) plus an optional trailing context line. Hung via
/// `safeAreaInset(.bottom)` so the sidebar/detail backgrounds extend to the
/// window bottom behind it — a plain VStack row would cut the material off
/// with its own bar (the primuse lesson, MacContentView :256).
///
/// ## Density and content (2026-10-13)
///
/// Q5 approved a 32pt strip with 11pt secondary text.
/// The trailing slot carries *context* — what the window is currently
/// scoped to — not the raw environment tag: `dev` / `default` is a build
/// fact with no place in a user's reading path, so it is shown only where
/// developer facts are honest (`isDiagnosticEnvironment`, the same single
/// rule the bootstrap block uses).
public struct StatusBar: View {
    let phase: ConnectionPhaseSnapshot
    let phaseText: String
    let environmentLabel: String
    /// What the window is scoped to (project name, task count, …). The app
    /// formats it; the shell does not interpret it.
    let context: String?

    public init(
        phase: ConnectionPhaseSnapshot,
        phaseText: String,
        environmentLabel: String,
        context: String? = nil
    ) {
        self.phase = phase
        self.phaseText = phaseText
        self.environmentLabel = environmentLabel
        self.context = context
    }

    public var body: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(phaseIndicatorColor)
                .frame(width: 6, height: 6)
                .accessibilityHidden(true)
            Text(phaseText)
                .lineLimit(1)
                .accessibilityIdentifier("statusbar.phase")
            Spacer(minLength: 12)
            if let context, !context.isEmpty {
                Text(context)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .accessibilityIdentifier("statusbar.context")
            }
            if isDiagnosticEnvironment {
                Text(environmentLabel)
                    .foregroundStyle(Color.dsTextMuted)
                    .accessibilityIdentifier("statusbar.environment")
            }
        }
        .font(AeroricFont.ui(AeroricFont.meta))
        .foregroundStyle(SessionPalette.secondary)
        .padding(.horizontal, 12)
        .frame(height: NativePanelGeometry.mainShell.statusBarHeight)
        .background(SessionPalette.gutter)
        // Redesign v3 (Orca 24pt strip): text only, on the gutter between panels.
    }

    /// Same single rule as the bootstrap block (`DiagnosticEnvironment`):
    /// only `ui-test`, the environment every UI test runs in. Any other
    /// launch — including `dev`, which is how the app is run interactively —
    /// shows the connection phase and the context, not the build's
    /// environment tag.
    private var isDiagnosticEnvironment: Bool {
        DiagnosticEnvironment.isDiagnostic(environmentLabel)
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
