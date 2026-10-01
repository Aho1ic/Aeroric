import SwiftUI
import AeroricUI

/// Bootstrap surface (P4-01 Action 6): shows the injected environment and the
/// build/wire identifiers so a cold launch can be verified mechanically by
/// `pnpm native:ui-test` (`AppLaunchTests`). Business UI arrives with P4-04.
///
/// P4-02 adds the live connection row: `ConnectionStore.phaseText` mirrors the
/// supervisor phase (launching → connecting → ready / reconnecting / failed),
/// so the cold-launch test can assert the wire is being driven, not just that
/// the window renders.
///
/// P4-03 wires the design layer for its first real render: the transparent
/// title-bar chrome (`WindowAppearance`), the AppKit material baseline
/// (`NativeMaterial`) and one `GlassPanel` shell — enough to verify the
/// material actually reacts to window activation and appearance changes.
struct BootstrapView: View {
    let services: AppServices

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Aeroric Native Preview")
                .font(.title2.weight(.semibold))
            GlassPanel(preference: .glass) {
                VStack(alignment: .leading, spacing: 12) {
                    LabeledContent("environment") {
                        Text(services.configuration.environmentLabel)
                            .accessibilityIdentifier("bootstrap.environment")
                    }
                    LabeledContent("protocol") {
                        Text("v\(services.protocolVersion)")
                            .accessibilityIdentifier("bootstrap.protocol")
                    }
                    LabeledContent("build") {
                        Text(services.buildIdentifier)
                            .accessibilityIdentifier("bootstrap.build")
                    }
                }
                .padding(16)
            }
            if let dataDirectory = services.configuration.dataDirectoryPath {
                LabeledContent("data dir") {
                    Text(dataDirectory)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .help(dataDirectory)
                        .accessibilityIdentifier("bootstrap.data-dir")
                }
            }
            LabeledContent("connection") {
                Text(services.connectionStore.phaseText)
                    .accessibilityIdentifier("bootstrap.connection")
            }
            LabeledContent("application") {
                Text(services.applicationStore.environmentTag)
                    .accessibilityIdentifier("bootstrap.application")
            }
        }
        .padding(32)
        .frame(minWidth: 460, minHeight: 260)
        .background(NativeMaterial.underWindowBackground.ignoresSafeArea())
        .background(WindowAppearance())
    }
}
