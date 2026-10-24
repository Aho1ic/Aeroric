#if DEBUG
import Foundation

enum WorkArrangementsReviewFixtures {
    static let disclaimer = "离线工作安排示例 · 全部为虚构数据"
    static let primaryTodoID = "goal-interface"
    static let activeRunID = "run-current"
    static let waitingIDs = ["wait-adapter", "wait-tests"]

    static func rule(for scenario: WAReviewScenario) -> WAReviewRule {
        var rule = WAReviewRule()
        if scenario != .w2 {
            rule.policy.permission = .limited
            rule.policyConfirmed = true
            rule.enabled = true
        }
        if [.w4, .w5, .w6, .w8].contains(scenario) { rule.policy.trigger = .schedule }
        return rule
    }

    static func todos(for scenario: WAReviewScenario) -> [WAReviewTodo] {
        var primary = WAReviewTodo(
            id: primaryTodoID, title: "星图项目：适配示例接口并保留必需核验", source: .user
        )
        if scenario != .w2 && scenario != .w5 {
            primary.intent = .assigned
            primary.assignmentPermission = .limited
        }
        let agent = WAReviewTodo(
            id: "agent-note", title: "补齐接口说明 · 同一份工作列表", source: .agent
        )
        let note = WAReviewTodo(
            id: "record-only", title: "先记一下：尚不交办的想法", source: .user, recordOnly: true
        )
        let unclear = WAReviewTodo(
            id: "unclear", title: "需要先澄清的目标", source: .agent, clarificationNeeded: true
        )
        let docs = WAReviewTodo(
            id: "backlog-docs", title: "存量：星图项目文档补充", source: .user, category: .documentation
        )
        var items = [primary, agent, note, unclear, docs]
        if scenario == .w7 || scenario == .w8 {
            items += [
                WAReviewTodo(
                    id: waitingIDs[0], title: "等待项 A：适配依赖接口（不覆盖旧修改）",
                    source: .user, intent: .assigned, assignmentPermission: .limited, allowWaiting: true,
                    wait: scenario == .w7 ? .sharedPoint : .budget
                ),
                WAReviewTodo(
                    id: waitingIDs[1], title: "等待项 B：相关验证（同一共享修改点）",
                    source: .agent, intent: .assigned, assignmentPermission: .limited, allowWaiting: true,
                    wait: scenario == .w7 ? .sharedPoint : .budget
                ),
            ]
        }
        return items
    }

    static func executions(for scenario: WAReviewScenario) -> [WAReviewExecution] {
        let original = WAReviewBasis(
            ruleRevision: 1, goal: "适配示例接口并保留必需核验", configuration: .a
        )
        var items: [WAReviewExecution] = []
        if scenario == .w1 {
            items.append(WAReviewExecution(
                id: "run-first-failure", todoID: primaryTodoID, primary: true, basis: original,
                phase: .failed, safety: .satisfied, sideEffects: .satisfied,
                hasFailureRecord: true,
                artifacts: "首次尝试失败；已保全诊断摘录，不被重试覆盖"
            ))
        }
        if [.w1, .w3, .w4, .w6, .w8].contains(scenario) {
            items.append(WAReviewExecution(
                id: activeRunID, todoID: primaryTodoID, primary: true, basis: original,
                phase: .running, safety: .failed, sideEffects: .unknown,
                artifacts: "已有部分适配成果（虚构）；未核验，不承诺外部回滚"
            ))
        }
        if scenario == .w7 || scenario == .w8 {
            items.append(WAReviewExecution(
                id: "independent-docs", todoID: nil, primary: false, usesRuleAuthority: false,
                basis: WAReviewBasis(
                    ruleRevision: 1, goal: "独立文档工作 · 港湾项目 / 共同资源组 B", configuration: .b
                ),
                phase: .running, safety: .failed, sideEffects: .unknown,
                artifacts: "独立、无相关依赖且不命中星图/组 A 限制；可继续"
            ))
        }
        // Direct work does not require manufacturing a todo for every execution.
        items.append(WAReviewExecution(
            id: "direct-history", todoID: nil, primary: false, usesRuleAuthority: false,
            basis: WAReviewBasis(ruleRevision: 1, goal: "无需待办的直接检查", configuration: .a),
            phase: .ended, safety: .satisfied, sideEffects: .satisfied,
            artifacts: "直接执行的历史结果；不代表任何待办已验收"
        ))
        return items
    }

    static let usage: [WAReviewUsage] = [
        WAReviewUsage(id: "usage-delegation", role: "主执行关联委派", scopes: [.local, .project, .shared], kind: .reported(8)),
        WAReviewUsage(id: "usage-reflection", role: "阶段反思", scopes: [.project, .shared], kind: .reported(3)),
        WAReviewUsage(id: "usage-governance", role: "独立治理", scopes: [.project, .shared], kind: .unknown),
        WAReviewUsage(id: "usage-retry", role: "获准重试", scopes: [.local, .project, .shared], kind: .estimate(5)),
        WAReviewUsage(id: "usage-catchup", role: "补一次当前检查", scopes: [.local, .project, .shared], kind: .reported(2)),
        // The same report is deliberately referenced twice, not charged twice.
        WAReviewUsage(id: "usage-delegation", role: "主执行关联委派", scopes: [.local, .project, .shared], kind: .reported(8)),
    ]

    static func explanation(for scenario: WAReviewScenario) -> String {
        switch scenario {
        case .w1:
            "创建只记录；保留失败与后续执行、直接执行。先载入结束，再比较缺整合/检查/监督与完整核验；用户验收另记。"
        case .w2:
            "先预览范围、配置别名、允许操作与资源，明确策略再启用；首次不接存量。批量交办/纳入与未来新增分开，Full Access 不等于启用。"
        case .w3:
            "暂停只挡新启动。Stop 请求、未知和安全收束分别载入；历史成果不回滚，被动事件不恢复用户控制。"
        case .w4:
            "规则草稿显示差异；保存、策略确认、启用、到点、已开始各自独立。在途沿用原目标/API 别名，但不能绕撤权、禁令或硬限。"
        case .w5:
            "预先开启的恢复补一次，才可合并三次已核实未启动的漏跑；只检查当前状态一次，历史仍未运行。未知原执行不重放。"
        case .w6:
            "载入三次繁忙到点；默认跳过。独立的结束后补一次最多保留一个当前意图；手动、定时、恢复均不重叠，未知/停止中不算空闲。"
        case .w7:
            "两项等示例依赖配置，港湾项目独立文档仍可推进。先载入安全释放，再重核；输入变化只对齐相关适配步骤，跳过不进队列。"
        case .w8:
            "示例消耗分局部、项目和共同组，报告/预估/未知分开。准入用尽不杀最后合规尝试；在途到限请求收束。补额不等于续旧执行、不清独立上限或监督。"
        }
    }
}
#endif
