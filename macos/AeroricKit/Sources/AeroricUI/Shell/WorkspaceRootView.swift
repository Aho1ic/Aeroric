import SwiftUI
import os

/// Root shell: TitleBar (top safe area) + Sidebar + Detail, with StatusBar
/// hung via `safeAreaInset(.bottom)` (the primuse lesson: a plain VStack
/// row would cut the material off behind floating chrome). Layout policy
/// (P4-04 Action 1): sidebar initial 220pt, range 180–360pt; main window
/// minimum 1000×640pt. Secondary surfaces compress first via the sidebar
/// visibility toggle and the width drag handle.
public struct WorkspaceRootView<State: WorkspaceStateProviding>: View {
    let state: State
    /// App-side execution for palette rows (P4-04 Action 4): the shell stays
    /// dumb — it dismisses and forwards; the app decides what a command id
    /// does. Ids outside the registered set are ignored by the router.
    let onCommand: (PaletteCommand) -> Void

    // Per-window state: WindowGroup copies the content view per window, so
    // each window owns its sidebar selection and palette presentation; the
    // focused window's bindings are exposed to AppCommands via
    // `focusedSceneValue` (below).
    @State private var selection: String?
    @State private var palettePresented = false

    @State private var sidebarWidth: CGFloat = 220
    @State private var sidebarVisible = true

    public init(
        state: State,
        onCommand: @escaping (PaletteCommand) -> Void = { _ in }
    ) {
        self.state = state
        self.onCommand = onCommand
    }

    public var body: some View {
        HStack(spacing: 0) {
            if sidebarVisible {
                ProjectSidebar(
                    selection: $selection,
                    items: state.sidebarItems,
                    width: sidebarWidth
                )
                .transition(.move(edge: .leading).combined(with: .opacity))
            }
            WorkspaceDetail(state: state)
        }
        // The drag handle lives on the whole HStack's overlay so crossing
        // into the detail's scroll views never steals the gesture (the
        // primuse placement, MacContentView :246).
        .overlay(alignment: .leading) {
            if sidebarVisible {
                SidebarResizeHandle(width: $sidebarWidth)
                    .frame(width: 10)
                    .frame(maxHeight: .infinity)
                    .offset(x: sidebarWidth - 5)
            }
        }
        .safeAreaInset(edge: .top, spacing: 0) {
            TitleBar(title: "Aeroric")
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            StatusBar(
                phase: state.phaseSnapshot,
                phaseText: state.phaseText,
                environmentLabel: state.environmentLabel
            )
        }
        .background(NativeMaterial.underWindowBackground.ignoresSafeArea())
        .animation(.easeInOut(duration: DSMotion.normal), value: sidebarVisible)
        .frame(minWidth: 1000, minHeight: 640)
        .focusedValue(\.palettePresentation, $palettePresented)
        .focusedValue(\.workspaceSelection, $selection)
        .focusedSceneValue(\.workspaceSelection, $selection)
        .focusedValue(\.workspaceActivity, true)
        .overlay {
            if palettePresented {
                CommandPalette(
                    isPresented: $palettePresented,
                    commands: state.paletteCommands,
                    onSelect: handleCommand
                )
                .zIndex(1)
            }
        }
    }

    private func handleCommand(_ command: PaletteCommand) {
        onCommand(command)
    }
}

/// Drag handle for the sidebar width. Clamps to the P4-04 range 180–360pt.
struct SidebarResizeHandle: View {
    @Binding var width: CGFloat

    var body: some View {
        RoundedRectangle(cornerRadius: 1)
            .fill(Color.dsBorder.opacity(0.4))
            .frame(width: 3)
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 1)
                    .onChanged { value in
                        width = min(360, max(180, width + value.translation.width))
                    }
            )
            .accessibilityIdentifier("sidebar.resize.handle")
            .accessibilityLabel("调整侧栏宽度")
    }
}

// MARK: - Focused values (P4-04 Action 4)

/// The focused workspace window presents the command palette through this
/// binding; the menu command (Cmd+K) flips it. Multiple windows each
/// register their own binding — only the focused window's is seen here.
public struct PalettePresentationKey: FocusedValueKey {
    public typealias Value = Binding<Bool>
}

/// The focused window's sidebar selection: commands that target "the project
/// the user is looking at" bind through this (P4-04 Action 4's "focused
/// workspace decides the command target"). `focusedSceneValue` ensures the
/// value follows window focus, not just view focus.
public struct WorkspaceSelectionKey: FocusedValueKey {
    public typealias Value = Binding<String?>
}

/// Whether the focused scene is a workspace (settings/about windows register
/// nothing, so commands that need a workspace disable themselves).
public struct WorkspaceActivityKey: FocusedValueKey {
    public typealias Value = Bool
}

public extension FocusedValues {
    var palettePresentation: Binding<Bool>? {
        get { self[PalettePresentationKey.self] }
        set { self[PalettePresentationKey.self] = newValue }
    }

    var workspaceSelection: Binding<String?>? {
        get { self[WorkspaceSelectionKey.self] }
        set { self[WorkspaceSelectionKey.self] = newValue }
    }

    var workspaceActivity: Bool? {
        get { self[WorkspaceActivityKey.self] }
        set { self[WorkspaceActivityKey.self] = newValue }
    }
}
