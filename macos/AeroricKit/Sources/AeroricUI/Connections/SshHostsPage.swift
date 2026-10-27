import AeroricClient
import SwiftUI

/// 侧栏 → 连接 (2026-10-27 design v3): its own header (title, counts, search,
/// 新建分组, 添加主机) instead of the tool-section chrome; a Liquid Glass card
/// with hosts as flat capsule rows folded by group (empty groups included,
/// each with a ··· to rename or delete) and an editor in three parts.
public struct SshHostsPage: View {
    @Bindable var store: SshHostsStore
    @State private var query = ""
    @State private var collapsed: Set<String> = []
    @State private var confirmDelete = false
    @State private var groupSheet: GroupSheet?
    @State private var deletingGroup: String?

    /// Creating a group (optionally for the host being edited) or renaming one.
    enum GroupSheet: Identifiable {
        case create(forDraft: Bool)
        case rename(String)
        var id: String {
            switch self {
            case let .create(forDraft): "create.\(forDraft)"
            case let .rename(name): "rename.\(name)"
            }
        }
    }

    public init(store: SshHostsStore) { self.store = store }

    public var body: some View {
        VStack(spacing: AeroricSpace.l) {
            header
            HStack(spacing: 0) {
                list.frame(width: 240)
                HairlineDivider(.vertical)
                ScrollView {
                    editor
                        .frame(maxWidth: 640, alignment: .leading)
                        .padding(AeroricSpace.xxl)
                        .frame(maxWidth: .infinity)
                }
                .scrollIndicators(.hidden)
            }
            .glassSurface(.floatingCard, in: AeroricRadius.shape(26))
        }
        .padding(.horizontal, AeroricSpace.xl)
        .padding(.top, AeroricSpace.l)
        .padding(.bottom, AeroricSpace.xl)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(AeroricColor.canvas)
        .task { await store.refresh() }
        .sheet(item: $store.pendingTrust) { pending in
            HostTrustSheet(pending: pending, onTrust: { Task { await store.trustPending() } },
                           onCancel: { store.pendingTrust = nil })
        }
        .sheet(item: $groupSheet) { sheet in
            GroupNameSheet(sheet: sheet, store: store) { name in
                groupSheet = nil
                if case .create(true) = sheet { store.draft?.group = name }
            } onCancel: { groupSheet = nil }
        }
        .confirmationDialog("删除分组“\(deletingGroup ?? "")”？", isPresented: Binding(
            get: { deletingGroup != nil }, set: { if !$0 { deletingGroup = nil } }
        )) {
            Button("删除分组", role: .destructive) {
                if let name = deletingGroup { Task { await store.deleteGroup(name) } }
            }
        } message: {
            Text("组里的主机会移到“未分组”，不会被删除。")
        }
        .accessibilityIdentifier("connections.page")
    }

    // MARK: Header

