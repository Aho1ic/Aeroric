import Foundation

/// New-session scenarios and their recipe cards. Static until the host
/// recipe library (A01 / B2) is wired: a card only fills the input with an
/// editable template. It never sends, never changes model or permission and
/// never sets `recipeID` (Q12, D5). Names follow harness-feature-candidates
/// §4.1; template text is written here.
public enum Scenario: String, CaseIterable, Identifiable, Sendable {
    case build
    case collaborate
    case explore

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .build: "构建"
        case .collaborate: "协作"
        case .explore: "探索"
        }
    }

    public var symbol: String {
        switch self {
        case .build: "hammer"
        case .collaborate: "person.2"
        case .explore: "safari"
        }
    }

    public var recipes: [ScenarioRecipe] { ScenarioCatalog.recipes.filter { $0.scenario == self } }
}

public struct ScenarioRecipe: Identifiable, Equatable, Sendable {
    public let id: String
    public let scenario: Scenario
    public let title: String
    public let detail: String
    public let symbol: String
    /// Filled into the composer. `{…}` marks the part the user should replace.
    public let template: String

    /// Range of the first `{…}` placeholder, so the caret can land on it.
    public var placeholderRange: Range<String.Index>? {
        guard let open = template.firstIndex(of: "{"),
              let close = template[open...].firstIndex(of: "}") else { return nil }
        return open..<template.index(after: close)
    }
}

public enum ScenarioCatalog {
    public static let recipes: [ScenarioRecipe] = [
        // 构建
        .init(id: "brainstorm", scenario: .build, title: "头脑风暴与设计", detail: "先澄清需求，再给出可选方案",
              symbol: "lightbulb",
              template: "我想做：{描述目标}。\n先问我几个关键问题澄清需求，再给出 2–3 个方案并比较取舍，确认前不要写代码。"),
        .init(id: "write-plan", scenario: .build, title: "编写计划", detail: "把需求拆成可验证的步骤",
              symbol: "list.number",
              template: "为下面的需求写一份执行计划：{需求}。\n每一步写清要改的文件、验证命令和回滚点，先给我审。"),
        .init(id: "execute-plan", scenario: .build, title: "按计划执行", detail: "逐步实现并在每步后验证",
              symbol: "play.circle",
              template: "按这份计划逐步实现：{计划位置或内容}。\n每完成一步运行对应验证，失败就停下来说明原因。"),
        .init(id: "tdd", scenario: .build, title: "测试驱动开发", detail: "先写失败的测试，再实现",
              symbol: "checkmark.circle",
              template: "用测试驱动的方式实现：{功能}。\n先写一个会失败的测试并运行确认失败，再写最小实现让它通过，最后重构。"),
        .init(id: "debug", scenario: .build, title: "系统化调试", detail: "复现、定位根因、再修复",
              symbol: "ant",
              template: "帮我排查这个问题：{现象和复现步骤}。\n先稳定复现，再定位根因并说明证据，确认后再做最小修复。"),
        .init(id: "verify", scenario: .build, title: "完成前验证", detail: "跑检查并核对改动是否完整",
              symbol: "checkmark.seal",
              template: "在我提交前检查当前改动：{范围}。\n运行相关构建和测试，逐项核对需求是否满足，列出未验证的地方。"),
        // 协作
        .init(id: "request-review", scenario: .collaborate, title: "请求审查", detail: "整理改动供他人审查",
              symbol: "eye",
              template: "为 {分支或改动范围} 准备一份审查说明：改了什么、为什么、怎么验证、哪里风险最高。"),
        .init(id: "address-review", scenario: .collaborate, title: "处理审查意见", detail: "逐条核实后再改",
              symbol: "text.bubble",
              template: "处理这些审查意见：{意见内容或链接}。\n逐条判断是否成立并说明理由，成立的再修改，不成立的写出回复。"),
        .init(id: "parallel", scenario: .collaborate, title: "并行派发", detail: "拆成互不依赖的子任务",
              symbol: "arrow.triangle.branch",
              template: "把这个任务拆成可以并行的独立子任务：{任务}。\n说明每个子任务的输入、产出和不能互相触碰的文件。"),
        .init(id: "subagents", scenario: .collaborate, title: "子代理驱动开发", detail: "每步交给独立代理并复核",
              symbol: "person.3",
              template: "按计划 {计划} 实现，每个步骤交给一个独立子代理完成，完成后复核它的改动再进入下一步。"),
        .init(id: "code-review", scenario: .collaborate, title: "代码审查", detail: "找出正确性问题与风险",
              symbol: "doc.text.magnifyingglass",
              template: "审查 {文件、目录或分支} 的改动，重点找正确性问题、边界条件和安全风险，按严重程度排序。"),
        .init(id: "release-notes", scenario: .collaborate, title: "发布说明", detail: "从提交整理面向用户的说明",
              symbol: "megaphone",
              template: "根据 {版本范围，如 v1.2..HEAD} 的提交整理发布说明：新功能、修复、破坏性变更分开写。"),
        // 探索
        .init(id: "ask", scenario: .explore, title: "直接提问", detail: "关于这个项目的任何问题",
              symbol: "questionmark.bubble",
              template: "{你的问题}"),
        .init(id: "health-check", scenario: .explore, title: "项目体检", detail: "结构、依赖、技术债一览",
              symbol: "stethoscope",
              template: "给这个项目做一次体检：目录结构、主要依赖、测试覆盖和明显的技术债，最后列出 {数量} 条最值得做的改进。"),
        .init(id: "deps", scenario: .explore, title: "依赖更新", detail: "检查过期依赖与升级风险",
              symbol: "shippingbox",
              template: "检查 {包管理器或目录} 的依赖：哪些过期、升级有什么破坏性变更，给出分批升级建议，先不要改文件。"),
        .init(id: "coverage", scenario: .explore, title: "测试覆盖", detail: "找出缺测试的关键路径",
              symbol: "chart.bar",
              template: "找出 {模块} 里缺少测试的关键路径，按风险排序，并给出每条要补的测试用例。"),
        .init(id: "security", scenario: .explore, title: "安全审计", detail: "检查常见安全隐患",
              symbol: "lock.shield",
              template: "审计 {范围} 的安全隐患：注入、凭据泄露、权限检查、依赖漏洞，说明证据和修复建议。"),
        .init(id: "ci", scenario: .explore, title: "CI 排查", detail: "定位流水线失败原因",
              symbol: "wrench.adjustable",
              template: "CI 失败了：{失败的任务名或日志}。\n找出根因，区分是代码问题、环境问题还是偶发失败，再给出修复。"),
    ]

    /// Merges a template into what the user already typed.
    public enum Insertion: Sendable { case replace, append }

    public static func merged(_ current: String, with recipe: ScenarioRecipe, as insertion: Insertion) -> String {
        switch insertion {
        case .replace:
            return recipe.template
        case .append:
            let trimmed = current.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? recipe.template : trimmed + "\n\n" + recipe.template
        }
    }
}
