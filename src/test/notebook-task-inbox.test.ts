import { describe, expect, it } from "vitest";

import {
  collectInboxTasks,
  compareTasks,
  countOpenTasks,
  dueBucket,
  filterInboxTasks,
  groupInboxTasks,
  parseTaskMarks,
  todayIso,
  type InboxTask,
  type NoteTaskSource,
} from "../components/notebook/noteTaskInbox";

/** 造一条任务。只给要断言的字段,其余取无意义的默认值。 */
function task(over: Partial<InboxTask> = {}): InboxTask {
  return {
    text: "任务",
    tags: [],
    path: "/vault/a.md",
    title: "A",
    line: 1,
    checked: false,
    raw: "任务",
    ...over,
  };
}

describe("parseTaskMarks", () => {
  it("摘出标签,文本里不再留下它们", () => {
    const parsed = parseTaskMarks("交稿 #写作 #deadline");
    expect(parsed.tags).toEqual(["写作", "deadline"]);
    expect(parsed.text).toBe("交稿");
  });

  it("`#` 前面不是空白就不算标签", () => {
    // `##heading`、`a#b`、锚点链接里的 `#` 都不该被当成标签 —— 和标签云同一条口径。
    const parsed = parseTaskMarks("看 ##标题 和 a#b 与 [x](#锚点)");
    expect(parsed.tags).toEqual([]);
    expect(parsed.text).toBe("看 ##标题 和 a#b 与 [x](#锚点)");
  });

  it("标签按归一化 key 去重,显示保留第一次的原样", () => {
    const parsed = parseTaskMarks("写 #Work 再写 #work");
    expect(parsed.tags).toEqual(["Work"]);
  });

  it("标签字符集不含 `.`,和 Rust 侧一致", () => {
    // Markio 那条正则含 `.`,于是 `#v1.2` 在两处会被数成不同的东西。
    const parsed = parseTaskMarks("发 #v1.2 版");
    expect(parsed.tags).toEqual(["v1"]);
    expect(parsed.text).toBe("发 .2 版");
  });

  it("归一化后空掉的标签不算,原文留在文本里", () => {
    const parsed = parseTaskMarks("分隔 #- 之后");
    expect(parsed.tags).toEqual([]);
    expect(parsed.text).toBe("分隔 #- 之后");
  });

  it("认 emoji 优先级,且 emoji 比 `!word` 优先", () => {
    expect(parseTaskMarks("修 🔴 bug").priority).toBe("high");
    expect(parseTaskMarks("看 🟡 一下").priority).toBe("med");
    expect(parseTaskMarks("整理 🟢 桌面").priority).toBe("low");
    // 两种都写时按 emoji —— 和 Markio 一致。
    expect(parseTaskMarks("修 🔴 bug !low").priority).toBe("high");
  });

  it("认 `!high` / `!med` / `!low`,并摘掉它", () => {
    const parsed = parseTaskMarks("交稿 !high");
    expect(parsed.priority).toBe("high");
    expect(parsed.text).toBe("交稿");
  });

  it("`!highest` 这种不是优先级", () => {
    // `\b` 收尾:`!high` 后面接字母时整体不算。
    const parsed = parseTaskMarks("喊 !highest 一声");
    expect(parsed.priority).toBeUndefined();
    expect(parsed.text).toBe("喊 !highest 一声");
  });

  it("认 📅 / @ / 末尾括号三种截止写法", () => {
    expect(parseTaskMarks("交稿 📅 2026-09-01").due).toBe("2026-09-01");
    expect(parseTaskMarks("交稿 @2026-09-01").due).toBe("2026-09-01");
    expect(parseTaskMarks("交稿 (2026-09-01)").due).toBe("2026-09-01");
  });

  it("截止日期取到之后从文本里摘掉", () => {
    // 留着的话同一个信息显示两遍:一遍在日期徽标上,一遍在任务文本里。
    expect(parseTaskMarks("交稿 @2026-09-01").text).toBe("交稿");
    expect(parseTaskMarks("交稿 📅 2026-09-01").text).toBe("交稿");
    expect(parseTaskMarks("交稿 (2026-09-01)").text).toBe("交稿");
  });

  it("📅 比 @ 优先,@ 比末尾括号优先", () => {
    expect(parseTaskMarks("交稿 📅 2026-09-01 @2026-10-01").due).toBe("2026-09-01");
    expect(parseTaskMarks("交稿 @2026-10-01 (2026-11-01)").due).toBe("2026-10-01");
  });

  it("裸 ISO 日期在首尾算截止", () => {
    expect(parseTaskMarks("2026-09-01 交稿").due).toBe("2026-09-01");
    expect(parseTaskMarks("2026-09-01 交稿").text).toBe("交稿");
    expect(parseTaskMarks("交稿 2026-09-01").due).toBe("2026-09-01");
    expect(parseTaskMarks("交稿 2026-09-01").text).toBe("交稿");
  });

  it("句子中间的裸日期不算截止", () => {
    // Markio 的兜底认"任意位置",于是这一条会凭空长出一个截止日期。
    const parsed = parseTaskMarks("复盘 2026-08-01 那次故障");
    expect(parsed.due).toBeUndefined();
    expect(parsed.text).toBe("复盘 2026-08-01 那次故障");
  });

  it("不存在的日期不算截止", () => {
    // 形状对但不存在。收进来的话排序按字符串、分桶按 Date,同一条任务会有两个日期。
    const parsed = parseTaskMarks("交稿 @2026-02-30");
    expect(parsed.due).toBeUndefined();
    expect(parsed.text).toBe("交稿 @2026-02-30");
  });

  it("闰年 2 月 29 日算,平年不算", () => {
    expect(parseTaskMarks("交稿 @2028-02-29").due).toBe("2028-02-29");
    expect(parseTaskMarks("交稿 @2026-02-29").due).toBeUndefined();
  });

  it("月份为 0 或 13 不算", () => {
    expect(parseTaskMarks("交稿 @2026-00-10").due).toBeUndefined();
    expect(parseTaskMarks("交稿 @2026-13-10").due).toBeUndefined();
  });

  it("三种标记一起出现时都能摘干净", () => {
    const parsed = parseTaskMarks("交稿 #写作 @2026-09-01 !high");
    expect(parsed).toEqual({
      text: "交稿",
      tags: ["写作"],
      due: "2026-09-01",
      priority: "high",
    });
  });

  it("整行只有标记时文本是空串", () => {
    const parsed = parseTaskMarks("#写作 @2026-09-01");
    expect(parsed.text).toBe("");
    expect(parsed.tags).toEqual(["写作"]);
    expect(parsed.due).toBe("2026-09-01");
  });

  it("折叠多余空白", () => {
    expect(parseTaskMarks("交    稿   #写作").text).toBe("交 稿");
  });

  it("没有任何标记时原文照旧", () => {
    const parsed = parseTaskMarks("写周报");
    expect(parsed).toEqual({
      text: "写周报",
      tags: [],
      due: undefined,
      priority: undefined,
      progress: undefined,
    });
  });

  it("认 `{30%}` 完成度并摘掉它", () => {
    const parsed = parseTaskMarks("写周报 {30%}");
    expect(parsed.progress).toBe(30);
    expect(parsed.text).toBe("写周报");
  });

  it("完成度超过 100 夹到 100", () => {
    expect(parseTaskMarks("交稿 {999%}").progress).toBe(100);
  });

  it("`{0%}` 是写了 0,不是没写", () => {
    expect(parseTaskMarks("交稿 {0%}").progress).toBe(0);
    expect(parseTaskMarks("交稿").progress).toBeUndefined();
  });

  it("完成度要求词边界,不吃 `width:{50%}` 这种", () => {
    // Markio 那条是裸的 \{(\d{1,3})%\},会把它当完成度**并从文本里抹掉**。
    const parsed = parseTaskMarks("改 CSS width:{50%} 那一行");
    expect(parsed.progress).toBeUndefined();
    expect(parsed.text).toBe("改 CSS width:{50%} 那一行");
  });

  it("四种标记一起出现时都能摘干净", () => {
    const parsed = parseTaskMarks("交稿 #写作 !high @2026-09-01 {30%}");
    expect(parsed).toEqual({
      text: "交稿",
      tags: ["写作"],
      due: "2026-09-01",
      priority: "high",
      progress: 30,
    });
  });
});

