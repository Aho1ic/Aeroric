import SwiftUI

/// Project navigation. System `List(selection:)` sits on the real
/// `NativeMaterial.sidebar` material (the List's own chrome background is
/// hidden so the material shows through) — no hand-drawn selection state
/// (ADR-001 §4 rule 4; the shared selection vocabulary lives in
/// `NativeSelection.swift`).
///
/// Width policy (P4-04 Action 1): initial 220pt, range 180–360pt; the drag
/// handle lives in `WorkspaceRootView`'s overlay so it cannot be captured by
/// the List's scroll views.
public struct ProjectSidebar: View {
    @Binding var selection: String?
    let items: [SidebarItem]
    let width: CGFloat
    public init(selection: Binding<String?>, items: [SidebarItem], width: CGFloat) {
        self._selection = selection
        self.items = items
        self.width = width
    }

    public var body: some View {
        NativeMaterial.sidebar
            .overlay {
                if items.isEmpty {
                    emptyContent
                } else {
                    List(selection: $selection) {
                        ForEach(items) { item in
                            Text(item.title)
                                .lineLimit(1)
                                .tag(Optional(item.id))
                                .accessibilityIdentifier("sidebar.item.\(item.id)")
                        }
                    }
                    .listStyle(.sidebar)
                    .scrollContentBackground(.hidden)
                    .accessibilityIdentifier("sidebar.list")
                }
            }
            .frame(width: width)
            // Container-level identifiers propagate onto leaves and replace
            // their own (see WorkspaceDetail) — sidebar.item.* / sidebar.list
            // must stay queryable, so nothing is tagged here.
    }

    private var emptyContent: some View {
        VStack(spacing: 8) {
            Text("项目")
                .font(.headline)
                .foregroundStyle(Color.dsText)
            Text("暂无项目")
                .font(.caption)
                .foregroundStyle(Color.dsTextMuted)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
