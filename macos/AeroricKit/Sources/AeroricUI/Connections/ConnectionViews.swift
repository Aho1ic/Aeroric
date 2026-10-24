import SwiftUI

/// P7-01 Action 2：Connections 视图集（六视图）。
public struct ConnectionList: View {
    let store: ConnectionStore

    public init(store: ConnectionStore) {
        self.store = store
    }

    public var body: some View {
        Group {
            if store.connections.isEmpty {
                // AP8: without this the pane is an unexplained black rectangle —
                // measured 2026-10-13 in the real window, where an empty
                // connection list and a failed render looked identical. An empty
                // list is a state the product must *say*, not leave blank.
                VStack(spacing: 8) {
                    Image(systemName: "server.rack")
                        .font(.system(size: 24))
                        .foregroundStyle(Color.dsTextMuted)
                    Text("暂无 SSH 连接")
                        .font(.callout)
                        .foregroundStyle(Color.dsTextSecondary)
                    Text("在下方填写名称、主机与用户名后保存")
                        .font(.caption)
                        .foregroundStyle(Color.dsTextMuted)
                        .multilineTextAlignment(.center)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityIdentifier("connections.list-empty")
            } else {
                List(store.connections, selection: Binding(
                    get: { store.selectedID },
                    set: { store.select($0 ?? "") }
                )) { connection in
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 4) {
                            Text(connection.name)
                                .font(.caption)
                            if connection.hasPassword {
                                Image(systemName: "lock.fill")
                                    .font(.caption2)
                                    .foregroundStyle(Color.dsTextMuted)
                            }
                        }
                        Text("\(connection.username)@\(connection.host):\(connection.port)")
                            .font(.caption2)
                            .foregroundStyle(Color.dsTextMuted)
                    }
                    .accessibilityIdentifier("connections.item.\(connection.id)")
                }
                .accessibilityIdentifier("connections.list")
            }
        }
    }
}

/// SSH 连接编辑器（Action 1：保留 credential owner 与连接配置格式；密码
/// 不预填、不进日志）。
public struct SSHConnectionEditor: View {
    public var onSave: ((_ name: String, _ host: String, _ port: UInt16, _ username: String, _ password: String?) -> Void)?
    @State private var name = ""
    @State private var host = ""
    @State private var port = 22
    @State private var username = ""
    @State private var password = ""
    @State private var error: String?

    public init(
        onSave: ((_ name: String, _ host: String, _ port: UInt16, _ username: String, _ password: String?) -> Void)? = nil
    ) {
        self.onSave = onSave
    }

    public var body: some View {
        Form {
            TextField("名称", text: $name)
            TextField("主机", text: $host)
            Stepper("端口 \(port)", value: $port, in: 1...65535)
            TextField("用户名", text: $username)
            // 密码不预填、不进日志（SecureField；传一次后清内存）。
            SecureField("密码（可选）", text: $password)
                .accessibilityIdentifier("connections.password")
            if let error {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(Color.dsDanger)
            }
            Button("保存") {
                guard !name.trimmingCharacters(in: .whitespaces).isEmpty,
                      !host.trimmingCharacters(in: .whitespaces).isEmpty,
                      !username.trimmingCharacters(in: .whitespaces).isEmpty
                else {
                    error = "名称、主机、用户名不能为空"
                    return
                }
                error = nil
                let passwordCopy = password
                password = ""
                onSave?(name, host, UInt16(port), username, passwordCopy.isEmpty ? nil : passwordCopy)
            }
            .accessibilityIdentifier("connections.save")
        }
        .padding(16)
        .accessibilityIdentifier("connections.editor")
    }
}

/// HostKeyPrompt（首连/主机 key 变化分别提示；拒绝不可自动重试为信任）。
public struct HostKeyPrompt: View {
    public enum Kind: Equatable {
        case firstConnection
        case keyChanged(previousFingerprint: String)

        var isKeyChanged: Bool {
            if case .keyChanged = self { return true }
            return false
        }
    }

    public let kind: Kind
    public let fingerprints: [String]
    public var onTrust: () -> Void
    public var onReject: () -> Void

    public init(
        kind: Kind, fingerprints: [String],
        onTrust: @escaping () -> Void, onReject: @escaping () -> Void
    ) {
        self.kind = kind
        self.fingerprints = fingerprints
        self.onTrust = onTrust
        self.onReject = onReject
    }

    private var title: String {
        switch kind {
        case .firstConnection: "首次连接该主机"
        case .keyChanged: "主机 key 已变化"
        }
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 6) {
                Image(systemName: kind.isKeyChanged ? "exclamationmark.triangle.fill" : "key")
                    .foregroundStyle(kind.isKeyChanged ? Color.dsDanger : Color.dsWarning)
                Text(title)
                    .font(.headline)
                    .accessibilityIdentifier("connections.hostkey.title")
            }
            if case let .keyChanged(previous) = kind {
                Text("之前信任的指纹：\(previous)——如果这不是你预期的主机更换，请拒绝。")
                    .font(.caption)
                    .foregroundStyle(Color.dsTextSecondary)
                    .accessibilityIdentifier("connections.hostkey.previous")
            }
            VStack(alignment: .leading, spacing: 4) {
                ForEach(fingerprints, id: \.self) { fingerprint in
                    Text(fingerprint)
                        .font(.caption2.monospaced())
                        .textSelection(.enabled)
                }
            }
            HStack {
                Button("信任并连接") { onTrust() }
                    .accessibilityIdentifier("connections.hostkey.trust")
                Button("拒绝", role: .destructive) { onReject() }
                    .accessibilityIdentifier("connections.hostkey.reject")
            }
        }
        .padding(16)
        .frame(minWidth: 400)
        .accessibilityIdentifier("connections.hostkey-prompt")
    }
}