describe("todayIso", () => {
  it("取的是本地日期,不是 UTC 日期", () => {
    /* Markio 那份用 `now.toISOString().slice(0, 10)`。UTC+8 在早上 08:00 之前,
       那给的是昨天 —— 今天到期的任务被分到「明天」,过期的分到「今天」。
       这里造一个"本地 8 月 28 日凌晨、UTC 还在 27 日"的时刻:构造函数吃的是本地
       时间,所以在任何正时区下 toISOString 都会退到前一天。 */
    const localMidnight = new Date(2026, 7, 28, 0, 30, 0);
    expect(todayIso(localMidnight)).toBe("2026-08-28");
    expect(localMidnight.getDate()).toBe(28);
  });

  it("月和日补零", () => {
    expect(todayIso(new Date(2026, 0, 5))).toBe("2026-01-05");
  });
});

describe("dueBucket", () => {
  const today = "2026-08-28";

  it("没有截止是 none", () => {
    expect(dueBucket(undefined, today)).toBe("none");
  });

  it("过去是 overdue,今天是 today,明天是 tomorrow", () => {
    expect(dueBucket("2026-08-27", today)).toBe("overdue");
    expect(dueBucket(today, today)).toBe("today");
    expect(dueBucket("2026-08-29", today)).toBe("tomorrow");
  });

  it("7 天内是 thisWeek,第 8 天起是 later", () => {
    expect(dueBucket("2026-08-30", today)).toBe("thisWeek");
    expect(dueBucket("2026-09-04", today)).toBe("thisWeek");
    expect(dueBucket("2026-09-05", today)).toBe("later");
  });

  it("跨月和跨年都按真实日历算", () => {
    // 字符串比较能判先后,但"+1 天"必须真的走日历 —— 8-31 的明天是 9-1。
    expect(dueBucket("2026-09-01", "2026-08-31")).toBe("tomorrow");
    expect(dueBucket("2027-01-01", "2026-12-31")).toBe("tomorrow");
    expect(dueBucket("2027-01-05", "2026-12-31")).toBe("thisWeek");
  });

  it("闰年 2 月末的明天是 2 月 29 日", () => {
    expect(dueBucket("2028-02-29", "2028-02-28")).toBe("tomorrow");
  });
});

