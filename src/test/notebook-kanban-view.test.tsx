/* `NoteKanbanView` 的用例:列怎么摆、勾选传什么、添加入口的交互。
 *
 * 模型层(列划分 / 归属 / 写回)在 `notebook-kanban.test.ts`,这里只验这一层。
 */

import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { NoteKanbanView } from "../components/notebook/NoteKanbanView";

/**
 * 假 `t`:返回 `key(名=值,…)`。
 *
 * 不能只返回 key —— 那样"报的数对不对"根本没被断言到(key 里没有 `{}` 占位符,
 * 插值无处可去)。把 vars 拼进去,用例才验得到 1 / 3 / 33%。
 */
const t = (key: string, vars?: Record<string, string | number>) =>
  vars
    ? `${key}(${Object.entries(vars)
        .map(([name, value]) => `${name}=${value}`)
        .join(",")})`
    : key;

const BODY = [
  "# 本周计划",
  "",
  "## 📥 待办",
  "- [ ] 写周报 #work !high @2026-09-01 {30%}",
  "- [ ] 修 bug",
  "",
  "## 完成",
  "- [x] 开周会",
  "",
].join("\n");

type Append = (column: { title: string; headingRaw: string; offset: number }, text: string) => void;

function setup(body = BODY, append?: Append) {
  const onToggleLine = vi.fn();
  const onAppend = append ?? (vi.fn() as unknown as Append);
  render(<NoteKanbanView body={body} onToggleLine={onToggleLine} onAppend={onAppend} t={t} />);
  return { onToggleLine, onAppend };
}

