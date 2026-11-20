import AeroricProtocol
import SwiftUI
import UniformTypeIdentifiers

/// Agent 配置 → 能力包 (H6, 2026-11-21): install once, every harness's new
/// sessions get it; each pack shows, per harness, what is given and why not.
public struct CapabilityPacksPane: View {
    let book: PackBook
    @State private var draft: PackServerDraft?
    @State private var importing = false
    @State private var expanded: Set<String> = []
    @State private var confirming: LivePack?
    @State private var removing: LivePack?

    public init(book: PackBook) { self.book = book }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AeroricSpace.l) {
                HStack(spacing: AeroricSpace.m) {
                    Button("添加 MCP 服务器") { draft = PackServerDraft() }
                        .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                        .fixedSize()
                        .accessibilityIdentifier("agent-config.packs.add-server")
                    Button("导入能力包文件夹…") { importing = true }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .help("选一个含 pack.json 的文件夹；导入后要先确认来源，它的 MCP 服务器和事件处理才会运行")
                        .accessibilityIdentifier("agent-config.packs.import")
                    Spacer()
                }
                Text("能力包里的技能、MCP 服务器、角色、命令和事件处理在这里装一次，Claude、Codex、OMP、DSH 的新会话都能用；某一家用不上的，展开后写明原因。改动在会话下次启动时生效。")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
                if let error = book.errorText {
                    Text(error)
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.danger)
                        .textSelection(.enabled)
                }
                if book.loaded && book.overview.packs.isEmpty && book.overview.broken.isEmpty {
                    PaneEmptyState(systemImage: "shippingbox", title: "还没有能力包",
                                   hint: "添加一个 MCP 服务器，或导入一个能力包文件夹")
                        .frame(height: 220)
                }
                ForEach(book.overview.packs) { view in card(view) }
                if !book.overview.broken.isEmpty { broken }
            }
            .frame(maxWidth: 760, alignment: .leading)
            .padding(.horizontal, AeroricSpace.huge)
            .padding(.vertical, AeroricSpace.xxxl)
            .frame(maxWidth: .infinity)
        }
        .scrollIndicators(.hidden)
        .task { await book.refresh() }
        .sheet(item: Binding(get: { draft.map(DraftBox.init) }, set: { draft = $0?.draft })) { box in
            ServerForm(draft: box.draft, onCancel: { draft = nil }) { server in
                Task { if await book.save(server) { draft = nil } }
            }
        }
        .fileImporter(isPresented: $importing, allowedContentTypes: [.folder]) { result in
            if case let .success(url) = result { Task { await book.importFolder(url.path) } }
        }
        .confirmationDialog("确认这个能力包的来源？", isPresented: Binding(get: { confirming != nil },
                                                                     set: { if !$0 { confirming = nil } }),
                            presenting: confirming) { pack in
            Button("确认来源") { Task { await book.setConfirmed(pack.id, true) } }
            Button("取消", role: .cancel) {}
        } message: { pack in
            Text("确认后，“\(pack.name)”里的 MCP 服务器和事件处理会在会话启动时运行这台 Mac 上的命令。来源：\(PackText.source(pack))")
        }
        .confirmationDialog("删除这个能力包？", isPresented: Binding(get: { removing != nil },
                                                                 set: { if !$0 { removing = nil } }),
                            presenting: removing) { pack in
            Button("移到废纸篓", role: .destructive) { Task { await book.remove(pack.id) } }
            Button("取消", role: .cancel) {}
        } message: { pack in
            Text("“\(pack.name)”的文件夹会移到废纸篓，会话下次启动时不会带上它。")
        }
        .accessibilityIdentifier("agent-config.packs")
    }

    private func card(_ view: LivePackView) -> some View {
        let pack = view.pack
        let open = expanded.contains(pack.id)
        return VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.m) {
                Image(systemName: "shippingbox").foregroundStyle(AeroricColor.accent).frame(width: 20)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: AeroricSpace.s) {
                        Text(pack.name).font(.system(size: AeroricType.secondary, weight: .medium))
                        Text(view.trusted ? "来源已确认" : "待确认来源")
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(view.trusted ? AeroricColor.success : AeroricColor.warning)
                    }
                    .lineLimit(1)
                    Text("\(PackText.contents(pack)) · \(PackText.source(pack))")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .help(pack.description.isEmpty ? PackText.source(pack) : pack.description)
                }
                Spacer(minLength: AeroricSpace.m)
                if !view.trusted {
                    Button("确认来源") { confirming = pack }
                        .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                        .fixedSize()
                } else if !pack.isOwnServer && pack.sourceKind == "local" {
                    Button("撤销确认") { Task { await book.setConfirmed(pack.id, false) } }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .help("撤销后，它的 MCP 服务器和事件处理不再随会话启动")
                }
                if pack.isOwnServer, let server = pack.servers.first {
                    Button("编辑") { draft = PackServerDraft(server) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                }
                Button("删除") { removing = pack }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
                Toggle("", isOn: Binding(get: { pack.enabled }, set: { on in Task { await book.setEnabled(pack.id, on) } }))
                    .labelsHidden()
                    .toggleStyle(.switch)
                    .controlSize(.mini)
                    .accessibilityLabel("启用 \(pack.name)")
            }
            Button {
                if open { expanded.remove(pack.id) } else { expanded.insert(pack.id) }
            } label: {
                HStack(spacing: AeroricSpace.s) {
                    Image(systemName: open ? "chevron.down" : "chevron.right")
                    Text(view.places.map(PackText.place).joined(separator: "  "))
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("agent-config.packs.places")
            if open {
                ForEach(view.places, id: \.place) { place in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(place.place).font(.system(size: AeroricType.caption, weight: .semibold))
                        ForEach(Array(place.items.enumerated()), id: \.offset) { _, item in
                            Label {
                                Text(PackText.item(item)).fixedSize(horizontal: false, vertical: true)
                            } icon: {
                                Image(systemName: item.ready ? "checkmark.circle.fill" : "minus.circle")
                                    .foregroundStyle(item.ready ? AeroricColor.success : AeroricColor.textTertiary)
                            }
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textSecondary)
                        }
                    }
                    .padding(.leading, AeroricSpace.xl)
                }
            }
        }
        .padding(AeroricSpace.l)
        .background(AeroricColor.layer1, in: AeroricRadius.shape(AeroricRadius.l))
    }

    private var broken: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Text("读不了的文件夹").font(.system(size: AeroricType.secondary, weight: .semibold))
            ForEach(Array(book.overview.broken.enumerated()), id: \.offset) { _, entry in
                Text("\(entry.folder)：\(entry.reason)")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.warning)
                    .textSelection(.enabled)
            }
        }
    }
}

