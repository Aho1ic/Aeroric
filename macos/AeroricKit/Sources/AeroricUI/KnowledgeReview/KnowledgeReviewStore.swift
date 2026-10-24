#if DEBUG
import Foundation
import Observation

/// One window's finite fictional demonstration. There is no I/O, production
/// client, background task, defaults access or shared singleton. Opening a new
/// window uses a new instance; reset also invalidates every outstanding draft.
@MainActor
@Observable
final class KnowledgeReviewStore {
    let instanceID = UUID()
    private(set) var scenario: KnowledgeReviewScenario
    private(set) var items: [KnowledgeReviewItem]
    private(set) var section: KnowledgeReviewSection
    var selectedItemID: KnowledgeReviewItemID?
    private(set) var selectedCandidateIDs: Set<KnowledgeReviewItemID> = []
    private(set) var admissionConfiguration = KnowledgeReviewAdmissionConfiguration()
    private(set) var permission: KnowledgeReviewPermission = .standard
    private(set) var hasProcessedNewCandidates = false
    private(set) var selectedWorkID: KnowledgeReviewWorkID
    private(set) var referenceScenario: KnowledgeReviewReferenceScenario = .sourced
    private(set) var impactRows: [KnowledgeReviewImpact]
    private(set) var notice: String

    // Value copies are editable by native form bindings. Private active IDs,
    // generation and version checks reject cancelled, superseded or stale
    // copies even if a late sheet callback puts one back into these properties.
    var contentDraft: KnowledgeReviewContentDraft?
    var configurationDraft: KnowledgeReviewConfigurationDraft?
    var revisionDraft: KnowledgeReviewRevisionDraft?
    private var generation = UUID()
    private var activeContentDraftID: UUID?
    private var activeConfigurationDraftID: UUID?
    private var activeRevisionDraftID: UUID?

    static let sampleOnlyNotice = "仅本窗口虚构内存；未保存真实知识、配置或权限"

    init(scenario: KnowledgeReviewScenario = .candidates) {
        self.scenario = scenario
        items = KnowledgeReviewFixtures.items(for: scenario)
        section = KnowledgeReviewFixtures.defaultSection(for: scenario)
        selectedItemID = KnowledgeReviewFixtures.defaultSelection(for: scenario)
        selectedWorkID = scenario == .references ? .storageTask : .apiTask
        impactRows = KnowledgeReviewFixtures.impacts(for: scenario)
        notice = "已载入 \(scenario.menuTitle) · \(Self.sampleOnlyNotice)"
    }

    // MARK: - Selection and read-only projections

    var candidateItems: [KnowledgeReviewItem] { items.filter { $0.state == .candidate } }
    var reusableItems: [KnowledgeReviewItem] { items.filter { $0.state == .reusable } }
    var historyItems: [KnowledgeReviewItem] { items.filter(\.hasHistory) }
    var visibleItems: [KnowledgeReviewItem] {
        switch section {
        case .candidates: candidateItems
        case .reusable: reusableItems
        case .history: historyItems
        }
    }
    var selectedItem: KnowledgeReviewItem? {
        visibleItems.first { $0.id == selectedItemID }
    }
    var works: [KnowledgeReviewWork] { KnowledgeReviewFixtures.works }
    var selectedWork: KnowledgeReviewWork { KnowledgeReviewFixtures.work(selectedWorkID) }
    var autoAdmissionPropagationNote: String { KnowledgeReviewFixtures.closingAutomaticAdmissionNote }
    var capturedReferences: [KnowledgeReviewCapturedReference] {
        impactRows.compactMap(\.capturedReference)
    }

    func item(id: KnowledgeReviewItemID) -> KnowledgeReviewItem? {
        items.first { $0.id == id }
    }

    func setSection(_ section: KnowledgeReviewSection) {
        self.section = section
        repairDetailSelection()
    }

    func selectItem(_ id: KnowledgeReviewItemID?) {
        guard let id else {
            selectedItemID = nil
            return
        }
        guard visibleItems.contains(where: { $0.id == id }) else { return }
        selectedItemID = id
    }

