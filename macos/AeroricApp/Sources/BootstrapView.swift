import SwiftUI

/// Bootstrap surface (P4-01 Action 6): shows the injected environment and the
/// build/wire identifiers so a cold launch can be verified mechanically by
/// `pnpm native:ui-test` (`AppLaunchTests`). Business UI arrives with P4-04;
/// connection state arrives with P4-02.
struct BootstrapView: View {
    let services: AppServices

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Aeroric Native Preview")
                .font(.title2.weight(.semibold))
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
            if let dataDirectory = services.configuration.dataDirectoryPath {
                LabeledContent("data dir") {
                    Text(dataDirectory)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .help(dataDirectory)
                        .accessibilityIdentifier("bootstrap.data-dir")
                }
            }
        }
        .padding(32)
        .frame(minWidth: 460, minHeight: 260)
    }
}
