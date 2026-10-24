import XCTest
@testable import AeroricUI

/// Shell policy tests (P4-04): the pure `DetailState.resolve` mapping and
/// the pure `filterCommands` query filter. No AppKit session is needed —
/// both are plain value-level decisions.
final class ShellPolicyTests: XCTestCase {
    // MARK: DetailState.resolve — the four shell states are deliberately
    // separate (P4-04 Action 5): loading never looks like "no projects".

    func testResolveIdleAndConnectingAreLoading() {
        XCTAssertEqual(DetailState.resolve(.idle), .loading)
        XCTAssertEqual(DetailState.resolve(.connecting), .loading)
    }

    func testResolveReadyIsEmpty() {
        XCTAssertEqual(DetailState.resolve(.ready), .empty)
    }

    func testResolveDegradedCarriesReason() {
        XCTAssertEqual(
            DetailState.resolve(.degraded(reason: "spawn failed")),
            .disconnected(reason: "spawn failed")
        )
    }

    func testResolveReconnectingIsDisconnectedWithoutReason() {
        XCTAssertEqual(DetailState.resolve(.reconnecting), .disconnected(reason: nil))
    }

    func testResolveStoppedIsDisconnected() {
        XCTAssertEqual(DetailState.resolve(.stopped), .disconnected(reason: nil))
    }

    func testResolveIncompatibleIsDistinctError() {
        XCTAssertEqual(
            DetailState.resolve(.incompatible(reason: "wire v9")),
            .incompatible(reason: "wire v9")
        )
    }

    // MARK: DiagnosticEnvironment — developer scaffolding vs product surface

    /// 只有 `ui-test` 渲染开发者脚手架(五行的 bootstrap 块、状态栏的环境标签)。
    ///
    /// 反事实(2026-10-13,实测):第一版把 `dev` 也算进来,理由是"开发时想看到
    /// 自己在跑哪个构建"。但 `native:dev` 正是**交互式运行应用的方式**,于是那
    /// 五行与 `dev` 标签成了正在被重做的那个界面里的常驻家具——用户报的"杂乱"
    /// 就包含它。UI 测试全部跑在 `ui-test`(`scripts/native/ui-test.mjs:63`),
    /// 所以收窄到 `ui-test` 不损失任何测试面。
    func testOnlyTheTestEnvironmentRendersDeveloperScaffolding() {
        XCTAssertTrue(DiagnosticEnvironment.isDiagnostic("ui-test"))
        // 大小写与空白不该改变判定:环境标签来自注入,不是受控输入。
        XCTAssertTrue(DiagnosticEnvironment.isDiagnostic("  UI-Test "))

        for label in ["dev", "default", "production", "", "ui-testing"] {
            XCTAssertFalse(
                DiagnosticEnvironment.isDiagnostic(label),
                "「\(label)」不是测试环境,不该渲染开发者脚手架"
            )
        }
    }

    // MARK: SidebarGroup — the navigation spine's section order

    /// `renderOrder` 必须是 `allCases` 的一个排列：不漏、不重。
    ///
    /// 反事实:侧栏按 `renderOrder` 过滤分组,所以**新加一个 `SidebarGroup`
    /// 却忘了加进 `renderOrder`**,那一组的行会静默消失——不报错、不崩溃,
    /// 只是用户再也看不到那些工作面。这不是假想:分组是枚举,而渲染顺序是
    /// 手写数组,两者之间没有任何编译器强制的关系。
    func testGroupRenderOrderCoversEveryGroupExactlyOnce() {
        let order = SidebarGroup.renderOrder
        XCTAssertEqual(
            Set(order), Set(SidebarGroup.allCases),
            "renderOrder 漏掉了分组——那些组的行永远不会渲染"
        )
        XCTAssertEqual(
            order.count, SidebarGroup.allCases.count,
            "renderOrder 里有重复分组——同一组会被渲染两次"
        )
    }

    /// 渲染顺序必须**真的按分组重排**,而不是照抄调用方给的顺序。
    ///
    /// 反事实(2026-10-13,UI 实测):`AppServices.sidebarItems` 按「12 个工作面
    /// 在前、项目在后」供给,而侧栏按 `工作→项目→工具→系统` 渲染。shell 的
    /// ↑/↓ 曾直接走供给顺序,于是从「任务」按一下 ↓ 跳到 **Git**——屏幕上它在
    /// 三个分组之后。两个顺序必须来自同一个地方,这条用例钉住那个地方。
    func testRenderedOrderRegroupsInsteadOfCopyingTheSuppliedOrder() {
        let items = [
            SidebarItem(id: "s.tasks", title: "任务", group: .work),
            SidebarItem(id: "s.files", title: "文件", group: .tools),
            SidebarItem(id: "p.alpha", title: "alpha", group: .projects),
            SidebarItem(id: "s.ssh", title: "SSH", group: .system),
            SidebarItem(id: "s.git", title: "Git", group: .tools),
        ]
        XCTAssertEqual(
            SidebarOrdering.rendered(items).map(\.id),
            ["s.tasks", "p.alpha", "s.files", "s.git", "s.ssh"],
            "渲染顺序＝按分组重排;组内保持调用方给的相对顺序"
        )
        // 组内顺序是调用方的,不能被排序打乱(项目列表的顺序来自宿主快照)。
        XCTAssertEqual(
            SidebarOrdering.rendered(items).filter { $0.group == .tools }.map(\.id),
            ["s.files", "s.git"]
        )
        XCTAssertTrue(SidebarOrdering.rendered([]).isEmpty)
    }

    // MARK: filterCommands

    func testFilterCommandsEmptyQueryReturnsAll() {
        let commands = [
            PaletteCommand(id: "a", title: "打开项目"),
            PaletteCommand(id: "b", title: "设置…"),
        ]
        XCTAssertEqual(filterCommands(commands, query: ""), commands)
        XCTAssertEqual(filterCommands(commands, query: "   "), commands)
    }

    func testFilterCommandsCaseInsensitiveSubstring() {
        let commands = [
            PaletteCommand(id: "a", title: "Open Project…"),
            PaletteCommand(id: "b", title: "Settings…"),
        ]
        XCTAssertEqual(
            filterCommands(commands, query: "open proj"),
            [PaletteCommand(id: "a", title: "Open Project…")]
        )
    }

    func testFilterCommandsNoMatchIsEmpty() {
        let commands = [PaletteCommand(id: "a", title: "设置…")]
        XCTAssertTrue(filterCommands(commands, query: "xyz").isEmpty)
    }

    func testFilterCommandsCJKSubstringMatches() {
        let commands = [
            PaletteCommand(id: "a", title: "打开项目…"),
            PaletteCommand(id: "b", title: "设置…"),
        ]
        XCTAssertEqual(
            filterCommands(commands, query: "项目"),
            [PaletteCommand(id: "a", title: "打开项目…")]
        )
    }
}