    private var header: some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: "network")
                .font(.system(size: 17, weight: .medium))
                .foregroundStyle(AeroricColor.textSecondary)
            VStack(alignment: .leading, spacing: 1) {
                Text("连接").font(.system(size: AeroricType.title, weight: .semibold))
                Text("SSH 主机 · \(store.hosts.count) 台 · \(store.groupNames.count) 个分组")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            Spacer()
            CapsuleField(systemImage: "magnifyingglass", placeholder: "搜索主机", text: $query, height: 30)
                .frame(width: 200)
            Button { groupSheet = .create(forDraft: false) } label: { Label("新建分组", systemImage: "folder.badge.plus") }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                .accessibilityIdentifier("connections.new-group")
            Button { store.newHost() } label: { Label("添加主机", systemImage: "plus") }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                .accessibilityIdentifier("connections.add")
        }
    }

    // MARK: List

    private var list: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 2) {
                let groups = SshHostsStore.grouped(store.hosts, groups: store.savedGroups, query: query)
                if groups.isEmpty {
                    Text(store.hosts.isEmpty ? "还没有 SSH 主机。先“新建分组”或直接“添加主机”。" : "没有匹配的主机")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(AeroricSpace.m)
                }
                ForEach(groups, id: \.group) { group in
                    groupHeader(group.group, count: group.hosts.count)
                    if !collapsed.contains(group.group) || !query.isEmpty {
                        if group.hosts.isEmpty {
                            Text("空分组：添加主机时选它")
                                .font(.system(size: AeroricType.caption))
                                .foregroundStyle(AeroricColor.textTertiary)
                                .padding(.horizontal, AeroricSpace.l)
                                .padding(.bottom, AeroricSpace.xs)
                        }
                        ForEach(group.hosts) { host in hostRow(host) }
                    }
                }
            }
            .padding(AeroricSpace.m)
        }
        .scrollIndicators(.hidden)
    }

    private func groupHeader(_ name: String, count: Int) -> some View {
        HStack(spacing: AeroricSpace.xs) {
            Button {
                if collapsed.contains(name) { collapsed.remove(name) } else { collapsed.insert(name) }
            } label: {
                HStack(spacing: AeroricSpace.xs) {
                    Image(systemName: collapsed.contains(name) && query.isEmpty ? "chevron.right" : "chevron.down")
                        .font(.system(size: 9, weight: .semibold))
                    Text("\(name) · \(count)")
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            if name != SshHostsStore.ungrouped {
                Menu {
                    Button("重命名…") { groupSheet = .rename(name) }
                    Button("删除分组…", role: .destructive) { deletingGroup = name }
                } label: {
                    Image(systemName: "ellipsis").frame(width: 20, height: 16).contentShape(Rectangle())
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .accessibilityLabel("分组操作")
            }
        }
        .font(.system(size: AeroricType.caption, weight: .medium))
        .foregroundStyle(AeroricColor.textTertiary)
        .padding(.horizontal, AeroricSpace.s)
        .padding(.top, AeroricSpace.m)
        .padding(.bottom, 2)
    }

    private func hostRow(_ host: SshClient.Host) -> some View {
        let selected = store.selectedID == host.id
        return Button { store.select(host.id) } label: {
            HStack(spacing: AeroricSpace.s) {
                Circle().fill(dotColor(host.id)).frame(width: 7, height: 7)
                Text(host.name).font(.system(size: AeroricType.secondary, weight: selected ? .semibold : .regular))
                    .lineLimit(1)
                Spacer(minLength: AeroricSpace.s)
                Text(isUntrusted(host.id) ? "待信任" : host.host)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(isUntrusted(host.id) ? AeroricColor.warning : AeroricColor.textTertiary)
                    .lineLimit(1)
            }
            .padding(.horizontal, AeroricSpace.l)
            .frame(height: 28)
            .background(selected ? AeroricColor.selection : Color.clear, in: Capsule(style: .continuous))
            .overlay {
                if selected { Capsule(style: .continuous).strokeBorder(AeroricColor.accent.opacity(0.35), lineWidth: 1) }
            }
            .contentShape(Capsule(style: .continuous))
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("connections.host.\(host.id)")
    }

    private func isUntrusted(_ id: String) -> Bool {
        if case .result(.untrusted)? = store.test(for: id) { return true }
        return false
    }

    private func dotColor(_ id: String) -> Color {
        switch store.test(for: id) {
        case .result(.ok)?: AeroricColor.success
        case .result(.untrusted)?: AeroricColor.warning
        case .result(.failed)?, .result(.unreachable)?: AeroricColor.danger
        case .testing?: AeroricColor.running
        case nil: AeroricColor.textTertiary
        }
    }

    // MARK: Editor

    @ViewBuilder private var editor: some View {
        if store.draft != nil {
            VStack(alignment: .leading, spacing: AeroricSpace.l) {
                HStack(spacing: AeroricSpace.m) {
                    Text(store.selectedID == nil ? "添加 SSH 主机" : (store.draft?.name.isEmpty == false ? store.draft!.name : "SSH 主机"))
                        .font(.system(size: AeroricType.headline, weight: .semibold))
                    if let id = store.selectedID { trustChip(id) }
                }
                section("基本信息") {
                    HStack(spacing: AeroricSpace.l) {
                        labeled("名称") { CapsuleField(placeholder: "开发机", text: draftBinding(\.name)) }
                        labeled("分组") { groupMenu }
                    }
                }
                section("连接") {
                    HStack(spacing: AeroricSpace.l) {
                        labeled("主机") { CapsuleField(placeholder: "dev.example.com", text: draftBinding(\.host)) }
                            .frame(maxWidth: .infinity)
                        labeled("端口") { CapsuleField(placeholder: "22", text: portBinding) }.frame(width: 90)
                        labeled("用户名") { CapsuleField(placeholder: "me", text: draftBinding(\.username)) }
                            .frame(width: 160)
                    }
                }
                section("登录") {
                    CapsuleSlider(selection: $store.authMode, groupTitle: "登录方式", mode: .titleOnly,
                                  identifier: "connections.auth", segments: [
                                      .init(value: .key, title: "密钥 / ssh-agent"),
                                      .init(value: .password, title: "密码"),
                                  ])
                                  .fixedSize()
                    if store.authMode == .key {
                        labeled("私钥文件（可空：用 ssh-agent 与默认密钥）") {
                            CapsuleField(placeholder: "~/.ssh/id_ed25519", text: identityBinding, trailing: "folder") {
                                pickIdentityFile()
                            }
                        }
                    } else {
                        labeled(store.draft?.hasPassword == true ? "密码（已存在钥匙串；留空保持不变）" : "密码（存在钥匙串里）") {
                            CapsuleField(placeholder: "••••••", text: $store.passwordDraft, secure: true)
                        }
                    }
                }
                if let state = store.selectedID.flatMap(store.test(for:)) { testCapsule(state) }
                if let error = store.errorText {
                    StatusCapsule(text: error, systemImage: "exclamationmark.circle", tint: AeroricColor.danger)
                }
                HStack(spacing: AeroricSpace.m) {
                    if store.selectedID != nil {
                        Button("删除主机", role: .destructive) { confirmDelete = true }
                            .buttonStyle(AeroricCapsuleButtonStyle(.destructive))
                    }
                    if let problem = store.draftProblem {
                        Text(problem).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                    }
                    Spacer()
                    if store.selectedID != nil {
                        Button { Task { await store.test() } } label: { Label("测试连接", systemImage: "bolt.horizontal") }
                            .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                            .accessibilityIdentifier("connections.test")
                    }
                    Button("保存") { Task { await store.save() } }
                        .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                        .disabled(store.draftProblem != nil || store.busy)
                        .keyboardShortcut(.defaultAction)
                        .accessibilityIdentifier("connections.save")
                }
                .padding(.top, AeroricSpace.s)
            }
            .confirmationDialog("删除这台 SSH 主机？", isPresented: $confirmDelete) {
                Button("删除", role: .destructive) { Task { await store.delete() } }
            } message: {
                Text("已存的密码也会从钥匙串删除。用它打开的远程项目会无法连接。")
            }
        } else {
            AeroricEmptyState(systemImage: "server.rack", title: "SSH 主机",
                              message: "添加主机后，可以在项目的“+”里打开远程项目，Agent 会在远程机器上运行。")
                .padding(.top, 80)
        }
    }

    @ViewBuilder private func trustChip(_ id: String) -> some View {
        switch store.test(for: id) {
        case .result(.ok)?: StatusChip(text: "已信任", systemImage: "checkmark", tint: AeroricColor.success)
        case .result(.untrusted)?: StatusChip(text: "待信任", tint: AeroricColor.warning)
        case .result(.failed)?, .result(.unreachable)?: StatusChip(text: "连不上", tint: AeroricColor.danger)
        case .testing?: StatusChip(text: "连接中", tint: AeroricColor.running)
        case nil: EmptyView()
        }
    }

    private var groupMenu: some View {
        Menu {
            Button { store.draft?.group = nil } label: {
                if store.draft?.group?.isEmpty != false { Label(SshHostsStore.ungrouped, systemImage: "checkmark") }
                else { Text(SshHostsStore.ungrouped) }
            }
            ForEach(store.groupNames, id: \.self) { name in
                Button { store.draft?.group = name } label: {
                    if store.draft?.group == name { Label(name, systemImage: "checkmark") } else { Text(name) }
                }
            }
            Divider()
            Button("新建分组…", systemImage: "folder.badge.plus") { groupSheet = .create(forDraft: true) }
        } label: {
            HStack {
                Text(store.draft?.group?.isEmpty == false ? store.draft!.group! : SshHostsStore.ungrouped)
                    .foregroundStyle(store.draft?.group?.isEmpty == false ? AeroricColor.text : AeroricColor.textTertiary)
                Spacer()
                Image(systemName: "chevron.down").foregroundStyle(AeroricColor.textSecondary)
            }
            .font(.system(size: AeroricType.body))
            .padding(.horizontal, AeroricSpace.l)
            .frame(height: 32)
            .background(AeroricColor.layer1.opacity(0.85), in: Capsule(style: .continuous))
            .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator2, lineWidth: 1) }
            .contentShape(Capsule(style: .continuous))
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .accessibilityIdentifier("connections.group")
    }

    @ViewBuilder private func testCapsule(_ state: SshHostsStore.TestState) -> some View {
        switch state {
        case .testing:
            StatusCapsule(text: "正在连接…", systemImage: "hourglass", tint: AeroricColor.running)
        case let .result(.ok(agents)):
            let found = agents.compactMap { agent in agent.version.map { "\(agent.agent) \(Self.shortVersion($0))" } }
            let missing = agents.filter { $0.version == nil }.map(\.agent)
            let tail = [found.isEmpty ? nil : "远程有 " + found.joined(separator: "、"),
                        missing.isEmpty ? nil : missing.joined(separator: "、") + " 未安装"]
                .compactMap { $0 }.joined(separator: "，")
            StatusCapsule(text: "连接正常" + (tail.isEmpty ? "" : " · " + tail), systemImage: "checkmark.circle",
                          tint: AeroricColor.success)
        case .result(.untrusted):
            StatusCapsule(text: "尚未信任这台主机的指纹", systemImage: "shield.lefthalf.filled", tint: AeroricColor.warning)
        case let .result(.unreachable(message)), let .result(.failed(message)):
            StatusCapsule(text: message, systemImage: "exclamationmark.triangle", tint: AeroricColor.danger)
        }
    }

    static func shortVersion(_ text: String) -> String {
        text.split(separator: " ").first { $0.first?.isNumber == true }.map(String.init) ?? text
    }

    private func section(_ title: String, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Text(title).font(.system(size: AeroricType.secondary, weight: .semibold))
                .foregroundStyle(AeroricColor.textSecondary)
            content()
        }
    }

    private func labeled(_ title: String, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            Text(title).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
            content()
        }
    }

    private func draftBinding(_ path: WritableKeyPath<SshClient.Host, String>) -> Binding<String> {
        Binding(get: { store.draft?[keyPath: path] ?? "" }, set: { store.draft?[keyPath: path] = $0 })
    }

    private var identityBinding: Binding<String> {
        Binding(get: { store.draft?.identityFile ?? "" }, set: { store.draft?.identityFile = $0.isEmpty ? nil : $0 })
    }

    private var portBinding: Binding<String> {
        Binding(get: { store.draft.map { String($0.port) } ?? "" },
                set: { store.draft?.port = Int($0.filter(\.isNumber)) ?? 0 })
    }

    private func pickIdentityFile() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.showsHiddenFiles = true
        panel.directoryURL = URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent(".ssh")
        panel.prompt = "选择"
        if panel.runModal() == .OK, let url = panel.url { identityBinding.wrappedValue = url.path }
    }
}

