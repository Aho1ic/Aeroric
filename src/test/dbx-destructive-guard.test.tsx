/**
 * 破坏性对象操作必须先过确认框。
 *
 * 为什么需要它:`useDbxObjectOperations` 里 6 个 drop 入口直接下发 DROP 语句,而没有
 * 任何测试覆盖它——确认框被误删或条件写反都不会让现有测试变红,只会在用户点了
 * "取消"之后真的把表删掉。
 *
 * 计数为什么用 Proxy 而不是逐个 spy:真实 `databaseApi` 是惰性 Proxy,键不可枚举,
 * `Object.values` 拿到空数组。按名字 spy 会得到一个永远为 0 的计数器,让"没有执行"
 * 这条断言变成永真——本测试要防的正是这种假绿。
 */
import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { confirm } from "../lib/appDialog";
import { useDbxObjectOperations } from "../components/database/useDbxObjectOperations";
import { useDbxTreeContextMenuActions } from "../components/database/useDbxTreeContextMenuActions";
import type {
  DatabaseContextMenuState,
  DbxObjectContextMenuAction,
} from "../components/database/databaseViewModel";
import type { DbxObjectInfo } from "../types";

vi.mock("../lib/appDialog", () => ({ confirm: vi.fn(), prompt: vi.fn(), message: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));

/**
 * 记录所有落到 databaseApi 的调用,不管方法叫什么。
 *
 * 注意区分两类:`dbxBuild*Sql` 是纯构造器,确认框要用它把真实 SQL 显示给用户,所以
 * 取消时出现它是**正确**的;真正的执行只走 `dbxExecuteQuery`。断言必须只盯执行,
 * 否则会把正常的预览行为误报成漏掉确认。
 */
const apiCalls: string[] = [];

const EXECUTORS = /^dbx(ExecuteQuery|Execute|RunQuery)$/;

function executedCalls(): string[] {
  return apiCalls.filter((call) => EXECUTORS.test(call.split(" ::")[0].split("(")[0]));
}

vi.mock("../lib/databaseApi", () => ({
  databaseApi: new Proxy(
    {},
    {
      get: (_target, prop) => {
        if (typeof prop !== "string") return undefined;
        return (...args: unknown[]) => {
          apiCalls.push(`${prop}(${JSON.stringify(args).slice(0, 200)})`);
          // `dbxBuild*Sql` 返回的是 SQL 字符串本身,会被直接拼进确认文案再交给执行;
          // 返回对象会让文案变成 "[object Object]",测的就不是真实数据流了。
          if (/^dbxBuild.*Sql$/.test(prop)) return Promise.resolve("DROP TABLE `victim`");
          return Promise.resolve({
            rows: [],
            columns: [],
            rowsAffected: 0,
            sql: "DROP TABLE `victim`",
          });
        };
      },
    },
  ),
}));

/**
 * hook 的依赖面很宽(state / controller / dialog 状态…),逐个构造与本测试无关。
 * Proxy 兜住所有属性访问,只把真正要断言的出口(confirm、databaseApi)换成记录器。
 */
function permissive(): unknown {
  const fn = () => permissive();
  return new Proxy(fn, {
    get: (_target, prop) => {
      if (prop === "then") return undefined;
      if (prop === Symbol.toPrimitive || prop === "toString") return () => "";
      return permissive();
    },
    apply: () => permissive(),
  });
}

type Ops = Record<string, (...args: unknown[]) => Promise<unknown>>;

function renderOps(): Ops {
  const { result } = renderHook(
    () => (useDbxObjectOperations as unknown as (deps: unknown) => Ops)(permissive()),
    { wrapper: I18nProvider },
  );
  return result.current;
}

/** mysql:过 `isSqlDbxConnection`(非 redis/mongodb),也不在禁 TRUNCATE 名单里。 */
const CONNECTION = { id: "c1", dbType: "mysql", name: "conn" };

/** 过 `isDbxTableObject`(object_type 必须是 table)。 */
const TABLE = { name: "victim", schema: "public", object_type: "table" };

/**
 * 每个入口按真实签名给参数,不用统一的万能替身。
 *
 * 这点很关键:几个入口在到 confirm 之前有形状闸门——`dropDbxTableObject` 与两个子对象
 * 入口都要求 `isDbxTableObject(object)`,`dropDbxTableChildObject` 还要求
 * `dbxTableChildObjectType(childObject)` 非空。喂错形状会让它们在闸门处静默 return,
 * 于是"没有执行"变成永真断言——测的是我的构造,不是守卫。所以逐个按签名写死。
 */
const DROP_ENTRIES: Record<string, readonly unknown[]> = {
  dropDbxObject: [CONNECTION, "db", { ...TABLE, object_type: "view" }],
  dropDbxTableObject: [CONNECTION, "db", TABLE],
  dropDbxDatabase: [CONNECTION, "db"],
  dropDbxSchema: [CONNECTION, "db", "public"],
  dropDbxColumn: [CONNECTION, "db", TABLE, { name: "victim_col" }],
  dropDbxTableChildObject: [
    CONNECTION,
    "db",
    TABLE,
    { name: "victim_trg", object_type: "trigger" },
  ],
};

const ALL_DROP_ENTRIES = Object.keys(DROP_ENTRIES);

describe("dbx destructive object operations", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiCalls.length = 0;
  });

  it("exposes every drop entry point", () => {
    const ops = renderOps();
    // 入口被改名或删掉时这里会红,提醒同步维护参数表。
    for (const name of ALL_DROP_ENTRIES) {
      expect(typeof ops[name], `${name} is missing from the hook`).toBe("function");
    }
  });

  /**
   * 变异检查:逐个证明"没有执行"不是因为形状闸门把调用挡在了 confirm 之前。
   * 每个入口在**批准**时都必须真的打到 databaseApi,否则它那条拒绝断言是永真的。
   */
  for (const name of ALL_DROP_ENTRIES) {
    it(`${name} really executes when approved, so its decline assertion means something`, async () => {
      vi.mocked(confirm).mockResolvedValue(true);
      const ops = renderOps();

      await ops[name](...DROP_ENTRIES[name]);

      expect(
        executedCalls(),
        `approving ${name} reached no execution; its decline assertion is vacuous`,
      ).not.toEqual([]);
    });
  }

  for (const name of ALL_DROP_ENTRIES) {
    it(`${name} asks for confirmation and executes nothing when declined`, async () => {
      vi.mocked(confirm).mockResolvedValue(false);
      const ops = renderOps();

      await ops[name](...DROP_ENTRIES[name]);

      expect(confirm, `${name} dropped its confirmation prompt`).toHaveBeenCalled();
      expect(executedCalls(), `${name} executed despite the user declining`).toEqual([]);
    });
  }

  it("treats a dismissed dialog (undefined) as a decline", async () => {
    // appDialog 在窗口被强行关掉时可能既不 resolve true 也不 reject。
    vi.mocked(confirm).mockResolvedValue(undefined as unknown as boolean);
    const ops = renderOps();

    await ops.dropDbxObject(...DROP_ENTRIES.dropDbxObject);

    expect(executedCalls()).toEqual([]);
  });

  it("names the doomed child object in the prompt, not just the table", async () => {
    vi.mocked(confirm).mockResolvedValue(false);
    const ops = renderOps();

    await ops.dropDbxColumn(...DROP_ENTRIES.dropDbxColumn);

    // 删列的弹窗必须点名是哪一列;只说表名会让人以为要删整张表。
    expect(String(vi.mocked(confirm).mock.calls[0]?.[0])).toContain("victim_col");
  });
});