private struct DraftBox: Identifiable {
    let draft: PackServerDraft
    var id: String { draft.editing ? draft.name : "new" }
}

/// Add or edit one MCP server of the user's own.
private struct ServerForm: View {
    @State var draft: PackServerDraft
    let onCancel: () -> Void
    let onSave: (LivePackServer) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            Text(draft.editing ? "编辑 MCP 服务器" : "添加 MCP 服务器")
                .font(.system(size: AeroricType.body, weight: .semibold))
            if draft.editing {
                Text(draft.name).font(.system(size: AeroricType.secondary).monospaced())
            } else {
                CapsuleField(placeholder: "名字（字母、数字、- 和 _，如 github）", text: $draft.name)
            }
            CapsuleField(placeholder: "用途，一句话（如：查 GitHub 的议题和拉取请求）", text: $draft.description)
            CapsuleField(placeholder: "命令（如 npx）", text: $draft.command)
            CapsuleField(placeholder: "参数，空格分隔（如 -y @modelcontextprotocol/server-github）", text: $draft.args)
            Text("环境变量，每行一个 KEY=VALUE；显示为 •••••••• 的保持原值")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            TextEditor(text: $draft.env)
                .font(.system(size: AeroricType.secondary).monospaced())
                .frame(height: 80)
                .scrollContentBackground(.hidden)
                .padding(AeroricSpace.s)
                .background(AeroricColor.layer1, in: AeroricRadius.shape(AeroricRadius.m))
            Text("这台 Mac 上的命令，会话启动时运行；它的工具照常要过权限。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            HStack {
                Spacer()
                Button("取消", action: onCancel)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                Button("保存") { onSave(draft.server) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .disabled(!draft.ready)
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 480)
    }
}
