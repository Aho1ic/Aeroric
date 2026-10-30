import SwiftUI

/// Agent 配置 → 技能 (B2, 2026-10-30, Q10): every built-in skill with its
/// switch and status, where it came from, and the projects holding copies.
public struct BuiltinSkillsPane: View {
    let book: BuiltinSkillBook
    @State private var group = "develop"

    public init(book: BuiltinSkillBook) { self.book = book }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AeroricSpace.l) {
                HStack {
                    CapsuleSlider(selection: $group, groupTitle: "技能分组", segments: [
                        .init(value: "develop", title: "开发 \(count("develop"))", systemImage: "hammer"),
                        .init(value: "science", title: "科研 \(count("science"))", systemImage: "flask"),
                    ])
                    .fixedSize()
                    Spacer()
                }
                Text("内置技能随 App 提供，四个 Harness 的新会话都能用。你自己装的同名技能优先。")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                VStack(spacing: 0) {
                    ForEach(book.items.filter { $0.group == group }) { item in
                        row(item)
                        if item.id != book.items.last(where: { $0.group == group })?.id { HairlineDivider() }
                    }
                }
                .padding(.horizontal, AeroricSpace.l)
                .background(AeroricColor.layer1, in: AeroricRadius.shape(AeroricRadius.l))
                if !book.projects.isEmpty { projects }
                Text(group == "develop"
                     ? "来源：obra/superpowers v6.3.0（MIT），经 codeg 收录。中文名称和说明由 Aeroric 撰写。"
                     : "来源：K-Dense-AI/scientific-agent-skills（MIT）。中文名称和说明由 Aeroric 撰写。")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            .frame(maxWidth: 760, alignment: .leading)
            .padding(.horizontal, AeroricSpace.huge)
            .padding(.vertical, AeroricSpace.xxxl)
            .frame(maxWidth: .infinity)
        }
        .scrollIndicators(.hidden)
        .task { await book.refresh() }
        .accessibilityIdentifier("agent-config.skills")
    }

    private func count(_ group: String) -> Int { book.items.filter { $0.group == group }.count }

    private func row(_ item: BuiltinSkillBook.Item) -> some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: item.symbol)
                .foregroundStyle(AeroricColor.accent)
                .frame(width: 20)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: AeroricSpace.s) {
                    Text(item.title).font(.system(size: AeroricType.secondary, weight: .medium))
                    Text(item.id).font(.system(size: AeroricType.caption).monospaced())
                        .foregroundStyle(AeroricColor.textTertiary)
                }
                .lineLimit(1)
                Text(BuiltinSkillBook.status(item) ?? item.detail)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(BuiltinSkillBook.status(item) == nil ? AeroricColor.textSecondary : AeroricColor.warning)
                    .lineLimit(1)
            }
            Spacer(minLength: AeroricSpace.m)
            Toggle("", isOn: Binding(get: { item.enabled }, set: { on in Task { await book.setEnabled(item.id, on) } }))
                .labelsHidden()
                .toggleStyle(.switch)
                .controlSize(.mini)
                .accessibilityLabel("启用 \(item.title)")
        }
        .frame(height: 50)
        .help(item.detail)
    }

    private var projects: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Text("项目里的副本").font(.system(size: AeroricType.secondary, weight: .semibold))
            Text("Codex 直连会话要把技能放进项目的 .agents/skills 才能用；这些副本已排除在 Git 之外。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            ForEach(book.projects) { project in
                HStack {
                    Text(project.name).lineLimit(1)
                    Text("\(project.count) 个").foregroundStyle(AeroricColor.textTertiary).monospacedDigit()
                    Spacer()
                    Button("从这个项目移除") { Task { _ = await book.removeFromProject(project.path) } }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .help("移到废纸篓，并清掉 Git 排除规则")
                }
                .font(.system(size: AeroricType.secondary))
                .padding(.horizontal, AeroricSpace.l)
                .frame(height: 40)
                .background(AeroricColor.layer1, in: Capsule(style: .continuous))
            }
        }
    }
}

/// 工作配方 panel from the composer's + menu (Q5): every built-in skill as a
/// card, by group; a card fills the input like a recipe chip does.
struct BuiltinRecipePanel: View {
    let book: BuiltinSkillBook
    let onChoose: (ScenarioRecipe) -> Void
    @State private var group = "develop"

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack {
                Text("工作配方").font(.system(size: AeroricType.secondary, weight: .semibold))
                Spacer()
                CapsuleSlider(selection: $group, groupTitle: "配方分组", segments: [
                    .init(value: "develop", title: "开发", systemImage: "hammer"),
                    .init(value: "science", title: "科研", systemImage: "flask"),
                ])
                .fixedSize()
            }
            ScrollView {
                LazyVGrid(columns: [GridItem(.flexible(), spacing: AeroricSpace.s), GridItem(.flexible())],
                          spacing: AeroricSpace.s) {
                    ForEach(book.items.filter { $0.group == group && $0.enabled }) { item in
                        card(item)
                    }
                }
            }
            .scrollIndicators(.hidden)
            .frame(height: 300)
        }
        .padding(AeroricSpace.l)
        .frame(width: 460)
    }

    private func card(_ item: BuiltinSkillBook.Item) -> some View {
        Button { onChoose(ScenarioCatalog.recipe(forSkill: item)) } label: {
            VStack(alignment: .leading, spacing: 3) {
                HStack {
                    Image(systemName: item.symbol).foregroundStyle(AeroricColor.accent)
                    Spacer()
                    if item.missing.contains("python") {
                        Text("需要 Python").font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.warning)
                    }
                }
                Text(item.title).font(.system(size: AeroricType.secondary, weight: .medium)).lineLimit(1)
                Text(item.detail).font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary).lineLimit(1)
            }
            .padding(AeroricSpace.m)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(AeroricColor.layer1, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            .contentShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        }
        .buttonStyle(.plain)
        .help(item.missing.contains("python") ? "这个技能要跑 Python 脚本，本机没找到 python3" : item.detail)
        .accessibilityIdentifier("recipes.skill.\(item.id)")
    }
}
