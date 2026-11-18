import AeroricProtocol
import AppKit
import SwiftUI

/// 设置 → 参考代码库 (Q-R6): the folders, how far each index is, and adding
/// one (a folder, a name, an optional line on what it is good for).
struct ReferenceLibraryPane: View {
    let store: ReferenceLibraryStore
    /// A folder picked and waiting for its name and purpose.
    @State private var picked: URL?
    @State private var name = ""
    @State private var purpose = ""
    @State private var removing: LiveReference?

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xl) {
            Text("本机上你收集的其他仓库。Agent 写代码前可以用 search_references 和 read_reference 查别人怎么实现同类功能；只读，Aeroric 不改也不拉取这些文件夹。所有项目共用这一份。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
            SettingsCard("文件夹") {
                if store.references.isEmpty {
                    SettingsRow("还没有参考代码库", detail: "添加一个文件夹后，Aeroric 会在后台建索引。") { EmptyView() }
                }
                ForEach(store.references) { reference in
                    row(reference)
                }
            }
            if let picked {
                addForm(picked)
            } else {
                Button { choose() } label: { Label("添加文件夹…", systemImage: "plus") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .accessibilityIdentifier("settings.references.add")
            }
            if let error = store.lastError {
                Text(error)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.danger)
                    .accessibilityIdentifier("settings.references.error")
            }
        }
        .task { await store.refresh() }
        .confirmationDialog("从参考代码库移除「\(removing?.name ?? "")」？", isPresented: Binding(
            get: { removing != nil }, set: { if !$0 { removing = nil } }
        )) {
            Button("移除", role: .destructive) {
                if let id = removing?.id { Task { await store.remove(id) } }
                removing = nil
            }
        } message: {
            Text("只删索引，文件夹本身不动。")
        }
    }

    private func row(_ reference: LiveReference) -> some View {
        SettingsRow(reference.name, detail: detail(reference)) {
            HStack(spacing: AeroricSpace.s) {
                Button("重建索引") { Task { await store.reindex(reference.id) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .disabled(reference.indexing)
                    .accessibilityIdentifier("settings.references.reindex.\(reference.id)")
                Button("移除") { removing = reference }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .accessibilityIdentifier("settings.references.remove.\(reference.id)")
            }
        }
        .help(reference.path)
        .accessibilityIdentifier("settings.references.row.\(reference.id)")
    }

    private func detail(_ reference: LiveReference) -> String {
        var lines = [reference.path]
        if let purpose = reference.purpose { lines.append(purpose) }
        lines.append(reference.statusText)
        return lines.joined(separator: "\n")
    }

    private func addForm(_ folder: URL) -> some View {
        SettingsCard("添加 \(folder.path)") {
            SettingsRow("名称", detail: "Agent 用它指明在哪个库里读。") {
                TextField("", text: $name)
                    .textFieldStyle(.capsule)
                    .frame(width: 220)
                    .accessibilityIdentifier("settings.references.name")
            }
            SettingsRow("用途", detail: "可空，比如“YOLO 检测头和标签分配的写法”。") {
                TextField("", text: $purpose)
                    .textFieldStyle(.capsule)
                    .frame(width: 220)
                    .accessibilityIdentifier("settings.references.purpose")
            }
            SettingsRow("") {
                HStack(spacing: AeroricSpace.s) {
                    Button("取消") { picked = nil }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    Button("添加") {
                        let (path, name, purpose) = (folder.path, name, purpose)
                        picked = nil
                        Task { await store.add(path: path, name: name, purpose: purpose) }
                    }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .accessibilityIdentifier("settings.references.confirm")
                }
            }
        }
    }

    private func choose() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.prompt = "选择"
        panel.message = "选择一个要加入参考代码库的文件夹"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        name = url.lastPathComponent
        purpose = ""
        picked = url
    }
}
