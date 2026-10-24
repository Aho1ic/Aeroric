#if DEBUG
import Foundation

struct TaskDeliverablesReviewFixture {
    var deliverables: [TaskDeliverable]
    var feedbacks: [TaskDeliverablesReviewFeedback]
    var evidence: [TaskDeliverablesReviewEvidence]
    var operations: [TaskDeliverablesReviewOperationIntent]
    var rounds: [TaskDeliverablesReviewRound]
    var conditions: TaskDeliverablesReviewConditions
    var selectedDeliverableID: TaskDeliverableID
    var selectedVersionID: String
}

enum TaskDeliverablesReviewFixtures {
    static let offlineNotice =
        "离线成果示例 · 全部为虚构数据 · 不连接文件、Agent、API、PTY、host 或网络"
    static let versionBoundary =
        "版本内容、检查结果和意见依据只在本窗口内存中保留；不代表生产留存、恢复或并发安全。"
    static let acceptanceBoundary =
        "提交、Agent 自报、示例核验、用户历史接受和本次查看彼此独立；没有自动全绿。"

    static func make(_ scenario: TaskDeliverablesReviewScenario) -> TaskDeliverablesReviewFixture {
        let api = apiDocument
        let diff = validationDiff
        let image = previewImage
        let installer = installerPackage
        let temporary = temporaryLog
        let feedback = initialFeedback

        switch scenario {
        case .collection:
            return fixture(
                deliverables: [api, diff, image, installer, temporary],
                selected: (api.id, "api-v1")
            )
        case .versions:
            return fixture(
                deliverables: [api, diff, image, installer, temporary],
                selected: (api.id, "api-v1")
            )
        case .reading:
            return fixture(
                deliverables: [api, diff, image, installer, temporary],
                selected: (installer.id, "installer-v1")
            )
        case .feedback:
            return fixture(
                deliverables: [api, diff, image, installer, temporary],
                selected: (api.id, "api-v1")
            )
        case .evidence:
            return fixture(
                deliverables: [api, diff, image, installer, temporary],
                feedbacks: [feedback],
                selected: (api.id, "api-v1")
            )
        case .rework:
            return fixture(
                deliverables: [api, diff, image, installer, temporary],
                feedbacks: [feedback],
                evidence: [acceptedEvidence],
                selected: (api.id, "api-v1")
            )
        case .continuation:
            return fixture(
                deliverables: [api, diff, image, installer, temporary],
                feedbacks: [feedback],
                evidence: [acceptedEvidence],
                rounds: [queuedRound],
                conditions: TaskDeliverablesReviewConditions(),
                selected: (api.id, "api-v1")
            )
        case .reset:
            return fixture(
                deliverables: [api, diff, image, installer, temporary],
                selected: (api.id, "api-v1")
            )
        }
    }

    static func taskTitle(for scenario: TaskDeliverablesReviewScenario) -> String {
        scenario == .rework || scenario == .continuation
            ? "虚构任务 · API 错误呈现返工"
            : "虚构任务 · 海港交付审阅"
    }

    private static func fixture(
        deliverables: [TaskDeliverable],
        feedbacks: [TaskDeliverablesReviewFeedback] = [],
        evidence: [TaskDeliverablesReviewEvidence] = [],
        operations: [TaskDeliverablesReviewOperationIntent] = [],
        rounds: [TaskDeliverablesReviewRound] = [],
        conditions: TaskDeliverablesReviewConditions = .init(),
        selected: (TaskDeliverableID, String)
    ) -> TaskDeliverablesReviewFixture {
        TaskDeliverablesReviewFixture(
            deliverables: deliverables,
            feedbacks: feedbacks,
            evidence: evidence,
            operations: operations,
            rounds: rounds,
            conditions: conditions,
            selectedDeliverableID: selected.0,
            selectedVersionID: selected.1
        )
    }