/// 新建分组 / 重命名分组 (design v3): one capsule field in a glass card.
struct GroupNameSheet: View {
    let sheet: SshHostsPage.GroupSheet
    let store: SshHostsStore
    let onDone: (String) -> Void
    let onCancel: () -> Void
    @State private var name = ""

    private var renaming: String? {
        if case let .rename(old) = sheet { return old }
        return nil
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Label(renaming == nil ? "新建分组" : "重命名分组", systemImage: "folder.badge.plus")
                .font(.system(size: AeroricType.headline, weight: .semibold))
            CapsuleField(placeholder: "测试环境", text: $name)
                .onSubmit(commit)
            if !name.isEmpty, let problem = store.groupProblem(name, renaming: renaming) {
                Text(problem).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.danger)
            }
            HStack {
                Spacer()
                Button("取消", action: onCancel).buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                Button(renaming == nil ? "创建" : "重命名", action: commit)
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                    .keyboardShortcut(.defaultAction)
                    .disabled(store.groupProblem(name, renaming: renaming) != nil)
                    .accessibilityIdentifier("connections.group-sheet.commit")
            }
        }
        .padding(AeroricSpace.xxl)
        .frame(width: 340)
        .glassSurface(.floatingCard, in: AeroricRadius.shape(26))
        .onAppear { name = renaming ?? "" }
    }

    private func commit() {
        guard store.groupProblem(name, renaming: renaming) == nil else { return }
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        Task {
            if let old = renaming { await store.renameGroup(old, to: trimmed) } else { await store.createGroup(trimmed) }
            onDone(trimmed)
        }
    }
}

