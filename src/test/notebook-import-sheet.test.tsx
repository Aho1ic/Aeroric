/* 导入面板的画法。
 *
 * 用真实的 `staticT` 而不是身份函数 —— 报告区全是插值文案,拿 key 当文案会让
 * 「插值漏了」这类错误在测试里看不见。
 *
 * 盯的是几件「画错了不报错、只是看起来正常」的事:
 *
 * - issue 和 status 正交:一条**成功导入**的笔记也可以带附件丢失。把 issue 画在
 *   status 的分支里会让这一条彻底消失,而它正是用户要拿去回源端找图的那条。
 * - 报告写不进去**不算导入失败**(笔记已经在库里了),所以那句提示不能是 alert。
 * - 明细在前端再截一道,截掉的部分必须点明差额。
 */

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";

import { NoteImportSheet } from "../components/notebook/NoteImportSheet";
import { availableImportProviders, importStatusText } from "../components/notebook/noteImport";
import type { ImportItem, ImportReport } from "../components/notebook/notebookApi";
import { staticT } from "../i18n";

function report(overrides: Partial<ImportReport> = {}): ImportReport {
  return {
    provider: "evernote",
    dest: "imports/evernote",
    imported: 0,
    skipped: 0,
    failed: 0,
    resourceLost: 0,
    degraded: 0,
    items: [],
    truncated: 0,
    reportPath: "imports/evernote/报告.md",
    ...overrides,
  };
}

function renderSheet(overrides: Partial<Parameters<typeof NoteImportSheet>[0]> = {}) {
  /* 两个回调提到 spread 外面单独声明再显式返回:留在对象字面量里的话 `...overrides`
   * (类型是 Partial)会把它们的类型放宽成「回调 | Mock」的联合,调用点读 `.mock.calls`
   * 就过不了 tsc。没有测试覆写这两个,所以显式返回不改变行为。 */
  const onRun = vi.fn();
  const onClose = vi.fn();
  const props = {
    providers: availableImportProviders("macos"),
    busy: null,
    report: null,
    error: null,
    onRun,
    onClose,
    t: staticT,
    ...overrides,
  };
  render(<NoteImportSheet {...props} />);
  return { ...props, onRun, onClose };
}

describe("provider 列表", () => {
  it("每个可用 provider 画一个按钮,点下去带着 provider 本身回调", () => {
    const props = renderSheet();
    const button = screen.getByRole("button", { name: /Obsidian/ });
    fireEvent.click(button);
    expect(props.onRun).toHaveBeenCalledTimes(1);
    expect(props.onRun.mock.calls[0][0].id).toBe("obsidian");
  });

  it("跑着的时候全部禁用 —— 两个导入器可能落到同一个目录", () => {
    renderSheet({ busy: "evernote" });
    for (const name of [/Obsidian/, /Logseq/, /Evernote/]) {
      expect(screen.getByRole("button", { name })).toBeDisabled();
    }
  });

  it("非 macOS 不画 Apple 备忘录 —— 后端在那些平台直接返回错误", () => {
    renderSheet({ providers: availableImportProviders("windows") });
    expect(screen.queryByRole("button", { name: /Apple/ })).toBeNull();
  });
});

