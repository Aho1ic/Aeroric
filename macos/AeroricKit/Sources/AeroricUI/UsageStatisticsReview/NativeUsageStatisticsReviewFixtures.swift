#if DEBUG
enum NativeUsageStatisticsReviewFixtures {
    static let notice = "独立 DEBUG 离线样例 · 全部为确定虚构内存 · 未连接真实统计、账单或执行服务 · 可重置，非产品验收"
    static let initialAsOf = "2026-09-13 01:00 UTC"
    static let v1 = USReviewPrice(version: "示例价 v1 · 2026-09-01",
                                  freshPerMillion: 2, cachePerMillion: 1, outputPerMillion: 7)
    static let customPrice = USReviewPrice(version: "自定义示例价 · 仅 F / API-A / Model-A",
                                          freshPerMillion: 3, cachePerMillion: 1, outputPerMillion: 8)

    static func corpus() -> [USReviewUsage] {
        [
            usage("F", tokens: 1400, task: "T-A", role: "主执行", executor: "Codex",
                  api: "API-A", model: "Model-A", relation: "失败尝试，已发生使用仍保留",
                  minute: 1410, input: 1000, cache: 200, output: 400),
            usage("R", tokens: 800, task: "T-A", role: "主执行", executor: "Codex",
                  api: "API-A", model: "Model-A", relation: "独立重试，独立 attempt", minute: 1380),
            usage("V", tokens: 600, task: "T-A", role: "反思", executor: "Claude Code",
                  api: "API-B", model: "Model-B", relation: "适用反思，原币 EUR", minute: 1350),
            usage("D", tokens: 300, task: "T-A-child", role: "委派", executor: "DSH",
                  api: "API-C", model: "Model-D", relation: "父项已含委派，不能再加子项", minute: 1320),
            usage("S", tokens: 500, task: nil, role: "共享治理", executor: "OMP",
                  api: "API-D", model: nil, relation: "关联 T-A / T-B；单笔共享，未分摊", minute: 1290,
                  shared: true),
            usage("N", tokens: 200, task: "T-N", project: nil, role: nil, executor: "custom",
                  api: nil, model: nil, relation: "无项目，保留未归属", minute: 1260),
            usage("U", tokens: nil, task: "T-A", role: nil, executor: "custom",
                  api: nil, model: nil, relation: "不支持 / 缺报告，未知不是零", minute: 1230),
        ]
    }

    static func usage(_ id: String, tokens: Int?, task: String?, project: String? = "P-A",
                      role: String?, executor: String, api: String?, model: String?,
                      relation: String, minute: Int, shared: Bool = false,
                      input: Int? = nil, cache: Int? = nil, output: Int? = nil) -> USReviewUsage {
        .init(id: id, attempt: "attempt-\(id)", task: task, project: project, role: role,
              executor: executor, api: api, model: model, relation: relation, shared: shared,
              occurrence: .instant(minute),
              tokens: .init(value: tokens, basis: tokens == nil ? .unknown : .reported,
                            source: id == "F" ? "A-r1" : "demo-source-\(id)",
                            coverage: tokens == nil ? "缺报告，不纳入已知部分" : "仅此 usageID，不代表全量覆盖"),
              input: input, cacheRead: cache, output: output, revision: id == "F" ? "A-r1" : "\(id)-r1")
    }

    static var retry: USReviewUsage {
        usage("E-retry", tokens: 400, task: "T-A", role: "主执行", executor: "Codex",
              api: "API-A", model: "Model-A", relation: "显式模拟的独立已发生重试，不启动执行", minute: 1420)
    }
    static var newReport: USReviewUsage {
        usage("E-new", tokens: 100, task: "T-A", role: "主执行", executor: "Codex",
              api: "API-A", model: "Model-A", relation: "清理后显式新报告，不复活旧明细", minute: 1430)
    }
    static var crossPeriod: USReviewUsage {
        var value = usage("X", tokens: 900, task: "T-A", role: nil, executor: "custom",
                          api: nil, model: nil, relation: "跨期累计，不能猜分", minute: 0)
        // Both sides of the selected day's interval; overlap alone is not coverage.
        value = .init(id: value.id, attempt: value.attempt, task: value.task, project: value.project,
                      role: value.role, executor: value.executor, api: value.api, model: value.model,
                      relation: value.relation, shared: false, occurrence: .cumulative(-1440..<2880),
                      tokens: value.tokens, revision: value.revision)
        return value
    }
    static var zero: USReviewMetric {
        .init(value: 0, basis: .observedZero, source: "Z · 固定完整观测范围",
              coverage: "仅 Token 在声明范围观测为零，费用仍未知")
    }

