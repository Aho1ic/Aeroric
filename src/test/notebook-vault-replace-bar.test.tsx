import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { NoteVaultReplaceBar } from "../components/notebook/NoteVaultReplaceBar";
import type {
  VaultReplaceMatch,
  VaultReplacePreview,
} from "../components/notebook/noteVaultReplace";

/* 文案回 key,带参数的把参数值拼在后面 —— 数字是这块界面的要点(「替换 N 处 / M 篇」
   在勾选之后必须跟着变),所以这里连数字一起验,不只验 key 出现。 */
const t = (key: string, vars?: Record<string, string | number>) =>
  vars ? `${key}:${Object.values(vars).join(",")}` : key;

function match(over: Partial<VaultReplaceMatch> = {}): VaultReplaceMatch {
  return {
    path: "/vault/A.md",
    name: "A.md",
    line: 3,
    column: 1,
    lineText: "cat sat",
    matchText: "cat",
    replacementText: "dog",
    start: 10,
    end: 13,
    ...over,
  };
}

function preview(over: Partial<VaultReplacePreview> = {}): VaultReplacePreview {
  return {
    query: "cat",
    replacement: "dog",
    files: [
      {
        path: "/vault/A.md",
        name: "A.md",
        matches: [match(), match({ line: 7, start: 40, end: 43 })],
      },
      {
        path: "/vault/B.md",
        name: "B.md",
        matches: [match({ path: "/vault/B.md", name: "B.md", line: 1, start: 0, end: 3 })],
      },
    ],
    totalMatches: 3,
    truncated: false,
    ...over,
  };
}

function renderBar(over: Partial<Parameters<typeof NoteVaultReplaceBar>[0]> = {}) {
  const props = {
    value: "dog",
    onValueChange: vi.fn(),
    preview: null as VaultReplacePreview | null,
    excluded: new Set<string>() as ReadonlySet<string>,
    onToggleFile: vi.fn(),
    busy: false,
    summary: null,
    canPreview: true,
    onPreview: vi.fn(),
    onApply: vi.fn(),
    t,
    ...over,
  };
  const view = render(<NoteVaultReplaceBar {...props} />);
  return {
    ...props,
    rerender: (next: Partial<typeof props>) =>
      view.rerender(<NoteVaultReplaceBar {...props} {...next} />),
  };
}

const status = () => document.querySelector('[aria-live="polite"]')?.textContent ?? "";
const previewButton = () => screen.getByRole("button", { name: "notebook.replaceVaultPreview" });
const applyButton = () => screen.getByRole("button", { name: "notebook.replaceVaultApply" });

