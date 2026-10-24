#if DEBUG
import XCTest
@testable import AeroricUI

/// Pure dictionaries, value fixtures and instance-local memory only.
/// No View/App/lifecycle construction or external resources are used here.
final class NativeUsageStatisticsReviewTests: XCTestCase {
    private static let oldKeys = [
        "AERORIC_NATIVE_TOOLS_CONNECTIONS_REVIEW",
        "AERORIC_NATIVE_TASK_DELIVERABLES_REVIEW",
        "AERORIC_NATIVE_TASK_COMMUNICATION_REVIEW",
        "AERORIC_NATIVE_WORK_ARRANGEMENTS_REVIEW",
        "AERORIC_NATIVE_SKILL_REVIEW",
        "AERORIC_NATIVE_KNOWLEDGE_REVIEW",
        "AERORIC_NATIVE_EVOLUTION_REVIEW",
        "AERORIC_NATIVE_VISUAL_REVIEW",
        "AERORIC_NATIVE_COLLABORATION_REVIEW",
    ]

    func testEntryAbsencePreservesEveryOldKeySubsetRegardlessOfValues() {
        let keys = NativeUsageStatisticsReviewEntry.otherReviewKeys
        XCTAssertEqual(NativeUsageStatisticsReviewEntry.flagKey,
                       "AERORIC_NATIVE_USAGE_STATISTICS_REVIEW")
        XCTAssertEqual(Set(keys), Set(Self.oldKeys))
        XCTAssertEqual(keys.count, Self.oldKeys.count)
        XCTAssertEqual(Set(keys).count, keys.count)
        for value in ["1", "0", "", "invalid"] {
            for mask in 0..<(1 << keys.count) {
                var input = ["AERORIC_ENV": "unparsed", "AERORIC_DATA_DIR": "unparsed"]
                for (index, key) in keys.enumerated() where mask & (1 << index) != 0 {
                    input[key] = value
                }
                XCTAssertEqual(NativeUsageStatisticsReviewEntry.decide(environment: input),
                               .inactive, "mask \(mask), old value \(value)")
            }
        }
    }

    func testEntryRequiresExactOneButDoesNotInterpretUnrelatedDictionaryValues() {
        let key = NativeUsageStatisticsReviewEntry.flagKey
        XCTAssertEqual(NativeUsageStatisticsReviewEntry.decide(environment: [key: "1"]), .review)
        for value in ["", "0", "01", "true", "2", " 1", "1 ", "1\n", "DEMO-INJECTED"] {
            XCTAssertEqual(NativeUsageStatisticsReviewEntry.decide(environment: [key: value]),
                           .refused([.invalidFlag]))
        }
        for value in ["", "0", "1", "not-a-directory", "DEMO-INJECTED"] {
            XCTAssertEqual(NativeUsageStatisticsReviewEntry.decide(environment: [
                key: "1", "AERORIC_ENV": value, "AERORIC_DATA_DIR": value,
            ]), .review)
        }
        let explanations = USReviewRefusalReason.allCases.map { $0.title + $0.detail }.joined()
        XCTAssertFalse(explanations.contains("DEMO-INJECTED"))
        XCTAssertFalse(explanations.isEmpty)
    }

    func testEntryRejectsEveryNonemptyOldKeySubsetIncludingZeroEmptyAndInvalid() {
        let keys = NativeUsageStatisticsReviewEntry.otherReviewKeys
        for value in ["1", "0", "", "invalid"] {
            for mask in 1..<(1 << keys.count) {
                var input = [NativeUsageStatisticsReviewEntry.flagKey: "1"]
                for (index, key) in keys.enumerated() where mask & (1 << index) != 0 {
                    input[key] = value
                }
                XCTAssertEqual(NativeUsageStatisticsReviewEntry.decide(environment: input),
                               .refused([.conflictingReview]), "mask \(mask), old value \(value)")
            }
        }
        var mixed = [NativeUsageStatisticsReviewEntry.flagKey: "1"]
        let values = ["1", "0", "", "invalid"]
        for (index, key) in keys.enumerated() { mixed[key] = values[index % values.count] }
        XCTAssertEqual(NativeUsageStatisticsReviewEntry.decide(environment: mixed),
                       .refused([.conflictingReview]))
    }

    func testOwnerFactoryIsLazyForInactiveAndRefusedAndRunsOnceForReview() {
        var constructions = 0
        let denied: [USReviewEntryDecision] = [
            .inactive, .refused([.invalidFlag]), .refused([.conflictingReview]),
            .refused([.invalidFlag, .conflictingReview]), .refused([]),
        ]
        for decision in denied {
            let token: Int? = NativeUsageStatisticsReviewEntry.constructOwner(decision: decision) {
                constructions += 1
                return 73
            }
            XCTAssertNil(token)
            XCTAssertEqual(constructions, 0)
        }
        let token = NativeUsageStatisticsReviewEntry.constructOwner(decision: .review) {
            constructions += 1
            return 73
        }
        XCTAssertEqual(token, 73)
        XCTAssertEqual(constructions, 1)
    }

    func testUS1HierarchyUsesOneCorpusAndCountsSharedUsageOnlyOnce() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let records = store.records
            let facts = store.protectedFacts
            XCTAssertEqual(store.summary.knownTokens, 3100)
            XCTAssertEqual(store.summary.recordCount, 4, "Unknown U is not a known-value record")
            XCTAssertEqual(Set(store.visibleDetails.map(\.id)), ["F", "R", "V", "D", "U"])
            XCTAssertEqual(store.records.first { $0.id == "U" }?.tokens.basis, .unknown)
            XCTAssertNil(store.records.first { $0.id == "U" }?.tokens.value)
            XCTAssertFalse(store.summary.missing.isEmpty)
            XCTAssertFalse(store.summary.hasSummaryGap)
            XCTAssertEqual(store.records.filter { $0.id == "S" }.count, 1)
            XCTAssertEqual(store.records.first { $0.id == "S" }?.tokens.value, 500)
            XCTAssertFalse(store.visibleDetails.contains { $0.id == "S" })

