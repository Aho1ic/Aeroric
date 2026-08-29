import { describe, expect, it } from "vitest";
import { buildLinkIndex, scanWikiLinks } from "../components/notebook/noteLinks";
import {
  bodyOffsetOfFileLine,
  collectBacklinks,
  countBacklinks,
  offsetOfLine,
  type NoteLinkSource,
} from "../components/notebook/noteBacklinks";

/* 反链的三层里,这个文件盯住第三层(折叠 / 去重 / 排序 / 自引用),外加一份
 * 跨语言的黄金用例。
 *
 * 第一层(Rust 的词法提取)在 `notebook/links.rs`,第二层(`[[foo]]` 该指向谁)
 * 在 `notebook-note-links.test.ts`。 */

const VAULT = "/vault";
const note = (path: string, title: string) => ({ path, title });

/** 造一条来源笔记。行号从 1 开始按数组下标给,预览就是那一行本身。 */
function source(path: string, lines: string[]): NoteLinkSource {
  const links: NoteLinkSource["links"] = [];
  lines.forEach((line, index) => {
    for (const hit of scanWikiLinks(line)) {
      links.push({ raw: hit.raw, line: index + 1, preview: line, embed: hit.embed });
    }
  });
  return { path, links };
}

describe("与 Rust 侧词法提取的共享黄金用例", () => {
  /* 同一张表在 `src-tauri/src/notebook/links.rs` 的 `golden()` 里跑 `scan_line`,
     期望值逐字相同。

     为什么要有这份重复:Rust 那层是手写的正则等价物(为了逐行拿行号 + 不把整个
     vault 的正文搬进 JS)。两边各自的注释都写着"我和另一边等价" —— 声明不值钱,
     同一张表两边都过才值钱。改这里的任何一行,记得同步改那边。 */
  const golden: [string, string[]][] = [
    ["见 [[周报]] 和 [[notes/foo|别名]]", ["周报", "notes/foo|别名"]],
    // 目标为空不是链接。
    ["[[]] 空的", []],
    // 没闭合的、只有一个 `]` 的,都不是。
    ["[[周报 没有闭合", []],
    ["[[周报]", []],
    // body 里不许有 `]`,于是这两行整个不匹配 —— 哪怕后面还有一对 `]]`。
    ["[[a]b]]", []],
    ["[[a] b]]", []],
    // 多余的 `[[` 落进 body 里(正则从失败位置之后一格重试的结果)。
    ["[[[[a]]", ["[[a"]],
    ["[[a]] [[b]]", ["a", "b"]],
    ["![[图]] 与 [[图]]", ["图", "图"]],
    // 200 是上限,201 超。
    [`[[${"x".repeat(200)}]]`, ["x".repeat(200)]],
    [`[[${"x".repeat(201)}]]`, []],
    /* 这一条盯的是"失败后退**一格**"本身:从第一个 `[[` 起算 body 是 `[` + 200 个
       x,201 超限;退一格之后正好 200,于是仍然匹配得上。退两格就整条漏掉。正则
       引擎的 lastIndex 就是加一 —— Rust 那边手写的重试步长必须一致。 */
    [`[[[${"x".repeat(200)}]]`, ["x".repeat(200)]],
    // JS 正则的量词数 UTF-16 code unit:100 个星平面字符正好 200。
    [`[[${"🙂".repeat(100)}]]`, ["🙂".repeat(100)]],
    [`[[${"🙂".repeat(101)}]]`, []],
  ];

  it.each(golden)("%s", (line, expected) => {
    expect(scanWikiLinks(line).map((hit) => hit.raw)).toEqual(expected);
  });
});

