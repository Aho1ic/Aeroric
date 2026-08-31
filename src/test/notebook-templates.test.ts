import { describe, expect, it } from "vitest";
import {
  buildTemplate,
  DAILY_TEMPLATE,
  isoWeek,
  NOTE_TEMPLATES,
  templateVars,
  ymd,
} from "../components/notebook/noteTemplates";
import { en } from "../i18n/en";
import { zh } from "../i18n/zh";

/** 和 `src/i18n.tsx` 里同一套占位符展开:`{name}` 按 params 取值。 */
function fakeT(catalog: Record<string, string>) {
  return (key: string, vars?: Record<string, string | number>): string => {
    const raw = catalog[key];
    if (raw === undefined) throw new Error(`missing key: ${key}`);
    return raw.replace(/\{(\w+)\}/g, (whole, name: string) => {
      const value = vars?.[name];
      return value === undefined ? whole : String(value);
    });
  };
}

describe("ymd", () => {
  it("pads month and day to two digits", () => {
    expect(ymd(new Date(2026, 0, 5))).toBe("2026-01-05");
    expect(ymd(new Date(2026, 11, 31))).toBe("2026-12-31");
  });
});

describe("isoWeek", () => {
  /* 跨年那几天是这段唯一容易错的地方,所以四个方向各钉一个:年初的周四、年初
     不属于本年的周一、上一年最后几天已经属于下一年的、以及 53 周年的年末。 */
  it("counts 2026-01-01 (a Thursday) as 2026-W01", () => {
    expect(isoWeek(new Date(2026, 0, 1))).toEqual({ year: 2026, week: 1 });
  });

  it("counts 2025-12-29 (a Monday) as 2026-W01", () => {
    // 这一周的星期四是 2026-01-01,所以整周属于 2026 —— 年份必须取挪过之后那天的。
    expect(isoWeek(new Date(2025, 11, 29))).toEqual({ year: 2026, week: 1 });
  });

  it("counts 2027-01-01 (a Friday) as 2026-W53", () => {
    // 2026 有 53 个 ISO 周(1 月 1 日是周四),所以 2027 头三天还在 2026 里。
    expect(isoWeek(new Date(2027, 0, 1))).toEqual({ year: 2026, week: 53 });
  });

  it("counts 2026-01-04 (a Sunday) as 2026-W01 and 01-05 (Monday) as W02", () => {
    // 周日是本周最后一天,不是下一周第一天。ISO 里周一才换周。
    expect(isoWeek(new Date(2026, 0, 4))).toEqual({ year: 2026, week: 1 });
    expect(isoWeek(new Date(2026, 0, 5))).toEqual({ year: 2026, week: 2 });
  });

  it("stays on the calendar week across a DST switch", () => {
    /* 美东 2026-03-08 进夏令时,那天只有 23 小时。按本地毫秒差算周号会少一天,
       于是 03-09(周一)会被算回上一周。TZ 由测试环境决定,所以这里不假设时区,
       只断言「周一到周日是同一周,再往后一天就换周」这条恒等关系。 */
    const monday = isoWeek(new Date(2026, 2, 9));
    expect(isoWeek(new Date(2026, 2, 15))).toEqual(monday);
    expect(isoWeek(new Date(2026, 2, 16))).toEqual({ ...monday, week: monday.week + 1 });
  });
});

describe("templateVars", () => {
  it("derives date / month / quarter from the calendar year", () => {
    const vars = templateVars(new Date(2026, 7, 28));
    expect(vars.date).toBe("2026-08-28");
    expect(vars.month).toBe("2026-08");
    expect(vars.quarter).toBe(3);
    expect(vars.week).toBe("35");
  });

  it("pads the week number", () => {
    expect(templateVars(new Date(2026, 0, 8)).week).toBe("02");
  });

  it("takes year from the ISO week, not the calendar year", () => {
    // 2025-12-29 属于 2026-W01:周报标题写 2025-W01 会指向一年前那一周。
    expect(templateVars(new Date(2025, 11, 29)).year).toBe(2026);
  });

  it("puts each quarter boundary in the right quarter", () => {
    expect(templateVars(new Date(2026, 2, 31)).quarter).toBe(1);
    expect(templateVars(new Date(2026, 3, 1)).quarter).toBe(2);
    expect(templateVars(new Date(2026, 11, 31)).quarter).toBe(4);
  });
});

