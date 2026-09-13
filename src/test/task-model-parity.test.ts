import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * `src/types.ts` 的 `Task` 与 `src-tauri/src/storage.rs` 的 `Task` 是同一份任务记录的
 * 两侧声明:前端整份写进 `save_project_tasks`,Rust 反序列化后原样落盘。
 *
 * 为什么需要机械守卫:serde 默认**忽略未知字段**且这里没有 `deny_unknown_fields`,
 * 所以只存在于 TS 一侧的字段既不会编译报错、也不会运行报错 —— 它每次往返都被静默丢掉。
 * 历史上 `dshWorkspaceId` / `dshPromptMode` 就是这么活了很久:声明在 TS 侧、被 App.tsx
 * 当入参读取,却永远是 undefined。反方向同样危险:只在 Rust 侧新增字段,前端整份覆盖写回
 * 时会把它抹成缺省。
 *
 * 这个测试读源码而不是构造对象,因为「字段集合」在 TS 里是编译期信息、在 Rust 里是宏展开
 * 前信息,运行时都拿不到完整清单。
 */

const TS_PATH = "src/types.ts";
const RUST_PATH = "src-tauri/src/storage.rs";

/** 字段数下限,防「解析器坏了返回空集合,两边都空所以相等」这类假绿灯。 */
const MIN_FIELDS = 30;

/**
 * 取 `relative` 源文件里 `header` 之后第一对配平花括号的内容,注释已剥离。
 *
 * 剥注释:注释里的 `foo: Bar` 与 `rename = "x"` 都不是声明。
 * 按深度配平而不是找第一个 `}`:字段类型里出现内联对象/泛型块时,朴素找法会在结构体
 * 中途截断,于是漏掉后半截字段 —— 那正好是「新增字段漏同步」最常发生的位置。
 *
 * jsdom 环境下 import.meta.url 不是 file: URL,改从项目根解析(同 z-layers.test.ts)。
 */
function declarationBody(relative: string, header: string): string {
  const source = readFileSync(resolve(process.cwd(), relative), "utf8")
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\/\/.*$/gm, "");
  const headerAt = source.indexOf(header);
  expect(headerAt, `${relative} 里找不到 ${header}`).toBeGreaterThanOrEqual(0);
  const open = source.indexOf("{", headerAt);
  let depth = 0;
  for (let index = open; index < source.length; index += 1) {
    const char = source[index];
    if (char === "{") depth += 1;
    else if (char === "}") {
      depth -= 1;
      if (depth === 0) return source.slice(open + 1, index);
    }
  }
  throw new Error(`${relative} 里 ${header} 的花括号没有闭合`);
}

/** TS 接口里的属性名(`name?: T` 与 `name: T` 都算)。 */
function tsTaskFields(): string[] {
  const fields: string[] = [];
  for (const line of declarationBody(TS_PATH, "export interface Task ").split("\n")) {
    const match = /^\s*([A-Za-z_$][\w$]*)\??\s*:/.exec(line);
    if (match) fields.push(match[1]);
  }
  return fields.sort();
}

/** Rust 字段的 snake 名与线上名(`#[serde(rename = "...")]`,缺省时按 camelCase 推导)。 */
interface RustField {
  snake: string;
  wire: string;
  renamed: boolean;
}

/**
 * serde 属性既可能写成一行、也可能拆成多行(`#[serde(\n rename = "x",\n default\n)]`),
 * 所以按行累积 pending rename,遇到字段声明时消费掉。`rename_all` 不会误命中:`rename`
 * 后面紧跟 `_` 时不成词边界。
 */
function rustTaskFields(): RustField[] {
  const fields: RustField[] = [];
  let pendingRename: string | null = null;
  for (const line of declarationBody(RUST_PATH, "pub struct Task ").split("\n")) {
    const rename = /(?:^|[(,\s])rename\s*=\s*"([^"]+)"/.exec(line);
    if (rename) pendingRename = rename[1];
    const field = /^\s*pub\s+([a-z_][\w]*)\s*:/.exec(line);
    if (!field) continue;
    const snake = field[1];
    const derived = snake.replace(/_([a-z0-9])/g, (_match, char: string) => char.toUpperCase());
    fields.push({ snake, wire: pendingRename ?? derived, renamed: pendingRename !== null });
    pendingRename = null;
  }
  return fields.sort((left, right) => (left.wire < right.wire ? -1 : 1));
}

describe("Task model parity between src/types.ts and src-tauri/src/storage.rs", () => {
  it("declares the same field set on both sides", () => {
    const ts = tsTaskFields();
    const wire = rustTaskFields().map((field) => field.wire);

    // 先证明两个解析器都真的解析到了东西,否则下面的相等断言会变成永真。
    expect(ts.length, `${TS_PATH} 只解析到 ${ts.length} 个字段,解析器大概率坏了`).toBeGreaterThan(
      MIN_FIELDS,
    );
    expect(
      wire.length,
      `${RUST_PATH} 只解析到 ${wire.length} 个字段,解析器大概率坏了`,
    ).toBeGreaterThan(MIN_FIELDS);

    expect(
      ts.filter((name) => !wire.includes(name)),
      `以下字段只存在于 ${TS_PATH}:Rust 侧没有对应字段,serde 会在 save_project_tasks ` +
        `往返时静默丢弃它们(写进去读不回来)`,
    ).toEqual([]);
    expect(
      wire.filter((name) => !ts.includes(name)),
      `以下字段只存在于 ${RUST_PATH}:前端整份覆盖写回任务时会把它们抹成缺省`,
    ).toEqual([]);
  });

  it("keeps every serde rename equal to the camelCase of its snake name", () => {
    const rust = rustTaskFields();
    const renamed = rust.filter((field) => field.renamed);

    // 这条守的是「rename 只做 snake→camel,不做改名」。手写一个不一致的 rename
    // (`rename = "dsh_session_id"`、拼写差一个字母)会让上面那条集合相等断言报出
    // 一个看起来像「TS 少了字段」的假象,真正的错却在 Rust 侧的属性里。
    expect(renamed.length, "Task 里一个显式 rename 都没解析到,属性解析大概率坏了").toBeGreaterThan(
      MIN_FIELDS / 2,
    );
    const inconsistent = renamed.filter(
      (field) =>
        field.wire !==
        field.snake.replace(/_([a-z0-9])/g, (_m, char: string) => char.toUpperCase()),
    );
    expect(
      inconsistent.map((field) => `${field.snake} -> ${field.wire}`),
      "serde rename 必须就是 snake 名的 camelCase",
    ).toEqual([]);
  });

  it("keeps the dsh session fields that are actually persisted", () => {
    const ts = tsTaskFields();
    const wire = rustTaskFields().map((field) => field.wire);

    // dsh 侧只有这三个字段有真实写入端(会话注册 / 新建任务时的预设选择)。
    // `dshWorkspaceId` / `dshPromptMode` 曾在 TS 侧存在但全库无赋值,已删除:
    // prompt 模式由 DshComposer 的本地 state 直传 prompt_dsh_task,不是任务属性。
    // 这条同时钉住「不要再把它们加回来」——两侧都加会通过上面的集合相等,却又是死字段。
    for (const field of ["dshSessionId", "dshSessionPath", "dshAgentPreset"]) {
      expect(ts).toContain(field);
      expect(wire).toContain(field);
    }
    for (const dead of ["dshWorkspaceId", "dshPromptMode"]) {
      expect(ts).not.toContain(dead);
      expect(wire).not.toContain(dead);
    }
  });
});