    /// Checkbox selection deliberately does not change the detail focus.
    func setCandidateSelected(_ id: KnowledgeReviewItemID, selected: Bool) {
        guard item(id: id)?.state == .candidate else { return }
        if selected {
            selectedCandidateIDs.insert(id)
        } else {
            selectedCandidateIDs.remove(id)
        }
    }

    var batchConfirmationBlockedReason: String? {
        if let reason = batchDiscardBlockedReason { return reason }
        for candidate in candidateItems where selectedCandidateIDs.contains(candidate.id) {
            if let reason = candidate.confirmationBlockedReason {
                return "\(candidate.title)：\(reason)；整批尚未确认，请取消该项勾选后再审阅。"
            }
        }
        return nil
    }

    var batchDiscardBlockedReason: String? {
        guard section == .candidates else { return "请在候选栏目操作批量勾选。" }
        guard !selectedCandidateIDs.isEmpty else { return "请先勾选候选；详情焦点不是批量勾选。" }
        guard selectedCandidateIDs.allSatisfy({ item(id: $0)?.state == .candidate }) else {
            return "勾选包含已不再是候选的项；未应用整批操作。"
        }
        return nil
    }

    @discardableResult
    func confirmSelectedCandidates() -> Bool {
        if let reason = batchConfirmationBlockedReason { return refuse(reason) }
        let ids = selectedCandidateIDs
        for index in items.indices where ids.contains(items[index].id) {
            items[index].state = .reusable
            items[index].admissionOrigin = .manual
        }
        finishBatch(ids)
        notice = "已在本窗口确认 \(ids.count) 条虚构候选；保留来源、适用范围、限制与原核验标记，不代表当前验证。"
        return true
    }

    @discardableResult
    func discardSelectedCandidates() -> Bool {
        if let reason = batchDiscardBlockedReason { return refuse(reason) }
        let ids = selectedCandidateIDs
        for index in items.indices where ids.contains(items[index].id) {
            items[index].state = .discarded
        }
        finishBatch(ids)
        notice = "已在本窗口丢弃 \(ids.count) 条虚构候选；可在样例历史回看，未删除真实资料。"
        return true
    }

    // MARK: - Candidate content draft

    func beginContentEdit() {
        guard let item = selectedItem, item.state == .candidate else {
            notice = "只有当前候选可编辑内容草稿；可复用接口请走带依据的修订。"
            return
        }
        clearDrafts()
        let id = UUID()
        activeContentDraftID = id
        contentDraft = .init(
            id: id, generation: generation, originalVersion: item.current.id,
            content: item.content, applicability: item.applicability.summary
        )
    }

    func cancelContentDraft() {
        contentDraft = nil
        activeContentDraftID = nil
        notice = "已取消内容草稿；内容、来源与适用范围均未应用改动。"
    }

    @discardableResult
    func applyContentDraft() -> Bool {
        guard let draft = contentDraft,
              draft.id == activeContentDraftID, draft.generation == generation,
              let index = items.firstIndex(where: { $0.id == draft.originalVersion.itemID }),
              items[index].state == .candidate,
              items[index].current.id == draft.originalVersion else {
            return refuse("内容草稿已失效或不存在；不能写回重置后的样例。")
        }
        let content = trimmed(draft.content)
        let applicability = trimmed(draft.applicability)
        guard !content.isEmpty, !applicability.isEmpty else {
            return refuse("内容与适用说明不能为空；草稿尚未应用。")
        }
        let changed = items[index].content != content || items[index].applicability.summary != applicability
        items[index].current.content = content
        items[index].current.applicability.summary = applicability
        // The source remains attached, but a changed candidate cannot inherit
        // a preloaded check's verification of different text/conditions.
        if changed && items[index].current.evidence == .preloadedVerified {
            items[index].current.evidence = .userStatement
        }
        contentDraft = nil
        activeContentDraftID = nil
        notice = "已应用虚构候选内容与适用说明；保留项目、来源与限制，编辑不制造新的核验记录。"
        return true
    }

    // MARK: - Fixed configuration, independent permission and one new batch

    func beginConfiguration() {
        clearDrafts()
        let id = UUID()
        activeConfigurationDraftID = id
        configurationDraft = .init(id: id, generation: generation, configuration: admissionConfiguration)
    }

    func cancelConfigurationDraft() {
        configurationDraft = nil
        activeConfigurationDraftID = nil
        notice = "已取消虚构自动入库配置；未应用，任务授权示例不变。"
    }

