/* 自定义模板的展开(占位符替换 + 转成面板要的形状)。
 *
 * 后端那份(读目录、拆 frontmatter)在 `src-tauri/src/notebook/user_templates.rs` 里有
 * 自己的单元测试。这里只管前端这一层。
 */

import { describe, expect, it } from "vitest";
import {
  expandUserTemplate,
  fillDateTime,
  fillTitle,
  hhmm,
  userTemplateKeywords,
} from "../components/notebook/noteUserTemplates";
import type { UserTemplate } from "../components/notebook/notebookApi";

function template(overrides: Partial<UserTemplate> = {}): UserTemplate {
  return {
    id: "meeting",
    title: "会议纪要",
    name: "{{date}} 会议",
    body: "# {{title}}\n",
    ...overrides,
  };
}

describe("hhmm", () => {
  it("补零到两位", () => {
    expect(hhmm(new Date(2026, 7, 28, 9, 5))).toBe("09:05");
    expect(hhmm(new Date(2026, 7, 28, 14, 30))).toBe("14:30");
  });

  it("午夜是 00:00 而不是 24:00", () => {
    expect(hhmm(new Date(2026, 7, 28, 0, 0))).toBe("00:00");
  });
});

describe("fillDateTime", () => {
  const now = new Date(2026, 7, 28, 14, 7);

  it("展开 date 与 time", () => {
    expect(fillDateTime("{{date}} {{time}}", now)).toBe("2026-08-28 14:07");
  });

  it("同一个占位符出现几次就替换几次", () => {
    // 用 replace 而不是 replaceAll(或漏了 /g)时只有第一处会变。
    expect(fillDateTime("{{date}} → {{date}}", now)).toBe("2026-08-28 → 2026-08-28");
  });

  it("大小写与内部空格都认", () => {
    // 用户写 {{Date}} 的意思毫无疑问是同一个占位符。留一个没展开的看起来像坏了。
    expect(fillDateTime("{{Date}} {{ TIME }}", now)).toBe("2026-08-28 14:07");
  });

  it("不动 title", () => {
    // 它要等最终标题定下来才替换。这一步就替掉的话正文里的 # {{title}} 会变成空。
    expect(fillDateTime("# {{title}}", now)).toBe("# {{title}}");
  });

  it("不认识的占位符原样留着", () => {
    // 静默删掉的话用户以为自己写错了名字;留着才看得出「这个我不支持」。
    expect(fillDateTime("{{author}}", now)).toBe("{{author}}");
  });
});

describe("fillTitle", () => {
  it("替换所有 title,大小写不敏感", () => {
    expect(fillTitle("# {{title}}\n\n见 {{Title}}", "周会")).toBe("# 周会\n\n见 周会");
  });

  it("替换成空串就是删掉", () => {
    expect(fillTitle("{{title}} 会议", "")).toBe(" 会议");
  });
});

describe("expandUserTemplate", () => {
  const now = new Date(2026, 7, 28, 14, 7);

  it("id 带 user: 前缀", () => {
    // 和内置模板的 id 分开:两边都可能叫 meeting,撞了之后命令面板里会有两条
    // 一样的 key。
    expect(expandUserTemplate(template(), now).id).toBe("user:meeting");
  });

  it("name 里的日期展开,正文里的 title 留着", () => {
    const entry = expandUserTemplate(template(), now);
    expect(entry.name).toBe("2026-08-28 会议");
    expect(entry.body).toBe("# {{title}}\n");
  });

  it("name 里的 title 占位符被删掉", () => {
    // 拿标题当标题的一部分是循环引用。
    const entry = expandUserTemplate(template({ name: "{{title}} {{date}}" }), now);
    expect(entry.name).toBe("2026-08-28");
  });

  it("name 展开完是空的就回落到 title", () => {
    // 一条没有标题的新笔记在列表里是一行点不动的空白。
    const entry = expandUserTemplate(template({ name: "{{title}}" }), now);
    expect(entry.name).toBe("会议纪要");
  });

  it("正文里的日期也展开", () => {
    const entry = expandUserTemplate(template({ body: "记于 {{date}} {{time}}\n" }), now);
    expect(entry.body).toBe("记于 2026-08-28 14:07\n");
  });

  it("title 原样传出去", () => {
    // 它是命令面板里的 label,不是文件名,不参与占位符。
    expect(expandUserTemplate(template({ title: "{{date}} 不该展开" }), now).title).toBe(
      "{{date}} 不该展开",
    );
  });
});

describe("userTemplateKeywords", () => {
  it("给出文件名 stem", () => {
    // 用户可能记得的是文件叫什么,而不是 frontmatter 里写的显示名。
    expect(userTemplateKeywords(template())).toEqual(["meeting"]);
  });

  it("stem 和标题一样时不重复给", () => {
    expect(userTemplateKeywords(template({ id: "会议纪要" }))).toEqual([]);
  });
});
