import { describe, expect, it } from "vitest";
import { agentForm, panelChrome, runDebugForm, settingsForm, wslForm } from "../styles/panelChrome";

/**
 * `styles/panelChrome.ts` 收拢了 14 个组件里字面完全一致的样式常量。合并的前提是
 * 「不影响外观」—— 迁移时逐个抽出常量体、去注释去空白后按属性排序比对过,只有完全
 * 相同的才搬。
 *
 * 这个测试把迁移那一刻的值钉住。它防的不是「合并错了」(那在迁移时已校验),而是
 * 之后有人为了某一个调用点改共用值 —— 现在一改就是 24 处一起变,而 24 处里只有
 * 一处会被肉眼看到。
 *
 * 同样重要的是那几组**没有**合并的:三种 `label` 字重 600/650、间距 5/6 各不相同,
 * 它们是三套视觉共用了一个泛名。下面显式断言它们仍然彼此不同 —— 谁想「顺手统一」
 * 一下,会在这里被拦住。
 */

describe("panelChrome:工具面板外壳", () => {
  it("顶栏高 38、字重 650", () => {
    expect(panelChrome.header).toEqual({
      height: 38,
      display: "flex",
      alignItems: "center",
      gap: 8,
      padding: "0 10px",
      borderBottom: "1px solid var(--border-dim)",
      fontSize: 12,
      fontWeight: 650,
    });
  });

  it("顶栏图标按钮自带 marginLeft:auto(靠它顶到右侧,调用点不再补)", () => {
    expect(panelChrome.headerIconButton.marginLeft).toBe("auto");
    expect(panelChrome.headerIconButton.width).toBe(24);
    expect(panelChrome.headerIconButton.height).toBe(24);
    expect(panelChrome.headerIconButton.background).toBe("transparent");
  });

  it("错误条用 --danger,并有下边框(它贴在顶栏下方)", () => {
    expect(panelChrome.errorBar).toEqual({
      padding: "7px 10px",
      color: "var(--danger)",
      fontSize: 11,
      borderBottom: "1px solid var(--border-dim)",
    });
  });
});

describe("runDebugForm:Run 与 Debug 两个面板的表单", () => {
  it("input 高 26、字号 11(比设置页那套小一档)", () => {
    expect(runDebugForm.input.height).toBe(26);
    expect(runDebugForm.input.fontSize).toBe(11);
    expect(runDebugForm.input.background).toBe("var(--bg-card)");
  });

  it("label 是竖排的 flex(标签在输入框上方)", () => {
    expect(runDebugForm.label.display).toBe("flex");
    expect(runDebugForm.label.flexDirection).toBe("column");
    expect(runDebugForm.label.fontWeight).toBe(650);
  });

  it("status 用 marginLeft:auto + 单行省略(它在工具栏右端放长文本)", () => {
    expect(runDebugForm.status.marginLeft).toBe("auto");
    expect(runDebugForm.status.whiteSpace).toBe("nowrap");
    expect(runDebugForm.status.textOverflow).toBe("ellipsis");
    expect(runDebugForm.status.overflow).toBe("hidden");
  });

  it("list 可滚且 minHeight:0(否则在 flex 列里撑破容器而不是自己滚)", () => {
    expect(runDebugForm.list.minHeight).toBe(0);
    expect(runDebugForm.list.overflowY).toBe("auto");
  });

  it("empty 居中", () => {
    expect(runDebugForm.empty.textAlign).toBe("center");
  });

  it("toolbar 可换行(按钮多了要折行,不是挤压)", () => {
    expect(runDebugForm.toolbar.flexWrap).toBe("wrap");
  });
});

describe("settingsForm:设置页表单", () => {
  it("input 是等宽字体、padding 7px 10px", () => {
    expect(settingsForm.input.fontFamily).toBe("var(--font-mono)");
    expect(settingsForm.input.padding).toBe("7px 10px");
    expect(settingsForm.input.fontSize).toBe(12.5);
  });

  it("input 有 boxSizing:border-box(width:100% 配 padding,少了会溢出)", () => {
    expect(settingsForm.input.width).toBe("100%");
    expect(settingsForm.input.boxSizing).toBe("border-box");
  });

  it("label 是 block、字重 600、下间距 5", () => {
    expect(settingsForm.label).toEqual({
      fontSize: 12,
      fontWeight: 600,
      color: "var(--text-secondary)",
      marginBottom: 5,
      display: "block",
    });
  });

  it("hint 上间距 3、无 lineHeight", () => {
    expect(settingsForm.hint).toEqual({
      fontSize: 11,
      color: "var(--text-hint)",
      marginTop: 3,
    });
  });
});

describe("没有合并的那几组仍然彼此不同", () => {
  it("agentForm.label 与 settingsForm.label 的字重和间距不同", () => {
    // 650 vs 600、6 vs 5。统一成一个会让两处里的一处位移 1px、字重变一档。
    expect(agentForm.label.fontWeight).toBe(650);
    expect(settingsForm.label.fontWeight).toBe(600);
    expect(agentForm.label.marginBottom).toBe(6);
    expect(settingsForm.label.marginBottom).toBe(5);
    expect(agentForm.label).not.toEqual(settingsForm.label);
  });

  it("runDebugForm.label 是 flex 竖排,另两种是 block", () => {
    expect(runDebugForm.label.display).toBe("flex");
    expect(settingsForm.label.display).toBe("block");
    expect(agentForm.label.display).toBe("block");
  });

  it("wslForm.input 与 settingsForm.input 的字体和 padding 不同", () => {
    // ui vs mono、8px vs 7px。WSL 那两处填的是 Windows 路径,用界面字体。
    expect(wslForm.input.fontFamily).toBe("var(--font-ui)");
    expect(settingsForm.input.fontFamily).toBe("var(--font-mono)");
    expect(wslForm.input.padding).toBe("8px 10px");
    expect(settingsForm.input.padding).toBe("7px 10px");
    expect(wslForm.input).not.toEqual(settingsForm.input);
  });

  it("三种 input 互不相等(高 26 / mono 12.5 / ui 8px 三套视觉)", () => {
    const inputs = [runDebugForm.input, settingsForm.input, wslForm.input];
    for (let i = 0; i < inputs.length; i += 1) {
      for (let j = i + 1; j < inputs.length; j += 1) {
        expect(inputs[i]).not.toEqual(inputs[j]);
      }
    }
  });
});

describe("共用模块没有被并进扁平的 s 命名空间", () => {
  it("panelChrome 的短名没有被 spread 进 s(否则会撞 panels.ts 的 toolbar)", async () => {
    // 刻意不进 styles/index.ts 的 spread —— `header` / `input` 这种名字进去会撞。
    // `toolbar` 已经被 styles/panels.ts 占用:真 spread 了,两者按顺序互相吞掉一个。
    const s = (await import("../styles")).default as Record<string, unknown>;
    const { panels } = await import("../styles");
    expect(panels).toHaveProperty("toolbar");
    expect(s.toolbar).toBe(panels.toolbar);
    expect(s.toolbar).not.toBe(panelChrome as unknown);

    // 其余短名在 s 里根本不存在,即确实没被 spread 进去。
    for (const key of ["header", "input", "label", "hint", "empty", "status", "errorBar"]) {
      expect(s, `${key} 出现在扁平 s 里,说明 panelChrome 被 spread 了`).not.toHaveProperty(key);
    }
  });
});
