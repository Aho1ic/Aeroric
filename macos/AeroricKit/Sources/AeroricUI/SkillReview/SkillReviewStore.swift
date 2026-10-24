#if DEBUG
import Foundation
import Observation

/// Finite, window-local fixture transitions. No persistence, engine, agent or client.
@MainActor @Observable
final class SkillReviewStore {
    let instanceID = UUID()
    private(set) var generation = UUID()
    private(set) var scenario: SkillReviewScenario = .drafts
    private(set) var section: SkillReviewSection = .drafts
    private(set) var project: SkillReviewProject = .harbor
    var executor: SkillReviewExecutor = .codex
    var preview = SkillReviewPreview()
    private(set) var items: [SkillReviewItem] = []
    private(set) var works: [SkillReviewWork] = []
    var selectedItemID: SkillReviewItemID?
    private(set) var selectedWorkID: SkillReviewWorkID = .api
    private(set) var inFlightCondition: SkillReviewInFlightCondition = .allowed
    private(set) var stoppedWorkIDs: Set<SkillReviewWorkID> = []
    var sheet: SkillReviewSheet?
    private(set) var feedback = "仅内存示例；请选择一份有内容的草稿。"

    init(scenario: SkillReviewScenario = .drafts) { load(scenario) }

    var visibleItems: [SkillReviewItem] {
        switch section {
        case .drafts:
            items.filter { $0.contentState != .confirmed || scenario == .drafts || scenario == .reset }
        case .requests:
            // Keep the item being decided visible after enabling it, so a
            // native List cannot drop selection midway through the review.
            items.filter { $0.enabledTarget == nil || !$0.hasMatchingConsent || $0.id == selectedItemID }
        case .skills:
            items
        case .updates:
            items.filter { $0.id == .boundary }
        }
    }
    var selectedItem: SkillReviewItem? { items.first { $0.id == selectedItemID } }
    var selectedWork: SkillReviewWork? { works.first { $0.id == selectedWorkID } }
    var relatedItems: [SkillReviewItem] {
        guard let work = selectedWork else { return [] }
        return items.filter { $0.proposal.relevantWork.contains(work.kind) }
    }
    var canMutateProject: Bool { project == .harbor }
    var stopRequested: Bool { stoppedWorkIDs.contains(selectedWorkID) }

    func item(_ id: SkillReviewItemID) -> SkillReviewItem? { items.first { $0.id == id } }

    func load(_ next: SkillReviewScenario) {
        generation = UUID()
        sheet = nil
        scenario = next
        section = SkillReviewFixtures.section(for: next)
        project = .harbor
        executor = .codex
        inFlightCondition = .allowed
        stoppedWorkIDs = []
        items = SkillReviewFixtures.items(for: next)
        works = SkillReviewFixtures.works(items: items, scenario: next)
        selectedWorkID = next == .inFlight ? .inFlight : .api
        selectedItemID = visibleItems.first?.id
        feedback = next.summary
    }

    func reset() {
        preview = SkillReviewPreview()
        load(.drafts)
        feedback = "已重置全部内存状态与外观/权限预览（示例）；未保存真实偏好。"
    }

    func setSection(_ next: SkillReviewSection) {
        sheet = nil
        section = next
        selectedItemID = visibleItems.first?.id
    }

    func setProject(_ next: SkillReviewProject) {
        generation = UUID()
        sheet = nil
        project = next
        feedback = next == .harbor
            ? "仅星港排程范围（示例）；任务操作权限仍默认仅当前任务。"
            : "潮汐笔记未获准（示例），不自动扩围或全局启用。"
    }

    func setWork(_ next: SkillReviewWorkID) {
        guard works.contains(where: { $0.id == next }) else { return }
        selectedWorkID = next
        feedback = "已切换虚构任务/工作包；没有向 Agent 发送指令。"
    }

    func makeDraft() -> SkillReviewEditDraft? {
        guard canMutateProject, let item = selectedItem,
              item.contentState != .discarded else { return nil }
        return .init(id: UUID(), generation: generation, viewingProject: project, version: item.proposal)
    }

    func beginEdit() {
        guard let draft = makeDraft() else { return }
        sheet = .edit(draft)
    }

    func cancelSheet() { sheet = nil }

