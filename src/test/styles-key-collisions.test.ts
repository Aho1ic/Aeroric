import { describe, expect, it } from "vitest";
import s, {
  common,
  database,
  dialogs,
  font,
  gitDiff,
  layout,
  panels,
  remoteAccess,
  skillHub,
  task,
  terminal,
  timeline,
  usage,
} from "../styles";

/**
 * `styles/index.ts` 把所有模块 spread 进一个扁平的 `s`。同名键会被后 spread 的模块
 * 静默吞掉 —— 曾经真的发生过:`usageSourceList` / `usageMetricLabel` /
 * `usageMetricValue` 同时存在于 common.ts(为 UsagePopover 的 flex 行布局而写)
 * 和 usage.ts(为 UsageDashboard 的 grid 卡片布局而写),spread 顺序让 usage 胜出,
 * UsagePopover 于是一直拿到带 `marginTop: 11` 的卡片样式,在居中行里下移 11px。
 *
 * 这个测试让同类事故在 CI 就暴露,而不是靠肉眼看渲染。
 */
const modules: Record<string, Record<string, unknown>> = {
  layout,
  panels,
  remoteAccess,
  terminal,
  dialogs,
  task,
  gitDiff,
  common,
  database,
  font,
  timeline,
  skillHub,
  usage,
};

describe("styles 模块键冲突", () => {
  it("任意两个样式模块之间没有同名顶层键", () => {
    const owners = new Map<string, string[]>();
    for (const [moduleName, moduleStyles] of Object.entries(modules)) {
      for (const key of Object.keys(moduleStyles)) {
        owners.set(key, [...(owners.get(key) ?? []), moduleName]);
      }
    }

    const collisions = [...owners.entries()]
      .filter(([, mods]) => mods.length > 1)
      .map(([key, mods]) => `${key} <- ${mods.join(", ")}`);

    expect(collisions).toEqual([]);
  });

  it("扁平后的 s 保留了每个模块的全部键", () => {
    // 键冲突的另一面:若某模块的键在 s 里消失,说明被覆盖了。
    for (const [moduleName, moduleStyles] of Object.entries(modules)) {
      for (const key of Object.keys(moduleStyles)) {
        expect(s, `${moduleName}.${key} 在扁平的 s 里丢失`).toHaveProperty(key);
        expect(s[key as keyof typeof s], `${moduleName}.${key} 被其它模块的同名键覆盖`).toBe(
          moduleStyles[key],
        );
      }
    }
  });
});

describe("UsagePopover 与 UsageDashboard 的用量样式互不干扰", () => {
  it("popover 版的 label 用 flex:1 把 value 顶到行右侧", () => {
    expect(common.usagePopoverMetricLabel.flex).toBe(1);
  });

  it("popover 版的 value 没有 marginTop(它在居中的 flex 行里)", () => {
    expect(common.usagePopoverMetricValue).not.toHaveProperty("marginTop");
  });

  it("dashboard 版的 value 保留 marginTop(它在卡片里竖排于 label 下方)", () => {
    expect(usage.usageMetricValue.marginTop).toBe(11);
  });

  it("popover 的 source 列表是竖列,dashboard 的是 grid", () => {
    expect(common.usagePopoverSourceList.display).toBe("flex");
    expect(common.usagePopoverSourceList.flexDirection).toBe("column");
    expect(usage.usageSourceList.display).toBe("grid");
  });
});
