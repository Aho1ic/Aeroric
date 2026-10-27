import SwiftUI

/// Settings window (M3): left navigation, right scrolling content of cards.
/// Only settings that are actually read somewhere appear (D4). No container
/// accessibility identifier here: it would replace every descendant's own.
public struct SettingsRootView<State: SettingsStateProviding>: View {
    let state: State
    /// Selected by `id`, like the task list: `List(data, selection:)` tags rows
    /// with their `id` (a `String`), so a `SettingsPane?` binding never matched
    /// and no row could be selected.
    @SwiftUI.State private var paneID: SettingsPane.ID? = SettingsPane.general.id

    private var pane: SettingsPane? { paneID.flatMap(SettingsPane.init(rawValue:)) }

    public init(state: State) {
        self.state = state
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
            List(panes, selection: $paneID) { item in
                Label(item.title, systemImage: item.systemImage)
                    .tag(Optional(item.id))
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
            GeneralSettingsPane()
            if let keepAwake = state.settingsKeepAwake, keepAwake.isAvailable {
                KeepAwakeSettingsCard(store: keepAwake)
            }
            MenuBarSettingsCard()
        case .appearance:
            AppearanceSettingsPane()
        case .agents:
            AgentsSettingsPane(openAgentConfig: state.settingsOpenAgentConfig)
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
