import { describe, expect, it } from "vitest";
import {
  appendCapture,
  capturePath,
  captureRelativePath,
  captureTimeLabel,
  INBOX_NAME,
  inboxNotePath,
} from "../components/notebook/noteCapture";

describe("capturePath", () => {
  it("sends today's capture to the daily note", () => {
    expect(capturePath("/vault", "today", new Date(2026, 7, 28))).toBe(
      "/vault/Daily/2026-08-28.md",
    );
  });

  it("sends an inbox capture to a single file", () => {
    /* 收集箱刻意是**一个文件**而不是一篇篇新笔记:捕获多半是一句话,一句话一篇会让
       笔记列表在一周内变得没法用。 */
    expect(capturePath("/vault", "inbox", new Date(2026, 7, 28))).toBe(`/vault/${INBOX_NAME}`);
    expect(inboxNotePath("/vault")).toBe("/vault/Inbox.md");
  });

  it("keeps the inbox path independent of the date", () => {
    const a = capturePath("/vault", "inbox", new Date(2026, 7, 28));
    const b = capturePath("/vault", "inbox", new Date(2027, 0, 1));
    expect(a).toBe(b);
  });
});

describe("captureRelativePath", () => {
  it("shows the vault-relative target", () => {
    expect(captureRelativePath("today", new Date(2026, 7, 28))).toBe("Daily/2026-08-28.md");
    expect(captureRelativePath("inbox", new Date(2026, 7, 28))).toBe("Inbox.md");
  });

  it("matches the tail of the absolute path it stands for", () => {
    // 显示的落点和真正写入的文件必须是同一个,否则提示是假的。
    const date = new Date(2026, 7, 28);
    for (const target of ["today", "inbox"] as const) {
      expect(capturePath("/vault", target, date)).toBe(
        `/vault/${captureRelativePath(target, date)}`,
      );
    }
  });
});

describe("captureTimeLabel", () => {
  it("pads to 24-hour HH:MM", () => {
    expect(captureTimeLabel(new Date(2026, 7, 28, 9, 5))).toBe("09:05");
    expect(captureTimeLabel(new Date(2026, 7, 28, 14, 7))).toBe("14:07");
    expect(captureTimeLabel(new Date(2026, 7, 28, 0, 0))).toBe("00:00");
    expect(captureTimeLabel(new Date(2026, 7, 28, 23, 59))).toBe("23:59");
  });

  it("does not switch to 12-hour for the afternoon", () => {
    // 12 小时制会让 `## 02:30` 一天出现两次,而它是块与块之间唯一的区分标记。
    expect(captureTimeLabel(new Date(2026, 7, 28, 13, 30))).toBe("13:30");
  });
});

describe("appendCapture", () => {
  it("starts a timestamped block", () => {
    expect(appendCapture("", "记得回邮件", "14:07")).toBe("## 14:07\n\n记得回邮件\n");
  });

  it("separates the new block from existing prose with one blank line", () => {
    expect(appendCapture("# 日记\n\n正文\n", "第二条", "14:07")).toBe(
      "# 日记\n\n正文\n\n## 14:07\n\n第二条\n",
    );
  });

  it("normalizes however much trailing whitespace was there", () => {
    /* 原文末尾有 0 / 1 / 3 个换行取决于上一次是谁写的(模板、用户、上一次捕获)。
       不归一化的话块与块之间的间距每次都不一样。 */
    const expected = "正文\n\n## 14:07\n\n新的\n";
    expect(appendCapture("正文", "新的", "14:07")).toBe(expected);
    expect(appendCapture("正文\n", "新的", "14:07")).toBe(expected);
    expect(appendCapture("正文\n\n\n", "新的", "14:07")).toBe(expected);
    expect(appendCapture("正文  \n \t\n", "新的", "14:07")).toBe(expected);
  });

  it("keeps line breaks inside the captured text", () => {
    // 用户按了回车就是想分行,只 trim 两头。
    expect(appendCapture("", "第一行\n第二行", "14:07")).toBe("## 14:07\n\n第一行\n第二行\n");
  });

  it("trims the captured text at both ends", () => {
    expect(appendCapture("", "  记一句  \n\n", "14:07")).toBe("## 14:07\n\n记一句\n");
  });

  it("returns the body untouched for blank text", () => {
    /* 调用方本来就该挡住,这里再兜一次:追加一个只有时间标题的空块比什么都不做更糟。 */
    expect(appendCapture("正文\n", "", "14:07")).toBe("正文\n");
    expect(appendCapture("正文\n", "   \n\t", "14:07")).toBe("正文\n");
  });

  it("stacks up over repeated captures", () => {
    let body = "";
    body = appendCapture(body, "一", "09:00");
    body = appendCapture(body, "二", "14:07");
    body = appendCapture(body, "三", "23:59");
    expect(body).toBe("## 09:00\n\n一\n\n## 14:07\n\n二\n\n## 23:59\n\n三\n");
  });

  it("does not touch what is already in the body", () => {
    // 追加只在末尾发生 —— 已有内容(包括看起来像捕获块的部分)原样保留。
    const before = "## 09:00\n\n一\n";
    expect(appendCapture(before, "二", "14:07").startsWith(before)).toBe(true);
  });

  it("keeps a markdown table in the captured text intact", () => {
    const text = "| a | b |\n| --- | --- |\n| 1 | 2 |";
    expect(appendCapture("", text, "14:07")).toBe(`## 14:07\n\n${text}\n`);
  });
});
