import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  loadNoteRecents,
  NOTE_RECENTS_LIMIT,
  resolveNoteRecents,
  saveNoteRecents,
  touchNoteRecent,
} from "../components/notebook/noteRecents";

const VAULT = "/Users/me/notes";

beforeEach(() => {
  localStorage.clear();
});

describe("touchNoteRecent", () => {
  it("按 vault 相对路径记账,不记绝对路径", () => {
    /* 这是整个模块最要紧的一条。存绝对路径的话,vault 换个位置(或同一份笔记在
       另一台机器上)整份名单会静默全部失效 —— 面板不报错,只是空查询下什么都不列。 */
    expect(touchNoteRecent(VAULT, `${VAULT}/a/Index.md`, [])).toEqual(["a/Index.md"]);
  });

  it("插到最前面", () => {
    expect(touchNoteRecent(VAULT, `${VAULT}/B.md`, ["A.md"])).toEqual(["B.md", "A.md"]);
  });

  it("重复打开同一篇只改变位置,不留两条", () => {
    expect(touchNoteRecent(VAULT, `${VAULT}/A.md`, ["B.md", "A.md", "C.md"])).toEqual([
      "A.md",
      "B.md",
      "C.md",
    ]);
  });

  it("触顶时丢最旧的", () => {
    const full = Array.from({ length: NOTE_RECENTS_LIMIT }, (_, i) => `${i}.md`);
    const next = touchNoteRecent(VAULT, `${VAULT}/new.md`, full);
    expect(next).toHaveLength(NOTE_RECENTS_LIMIT);
    expect(next[0]).toBe("new.md");
    expect(next).not.toContain(`${NOTE_RECENTS_LIMIT - 1}.md`);
  });

  it("笔记不在 vault 里时原样返回", () => {
    /* 算不出相对路径就不记。硬塞一个 `../` 开头的键会让不同库的同名文件互相串 ——
       用户在 A 库里看到 B 库的笔记名,点了还打不开。 */
    expect(touchNoteRecent(VAULT, "/elsewhere/Ghost.md", ["A.md"])).toEqual(["A.md"]);
  });

  it("不改传进来的数组", () => {
    // 它是 state,原地改会让 React 看不到变化。
    const current = ["A.md"];
    touchNoteRecent(VAULT, `${VAULT}/B.md`, current);
    expect(current).toEqual(["A.md"]);
  });

  it("vault 为空时原样返回", () => {
    expect(touchNoteRecent("", "/x/A.md", ["A.md"])).toEqual(["A.md"]);
  });
});