    @discardableResult
    func applyConfigurationDraft() -> Bool {
        guard let draft = configurationDraft,
              draft.id == activeConfigurationDraftID, draft.generation == generation else {
            return refuse("配置草稿已失效或不存在；不能应用到新的样例。")
        }
        admissionConfiguration = draft.configuration
        configurationDraft = nil
        activeConfigurationDraftID = nil
        let enabled = admissionConfiguration.enabled ? "开启" : "关闭"
        notice = "已\(enabled)本窗口自动入库预览（\(admissionConfiguration.preset.title)）；任务授权未改变。\(autoAdmissionPropagationNote)"
        return true
    }

    func setPermission(_ permission: KnowledgeReviewPermission) {
        self.permission = permission
        notice = "已切换\(permission.title)；只是内存情景，不改变自动入库、资料范围或真实任务权限。"
    }

    func automaticAdmissionReason(for item: KnowledgeReviewItem) -> String? {
        if let reason = item.confirmationBlockedReason { return reason }
        guard admissionConfiguration.enabled else { return "自动入库预览关闭；新内容仍留候选供集中审阅。" }
        let preset = admissionConfiguration.preset
        guard item.kind == preset.kind,
              item.source.kind == preset.sourceKind,
              item.applicability.project == preset.project else {
            return "不符合固定预设“\(preset.title)”的类型／来源／项目组合，仍留候选。"
        }
        if preset == .verifiedSolutions && item.evidence != .preloadedVerified {
            return "没有与原内容相符的预置核验记录，仍留候选。"
        }
        return nil
    }

    @discardableResult
    func processNewCandidates() -> Bool {
        guard !hasProcessedNewCandidates else {
            return refuse("本次固定批次已处理；不重复添加，也不因后来改配置追溯入库。请重置后重演。")
        }
        hasProcessedNewCandidates = true
        var added = 0
        var admitted = 0
        for fixture in KnowledgeReviewFixtures.newBatch {
            // K3 can already contain exception rows. Do not resurrect a
            // discarded fixture or reevaluate any previously visible item.
            guard item(id: fixture.id) == nil else { continue }
            var candidate = fixture
            if automaticAdmissionReason(for: candidate) == nil {
                candidate.state = .reusable
                candidate.admissionOrigin = .automatic
                admitted += 1
            }
            items.append(candidate)
            added += 1
        }
        repairDetailSelection()
        notice = "已处理一次固定虚构批次：新增 \(added) 条，其中 \(admitted) 条按预设免逐条确认；其余仍候选。\(KnowledgeReviewFixtures.processNewNote)"
        return true
    }

    // MARK: - Reference projection, not retrieval or injection

    func setWork(_ id: KnowledgeReviewWorkID) {
        selectedWorkID = id
    }

    func setReferenceScenario(_ scenario: KnowledgeReviewReferenceScenario) {
        referenceScenario = scenario
    }

    var currentRequirement: String {
        if selectedWorkID == .storageTask && referenceScenario == .currentUserRequest {
            return "当前虚构用户明确要求：使用 Postgres；历史 SQLite 选择不得覆盖此要求。"
        }
        return "当前虚构要求：\(selectedWork.request)"
    }

    var referenceRows: [KnowledgeReviewReference] {
        selectedWork.relatedItemIDs.compactMap { id in
            guard let item = item(id: id),
                  item.applicability.project == .harbor,
                  item.state == .reusable || item.state == .retired else { return nil }
            var version = item.current
            let condition: KnowledgeReviewReferenceCondition
            let note: String
            if item.state == .retired {
                condition = .retired
                note = KnowledgeReviewFixtures.retirementNote
            } else {
                switch referenceScenario {
                case .sourced:
                    condition = version.source.isAvailable ? .reference : .missingSource
                    note = id == .databaseChoice
                        ? "仅解释历史 SQLite 单机原型的条件，不要求新任务照搬。"
                        : KnowledgeReviewFixtures.referenceBoundaryNote
                case .missingSource:
                    version.source = .init(
                        id: version.source.id, kind: version.source.kind,
                        title: version.source.title, excerpt: nil
                    )
                    condition = .missingSource
                    note = "虚构情景：来源摘录缺失，无法回源；不编造依据，也不去查真实历史。"
                case .currentUserRequest:
                    if id == .databaseChoice {
                        condition = .currentRequestPriority
                        note = "当前明确要求是 Postgres；SQLite 只保留为历史解释，不作为推荐方案。"
                    } else {
                        condition = .reference
                        note = "以当前虚构任务要求为准；参考不能升级为系统指令。"
                    }
                case .outdatedConflict:
                    condition = .outdatedConflict
                    note = "虚构情景：适用条件已变或与当前要求冲突，先待复核，不作为当前确定结论推荐。"
                }
            }
            return .init(itemID: id, itemTitle: item.title, version: version, condition: condition, note: note)
        }
    }