    @discardableResult
    func apply(_ draft: SkillReviewEditDraft) -> Bool {
        guard case let .edit(activeDraft) = sheet, activeDraft.id == draft.id,
              activeDraft.version.itemID == draft.version.itemID,
              activeDraft.version.number == draft.version.number,
              draft.generation == generation, draft.viewingProject == project,
              canMutateProject, let index = items.firstIndex(where: { $0.id == draft.version.itemID }),
              items[index].contentState != .discarded,
              !draft.version.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              !draft.version.content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else { return refuse("草稿已过期、为空或不属当前项目（示例）；未应用。") }
        let changed = draft.version != items[index].proposal
        if changed {
            let editedTarget = items[index].target
            items[index].proposal = draft.version
            items[index].contentState = .pending
            items[index].response = .unanswered
            items[index].consentIsPreloaded = false
            items[index].approvedTarget = nil
            items[index].installOutcome = .unloaded
            items[index].installedTarget = nil
            if items[index].enabledTarget == editedTarget { items[index].enabledTarget = nil }
            // Old immutable history/references remain evidence of the old specimen,
            // never consent for the changed proposal or permission to hot-replace it.
        }
        sheet = nil
        feedback = changed
            ? "修改仅应用到窗口内存（示例）；旧同意不适用于修改后的对象，未安装或执行。"
            : "展示对象没有变化（示例）；原内存同意/结果/启用保持不变，没有实际安装或执行。"
        return true
    }

    @discardableResult
    func confirmContent() -> Bool {
        guard canMutateProject, let index = selectedIndex,
              items[index].contentState != .discarded else { return refuse("无法确认该草稿（示例）。") }
        items[index].contentState = .confirmed
        feedback = "已确认内容与星港排程范围（示例）；这不是安装同意、启用或操作授权。"
        return true
    }

    @discardableResult
    func discardDraft() -> Bool {
        guard canMutateProject, let index = selectedIndex,
              items[index].installedHistory.isEmpty else { return refuse("仅可丢弃未安装草稿（示例）。") }
        items[index].contentState = .discarded
        items[index].response = .unanswered
        items[index].consentIsPreloaded = false
        items[index].approvedTarget = nil
        items[index].installOutcome = .unloaded
        items[index].installedTarget = nil
        items[index].enabledTarget = nil
        feedback = "已丢弃草稿（示例），不是停用真实技能；重置可以恢复。"
        return true
    }

    @discardableResult
    func respond(_ response: SkillReviewConsentResponse) -> Bool {
        guard canMutateProject, let index = selectedIndex,
              items[index].contentState != .discarded else { return refuse("当前项目不能批准此对象（示例）。") }
        // Rejecting a candidate never erases the separately approved old version.
        items[index].response = response
        items[index].consentIsPreloaded = false
        items[index].approvedTarget = response == .agreed ? items[index].target : nil
        let displayed = items[index].target
        if response != .agreed {
            if items[index].approvedHistory.contains(displayed),
               !items[index].withdrawnTargets.contains(displayed) {
                items[index].withdrawnTargets.append(displayed)
            }
            if items[index].enabledTarget == items[index].target { items[index].enabledTarget = nil }
            if items[index].installedTarget == items[index].target {
                items[index].installedTarget = nil
                items[index].installOutcome = .unloaded
            }
        }
        if response == .agreed { items[index].withdrawnTargets.removeAll { $0 == displayed } }
        if response == .agreed, let target = items[index].approvedTarget,
           !items[index].approvedHistory.contains(target) {
            items[index].approvedHistory.append(target)
        }
        feedback = response.title + "；演示安装结果、项目启用、可用性和任务选用仍分别显示。"
        return true
    }

    @discardableResult
    func loadInstall(_ outcome: SkillReviewInstallOutcome) -> Bool {
        guard canMutateProject, let index = selectedIndex, items[index].hasMatchingConsent
        else { return refuse("未答、拒绝或对象已变更，不能载入安装结果（示例）；Full Access 不能替代同意。") }
        items[index].installOutcome = outcome
        items[index].installedTarget = outcome == .success ? items[index].target : nil
        let target = items[index].target
        if outcome == .success {
            items[index].unusableInstallTargets.removeAll { $0 == target }
            if !items[index].installedHistory.contains(target) { items[index].installedHistory.append(target) }
        } else {
            if !items[index].unusableInstallTargets.contains(target) { items[index].unusableInstallTargets.append(target) }
            if items[index].enabledTarget == target { items[index].enabledTarget = nil }
        }
        feedback = outcome.title + "；这不是实际安装、可用性检测、版本启用或消费证据。"
        return true
    }

