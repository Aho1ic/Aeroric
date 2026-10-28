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
        .sheet(item: $store.pendingInstall) { pending in
            RemoteInstallSheet(pending: pending,
                               method: Binding(get: { store.pendingInstall?.method ?? pending.method },
                                               set: { store.pendingInstall?.method = $0 }),
                               onInstall: { Task { await store.confirmInstall() } },
                               onCancel: { store.pendingInstall = nil })
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
                if let id = store.selectedID, let probe = store.probes[id] { remoteAgents(id, probe) }
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

    private static let agentTitles = [("claude", "Claude Code", "asterisk"),
                                      ("codex", "Codex", "chevron.left.forwardslash.chevron.right"),
                                      ("omp", "omp", "circle.hexagongrid")]

    /// R1/R3: each agent on the host, with install / upgrade where needed.
    private func remoteAgents(_ hostID: String, _ probe: SshClient.Probe) -> some View {
        section("远程 Agent") {
            Text(probe.npm.map { "npm \($0)" + (probe.node.map { " · node \($0)" } ?? "") } ?? "远程没有 npm：可用“本机下载后传输”安装")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            ForEach(Self.agentTitles, id: \.0) { agent, title, symbol in
                remoteAgentRow(hostID: hostID, agent: agent, title: title, symbol: symbol,
                               state: SshHostsStore.remoteAgent(agent, in: probe))
            }
            if store.packageCacheBytes > 0 {
                HStack(spacing: AeroricSpace.s) {
                    Text("本机缓存的独立安装包 "
                        + ByteCountFormatter.string(fromByteCount: Int64(clamping: store.packageCacheBytes), countStyle: .file))
                    Button("清空") { Task { await store.clearPackageCache() } }
                        .buttonStyle(.link)
                        .accessibilityIdentifier("connections.packages.clear")
                }
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            }
        }
    }

    private func remoteAgentRow(hostID: String, agent: String, title: String, symbol: String,
                                state: SshHostsStore.RemoteAgent) -> some View {
        let job = store.installJob(hostID, agent)
        return VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            HStack(spacing: AeroricSpace.m) {
                Image(systemName: symbol).frame(width: 16).foregroundStyle(AeroricColor.textSecondary)
                Text(title).font(.system(size: AeroricType.secondary, weight: .medium))
                if job?.running == true {
                    StatusChip(text: "安装中", tint: AeroricColor.running)
                } else {
                    switch state {
                    case .missing: StatusChip(text: "未安装", tint: AeroricColor.danger)
                    case let .tooOld(version, minimum):
                        StatusChip(text: "\(version) · 需要 \(minimum)+", tint: AeroricColor.warning)
                    case let .ready(version): StatusChip(text: version, systemImage: "checkmark", tint: AeroricColor.success)
                    }
                }
                Spacer()
                if job?.running == true || store.preparing.contains(hostID + "." + agent) {
                    ProgressView().controlSize(.small)
                } else {
                    switch state {
                    case .missing:
                        Button { Task { await store.prepareInstall(hostID: hostID, agent: agent) } } label: {
                            Label("在远程安装", systemImage: "arrow.down.circle")
                        }
                        .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                        .accessibilityIdentifier("connections.install.\(agent)")
                    case .tooOld:
                        Button { Task { await store.prepareInstall(hostID: hostID, agent: agent) } } label: {
                            Label("在远程升级", systemImage: "arrow.up.circle")
                        }
                        .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                        .accessibilityIdentifier("connections.upgrade.\(agent)")
                    case .ready:
                        Text("已可用").font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                    }
                }
            }
            .padding(.leading, AeroricSpace.l)
            .padding(.trailing, AeroricSpace.s)
            .frame(height: 38)
            .background(AeroricColor.layer1.opacity(0.85), in: Capsule(style: .continuous))
            .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator1, lineWidth: 1) }
            if let job, job.running, job.method == "offline" {
                OfflineInstallProgress(job: job).padding(.leading, AeroricSpace.xl)
            }
            if let job, job.running || job.state == "failed", !job.log.isEmpty || job.message != nil {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(Array(job.log.suffix(6).enumerated()), id: \.offset) { _, line in
                        Text(line).lineLimit(1).truncationMode(.middle)
                    }
                    if job.state == "failed", let message = job.message {
                        Text(message).foregroundStyle(AeroricColor.danger).fixedSize(horizontal: false, vertical: true)
                    }
                }
                .font(AeroricType.mono(AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(AeroricSpace.m)
                .background(AeroricColor.layer2, in: AeroricRadius.shape(16))
                .padding(.leading, AeroricSpace.xl)
            }
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

/// "在 Mac mini 上安装 Claude Code": pick direct or offline (O1), see exactly
/// what runs / what is sent before anything happens.
struct RemoteInstallSheet: View {
    let pending: SshHostsStore.PendingInstall
    @Binding var method: String
    let onInstall: () -> Void
    let onCancel: () -> Void

    private var title: String {
        let name = ["claude": "Claude Code", "codex": "Codex", "omp": "omp"][pending.agent] ?? pending.agent
        return "在 \(pending.hostName) 上\(pending.upgrade ? "升级" : "安装") \(name)"
    }

    private var plan: SshClient.InstallPlan { pending.plan }
    private var ready: Bool { method == "offline" ? plan.offline != nil : plan.command != nil }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Label(title, systemImage: "arrow.down.circle").font(.system(size: AeroricType.headline, weight: .semibold))
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                Text("安装方式").font(.system(size: AeroricType.secondary)).foregroundStyle(AeroricColor.textSecondary)
                CapsuleSlider(selection: $method, groupTitle: "安装方式", identifier: "connections.install.method",
                              segments: [
                                  .init(value: "direct", title: "远程直接下载", systemImage: "network"),
                                  .init(value: "offline", title: "本机下载后传输", systemImage: "arrow.up.forward.app"),
                              ])
                    .fixedSize()
            }
            if let reason = recommendation {
                StatusCapsule(text: reason, systemImage: "wifi.slash", tint: AeroricColor.warning)
            }
            if method == "offline" { offlineDetails } else { directDetails }
            HStack {
                Spacer()
                Button("取消", action: onCancel).buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                if ready {
                    Button(confirmTitle, action: onInstall)
                        .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                        .keyboardShortcut(.defaultAction)
                        .accessibilityIdentifier("connections.install.confirm")
                }
            }
        }
        .padding(AeroricSpace.xxl)
        .frame(width: 500)
        .glassSurface(.floatingCard, in: AeroricRadius.shape(26))
    }

    private var confirmTitle: String {
        method == "offline" ? "传输并\(pending.upgrade ? "升级" : "安装")" : (pending.upgrade ? "升级" : "安装")
    }

    /// Why the offline way was picked for the user, shown while it is selected.
    private var recommendation: String? {
        guard plan.recommended == "offline", method == "offline" else { return nil }
        if plan.command == nil { return "远程没有 npm，已自动选择传输：独立安装包不需要 Node.js" }
        return "远程连不上 \(plan.directHost)，已自动选择传输"
    }

    @ViewBuilder private var directDetails: some View {
        if let command = plan.command {
            Text("将经 SSH 在远程运行：").font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
            commandCapsule(command)
            if plan.directReachable == false {
                StatusCapsule(text: "上次测试时远程连不上 \(plan.directHost)，直接下载可能会失败",
                              systemImage: "exclamationmark.triangle", tint: AeroricColor.warning)
            }
        } else {
            StatusCapsule(text: "远程没有 npm：改用“本机下载后传输”，或先在远程安装 Node.js（含 npm）。",
                          systemImage: "info.circle", tint: AeroricColor.warning)
        }
    }

    @ViewBuilder private var offlineDetails: some View {
        if let offline = plan.offline {
            VStack(spacing: AeroricSpace.xs) {
                detail("远程平台", offline.platform)
                detail("版本", "\(offline.version)（官方独立包）")
                detail("大小", ByteCountFormatter.string(fromByteCount: Int64(clamping: offline.size), countStyle: .file)
                    + (offline.cached ? " · 本机已缓存" : " · 先下载到本机"))
                detail("装到", offline.destination, mono: true)
            }
            Text("本机校验 sha256 后经这条 SSH 连接传过去，远程再校验一次才放到位；不改远程的 shell 配置。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
        } else {
            StatusCapsule(text: plan.offlineError ?? "暂时拿不到独立安装包", systemImage: "exclamationmark.triangle",
                          tint: AeroricColor.danger)
        }
    }

    private func detail(_ label: String, _ value: String, mono: Bool = false) -> some View {
        HStack {
            Text(label).foregroundStyle(AeroricColor.textSecondary)
            Spacer()
            Text(value).font(mono ? AeroricType.mono(AeroricType.caption) : .system(size: AeroricType.secondary))
                .textSelection(.enabled)
        }
        .font(.system(size: AeroricType.secondary))
    }

    private func commandCapsule(_ command: String) -> some View {
        Text(command)
            .font(AeroricType.mono(AeroricType.caption))
            .textSelection(.enabled)
            .padding(.horizontal, AeroricSpace.l)
            .frame(maxWidth: .infinity, minHeight: 32, alignment: .leading)
            .background(AeroricColor.layer1.opacity(0.85), in: Capsule(style: .continuous))
            .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator2, lineWidth: 1) }
    }
}

