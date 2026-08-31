import { describe, expect, it } from "vitest";
import {
  DAILY_DIR,
  dailyDateFromPath,
  dailyNoteName,
  dailyNotePath,
  dailyStepFrom,
  shiftDays,
} from "../components/notebook/noteDaily";

describe("dailyNoteName / dailyNotePath", () => {
  it("names the file after the date, zero padded", () => {
    expect(dailyNoteName(new Date(2026, 0, 5))).toBe("2026-01-05");
  });

  it("lands under the Daily subdirectory of the vault", () => {
    expect(dailyNotePath("/vault", new Date(2026, 7, 28))).toBe(
      `/vault/${DAILY_DIR}/2026-08-28.md`,
    );
  });

  it("gives the same path for the same day, every time", () => {
    // 「打开今天的日记」必须每次都落到同一个文件上,否则会攒出一串 `-2` `-3`。
    const a = dailyNotePath("/vault", new Date(2026, 7, 28, 1, 0, 0));
    const b = dailyNotePath("/vault", new Date(2026, 7, 28, 23, 59, 59));
    expect(a).toBe(b);
  });
});

describe("dailyDateFromPath", () => {
  it("reads the date out of a daily note path", () => {
    const date = dailyDateFromPath("/vault/Daily/2026-08-28.md");
    expect(date && [date.getFullYear(), date.getMonth(), date.getDate()]).toEqual([2026, 7, 28]);
  });

  it("accepts a Windows path", () => {
    expect(dailyDateFromPath("C:\\vault\\Daily\\2026-08-28.md")).not.toBeNull();
  });

  it("accepts a lowercase directory name", () => {
    expect(dailyDateFromPath("/vault/daily/2026-08-28.md")).not.toBeNull();
  });

  it("rejects a date-named note outside Daily/", () => {
    // 用户完全可能把一篇会议记录命名成日期。它不是日记,不该能被「前一天」翻走。
    expect(dailyDateFromPath("/vault/2026-08-28.md")).toBeNull();
    expect(dailyDateFromPath("/vault/Meetings/2026-08-28.md")).toBeNull();
  });

  it("rejects a stem that merely ends with a date", () => {
    /* `会议-2026-08-28.md` 认成日记之后,「前一天」会从它跳到 `2026-08-27.md`,
       而用户以为自己在翻会议记录。整个文件名必须就是日期。 */
    expect(dailyDateFromPath("/vault/Daily/会议-2026-08-28.md")).toBeNull();
    expect(dailyDateFromPath("/vault/Daily/2026-08-28-2.md")).toBeNull();
  });

  it("rejects a date that does not exist", () => {
    /* `new Date(2026, 1, 30)` 不失败,它滚到 3 月 2 日 —— 于是 `2026-02-30.md` 被
       解析成 3 月 2 日,「前一天」跳到 3 月 1 日。一个不存在的日期静默变成了另一个
       存在的日期,`Number.isNaN` 抓不到,只有回对一遍才行。 */
    expect(dailyDateFromPath("/vault/Daily/2026-02-30.md")).toBeNull();
    expect(dailyDateFromPath("/vault/Daily/2026-13-01.md")).toBeNull();
    expect(dailyDateFromPath("/vault/Daily/2026-00-10.md")).toBeNull();
    expect(dailyDateFromPath("/vault/Daily/2026-04-31.md")).toBeNull();
  });

  it("accepts Feb 29 in a leap year and rejects it otherwise", () => {
    expect(dailyDateFromPath("/vault/Daily/2028-02-29.md")).not.toBeNull();
    expect(dailyDateFromPath("/vault/Daily/2026-02-29.md")).toBeNull();
  });

  it("rejects a non-markdown file and a missing extension", () => {
    expect(dailyDateFromPath("/vault/Daily/2026-08-28.txt")).toBeNull();
    expect(dailyDateFromPath("/vault/Daily/2026-08-28")).toBeNull();
  });

  it("rejects an unpadded date", () => {
    // 我们自己写出来的名字永远是补零的,所以 `2026-8-28.md` 不是我们建的日记。
    expect(dailyDateFromPath("/vault/Daily/2026-8-28.md")).toBeNull();
  });

  it("round-trips with dailyNotePath", () => {
    const date = new Date(2026, 7, 28);
    const parsed = dailyDateFromPath(dailyNotePath("/vault", date));
    expect(parsed && dailyNoteName(parsed)).toBe(dailyNoteName(date));
  });
});

