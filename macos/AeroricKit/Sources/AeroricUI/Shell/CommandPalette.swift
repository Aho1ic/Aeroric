import SwiftUI

/// Filter for the palette. Pure function — unit-tested in ShellPolicyTests.
/// Empty (or whitespace-only) queries return the full list in app order;
/// otherwise case-insensitive substring match on the title.
public func filterCommands(_ commands: [PaletteCommand], query: String) -> [PaletteCommand] {
    let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else { return commands }
    return commands.filter { $0.title.localizedCaseInsensitiveContains(trimmed) }
}

/// Command palette surface (P4-04 Action 1 skeleton): query field, filtered
/// command list, Enter to run, Esc to dismiss. The command list is a system
/// `List(selection:)` — no hand-drawn highlight (ADR-001 §4 rule 4). The
/// command list and execution targets come from `WorkspaceStateProviding`
/// (P4-04 Action 4 wires the app commands); this view owns only the
/// interaction mechanics.
public struct CommandPalette: View {
    @Binding var isPresented: Bool
    let commands: [PaletteCommand]
    let onSelect: (PaletteCommand) -> Void

    @State private var query = ""
    @State private var selectedCommand: PaletteCommand?

    public init(
        isPresented: Binding<Bool>,
        commands: [PaletteCommand],
        onSelect: @escaping (PaletteCommand) -> Void
    ) {
        self._isPresented = isPresented
        self.commands = commands
        self.onSelect = onSelect
    }

    private var visible: [PaletteCommand] {
        filterCommands(commands, query: query)
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(AeroricColor.textTertiary)
                TextField("输入命令…", text: $query)
                    .textFieldStyle(.plain)
                    .onSubmit(runSelected)
                Text("esc")
                    .font(.caption)
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(.thinMaterial)
            Divider().overlay(AeroricColor.separator2)
            if visible.isEmpty {
                Text("没有匹配的命令")
                    .font(.callout)
                    .foregroundStyle(AeroricColor.textTertiary)
                    .padding(.vertical, 20)
            } else {
                List(selection: $selectedCommand) {
                    ForEach(visible) { command in
                        HStack {
                            Text(command.title)
                            Spacer()
                        }
                        .contentShape(Rectangle())
                        .tag(Optional(command))
                        .onTapGesture { run(command) }
                        .accessibilityIdentifier("palette.command.\(command.id)")
                    }
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                .background(.ultraThinMaterial)
            }
        }
        .frame(width: 520)
        .frame(maxHeight: 380)
        .clipShape(RoundedRectangle(cornerRadius: AeroricRadius.xl, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: AeroricRadius.xl, style: .continuous)
                .strokeBorder(AeroricColor.separator2.opacity(0.6), lineWidth: 0.5)
        )
        .shadow(color: .black.opacity(0.18), radius: 18, y: 6)
        .onAppear { selectedCommand = nil }
        .background(
            Button("") { isPresented = false }
                .keyboardShortcut(.escape, modifiers: [])
                .hidden()
        )
        .accessibilityIdentifier("command.palette")
    }

    private func runSelected() {
        let target = selectedCommand ?? visible.first
        guard let target else { return }
        run(target)
    }

    private func run(_ command: PaletteCommand) {
        isPresented = false
        onSelect(command)
    }
}
