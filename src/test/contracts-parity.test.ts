import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import {
  type AgentType,
  type PermissionMode,
  type ProjectLocation,
  type ProtocolFamily,
  type TaskStatus,
  type BuiltInAgentType,
} from "@aeroric/remote-contracts";
import * as desktop from "../types";

const ENVELOPE = "contracts/native/v1/envelope.schema.json";
const SHARED = "packages/remote-contracts/src/shared.ts";

/** 取 `shared.ts` 里一个字面量联合的成员,声明可以跨行(`export type X =\n | "a"\n | "b";`)。 */
function tsUnionMembers(name: string): string[] {
  const source = readFileSync(resolve(process.cwd(), SHARED), "utf8");
  const decl = new RegExp(`export type ${name}\\s*=([\\s\\S]*?);`).exec(source);
  expect(decl, `${SHARED} 里找不到 export type ${name} = …`).toBeTruthy();
  return [...decl![1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
}

describe("published vocabulary vs the TypeScript unions", () => {
  /**
   * `envelope.schema.json` 的 `constants` 由 Rust 生成(`export-schema.rs` 读 `…::ALL`),
   * 并且 `contracts --check` 逐字节比它 —— 所以"产物 == Rust"这一头本来就是硬的。
   * 缺的是另一头:`shared.ts` 自己写着"Rust 侧不强制 codegen;parity 测试锁关键字段名
   * (见 src/test/contracts-parity.test.ts)",而那里的断言全是 TS↔TS 的赋值兼容,
   * 从不读这份产物。后果很具体:Rust 新增一个族/状态时,产物会红、TS 联合不会,
   * 于是桌面与移动端拿着一个漏值的字面量联合继续给 wire 值做穷举。
   */
  const vocabularies: Array<{ constant: string; union: string; atLeast: number }> = [
    { constant: "protocolFamilies", union: "ProtocolFamily", atLeast: 4 },
    { constant: "taskStatuses", union: "TaskStatus", atLeast: 9 },
    { constant: "permissionModes", union: "PermissionMode", atLeast: 3 },
    { constant: "builtInAgents", union: "BuiltInAgentType", atLeast: 5 },
  ];

  it.each(vocabularies)(
    "keeps $union identical to the published $constant list",
    ({ constant, union, atLeast }) => {
      const published = JSON.parse(readFileSync(resolve(process.cwd(), ENVELOPE), "utf8"))
        .constants[constant];
      expect(Array.isArray(published), `${ENVELOPE} 里没有 ${constant} 这份清单`).toBe(true);
      // 正控:产物读空/读错键会得到 [] 或 undefined,下面的相等断言就会变成永真。
      expect(
        published.length,
        `${ENVELOPE} 的 ${constant} 只有 ${published.length} 项,产物大概率没被正确读取`,
      ).toBeGreaterThanOrEqual(atLeast);

      const declared = tsUnionMembers(union);
      expect(
        declared.length,
        `${SHARED} 里 ${union} 只解析到 ${declared.length} 个成员,解析器坏了`,
      ).toBeGreaterThanOrEqual(atLeast);

      expect(
        [...declared].sort(),
        `${union} 与 Rust 公布的 ${constant} 不一致:一侧改了另一侧不会报错(见本 describe 的说明)`,
      ).toEqual([...published].sort());
    },
  );
});

/**
 * 桌面 types 必须从 contracts re-export 跨端词表，而不是再抄一份字面量。
 * 这里锁的是「同一类型身份」：值级断言无意义，用赋值兼容 + 关键字面量成员存在性。
 */
describe("remote-contracts shared vocabulary", () => {
  it("re-exports agent/task/location unions from desktop types", () => {
    const family: ProtocolFamily = "omp";
    const mode: PermissionMode = "ask";
    const status: TaskStatus = "running";
    const builtin: BuiltInAgentType = "claude_gpt55";
    const agent: AgentType = builtin;
    const location: ProjectLocation = { kind: "local", path: "/x" };

    // 桌面侧同名导出必须接受 contracts 的值（同一类型）。
    const desktopFamily: desktop.ProtocolFamily = family;
    const desktopMode: desktop.PermissionMode = mode;
    const desktopStatus: desktop.TaskStatus = status;
    const desktopAgent: desktop.AgentType = agent;
    const desktopLocation: desktop.ProjectLocation = location;

    expect(desktopFamily).toBe("omp");
    expect(desktopMode).toBe("ask");
    expect(desktopStatus).toBe("running");
    expect(desktopAgent).toBe("claude_gpt55");
    expect(desktopLocation.kind).toBe("local");
  });

  it("TaskStatus covers the lifecycle set used by boot normalization", () => {
    const statuses: TaskStatus[] = [
      "todo",
      "pending",
      "running",
      "input_required",
      "detached",
      "interrupted",
      "done",
      "failed",
      "cancelled",
    ];
    expect(new Set(statuses).size).toBe(9);
  });
});