describe("compareTasks", () => {
  it("未完成排在已完成前面", () => {
    const done = task({ checked: true, text: "a" });
    const open = task({ checked: false, text: "z" });
    expect([done, open].sort(compareTasks)).toEqual([open, done]);
  });

  it("按优先级 high → med → low → 无", () => {
    const none = task({ text: "d" });
    const low = task({ text: "c", priority: "low" });
    const med = task({ text: "b", priority: "med" });
    const high = task({ text: "a", priority: "high" });
    expect([none, low, med, high].sort(compareTasks).map((it) => it.text)).toEqual([
      "a",
      "b",
      "c",
      "d",
    ]);
  });

  it("同优先级时有截止的在前,早的在前", () => {
    const late = task({ text: "b", due: "2026-09-02" });
    const early = task({ text: "c", due: "2026-09-01" });
    const noDue = task({ text: "a" });
    expect([noDue, late, early].sort(compareTasks).map((it) => it.text)).toEqual(["c", "b", "a"]);
  });

  it("全打平时按路径再按行号,不是不稳定的", () => {
    /* Markio 的比较器只以文本比较收尾:同名任务永远返回 0,顺序由 sort 的
       实现和输入顺序决定,两次扫描之间会跳。 */
    const a = task({ path: "/vault/a.md", line: 9, text: "同名" });
    const b = task({ path: "/vault/b.md", line: 2, text: "同名" });
    const c = task({ path: "/vault/a.md", line: 3, text: "同名" });
    const sorted = [b, a, c].sort(compareTasks);
    expect(sorted.map((it) => `${it.path}:${it.line}`)).toEqual([
      "/vault/a.md:3",
      "/vault/a.md:9",
      "/vault/b.md:2",
    ]);
    // 换一个输入顺序,结果必须一样。
    expect([c, b, a].sort(compareTasks)).toEqual(sorted);
  });
});