            store.selectScope(.project)
            XCTAssertEqual(store.summary.knownTokens, 3600)
            XCTAssertEqual(store.summary.recordCount, 5)
            XCTAssertEqual(store.visibleDetails.filter { $0.id == "S" }.count, 1)
            XCTAssertFalse(store.visibleDetails.contains { $0.id == "N" })
            store.selectScope(.global)
            XCTAssertEqual(store.summary.knownTokens, 3800)
            XCTAssertEqual(store.summary.recordCount, 6)
            XCTAssertEqual(store.visibleDetails.first { $0.id == "N" }?.tokens.value, 200)
            XCTAssertNil(store.visibleDetails.first { $0.id == "N" }?.project)
            XCTAssertEqual(Set(store.visibleDetails.map(\.executor)),
                           ["Codex", "Claude Code", "OMP", "DSH", "custom"])
            XCTAssertEqual(store.records, records)
            XCTAssertEqual(store.protectedFacts, facts)
        }
    }

    func testUS1EveryBreakdownIsAnAlternativeProjectionOfTheSameKnownSet() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let records = store.records
            let snapshots = store.snapshots
            store.selectScope(.global)
            let expected: [(USReviewDimension, [String: Int])] = [
                (.executor, ["Codex": 2200, "Claude Code": 600, "DSH": 300, "OMP": 500, "custom": 200]),
                (.role, ["主执行": 2200, "反思": 600, "委派": 300, "共享治理": 500, "缺角色": 200]),
                (.api, ["API-A": 2200, "API-B": 600, "API-C": 300, "API-D": 500, "缺 API": 200]),
                (.model, ["Model-A": 2200, "Model-B": 600, "Model-D": 300, "缺模型": 700]),
            ]
            for (dimension, groups) in expected {
                store.dimension = dimension
                XCTAssertEqual(Dictionary(uniqueKeysWithValues: store.breakdown.map { ($0.title, $0.tokens) }),
                               groups, dimension.rawValue)
                XCTAssertEqual(store.breakdown.reduce(0) { $0 + $1.tokens }, 3800)
                XCTAssertEqual(store.summary.knownTokens, 3800, "Do not add different dimensions together")
            }
            store.selectScope(.task)
            for dimension in USReviewDimension.allCases {
                store.dimension = dimension
                XCTAssertEqual(store.breakdown.reduce(0) { $0 + $1.tokens }, 3100)
            }
            XCTAssertEqual(store.records, records)
            XCTAssertEqual(store.snapshots, snapshots)
        }
    }

    func testUS1CacheIsIncludedInInputAndFailureAndChildRemainDistinct() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            XCTAssertEqual(store.fRecord?.input, 1000)
            XCTAssertEqual(store.fRecord?.cacheRead, 200)
            XCTAssertEqual(store.fRecord?.output, 400)
            XCTAssertEqual(store.summary.freshInput, 800)
            XCTAssertEqual(store.summary.cacheRead, 200)
            XCTAssertEqual(store.summary.output, 400)
            XCTAssertEqual(store.fRecord?.tokens.value, 1400)
            XCTAssertEqual(store.records.first { $0.id == "R" }?.tokens.value, 800)
            XCTAssertNotEqual(store.fRecord?.attempt, store.records.first { $0.id == "R" }?.attempt)
            XCTAssertEqual(store.records.first { $0.id == "D" }?.task, "T-A-child")
            XCTAssertEqual(store.summary.knownTokens, 3100, "D is already included, not added twice")
            XCTAssertEqual(store.trend.first { $0.id == "selected" }?.parts, [800, 400, 200, 1700])
            XCTAssertEqual(store.trend.first { $0.id == "selected" }?.total, 3100)
            XCTAssertNil(store.trend.first { $0.id == "missing-07" }?.parts)
            XCTAssertEqual(store.summary.knownTokens, 3100, "Adjacent-day samples are not selected-day usage")
        }
    }

    func testUS1DuplicateAndIndependentRetryAreIdempotentAndDoNotAlterControl() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let records = store.records
            let facts = store.protectedFacts
            let snapshots = store.snapshots
            store.receiveDuplicate()
            XCTAssertEqual(store.records, records)
            XCTAssertEqual(store.summary.knownTokens, 3100)
            XCTAssertEqual(store.dataRevision, 0)
            let duplicate = Self.state(store)
            store.receiveDuplicate()
            XCTAssertEqual(Self.state(store), duplicate)
            store.receiveRetry()
            XCTAssertEqual(store.summary.knownTokens, 3500)
            XCTAssertEqual(store.records.count, records.count + 1)
            XCTAssertEqual(store.records.filter { $0.id == "E-retry" }.count, 1)
            XCTAssertEqual(store.records.first { $0.id == "E-retry" }?.tokens.value, 400)
            XCTAssertEqual(store.records.filter { $0.id != "E-retry" }, records)
            let retry = Self.state(store)
            store.receiveRetry()
            XCTAssertEqual(Self.state(store), retry)
            XCTAssertEqual(store.protectedFacts, facts)
            XCTAssertEqual(store.snapshots, snapshots)
        }
    }

    func testUS1CurrentConfigurationAndNotStartedPresetDoNotRelabelHistory() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let records = store.records
            let facts = store.protectedFacts
            let summary = store.summary
            store.changeCurrentConfiguration()
            XCTAssertNotEqual(store.currentConfiguration, "Codex / API-A / Model-A")
            XCTAssertEqual(store.records, records)
            XCTAssertEqual(store.summary, summary)
            XCTAssertEqual(store.fRecord?.executor, "Codex")
            XCTAssertEqual(store.fRecord?.api, "API-A")
            XCTAssertEqual(store.fRecord?.model, "Model-A")
            store.loadPreset("US1/not-started")
            XCTAssertTrue(store.notStarted)
            XCTAssertEqual(store.records, records)
            XCTAssertEqual(store.summary, summary)
            XCTAssertTrue(store.eventIDs.isEmpty)
            XCTAssertEqual(store.protectedFacts, facts)
        }
    }

    func testUS2OriginalCurrenciesEstimatesUnknownFeesAndTrueZeroStaySeparate() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            XCTAssertEqual(store.costs.amounts.map(\.usageID), ["F", "V"])
            XCTAssertEqual(store.costs.amounts.first { $0.usageID == "F" }?.microUnits, 4600)
            XCTAssertEqual(store.costs.amounts.first { $0.usageID == "F" }?.currency, "USD")
            XCTAssertEqual(store.costs.amounts.first { $0.usageID == "V" }?.microUnits, 3000)
            XCTAssertEqual(store.costs.amounts.first { $0.usageID == "V" }?.currency, "EUR")
            XCTAssertTrue(store.costs.amounts.allSatisfy { $0.basis == .estimated })
            XCTAssertTrue(store.costs.reported.isEmpty)
            XCTAssertNil(store.costs.conversionUSD)
            XCTAssertFalse(store.costs.limitations.isEmpty)
            XCTAssertFalse(store.costs.subscription.isEmpty)
            for id in ["R", "D", "S", "N", "U", "Z"] {
                XCTAssertFalse(store.costs.amounts.contains { $0.usageID == id },
                               "\(id) has no justified fee; absence must not become a zero amount")
            }
            XCTAssertEqual(NativeUsageStatisticsReviewFixtures.zero.value, 0)
            XCTAssertEqual(NativeUsageStatisticsReviewFixtures.zero.basis, .observedZero)
            XCTAssertNil(store.records.first { $0.id == "U" }?.tokens.value)
            let amounts = store.costs.amounts
            let snapshots = store.snapshots
            let facts = store.protectedFacts
            store.showConversion = true
            XCTAssertEqual(store.costs.conversionUSD, 7900, "4600 USD + 3000 EUR × 1.10, in micro-USD")
            XCTAssertEqual(store.costs.amounts, amounts)
            XCTAssertTrue(store.costs.reported.isEmpty)
            XCTAssertEqual(store.snapshots, snapshots)
            XCTAssertEqual(store.protectedFacts, facts)
        }
    }

    func testUS2PriceRejectsMissingComponentsAndDoesNotUseSimilarModelPrices() throws {
        let price = NativeUsageStatisticsReviewFixtures.v1
        let f = try XCTUnwrap(NativeUsageStatisticsReviewFixtures.corpus().first { $0.id == "F" })
        XCTAssertEqual(price.estimate(f), 4600)
        for component in 0..<4 {
            var incomplete = f
            switch component {
            case 0: incomplete.input = nil
            case 1: incomplete.cacheRead = nil
            case 2: incomplete.output = nil
            default: incomplete.input = 100
            }
            XCTAssertNil(price.estimate(incomplete))
        }
        for record in NativeUsageStatisticsReviewFixtures.corpus() where record.id != "F" {
            XCTAssertNil(price.estimate(record), record.id)
        }
        let otherAPI = NativeUsageStatisticsReviewFixtures.usage(
            "F", tokens: 1400, task: "T-A", role: "主执行", executor: "Codex",
            api: "API-other", model: "Model-A", relation: "不同 API，无适用价格",
            minute: 1410, input: 1000, cache: 200, output: 400)
        let otherModel = NativeUsageStatisticsReviewFixtures.usage(
            "F", tokens: 1400, task: "T-A", role: "主执行", executor: "Codex",
            api: "API-A", model: "Model-other", relation: "不同模型，无适用价格",
            minute: 1410, input: 1000, cache: 200, output: 400)
        XCTAssertNil(price.estimate(otherAPI))
        XCTAssertNil(price.estimate(otherModel))
    }

    func testUS2PriceDraftCancelApplyAndRepeatedApplyPreserveOriginalEvidence() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            XCTAssertEqual(NativeUsageStatisticsReviewFixtures.customPrice.freshPerMillion, 3)
            XCTAssertEqual(NativeUsageStatisticsReviewFixtures.customPrice.cachePerMillion, 1)
            XCTAssertEqual(NativeUsageStatisticsReviewFixtures.customPrice.outputPerMillion, 8)
            let before = Self.state(store)
            store.beginPriceDraft()
            XCTAssertEqual(store.priceDraft?.preview, 5800)
            XCTAssertEqual(store.priceDraft?.context, store.context)
            XCTAssertEqual(store.priceDraft?.usageRevision, "A-r1")
            XCTAssertEqual(store.priceDraft?.revision, store.dataRevision)
            XCTAssertEqual(store.price, NativeUsageStatisticsReviewFixtures.v1)
            XCTAssertEqual(store.costs.amounts.first { $0.usageID == "F" }?.microUnits, 4600)
            store.cancelPriceDraft()
            XCTAssertEqual(Self.state(store), before)
            store.beginPriceDraft()
            store.applyPriceDraft()
            XCTAssertNil(store.priceDraft)
            XCTAssertEqual(store.price, NativeUsageStatisticsReviewFixtures.customPrice)
            XCTAssertEqual(store.costs.amounts.first { $0.usageID == "F" }?.microUnits, 5800)
            XCTAssertEqual(store.costs.amounts.first { $0.usageID == "F" }?.basis, .estimated)
            XCTAssertEqual(store.records, before.records)
            XCTAssertEqual(store.snapshots, before.snapshots)
            XCTAssertEqual(store.protectedFacts, before.facts)
            XCTAssertNil(store.reportedFCost)
            let applied = Self.state(store)
            store.applyPriceDraft()
            XCTAssertEqual(Self.state(store), applied)
        }
    }

    func testUS2AppliedPriceIsBoundToScopePeriodAndZoneNotAGlobalOverride() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            store.beginPriceDraft()
            store.applyPriceDraft()
            let taskContext = store.context
            store.selectScope(.project)
            XCTAssertEqual(store.price, NativeUsageStatisticsReviewFixtures.v1)
            XCTAssertEqual(store.costs.amounts.first { $0.usageID == "F" }?.microUnits, 4600)
            store.selectScope(.global)
            XCTAssertEqual(store.price, NativeUsageStatisticsReviewFixtures.v1)
            store.selectScope(.task)
            XCTAssertEqual(store.context, taskContext)
            XCTAssertEqual(store.price, NativeUsageStatisticsReviewFixtures.customPrice)
            store.selectPeriod(.rolling)
            XCTAssertEqual(store.price, NativeUsageStatisticsReviewFixtures.v1)
            store.selectPeriod(.day)
            store.selectZone(.plus8)
            XCTAssertEqual(store.price, NativeUsageStatisticsReviewFixtures.v1)
            store.selectZone(.utc)
            XCTAssertEqual(store.price, NativeUsageStatisticsReviewFixtures.customPrice)
        }
    }

    func testAllDraftsBecomeInvalidOnScopePeriodZoneOrDataRevisionChange() async {
        await MainActor.run {
            for change in 0..<6 {
                let store = NativeUsageStatisticsReviewStore()
                store.beginPriceDraft()
                store.beginCleanup(.details)
                store.beginQuotaDraft()
                XCTAssertNotNil(store.priceDraft)
                XCTAssertNotNil(store.cleanupDraft)
                XCTAssertNotNil(store.quotaDraft)
                switch change {
                case 0: store.selectScope(.project)
                case 1: store.selectPeriod(.rolling)
                case 2: store.selectZone(.plus8)
                case 3: store.receiveRetry()
                case 4: store.reviseOutput(up: true)
                default: store.receiveLateCost()
                }
                XCTAssertNil(store.priceDraft, "change \(change)")
                XCTAssertNil(store.cleanupDraft, "change \(change)")
                XCTAssertNil(store.quotaDraft, "change \(change)")
                let afterChange = Self.state(store)
                store.applyPriceDraft()
                store.applyCleanup()
                store.applyQuotaDraft()
                XCTAssertEqual(Self.state(store), afterChange, "Stale drafts must not apply: \(change)")
                XCTAssertEqual(store.price, NativeUsageStatisticsReviewFixtures.v1)
                XCTAssertTrue(store.removedDetailIDs.isEmpty)
                XCTAssertEqual(store.projectLimit, 3600)
            }
        }
    }

    func testUS2LateReportedCostDoesNotReplaceEstimateOrModifyOldSnapshot() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let records = store.records
            let summary = store.summary
            let snapshots = store.snapshots
            let facts = store.protectedFacts
            store.receiveLateCost()
            XCTAssertEqual(store.reportedFCost?.microUnits, 4800)
            XCTAssertEqual(store.reportedFCost?.currency, "USD")
            XCTAssertEqual(store.reportedFCost?.basis, .reported)
            XCTAssertEqual(store.costs.reported.map(\.microUnits), [4800])
            XCTAssertEqual(store.costs.amounts.first { $0.usageID == "F" }?.microUnits, 4600)
            XCTAssertEqual(store.costs.amounts.first { $0.usageID == "F" }?.basis, .estimated)
            XCTAssertEqual(store.records, records)
            XCTAssertEqual(store.summary, summary)
            XCTAssertEqual(store.snapshots, snapshots)
            XCTAssertEqual(store.protectedFacts, facts)
            let received = Self.state(store)
            store.receiveLateCost()
            XCTAssertEqual(Self.state(store), received)
            store.createSnapshot()
            XCTAssertEqual(store.snapshots.first, snapshots.first)
            XCTAssertEqual(store.snapshots.last?.costs.reported.map(\.microUnits), [4800])
            XCTAssertEqual(store.snapshots.last?.asOf, "2026-09-13 03:00 UTC")
        }
    }

    func testUS3UpAndDownRevisionReplaceTheSameUsageAndKeepOldSnapshots() async {
        await MainActor.run {
            for up in [true, false] {
                let store = NativeUsageStatisticsReviewStore()
                let records = store.records
                let snapshots = store.snapshots
                let facts = store.protectedFacts
                let originalHistory = store.revisionHistory
                store.reviseOutput(up: up)
                XCTAssertEqual(store.fRecord?.tokens.value, up ? 1500 : 1300)
                XCTAssertEqual(store.fRecord?.output, up ? 500 : 300)
                XCTAssertEqual(store.fRecord?.revision, up ? "A-r2" : "A-r2d")
                XCTAssertEqual(store.fRecord?.tokens.basis, .reported)
                XCTAssertEqual(store.summary.knownTokens, up ? 3200 : 3000)
                XCTAssertEqual(store.costs.amounts.first { $0.usageID == "F" }?.microUnits, up ? 5300 : 3900)
                XCTAssertEqual(store.records.count, records.count)
                XCTAssertEqual(store.records.filter { $0.id != "F" }, records.filter { $0.id != "F" })
                XCTAssertEqual(Array(store.revisionHistory.prefix(originalHistory.count)), originalHistory)
                XCTAssertEqual(store.snapshots, snapshots)
                XCTAssertEqual(store.protectedFacts, facts)
                let revised = Self.state(store)
                store.reviseOutput(up: up)
                XCTAssertEqual(Self.state(store), revised)
                store.createSnapshot()
                XCTAssertEqual(store.snapshots.first, snapshots.first)
                XCTAssertEqual(store.snapshots.first?.fTokens, 1400)
                XCTAssertEqual(store.snapshots.last?.fTokens, up ? 1500 : 1300)
                XCTAssertEqual(store.snapshots.last?.summary.knownTokens, up ? 3200 : 3000)
            }
        }
    }

    func testUS3ConflictingSourceDoesNotOverwriteAddOrVerifyExistingMetrics() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let before = Self.state(store)
            store.receiveConflict()
            XCTAssertFalse(store.conflictCandidates.isEmpty)
            XCTAssertEqual(store.records, before.records)
            XCTAssertEqual(store.summary, before.summary)
            XCTAssertEqual(store.costs, before.costs)
            XCTAssertEqual(store.snapshots, before.snapshots)
            XCTAssertEqual(store.protectedFacts, before.facts)
            XCTAssertEqual(store.successfulAsOf, before.asOf)
            XCTAssertEqual(store.fRecord?.revision, "A-r1")
            XCTAssertEqual(store.fRecord?.tokens.value, 1400)
            XCTAssertEqual(store.fRecord?.tokens.basis, .reported)
            let conflict = Self.state(store)
            store.receiveConflict()
            XCTAssertEqual(Self.state(store), conflict)
        }
    }

    func testUS3RefreshFailureRetainsOldValueAndAsOfWhileEmptyRemainsUnknown() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let before = Self.state(store)
            store.refresh()
            store.receiveRefreshFailure()
            XCTAssertTrue(store.refreshFailed)
            XCTAssertTrue(store.hasSuccessfulReport)
            XCTAssertEqual(store.summary, before.summary)
            XCTAssertEqual(store.successfulAsOf, before.asOf)
            XCTAssertEqual(store.records, before.records)
            XCTAssertEqual(store.snapshots, before.snapshots)
            XCTAssertEqual(store.execution, before.execution)
            XCTAssertEqual(store.protectedFacts, before.facts)
            store.refresh()
            XCTAssertTrue(store.refreshFailed, "Ordinary refresh cannot manufacture success")
            XCTAssertEqual(store.successfulAsOf, before.asOf)
            store.loadPreset("US3/failure-empty")
            XCTAssertTrue(store.refreshFailed)
            XCTAssertFalse(store.hasSuccessfulReport)
            XCTAssertNil(store.successfulAsOf)
            XCTAssertNil(store.summary.knownTokens)
            XCTAssertNil(store.summary.freshInput)
            XCTAssertNil(store.summary.cacheRead)
            XCTAssertNil(store.summary.output)
            XCTAssertEqual(store.summary.recordCount, 0)
            XCTAssertTrue(store.visibleDetails.isEmpty)
            XCTAssertTrue(store.costs.amounts.isEmpty)
            XCTAssertTrue(store.costs.reported.isEmpty)
            XCTAssertNil(store.costs.conversionUSD)
            XCTAssertTrue(store.snapshots.isEmpty)
            store.refresh()
            XCTAssertNil(store.summary.knownTokens)
            XCTAssertFalse(store.hasSuccessfulReport)
            XCTAssertEqual(store.protectedFacts, before.facts)
        }
    }

    func testUS3TimeWindowsOnlyIncludeTheEntireCumulativeInterval() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            XCTAssertFalse(store.records.contains { $0.id == "X" })
            store.loadPreset("US3/time-windows")
            let records = store.records
            let snapshots = store.snapshots
            let facts = store.protectedFacts
            XCTAssertEqual(store.records.first { $0.id == "X" }?.tokens.value, 900)
            XCTAssertEqual(store.summary.knownTokens, 3100)
            XCTAssertTrue(store.timeProjection.unallocatedRecordIDs.contains("X"))
            for zone in USReviewZone.allCases {
                store.selectZone(zone)
                XCTAssertEqual(store.summary.knownTokens, 3100, zone.rawValue)
                XCTAssertTrue(store.timeProjection.unallocatedRecordIDs.contains("X"))
            }
            store.selectZone(.utc)
            store.selectPeriod(.rolling)
            XCTAssertEqual(store.context.interval, 720..<2160)
            XCTAssertEqual(store.summary.knownTokens, 3100, "Overlap is not a reliable split of X")
            XCTAssertTrue(store.timeProjection.unallocatedRecordIDs.contains("X"))
            store.selectPeriod(.completeInterval)
            XCTAssertEqual(store.context.interval, -1440..<2880)
            XCTAssertEqual(store.summary.knownTokens, 4000, "3100 + X900, without an E-new100 event")
            XCTAssertFalse(store.timeProjection.unallocatedRecordIDs.contains("X"))
            XCTAssertEqual(store.records, records)
            XCTAssertEqual(store.snapshots, snapshots)
            XCTAssertEqual(store.protectedFacts, facts)
        }
    }

    func testUS3NewSnapshotWithoutSuccessfulReportDoesNotExposeBaselineF() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            store.loadPreset("US3/failure-empty")
            store.createSnapshot()
            let snapshot = store.snapshots.last
            XCTAssertNil(snapshot?.fTokens)
            XCTAssertNotEqual(snapshot?.fRevision, "A-r1")
            XCTAssertNil(snapshot?.asOf)
            XCTAssertNil(snapshot?.summary.knownTokens)
            XCTAssertTrue(snapshot?.costs.amounts.isEmpty == true)
            XCTAssertTrue(snapshot?.costs.reported.isEmpty == true)
            store.refresh()
            XCTAssertEqual(store.snapshots.last, snapshot)
        }
    }

    func testUS3TypedParallelDurationDistinguishesCumulativeElapsedAndObservationGaps() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            store.loadPreset("US3/time-windows")
            let facts = store.protectedFacts
            let projection = store.timeProjection
            XCTAssertEqual(projection.cumulativeSeconds, 1200)
            XCTAssertEqual(projection.elapsedSeconds, 600)
            XCTAssertNotEqual(projection.cumulativeSeconds, projection.elapsedSeconds)
            XCTAssertEqual(projection.interval, 0..<1440)
            XCTAssertFalse(projection.limitations.isEmpty)
            let dateOnly = USReviewOccurrence.dateOnly
            XCTAssertTrue(dateOnly.belongs(to: store.context))
            store.selectZone(.plus8)
            XCTAssertEqual(store.context.interval, 960..<2400)
            XCTAssertFalse(dateOnly.belongs(to: store.context), "Date precision must not become an invented instant")
            XCTAssertEqual(store.timeProjection.cumulativeSeconds, 1200)
            XCTAssertEqual(store.timeProjection.elapsedSeconds, 600)
            store.selectZone(.minus7)
            XCTAssertEqual(store.context.interval, 420..<1860)
            XCTAssertEqual(store.protectedFacts, facts)
        }
    }

    func testUS4GlobalUserFilterDoesNotExpandAnyRoleSummary() async {
        await MainActor.run {
            for role in USReviewRole.allCases {
                let store = NativeUsageStatisticsReviewStore()
                store.selectRole(role)
                let summary = store.roleSummary
                let facts = store.protectedFacts
                store.selectScope(.global)
                XCTAssertEqual(store.summary.knownTokens, 3800)
                XCTAssertEqual(store.roleSummary, summary)
                XCTAssertEqual(store.protectedFacts, facts)
                switch role {
                case .primary: XCTAssertEqual(summary.knownTokens, 3100)
                case .reflection: XCTAssertEqual(summary.knownTokens, 600)
                case .delegation: XCTAssertEqual(summary.knownTokens, 300)
                case .governance: XCTAssertNil(summary.knownTokens)
                }
            }
        }
    }

    func testUS4GovernanceGapRequestDoesNotLeakRestrictedNumbersOrGrantAccess() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            store.selectScope(.global)
            store.selectRole(.governance)
            let summary = store.roleSummary
            let facts = store.protectedFacts
            XCTAssertNil(summary.knownTokens)
            let text = summary.rows.joined()
            for restricted in ["P-A", "T-B", "API-A", "3800", "3,800", "3600", "3,600"] {
                XCTAssertFalse(text.contains(restricted), restricted)
            }
            store.requestBroaderScope()
            XCTAssertTrue(store.scopeRequest)
            XCTAssertFalse(store.preauthorized)
            XCTAssertEqual(store.roleSummary, summary)
            store.createPreview()
            XCTAssertEqual(store.preview?.role, .governance)
            let previewText = store.preview?.lines.joined() ?? ""
            for restricted in ["P-A", "T-B", "3800", "3,800", "3600", "3,600"] {
                XCTAssertFalse(previewText.contains(restricted), restricted)
            }
            XCTAssertEqual(store.protectedFacts, facts)
        }
    }

    func testUS4PreauthorizedCoverageDoesNotRequestTheSamePermissionAgain() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            store.selectRole(.governance)
            store.requestBroaderScope()
            store.loadPreauthorizedScope(true)
            XCTAssertTrue(store.preauthorized)
            XCTAssertFalse(store.scopeRequest)
            XCTAssertEqual(store.roleSummary.knownTokens, 3600)
            let authorized = store.roleSummary
            store.selectScope(.global)
            XCTAssertEqual(store.roleSummary, authorized)
            store.loadPreauthorizedScope(true)
            XCTAssertFalse(store.scopeRequest)
            XCTAssertEqual(store.roleSummary, authorized)
            XCTAssertNotEqual(store.roleSummary.knownTokens, store.summary.knownTokens,
                              "The grant does not include every global record")
            store.loadPreauthorizedScope(false)
            XCTAssertNil(store.roleSummary.knownTokens)
            XCTAssertFalse(store.scopeRequest)
        }
    }

    func testUS4PreviewUsesOnlyMinimalFieldsAndIsAValueCopy() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let facts = store.protectedFacts
            let expectedFields = ["指标", "期间", "原币", "来源", "覆盖", "修订口径"]
            store.createPreview()
            let original = store.preview
            XCTAssertEqual(original?.fields, expectedFields)
            XCTAssertEqual(original?.lines.count, expectedFields.count)
            XCTAssertTrue(store.exportIntents.isEmpty)
            let text = original?.lines.joined() ?? ""
            for rawField in ["attempt-F", "API-A", "Model-A", "demo-session-original", "demo-workspace"] {
                XCTAssertFalse(text.contains(rawField), rawField)
            }
            store.receiveRetry()
            XCTAssertEqual(store.preview, original, "Live metrics cannot silently rewrite an existing preview")
            store.includeSafeLabel = true
            store.createPreview()
            XCTAssertEqual(store.preview?.fields, expectedFields + ["安全角色标签"])
            XCTAssertEqual(original?.fields, expectedFields)
            XCTAssertEqual(original?.lines.first, "指标：3,100 Token")
            XCTAssertEqual(store.protectedFacts, facts)
        }
    }

    func testUS4PreviewCloseRoleChangeAndExportIntentsDoNotSendOrBroadenScope() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let records = store.records
            let facts = store.protectedFacts
            store.expressExportIntent(.file)
            store.expressExportIntent(.external)
            XCTAssertTrue(store.exportIntents.isEmpty, "An intent requires a deliberate preview")
            store.createPreview()
            store.expressExportIntent(.file)
            XCTAssertEqual(store.exportIntents, [.file])
            store.expressExportIntent(.file)
            XCTAssertEqual(store.exportIntents, [.file])
            store.expressExportIntent(.external)
            XCTAssertEqual(store.exportIntents, [.file, .external])
            XCTAssertFalse(store.preauthorized)
            store.closePreview()
            XCTAssertNil(store.preview)
            XCTAssertTrue(store.exportIntents.isEmpty)
            store.createPreview()
            store.expressExportIntent(.external)
            store.requestBroaderScope()
            store.selectRole(.reflection)
            XCTAssertNil(store.preview)
            XCTAssertTrue(store.exportIntents.isEmpty)
            XCTAssertFalse(store.scopeRequest)
            XCTAssertEqual(store.roleSummary.knownTokens, 600)
            XCTAssertEqual(store.records, records)
            XCTAssertEqual(store.protectedFacts, facts)
        }
    }

    func testUS4PreviewIncludesOnlyGrantedCurrencySourcesAndCapturedRevisions() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            store.createPreview()
            let original = store.preview
            let text = original?.lines.joined() ?? ""
            for basis in ["USD 0.0046", "EUR 0.0030", "估算", "A-r1", "示例价 v1"] {
                XCTAssertTrue(text.contains(basis), basis)
            }
            store.selectScope(.global)
            store.createPreview()
            XCTAssertEqual(store.preview, original, "User scope cannot select a different role grant or basis")
            store.selectScope(.task)
            store.receiveLateCost()
            store.reviseOutput(up: true)
            XCTAssertEqual(store.preview, original)
            store.createPreview()
            let revised = store.preview?.lines.joined() ?? ""
            for basis in ["USD 0.0053", "USD 0.0048", "A-r2", "C-r2", "未核对"] {
                XCTAssertTrue(revised.contains(basis), basis)
            }
            store.selectRole(.reflection)
            store.createPreview()
            let reflection = store.preview?.lines.joined() ?? ""
            XCTAssertTrue(reflection.contains("EUR 0.0030"))
            for excluded in ["USD 0.0048", "USD 0.0053", "A-r2", "C-r2"] {
                XCTAssertFalse(reflection.contains(excluded), excluded)
            }
            store.selectRole(.governance)
            store.selectScope(.global)
            store.createPreview()
            let restricted = store.preview?.lines.joined() ?? ""
            for excluded in ["USD", "EUR", "A-r1", "A-r2", "C-r2", "示例价 v1"] {
                XCTAssertFalse(restricted.contains(excluded), excluded)
            }
        }
    }

    func testUS5CleanupPreviewAndCancelPreserveTheEntireState() async {
        await MainActor.run {
            for layer in USReviewCleanupLayer.allCases {
                let store = NativeUsageStatisticsReviewStore()
                let before = Self.state(store)
                store.beginCleanup(layer)
                XCTAssertEqual(store.cleanupDraft?.context, store.context)
                XCTAssertEqual(store.cleanupDraft?.layer, layer)
                XCTAssertEqual(store.cleanupDraft?.revision, store.dataRevision)
                XCTAssertEqual(store.cleanupDraft?.recordIDs, ["F", "R", "V", "D", "U"])
                XCTAssertEqual(store.records, before.records)
                XCTAssertEqual(store.summary, before.summary)
                store.cancelCleanup()
                XCTAssertEqual(Self.state(store), before)
            }
        }
    }

    func testUS5DetailRemovalRetainsSummaryButCannotRecreateFInANewSnapshot() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let summary = store.summary
            let snapshots = store.snapshots
            let facts = store.protectedFacts
            store.beginCleanup(.details)
            store.applyCleanup()
            XCTAssertTrue(store.visibleDetails.isEmpty)
            XCTAssertEqual(store.removedDetailIDs, ["F", "R", "V", "D", "U"])
            XCTAssertTrue(store.removedSummaryIDs.isEmpty)
            XCTAssertEqual(store.summary, summary)
            XCTAssertEqual(store.snapshots, snapshots)
            XCTAssertEqual(store.protectedFacts, facts)
            let removed = Self.state(store)
            store.applyCleanup()
            XCTAssertEqual(Self.state(store), removed)
            store.refresh()
            XCTAssertTrue(store.visibleDetails.isEmpty)
            XCTAssertEqual(store.summary, summary)
            store.createSnapshot()
            XCTAssertEqual(store.snapshots.first, snapshots.first)
            XCTAssertNil(store.snapshots.last?.fTokens)
            XCTAssertNotEqual(store.snapshots.last?.fRevision, "A-r1")
            XCTAssertEqual(store.snapshots.last?.summary.knownTokens, 3100)
            XCTAssertFalse(store.sourceRows.contains { $0.hasPrefix("F：") })
        }
    }

    func testUS5SummaryRemovalIsAGapNotZeroAndOtherScopesKeepTheirOwnKnownPart() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let details = store.visibleDetails
            let facts = store.protectedFacts
            store.beginCleanup(.summary)
            store.applyCleanup()
            XCTAssertNil(store.summary.knownTokens)
            XCTAssertEqual(store.summary.recordCount, 0)
            XCTAssertTrue(store.summary.hasSummaryGap)
            XCTAssertFalse(store.summary.missing.isEmpty)
            XCTAssertNil(store.trend.first { $0.id == "selected" }?.parts)
            XCTAssertEqual(store.visibleDetails, details, "Summary and detail cleanup are distinct layers")
            store.createSnapshot()
            XCTAssertNil(store.snapshots.last?.fTokens)
            store.selectScope(.project)
            XCTAssertEqual(store.summary.knownTokens, 3600, "The task-only confirmation does not clear project summaries")
            XCTAssertFalse(store.summary.hasSummaryGap)
            store.selectScope(.global)
            XCTAssertEqual(store.summary.knownTokens, 3800, "Other retained scopes are not recomputed from task gaps")
            XCTAssertFalse(store.summary.hasSummaryGap)
            store.selectScope(.task)
            XCTAssertNil(store.summary.knownTokens)
            XCTAssertTrue(store.summary.hasSummaryGap)
            XCTAssertEqual(store.protectedFacts, facts)
        }
    }

    func testUS5SummaryCleanupIsBoundToEachConfirmedScopePeriodAndZone() async {
        await MainActor.run {
            for scope in USReviewScope.allCases {
                let store = NativeUsageStatisticsReviewStore()
                store.selectScope(scope)
                store.beginCleanup(.summary)
                store.applyCleanup()
                let cleared = store.context
                XCTAssertNil(store.summary.knownTokens)
                XCTAssertTrue(store.summary.hasSummaryGap)
                for other in USReviewScope.allCases where other != scope {
                    store.selectScope(other)
                    XCTAssertEqual(store.summary.knownTokens, other == .task ? 3100 : other == .project ? 3600 : 3800)
                    XCTAssertFalse(store.summary.hasSummaryGap)
                }
                store.selectScope(scope)
                store.selectPeriod(.rolling)
                XCTAssertNotNil(store.summary.knownTokens)
                XCTAssertFalse(store.summary.hasSummaryGap)
                store.selectPeriod(.day)
                store.selectZone(.plus8)
                XCTAssertNotNil(store.summary.knownTokens)
                XCTAssertFalse(store.summary.hasSummaryGap)
                store.selectZone(.utc)
                XCTAssertEqual(store.context, cleared)
                XCTAssertNil(store.summary.knownTokens)
                XCTAssertTrue(store.summary.hasSummaryGap)
                store.reset()
                store.selectScope(scope)
                XCTAssertFalse(store.summary.hasSummaryGap)
                XCTAssertTrue(store.removedSummaryIDs.isEmpty)
            }
        }
    }

    func testUS5AfterBothLayersAreClearedNewReportAddsOnly100AndTrendKeepsGap() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let facts = store.protectedFacts
            let originalSnapshot = store.snapshots.first
            store.beginCleanup(.details)
            store.applyCleanup()
            store.beginCleanup(.summary)
            store.applyCleanup()
            XCTAssertNil(store.summary.knownTokens)
            XCTAssertTrue(store.summary.hasSummaryGap)
            store.refresh()
            XCTAssertTrue(store.visibleDetails.isEmpty)
            XCTAssertNil(store.summary.knownTokens)
            store.receiveNewReport()
            XCTAssertEqual(store.visibleDetails.map(\.id), ["E-new"])
            XCTAssertEqual(store.summary.knownTokens, 100)
            XCTAssertEqual(store.summary.recordCount, 1)
            XCTAssertTrue(store.summary.hasSummaryGap)
            XCTAssertNil(store.trend.first { $0.id == "selected" }?.parts,
                         "A known new part must not fill a historical trend gap")
            XCTAssertNil(store.trend.first { $0.id == "selected" }?.total)
            let newReport = Self.state(store)
            store.receiveNewReport()
            XCTAssertEqual(Self.state(store), newReport)
            store.refresh()
            XCTAssertEqual(store.visibleDetails.map(\.id), ["E-new"])
            store.createSnapshot()
            XCTAssertNil(store.snapshots.last?.fTokens)
            XCTAssertEqual(store.snapshots.last?.summary.knownTokens, 100)
            XCTAssertTrue(store.snapshots.last?.summary.hasSummaryGap == true)
            XCTAssertEqual(store.snapshots.first, originalSnapshot)
            XCTAssertEqual(store.protectedFacts, facts)
        }
    }

    func testUS5NewReportAfterDetailOnlyCleanupDoesNotReloadOldDetails() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            store.loadPreset("US5/details-removed")
            let facts = store.protectedFacts
            store.receiveNewReport()
            XCTAssertEqual(store.visibleDetails.map(\.id), ["E-new"])
            XCTAssertEqual(store.summary.knownTokens, 3200)
            XCTAssertFalse(store.summary.hasSummaryGap)
            store.refresh()
            XCTAssertEqual(store.visibleDetails.map(\.id), ["E-new"])
            XCTAssertEqual(store.protectedFacts, facts)
        }
    }

    func testUS5ProjectCleanupAndProtectedRecordsDoNotAffectUnrelatedOrControlFacts() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let records = store.records
            let facts = store.protectedFacts
            let resources = store.resourceProjection
            let snapshots = store.snapshots
            store.beginCleanup(.protectedRecords)
            store.applyCleanup()
            XCTAssertEqual(store.records, records)
            XCTAssertTrue(store.removedDetailIDs.isEmpty)
            XCTAssertTrue(store.removedSummaryIDs.isEmpty)
            XCTAssertEqual(store.resourceProjection, resources)
            XCTAssertEqual(store.snapshots, snapshots)
            XCTAssertEqual(store.protectedFacts, facts)
            store.selectScope(.project)
            store.beginCleanup(.details)
            XCTAssertEqual(store.cleanupDraft?.recordIDs, ["F", "R", "V", "D", "S", "U"])
            store.applyCleanup()
            store.selectScope(.global)
            XCTAssertEqual(store.visibleDetails.map(\.id), ["N"])
            XCTAssertEqual(store.summary.knownTokens, 3800)
            XCTAssertEqual(store.protectedFacts, facts)
            XCTAssertEqual(store.resourceProjection, resources)
        }
    }

    func testUS6WarningDisplayUnknownRequiredUnknownAndCapabilityHaveDistinctAdmission() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let facts = store.protectedFacts
            XCTAssertEqual(store.resourceProjection.projectKnown, 3600)
            XCTAssertEqual(store.resourceProjection.projectLimit, 3600)
            XCTAssertEqual(store.resourceProjection.taskKnown, 3100)
            XCTAssertEqual(store.resourceProjection.taskLimit, 5000)
            XCTAssertTrue(store.resourceProjection.newAdmissionBlocked)
            XCTAssertFalse(store.resourceProjection.independentWork.isEmpty)
            store.selectResourcePreset(.warning)
            XCTAssertFalse(store.resourceProjection.newAdmissionBlocked)
            XCTAssertTrue(store.resourceProjection.reasons.isEmpty)
            store.selectResourcePreset(.displayUnknown)
            XCTAssertNil(store.resourceProjection.projectKnown)
            XCTAssertNil(store.resourceProjection.taskKnown)
            XCTAssertFalse(store.resourceProjection.newAdmissionBlocked)
            store.selectResourcePreset(.requiredUnknown)
            XCTAssertNil(store.resourceProjection.projectKnown)
            XCTAssertTrue(store.resourceProjection.newAdmissionBlocked)
            let requiredReasons = store.resourceProjection.reasons
            XCTAssertFalse(requiredReasons.isEmpty)
            store.selectResourcePreset(.capability)
            XCTAssertTrue(store.resourceProjection.newAdmissionBlocked)
            XCTAssertNotEqual(store.resourceProjection.reasons, requiredReasons)
            XCTAssertEqual(store.protectedFacts, facts)
        }
    }

    func testUS6ConcurrencyOnlyBlocksNewAdmissionAndCannotConfirmAnUnrequestedStop() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            store.selectResourcePreset(.concurrency)
            let facts = store.protectedFacts
            XCTAssertEqual(store.execution, .running)
            XCTAssertTrue(store.resourceProjection.newAdmissionBlocked)
            store.refresh()
            store.simulateWinding()
            store.simulateStopVerification(known: true)
            XCTAssertEqual(store.execution, .running)
            store.beginQuotaDraft()
            store.applyQuotaDraft()
            XCTAssertEqual(store.execution, .running)
            XCTAssertTrue(store.resourceProjection.newAdmissionBlocked,
                          "Raising a token limit cannot free a concurrency slot")
            XCTAssertEqual(store.protectedFacts, facts)
        }
    }

    func testUS6StopRequestWindingAndKnownOrUnknownResultsRequireExplicitEvents() async {
        await MainActor.run {
            for known in [true, false] {
                let store = NativeUsageStatisticsReviewStore()
                store.selectResourcePreset(.warning)
                let facts = store.protectedFacts
                store.simulateLimitRequest()
                XCTAssertEqual(store.execution, .stopRequested)
                XCTAssertTrue(store.resourceProjection.newAdmissionBlocked)
                store.refresh()
                store.recheckAdmission()
                XCTAssertEqual(store.execution, .stopRequested)
                XCTAssertFalse(store.admissionCanContinue)
                store.simulateWinding()
                XCTAssertEqual(store.execution, .winding)
                XCTAssertTrue(store.resourceProjection.newAdmissionBlocked)
                store.simulateStopVerification(known: known)
                XCTAssertEqual(store.execution, known ? .stopped : .unknown)
                if !known {
                    XCTAssertTrue(store.resourceProjection.newAdmissionBlocked)
                    store.simulateStopVerification(known: true)
                    XCTAssertEqual(store.execution, .unknown, "Unknown is not silently promoted to stopped")
                }
                XCTAssertEqual(store.protectedFacts, facts)
            }
        }
    }

    func testUS6QuotaCancelApplyAndRepeatedApplyDoNotRestartOrClearStopAndRetry() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let before = Self.state(store)
            store.beginQuotaDraft()
            XCTAssertEqual(store.quotaDraft?.context, store.context)
            XCTAssertEqual(store.quotaDraft?.previousLimit, 3600)
            XCTAssertEqual(store.quotaDraft?.proposedLimit, 4500)
            XCTAssertEqual(store.projectLimit, 3600)
            store.cancelQuotaDraft()
            XCTAssertEqual(Self.state(store), before)
            store.beginQuotaDraft()
            store.applyQuotaDraft()
            XCTAssertEqual(store.projectLimit, 4500)
            XCTAssertEqual(store.execution, before.execution)
            XCTAssertEqual(store.continuationBlocks, [.userStop])
            XCTAssertFalse(store.admissionCanContinue)
            XCTAssertEqual(store.records, before.records)
            XCTAssertEqual(store.snapshots, before.snapshots)
            XCTAssertEqual(store.protectedFacts, before.facts)
            let applied = Self.state(store)
            store.applyQuotaDraft()
            XCTAssertEqual(Self.state(store), applied)
            store.selectScope(.global)
            store.beginQuotaDraft()
            XCTAssertNil(store.quotaDraft, "The finite quota editor cannot silently target a project from global")
        }
    }

    func testUS6ContinueAndTopUpContinueAreDistinctUnexecutedIntents() async {
        await MainActor.run {
            let ordinary = NativeUsageStatisticsReviewStore()
            let topUp = NativeUsageStatisticsReviewStore()
            let facts = ordinary.protectedFacts
            ordinary.expressContinue()
            XCTAssertTrue(ordinary.continueIntent)
            XCTAssertFalse(ordinary.topUpContinueIntent)
            XCTAssertNil(ordinary.quotaDraft)
            topUp.expressContinue(topUp: true)
            XCTAssertTrue(topUp.topUpContinueIntent)
            XCTAssertFalse(topUp.continueIntent)
            XCTAssertNotNil(topUp.quotaDraft)
            for store in [ordinary, topUp] {
                XCTAssertEqual(store.projectLimit, 3600)
                XCTAssertEqual(store.execution, .stopped)
                XCTAssertFalse(store.admissionCanContinue)
                store.recheckAdmission()
                XCTAssertFalse(store.admissionCanContinue)
                XCTAssertTrue(store.continuationReasons.contains(USReviewContinuationBlock.userStop.title))
                XCTAssertEqual(store.protectedFacts, facts)
            }
        }
    }

    func testUS6AutomaticAndOrdinaryWaitSuccessAreAdmissionNotExecutionOrVerification() async {
        await MainActor.run {
            for policy in [USReviewContinuePolicy.automatic, .ordinaryWait] {
                let store = NativeUsageStatisticsReviewStore()
                let records = store.records
                let facts = store.protectedFacts
                store.loadContinuationFixture(policy: policy, block: nil)
                XCTAssertTrue(store.continuationReasons.isEmpty)
                XCTAssertFalse(store.admissionCanContinue, "Complete recheck is still required")
                XCTAssertFalse(store.continueIntent)
                XCTAssertFalse(store.topUpContinueIntent)
                XCTAssertFalse(store.scopeRequest)
                store.recheckAdmission()
                XCTAssertTrue(store.admissionCanContinue)
                XCTAssertTrue(store.continuationReasons.isEmpty)
                XCTAssertEqual(store.execution, .stopped)
                XCTAssertEqual(store.records, records)
                XCTAssertEqual(store.protectedFacts, facts)
                XCTAssertEqual(store.resourceProjection.verification,
                               facts.supervision + " / " + facts.acceptance)
                XCTAssertFalse(store.scopeRequest)
                let rechecked = Self.state(store)
                store.recheckAdmission()
                XCTAssertEqual(Self.state(store), rechecked)
            }
        }
    }

    func testUS6ManualContinuationNeedsExplicitIntentAndASeparateRecheck() async {
        await MainActor.run {
            for topUp in [false, true] {
                let store = NativeUsageStatisticsReviewStore()
                store.loadContinuationFixture(policy: .manual, block: nil)
                let facts = store.protectedFacts
                XCTAssertEqual(store.continuationReasons, [USReviewContinuationBlock.missingContinue.title])
                store.recheckAdmission()
                XCTAssertFalse(store.admissionCanContinue)
                store.expressContinue(topUp: topUp)
                XCTAssertFalse(store.admissionCanContinue, "An intent is not a successful recheck")
                XCTAssertTrue(store.continuationReasons.isEmpty)
                store.recheckAdmission()
                XCTAssertTrue(store.admissionCanContinue)
                XCTAssertEqual(store.execution, .stopped)
                XCTAssertEqual(store.protectedFacts, facts)
            }
        }
    }

    func testUS6EachContinuationBlockIndependentlyDefeatsTheSameSuccessfulBaseline() async {
        await MainActor.run {
            let baseline = NativeUsageStatisticsReviewStore()
            baseline.loadContinuationFixture(policy: .automatic, block: nil)
            baseline.recheckAdmission()
            XCTAssertTrue(baseline.admissionCanContinue)
            for block in USReviewContinuationBlock.allCases {
                let store = NativeUsageStatisticsReviewStore()
                store.loadContinuationFixture(policy: .automatic, block: block)
                XCTAssertEqual(store.records, baseline.records)
                XCTAssertEqual(store.projectLimit, baseline.projectLimit)
                XCTAssertEqual(store.execution, baseline.execution)
                XCTAssertEqual(store.protectedFacts, baseline.protectedFacts)
                XCTAssertEqual(store.continuationBlocks, [block])
                XCTAssertEqual(store.continuationReasons, [block.title])
                store.recheckAdmission()
                XCTAssertFalse(store.admissionCanContinue, block.rawValue)
                store.expressContinue()
                store.expressContinue(topUp: true)
                store.applyQuotaDraft()
                store.refresh()
                store.recheckAdmission()
                XCTAssertEqual(store.continuationReasons, [block.title],
                               "Recent generic intents must not erase a specific blocker")
                XCTAssertFalse(store.admissionCanContinue, block.rawValue)
                XCTAssertEqual(store.execution, .stopped)
                XCTAssertEqual(store.protectedFacts, baseline.protectedFacts)
            }
        }
    }

    func testUS6NewRequiredResourceOrUnsafeOriginalExecutionInvalidatesPriorAdmission() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            store.loadContinuationFixture(policy: .automatic, block: nil)
            store.recheckAdmission()
            XCTAssertTrue(store.admissionCanContinue)
            store.selectResourcePreset(.requiredUnknown)
            XCTAssertFalse(store.admissionCanContinue)
            store.recheckAdmission()
            XCTAssertFalse(store.admissionCanContinue)
            XCTAssertFalse(store.resourceProjection.reasons.isEmpty)
            store.loadContinuationFixture(policy: .automatic, block: nil)
            store.recheckAdmission()
            store.simulateLimitRequest()
            XCTAssertFalse(store.admissionCanContinue)
            store.simulateWinding()
            store.simulateStopVerification(known: false)
            store.recheckAdmission()
            XCTAssertFalse(store.admissionCanContinue)
            XCTAssertTrue(store.continuationReasons.contains(USReviewContinuationBlock.originalExecution.title))
            XCTAssertEqual(store.execution, .unknown)
        }
    }

    func testResetRestoresFixedValuesAndDoesNotAffectAnotherInstance() async {
        await MainActor.run {
            let changed = NativeUsageStatisticsReviewStore()
            let untouched = NativeUsageStatisticsReviewStore()
            let untouchedBefore = Self.state(untouched)
            let baseline = NativeUsageStatisticsReviewStore()
            baseline.loadPreset("US1/baseline")
            Self.contaminate(changed)
            XCTAssertNotEqual(changed.records, untouched.records)
            XCTAssertEqual(Self.state(untouched), untouchedBefore)
            changed.reset()
            XCTAssertEqual(Self.state(changed), Self.state(baseline))
            XCTAssertEqual(Self.state(untouched), untouchedBefore)
            changed.reset()
            XCTAssertEqual(Self.state(changed), Self.state(baseline))
            changed.selectScope(.project)
            XCTAssertEqual(changed.price, NativeUsageStatisticsReviewFixtures.v1,
                           "Reset must also remove reference prices in a previously visited context")
        }
    }

    func testEveryApprovedPresetLoadsIndependentlyOfPriorDraftsEventsAndCleanup() async {
        await MainActor.run {
            let presets = NativeUsageStatisticsReviewFixtures.presets
            XCTAssertEqual(presets.count, 59)
            XCTAssertEqual(Set(presets.map(\.id)).count, presets.count)
            XCTAssertEqual(Set(presets.map(\.group)), ["US1", "US2", "US3", "US4", "US5", "US6"])
            for preset in presets {
                let reused = NativeUsageStatisticsReviewStore()
                let fresh = NativeUsageStatisticsReviewStore()
                Self.contaminate(reused)
                reused.loadPreset(preset.id)
                fresh.loadPreset(preset.id)
                XCTAssertEqual(reused.presetID, preset.id)
                XCTAssertEqual(Self.state(reused), Self.state(fresh), preset.id)
                let freshState = Self.state(fresh)
                reused.receiveRetry()
                reused.requestBroaderScope()
                reused.createPreview()
                reused.beginCleanup(.summary)
                XCTAssertEqual(Self.state(fresh), freshState, "No shared mutable state: \(preset.id)")
            }
        }
    }

    func testPresentationSystemPreferencesCannotBeWeakenedAndDoNotChangeFacts() async {
        await MainActor.run {
            let store = NativeUsageStatisticsReviewStore()
            let records = store.records
            let facts = store.protectedFacts
            let context = store.context
            let resources = store.resourceProjection
            let snapshots = store.snapshots
            for mask in 0..<(1 << 7) {
                let classic = mask & 1 != 0
                let systemTransparency = mask & 2 != 0
                let systemContrast = mask & 4 != 0
                let systemMotion = mask & 8 != 0
                let demoTransparency = mask & 16 != 0
                let demoContrast = mask & 32 != 0
                let demoMotion = mask & 64 != 0
                store.classic = classic
                store.demoTransparency = demoTransparency
                store.demoContrast = demoContrast
                store.demoMotion = demoMotion
                let result = USReviewPresentation.resolve(
                    classic: store.classic, systemTransparency: systemTransparency,
                    systemContrast: systemContrast, systemMotion: systemMotion,
                    demoTransparency: store.demoTransparency, demoContrast: store.demoContrast,
                    demoMotion: store.demoMotion)
                XCTAssertEqual(result.opaque, classic || systemTransparency || demoTransparency)
                XCTAssertEqual(result.strongContrast, systemContrast || demoContrast)
                XCTAssertEqual(result.disableAnimation, systemMotion || demoMotion)
                XCTAssertEqual(store.records, records)
                XCTAssertEqual(store.protectedFacts, facts)
                XCTAssertEqual(store.context, context)
                XCTAssertEqual(store.resourceProjection, resources)
                XCTAssertEqual(store.snapshots, snapshots)
            }
        }
    }

    // These value captures deliberately include both live projections and their
    // underlying local facts; a title/counter alone cannot prove instance isolation.
    private struct ReviewState: Equatable {
        let preset: String
        let context: USReviewContext
        let dimension: USReviewDimension
        let presentation: [Bool]
        let records: [USReviewUsage]
        let summary: USReviewSummary
        let breakdown: [USReviewBreakdown]
        let costs: USReviewCosts
        let trend: [USReviewTrendBar]
        let time: USReviewTimeProjection
        let revision: Int
        let events: Set<String>
        let history: [String]
        let conflicts: [String]
        let asOf: String?
        let failed: Bool
        let successful: Bool
        let price: USReviewPrice
        let priceDraft: USReviewPriceDraft?
        let reportedCost: USReviewMoney?
        let snapshots: [USReviewSnapshot]
        let configuration: String
        let notStarted: Bool
        let role: USReviewRoleSummary
        let preauthorized: Bool
        let scopeRequest: Bool
        let preview: USReviewPreview?
        let exportIntents: [USReviewExportIntent]
        let safeLabel: Bool
        let cleanupDraft: USReviewCleanupDraft?
        let removedDetails: Set<String>
        let removedSummaries: Set<String>
        let cleanupResult: String
        let resourcePreset: USReviewResourcePreset
        let resources: USReviewResourceProjection
        let quotaDraft: USReviewQuotaDraft?
        let execution: USReviewExecution
        let policy: USReviewContinuePolicy
        let blocks: Set<USReviewContinuationBlock>
        let continueIntent: Bool
        let topUpIntent: Bool
        let rechecked: Bool
        let canContinue: Bool
        let reasons: [String]
        let facts: USReviewProtectedFacts
        let message: String
    }

    @MainActor
    private static func state(_ store: NativeUsageStatisticsReviewStore) -> ReviewState {
        .init(
            preset: store.presetID, context: store.context, dimension: store.dimension,
            presentation: [store.showConversion, store.classic, store.demoTransparency,
                           store.demoContrast, store.demoMotion],
            records: store.records, summary: store.summary, breakdown: store.breakdown,
            costs: store.costs, trend: store.trend, time: store.timeProjection,
            revision: store.dataRevision, events: store.eventIDs, history: store.revisionHistory,
            conflicts: store.conflictCandidates, asOf: store.successfulAsOf,
            failed: store.refreshFailed, successful: store.hasSuccessfulReport,
            price: store.price, priceDraft: store.priceDraft, reportedCost: store.reportedFCost,
            snapshots: store.snapshots, configuration: store.currentConfiguration, notStarted: store.notStarted,
            role: store.roleSummary, preauthorized: store.preauthorized, scopeRequest: store.scopeRequest,
            preview: store.preview, exportIntents: store.exportIntents, safeLabel: store.includeSafeLabel,
            cleanupDraft: store.cleanupDraft, removedDetails: store.removedDetailIDs,
            removedSummaries: store.removedSummaryIDs, cleanupResult: store.cleanupResult,
            resourcePreset: store.resourcePreset, resources: store.resourceProjection, quotaDraft: store.quotaDraft,
            execution: store.execution, policy: store.continuePolicy, blocks: store.continuationBlocks,
            continueIntent: store.continueIntent, topUpIntent: store.topUpContinueIntent,
            rechecked: store.admissionRechecked, canContinue: store.admissionCanContinue,
            reasons: store.continuationReasons, facts: store.protectedFacts, message: store.actionMessage)
    }

    @MainActor
    private static func contaminate(_ store: NativeUsageStatisticsReviewStore) {
        store.beginPriceDraft()
        store.applyPriceDraft()
        store.selectScope(.project)
        store.beginPriceDraft()
        store.applyPriceDraft()
        store.receiveRetry()
        store.receiveLateCost()
        store.reviseOutput(up: true)
        store.receiveConflict()
        store.changeCurrentConfiguration()
        store.dimension = .model
        store.showConversion = true
        store.selectRole(.governance)
        store.loadPreauthorizedScope(true)
        store.requestBroaderScope()
        store.includeSafeLabel = true
        store.createPreview()
        store.expressExportIntent(.file)
        store.expressExportIntent(.external)
        store.beginCleanup(.details)
        store.applyCleanup()
        store.beginCleanup(.summary)
        store.applyCleanup()
        store.createSnapshot()
        store.receiveRefreshFailure()
        store.loadContinuationFixture(policy: .automatic, block: .retryLimit)
        store.expressContinue(topUp: true)
        store.recheckAdmission()
        store.beginCleanup(.protectedRecords)
        store.selectPeriod(.rolling)
        store.selectZone(.plus8)
    }
}
#endif