/**
 * 侧边树右键菜单自己那条守卫。
 *
 * 与上面那组的区别:`useDbxTreeContextMenuActions` 里的 emptyTable / truncateTable /
 * dropTable 不是转发给 `dropDbx*`,而是自己 build SQL、自己 confirm、自己
 * `dbxExecuteQuery`——是一条独立的破坏性路径,上面那组测试完全盖不到它。
 * TRUNCATE 和 EMPTY 尤其值得盯:它们不删表结构,误执行后表还在,数据没了。
 *
 * 这里能用真实契约构造:菜单状态是个可辨识联合,连接就是数组里查 id。
 */
const TABLE_MENU: DatabaseContextMenuState = {
  x: 0,
  y: 0,
  connectionId: "c1",
  database: "db",
  object: TABLE as unknown as DbxObjectInfo,
  kind: "dbx-object",
};

/**
 * 只有菜单状态与连接列表是真值,其余依赖(setState、loader、四个对话框控制器)与本测试
 * 无关,继续用 Proxy 兜住。断言的出口仍然只有 confirm 与 databaseApi。
 */
function treeDeps(): unknown {
  const real: Record<string, unknown> = {
    contextMenu: TABLE_MENU,
    dbxConnections: [CONNECTION],
    activeDbxObject: null,
    activeDbxDatabase: "db",
    dbxObjects: [TABLE],
  };
  return new Proxy(real, {
    get: (target, prop) => {
      if (typeof prop !== "string") return Reflect.get(target, prop);
      if (prop in target) return target[prop];
      return permissive();
    },
  });
}