    private static var apiDocument: TaskDeliverable {
        TaskDeliverable(
            id: .apiDocument,
            title: "错误响应契约说明",
            description: "阶段提交：说明 code / message 约定和空输入限制。",
            kind: .document,
            sourcePackage: "工作包 · API 错误接口",
            submittedBy: "主 Agent（示例）",
            collectionState: .collected,
            versions: [
                TaskDeliverableVersion(
                    id: "api-v1", number: "1", source: "阶段提交事件 · 2026-10-18（示例）",
                    contentKind: .markdown,
                    content: """
                        # 错误响应契约 v1

                        所有错误响应包含 `code` 和 `message`。本版本只说明字段形状，
                        不覆盖空输入、网络重试或用户验收。
                        """,
                    readback: .embedded,
                    checks: [
                        check(
                            "api-v1-shape", "字段形状核对", "code / message",
                            .passed, "预置范围内已核对 · 非本次实测")
                    ],
                    knownLocation: "文档 §1 · 字段形状",
                    limitation: "v1 没有空输入处理说明；后续 v2 的影响点需单独核对。"
                ),
                TaskDeliverableVersion(
                    id: "api-v2", number: "2", source: "返工关联交付 · 2026-10-19（示例）",
                    contentKind: .markdown,
                    content: """
                        # 错误响应契约 v2

                        保留 `code` / `message`，并为缺少字段的请求增加可读错误示例。
                        这是新的交付内容，不会覆盖 v1 的原文或检查记录。
                        """,
                    readback: .embedded,
                    checks: [
                        check(
                            "api-v2-shape", "字段形状核对", "code / message",
                            .passed, "示例核对结果 · 仅适用 v2 字段范围"),
                        check(
                            "api-v2-input", "空输入影响点", "缺少字段分支",
                            .pending, "新交付出现后仍待核对实际影响"),
                    ],
                    knownLocation: "文档 §2 · 空输入",
                    limitation: "v2 只关联本反馈范围，不能自动使全部意见结案。"
                ),
            ],
            currentWorkFile: TaskCurrentWorkFile(
                title: "当前工作文件示意 · ErrorResponse.swift",
                content: "当前编辑中的虚构文件；它不是 v1 / v2 的保存副本。",
                note: "当前文件仅供另看，不能替代选定交付版本。"
            ),
            originalTitle: "错误响应契约说明",
            originalDescription: "阶段提交：说明 code / message 约定和空输入限制。"
        )
    }

    private static var validationDiff: TaskDeliverable {
        TaskDeliverable(
            id: .validationDiff,
            title: "空输入校验差异",
            description: "代码差异片段，附固定基线和适用范围。",
            kind: .diff,
            sourcePackage: "工作包 · 客户端错误呈现",
            submittedBy: "接口成员（示例）",
            collectionState: .collected,
            versions: [
                TaskDeliverableVersion(
                    id: "diff-v1", number: "1", source: "阶段提交事件 · 2026-10-18（示例）",
                    contentKind: .diff,
                    content: """
                        - return .success(value)
                        + return .failure(.emptyInput)
                        """,
                    readback: .embedded,
                    checks: [
                        check(
                            "diff-v1-baseline", "固定基线核对", "FormValidator.swift@base",
                            .passed, "仅演示固定 diff 基线")
                    ],
                    knownLocation: "FormValidator.swift · 差异片段",
                    limitation: "行号不是持久锚点；新版位置变化需重新核对。"
                ),
                TaskDeliverableVersion(
                    id: "diff-v2", number: "2", source: "更正关联交付 · 2026-10-19（示例）",
                    contentKind: .diff,
                    content: """
                        @@ unknown location
                        空输入路径已移动；原 v1 行号无法可靠映射。
                        """,
                    readback: .referenceOnly,
                    checks: [
                        check(
                            "diff-v2-location", "位置映射", "原 v1 片段",
                            .unknown, "示例位置未知，保留 v1 原依据")
                    ],
                    knownLocation: nil,
                    limitation: "只有引用和未知位置；不以当前文件或 hash 补成完整副本。"
                ),
            ],
            currentWorkFile: TaskCurrentWorkFile(
                title: "当前工作文件示意 · FormValidator.swift",
                content: "当前工作区另有未提交改动。",
                note: "当前内容不自动成为交付版本。"
            ),
            originalTitle: "空输入校验差异",
            originalDescription: "代码差异片段，附固定基线和适用范围。"
        )
    }

