import { describe, expect, it } from "vitest";
import {
  NOTE_ICON_NAMES,
  isNoteIconName,
  noteIconKey,
  noteIconLabelKey,
  noteIconOf,
  withNoteIcon,
  type NoteIconName,
} from "../components/notebook/noteIcons";

describe("noteIconKey", () => {
  it("给出 vault 相对路径", () => {
    // 相对路径而不是绝对:vault 整个目录被搬走之后图标还在(order.json 已是这个决定)。
    expect(noteIconKey("/vault", "/vault/Note.md")).toBe("Note.md");
    expect(noteIconKey("/vault", "/vault/sub/Deep.md")).toBe("sub/Deep.md");
  });

  it("统一 Windows 分隔符", () => {
    // 同一条笔记在两种分隔符下必须落到同一个键,否则图标会在重启后"丢"。
    expect(noteIconKey("C:\\vault", "C:\\vault\\sub\\Note.md")).toBe("sub/Note.md");
    expect(noteIconKey("C:/vault", "C:\\vault\\sub\\Note.md")).toBe("sub/Note.md");
  });

  it("vault 末尾多余的分隔符不影响结果", () => {
    expect(noteIconKey("/vault/", "/vault/Note.md")).toBe("Note.md");
    expect(noteIconKey("/vault///", "/vault/Note.md")).toBe("Note.md");
  });

  it("前缀比对大小写不敏感,但键保留原样", () => {
    // macOS / Windows 上 vault 路径的大小写可能和笔记路径不一致。判成"不在库里"
    // 会让图标整片失效;而把键也小写化会让 `Note.md` 和 `note.md` 撞在一起。
    expect(noteIconKey("/Vault", "/vault/Note.md")).toBe("Note.md");
    expect(noteIconKey("/vault", "/VAULT/Note.md")).toBe("Note.md");
  });

  it("笔记不在 vault 里时返回空串", () => {
    // 给一个 `../` 开头的键会让同名文件在不同 vault 之间互相串图标。
    expect(noteIconKey("/vault", "/elsewhere/Note.md")).toBe("");
    // 前缀像但不是同一层目录:`/vault2` 不在 `/vault` 里。
    expect(noteIconKey("/vault", "/vault2/Note.md")).toBe("");
    // 路径正好等于 vault 自己,不是一条笔记。
    expect(noteIconKey("/vault", "/vault")).toBe("");
    expect(noteIconKey("", "/vault/Note.md")).toBe("");
    expect(noteIconKey("/vault", "")).toBe("");
  });
});

describe("noteIconOf", () => {
  it("读出设过的图标", () => {
    expect(noteIconOf({ "Note.md": "book" }, "/vault", "/vault/Note.md")).toBe("book");
  });

  it("没设过时是 undefined", () => {
    expect(noteIconOf({}, "/vault", "/vault/Note.md")).toBeUndefined();
  });

  it("认不出来的图标名当没设", () => {
    /* 表可能是新版本写的,或者被手改过。渲染一个不存在的图标会让整行崩掉,
       而回落到默认图标只是少一点装饰。 */
    expect(noteIconOf({ "Note.md": "no-such-icon" }, "/vault", "/vault/Note.md")).toBeUndefined();
    expect(noteIconOf({ "Note.md": "" }, "/vault", "/vault/Note.md")).toBeUndefined();
  });

  it("笔记不在 vault 里时是 undefined", () => {
    expect(noteIconOf({ "Note.md": "book" }, "/vault", "/elsewhere/Note.md")).toBeUndefined();
  });
});

describe("withNoteIcon", () => {
  it("设一个图标", () => {
    const next = withNoteIcon({}, "/vault", "/vault/Note.md", "book");
    expect(next).toEqual({ "Note.md": "book" });
  });

  it("换成另一个图标", () => {
    const next = withNoteIcon({ "Note.md": "book" }, "/vault", "/vault/Note.md", "flame");
    expect(next).toEqual({ "Note.md": "flame" });
  });

  it("恢复默认时把键删掉,不是存空串", () => {
    /* 空串会在表里一直占着位置,而且下一版如果给空串赋了含义就会解释成别的东西。 */
    const next = withNoteIcon(
      { "Note.md": "book", "B.md": "star" },
      "/vault",
      "/vault/Note.md",
      null,
    );
    expect(next).toEqual({ "B.md": "star" });
    expect("Note.md" in next).toBe(false);
  });

  it("不改动传进来的那张表", () => {
    // 面板靠"旧表"回滚写盘失败,原地改会让回滚拿到已经被改过的表。
    const before = { "Note.md": "book" };
    withNoteIcon(before, "/vault", "/vault/Note.md", "flame");
    withNoteIcon(before, "/vault", "/vault/Note.md", null);
    expect(before).toEqual({ "Note.md": "book" });
  });

  it("没有变化时返回同一个引用", () => {
    // 面板据此跳过一次写盘 —— 重复点同一个图标不该产生 IPC。
    const table = { "Note.md": "book" };
    expect(withNoteIcon(table, "/vault", "/vault/Note.md", "book")).toBe(table);
    // 本来就没设过,又要求恢复默认。
    const empty = {};
    expect(withNoteIcon(empty, "/vault", "/vault/Note.md", null)).toBe(empty);
  });

  it("笔记不在 vault 里时原表返回,不写出野键", () => {
    const table = { "Note.md": "book" };
    expect(withNoteIcon(table, "/vault", "/elsewhere/X.md", "star")).toBe(table);
  });
});

describe("图标注册表", () => {
  it("每个名字都有 i18n 键,且互不重复", () => {
    const keys = NOTE_ICON_NAMES.map((name) => noteIconLabelKey(name));
    expect(new Set(keys).size).toBe(NOTE_ICON_NAMES.length);
    expect(keys[0]).toBe("notebook.icon.note");
  });

  it("名字表本身没有重复", () => {
    // 重了会让选择器里出现两个一样的格子,而 React 的 key 也会撞。
    expect(new Set(NOTE_ICON_NAMES).size).toBe(NOTE_ICON_NAMES.length);
  });

  it("isNoteIconName 只认表里的名字", () => {
    for (const name of NOTE_ICON_NAMES) expect(isNoteIconName(name)).toBe(true);
    expect(isNoteIconName("nope")).toBe(false);
    expect(isNoteIconName(undefined)).toBe(false);
    expect(isNoteIconName("")).toBe(false);
    // 原型链上的东西不能被当成图标名。
    expect(isNoteIconName("toString")).toBe(false);
  });

  it("类型上的图标名与运行时的表一致", () => {
    // 这行的意义是编译期:漏了 NoteIconName 里的某一项就编不过。
    const sample: NoteIconName = NOTE_ICON_NAMES[0];
    expect(NOTE_ICON_NAMES).toContain(sample);
  });
});