describe("collectInboxTasks", () => {
  const sources: NoteTaskSource[] = [
    {
      path: "/vault/plan.md",
      tasks: [
        { line: 3, checked: false, text: "交稿 #写作 @2026-09-01 !high" },
        { line: 5, checked: true, text: "已完成的" },
      ],
    },
    {
      path: "/vault/other.md",
      tasks: [{ line: 2, checked: false, text: "写周报" }],
    },
  ];

  it("解析标记并带上来源", () => {
    const tasks = collectInboxTasks(sources, (path) => (path.includes("plan") ? "计划" : "其它"));
    const first = tasks[0]!;
    expect(first).toMatchObject({
      text: "交稿",
      tags: ["写作"],
      due: "2026-09-01",
      priority: "high",
      path: "/vault/plan.md",
      title: "计划",
      line: 3,
      checked: false,
    });
  });

  it("保留未摘标记的原文", () => {
    // 「复制任务文本」和悬浮提示用它 —— 用户写的是这个。
    const tasks = collectInboxTasks(sources, () => "T");
    expect(tasks[0]!.raw).toBe("交稿 #写作 @2026-09-01 !high");
  });

  it("返回时已排好序", () => {
    const tasks = collectInboxTasks(sources, () => "T");
    // high 的在前,已完成的在最后。
    expect(tasks.map((it) => it.text)).toEqual(["交稿", "写周报", "已完成的"]);
  });

  it("标题走调用方给的口径", () => {
    const tasks = collectInboxTasks(sources, () => "统一标题");
    expect(new Set(tasks.map((it) => it.title))).toEqual(new Set(["统一标题"]));
  });

  it("空输入返回空数组", () => {
    expect(collectInboxTasks([], () => "T")).toEqual([]);
  });
});

describe("filterInboxTasks", () => {
  const tasks = [
    task({ text: "交稿", tags: ["写作"], title: "计划", path: "/vault/plan.md" }),
    task({ text: "写周报", title: "其它", path: "/vault/other.md" }),
    task({ text: "做完了", title: "计划", path: "/vault/plan.md", checked: true }),
  ];

  it("默认藏掉已完成的", () => {
    expect(filterInboxTasks(tasks, "", false).map((it) => it.text)).toEqual(["交稿", "写周报"]);
  });

  it("开了显示已完成就都出来", () => {
    expect(filterInboxTasks(tasks, "", true)).toHaveLength(3);
  });

  it("匹配任务文本", () => {
    expect(filterInboxTasks(tasks, "周报", true).map((it) => it.text)).toEqual(["写周报"]);
  });

  it("匹配标签", () => {
    expect(filterInboxTasks(tasks, "写作", true).map((it) => it.text)).toEqual(["交稿"]);
  });

  it("匹配笔记标题", () => {
    expect(filterInboxTasks(tasks, "计划", true).map((it) => it.text)).toEqual(["交稿", "做完了"]);
  });

  it("大小写无关", () => {
    const mixed = [task({ text: "Ship IT", tags: ["Work"] })];
    expect(filterInboxTasks(mixed, "ship", true)).toHaveLength(1);
    expect(filterInboxTasks(mixed, "work", true)).toHaveLength(1);
  });

  it("筛选和「显示已完成」同时生效", () => {
    expect(filterInboxTasks(tasks, "计划", false).map((it) => it.text)).toEqual(["交稿"]);
  });

  it("不匹配原文", () => {
    /* 匹配 raw 的话 `@2026` 这种输入会命中一批文本里根本看不到那个词的任务,
       用户会以为筛选坏了。 */
    const withRaw = [task({ text: "交稿", raw: "交稿 @2026-09-01" })];
    expect(filterInboxTasks(withRaw, "2026", true)).toEqual([]);
  });

  it("只有空白的输入等于没筛", () => {
    expect(filterInboxTasks(tasks, "   ", true)).toHaveLength(3);
  });
});

