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
    /// What the window is scoped to, or why counts are unavailable. The app
    /// formats it; the shell does not interpret it.
    let context: String?
    /// Live running / waiting counts (overhaul M1); nil until the mirror loads.
    let counts: ActivityCounts?
    /// The host holds a no-idle-sleep assertion for running tasks (A14).
    let keepingAwake: Bool

    public init(
        phase: ConnectionPhaseSnapshot,
        phaseText: String,
        environmentLabel: String,
        context: String? = nil,
        counts: ActivityCounts? = nil,
        keepingAwake: Bool = false
    ) {
        self.phase = phase
        self.phaseText = phaseText
        self.environmentLabel = environmentLabel
        self.context = context
        self.counts = counts
        self.keepingAwake = keepingAwake
    }

    public var body: some View {
        HStack(spacing: AeroricSpace.m) {
            Circle()
                .fill(phaseIndicatorColor)
                .frame(width: 6, height: 6)
                .accessibilityHidden(true)
            Text(phaseText)
                .lineLimit(1)
                .accessibilityIdentifier("statusbar.phase")
            Spacer(minLength: AeroricSpace.l)
            if let context, !context.isEmpty {
                Text(context)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .accessibilityIdentifier("statusbar.context")
            }
            if keepingAwake {
                Label("阻止睡眠中", systemImage: "cup.and.saucer")
                    .labelStyle(.titleAndIcon)
                    .help("有任务在运行，Aeroric 正在阻止 Mac 进入空闲睡眠")
                    .accessibilityIdentifier("statusbar.keep-awake")
            }
            if let counts {
                countLabel(.running, "运行中", counts.running)
                countLabel(.waiting, "待回应", counts.waiting)
            }
            if isDiagnosticEnvironment {
                Text(environmentLabel)
                    .foregroundStyle(AeroricColor.textTertiary)
                    .accessibilityIdentifier("statusbar.environment")
            }
        }
        .font(.system(size: AeroricType.caption))
        .foregroundStyle(AeroricColor.textSecondary)
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: NativePanelGeometry.mainShell.statusBarHeight)
        .background(AeroricColor.canvas)
        .overlay(alignment: .top) { HairlineDivider() }
    }

    private func countLabel(_ state: ActivityState, _ title: String, _ value: Int) -> some View {
        HStack(spacing: AeroricSpace.xs) {
            Circle().fill(value > 0 ? state.color : AeroricColor.textTertiary.opacity(0.5))
                .frame(width: 6, height: 6)
            Text("\(title) \(value)").monospacedDigit()
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("statusbar.count.\(title)")
    }

    /// Same single rule as the bootstrap block (`DiagnosticEnvironment`):
    /// only `ui-test`, the environment every UI test runs in. Any other
    /// launch — including `dev`, which is how the app is run interactively —
    /// shows the connection phase and the context, not the build's
    /// environment tag.
    private var isDiagnosticEnvironment: Bool {
        DiagnosticEnvironment.isDiagnostic(environmentLabel)
    }

    private var phaseIndicatorColor: Color { ConnectionIndicator.color(phase) }
}