/// A text field drawn as a rounded capsule (2026-10-27 design).
struct CapsuleField: View {
    var systemImage: String?
    var placeholder: String
    @Binding var text: String
    var secure = false
    var height: CGFloat = 32
    var trailing: String?
    var onTrailing: (() -> Void)?

    init(systemImage: String? = nil, placeholder: String, text: Binding<String>, secure: Bool = false,
         height: CGFloat = 32, trailing: String? = nil, onTrailing: (() -> Void)? = nil) {
        self.systemImage = systemImage
        self.placeholder = placeholder
        _text = text
        self.secure = secure
        self.height = height
        self.trailing = trailing
        self.onTrailing = onTrailing
    }

    var body: some View {
        HStack(spacing: AeroricSpace.s) {
            if let systemImage {
                Image(systemName: systemImage).foregroundStyle(AeroricColor.textTertiary)
            }
            Group {
                if secure { SecureField(placeholder, text: $text) } else { TextField(placeholder, text: $text) }
            }
            .textFieldStyle(.plain)
            .font(.system(size: AeroricType.body))
            if let trailing {
                Button { onTrailing?() } label: { Image(systemName: trailing).foregroundStyle(AeroricColor.textSecondary) }
                    .buttonStyle(.borderless)
            }
        }
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: height)
        .background(AeroricColor.layer1.opacity(0.85), in: Capsule(style: .continuous))
        .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator2, lineWidth: 1) }
    }
}