describe("报告区", () => {
  it("还没跑过时不画报告区", () => {
    renderSheet();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("三个划分计数出现在汇总里", () => {
    renderSheet({ report: report({ imported: 12, skipped: 3, failed: 1 }) });
    const summary = screen.getByRole("status").textContent ?? "";
    expect(summary).toContain("12");
    expect(summary).toContain("3");
    expect(summary).toContain("1");
  });

  it("成功导入的条目也画出它的附件丢失 —— issue 和 status 正交,画在 status 分支里这一条会消失", () => {
    const item: ImportItem = {
      source: "会议纪要.enex",
      dest: "imports/evernote/会议纪要.md",
      status: { kind: "imported" },
      issues: [{ kind: "resourceLost", target: "图.png", detail: "resource 里没有对应 hash" }],
    };
    renderSheet({ report: report({ imported: 1, resourceLost: 1, items: [item] }) });
    /* 断言插值进去的数据,不是文案本身 —— 改中英措辞不该让这条挂。状态那句用
       `importStatusText` 的真实返回来比,同样绕开措辞。 */
    expect(screen.getByText(importStatusText({ kind: "imported" }, staticT))).toBeTruthy();
    expect(screen.getByText(/图\.png/)).toBeTruthy();
    expect(screen.getByText(/resource 里没有对应 hash/)).toBeTruthy();
  });

  it("跳过的理由画出来,而且两档不共用一句 —— 「已经导过」和「被锁定」对用户是两件事", () => {
    const items: ImportItem[] = [
      {
        source: "旧笔记",
        dest: null,
        status: { kind: "skipped", reason: { kind: "alreadyImported" } },
      },
      {
        source: "私密备忘录",
        dest: null,
        status: { kind: "skipped", reason: { kind: "unreadable", detail: "备忘录被锁定" } },
      },
    ];
    renderSheet({ report: report({ skipped: 2, items }) });
    // 「被锁定」那句带 detail,是语言无关的可断言数据。
    expect(screen.getByText(/备忘录被锁定/)).toBeTruthy();
    // 两行的状态文案必须不同 —— 共用一句的话用户分不出该不该重试。
    const first = screen.getByText(
      importStatusText({ kind: "skipped", reason: { kind: "alreadyImported" } }, staticT),
    );
    expect(first).toBeTruthy();
    expect(first.textContent).not.toBe(
      importStatusText(
        { kind: "skipped", reason: { kind: "unreadable", detail: "备忘录被锁定" } },
        staticT,
      ),
    );
  });

  it("issues 缺字段时不崩 —— 后端在空数组时不序列化这个字段", () => {
    const item: ImportItem = {
      source: "普通笔记",
      dest: "imports/evernote/普通笔记.md",
      status: { kind: "imported" },
      // issues 故意不给。
    };
    renderSheet({ report: report({ imported: 1, items: [item] }) });
    expect(screen.getByText("普通笔记")).toBeTruthy();
  });

  it("报告落盘了就说在哪 —— 全量明细只在那篇笔记里", () => {
    renderSheet({ report: report({ reportPath: "imports/evernote/报告.md" }) });
    expect(screen.getByText(/imports\/evernote\/报告\.md/)).toBeTruthy();
  });

  it("报告没写进去是提示不是 alert —— 笔记已经在库里了,这不算导入失败", () => {
    renderSheet({ report: report({ imported: 3, reportPath: null }) });
    expect(screen.getByText(staticT("notebook.importReportUnsaved"))).toBeTruthy();
    // alert 只留给真正的失败。报告写不进去时笔记已经落地,不该用 alert 打断。
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("明细超过上限时点明差额 —— 不说的话用户以为只有这么多条", () => {
    const items: ImportItem[] = Array.from({ length: 60 }, (_, index) => ({
      source: `笔记-${index}`,
      dest: `imports/evernote/笔记-${index}.md`,
      status: { kind: "imported" as const },
    }));
    renderSheet({ report: report({ imported: 60, items }) });
    expect(screen.getByText("笔记-49")).toBeTruthy();
    expect(screen.queryByText("笔记-50")).toBeNull();
    /* 差额那句要带真实差额(60 - 50)。用整句比而不是找 /10/ —— 后者会被别处的
       数字命中,是个几乎不可能失败的断言。 */
    expect(screen.getByText(staticT("notebook.importMoreInReport", { count: "10" }))).toBeTruthy();
  });
});

describe("失败与关闭", () => {
  it("错误走 alert", () => {
    renderSheet({ error: "导入失败:文件不存在" });
    expect(screen.getByRole("alert").textContent).toContain("文件不存在");
  });

  /* 只钉「Esc 关窗」。那句 `stopPropagation` 在这里**测不出来** —— React 把监听装在
     根容器上,原生事件在合成事件派发之前就已经冒过父节点了,所以给父节点挂监听既拦不住
     也证明不了什么(试过:断言父监听没被调用,在带与不带 stopPropagation 两版下都失败)。
     `noteSheetChrome.ts` 里对同一件事有一段更长的说明。 */
  it("Esc 关窗", () => {
    const props = renderSheet();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(props.onClose).toHaveBeenCalledTimes(1);
  });

  it("IME 组字中的 Esc 不关窗 —— 那是「取消候选词」", () => {
    const props = renderSheet();
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape", isComposing: true });
    expect(props.onClose).not.toHaveBeenCalled();
  });
});