    @discardableResult
    func enableVersion() -> Bool {
        guard canMutateProject, let index = selectedIndex, items[index].canEnable
        else { return refuse("须先同意该对象并载入预置成功记录，才可演示项目启用。") }
        items[index].enabledTarget = items[index].target
        feedback = "已启用该版本于星港排程（示例）；未选用到任何新任务，没有真实加载/消费。"
        return true
    }

    func stageUpdate() {
        guard canMutateProject, let index = selectedIndex, items[index].id == .boundary else { return }
        guard items[index].proposal.number != "1.1" else { return }
        items[index].updateBaseline = items[index].enabledTarget ?? items[index].installedHistory.last
        items[index].proposal = SkillReviewFixtures.version(.boundary, updated: true)
        items[index].response = .unanswered
        items[index].consentIsPreloaded = false
        items[index].approvedTarget = nil
        items[index].installOutcome = .unloaded
        items[index].installedTarget = nil
        feedback = "已展示 v1.1 更新候选（示例）；未覆盖已启用 v1.0，在途版本不变。"
    }

    func loadInFlight(_ condition: SkillReviewInFlightCondition) {
        guard canMutateProject else {
            _ = refuse("另一项目未获准（示例）；不能改变星港排程的有限记录。")
            return
        }
        inFlightCondition = condition
        feedback = condition.title + "。这是有限记录，不是安全判据或撤销传播；不会换版/回放/清上下文。"
    }

    func requestStop() {
        guard canMutateProject else {
            _ = refuse("另一项目未获准（示例）；不能改变星港排程的工作意图。")
            return
        }
        stoppedWorkIDs.insert(selectedWorkID)
        feedback = "用户 Stop：停止工作意图（示例）优先，禁止继续选用；未发送真实信号，未抹除已消费上下文。"
    }

    var inFlightReason: String? {
        if stopRequested { return "Stop 已要求停止工作（示例）；允许记录不能复活该意图。" }
        return inFlightCondition.blockingReason
    }

    /// Describes the DISPLAYED candidate, not a separately enabled old version.
    /// An optimistic compatibility preset never hides a failed/missing result.
    func proposalUsageBlock(for id: SkillReviewItemID) -> String? {
        guard canMutateProject else { return "当前查看项目未获准（示例），不自动扩围。" }
        guard let item = item(id), item.hasMatchingConsent else {
            return "所展示对象尚无匹配的人工同意（示例），不可启用/选用。"
        }
        guard item.installOutcome != .failure else {
            return "所展示版本的预置安装失败（示例），不是可用的已安装资产，不可选用。"
        }
        guard item.installOutcome == .success, item.installedTarget == item.target else {
            return "所展示版本的预置成功记录尚未载入（示例），不可选用。"
        }
        guard item.enabledTarget == item.target else {
            return "所展示版本尚未在项目启用（示例）；成功记录不等于启用或选用。"
        }
        return selectionBlock(for: id)
    }

    /// The same projection gates both manual and simulated selection.
    func selectionBlock(for id: SkillReviewItemID, workID: SkillReviewWorkID? = nil) -> String? {
        guard project == .harbor else { return "另一项目未获准（示例），不得扩围。" }
        guard !stoppedWorkIDs.contains(workID ?? selectedWorkID) else { return "用户 Stop 优先（示例）；不再选用。" }
        guard let item = item(id),
              let work = works.first(where: { $0.id == (workID ?? selectedWorkID) })
        else { return "缺少示例对象或任务。" }
        if work.isInFlight { return "在途引用固定；不能手动添加/替换版本（示例）。" }
        guard let target = item.enabledTarget else { return "项目版本未启用（示例）。" }
        guard target.project == project, item.approvedHistory.contains(target),
              item.installedHistory.contains(target) else { return "缺少该版本的同意或预置成功依据（示例）。" }
        guard !item.withdrawnTargets.contains(target) else { return "该版本同意已取消/拒绝（示例），历史记录不恢复授权。" }
        guard !item.unusableInstallTargets.contains(target) else { return "该版本的预置安装失败/未载入（示例），不可选用。" }
        if id == .boundary, target.version.number == "1.0",
           let reason = inFlightCondition.blockingReason { return reason }
        guard target.version.relevantWork.contains(work.kind) else { return "与此任务/工作包需求无关（示例），不整库选用。" }
        let availability = target.version.availability(for: executor)
        guard availability.state == .available else { return availability.reason }
        return nil
    }