    var recommendedReferences: [KnowledgeReviewReference] { referenceRows.filter(\.isRecommended) }

    func capturedReferenceNote(_ reference: KnowledgeReviewCapturedReference) -> String {
        guard let current = item(id: reference.version.itemID) else {
            return "虚构旧引用 \(reference.version.title)；当前项不可用，不编造现行依据。"
        }
        let state = current.state == .retired ? "已停用" : "当前为 \(current.current.title)"
        return "已捕获的虚构旧引用 \(reference.version.title)（\(state)）；仅历史回看，不代表模型已清除或更正。"
    }

    // MARK: - One explicit v1 -> v2 correction and retirement

    var revisionBlockedReason: String? {
        guard let item = selectedItem else { return "请先选择虚构接口知识。" }
        guard item.id == .interfaceContract, item.state == .reusable else {
            return "本样例只演示已确认接口知识的用户纠正；候选或 Agent 猜测不能替代它。"
        }
        guard item.current.number == 1 else { return "本样例仅演示 v1 → v2；再次修订请先重置。" }
        return nil
    }

    func beginRevision() {
        if let reason = revisionBlockedReason {
            notice = reason
            return
        }
        guard let item = selectedItem else { return }
        clearDrafts()
        let id = UUID()
        activeRevisionDraftID = id
        revisionDraft = .init(
            id: id, generation: generation, originalVersion: item.current.id,
            content: KnowledgeReviewFixtures.correctionContent,
            applicability: KnowledgeReviewFixtures.correctionApplicability, basis: ""
        )
    }

    func cancelRevisionDraft() {
        revisionDraft = nil
        activeRevisionDraftID = nil
        notice = "已取消虚构用户纠正；没有生成 v2 或改动历史与在途引用。"
    }

    @discardableResult
    func applyRevisionDraft() -> Bool {
        guard let draft = revisionDraft,
              draft.id == activeRevisionDraftID, draft.generation == generation,
              draft.originalVersion == KnowledgeReviewFixtures.interfaceV1.id,
              let index = items.firstIndex(where: { $0.id == .interfaceContract }),
              items[index].state == .reusable,
              items[index].current.id == draft.originalVersion else {
            return refuse("修订草稿已失效或不是当前可纠正的 v1；未产生新版本。")
        }
        let content = trimmed(draft.content)
        let applicability = trimmed(draft.applicability)
        let basis = trimmed(draft.basis)
        guard !content.isEmpty, !applicability.isEmpty, !basis.isEmpty else {
            return refuse("修订内容、适用说明与用户纠正依据不能为空；未产生 v2。")
        }
        guard content != items[index].content || applicability != items[index].applicability.summary else {
            return refuse("修订需明确改变内容或适用说明；没有生成空变化版本。")
        }
        var old = items[index].current
        old.state = .superseded
        let new = KnowledgeReviewVersion(
            itemID: .interfaceContract, number: 2, content: content,
            source: .init(
                id: "fictional.user.interface.v2.draft", kind: .userCorrection,
                title: "虚构用户纠正 · 本窗口草稿（非实测）", excerpt: basis
            ),
            applicability: .init(
                project: old.applicability.project, summary: applicability, limits: old.applicability.limits
            ),
            evidence: .userStatement, state: .current, correctionBasis: basis, replaces: old.id
        )
        items[index].history.append(old)
        items[index].current = new
        items[index].admissionOrigin = .manual
        revisionDraft = nil
        activeRevisionDraftID = nil
        markInterfaceImpactsPending(version: new.id, retired: false)
        notice = "已在内存生成虚构用户纠正 v2；保留 v1、依据与替代关联。两条接口工作仍待局部核对。"
        return true
    }