    private static var previewImage: TaskDeliverable {
        TaskDeliverable(
            id: .previewImage,
            title: "错误状态图片示意",
            description: "已提交的图片示意；只表达成果来源和版本。",
            kind: .image,
            sourcePackage: "工作包 · 视觉确认",
            submittedBy: "视觉成员（示例）",
            collectionState: .submitted,
            versions: [
                TaskDeliverableVersion(
                    id: "image-v1", number: "1", source: "阶段提交事件 · 2026-10-18（示例）",
                    contentKind: .image,
                    content: "[图片示意：空输入错误状态，不加载外部图像]",
                    readback: .embedded,
                    checks: [
                        check(
                            "image-v1-visual", "视觉范围说明", "固定图片示意",
                            .notRun, "当前政策暂停真实窗口验收")
                    ],
                    knownLocation: nil,
                    limitation: "图片只是内嵌文字示意，不证明真实材质、AX 或布局。"
                )
            ],
            currentWorkFile: nil,
            originalTitle: "错误状态图片示意",
            originalDescription: "已提交的图片示意；只表达成果来源和版本。"
        )
    }

    private static var installerPackage: TaskDeliverable {
        TaskDeliverable(
            id: .installer,
            title: "测试安装包（未知格式）",
            description: "仅有类型、来源和限制，不调用安装器或外部应用。",
            kind: .package,
            sourcePackage: "工作包 · 本地打包",
            submittedBy: "构建成员（示例）",
            collectionState: .submitted,
            versions: [
                TaskDeliverableVersion(
                    id: "installer-v1", number: "1", source: "交付声明 · 未载入二进制",
                    contentKind: .unknown,
                    content: nil,
                    readback: .unavailable,
                    checks: [
                        check(
                            "installer-v1-build", "构建状态", "安装包内容",
                            .unknown, "仅有交付声明，未载入或执行未知格式")
                    ],
                    knownLocation: nil,
                    limitation: "未知格式不可安全被动呈现；运行 / 安装 / 外部打开均未执行。"
                )
            ],
            currentWorkFile: nil,
            originalTitle: "测试安装包（未知格式）",
            originalDescription: "仅有类型、来源和限制，不调用安装器或外部应用。"
        )
    }

    private static var temporaryLog: TaskDeliverable {
        TaskDeliverable(
            id: .temporaryLog,
            title: "调试临时日志",
            description: "临时内容没有作为成果提交，不默认塞入清单。",
            kind: .log,
            sourcePackage: "工作包 · 调试记录",
            submittedBy: "协作者（示例）",
            collectionState: .temporary,
            versions: [
                TaskDeliverableVersion(
                    id: "log-v1", number: "1", source: "临时内存记录",
                    contentKind: .markdown,
                    content: "临时日志：只用于说明为什么不默认收录。",
                    readback: .embedded,
                    checks: [
                        check(
                            "log-v1-check", "日志状态", "是否交付",
                            .notRun, "临时日志不是已提交成果")
                    ],
                    knownLocation: nil,
                    limitation: "未明确提交；不因文件类型自动成为成果。"
                )
            ],
            currentWorkFile: nil,
            originalTitle: "调试临时日志",
            originalDescription: "临时内容没有作为成果提交，不默认塞入清单。"
        )
    }

    private static var initialFeedback: TaskDeliverablesReviewFeedback {
        TaskDeliverablesReviewFeedback(
            id: "feedback-1",
            taskID: "task-harbor-deliverables",
            deliverableID: .apiDocument,
            versionID: "api-v1",
            location: .known("文档 §1 · 字段形状"),
            text: "请补充空输入限制，并保留原 v1 依据。",
            recipient: .coordinator,
            state: .sent,
            correctionOf: nil
        )
    }

    private static var acceptedEvidence: TaskDeliverablesReviewEvidence {
        TaskDeliverablesReviewEvidence(
            id: "evidence-accepted-v1",
            feedbackID: nil,
            versionID: "api-v1",
            kind: .userAccepted,
            detail: "预置历史事实：用户接受 v1 当时范围；不延伸到 v2。",
            isExampleEvent: true
        )
    }

    private static var queuedRound: TaskDeliverablesReviewRound {
        TaskDeliverablesReviewRound(
            id: "round-rework-1",
            taskID: "task-harbor-deliverables",
            feedbackID: "feedback-1",
            sourceVersionID: "api-v1",
            state: .blocked,
            detail: "同任务返工已交办；等待资源、原执行核实和明确恢复。"
        )
    }

    private static func check(
        _ id: String,
        _ title: String,
        _ scope: String,
        _ result: TaskCheckResult,
        _ evidence: String
    ) -> TaskDeliverableCheck {
        TaskDeliverableCheck(id: id, title: title, scope: scope, result: result, evidence: evidence)
    }
}
#endif