describe("collectBacklinks", () => {
  const target = note(`${VAULT}/cao-gao.md`, "周报");
  const other = note(`${VAULT}/other.md`, "别的");
  const index = buildLinkIndex([target, other]);

  it("按标题写的链接也算反链", () => {
    // 这是随手记与 Markio 的实质差异:文件名是 `cao-gao`,标题是「周报」。
    // 按 stem grep 的实现(Markio 的 find_backlinks)会整片漏掉这一类。
    const groups = collectBacklinks([source(other.path, ["见 [[周报]]"])], index, target.path);
    expect(groups).toHaveLength(1);
    expect(groups[0]!.hits).toEqual([{ line: 1, preview: "见 [[周报]]", embed: false }]);
  });

  it("按文件名写的链接也算", () => {
    const groups = collectBacklinks([source(other.path, ["见 [[cao-gao]]"])], index, target.path);
    expect(countBacklinks(groups)).toBe(1);
  });

  it("带 #小节 和 |别名 的链接算到同一篇上", () => {
    const groups = collectBacklinks(
      [source(other.path, ["[[周报#本周]]", "[[周报|上周那篇]]"])],
      index,
      target.path,
    );
    expect(groups[0]!.hits.map((hit) => hit.line)).toEqual([1, 2]);
  });

  it("嵌入算反链,并标出来", () => {
    // `![[..]]` 是更强的引用,不是别的东西 —— 漏掉它会让"这篇被谁用了"的答案是错的。
    const groups = collectBacklinks([source(other.path, ["![[周报]]"])], index, target.path);
    expect(groups[0]!.hits[0]!.embed).toBe(true);
  });

  it("同一行出现两次只留一条", () => {
    // 列表是给人扫的,同一行重复两遍没有增量信息,点进去也是同一个位置。
    const groups = collectBacklinks(
      [source(other.path, ["见 [[周报]] 和 [[周报#本周]]"])],
      index,
      target.path,
    );
    expect(groups[0]!.hits).toHaveLength(1);
  });

  it("不同行各算一条,按行号排", () => {
    const groups = collectBacklinks(
      [
        {
          path: other.path,
          links: [
            { raw: "周报", line: 9, preview: "后面那行", embed: false },
            { raw: "周报", line: 2, preview: "前面那行", embed: false },
          ],
        },
      ],
      index,
      target.path,
    );
    expect(groups[0]!.hits.map((hit) => hit.line)).toEqual([2, 9]);
  });

  it("自引用不算", () => {
    /* 一篇里写 `[[自己]]` 是排版手法(目录、模板)。算进去的话每篇笔记的反链里
       都会有它自己,而那一条永远没有信息量。 */
    const groups = collectBacklinks(
      [source(target.path, ["[[周报]] 指向我自己"])],
      index,
      target.path,
    );
    expect(groups).toEqual([]);
  });

  it("大小写不同的同一个路径仍然判成自引用", () => {
    // macOS 默认文件系统不区分大小写,两种拼法是同一个文件。
    const groups = collectBacklinks(
      [source(`${VAULT}/CAO-GAO.md`, ["[[周报]]"])],
      index,
      target.path,
    );
    expect(groups).toEqual([]);
  });

  it("指向别人的链接不算", () => {
    const groups = collectBacklinks([source(other.path, ["[[别的]]"])], index, target.path);
    expect(groups).toEqual([]);
  });

  it("解析不到的死链不算", () => {
    /* 反链是"谁指向我",一个指不到任何笔记的目标没有"我"可言。死链的提示是
       渲染那一层的事。 */
    const groups = collectBacklinks([source(other.path, ["[[根本不存在]]"])], index, target.path);
    expect(groups).toEqual([]);
  });

  it("一篇里没有命中就不进结果", () => {
    // 空组会让面板显示一个点开什么都没有的来源。
    const groups = collectBacklinks(
      [source(other.path, ["[[别的]]"]), source(`${VAULT}/third.md`, ["[[周报]]"])],
      index,
      target.path,
    );
    expect(groups.map((group) => group.path)).toEqual([`${VAULT}/third.md`]);
  });

  it("来源标题取自索引,不是文件名", () => {
    const groups = collectBacklinks([source(other.path, ["[[周报]]"])], index, target.path);
    expect(groups[0]!.title).toBe("别的");
  });

  it("索引里没有的来源回落到路径", () => {
    // 刚被删掉的笔记还留在上一次扫描结果里。显示路径比显示空白好。
    const stray = `${VAULT}/stray.md`;
    const groups = collectBacklinks([source(stray, ["[[周报]]"])], index, target.path);
    expect(groups[0]!.title).toBe(stray);
  });

  it("来源顺序沿用入参(Rust 已按路径排好)", () => {
    const groups = collectBacklinks(
      [source(`${VAULT}/b.md`, ["[[周报]]"]), source(`${VAULT}/a.md`, ["[[周报]]"])],
      index,
      target.path,
    );
    expect(groups.map((group) => group.path)).toEqual([`${VAULT}/b.md`, `${VAULT}/a.md`]);
  });

  it("目标路径为空时没有反链", () => {
    expect(collectBacklinks([source(other.path, ["[[周报]]"])], index, "")).toEqual([]);
  });

  it("countBacklinks 数的是条数不是篇数", () => {
    const groups = collectBacklinks(
      [
        source(other.path, ["[[周报]]", "又一处 [[周报]]"]),
        source(`${VAULT}/third.md`, ["[[周报]]"]),
      ],
      index,
      target.path,
    );
    expect(groups).toHaveLength(2);
    expect(countBacklinks(groups)).toBe(3);
  });
});