/// A one-line status as a tinted capsule.
struct StatusCapsule: View {
    let text: String
    let systemImage: String
    let tint: Color

    var body: some View {
        Label(text, systemImage: systemImage)
            .font(.system(size: AeroricType.secondary))
            .foregroundStyle(tint)
            .lineLimit(2)
            .textSelection(.enabled)
            .padding(.horizontal, AeroricSpace.l)
            .padding(.vertical, AeroricSpace.s)
            .background(tint.opacity(0.12), in: Capsule(style: .continuous))
    }
}

/// First connection to a host: confirm its fingerprint (never accepted on its own).
struct HostTrustSheet: View {
    let pending: SshHostsStore.PendingTrust
    let onTrust: () -> Void
    let onCancel: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Label("第一次连接 \(pending.hostName)", systemImage: "questionmark.shield")
                .font(.system(size: AeroricType.headline, weight: .semibold))
                .foregroundStyle(AeroricColor.text)
            Text("确认这是你的服务器的指纹（在服务器上运行 ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub 可查看）。")
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            ForEach(pending.keys, id: \.knownHostsLine) { key in
                Text("\(key.keyType) · \(key.fingerprint)")
                    .font(AeroricType.mono(AeroricType.caption))
                    .textSelection(.enabled)
                    .padding(.horizontal, AeroricSpace.l)
                    .frame(maxWidth: .infinity, minHeight: 32, alignment: .leading)
                    .background(AeroricColor.layer1.opacity(0.85), in: Capsule(style: .continuous))
                    .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator2, lineWidth: 1) }
            }
            HStack {
                Spacer()
                Button("取消", action: onCancel).buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                Button("信任并继续", action: onTrust)
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                    .keyboardShortcut(.defaultAction)
                    .accessibilityIdentifier("connections.trust")
            }
        }
        .padding(AeroricSpace.xxl)
        .frame(width: 460)
        .glassSurface(.floatingCard, in: AeroricRadius.shape(26))
    }
}