describe("NoteVaultReplaceBar", () => {
  it("输入框改动回传原文,不 trim", () => {
    const props = renderBar({ value: "" });
    const input = screen.getByRole("textbox", { name: "notebook.replaceVault" });
    /* 尾随空格必须原样回去:「把 `cat` 换成 `dog `」是合法需求,而 trim 过的话
       用户永远打不出以空格结尾的替换文本。 */
    fireEvent.change(input, { target: { value: "dog " } });
    expect(props.onValueChange).toHaveBeenCalledWith("dog ");
  });

  it("回车触发预览,别的键不触发", () => {
    const props = renderBar();
    const input = screen.getByRole("textbox", { name: "notebook.replaceVault" });
    fireEvent.keyDown(input, { key: "a" });
    expect(props.onPreview).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onPreview).toHaveBeenCalledTimes(1);
  });

  describe("没预览过的时候", () => {
    it("提示先预览,替换按钮点不动", () => {
      const props = renderBar();
      expect(status()).toContain("notebook.replaceVaultHint");
      expect(applyButton()).toBeDisabled();
      /* disabled 之外还要验真的没调用 —— `disabled` 属性在测试里容易写对而事件
         照样绑着(比如用 style 假装禁用),那样键盘用户还是能触发。 */
      fireEvent.click(applyButton());
      expect(props.onApply).not.toHaveBeenCalled();
    });

    it("查询为空时预览也点不动", () => {
      const props = renderBar({ canPreview: false });
      expect(previewButton()).toBeDisabled();
      fireEvent.click(previewButton());
      expect(props.onPreview).not.toHaveBeenCalled();
    });

    it("查询非空时预览可点", () => {
      const props = renderBar({ canPreview: true });
      fireEvent.click(previewButton());
      expect(props.onPreview).toHaveBeenCalledTimes(1);
    });
  });

  describe("预览之后", () => {
    it("按文件分组列出命中,每条画出旧 → 新", () => {
      renderBar({ preview: preview() });
      const groupA = screen.getByRole("region", {
        name: "notebook.replaceVaultFileGroup:A.md",
      });
      expect(within(groupA).getAllByText("cat")).toHaveLength(2);
      expect(within(groupA).getAllByText("dog")).toHaveLength(2);
      expect(
        screen.getByRole("region", { name: "notebook.replaceVaultFileGroup:B.md" }),
      ).toBeInTheDocument();
    });

    it("统计行按勾选之后的数字算", () => {
      const { rerender } = renderBar({ preview: preview() });
      expect(status()).toContain("notebook.replaceVaultSummary:3,2");
      /* 勾掉一个文件之后统计必须跟着降。只验初始数字的话,把 `previewCounts` 换成
         直接读 `preview.totalMatches` 也能过 —— 那正是排除功能失效的样子。 */
      rerender({ preview: preview(), excluded: new Set(["/vault/A.md"]) });
      expect(status()).toContain("notebook.replaceVaultSummary:1,1");
    });

    it("勾选框回传文件路径", () => {
      const props = renderBar({ preview: preview() });
      fireEvent.click(
        screen.getByRole("checkbox", { name: "notebook.replaceVaultFileToggle:A.md" }),
      );
      expect(props.onToggleFile).toHaveBeenCalledWith("/vault/A.md");
    });

    it("被排除的文件勾选框是未选中状态", () => {
      renderBar({ preview: preview(), excluded: new Set(["/vault/B.md"]) });
      expect(
        screen.getByRole("checkbox", { name: "notebook.replaceVaultFileToggle:A.md" }),
      ).toBeChecked();
      expect(
        screen.getByRole("checkbox", { name: "notebook.replaceVaultFileToggle:B.md" }),
      ).not.toBeChecked();
    });

    it("有命中就能落笔", () => {
      const props = renderBar({ preview: preview() });
      expect(applyButton()).toBeEnabled();
      fireEvent.click(applyButton());
      expect(props.onApply).toHaveBeenCalledTimes(1);
    });

    it("全部文件都被勾掉时落笔按钮关掉", () => {
      const props = renderBar({
        preview: preview(),
        excluded: new Set(["/vault/A.md", "/vault/B.md"]),
      });
      /* 这一条是安全阀:一条都不提交时 `apply_text_replacements` 收到空数组,后端
         会把它当成「没什么要改的」正常返回 —— 于是界面报「已替换 0 处」,看起来像
         成功执行过。宁可按钮直接不可点。 */
      expect(applyButton()).toBeDisabled();
      fireEvent.click(applyButton());
      expect(props.onApply).not.toHaveBeenCalled();
      expect(status()).toContain("notebook.replaceVaultEmpty");
    });

    it("零命中的预览也报「没有可替换的内容」", () => {
      renderBar({ preview: preview({ files: [], totalMatches: 0 }) });
      expect(status()).toContain("notebook.replaceVaultEmpty");
      expect(applyButton()).toBeDisabled();
    });

    it("触顶的预览要说出来", () => {
      renderBar({ preview: preview({ truncated: true }) });
      expect(status()).toContain("notebook.replaceVaultTruncated");
      // 触顶不影响能不能替换 —— 预览里那批仍然是真命中。
      expect(applyButton()).toBeEnabled();
    });

    it("替换成空串时画出占位而不是空白", () => {
      renderBar({
        preview: preview({
          replacement: "",
          files: [{ path: "/vault/A.md", name: "A.md", matches: [match({ replacementText: "" })] }],
        }),
      });
      /* 空串是合法替换(=删掉命中)。不画占位的话那一格是空白,和「预览坏了」
         长得一模一样。 */
      expect(screen.getByText("notebook.replaceVaultEmptyText")).toBeInTheDocument();
    });
  });

  describe("在飞与结果", () => {
    it("busy 时两个按钮都锁住", () => {
      const props = renderBar({ preview: preview(), busy: true });
      expect(status()).toContain("notebook.replaceVaultRunning");
      expect(previewButton()).toBeDisabled();
      expect(applyButton()).toBeDisabled();
      fireEvent.click(applyButton());
      fireEvent.click(previewButton());
      expect(props.onApply).not.toHaveBeenCalled();
      expect(props.onPreview).not.toHaveBeenCalled();
    });

    it("落笔结果盖住统计行", () => {
      renderBar({
        preview: null,
        summary: { filesChanged: 2, replacementsApplied: 3, replacementsSkipped: 0 },
      });
      expect(status()).toContain("notebook.replaceVaultDone:3,2");
      expect(status()).not.toContain("notebook.replaceVaultSkipped");
    });

    it("有跳过就把跳过数说出来", () => {
      renderBar({
        preview: null,
        summary: { filesChanged: 1, replacementsApplied: 2, replacementsSkipped: 4 },
      });
      /* 跳过 = 乐观锁没对上 = 那个文件在预览之后被改过。不说的话用户看到的是
         「点了全部替换,结果只改了一部分」,而无从得知为什么。 */
      expect(status()).toContain("notebook.replaceVaultSkipped:4");
    });

    it("busy 优先于上一次的结果", () => {
      renderBar({
        busy: true,
        summary: { filesChanged: 1, replacementsApplied: 1, replacementsSkipped: 0 },
      });
      expect(status()).toContain("notebook.replaceVaultRunning");
      expect(status()).not.toContain("notebook.replaceVaultDone");
    });
  });
});