describe("groupInboxTasks", () => {
  const today = "2026-08-28";

  it("按时间分组,顺序是 过期 → 今天 → 明天 → 本周 → 以后 → 无", () => {
    const tasks = [
      task({ text: "以后", due: "2026-12-01" }),
      task({ text: "无期限" }),
      task({ text: "今天", due: today }),
      task({ text: "过期", due: "2026-08-01" }),
      task({ text: "本周", due: "2026-09-02" }),
      task({ text: "明天", due: "2026-08-29" }),
    ];
    expect(groupInboxTasks(tasks, "time", today).map((g) => g.key)).toEqual([
      "overdue",
      "today",
      "tomorrow",
      "thisWeek",
      "later",
      "none",
    ]);
  });

  it("空组不出现", () => {
    // 六个时间桶里通常只有两三个有内容,把空的画出来会让清单看起来全是标题。
    const groups = groupInboxTasks([task({ due: today })], "time", today);
    expect(groups.map((g) => g.key)).toEqual(["today"]);
  });

  it("按优先级分组,顺序是 高 → 中 → 低 → 无", () => {
    const tasks = [
      task({ text: "无" }),
      task({ text: "低", priority: "low" }),
      task({ text: "高", priority: "high" }),
      task({ text: "中", priority: "med" }),
    ];
    expect(groupInboxTasks(tasks, "priority", today).map((g) => g.key)).toEqual([
      "high",
      "med",
      "low",
      "_",
    ]);
  });

  it("按笔记分组,组名是标题、组间按标题字典序", () => {
    const tasks = [
      task({ path: "/vault/z.md", title: "阿计划" }),
      task({ path: "/vault/a.md", title: "本周" }),
    ];
    const groups = groupInboxTasks(tasks, "note", today);
    expect(groups.map((g) => g.title)).toEqual(["阿计划", "本周"]);
    // key 是路径 —— 两篇同名笔记不能折成一组。
    expect(groups.map((g) => g.key)).toEqual(["/vault/z.md", "/vault/a.md"]);
  });

  it("同标题的两篇笔记仍然是两组,顺序稳定", () => {
    const tasks = [
      task({ path: "/vault/b.md", title: "同名", line: 1 }),
      task({ path: "/vault/a.md", title: "同名", line: 1 }),
    ];
    const groups = groupInboxTasks(tasks, "note", today);
    expect(groups.map((g) => g.key)).toEqual(["/vault/a.md", "/vault/b.md"]);
  });

  it("组内按 compareTasks 排,不是按输入顺序", () => {
    const tasks = [
      task({ text: "低", due: today, priority: "low" }),
      task({ text: "高", due: today, priority: "high" }),
    ];
    const groups = groupInboxTasks(tasks, "time", today);
    expect(groups[0]!.tasks.map((it) => it.text)).toEqual(["高", "低"]);
  });

  it("每组带上分组方式,好让调用方决定文案从哪来", () => {
    expect(groupInboxTasks([task()], "time", today)[0]!.kind).toBe("time");
    expect(groupInboxTasks([task()], "priority", today)[0]!.kind).toBe("priority");
    expect(groupInboxTasks([task()], "note", today)[0]!.kind).toBe("note");
  });

  it("不改动传进来的数组", () => {
    const tasks = [task({ text: "b", due: today }), task({ text: "a", due: today })];
    const before = tasks.map((it) => it.text);
    groupInboxTasks(tasks, "time", today);
    expect(tasks.map((it) => it.text)).toEqual(before);
  });

  it("空输入返回空数组", () => {
    expect(groupInboxTasks([], "time", today)).toEqual([]);
  });
});

describe("countOpenTasks", () => {
  it("只数未完成的", () => {
    const tasks = [task(), task({ checked: true }), task()];
    expect(countOpenTasks(tasks)).toBe(2);
  });

  it("全做完是 0", () => {
    expect(countOpenTasks([task({ checked: true })])).toBe(0);
  });

  it("空数组是 0", () => {
    expect(countOpenTasks([])).toBe(0);
  });
});
