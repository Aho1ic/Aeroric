import SwiftUI

/// Settings window (M3): left navigation, right scrolling content. Only
/// settings that are actually read somewhere appear (D4). No container
/// accessibility identifier here: it would replace every descendant's own.
///
/// 1124 (note `2026-11-24-settings-redesign`): the navigation is grouped
/// (应用 / Agents / 数据与隐私 / 集成 / 编码, 关于 last) under a search field;
/// a page opens with its title and a one-line subtitle; a search hit opens
/// its page, scrolls to the row and flashes it.
public struct SettingsRootView<State: SettingsStateProviding>: View {
    let state: State
    /// Selected by `id`, like the task list: `List(data, selection:)` tags rows
    /// with their `id` (a `String`), so a `SettingsPane?` binding never matched
    /// and no row could be selected.
    @SwiftUI.State private var paneID: SettingsPane.ID? = SettingsPane.general.id
    @SwiftUI.State private var query = ""
    @SwiftUI.State private var highlight: String?
    @SwiftUI.State private var pendingAnchor: String?

    private var pane: SettingsPane { paneID.flatMap(SettingsPane.init(rawValue:)) ?? .general }

    public init(state: State) {
        self.state = state
    }

    private var panes: [SettingsPane] {
        SettingsPane.allCases.filter {
            switch $0 {
            case .legacyCleanup: state.legacyCleanup != nil
            case .imports: state.settingsImportBook?.isAvailable == true
            case .references: state.settingsReferences?.isAvailable == true
            case .diagnostics: state.settingsDiagnosticsExporter != nil
            case .remote: state.settingsRemote != nil
            default: true
            }
        }
    }

    private var hits: [SettingsIndex.Entry] { SettingsIndex.search(query, in: panes) }

    public var body: some View {
        HStack(spacing: 0) {
            navigation
                .frame(width: 220)
                .background(AeroricColor.sidebar)
                .accessibilityIdentifier("settings.nav")
            HairlineDivider(.vertical)
            page
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(AeroricColor.canvas)
        }
        .frame(minWidth: 860, minHeight: 560)
        // Owner preference: no scroll bars anywhere (2026-10-29).
        .scrollIndicators(.hidden)
        // Round-2 review D1: the one look at connect can fail (the host is
        // busy starting) and then 参考代码库 never showed; ask again each
        // time settings opens.
        .task {
            if let references = state.settingsReferences, !references.isAvailable {
                await references.refresh()
            }
        }
    }

    // MARK: Navigation