describe("NoteKanbanView", () => {
  it("按列渲染,列头显示 emoji、标题和未完成数", () => {
    setup();
    const columns = screen.getAllByRole("region");
    expect(columns).toHaveLength(2);
    // 大标题不算列(Markio 会多出一个空列)。
    expect(screen.getByRole("heading", { name: "待办" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "完成" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "本周计划" })).toBeNull();
    // emoji 从标题里分出来了,「待办」三个字一个都没被啃掉。
    expect(within(columns[0]!).getByText("📥")).toBeTruthy();
    expect(within(columns[0]!).getByTitle("notebook.kanbanOpenCount(count=2)")).toBeTruthy();
    expect(within(columns[1]!).getByTitle("notebook.kanbanOpenCount(count=0)")).toBeTruthy();
  });

  it("卡片显示摘掉标记后的文本、标签、截止日期与完成度", () => {
    setup();
    expect(screen.getByText("写周报")).toBeTruthy();
    expect(screen.getByText("#work")).toBeTruthy();
    expect(screen.getByText("2026-09-01")).toBeTruthy();
    expect(screen.getByTitle("30%")).toBeTruthy();
    expect(screen.getByTitle("notebook.taskPriority.high")).toBeTruthy();
  });

  it("顶栏报完成度:3 条里 1 条完成 = 33%", () => {
    setup();
    expect(screen.getByText("notebook.kanbanProgress(done=1,total=3,percent=33)")).toBeTruthy();
  });

  it("勾选时把渲染那一刻看到的状态当乐观锁传出去", () => {
    const { onToggleLine } = setup();
    const boxes = screen.getAllByRole("checkbox");
    fireEvent.click(boxes[0]!);
    // 「写周报」在正文第 4 行,当时未勾选。
    expect(onToggleLine).toHaveBeenCalledWith(4, false);
    fireEvent.click(boxes[2]!);
    // 「开周会」在第 8 行,当时已勾选 —— 传 true,让 toggleTaskLine 有得可比。
    expect(onToggleLine).toHaveBeenCalledWith(8, true);
  });

  it("添加任务:回车提交,把点的那一列传出去", () => {
    const append = vi.fn();
    setup(BODY, append);
    const columns = screen.getAllByRole("region");
    fireEvent.click(within(columns[1]!).getByRole("button", { name: "notebook.kanbanAdd" }));
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "新任务" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(append).toHaveBeenCalledTimes(1);
    const [column, text] = append.mock.calls[0]!;
    expect(text).toBe("新任务");
    // 传的是第二列 —— 列头原文对得上「## 完成」。
    expect(column.title).toBe("完成");
    expect(column.headingRaw).toBe("## 完成");
  });

  it("失焦也提交:输了字去点别处,不该悄悄丢掉", () => {
    const append = vi.fn();
    setup(BODY, append);
    fireEvent.click(screen.getAllByRole("button", { name: "notebook.kanbanAdd" })[0]!);
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "新任务" } });
    fireEvent.blur(input);
    expect(append).toHaveBeenCalledWith(expect.objectContaining({ title: "待办" }), "新任务");
  });

  it("Escape 放弃,不提交", () => {
    const append = vi.fn();
    setup(BODY, append);
    fireEvent.click(screen.getAllByRole("button", { name: "notebook.kanbanAdd" })[0]!);
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "不要这条" } });
    fireEvent.keyDown(input, { key: "Escape" });
    expect(append).not.toHaveBeenCalled();
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("空文案不提交", () => {
    const append = vi.fn();
    setup(BODY, append);
    fireEvent.click(screen.getAllByRole("button", { name: "notebook.kanbanAdd" })[0]!);
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.blur(input);
    expect(append).not.toHaveBeenCalled();
  });

  it("同名两列只打开点的那一个输入框", () => {
    const dup = ["## 进行中", "- [ ] 甲", "", "## 进行中", "- [ ] 乙", ""].join("\n");
    setup(dup, vi.fn());
    const columns = screen.getAllByRole("region");
    fireEvent.click(within(columns[1]!).getByRole("button", { name: "notebook.kanbanAdd" }));
    // 用标题当「正在输入的是哪一列」会同时开两个。
    expect(screen.getAllByRole("textbox")).toHaveLength(1);
    expect(within(columns[1]!).getByRole("textbox")).toBeTruthy();
  });

  it("未归属的任务如实报出来", () => {
    setup(["- [ ] 散落的", "", "## 待办", "- [ ] a", ""].join("\n"));
    expect(screen.getByText("notebook.kanbanUnplaced(count=1)")).toBeTruthy();
  });

  it("没有未归属任务时不显示那条提示", () => {
    setup();
    expect(screen.queryByText(/kanbanUnplaced/)).toBeNull();
  });

  it("没有列时给出写法说明,不显示空板", () => {
    setup(["就是一段普通正文", "- [ ] 一条任务", ""].join("\n"));
    expect(screen.getByText("notebook.kanbanNoColumns")).toBeTruthy();
    expect(screen.getByText("notebook.kanbanHowTo")).toBeTruthy();
    expect(screen.queryByRole("region")).toBeNull();
  });

  it("不传 onAppend 时没有添加入口,但勾选仍可用", () => {
    render(<NoteKanbanView body={BODY} onToggleLine={vi.fn()} t={t} />);
    expect(screen.queryByRole("button", { name: "notebook.kanbanAdd" })).toBeNull();
    expect(screen.getAllByRole("checkbox")).toHaveLength(3);
  });

  it("代码块里的看板语法不成列、也不成卡片", () => {
    setup(
      [
        "## 待办",
        "- [ ] 真任务",
        "",
        "```md",
        "# 这是代码块里的标题",
        "- [ ] 这是代码块里的例子",
        "```",
        "",
      ].join("\n"),
    );
    expect(screen.getAllByRole("region")).toHaveLength(1);
    expect(screen.getAllByRole("checkbox")).toHaveLength(1);
    expect(screen.queryByText("这是代码块里的例子")).toBeNull();
  });

  it("整行只有标记的卡片,读屏能念出原文", () => {
    setup(["## 待办", "- [ ] #work", ""].join("\n"));
    // 摘掉标签之后文本是空的,aria-label 退回原文而不是空串。
    expect(screen.getByRole("checkbox", { name: "#work" })).toBeTruthy();
  });

  it("正文变了就按新正文重画", () => {
    const { rerender } = render(
      <NoteKanbanView body={BODY} onToggleLine={vi.fn()} onAppend={vi.fn()} t={t} />,
    );
    expect(screen.getAllByRole("checkbox")).toHaveLength(3);
    rerender(
      <NoteKanbanView
        body={`${BODY}- [ ] 追加的\n`}
        onToggleLine={vi.fn()}
        onAppend={vi.fn()}
        t={t}
      />,
    );
    expect(screen.getAllByRole("checkbox")).toHaveLength(4);
  });
});