/// SFTP 浏览器（远程条目列表；remote cwd 与 local cwd 分开显示）。
public struct SFTPBrowser: View {
    public struct Entry: Identifiable {
        public var id: String { path }
        public var name: String
        public var path: String
        public var isDir: Bool
        public var size: UInt64?

        public init(name: String, path: String, isDir: Bool, size: UInt64? = nil) {
            self.name = name
            self.path = path
            self.isDir = isDir
            self.size = size
        }
    }

    let store: ConnectionStore
    public let entries: [Entry]
    public var onNavigate: ((_ path: String) -> Void)?

    public init(store: ConnectionStore, entries: [Entry], onNavigate: ((_ path: String) -> Void)? = nil) {
        self.store = store
        self.entries = entries
        self.onNavigate = onNavigate
    }

    public var body: some View {
        VStack(spacing: 0) {
            // remote cwd 与 local cwd 分开显示（防止误落本机）。
            if let remoteCWD = store.remoteCWD {
                Text("远程：\(remoteCWD)")
                    .font(.caption)
                    .foregroundStyle(Color.dsAccent)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .accessibilityIdentifier("connections.remote-cwd")
            }
            if entries.isEmpty {
                // Same defect as the connection list: an unlabelled black pane is
                // indistinguishable from a failed render. Measured 2026-10-13 —
                // this was the ~60%-of-window void in the SSH workspace.
                VStack(spacing: 8) {
                    Image(systemName: "folder.badge.questionmark")
                        .font(.system(size: 24))
                        .foregroundStyle(Color.dsTextMuted)
                    Text("没有可浏览的文件")
                        .font(.callout)
                        .foregroundStyle(Color.dsTextSecondary)
                    Text(store.remoteCWD == nil
                         ? "选择或新建一个连接后浏览远程文件"
                         : "此目录为空")
                        .font(.caption)
                        .foregroundStyle(Color.dsTextMuted)
                        .multilineTextAlignment(.center)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityIdentifier("connections.sftp-empty")
            } else {
            List(entries) { entry in
                HStack(spacing: 6) {
                    Image(systemName: entry.isDir ? "folder" : "doc")
                        .font(.caption2)
                        .foregroundStyle(entry.isDir ? Color.dsAccent : Color.dsTextSecondary)
                    Text(entry.name)
                        .font(.caption)
                    Spacer()
                    if let size = entry.size {
                        Text("\(size) B")
                            .font(.caption2)
                            .foregroundStyle(Color.dsTextMuted)
                    }
                }
                .contentShape(Rectangle())
                .onTapGesture {
                    if entry.isDir {
                        onNavigate?(entry.path)
                    }
                }
                .accessibilityIdentifier("connections.sftp.\(entry.path)")
            }
            .accessibilityIdentifier("connections.sftp")
            }
        }
    }
}

/// 传输队列视图（进度/取消/恢复/覆盖确认）。
public struct TransferQueueView: View {
    public struct Item: Identifiable {
        public var id: String
        public var direction: String
        public var name: String
        public var progress: Double
        public var status: String

        public init(id: String, direction: String, name: String, progress: Double, status: String) {
            self.id = id
            self.direction = direction
            self.name = name
            self.progress = progress
            self.status = status
        }
    }

    public let items: [Item]
    public var onCancel: ((_ id: String) -> Void)?
    public var onResume: ((_ id: String) -> Void)?

    public init(
        items: [Item],
        onCancel: ((_ id: String) -> Void)? = nil,
        onResume: ((_ id: String) -> Void)? = nil
    ) {
        self.items = items
        self.onCancel = onCancel
        self.onResume = onResume
    }

    public var body: some View {
        List(items) { item in
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(item.direction == "upload" ? "↑" : "↓")
                        .font(.caption)
                        .foregroundStyle(Color.dsAccent)
                    Text(item.name)
                        .font(.caption)
                    Spacer()
                    Text(item.status)
                        .font(.caption2)
                        .foregroundStyle(Color.dsTextMuted)
                }
                ProgressView(value: item.progress)
                    .accessibilityIdentifier("connections.transfer.progress.\(item.id)")
                HStack {
                    if item.status == "failed" || item.status == "cancelled" {
                        Button("恢复") { onResume?(item.id) }
                            .font(.caption2)
                            .accessibilityIdentifier("connections.transfer.resume.\(item.id)")
                    }
                    if item.status == "inProgress" || item.status == "pending" {
                        Button("取消") { onCancel?(item.id) }
                            .font(.caption2)
                            .accessibilityIdentifier("connections.transfer.cancel.\(item.id)")
                    }
                }
            }
            .accessibilityIdentifier("connections.transfer.\(item.id)")
        }
        .accessibilityIdentifier("connections.transfers")
    }
}

/// 远程工作区横幅（remote cwd 与 local cwd 分开显示，防止误落本机）。
public struct RemoteWorkspaceBanner: View {
    public let remoteCWD: String
    public let localCWD: String

    public init(remoteCWD: String, localCWD: String) {
        self.remoteCWD = remoteCWD
        self.localCWD = localCWD
    }

    public var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "server.rack")
                .font(.caption)
                .foregroundStyle(Color.dsAccent)
            Text("远程工作区：\(remoteCWD)（本机：\(localCWD)）")
                .font(.caption)
            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(Color.dsSurface.opacity(0.55))
        .accessibilityIdentifier("connections.remote-banner")
    }
}
