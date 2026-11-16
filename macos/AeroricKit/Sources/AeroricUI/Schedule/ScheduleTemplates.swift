import AeroricProtocol
import Foundation

/// 2026-11-14 定时任务第二批: 常用模板. Picking one only fills the new rule's
/// name, prompt, times and pre-check; everything stays editable. The list
/// follows backlog A12's routine code work (codeg's automation templates).
public struct ScheduleTemplate: Identifiable, Equatable, Sendable {
    public let id: String
    public let name: String
    public let systemImage: String
    public let repeatRule: LiveScheduleRepeat
    public let prompt: String
    public let check: String?

    public static let all: [ScheduleTemplate] = [
        ScheduleTemplate(
            id: "review", name: "审查新提交", systemImage: "eye", repeatRule: .daily(minute: 9 * 60),
            prompt: "审查过去 24 小时的新提交（前置检查的输出里有提交列表）：找出可能的 bug、遗漏的测试和前后不一致的地方，按严重程度列出，给出文件和行号。只读，不要改代码。",
            check: "git log --since='24 hours ago' --oneline | grep ."),
        ScheduleTemplate(
            id: "deps", name: "检查依赖更新", systemImage: "shippingbox",
            repeatRule: .weekly(weekdays: [1], minute: 10 * 60),
            prompt: "检查这个项目的依赖有没有可用的更新和已知的安全公告：列出值得升级的包、升级的风险和需要跟着改的地方。只给建议，不要改依赖文件。",
            check: nil),
        ScheduleTemplate(
            id: "tests", name: "补测试覆盖", systemImage: "checkmark.seal",
            repeatRule: .weekly(weekdays: [3], minute: 14 * 60),
            prompt: "找出最近改动过、但测试覆盖薄弱的模块，为其中最重要的一处补上测试，跑通后说明补了什么、还缺什么。",
            check: nil),
        ScheduleTemplate(
            id: "todo", name: "清理 TODO", systemImage: "checklist",
            repeatRule: .weekly(weekdays: [5], minute: 16 * 60),
            prompt: "看前置检查列出的 TODO 和 FIXME，挑出已经过时或可以顺手解决的，逐条给出处理建议；能安全解决的小项直接改掉，并说明改了什么。",
            check: "git grep -nE 'TODO|FIXME' | head -50 | grep ."),
        ScheduleTemplate(
            id: "ci", name: "排查 CI 失败", systemImage: "wrench.and.screwdriver",
            repeatRule: .every(hours: 2, fromMs: 0),
            prompt: "最近一次 CI 失败了。用 gh 查看失败的任务和日志，找出原因；能修就修好并说明，修不了就写清楚原因和建议。",
            check: "gh run list --limit 1 --json conclusion -q '.[0].conclusion' | grep -qx failure"),
        ScheduleTemplate(
            id: "release", name: "写发布说明", systemImage: "doc.text",
            repeatRule: .weekly(weekdays: [5], minute: 17 * 60),
            prompt: "根据过去 7 天的提交（前置检查的输出），写一份面向用户的发布说明：新功能、修复、注意事项。用中文，不要列内部重构。只写在回复里，不要改文件。",
            check: "git log --since='7 days ago' --oneline | grep ."),
        ScheduleTemplate(
            id: "security", name: "安全审计", systemImage: "lock.shield",
            repeatRule: .weekly(weekdays: [1], minute: 11 * 60),
            prompt: "做一次安全审计：查找硬编码的密钥、危险的命令拼接、未校验的输入和过宽的权限，按风险高低列出，给出文件和行号。只读，不要改代码，回复里也不要写出任何密钥的值。",
            check: nil),
        ScheduleTemplate(
            id: "health", name: "项目体检", systemImage: "stethoscope",
            repeatRule: .weekly(weekdays: [1], minute: 9 * 60 + 30),
            prompt: "给项目做一次体检：构建和测试能不能通过、明显的技术债、过大的文件和函数、过时的文档，列出最值得处理的 5 件事和理由。只读，不要改代码。",
            check: nil),
    ]
}
