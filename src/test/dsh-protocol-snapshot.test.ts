import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { DSH_PROTOCOL_SNAPSHOT } from "../dshProtocol";

/**
 * `src/dshProtocol.ts` 与 `src-tauri/src/dsh_webui/protocol_inventory.rs` 是两份
 * 逐项平行的清单:前端渲染兼容性诊断,后端 `DshProtocolCapabilities::snapshot()`
 * 也向 UI 报同一批数据。上游改一个方法名就要改两处,漏一处不会被编译或既有测试
 * 发现 —— 历史上 `sourceCommit` 就曾指向一个本地不存在的提交。这里把"两份必须
 * 一致"变成可检查的约束。
 *
 * 本文件靠正则读 Rust 源码,所以它自己也可能"读歪了还全绿"。三道自检兜住这一点:
 *   1. 每个解析结果都断言非空、且不少于一个下限条数;
 *   2. Rust 侧的 `&[&str]` 常量集合必须与下面 PARITY_PAIRS 覆盖的集合完全相等,
 *      新加一份清单忘了配对会直接失败;
 *   3. TS 快照里的数组字段集合同样必须被完全覆盖。
 */

// jsdom 环境下 import.meta.url 不是 file: URL,改从项目根解析(同 z-layers.test.ts)。
const INVENTORY_PATH = "src-tauri/src/dsh_webui/protocol_inventory.rs";
const inventorySource = readFileSync(resolve(process.cwd(), INVENTORY_PATH), "utf8");

/** 去掉行注释,避免注释里的字符串字面量被当成清单条目。 */
function withoutLineComments(source: string): string {
  return source.replace(/^\s*\/\/.*$/gm, "");
}

/** TS 快照里每份清单的下限条数,防"解析出空数组两边都空所以通过"。 */
const MIN_ENTRIES = 5;

const PARITY_PAIRS = [
  ["rpcMethods", "RPC_METHODS"],
  ["remoteMethods", "REMOTE_METHODS"],
  ["remoteEvents", "REMOTE_EVENTS"],
  ["muxFrames", "MUX_FRAMES"],
  ["hostFrames", "HOST_FRAMES"],
] as const;

function rustStrConst(name: string): string {
  const match = new RegExp(`const ${name}: &str = "([^"]*)";`).exec(inventorySource);
  if (!match) throw new Error(`missing Rust const ${name} in ${INVENTORY_PATH}`);
  return match[1];
}

function rustU32Const(name: string): number {
  const match = new RegExp(`const ${name}: u32 = (\\d+);`).exec(inventorySource);
  if (!match) throw new Error(`missing Rust const ${name} in ${INVENTORY_PATH}`);
  return Number(match[1]);
}

function rustStrSlice(name: string): string[] {
  const match = new RegExp(`const ${name}: &\\[&str\\] = &\\[([\\s\\S]*?)\\n\\];`).exec(
    inventorySource,
  );
  if (!match) throw new Error(`missing Rust slice ${name} in ${INVENTORY_PATH}`);
  const entries = [...withoutLineComments(match[1]).matchAll(/"([^"]+)"/g)].map(
    (entry) => entry[1],
  );
  if (entries.length < MIN_ENTRIES) {
    // 正则读歪了(Rust 侧换了排版、或末尾 `\n];` 被提前匹配)时必须报错,而不是
    // 交出一个短清单让 toEqual 去比。
    throw new Error(
      `parsed only ${entries.length} entries from ${name}; the regex in this test is probably stale`,
    );
  }
  return entries;
}

/** Rust 文件里所有 `&[&str]` 常量名,用来反查有没有清单没被配对。 */
function allRustSliceNames(): string[] {
  return [
    ...withoutLineComments(inventorySource).matchAll(/const ([A-Z0-9_]+): &\[&str\] = &\[/g),
  ].map((entry) => entry[1]);
}

/** TS 快照里所有字符串数组字段名。 */
function allSnapshotArrayKeys(): string[] {
  return Object.entries(DSH_PROTOCOL_SNAPSHOT)
    .filter(([, value]) => Array.isArray(value))
    .map(([key]) => key);
}

describe("dsh protocol snapshot parity", () => {
  it("agrees on the pinned source commit and version", () => {
    expect(DSH_PROTOCOL_SNAPSHOT.sourceCommit).toBe(rustStrConst("SOURCE_COMMIT"));
    expect(DSH_PROTOCOL_SNAPSHOT.packageVersion).toBe(rustStrConst("PACKAGE_VERSION"));
    expect(DSH_PROTOCOL_SNAPSHOT.protocolVersion).toBe(rustU32Const("PROTOCOL_VERSION"));
  });

  it.each(PARITY_PAIRS)("agrees on %s", (tsKey, rustName) => {
    const fromTs = DSH_PROTOCOL_SNAPSHOT[tsKey] as readonly string[];
    const fromRust = rustStrSlice(rustName);
    // 两侧都得真的有内容:空数组 === 空数组 是永真断言。
    expect(fromTs.length).toBeGreaterThanOrEqual(MIN_ENTRIES);
    expect([...fromTs]).toEqual(fromRust);
    // 清单里出现重复项说明有人手改时贴重了,前端的 includes 检查会掩盖掉。
    expect(new Set(fromTs).size).toBe(fromTs.length);
  });

  it("pins a full 40-hex source commit", () => {
    // 半个提交号或占位串会让漂移排查失去唯一锚点。
    expect(DSH_PROTOCOL_SNAPSHOT.sourceCommit).toMatch(/^[0-9a-f]{40}$/);
  });

  it("covers every inventory list on both sides", () => {
    // 新增一份清单(任一侧)却忘了加进 PARITY_PAIRS,上面的 it.each 会安静地不测它。
    const paired = PARITY_PAIRS.map(([, rustName]) => rustName);
    expect([...allRustSliceNames()].sort()).toEqual([...paired].sort());

    const pairedTsKeys = PARITY_PAIRS.map(([tsKey]) => tsKey as string);
    expect([...allSnapshotArrayKeys()].sort()).toEqual([...pairedTsKeys].sort());
  });
});