/// An offline install's four steps (O6), the current one with its bar.
struct OfflineInstallProgress: View {
    let job: SshClient.InstallJob

    private static let steps = [("download", "本机下载并校验 sha256"), ("upload", "经 SSH 传到远程"),
                                ("verify", "远程校验 sha256 并放到位"), ("check", "运行 --version 确认能启动")]

    var body: some View {
        let current = Self.steps.firstIndex { $0.0 == job.phase } ?? 0
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            if let fraction = job.fraction {
                HStack(spacing: AeroricSpace.m) {
                    ProgressView(value: fraction).progressViewStyle(.linear).tint(AeroricColor.accent)
                    Text(amount(fraction)).monospacedDigit().foregroundStyle(AeroricColor.textSecondary)
                }
                .font(.system(size: AeroricType.caption))
            }
            ForEach(Array(Self.steps.enumerated()), id: \.offset) { index, step in
                Label {
                    Text(step.1)
                } icon: {
                    if index < current {
                        Image(systemName: "checkmark.circle.fill").foregroundStyle(AeroricColor.success)
                    } else if index == current {
                        ProgressView().controlSize(.mini)
                    } else {
                        Image(systemName: "circle").foregroundStyle(AeroricColor.textTertiary)
                    }
                }
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(index <= current ? AeroricColor.text : AeroricColor.textTertiary)
            }
        }
        .padding(AeroricSpace.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(AeroricColor.layer2, in: AeroricRadius.shape(16))
    }

    private func amount(_ fraction: Double) -> String {
        let format = { (bytes: UInt64) in ByteCountFormatter.string(fromByteCount: Int64(clamping: bytes), countStyle: .file) }
        return "\(Int((fraction * 100).rounded()))% · \(format(job.done)) / \(format(job.total))"
    }
}