describe("shiftDays", () => {
  it("steps across a month boundary", () => {
    expect(dailyNoteName(shiftDays(new Date(2026, 7, 31), 1))).toBe("2026-09-01");
    expect(dailyNoteName(shiftDays(new Date(2026, 8, 1), -1))).toBe("2026-08-31");
  });

  it("steps across a year boundary", () => {
    expect(dailyNoteName(shiftDays(new Date(2026, 11, 31), 1))).toBe("2027-01-01");
  });

  it("handles a leap day", () => {
    expect(dailyNoteName(shiftDays(new Date(2028, 1, 28), 1))).toBe("2028-02-29");
    expect(dailyNoteName(shiftDays(new Date(2026, 1, 28), 1))).toBe("2026-03-01");
  });

  it("lands on the next calendar day across a DST switch", () => {
    /* 按毫秒加一天的话,只有 23 小时的那天会落回同一天的 23:00 —— 「后一天」点了
       没反应。TZ 由测试环境决定,所以对整年逐日断言:每一步都必须换一个日期名。 */
    let cursor = new Date(2026, 0, 1);
    for (let index = 0; index < 400; index += 1) {
      const next = shiftDays(cursor, 1);
      expect(dailyNoteName(next), dailyNoteName(cursor)).not.toBe(dailyNoteName(cursor));
      expect(next.getTime()).toBeGreaterThan(cursor.getTime());
      cursor = next;
    }
  });

  it("is its own inverse", () => {
    const date = new Date(2026, 2, 9);
    expect(dailyNoteName(shiftDays(shiftDays(date, -1), 1))).toBe(dailyNoteName(date));
  });

  it("keeps the time of day out of the result", () => {
    // 只保留年月日:带上 23:30 的话跨夏令时会把日期挤到隔天。
    const shifted = shiftDays(new Date(2026, 2, 8, 23, 30, 45, 500), 1);
    expect([shifted.getHours(), shifted.getMinutes(), shifted.getSeconds()]).toEqual([0, 0, 0]);
  });
});

describe("dailyStepFrom", () => {
  const today = new Date(2026, 7, 28);

  it("steps from the open daily note so the user can page through", () => {
    // 在 08-27 上按「前一天」要到 08-26,而不是又回到「今天减一天」。
    const step = dailyStepFrom("/vault/Daily/2026-08-27.md", today, -1);
    expect(dailyNoteName(step)).toBe("2026-08-26");
  });

  it("steps from today when the open note is not a daily note", () => {
    expect(dailyNoteName(dailyStepFrom("/vault/Alpha.md", today, -1))).toBe("2026-08-27");
    expect(dailyNoteName(dailyStepFrom("/vault/Daily/会议-2026-01-01.md", today, 1))).toBe(
      "2026-08-29",
    );
  });

  it("steps from today when nothing is open", () => {
    expect(dailyNoteName(dailyStepFrom(null, today, 1))).toBe("2026-08-29");
  });

  it("pages continuously when the caller feeds the result back", () => {
    let path = "/vault/Daily/2026-08-28.md";
    const seen: string[] = [];
    for (let index = 0; index < 3; index += 1) {
      const next = dailyStepFrom(path, today, -1);
      seen.push(dailyNoteName(next));
      path = dailyNotePath("/vault", next);
    }
    expect(seen).toEqual(["2026-08-27", "2026-08-26", "2026-08-25"]);
  });
});
