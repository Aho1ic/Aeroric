#if DEBUG
import Foundation

/// Fictional embedded text only. No paths, URLs, downloads, commands or real skill files.
enum SkillReviewFixtures {
    static let noConsumption = "无真实加载/消费证据；选用不等于模型消费或操作成功。"

    static func version(_ id: SkillReviewItemID, updated: Bool = false) -> SkillReviewVersion {
        let title: String
        let purpose: String
        let kind: SkillReviewWorkKind
        switch id {
        case .boundary:
            title = "边界差异三步复核"; purpose = "核对虚构接口边界与差异，不修改真实项目。"; kind = .api
        case .keyboard:
            title = "键盘路径复核"; purpose = "核对虚构界面的焦点顺序、取消与长文阅读。"; kind = .keyboard
        case .dependency:
            title = "附图核对"; purpose = "展示缺依赖/不支持的阻塞，不安装依赖。"; kind = .api
        case .unknown:
            title = "摘要交叉复核"; purpose = "展示尚未核实的可用性。"; kind = .keyboard
        case .unrelated:
            title = "归档标签整理"; purpose = "仅用于虚构归档需求，与当前两项工作无关。"; kind = .archive
        case .candidate:
            title = "新增边界说明"; purpose = "尚未答复的新技能申请，不参与按需选用。"; kind = .api
        case .missingSource:
            title = "无来源候选"; purpose = "展示没有来源时不可假装可用。"; kind = .api
        }
        let content = (1...24).map { index in
            "第\(index)段（完全虚构）：\(title)先描述观察对象，再分开列出适用前提、差异与可核对结果。" +
            "这是一段供阅读和滚动的内嵌审阅摘录，不是对 Agent 的指令，不授予任何操作权限。"
        }.joined(separator: "\n\n")
        let source = SkillReviewSource(
            title: updated ? "虚构复核摘录 B · 新版本来源（示例）" : "虚构复核摘录 A（示例）",
            excerpt: "虚构记录：观察到方法有助于人工核对，但尚未成为真实技能。\n\n" + content,
            limitation: "仅供离线样例审阅；未读取真实项目、历史或来源文件。知识摘录不自动升级为指令。"
        )
        let records: [SkillReviewExecutor: SkillReviewAvailability] = Dictionary(
            uniqueKeysWithValues: SkillReviewExecutor.allCases.map { executor in
                let state: SkillReviewAvailabilityState
                let reason: String
                switch id {
                case .dependency:
                    state = .blocked
                    reason = executor == .dsh
                        ? "预置记录：该示例能力不支持；不是真实 dsh 兼容性结论。"
                        : "预置记录：缺少虚构的图示核对依赖；不下载、不提权。"
                case .unknown, .missingSource:
                    state = .unknown
                    reason = "预置记录：依赖/适配依据尚未核实，未知不冒充可用。"
                default:
                    state = .available
                    reason = "仅预设情景可用；没有探测本机、执行者、依赖或模型消费。"
                }
                return (executor, SkillReviewAvailability(state: state, reason: reason))
            }
        )
        return SkillReviewVersion(
            itemID: id, title: title, number: updated ? "1.1" : "1.0",
            content: updated
                ? "v1.1 新增（示例）：先单独核对边界，再列出新增核对条件。\n\n" + content
                : content,
            source: id == .missingSource ? nil : source,
            conditions: updated
                ? "新增前提（示例）：先明确当前工作包边界及人工复核记录。"
                : "前提（示例）：用户已界定工作包；只在当前虚构项目内审阅。",
            validation: updated
                ? "新增验证要求（示例）：逐项说明新旧差异；仍不替代真实产物验收。"
                : "验证要求（示例）：逐条人工核对来源、前提和结果；不采信 Agent 自报。",
            intendedUse: purpose,
            capabilities: updated ? ["新增：边界交叉核对（示例）", "不改变权限/审批/验收"] : ["人工复核方法（示例）", "不改变权限/审批/验收"],
            dependencies: updated ? ["新增：虚构复核清单乙（预置满足，未检测）"] : ["虚构复核清单甲（预置，未检测）"],
            scopeNote: updated
                ? "拟范围变化：曾建议扩到潮汐笔记；本次不申请该扩围，同意对象仍仅星港排程（示例）。"
                : "拟启用仅星港排程（示例），不包含潮汐笔记或全局。",
            relevantWork: [kind], availability: records
        )
    }

    static func item(_ id: SkillReviewItemID, preloaded: Bool) -> SkillReviewItem {
        let proposal = version(id)
        var item = SkillReviewItem(id: id, proposal: proposal)
        if preloaded {
            let target = item.target
            item.response = .agreed
            item.consentIsPreloaded = true
            item.approvedTarget = target
            item.installOutcome = .success
            item.installedTarget = target
            item.enabledTarget = target
            item.approvedHistory = [target]
            item.installedHistory = [target]
        }
        return item
    }

    static func items(for scenario: SkillReviewScenario) -> [SkillReviewItem] {
        if scenario == .drafts || scenario == .reset {
            return [SkillReviewItemID.boundary, .keyboard].map {
                var result = item($0, preloaded: false)
                result.contentState = .pending
                return result
            }
        }
        if scenario == .consent {
            return [item(.boundary, preloaded: false), item(.keyboard, preloaded: true)]
        }
        return SkillReviewItemID.allCases.map { item($0, preloaded: $0 != .candidate) }
    }

    static func works(items: [SkillReviewItem], scenario: SkillReviewScenario) -> [SkillReviewWork] {
        let old = items.first { $0.id == .boundary }?.enabledTarget
        return [
            .init(id: .api, title: "接口边界复核（示例任务）",
                  requirement: "只需核对接口差异，不需要键盘方法或归档整理。",
                  kind: .api, isInFlight: false),
            .init(id: .keyboard, title: "键盘可达性复核（示例工作包）",
                  requirement: "只需核对焦点、取消与滚动，不需要接口差异或归档整理。",
                  kind: .keyboard, isInFlight: false),
            .init(id: .inFlight, title: "在途接口工作（示例，已固定 v1.0）",
                  requirement: "保留旧版依据；继续使用仍须获准、安全可用、宿主允许且没有 Stop。",
                  kind: .api, isInFlight: true,
                  references: old.map { [.init(target: $0, origin: .preloaded)] } ?? []),
            .init(id: .newTask, title: "更新后的新任务（示例）",
                  requirement: "按当前项目获准启用版本按需选用；并非已消费新版。",
                  kind: .api, isInFlight: false),
        ]
    }

    static func section(for scenario: SkillReviewScenario) -> SkillReviewSection {
        switch scenario {
        case .drafts, .reset: .drafts
        case .consent: .requests
        case .updates, .inFlight: .updates
        case .scope, .tasks, .availability: .skills
        }
    }
}
#endif
