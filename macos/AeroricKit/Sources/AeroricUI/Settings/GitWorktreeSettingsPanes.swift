import AeroricClient
import AppKit
import SwiftUI

/// 2026-11-24 设置 › Git: 提交说明的附加指引, added after the project's own
/// commit prompt when Aeroric writes a commit message (Git panel → 生成).
/// No branch prefix: isolated copies are detached worktrees and make no
/// branch, so there is nothing for a prefix to name.
struct GitSettingsPane: View {
    let client: GitSettingsClient?
    @State private var settings: GitSettings?
    @State private var guidance = ""
    @State private var note: String?
    @State private var saving = false

    var body: some View {
        SettingsCard("提交说明") {
            SettingsRow("提交说明的附加指引",
                        detail: note ?? "在 Git 面板点“生成”写提交说明时，在项目自己的提示后面加上这些要求。例如：用中文写；第一行不超过 50 个字。") {
                Button(saving ? "正在保存…" : "保存") { Task { await save() } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .disabled(client == nil || settings == nil || saving
                              || guidance.trimmingCharacters(in: .whitespacesAndNewlines)
                                 == settings?.commitGuidance)
                    .accessibilityIdentifier("settings.git.guidance-save")
            }
            TextEditor(text: $guidance)
                .font(.body)
                .frame(minHeight: 80, maxHeight: 160)
                .scrollContentBackground(.hidden)
                .padding(AeroricSpace.s)
                .background(RoundedRectangle(cornerRadius: 6).stroke(AeroricColor.separator2))
                .disabled(client == nil || settings == nil)
                .padding(.bottom, AeroricSpace.m)
                .accessibilityIdentifier("settings.git.guidance")
        }
        .task { await load() }
    }

    private func load() async {
        guard let client else {
            note = "未连接宿主"
            return
        }
        do {
            let loaded = try await client.settings()
            settings = loaded
            guidance = loaded.commitGuidance
        } catch {
            note = "没读到：\(error.localizedDescription)"
        }
    }

    private func save() async {
        guard let client, var next = settings else { return }
        saving = true
        defer { saving = false }
        next.commitGuidance = guidance
        do {
            let saved = try await client.save(next)
            settings = saved
            guidance = saved.commitGuidance
            note = saved.commitGuidance.isEmpty ? "已保存：不加附加指引" : "已保存"
        } catch {
            note = "没保存：\(error.localizedDescription)"
        }
    }
}

/// 2026-11-24 设置 › 工作副本: where isolated copies live (fixed), how long
/// an emptied one is kept, and the copies there are now.
struct WorktreeSettingsPane: View {
    let client: GitSettingsClient?
    @State private var settings: GitSettings?
    @State private var copies: [IsolatedCopy] = []
    @State private var note: String?

    static let keepChoices: [(Int, String)] = [(7, "7 天"), (30, "30 天"), (90, "90 天"), (365, "一年"), (0, "不自动删")]

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xxl) {
            SettingsCard("位置与清理") {
                SettingsRow("工作副本根目录",
                            detail: "独立副本放在各项目自己的 .aeroric/worktrees 下，不能改：带回改动和删除副本前的安全检查认这个位置。") {
                    Text("<项目>/.aeroric/worktrees")
                        .font(.caption.monospaced())
                        .foregroundStyle(AeroricColor.textSecondary)
                }
                SettingsRow("没有改动的副本保留多久",
                            detail: note ?? "副本里的改动都带回或放弃以后，过这么久自动删掉（宿主启动时清理）。还有改动没带回的副本从不自动删。") {
                    Picker("", selection: Binding(
                        get: { settings?.clearedCopyKeepDays ?? 30 },
                        set: { days in Task { await setKeep(days) } }
                    )) {
                        ForEach(Self.keepChoices, id: \.0) { days, title in Text(title).tag(days) }
                    }
                    .labelsHidden()
                    .frame(width: 140)
                    .disabled(client == nil || settings == nil)
                    .accessibilityIdentifier("settings.worktrees.keep")
                }
            }
            SettingsCard("现有的独立副本") {
                if copies.isEmpty {
                    Text(client == nil ? "未连接宿主" : "现在没有独立副本")
                        .font(.caption)
                        .foregroundStyle(AeroricColor.textSecondary)
                        .padding(.vertical, AeroricSpace.m)
                }
                ForEach(copies) { copy in
                    SettingsRow(copy.title ?? copy.taskID, detail: Self.detail(copy, keepDays: settings?.clearedCopyKeepDays)) {
                        Button("在访达中显示") {
                            NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: copy.path)])
                        }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    }
                }
            }
            .accessibilityIdentifier("settings.worktrees.copies")
        }
        .task { await load() }
    }

    private func load() async {
        guard let client else { return }
        do {
            settings = try await client.settings()
            copies = try await client.copies()
        } catch {
            note = "没读到：\(error.localizedDescription)"
        }
    }

    private func setKeep(_ days: Int) async {
        guard let client, var next = settings, next.clearedCopyKeepDays != days else { return }
        next.clearedCopyKeepDays = days
        do {
            settings = try await client.save(next)
            note = nil
        } catch {
            note = "没保存：\(error.localizedDescription)"
        }
    }

    /// "项目 · 创建于 11月24日 · 还有改动没带回" or "… · 已清空，约 N 天后删除".
    static func detail(_ copy: IsolatedCopy, keepDays: Int?, now: Date = Date()) -> String {
        let project = URL(fileURLWithPath: copy.origin).lastPathComponent
        let created = Date(timeIntervalSince1970: TimeInterval(copy.createdMs) / 1000)
            .formatted(.dateTime.month().day())
        var parts = [project, "创建于 \(created)"]
        if let cleared = copy.clearedMs {
            if let keepDays, keepDays > 0 {
                let due = TimeInterval(cleared) / 1000 + TimeInterval(keepDays) * 86_400
                let days = max(0, Int(ceil((due - now.timeIntervalSince1970) / 86_400)))
                parts.append(days == 0 ? "已清空，下次宿主启动时删除" : "已清空，约 \(days) 天后删除")
            } else {
                parts.append("已清空，不自动删")
            }
        } else {
            parts.append("还有改动没带回")
        }
        return parts.joined(separator: " · ")
    }
}
