import AeroricClient
import AppKit
import SwiftUI

/// 2026-11-24 设置 › 存储: one bar for what Aeroric's data takes, split by
/// kind, then each kind with its size and 在访达中显示 for its folders.
/// Read only; measuring walks the folders, so it waits for the page.
struct StorageSettingsPane: View {
    let load: @MainActor () async throws -> StorageUsage
    @State private var usage: StorageUsage?
    @State private var failure: String?
    @State private var measuring = false

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xxl) {
            SettingsCard("占用") {
                SettingsRow("数据目录", detail: summary) {
                    HStack(spacing: AeroricSpace.s) {
                        if let usage {
                            Button("在访达中显示") { Self.reveal(usage.dataDir) }
                                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                                .accessibilityIdentifier("settings.storage.reveal-data")
                        }
                        Button(measuring ? "正在计算…" : "重新计算") { Task { await measure() } }
                            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                            .disabled(measuring)
                            .accessibilityIdentifier("settings.storage.measure")
                    }
                }
                if let usage, usage.totalBytes > 0 {
                    StorageBar(usage: usage)
                        .padding(.vertical, AeroricSpace.m)
                        .accessibilityIdentifier("settings.storage.bar")
                }
            }
            if let usage {
                ForEach(usage.groups.filter { !$0.entries.isEmpty }) { group in
                    SettingsCard(group.title) {
                        ForEach(group.entries.prefix(StorageSettingsPane.entriesShown)) { entry in
                            SettingsRow(Self.name(of: entry.path, in: usage.dataDir),
                                        detail: Self.size(entry.bytes)) {
                                Button("在访达中显示") { Self.reveal(entry.path) }
                                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                            }
                        }
                        if group.entries.count > StorageSettingsPane.entriesShown {
                            Text("还有 \(group.entries.count - StorageSettingsPane.entriesShown) 项较小的没列出")
                                .font(.caption)
                                .foregroundStyle(AeroricColor.textSecondary)
                                .padding(.vertical, AeroricSpace.s)
                        }
                    }
                    .accessibilityIdentifier("settings.storage.group.\(group.id)")
                }
            }
        }
        .task { if usage == nil { await measure() } }
    }

    static let entriesShown = 8

    private var summary: String {
        if let failure { return "没算出来：\(failure)" }
        guard let usage else { return measuring ? "正在计算各目录的大小…" : "还没计算" }
        var text = "共 \(Self.size(usage.totalBytes))"
        if usage.unreadable > 0 { text += "，另有 \(usage.unreadable) 项读不了，没算进去" }
        return text
    }

    private func measure() async {
        measuring = true
        defer { measuring = false }
        do {
            usage = try await load()
            failure = nil
        } catch {
            failure = error.localizedDescription
        }
    }

    /// 1.2 GB, 330 MB: the byte counts Finder shows (base 1000).
    static func size(_ bytes: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
    }

    /// A path under the data folder by its relative name; others in full,
    /// with the home folder as ~.
    static func name(of path: String, in dataDir: String) -> String {
        let prefix = dataDir.hasSuffix("/") ? dataDir : dataDir + "/"
        if path.hasPrefix(prefix) { return String(path.dropFirst(prefix.count)) }
        let home = NSHomeDirectory()
        if path.hasPrefix(home + "/") { return "~" + path.dropFirst(home.count) }
        return path
    }

    static func reveal(_ path: String) {
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
    }
}

/// The bar and its legend: one colour per kind, widths by share of the total.
private struct StorageBar: View {
    let usage: StorageUsage

    private var shown: [StorageUsage.Group] { usage.groups.filter { $0.bytes > 0 } }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            GeometryReader { geometry in
                HStack(spacing: 1) {
                    ForEach(shown) { group in
                        Rectangle()
                            .fill(Self.color(group.id))
                            .frame(width: max(2, geometry.size.width * share(group)))
                    }
                }
                .frame(width: geometry.size.width, alignment: .leading)
            }
            .frame(height: 10)
            .clipShape(Capsule())
            FlowLegend(items: shown.map { ($0.title, Self.color($0.id), StorageSettingsPane.size($0.bytes)) })
        }
    }

    private func share(_ group: StorageUsage.Group) -> CGFloat {
        usage.totalBytes > 0 ? CGFloat(group.bytes) / CGFloat(usage.totalBytes) : 0
    }

    static func color(_ id: String) -> Color {
        switch id {
        case "sessions": AeroricColor.accent
        case "memory": AeroricColor.violet
        case "references": AeroricColor.teal
        case "worktrees": AeroricColor.warning
        case "logs": AeroricColor.running
        case "remotePackages": AeroricColor.success
        default: AeroricColor.textTertiary
        }
    }
}

/// Legend items, wrapped onto as many lines as the width needs.
private struct FlowLegend: View {
    let items: [(String, Color, String)]

    var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 180), alignment: .leading)],
                  alignment: .leading, spacing: AeroricSpace.s) {
            ForEach(items.indices, id: \.self) { index in
                let (title, color, size) = items[index]
                HStack(spacing: AeroricSpace.s) {
                    Circle().fill(color).frame(width: 8, height: 8)
                    Text(title).foregroundStyle(AeroricColor.text)
                    Text(size).foregroundStyle(AeroricColor.textSecondary)
                }
                .font(.caption)
            }
        }
    }
}