    /// Exact state IDs from the approved v3 reference; not six permanent product tabs.
    static let presets: [USReviewPreset] = [
        .init(id: "US1/baseline", title: "同批层级与切面"),
        .init(id: "US1/duplicate", title: "同笔重复到达"),
        .init(id: "US1/retry", title: "独立已发生重试"),
        .init(id: "US1/not-started", title: "未启动与历史身份"),
        .init(id: "US2/sources", title: "来源、原币与覆盖"),
        .init(id: "US2/price-draft", title: "价格草稿"),
        .init(id: "US2/price-cancelled", title: "取消价格草稿"),
        .init(id: "US2/price-applied", title: "应用自定义参考"),
        .init(id: "US2/conversion", title: "部分折算"),
        .init(id: "US2/conflict", title: "来源冲突"),
        .init(id: "US3/baseline", title: "固定更新基准"),
        .init(id: "US3/late", title: "可靠迟到费用报告"),
        .init(id: "US3/revise-up", title: "有据上调"),
        .init(id: "US3/revise-down", title: "有据下调"),
        .init(id: "US3/conflict", title: "关联不清的冲突"),
        .init(id: "US3/failure-stale", title: "刷新失败，保留旧值"),
        .init(id: "US3/failure-empty", title: "刷新失败，无旧值"),
        .init(id: "US3/new-snapshot-late", title: "补报后的新快照"),
        .init(id: "US3/new-snapshot-up", title: "上调后的新快照"),
        .init(id: "US3/new-snapshot-down", title: "下调后的新快照"),
        .init(id: "US3/time-windows", title: "时区、累计与并行时间"),
        .init(id: "US4/user", title: "用户筛选与角色分开"),
        .init(id: "US4/roles", title: "角色必要摘要"),
        .init(id: "US4/preauthorized", title: "预置授权范围"),
        .init(id: "US4/blocked", title: "越界意图"),
        .init(id: "US4/export-preview", title: "最小内存预览"),
        .init(id: "US4/file-intent", title: "文件导出意图"),
        .init(id: "US4/send-intent", title: "外部模型意图"),
        .init(id: "US4/closed", title: "关闭预览"),
        .init(id: "US5/baseline", title: "明细与汇总分层"),
        .init(id: "US5/draft-details", title: "明细清理草稿"),
        .init(id: "US5/draft-project-period", title: "项目期间清理草稿"),
        .init(id: "US5/project-removed", title: "项目明细清理"),
        .init(id: "US5/project-cancelled", title: "取消项目清理"),
        .init(id: "US5/draft-summary", title: "汇总清理草稿"),
        .init(id: "US5/draft-summary-after-details", title: "清明细后预览汇总清理"),
        .init(id: "US5/cancel-summary-after-details", title: "清明细后取消汇总清理"),
        .init(id: "US5/cancelled", title: "取消清理"),
        .init(id: "US5/details-removed", title: "明细已清，汇总保留"),
        .init(id: "US5/summary-gap", title: "汇总断点，不是零"),
        .init(id: "US5/summary-gap-after-details", title: "两层清理后的断点"),
        .init(id: "US5/refreshed", title: "普通刷新不复活"),
        .init(id: "US5/new-report", title: "清理后显式新报告"),
        .init(id: "US5/protected", title: "必要控制记录不可清"),
        .init(id: "US6/shared-cap", title: "局部有余，共享上限已到"),
        .init(id: "US6/warning", title: "预警不新增拦截"),
        .init(id: "US6/unknown", title: "展示未知与必需依据未知"),
        .init(id: "US6/concurrency", title: "并发满，合法在途继续"),
        .init(id: "US6/limit-request", title: "在途到限请求收束"),
        .init(id: "US6/winding", title: "收束中，不等于已停止"),
        .init(id: "US6/stopped", title: "模拟核实停止"),
        .init(id: "US6/stop-unknown", title: "原执行状态未知"),
        .init(id: "US6/capability", title: "硬限制能力受阻"),
        .init(id: "US6/quota-draft", title: "调额草稿"),
        .init(id: "US6/quota-applied", title: "调额不重启"),
        .init(id: "US6/continue-intent", title: "继续意图不是执行"),
        .init(id: "US6/topup-intent", title: "补额并继续意图"),
        .init(id: "US6/auto-success", title: "完整自动续行重核"),
        .init(id: "US6/auto-blocked", title: "各项独立续行阻因"),
    ]
}
#endif