    var retirementBlockedReason: String? {
        guard let item = selectedItem, item.state == .reusable else {
            return "请先选择当前可复用的虚构知识；候选或已停用项不能再次停用。"
        }
        guard item.applicability.project == .harbor else { return "不能操作其他项目知识。" }
        return nil
    }

    @discardableResult
    func retireSelectedItem() -> Bool {
        if let reason = retirementBlockedReason { return refuse(reason) }
        guard let selectedItem,
              let index = items.firstIndex(where: { $0.id == selectedItem.id }) else { return false }
        items[index].state = .retired
        items[index].current.state = .retired
        if selectedItem.id == .interfaceContract {
            markInterfaceImpactsPending(version: selectedItem.current.id, retired: true)
        }
        invalidateItemDrafts([selectedItem.id])
        // The actual retired item stays selected in history, not a different
        // reusable row with a misleading "retired" toast.
        section = .history
        selectedItemID = selectedItem.id
        notice = KnowledgeReviewFixtures.retirementNote
        return true
    }

    // MARK: - Local impact demonstration, not delivery or correction evidence

    @discardableResult
    func notify(_ id: KnowledgeReviewWorkID) -> Bool {
        guard let index = impactRows.firstIndex(where: { $0.id == id }),
              impactRows[index].isAffected, impactRows[index].state == .pending,
              impactRows[index].delivery == .notSent else {
            return refuse("当前行不需要新的通知演示；未更改局部核对状态。")
        }
        impactRows[index].delivery = .notified
        impactRows[index].note = "已演示发送虚构通知；仍待局部核对，不代表已纠正或撤销副作用。"
        notice = impactRows[index].note
        return true
    }

    @discardableResult
    func acknowledge(_ id: KnowledgeReviewWorkID) -> Bool {
        guard let index = impactRows.firstIndex(where: { $0.id == id }),
              impactRows[index].isAffected, impactRows[index].state == .pending,
              impactRows[index].delivery == .notified else {
            return refuse("只有已演示通知且仍待核对的接口行可演示 ACK；ACK 不是核验。")
        }
        impactRows[index].delivery = .acknowledged
        impactRows[index].note = "已演示虚构 ACK；仅表示收到示例通知，仍待局部核对，未完成纠正。"
        notice = impactRows[index].note
        return true
    }

    func preloadedCheckBlockedReason(for id: KnowledgeReviewWorkID) -> String? {
        checkBlockedReason(KnowledgeReviewFixtures.preloadedCheck, for: id)
    }

    @discardableResult
    func loadPreloadedCheck(for id: KnowledgeReviewWorkID) -> Bool {
        applyPreloadedCheck(KnowledgeReviewFixtures.preloadedCheck, to: id)
    }

    /// Testable boundary: even a structurally similar or wrong-version record
    /// cannot clear another affected row. Only the one embedded record exists.
    @discardableResult
    func applyPreloadedCheck(_ record: KnowledgeReviewCheckRecord, to id: KnowledgeReviewWorkID) -> Bool {
        if let reason = checkBlockedReason(record, for: id) { return refuse(reason) }
        guard let index = impactRows.firstIndex(where: { $0.id == id }) else { return false }
        impactRows[index].loadedRecord = record
        impactRows[index].state = .checkedPreloaded
        impactRows[index].note = "\(record.title)：\(record.summary)"
        notice = impactRows[index].note
        return true
    }

    @discardableResult
    func continueIndependentWork(_ id: KnowledgeReviewWorkID) -> Bool {
        guard id == .docsPackage,
              let index = impactRows.firstIndex(where: { $0.id == id }),
              !impactRows[index].isAffected, impactRows[index].state == .unaffected else {
            return refuse("仅未继续的独立安全文档可演示继续；不能越过受影响接口的局部核对。")
        }
        impactRows[index].state = .continuingDemo
        impactRows[index].note = "独立虚构文档已演示继续；仅改变内存展示，没有启动任务或重放操作。"
        notice = impactRows[index].note
        return true
    }