function renderTreeActions(): Record<
  string,
  (action: DbxObjectContextMenuAction) => Promise<unknown>
> {
  const { result } = renderHook(
    () =>
      (
        useDbxTreeContextMenuActions as unknown as (
          deps: unknown,
        ) => Record<string, (action: DbxObjectContextMenuAction) => Promise<unknown>>
      )(treeDeps()),
    { wrapper: I18nProvider },
  );
  return result.current;
}

/** 三个都走同一个 confirm→execute 出口,守卫写反会一起漏。 */
const TABLE_MUTATIONS = ["emptyTable", "truncateTable", "dropTable"] as const;

describe("dbx tree context menu destructive actions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiCalls.length = 0;
  });

  /** 变异检查:批准时必须真的执行,否则下面的拒绝断言全是永真。 */
  it("executes when approved, so the decline assertions mean something", async () => {
    vi.mocked(confirm).mockResolvedValue(true);
    const actions = renderTreeActions();

    await actions.runDbxObjectContextMenuAction("truncateTable");

    expect(
      executedCalls(),
      "approving truncateTable reached no execution; the decline assertions below would be vacuous",
    ).not.toEqual([]);
  });

  for (const action of TABLE_MUTATIONS) {
    it(`${action} asks for confirmation and executes nothing when declined`, async () => {
      vi.mocked(confirm).mockResolvedValue(false);
      const actions = renderTreeActions();

      await actions.runDbxObjectContextMenuAction(action);

      expect(confirm, `${action} dropped its confirmation prompt`).toHaveBeenCalled();
      expect(executedCalls(), `${action} executed despite the user declining`).toEqual([]);
    });
  }

  it("shows the real SQL in the prompt, so the user is not confirming blind", async () => {
    vi.mocked(confirm).mockResolvedValue(false);
    const actions = renderTreeActions();

    await actions.runDbxObjectContextMenuAction("dropTable");

    // 确认文案必须带上待执行语句;只显示"确定要删除吗"就等于让人盲签。
    expect(vi.mocked(confirm).mock.calls[0]?.[0]).toContain("DROP TABLE `victim`");
  });

  it("names the target object in the prompt", async () => {
    vi.mocked(confirm).mockResolvedValue(false);
    const actions = renderTreeActions();

    await actions.runDbxObjectContextMenuAction("truncateTable");

    // schema.name —— 弹窗必须说清动的是哪张表,否则多连接下极易删错库。
    expect(vi.mocked(confirm).mock.calls[0]?.[0]).toContain("public.victim");
  });

  it("treats a dismissed dialog (undefined) as a decline", async () => {
    vi.mocked(confirm).mockResolvedValue(undefined as unknown as boolean);
    const actions = renderTreeActions();

    await actions.runDbxObjectContextMenuAction("dropTable");

    expect(executedCalls()).toEqual([]);
  });
});
