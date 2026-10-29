import AppKit
import SwiftUI

/// 外观 › 字体 (2026-10-29): which font draws Chinese in conversation
/// messages. Mono (更纱黑体) and Maple Mono download inside the app — size
/// shown first, then progress — and are used as soon as they are in; 自定义
/// takes any installed family, typed in a capsule field or picked from the
/// families that draw Chinese.
struct ConversationFontSettingsRow: View {
    @AppStorage(ConversationCJKFont.storageKey) private var choiceRaw = ConversationCJKFont.pingFang.rawValue
    @AppStorage(ConversationCJKFont.customKey) private var custom = ""
    @State private var families: [String] = []
    private let downloads = FontDownloads.shared

    private var choice: ConversationCJKFont { ConversationCJKFont(rawValue: choiceRaw) ?? .pingFang }
    private var family: String? {
        _ = downloads.revision // re-resolve once a download registers its fonts
        return ConversationCJKFont.resolve(choice, custom: custom)
    }

    var body: some View {
        SettingsRow("会话中文字体", detail: "会话消息里的中文。英文与代码不变；字体不可用时按苹方显示。") {
            VStack(alignment: .trailing, spacing: AeroricSpace.m) {
                CapsuleSlider(selection: $choiceRaw, groupTitle: "会话中文字体",
                              identifier: "settings.appearance.cjk-font",
                              segments: ConversationCJKFont.allCases.map { .init(value: $0.rawValue, title: $0.title) })
                    .fixedSize()
                if choice == .custom { customField }
                status
                Text("预览：今天星期四，部署配置已完成。Deploy done.")
                    .font(Font(ConversationCJKFont.font(size: AeroricType.secondary, weight: .regular, family: family)))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(1)
                    .fixedSize()
                    .accessibilityIdentifier("settings.appearance.cjk-font.preview")
            }
        }
    }

    private var customField: some View {
        HStack(spacing: AeroricSpace.s) {
            CapsuleField(placeholder: "字体名，例如 LXGW WenKai", text: $custom, height: 28)
                .frame(width: 210)
                .accessibilityIdentifier("settings.appearance.cjk-font.custom")
            Menu("已安装") {
                ForEach(families, id: \.self) { name in
                    Button(name) { custom = name }
                }
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .accessibilityIdentifier("settings.appearance.cjk-font.installed")
        }
        .task { if families.isEmpty { families = ConversationCJKFont.installedChineseFamilies() } }
    }

    @ViewBuilder private var status: some View {
        switch choice {
        case .pingFang:
            EmptyView()
        case .custom:
            let name = custom.trimmingCharacters(in: .whitespacesAndNewlines)
            if !name.isEmpty {
                label(ok: family != nil, text: family != nil ? "本机已安装" : "本机没有这个字体，按苹方显示")
            }
        case .mono, .maple:
            if let family {
                label(ok: true, text: "已启用：\(family)")
            } else if let package = FontPackage.of(choice) {
                download(package)
            }
        }
    }

    @ViewBuilder private func download(_ package: FontPackage) -> some View {
        switch downloads.phase(package.choice) {
        case .idle:
            HStack(spacing: AeroricSpace.s) {
                Text("未下载 · 下载\(FontPackage.megabytes(package.downloadBytes))，占用\(FontPackage.megabytes(package.installedBytes))")
                    .font(.system(size: AeroricType.caption))
                    .fixedSize()
                    .foregroundStyle(AeroricColor.textTertiary)
                Button("下载并启用") { downloads.start(package.choice) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    // Never truncated to "下…" by the size text beside it.
                    .fixedSize()
                    .accessibilityIdentifier("settings.appearance.cjk-font.download")
            }
            .fixedSize()
        case let .downloading(fraction):
            // 2026-10-29: 下载中 | bar | percent; the percent has the width
            // of "100%" so going from 9% to 10% never reflows the row.
            HStack(spacing: AeroricSpace.s) {
                Text("下载中")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .fixedSize()
                ProgressView(value: fraction).frame(width: 140)
                Text("\(Int((fraction * 100).rounded()))%")
                    .font(.system(size: AeroricType.caption))
                    .monospacedDigit()
                    .foregroundStyle(AeroricColor.textSecondary)
                    .frame(width: 34, alignment: .trailing)
                    .fixedSize()
                Button("取消") { downloads.cancel(package.choice) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
            }
        case .installing:
            HStack(spacing: AeroricSpace.s) {
                ProgressView().controlSize(.small)
                Text("正在安装…").font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textSecondary)
            }
        case let .failed(message):
            HStack(spacing: AeroricSpace.s) {
                label(ok: false, text: message)
                Button("重试") { downloads.start(package.choice) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
            }
        }
    }

    private func label(ok: Bool, text: String) -> some View {
        Label(text, systemImage: ok ? "checkmark" : "exclamationmark.circle")
            .font(.system(size: AeroricType.caption))
            .foregroundStyle(ok ? AeroricColor.success : AeroricColor.warning)
    }
}
