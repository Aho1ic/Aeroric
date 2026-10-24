import SwiftUI

/// Settings window (M3): left navigation, right scrolling content of cards.
/// Only settings that are actually read somewhere appear (D4). No container
/// accessibility identifier here: it would replace every descendant's own.
public struct SettingsRootView<State: SettingsStateProviding>: View {
    let state: State
    /// Fallbacks for previews/tests without an `InteractionModeController`.
    let interactionMode: Binding<InteractionMode>?
    let onInteractionModeSave: ((InteractionMode) -> Void)?
    let interactionModeSaveError: String?
    @SwiftUI.State private var pane: SettingsPane? = .general

    public init(
        state: State,
        interactionMode: Binding<InteractionMode>? = nil,
        onInteractionModeSave: ((InteractionMode) -> Void)? = nil,
        interactionModeSaveError: String? = nil
    ) {
        self.state = state
        self.interactionMode = interactionMode
        self.onInteractionModeSave = onInteractionModeSave
        self.interactionModeSaveError = interactionModeSaveError
    }

    private var panes: [SettingsPane] {
        SettingsPane.allCases.filter {
            switch $0 {
            case .legacyCleanup: state.legacyCleanup != nil
            case .imports: state.settingsImportBook?.isAvailable == true
            default: true
            }
        }
    }

    public var body: some View {
        HStack(spacing: 0) {
            List(panes, selection: $pane) { item in
                Label(item.title, systemImage: item.systemImage)
                    .tag(Optional(item))
                    .accessibilityIdentifier("settings.nav.\(item.rawValue)")
            }
            .listStyle(.sidebar)
            .frame(width: 190)
            .accessibilityIdentifier("settings.nav")
            HairlineDivider(.vertical)
            ScrollView {
                VStack(alignment: .leading, spacing: AeroricSpace.xl) {
                    Text((pane ?? .general).title)
                        .font(.system(size: AeroricType.largeTitle, weight: .semibold))
                    content(pane ?? .general)
                }
                .padding(AeroricSpace.xxxl)
                .frame(maxWidth: 620, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .background(AeroricColor.canvas)
        }
        .frame(minWidth: 680, minHeight: 460)
    }

    @ViewBuilder private func content(_ pane: SettingsPane) -> some View {
        switch pane {
        case .general:
            GeneralSettingsPane(controller: state.settingsInteractionMode, fallbackMode: interactionMode,
                                onFallbackSave: onInteractionModeSave, fallbackError: interactionModeSaveError)
        case .appearance:
            AppearanceSettingsPane()
        case .agents:
            AgentsSettingsPane()
        case .imports:
            if let book = state.settingsImportBook { ImportsSettingsPane(book: book) }
        case .connection:
            ConnectionSettingsPane(state: state)
        case .legacyCleanup:
            if let cleanup = state.legacyCleanup {
                SettingsCard { LegacyCleanupEntry(store: cleanup).padding(AeroricSpace.l) }
            }
        case .about:
            AboutSettingsPane(state: state)
        }
    }
}