    private var navigation: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: "magnifyingglass")
                    .font(.system(size: 11))
                    .foregroundStyle(AeroricColor.textTertiary)
                TextField("搜索设置", text: $query)
                    .textFieldStyle(.plain)
                    .font(.system(size: AeroricType.body))
                    .accessibilityIdentifier("settings.search")
                if !query.isEmpty {
                    Button { query = "" } label: {
                        Image(systemName: "xmark.circle.fill").foregroundStyle(AeroricColor.textTertiary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("清空搜索")
                }
            }
            .padding(.horizontal, AeroricSpace.m)
            .frame(height: 30)
            .background(AeroricColor.layer2, in: Capsule(style: .continuous))
            .padding(.horizontal, AeroricSpace.m)
            .padding(.top, AeroricSpace.xl + AeroricSpace.l)
            ScrollView {
                VStack(alignment: .leading, spacing: 1) {
                    if query.trimmingCharacters(in: .whitespaces).isEmpty { groupedPanes } else { searchResults }
                }
                .padding(.horizontal, AeroricSpace.m)
                .padding(.bottom, AeroricSpace.l)
            }
            .scrollIndicators(.hidden)
        }
    }

    @ViewBuilder private var groupedPanes: some View {
        ForEach(SettingsGroup.allCases, id: \.self) { group in
            let members = panes.filter { $0.group == group }
            if !members.isEmpty {
                if let title = group.title {
                    Text(title)
                        .font(.system(size: AeroricType.caption, weight: .medium))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .padding(.leading, AeroricSpace.m)
                        .padding(.top, AeroricSpace.l)
                        .padding(.bottom, AeroricSpace.xs)
                } else {
                    Color.clear.frame(height: AeroricSpace.l)
                }
                ForEach(members) { item in navRow(item) }
            }
        }
    }

    private func navRow(_ item: SettingsPane) -> some View {
        HoverCapsuleRow(selected: paneID == item.id, height: 30, action: { open(item, anchor: nil) }) {
            Label(item.title, systemImage: item.systemImage)
                .font(.system(size: AeroricType.body))
                .foregroundStyle(AeroricColor.text)
        }
        .accessibilityIdentifier("settings.nav.\(item.rawValue)")
    }

    @ViewBuilder private var searchResults: some View {
        if hits.isEmpty {
            Text("没有找到")
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textTertiary)
                .padding(AeroricSpace.m)
                .accessibilityIdentifier("settings.search.empty")
        } else {
            ForEach(hits) { hit in
                HoverCapsuleRow(selected: paneID == hit.pane.id && highlight == hit.anchor, height: 40,
                                action: { open(hit.pane, anchor: hit.anchor) }) {
                    VStack(alignment: .leading, spacing: 1) {
                        Text(hit.anchor)
                            .font(.system(size: AeroricType.body))
                            .foregroundStyle(AeroricColor.text)
                            .lineLimit(1)
                        Text(hit.pane.title)
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textTertiary)
                    }
                }
                .accessibilityIdentifier("settings.search.hit.\(hit.id)")
            }
        }
    }

    private func open(_ target: SettingsPane, anchor: String?) {
        paneID = target.id
        pendingAnchor = anchor
        highlight = nil
    }

    // MARK: Page

    private var header: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            Text(pane.title)
                .font(.system(size: 22, weight: .semibold))
                .accessibilityIdentifier("settings.page.title")
            Text(pane.subtitle)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder private var page: some View {
        if pane.scrollsItself {
            VStack(alignment: .leading, spacing: AeroricSpace.l) {
                header.padding(.horizontal, AeroricSpace.xxxl).padding(.top, AeroricSpace.xxxl)
                content(pane)
            }
        } else {
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: AeroricSpace.xxl) {
                        header
                        content(pane)
                    }
                    .padding(AeroricSpace.xxxl)
                    .frame(maxWidth: 680, alignment: .leading)
                    // 2026-10-29: the column is centred in the pane.
                    .frame(maxWidth: .infinity, alignment: .center)
                    .environment(\.settingsHighlight, highlight)
                }
                .scrollIndicators(.hidden)
                .id(pane.id)
                .task(id: "\(pane.id)|\(pendingAnchor ?? "")") {
                    guard let anchor = pendingAnchor else { return }
                    try? await Task.sleep(for: .milliseconds(80))
                    withAnimation { proxy.scrollTo(SettingsRowAnchor.id(anchor), anchor: .center) }
                    highlight = anchor
                    try? await Task.sleep(for: .milliseconds(1400))
                    if highlight == anchor { highlight = nil }
                    pendingAnchor = nil
                }
            }
        }
    }

    private func opener(_ page: SettingsMainPage) -> (@MainActor () -> Void)? {
        guard let open = state.settingsOpenPage else { return nil }
        return { open(page) }
    }

    @ViewBuilder private func content(_ pane: SettingsPane) -> some View {
        switch pane {
        case .general:
            VStack(alignment: .leading, spacing: AeroricSpace.xxl) {
                GeneralSettingsPane()
                if let keepAwake = state.settingsKeepAwake, keepAwake.isAvailable {
                    KeepAwakeSettingsCard(store: keepAwake)
                }
                if let idleRelease = state.settingsIdleRelease, idleRelease.isAvailable {
                    IdleReleaseSettingsCard(store: idleRelease, resume: state.settingsResumeAfterRestart)
                }
                MenuBarSettingsCard()
                SidebarSettingsCard()
            }
        case .appearance:
            AppearanceSettingsPane()
        case .notifications:
            NotificationSettingsPane()
        case .shortcuts:
            ShortcutsSettingsPane()
        case .agents:
            AgentsSettingsPane(openAgentConfig: state.settingsOpenAgentConfig, expert: state.settingsExpert)
        case .legacyCleanup:
            if let cleanup = state.legacyCleanup {
                SettingsCard { LegacyCleanupEntry(store: cleanup).padding(.vertical, AeroricSpace.l) }
            }
        case .imports:
            if let book = state.settingsImportBook { ImportsSettingsPane(book: book) }
        case .memory:
            MemorySettingsPane(open: opener(.memory))
        case .diagnostics:
            if let exporter = state.settingsDiagnosticsExporter { DiagnosticsExportCard(export: exporter) }
        case .skills:
            SkillsSettingsPane(openSkills: opener(.skills))
        case .references:
            if let references = state.settingsReferences { ReferenceLibraryPane(store: references) }
        case .remote:
            if let remote = state.settingsRemote { RemoteAccessSettingsView(store: remote) }
        case .connection:
            ConnectionSettingsPane(state: state)
        case .git:
            GitSettingsPane()
        case .worktrees:
            WorktreeSettingsPane()
        case .about:
            AboutSettingsPane(state: state)
        }
    }
}
