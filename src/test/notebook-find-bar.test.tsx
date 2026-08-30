import { fireEvent, render, screen } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it, vi } from "vitest";

import { NoteFindBar, type NoteFindBarProps } from "../components/notebook/NoteFindBar";

/* 假 `t` 直接回 key:断言就能钉在 key 上,不受文案改动影响。 */
const t = (key: string) => key;

function setup(overrides: Partial<NoteFindBarProps> = {}) {
  const onFlagsChange = vi.fn();
  const onMove = vi.fn();
  const onReplaceOne = vi.fn();
  const onReplaceAll = vi.fn();
  const onClose = vi.fn();
  const props: NoteFindBarProps = {
    replaceOpen: false,
    onShowReplace: vi.fn(),
    query: "cat",
    onQueryChange: vi.fn(),
    replacement: "",
    onReplacementChange: vi.fn(),
    matchCount: 3,
    activeMatchIndex: 0,
    flags: { caseSensitive: false, wholeWord: false, regex: false },
    onFlagsChange,
    onMove,
    onReplaceOne,
    onReplaceAll,
    onClose,
    inputRef: createRef<HTMLInputElement>(),
    t,
    ...overrides,
  };
  render(<NoteFindBar {...props} />);
  return { onFlagsChange, onMove, onReplaceOne, onReplaceAll, onClose };
}

describe("NoteFindBar 三个开关", () => {
  it("三个开关都在,且默认都是未按下", () => {
    setup();
    for (const key of [
      "notebook.findCaseSensitive",
      "notebook.findWholeWord",
      "notebook.findRegex",
    ]) {
      expect(screen.getByRole("button", { name: key })).toHaveAttribute("aria-pressed", "false");
    }
  });

  it("开着的开关 aria-pressed 是 true", () => {
    setup({ flags: { caseSensitive: true, wholeWord: false, regex: true } });
    expect(screen.getByRole("button", { name: "notebook.findCaseSensitive" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "notebook.findWholeWord" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(screen.getByRole("button", { name: "notebook.findRegex" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("点开关只翻它自己那一位", () => {
    const { onFlagsChange } = setup({
      flags: { caseSensitive: true, wholeWord: false, regex: false },
    });
    fireEvent.click(screen.getByRole("button", { name: "notebook.findWholeWord" }));
    expect(onFlagsChange).toHaveBeenCalledWith({
      caseSensitive: true,
      wholeWord: true,
      regex: false,
    });
  });

  it("再点一次是关掉", () => {
    const { onFlagsChange } = setup({
      flags: { caseSensitive: false, wholeWord: false, regex: true },
    });
    fireEvent.click(screen.getByRole("button", { name: "notebook.findRegex" }));
    expect(onFlagsChange).toHaveBeenCalledWith({
      caseSensitive: false,
      wholeWord: false,
      regex: false,
    });
  });
});

describe("NoteFindBar 状态栏", () => {
  it("有命中时报序号和总数", () => {
    setup({ matchCount: 3, activeMatchIndex: 1 });
    expect(screen.getByText("2/3")).toBeInTheDocument();
  });

  it("命中被截断时加号提示还有更多", () => {
    setup({ matchCount: 50, activeMatchIndex: 0, capped: true });
    expect(screen.getByText("1/50+")).toBeInTheDocument();
  });

  it("没截断就没有加号", () => {
    setup({ matchCount: 50, activeMatchIndex: 0, capped: false });
    expect(screen.getByText("1/50")).toBeInTheDocument();
  });

  it("零命中报无匹配", () => {
    setup({ matchCount: 0 });
    expect(screen.getByText("notebook.noMatches")).toBeInTheDocument();
  });

  it("正则出错时报正则有误,而不是无匹配", () => {
    setup({ matchCount: 0, error: "Invalid group" });
    expect(screen.getByText("notebook.findInvalidRegex")).toBeInTheDocument();
    expect(screen.queryByText("notebook.noMatches")).not.toBeInTheDocument();
  });

  it("报错原文挂在 title 上,供悬停查看", () => {
    setup({ matchCount: 0, error: "Invalid group specifier" });
    expect(screen.getByText("notebook.findInvalidRegex")).toHaveAttribute(
      "title",
      "Invalid group specifier",
    );
  });

  it("整词放宽时给出中日韩提示", () => {
    setup({ wholeWordIgnored: true });
    expect(screen.getByText("notebook.findCjkBadge")).toBeInTheDocument();
  });

  it("没放宽就不给提示", () => {
    setup({ wholeWordIgnored: false });
    expect(screen.queryByText("notebook.findCjkBadge")).not.toBeInTheDocument();
  });

  it("正则出错时不再叠加整词提示", () => {
    // 正则没跑起来,「整词放宽」是上一轮的残留结论,摆出来只会误导。
    setup({ wholeWordIgnored: true, error: "Invalid group" });
    expect(screen.queryByText("notebook.findCjkBadge")).not.toBeInTheDocument();
  });
});

describe("NoteFindBar 导航与替换", () => {
  it("Enter 往下一个,Shift+Enter 往上一个", () => {
    const { onMove } = setup();
    const input = screen.getByLabelText("notebook.find");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onMove).toHaveBeenCalledWith(1);
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
    expect(onMove).toHaveBeenCalledWith(-1);
  });

  it("Escape 关掉查找栏", () => {
    const { onClose } = setup();
    fireEvent.keyDown(screen.getByLabelText("notebook.find"), { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });

  it("零命中时上下按钮禁用", () => {
    setup({ matchCount: 0 });
    expect(screen.getByRole("button", { name: "notebook.nextMatch" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "notebook.previousMatch" })).toBeDisabled();
  });

  it("替换行展开后两个替换按钮都在", () => {
    const { onReplaceOne, onReplaceAll } = setup({ replaceOpen: true });
    fireEvent.click(screen.getByRole("button", { name: "notebook.replace" }));
    expect(onReplaceOne).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "notebook.replaceAll" }));
    expect(onReplaceAll).toHaveBeenCalled();
  });
});