    // MARK: - Reset, stale draft guards and finite helpers

    func load(_ scenario: KnowledgeReviewScenario) {
        generation = UUID()
        self.scenario = scenario
        items = KnowledgeReviewFixtures.items(for: scenario)
        section = KnowledgeReviewFixtures.defaultSection(for: scenario)
        selectedItemID = KnowledgeReviewFixtures.defaultSelection(for: scenario)
        selectedCandidateIDs = []
        admissionConfiguration = .init()
        permission = .standard
        hasProcessedNewCandidates = false
        selectedWorkID = scenario == .references ? .storageTask : .apiTask
        referenceScenario = .sourced
        impactRows = KnowledgeReviewFixtures.impacts(for: scenario)
        clearDrafts()
        notice = "已载入 \(scenario.menuTitle)；重建全部虚构内存，旧草稿不能写回。"
    }

    func reset() {
        load(scenario)
        notice = "已重置 \(scenario.code) 的勾选、草稿、配置、授权示例、引用和版本；\(Self.sampleOnlyNotice)。"
    }

    private func clearDrafts() {
        contentDraft = nil
        configurationDraft = nil
        revisionDraft = nil
        activeContentDraftID = nil
        activeConfigurationDraftID = nil
        activeRevisionDraftID = nil
    }

    private func repairDetailSelection() {
        if !visibleItems.contains(where: { $0.id == selectedItemID }) {
            selectedItemID = visibleItems.first?.id
        }
    }

    private func finishBatch(_ ids: Set<KnowledgeReviewItemID>) {
        selectedCandidateIDs = []
        invalidateItemDrafts(ids)
        repairDetailSelection()
    }

    private func invalidateItemDrafts(_ ids: Set<KnowledgeReviewItemID>) {
        if let draft = contentDraft, ids.contains(draft.originalVersion.itemID) {
            contentDraft = nil
            activeContentDraftID = nil
        }
        if let draft = revisionDraft, ids.contains(draft.originalVersion.itemID) {
            revisionDraft = nil
            activeRevisionDraftID = nil
        }
    }

    private func markInterfaceImpactsPending(version: KnowledgeReviewVersionID, retired: Bool) {
        for index in impactRows.indices
        where impactRows[index].capturedReference?.version.itemID == .interfaceContract {
            impactRows[index].requiredVersion = version
            impactRows[index].state = .pending
            impactRows[index].delivery = .notSent
            impactRows[index].loadedRecord = nil
            impactRows[index].note = retired
                ? "虚构接口知识已停用；捕获的旧引用保留，仍需局部核对，不声称清除上下文。"
                : "虚构接口已纠正为 v2；捕获的 v1 引用保留，相关产物仍待局部核对。"
        }
    }

    private func checkBlockedReason(_ record: KnowledgeReviewCheckRecord, for id: KnowledgeReviewWorkID) -> String? {
        guard record == KnowledgeReviewFixtures.preloadedCheck else {
            return "不是本样例唯一的 PRELOADED 记录；不能伪造或改绑核对结果。"
        }
        guard record.workID == id,
              let row = impactRows.first(where: { $0.id == id }), row.isAffected else {
            return "预置记录只适用于 API 接口任务，不适用于客户端或独立文档。"
        }
        guard row.state == .pending else { return "该行不再等待此预置记录；没有重复执行检查。" }
        guard row.capturedReference?.version.id == record.capturedVersion,
              row.requiredVersion == record.applicableVersion,
              let item = item(id: record.applicableVersion.itemID),
              item.state == .reusable, item.current.state == .current,
              item.current.id == record.applicableVersion else {
            return "记录的知识项／旧引用／适用版本不匹配，或知识已停用；仍待核对。"
        }
        // A custom draft can also create a displayed v2. The fixed record
        // cannot verify different content merely because the number matches.
        guard item.content == KnowledgeReviewFixtures.interfaceV2.content,
              item.applicability == KnowledgeReviewFixtures.interfaceV2.applicability else {
            return "当前 v2 内容或适用条件不同于预置记录；不能沿用其示例核对结果。"
        }
        return nil
    }

    private func trimmed(_ value: String) -> String {
        value.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func refuse(_ reason: String) -> Bool {
        notice = reason
        return false
    }
}
#endif
