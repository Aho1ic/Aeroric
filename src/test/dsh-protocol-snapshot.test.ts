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

/** 字符串清单的一一对应关系。`remoteEvents` 是对象数组,单独测。 */
const PARITY_PAIRS = [
  ["rpcMethods", "RPC_METHODS"],
  ["remoteMethods", "REMOTE_METHODS"],
] as const;

/**
 * Rust 侧那些不与某个顶层 TS 数组字段一一对应的清单。它们各有自己的断言:
 * REMOTE_EVENTS/REMOTE_EVENT_MODES 组成 `remoteEvents` 的对象数组,
 * STREAM_* 三份组成 `streamFrames` 的嵌套对象,
 * RETIRED_* 三份只在 Rust 侧当"不许回归"的黑名单用(DSH-14 起,退役的
 * `/api/events.mux` + `/api/events.host` 帧名也在里面)。
 */
const RUST_ONLY_SLICES = [
  "REMOTE_EVENTS",
  "REMOTE_EVENT_MODES",
  "STREAM_FOLLOW_FRAMES",
  "STREAM_CONTROL_FRAMES",
  "STREAM_DOWNLINK_FRAMES",
  "RETIRED_APIPROXY_METHODS",
  "RETIRED_MUX_FRAMES",
  "RETIRED_HOST_FRAMES",
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

function rustStrSlice(name: string, min = MIN_ENTRIES): string[] {
  // 同时认单行(`= &["a", "b"];`)与多行(`&[\n "a",\n];`)两种 rustfmt 排版。
  // 单行时没有可选的换行前缀,`([\s\S]*?)` 会吃到行内内容。
  const match = new RegExp(
    `const ${name}: &\\[&str\\] = &\\[([\\s\\S]*?)\\n?\\];`,
  ).exec(inventorySource);
  if (!match) throw new Error(`missing Rust slice ${name} in ${INVENTORY_PATH}`);
  const entries = [...withoutLineComments(match[1]).matchAll(/"([^"]+)"/g)].map(
    (entry) => entry[1],
  );
  if (entries.length < min) {
    // 正则读歪了(Rust 侧换了排版、或末尾 `\n];` 被提前匹配)时必须报错,而不是
    // 交出一个短清单让 toEqual 去比。
    throw new Error(
      `parsed only ${entries.length} entries from ${name}; the regex in this test is probably stale`,
    );
  }
  return entries;
}

/**
 * Rust 文件里所有 `&[&str]` 常量名,用来反查有没有清单没被配对。
 *
 * 单行 `= &["a", "b"];` 形式(STREAM_* 三份)与多行形式都要认,否则新加的清单
 * 会从覆盖检查里漏掉。
 */
function allRustSliceNames(): string[] {
  return [
    ...withoutLineComments(inventorySource).matchAll(/const ([A-Z0-9_]+): &\[&str\] = &\[/g),
  ].map((entry) => entry[1]);
}

/** TS 快照里所有字符串数组字段名(对象数组与嵌套对象各有专属断言)。 */
function allSnapshotArrayKeys(): string[] {
  return Object.entries(DSH_PROTOCOL_SNAPSHOT)
    .filter(
      ([, value]) => Array.isArray(value) && value.every((entry) => typeof entry === "string"),
    )
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

  it("agrees on remoteEvents including dispatch modes", () => {
    const fromTs = DSH_PROTOCOL_SNAPSHOT.remoteEvents;
    const events = rustStrSlice("REMOTE_EVENTS");
    const modes = rustStrSlice("REMOTE_EVENT_MODES");
    expect(events).toHaveLength(modes.length);
    expect(fromTs.map((entry) => entry.event)).toEqual(events);
    expect(fromTs.map((entry) => entry.mode)).toEqual(modes);
    // 每条只能是两种派发模式之一;拼错会让前端把 waterfall 当通知,静默丢掉回复。
    expect(modes.every((mode) => mode === "emit" || mode === "waterfall")).toBe(true);
    expect(new Set(events).size).toBe(events.length);
  });

  it("agrees on the live stream frame vocabulary", () => {
    const { streamFrames } = DSH_PROTOCOL_SNAPSHOT;
    expect([...streamFrames.sessionFollow]).toEqual(rustStrSlice("STREAM_FOLLOW_FRAMES", 3));
    expect([...streamFrames.sessionControl]).toEqual(rustStrSlice("STREAM_CONTROL_FRAMES", 3));
    expect([...streamFrames.remoteDownlink]).toEqual(rustStrSlice("STREAM_DOWNLINK_FRAMES", 3));
  });

  it("keeps the retired apiproxy methods out of the snapshot", () => {
    // 这一条是 DSH-4 的核心回归:apiproxy 整域在 ce3391e280 被删,旧方法名再出现
    // 就意味着有人把上一个 pin 的清单粘回来了。
    const retired = rustStrSlice("RETIRED_APIPROXY_METHODS");
    const live = new Set<string>([
      ...DSH_PROTOCOL_SNAPSHOT.rpcMethods,
      ...DSH_PROTOCOL_SNAPSHOT.remoteMethods,
    ]);
    expect(retired.filter((method) => live.has(method))).toEqual([]);
    // 反向:新的 Typert 方法必须在表里,否则 invoke_dsh_remote 会拒掉真实调用。
    for (const method of [
      "session.page",
      "session.follow",
      "session.control",
      "workspaceFiles.list",
      "fileUploads.upload",
    ]) {
      expect(live.has(method)).toBe(true);
    }
  });

  it("keeps the retired events.mux / events.host frames out of the snapshot", () => {
    // 这一条是 DSH-14 的核心回归:下行换成了 `/api/remote.mux`,两条 firehose 的帧名
    // 只剩 Rust 侧的黑名单。它们再出现在活词表里,只意味着有人把上一个 pin 的表
    // 粘了回来。
    const retired = [
      ...rustStrSlice("RETIRED_MUX_FRAMES"),
      ...rustStrSlice("RETIRED_HOST_FRAMES"),
    ];
    // 黑名单本身被"顺手删空"时,下面的 filter 会变成永真断言,所以先钉两个哨兵。
    expect(retired).toContain("session/event");
    expect(retired).toContain("host/session-added");

    const { streamFrames } = DSH_PROTOCOL_SNAPSHOT;
    const live = new Set<string>([
      ...streamFrames.sessionFollow,
      ...streamFrames.sessionControl,
      ...streamFrames.remoteDownlink,
      ...DSH_PROTOCOL_SNAPSHOT.remoteEvents.map((entry) => entry.event),
    ]);
    expect(retired.filter((frame) => live.has(frame))).toEqual([]);

    // 快照上也不该再有 muxFrames / hostFrames 两个字段:清单退役后它们只会让
    // 兼容性诊断报一批任何 harness 都不会发的帧名。
    expect(Object.keys(DSH_PROTOCOL_SNAPSHOT)).not.toContain("muxFrames");
    expect(Object.keys(DSH_PROTOCOL_SNAPSHOT)).not.toContain("hostFrames");
  });

  it("covers every inventory list on both sides", () => {
    // 新增一份清单(任一侧)却忘了加进 PARITY_PAIRS,上面的 it.each 会安静地不测它。
    const paired = PARITY_PAIRS.map(([, rustName]) => rustName);
    expect([...allRustSliceNames()].sort()).toEqual(
      [...paired, ...RUST_ONLY_SLICES].sort(),
    );

    const pairedTsKeys = PARITY_PAIRS.map(([tsKey]) => tsKey as string);
    expect([...allSnapshotArrayKeys()].sort()).toEqual([...pairedTsKeys].sort());
  });
});