describe("buildTemplate", () => {
  it("expands placeholders in both the title and the body", () => {
    const built = buildTemplate(
      { nameKey: "n", bodyKey: "b" },
      new Date(2026, 7, 28),
      fakeT({ n: "{year}-W{week}", b: "# {date}\n\nQ{quarter} / {month}" }),
    );
    expect(built.title).toBe("2026-W35");
    expect(built.body).toBe("# 2026-08-28\n\nQ3 / 2026-08");
  });

  it("leaves unknown placeholders alone rather than writing undefined", () => {
    const built = buildTemplate(
      { nameKey: "n", bodyKey: "b" },
      new Date(2026, 7, 28),
      fakeT({ n: "t", b: "{nope}" }),
    );
    expect(built.body).toBe("{nope}");
  });
});

describe("the template catalog", () => {
  it("has a unique id per template", () => {
    const ids = NOTE_TEMPLATES.map((template) => template.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("does not ship a blank or folder template", () => {
    /* `blank` 就是现有的「新建随手记」,`folder` 在平铺列表里会建出一个面板看不见
       的空目录。两条都是刻意不搬的,不是漏了。 */
    const ids = NOTE_TEMPLATES.map((template) => String(template.id));
    expect(ids).not.toContain("blank");
    expect(ids).not.toContain("folder");
  });

  it.each([
    ["en", en],
    ["zh", zh],
  ])("has every key present in %s", (_lang, catalog) => {
    const keys = [
      ...NOTE_TEMPLATES.flatMap((template) => [
        template.titleKey,
        template.subKey,
        template.nameKey,
        template.bodyKey,
      ]),
      DAILY_TEMPLATE.nameKey,
      DAILY_TEMPLATE.bodyKey,
    ];
    for (const key of keys) {
      expect(catalog, key).toHaveProperty(key);
    }
  });

  it.each([
    ["en", en],
    ["zh", zh],
  ])("only uses placeholders templateVars provides, in %s", (_lang, catalog) => {
    /* 正文里写 `{quater}` 这种拼错的占位符不会报错,它会原样落进用户的笔记里。
       所以把两份文案里出现的占位符名字全收一遍,和 `templateVars` 的键对齐。 */
    const known = new Set(Object.keys(templateVars(new Date(2026, 7, 28))));
    const bodies = [
      ...NOTE_TEMPLATES.flatMap((template) => [
        catalog[template.nameKey],
        catalog[template.bodyKey],
      ]),
      catalog[DAILY_TEMPLATE.nameKey],
      catalog[DAILY_TEMPLATE.bodyKey],
    ];
    for (const text of bodies) {
      for (const match of (text ?? "").matchAll(/\{(\w+)\}/g)) {
        expect(known, `${text} → {${match[1]}}`).toContain(match[1]);
      }
    }
  });

  it.each([
    ["en", en],
    ["zh", zh],
  ])("renders every template to a non-empty body in %s", (_lang, catalog) => {
    const t = fakeT(catalog as Record<string, string>);
    for (const template of [...NOTE_TEMPLATES, DAILY_TEMPLATE]) {
      const built = buildTemplate(template, new Date(2026, 7, 28), t);
      expect(built.title.trim(), template.nameKey).not.toBe("");
      expect(built.body.trim(), template.bodyKey).not.toBe("");
      // 展开后不该还剩占位符 —— 剩下的说明用了 templateVars 没给的名字。
      expect(built.body, template.bodyKey).not.toMatch(/\{\w+\}/);
      expect(built.title, template.nameKey).not.toMatch(/\{\w+\}/);
    }
  });

  it("gives the daily note a title that is exactly the date", () => {
    /* 日记的文件名就是 `YYYY-MM-DD.md`,而标题存 frontmatter。两者不一致的话
       `[[2026-08-28]]` 指过去看到的标题会是另一个名字。 */
    const t = fakeT(en as unknown as Record<string, string>);
    expect(buildTemplate(DAILY_TEMPLATE, new Date(2026, 7, 28), t).title).toBe("2026-08-28");
  });
});
