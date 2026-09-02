import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { en } from "../i18n/en";
import { zh } from "../i18n/zh";

describe("translation catalogs", () => {
  it("keep the English and Chinese key sets aligned", () => {
    expect(Object.keys(en).sort()).toEqual(Object.keys(zh).sort());
  });
});

/**
 * `t()` 在 key 缺失时会**静默回退成显示 key 本身**
 * (`translations[language][key] ?? translations.en[key] ?? key`),
 * 所以少一条文案不会报错,只会让 UI 上露出 `appSettings.dshCopyPreset` 这种原始串。
 * 上面那条只比对 en/zh 是否对齐 —— 两边同时缺失时它是绿的。
 *
 * 曾经真的漏过 4 条(`DshPluginsPanel` 的预设操作按钮 tooltip/placeholder)。
 * 这条测试扫源码里所有**字面量** key 并要求它们在目录里存在。
 */
// 走 process.cwd() 而不是 __dirname:测试以 ESM 跑,且 node-builtins.d.ts 里
// 只声明了 process.cwd()(见 z-layers.test.ts 的同款用法)。
const SRC_DIR = path.resolve(process.cwd(), "src");

function collectSourceFiles(dir: string, acc: string[] = []): string[] {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "test" || entry.name === "i18n" || entry.name === "assets") continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) collectSourceFiles(full, acc);
    else if (/\.tsx?$/.test(entry.name)) acc.push(full);
  }
  return acc;
}

/**
 * 只收字面量调用:`t("a.b")` / `staticT("a.b")` / `translate("a.b")`。
 * 模板串与拼接(`` t(`x.${y}`) ``)天然无法静态求值,不在本测试范围 ——
 * 那类 key 由各自的功能测试覆盖。
 */
function collectLiteralKeys(text: string): string[] {
  const keys: string[] = [];
  const callRe = /\b(?:t|staticT|translate)\(\s*"((?:[^"\\]|\\.)+)"/g;
  for (const match of text.matchAll(callRe)) keys.push(match[1]);
  return keys;
}

describe("i18n key 完整性", () => {
  it("源码里所有字面量 t() key 都存在于 en 与 zh 目录", () => {
    const files = collectSourceFiles(SRC_DIR);
    const missing: string[] = [];

    for (const file of files) {
      const text = fs.readFileSync(file, "utf8");
      for (const key of collectLiteralKeys(text)) {
        // 有些 t() 的实参本身就是要显示的原文(如 DshComposer 里 t(`${arg} `)),
        // 但那些是模板串,不会被 collectLiteralKeys 收进来。字面量一律要求存在。
        if (!(key in en) || !(key in zh)) {
          missing.push(`${path.relative(SRC_DIR, file)} -> ${key}`);
        }
      }
    }

    expect(missing).toEqual([]);
  });
});