describe("loadNoteRecents / saveNoteRecents", () => {
  it("存了能读回来", () => {
    saveNoteRecents(VAULT, ["A.md", "b/C.md"]);
    expect(loadNoteRecents(VAULT)).toEqual(["A.md", "b/C.md"]);
  });

  it("按 vault 分开存:换库不该看到上一个库的历史", () => {
    saveNoteRecents(VAULT, ["A.md"]);
    saveNoteRecents("/other/vault", ["B.md"]);
    expect(loadNoteRecents(VAULT)).toEqual(["A.md"]);
    expect(loadNoteRecents("/other/vault")).toEqual(["B.md"]);
  });

  it("vault 路径末尾的斜杠不影响是同一个库", () => {
    // 调用方可能从不同地方拿到带不带斜杠的路径,那不该分裂成两份名单。
    saveNoteRecents(`${VAULT}/`, ["A.md"]);
    expect(loadNoteRecents(VAULT)).toEqual(["A.md"]);
  });

  it("Windows 反斜杠与正斜杠算同一个库", () => {
    saveNoteRecents("C:\\notes", ["A.md"]);
    expect(loadNoteRecents("C:/notes")).toEqual(["A.md"]);
  });

  it("没存过时是空名单", () => {
    expect(loadNoteRecents(VAULT)).toEqual([]);
  });

  it("存坏了当空名单,不抛", () => {
    // 手改过、或者上个版本的格式。让面板打不开比少一份名单糟得多。
    localStorage.setItem(`aeroric:notebookRecents:${VAULT}`, "{not json");
    expect(loadNoteRecents(VAULT)).toEqual([]);
  });

  it("存的不是数组时当空名单", () => {
    localStorage.setItem(`aeroric:notebookRecents:${VAULT}`, '{"a":1}');
    expect(loadNoteRecents(VAULT)).toEqual([]);
  });

  it("名单里的非字符串项被滤掉", () => {
    localStorage.setItem(`aeroric:notebookRecents:${VAULT}`, '["A.md",42,null,"B.md"]');
    expect(loadNoteRecents(VAULT)).toEqual(["A.md", "B.md"]);
  });

  it("读的时候也去重", () => {
    localStorage.setItem(`aeroric:notebookRecents:${VAULT}`, '["A.md","A.md","B.md"]');
    expect(loadNoteRecents(VAULT)).toEqual(["A.md", "B.md"]);
  });

  it("读的时候也守上限", () => {
    const tooMany = JSON.stringify(
      Array.from({ length: NOTE_RECENTS_LIMIT + 5 }, (_, i) => `${i}.md`),
    );
    localStorage.setItem(`aeroric:notebookRecents:${VAULT}`, tooMany);
    expect(loadNoteRecents(VAULT)).toHaveLength(NOTE_RECENTS_LIMIT);
  });

  it("存不下时不抛", () => {
    /* 配额满 / 隐私模式。丢一份 UI 便利名单不该让「打开笔记」这个动作失败 ——
       而它是在打开笔记的那条路径上被调用的。 */
    const spy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    try {
      expect(() => saveNoteRecents(VAULT, ["A.md"])).not.toThrow();
    } finally {
      spy.mockRestore();
    }
  });

  it("vault 为空时不读不写", () => {
    saveNoteRecents("", ["A.md"]);
    expect(loadNoteRecents("")).toEqual([]);
  });
});

describe("resolveNoteRecents", () => {
  it("相对路径换回当前的绝对路径", () => {
    expect(
      resolveNoteRecents(VAULT, ["a/Index.md"], [`${VAULT}/a/Index.md`, `${VAULT}/B.md`]),
    ).toEqual([`${VAULT}/a/Index.md`]);
  });

  it("保持名单自己的顺序,不用笔记列表的顺序", () => {
    // 笔记列表是修改时间倒序,而这份名单是「最近打开」—— 两者不是一回事。
    expect(resolveNoteRecents(VAULT, ["B.md", "A.md"], [`${VAULT}/A.md`, `${VAULT}/B.md`])).toEqual(
      [`${VAULT}/B.md`, `${VAULT}/A.md`],
    );
  });

  it("对不上的条目静默丢掉", () => {
    /* 笔记被删了、或被外部改了名,名单里就会留下死条目。面板拿到不存在的 id 会
       渲染一行点了没反应的候选,那比少一行糟得多。 */
    expect(resolveNoteRecents(VAULT, ["Gone.md", "A.md"], [`${VAULT}/A.md`])).toEqual([
      `${VAULT}/A.md`,
    ]);
  });

  it("同名不同目录不会互相串", () => {
    // 键是相对路径而不是文件名,所以 a/Index.md 和 b/Index.md 是两条不同的记录。
    expect(
      resolveNoteRecents(VAULT, ["b/Index.md"], [`${VAULT}/a/Index.md`, `${VAULT}/b/Index.md`]),
    ).toEqual([`${VAULT}/b/Index.md`]);
  });

  it("vault 换了位置,名单照样对得上", () => {
    /* 存相对路径的整个意义在这里:同一份笔记搬到别处(或在另一台机器上),名单
       仍然有效。 */
    const moved = "/Volumes/backup/notes";
    expect(resolveNoteRecents(moved, ["a/Index.md"], [`${moved}/a/Index.md`])).toEqual([
      `${moved}/a/Index.md`,
    ]);
  });

  it("vault 为空时返回空", () => {
    expect(resolveNoteRecents("", ["A.md"], ["/x/A.md"])).toEqual([]);
  });

  it("不在 vault 里的笔记不进结果", () => {
    expect(resolveNoteRecents(VAULT, ["A.md"], ["/elsewhere/A.md"])).toEqual([]);
  });
});
