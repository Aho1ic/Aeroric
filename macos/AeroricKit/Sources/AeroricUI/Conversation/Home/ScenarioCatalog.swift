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
    /// B2 (2026-10-30): the K-Dense research skills.
    case science

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .build: "构建"
        case .collaborate: "协作"
        case .explore: "探索"
        case .science: "科研"
        }
    }

    public var symbol: String {
        switch self {
        case .build: "hammer"
        case .collaborate: "person.2"
        case .explore: "safari"
        case .science: "flask"
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
    /// The built-in skill behind the card (B2), called before the template.
    public var skill: String? = nil

    /// Range of the first `{…}` placeholder, so the caret can land on it.
    public var placeholderRange: Range<String.Index>? {
        guard let open = template.firstIndex(of: "{"),
              let close = template[open...].firstIndex(of: "}") else { return nil }
        return open..<template.index(after: close)
    }
}

public enum ScenarioCatalog {
    /// Cards that call a built-in skill (B2, 2026-10-30).
    static let skills: [String: String] = [
        "brainstorm": "brainstorming", "write-plan": "writing-plans", "execute-plan": "executing-plans",
        "tdd": "test-driven-development", "debug": "systematic-debugging",
        "verify": "verification-before-completion", "request-review": "requesting-code-review",
        "address-review": "receiving-code-review", "parallel": "dispatching-parallel-agents",
        "subagents": "subagent-driven-development",
    ]

    public static let recipes: [ScenarioRecipe] = (cards + science).map { recipe in
        var recipe = recipe
        if recipe.skill == nil { recipe.skill = skills[recipe.id] }
        return recipe
    }

    /// The 科研 group: one card per research skill.
    static let science: [ScenarioRecipe] = [
        .init(id: "sci-brainstorm", scenario: .science, title: "科研头脑风暴", detail: "开放式发散研究思路",
              symbol: "brain", template: "我的研究方向是：{方向}。\n帮我发散出几个值得做的问题，并说明各自的价值和可行性。",
              skill: "scientific-brainstorming"),
        .init(id: "hypothesis", scenario: .science, title: "假设生成", detail: "从观察出发提出可检验的假设",
              symbol: "questionmark.circle", template: "我观察到：{现象或数据}。\n提出几个可检验的假设，并说明怎么检验。",
              skill: "hypothesis-generation"),
        .init(id: "experiment", scenario: .science, title: "实验设计", detail: "收数据前定设计、变量与对照",
              symbol: "flask", template: "我想研究：{研究问题}。\n帮我设计实验：自变量、因变量、对照、样本量和分析方法。",
              skill: "experimental-design"),
        .init(id: "power", scenario: .science, title: "统计功效", detail: "估算需要多大的样本量",
              symbol: "chart.bar.xaxis", template: "我的设计是：{设计与预期效应}。\n算一下需要多少样本才有足够的统计功效。",
              skill: "statistical-power"),
        .init(id: "stats", scenario: .science, title: "统计分析", detail: "选检验、查假设、解读结果",
              symbol: "function", template: "数据在：{文件或描述}。\n帮我选合适的检验，检查前提假设，并解读结果。",
              skill: "statistical-analysis"),
        .init(id: "eda", scenario: .science, title: "探索性数据分析", detail: "先摸清数据的分布与问题",
              symbol: "magnifyingglass", template: "对 {数据文件} 做一次探索性分析：分布、缺失、异常和变量关系。",
              skill: "exploratory-data-analysis"),
        .init(id: "sci-viz", scenario: .science, title: "科学可视化", detail: "做出能投稿的图",
              symbol: "chart.xyaxis.line", template: "把 {数据或结果} 画成能投稿的图，说明为什么这样画。",
              skill: "scientific-visualization"),
        .init(id: "critical", scenario: .science, title: "批判性思维", detail: "评估论断与证据质量",
              symbol: "scalemass", template: "评估这个论断和它的证据：{论断或论文}。指出漏洞和替代解释。",
              skill: "scientific-critical-thinking"),
        .init(id: "papers", scenario: .science, title: "论文检索", detail: "在学术数据库里查论文与引用",
              symbol: "books.vertical", template: "帮我找关于 {主题} 的重要论文，按相关性列出并概括要点。",
              skill: "paper-lookup"),
        .init(id: "peer-review", scenario: .science, title: "同行评审", detail: "按清单评审稿件或基金申请",
              symbol: "doc.text.magnifyingglass", template: "按同行评审的标准审阅 {稿件或申请书}，给出分项意见。",
              skill: "peer-review"),
        .init(id: "citations", scenario: .science, title: "引用管理", detail: "检索、整理与核对参考文献",
              symbol: "quote.opening", template: "整理并核对 {文档} 里的参考文献，补全缺失信息。",
              skill: "citation-management"),
        .init(id: "scholar-eval", scenario: .science, title: "学术评估", detail: "按 ScholarEval 框架评估成果",
              symbol: "graduationcap", template: "用 ScholarEval 框架评估 {论文或成果}。",
              skill: "scholar-evaluation"),
        .init(id: "schematics", scenario: .science, title: "科学示意图", detail: "生成论文用示意图（需要图像模型的 key）",
              symbol: "square.on.circle", template: "为 {要表达的机制或流程} 画一张论文示意图。",
              skill: "scientific-schematics"),
    ]

    /// A card for a skill picked in the 工作配方 panel: the matching card when
    /// there is one, else a plain one asking what to do.
    public static func recipe(forSkill item: BuiltinSkillBook.Item) -> ScenarioRecipe {
        if let card = recipes.first(where: { $0.skill == item.id }) { return card }
        return ScenarioRecipe(id: "skill.\(item.id)", scenario: item.group == "science" ? .science : .build,
                              title: item.title, detail: item.detail, symbol: item.symbol,
                              template: "{描述你要做的事}", skill: item.id)
    }

    static let cards: [ScenarioRecipe] = [
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

    public static func merged(_ current: String, with recipe: ScenarioRecipe, as insertion: Insertion,
                              invocation: String? = nil) -> String {
        let template = invocation.map { "\($0) \(recipe.template)" } ?? recipe.template
        switch insertion {
        case .replace:
            return template
        case .append:
            let trimmed = current.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? template : trimmed + "\n\n" + template
        }
    }
}