describe("offsetOfLine", () => {
  const source = "第一行\n第二行\n第三行\n";

  it("第一行是 0", () => {
    expect(offsetOfLine(source, 1)).toBe(0);
  });

  it("按 \\n 数到那一行的行首", () => {
    expect(offsetOfLine(source, 2)).toBe(4);
    expect(offsetOfLine(source, 3)).toBe(8);
  });

  it("行号越界时落在最后一行的行首", () => {
    /* 来源笔记在上一次扫描之后被删短了。滚到顶部会让人以为跳错了笔记,而
       "尽量靠近"至少落在同一篇的末尾。 */
    expect(offsetOfLine("a\nb", 99)).toBe(2);
  });

  it("行号 0 或负数当第一行", () => {
    expect(offsetOfLine(source, 0)).toBe(0);
    expect(offsetOfLine(source, -3)).toBe(0);
  });

  it("CRLF 文本的行号与 Rust 侧一致", () => {
    /* Rust 那边用 `content.lines()`,它把 `\r` 当行尾的一部分去掉;这里按 `\n`
       数。于是同一份 CRLF 文本在两边的行号对得上 —— 只是偏移里含那个 `\r`。 */
    expect(offsetOfLine("a\r\nb\r\nc", 2)).toBe(3);
    expect(offsetOfLine("a\r\nb\r\nc", 3)).toBe(6);
  });

  it("空文本只有第一行", () => {
    expect(offsetOfLine("", 5)).toBe(0);
  });
});

describe("bodyOffsetOfFileLine", () => {
  /* 反链的行号按整个 `.md` 文件数,而编辑器里装的是拆掉 frontmatter 之后的正文。
     这个换算错了的表现是"跳过去了,但停在别的行上",而偏几行取决于那篇笔记的
     frontmatter 有多长 —— 看起来像"有时候准有时候不准",最难往这里怀疑。 */
  const body = "第一行\n见 [[Target]]\n";
  const file = `---\ntitle: "Source"\n---\n\n${body}`;

  it("扣掉 frontmatter 那几行", () => {
    // 文件第 5 行是 `第一行`,第 6 行是 `见 [[Target]]`。
    expect(bodyOffsetOfFileLine(file, body, 5)).toBe(0);
    expect(bodyOffsetOfFileLine(file, body, 6)).toBe(4);
  });

  it("没有 frontmatter 时就是文件里的偏移", () => {
    expect(bodyOffsetOfFileLine(body, body, 2)).toBe(4);
  });

  it("落在 frontmatter 里的行号收敛到正文开头", () => {
    /* 理论上不会有(frontmatter 里的 `[[..]]` 也会被扫出来,那才走到这里)。负数
       偏移会让 CodeMirror 抛,而把光标放进一个编辑器里根本看不到的位置更糟。 */
    expect(bodyOffsetOfFileLine(file, body, 2)).toBe(0);
  });

  it("行号越界时落在正文最后一行的行首", () => {
    // 正文以 `\n` 结尾,所以"最后一行"是末尾那个空行,偏移就是正文长度。
    expect(bodyOffsetOfFileLine(file, body, 99)).toBe(body.length);
  });
});