/// 项目 + → 打开远程项目… (2026-10-27): host (by group), remote directory, name.
public struct OpenRemoteProjectSheet: View {
    @Bindable var store: SshHostsStore
    let onOpened: (String) -> Void
    let onCancel: () -> Void
    @State private var hostID: String?
    @State private var path = "~/"
    @State private var name = ""
    @State private var opening = false
    @State private var error: String?

    public init(store: SshHostsStore, onOpened: @escaping (String) -> Void, onCancel: @escaping () -> Void) {
        self.store = store
        self.onOpened = onOpened
        self.onCancel = onCancel
    }

    private var hostTitle: String {
        guard let host = store.hosts.first(where: { $0.id == hostID }) else { return "选择 SSH 主机" }
        return [host.group, host.name].compactMap { $0?.isEmpty == false ? $0 : nil }.joined(separator: " / ")
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Label("打开远程项目", systemImage: "globe").font(.system(size: AeroricType.headline, weight: .semibold))
            if store.hosts.isEmpty {
                StatusCapsule(text: "还没有 SSH 主机：先在侧栏“连接”里添加一台。", systemImage: "info.circle",
                              tint: AeroricColor.textSecondary)
            }
            field("SSH 主机") {
                Menu {
                    ForEach(SshHostsStore.grouped(store.hosts, groups: store.savedGroups).filter { !$0.hosts.isEmpty },
                            id: \.group) { group in
                        Section(group.group) {
                            ForEach(group.hosts) { host in Button("\(host.name) · \(host.username)@\(host.host)") { hostID = host.id } }
                        }
                    }
                } label: {
                    HStack {
                        Text(hostTitle).foregroundStyle(hostID == nil ? AeroricColor.textTertiary : AeroricColor.text)
                        Spacer()
                        Image(systemName: "chevron.down").foregroundStyle(AeroricColor.textSecondary)
                    }
                    .font(.system(size: AeroricType.body))
                    .padding(.horizontal, AeroricSpace.l)
                    .frame(height: 32)
                    .background(AeroricColor.layer1.opacity(0.85), in: Capsule(style: .continuous))
                    .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator2, lineWidth: 1) }
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .accessibilityIdentifier("remote-project.host")
            }
            field("远程目录") { CapsuleField(placeholder: "~/work/project", text: $path) }
            field("项目名（可空，默认取目录名）") { CapsuleField(placeholder: "project", text: $name) }
            if let error { StatusCapsule(text: error, systemImage: "exclamationmark.circle", tint: AeroricColor.danger) }
            HStack {
                Spacer()
                Button("取消", action: onCancel).buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                Button(opening ? "正在打开…" : "打开") { Task { await open() } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                    .keyboardShortcut(.defaultAction)
                    .disabled(hostID == nil || path.trimmingCharacters(in: .whitespaces).isEmpty || opening)
                    .accessibilityIdentifier("remote-project.open")
            }
        }
        .padding(AeroricSpace.xxl)
        .frame(width: 480)
        .glassSurface(.floatingCard, in: AeroricRadius.shape(26))
        .task {
            await store.refresh()
            if hostID == nil { hostID = store.selectedID ?? store.hosts.first?.id }
        }
    }

    private func field(_ title: String, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            Text(title).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textSecondary)
            content()
        }
    }

    private func open() async {
        guard let hostID else { return }
        opening = true
        defer { opening = false }
        do {
            let id = try await store.openRemoteProject(hostID: hostID, path: path.trimmingCharacters(in: .whitespaces),
                                                       name: name)
            onOpened(id)
        } catch {
            self.error = "打开失败：\(error)"
        }
    }
}
