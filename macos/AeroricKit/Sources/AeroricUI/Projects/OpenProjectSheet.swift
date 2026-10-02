import SwiftUI
import AppKit

/// P5-03：项目分组编辑器（分组名单的**本地编辑面**）。宿主权威面
/// （ADR-024 分组记录）由调用方提交；这里只管理编辑中的名字列表与
/// 重命名/新增/移除的交互，不做任何推断。
public struct ProjectGroupEditor: View {
    @Binding public var groups: [String]
    public var onSubmit: (([String]) -> Void)?

    @State private var newGroupName = ""
    @State private var renaming: (index: Int, name: String)?

    public init(groups: Binding<[String]>, onSubmit: (([String]) -> Void)? = nil) {
        self._groups = groups
        self.onSubmit = onSubmit
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("项目分组")
                .font(.headline)
            ForEach(Array(groups.enumerated()), id: \.offset) { index, group in
                HStack {
                    if let renaming, renaming.index == index {
                        TextField("分组名", text: Binding(
                            get: { renaming.name },
                            set: { self.renaming?.name = $0 }
                        ))
                        .onSubmit {
                            if !renaming.name.isEmpty {
                                groups[index] = renaming.name
                            }
                            self.renaming = nil
                        }
                    } else {
                        Text(group)
                        Spacer()
                        Button("重命名") { renaming = (index, group) }
                            .accessibilityIdentifier("group.rename.\(index)")
                        Button("移除") { groups.remove(at: index) }
                            .accessibilityIdentifier("group.remove.\(index)")
                    }
                }
            }
            HStack {
                TextField("新分组名", text: $newGroupName)
                    .accessibilityIdentifier("group.new-name")
                Button("添加") {
                    let trimmed = newGroupName.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !trimmed.isEmpty, !groups.contains(trimmed) else { return }
                    groups.append(trimmed)
                    newGroupName = ""
                }
                .accessibilityIdentifier("group.add")
                Spacer()
                Button("保存") { onSubmit?(groups) }
                    .disabled(groups.isEmpty)
                    .accessibilityIdentifier("group.save")
            }
        }
        .padding(12)
        .frame(width: 360)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("project.group-editor")
    }
}

/// P5-03 Action 5：打开项目。**NSOpenPanel 经注入的路径源**（platform UI
/// adapter 的缝），测试注入假路径源；panel 的取消返回 nil，调用方对
/// nil 不做任何事。客户端「选过」的路径**不是**服务端校验的替代——
/// server 校验是 host `project.open` 的职责（打不开就是失败应答）。
public struct OpenProjectSheet {
    public var pickPath: () -> String?

    public init(pickPath: @escaping () -> String?) {
        self.pickPath = pickPath
    }

    /// 生产路径源：NSOpenPanel 目录选择。
    public static func openPanelSource() -> OpenProjectSheet {
        OpenProjectSheet(pickPath: {
            let panel = NSOpenPanel()
            panel.canChooseDirectories = true
            panel.canChooseFiles = false
            panel.allowsMultipleSelection = false
            panel.prompt = "打开"
            panel.message = "选择要打开的项目目录"
            guard panel.runModal() == .OK else { return nil }
            return panel.url?.path
        })
    }

    public func run(onPicked: @escaping (String) -> Void) {
        guard let path = pickPath() else { return }
        onPicked(path)
    }
}