    @discardableResult
    func select(_ id: SkillReviewItemID, origin: SkillReviewSelectionOrigin = .manual) -> Bool {
        if let reason = selectionBlock(for: id) { return refuse(reason) }
        guard let target = item(id)?.enabledTarget,
              let index = works.firstIndex(where: { $0.id == selectedWorkID }) else { return false }
        works[index].references.removeAll { $0.id == id }
        works[index].references.append(.init(target: target, origin: origin))
        feedback = "\(origin.title)：已选 v\(target.version.number)（示例）；原范围内不重复使用批准。\(SkillReviewFixtures.noConsumption)"
        return true
    }

    func deselect(_ id: SkillReviewItemID) {
        guard canMutateProject,
              let index = works.firstIndex(where: { $0.id == selectedWorkID }),
              !works[index].isInFlight else { return }
        works[index].references.removeAll { $0.id == id }
        feedback = "已取消本任务示例选用；不代表卸载或清除模型上下文。"
    }

    func agentSelect() {
        guard canMutateProject, !stopRequested else {
            _ = refuse("项目未获准或用户 Stop 优先（示例）；原引用保留。")
            return
        }
        guard let index = works.firstIndex(where: { $0.id == selectedWorkID }),
              !works[index].isInFlight else {
            _ = refuse("在途引用不替换（示例）。")
            return
        }
        let candidates = items.filter { selectionBlock(for: $0.id) == nil }
        for candidate in candidates {
            if let target = candidate.enabledTarget {
                works[index].references.removeAll { $0.id == candidate.id }
                works[index].references.append(.init(target: target, origin: .agent))
            }
        }
        feedback = "本次演示 Agent 按需选用 \(candidates.count) 项；已有阻塞引用仅保留为历史，不算本次可用选用。不是整库加载，不构造 prompt。\(SkillReviewFixtures.noConsumption)"
    }

    func referenceBlock(_ reference: SkillReviewReference, work: SkillReviewWork) -> String? {
        guard project == reference.target.project else { return "当前查看项目未获准使用此引用（示例）。" }
        if stoppedWorkIDs.contains(work.id) { return "用户 Stop 优先（示例）；历史引用保留但不得继续使用。" }
        if reference.id == .boundary, reference.target.version.number == "1.0",
           let reason = inFlightCondition.blockingReason { return reason }
        guard let item = item(reference.id),
              item.approvedHistory.contains(reference.target),
              item.installedHistory.contains(reference.target)
        else { return "缺少该旧引用的获准/安装依据（示例）。" }
        guard !item.withdrawnTargets.contains(reference.target) else {
            return "该引用对象的同意已取消/拒绝（示例）；历史依据保留但不得继续使用。"
        }
        guard !item.unusableInstallTargets.contains(reference.target) else {
            return "该引用对象已载入失败/未载入记录（示例）；不当作可继续使用。"
        }
        // Historical evidence must not masquerade as current authorization
        // after the user explicitly withdraws/refuses that same specimen.
        // A separately reviewed new candidate does not revoke the old one.
        if item.target == reference.target, !item.hasMatchingConsent {
            return "该引用对象的当前同意已取消/拒绝（示例）；历史依据保留但不得继续使用。"
        }
        if item.target == reference.target, item.installOutcome != .success {
            return "该引用对象已载入失败/未载入记录（示例）；不当作可继续使用。"
        }
        guard reference.target.version.relevantWork.contains(work.kind) else {
            return "历史引用与此工作需求无关（示例）；不当作可继续使用。"
        }
        if !work.isInFlight, item.enabledTarget != reference.target {
            return "该非在途选择已不是项目启用版本（示例）；需重新按需选用，不自动换版。"
        }
        let availability = reference.target.version.availability(for: executor)
        return availability.state == .available ? nil : availability.reason
    }

    private var selectedIndex: Int? { items.firstIndex { $0.id == selectedItemID } }
    @discardableResult private func refuse(_ reason: String) -> Bool {
        feedback = reason + " " + SkillReviewFixtures.noConsumption
        return false
    }
}
#endif
