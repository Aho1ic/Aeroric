import { describe, expect, it } from "vitest";

import { detectTrigger } from "../components/notebook/noteTriggers";

/** 在 `text` 里用 `|` 标出光标,返回 detectTrigger 的结果。 */
function at(text: string) {
  const cursor = text.indexOf("|");
  if (cursor < 0) throw new Error("测试文本里要用 | 标出光标位置");
  return detectTrigger(text.replace("|", ""), cursor);
}

describe("detectTrigger", () => {
  describe("斜杠菜单", () => {
    it("行首的 / 触发", () => {
      expect(at("/|")).toEqual({ kind: "slash", start: 0, query: "" });
    });

    it("带查询", () => {
      expect(at("/tab|")).toEqual({ kind: "slash", start: 0, query: "tab" });
    });

    it("列表标记之后也触发 —— 在列表项里插代码块很常见", () => {
      expect(at("- /|")).toEqual({ kind: "slash", start: 2, query: "" });
      expect(at("1. /code|")).toEqual({ kind: "slash", start: 3, query: "code" });
      expect(at("> /|")).toEqual({ kind: "slash", start: 2, query: "" });
    });

    it("行中间的 / 不触发 —— 那绝大多数是路径和日期", () => {
      expect(at("see src/|")).toBeNull();
      expect(at("2026/08/|")).toBeNull();
    });

    it("第二个 / 不触发:路径的下一段不该重新弹菜单", () => {
      expect(at("/a/|")).toBeNull();
    });

    it("查询里有空格就不再是命令", () => {
      expect(at("/ta b|")).toBeNull();
    });
  });

  describe("围栏与行内代码", () => {
    it("围栏里的 [[ 不触发", () => {
      expect(at("```js\narr[[|\n```")).toBeNull();
    });

    it("围栏里的 # 不触发 —— 那是注释", () => {
      expect(at("```py\n# TODO|\n```")).toBeNull();
    });

    it("围栏里的 @ 不触发 —— 那是装饰器", () => {
      expect(at("```py\n@cache|\n```")).toBeNull();
    });

    it("围栏里的 : 不触发 —— 那是 CSS 伪类", () => {
      expect(at("```css\na:hover|\n```")).toBeNull();
    });

    it("围栏闭合之后恢复触发", () => {
      expect(at("```\ncode\n```\n#tag|")).toEqual({ kind: "tag", start: 13, query: "tag" });
    });

    it("~~~ 也是围栏", () => {
      expect(at("~~~\n#x|\n~~~")).toBeNull();
    });

    it("四个反引号里的三个反引号不算闭合", () => {
      // ```` 包住 ``` 是「展示围栏语法」的标准写法,提前闭合会让后面全部被当正文。
      expect(at("````\n```\n#x|\n````")).toBeNull();
    });

    it("围栏开启行上打字不算在块内", () => {
      /* 两处都是刻意的:
         - 用 `~~~` 而不是 ``` ``` ```:反引号开启行自身就有奇数个反引号,会先被行内
           代码那条挡掉,那样这条断言就跟「开启行算不算块内」无关了。
         - 后面**必须还有一行**:没有换行时扫描在「找不到换行」那条就返回了,同样绕开
           了要守的那个判断(`lineEnd >= cursor`)。 */
      expect(at("~~~ #tag|\nrest")).toEqual({ kind: "tag", start: 4, query: "tag" });
    });

    it("~~~ 不能闭合 ``` 围栏", () => {
      // 闭合标记的字符种类要和开启的一致,否则代码块里出现一行 ~~~ 就提前结束了。
      expect(at("```\n~~~\n#tag|")).toBeNull();
    });

    it("反引号围栏的开启行走的是行内代码那条", () => {
      // ```mermaid 有三个反引号 → 奇数 → 行内代码。结果一样是不触发,但原因不同。
      expect(at("```mermaid|")).toBeNull();
    });

    it("行内代码里不触发", () => {
      expect(at("`arr[[|")).toBeNull();
      expect(at("`a:b|")).toBeNull();
    });

    it("行内代码闭合之后恢复触发", () => {
      expect(at("`code` #tag|")).toEqual({ kind: "tag", start: 7, query: "tag" });
    });

    it("转义的反引号不参与配对", () => {
      // `\`` 是一个字面反引号,不开启行内代码。
      expect(at("\\` #tag|")).toEqual({ kind: "tag", start: 3, query: "tag" });
    });
  });

  describe("frontmatter", () => {
    it("frontmatter 里不触发", () => {
      expect(at("---\ntags: #x|\n---\nbody")).toBeNull();
    });

    it("frontmatter 之后正常触发", () => {
      expect(at("---\ntitle: a\n---\n#tag|")).toEqual({ kind: "tag", start: 17, query: "tag" });
    });

    it("还没闭合的 frontmatter 视为整篇都在里面", () => {
      expect(at("---\ntags: #x|")).toBeNull();
    });

    it("不在第一行的 --- 不是 frontmatter", () => {
      expect(at("body\n---\n#tag|")).toEqual({ kind: "tag", start: 9, query: "tag" });
    });

    it("正文里两条分割线之间的标签照样触发", () => {
      /* `---` 是 markdown 的分割线,正文里出现两条再正常不过。判 frontmatter 时不锚定
         第一行的话,这中间的一段会被当成 frontmatter,标签补全就静默失踪 —— 而上面那条
         用例的光标在「假 frontmatter」范围之外,恰好也能通过,守不住这件事。 */
      expect(at("a\n---\n#x|\n---\nb")).toEqual({ kind: "tag", start: 6, query: "x" });
    });
  });

  describe("[[ 双链", () => {
    it("空查询就触发", () => {
      expect(at("[[|")).toEqual({ kind: "wiki", start: 0, query: "" });
    });

    it("查询可以带空格和标点 —— 笔记标题什么字符都可能有", () => {
      expect(at("see [[My Note (v2), 草稿|")).toEqual({
        kind: "wiki",
        start: 4,
        query: "My Note (v2), 草稿",
      });
    });

    it("已经闭合的 ]] 之后不再触发", () => {
      expect(at("[[note]]|")).toBeNull();
    });

    it("嵌入 ![[ 也走 wiki", () => {
      // start 指向 `[[`,前面的 `!` 不动 —— 替换掉它会把嵌入语法降级成普通链接。
      expect(at("![[|")).toEqual({ kind: "wiki", start: 1, query: "" });
    });

    it("[[#heading 判成 wiki 而不是标签", () => {
      // `[[note#小节]]` 是合法的段内链接,先判 wiki 才不会把它当标签。
      expect(at("[[#head|")).toEqual({ kind: "wiki", start: 0, query: "#head" });
    });
  });

  describe("# 标签", () => {
    it("行首的 #x 触发", () => {
      expect(at("#work|")).toEqual({ kind: "tag", start: 0, query: "work" });
    });

    it("空白之后触发", () => {
      expect(at("done #work|")).toEqual({ kind: "tag", start: 5, query: "work" });
    });

    it("光秃秃的 # 不触发 —— 那是正在写标题", () => {
      expect(at("#|")).toBeNull();
    });

    it("# 加空格是标题,不是标签", () => {
      expect(at("# 标题|")).toBeNull();
    });

    it("纯数字不是标签 —— #42 是条目编号", () => {
      // 和 Rust `normalize_tag` 同一条规则:写进去后端也扫不出来。
      expect(at("#42|")).toBeNull();
      expect(at("#42x|")).toEqual({ kind: "tag", start: 0, query: "42x" });
    });

    it("紧贴在字母后面不触发", () => {
      // Rust `tags.rs` 的 ok_prefix 只允许行首或空白;放宽会写出后端扫不出的标签。
      expect(at("a#work|")).toBeNull();
    });

    it("紧贴在括号后面也不触发", () => {
      expect(at("({#id|")).toBeNull();
    });

    it("允许中文、斜杠、下划线和连字符", () => {
      expect(at("#工作/紧急|")).toEqual({ kind: "tag", start: 0, query: "工作/紧急" });
      expect(at("#a_b-c|")).toEqual({ kind: "tag", start: 0, query: "a_b-c" });
    });

    it("超过 64 字符就不再是「正在打标签」", () => {
      expect(at(`#${"a".repeat(64)}|`)).toBeNull();
      expect(at(`#${"a".repeat(63)}|`)).not.toBeNull();
    });
  });

  describe("@ 提及", () => {
    it("空查询就触发", () => {
      expect(at("@|")).toEqual({ kind: "mention", start: 0, query: "" });
    });

    it("带查询", () => {
      expect(at("ping @ali|")).toEqual({ kind: "mention", start: 5, query: "ali" });
    });

    it("邮箱里的 @ 不触发", () => {
      expect(at("me@example|")).toBeNull();
    });
  });

  describe(": emoji", () => {
    it("空查询就触发", () => {
      expect(at(":|")).toEqual({ kind: "emoji", start: 0, query: "" });
    });

    it("带查询", () => {
      expect(at("ship :roc|")).toEqual({ kind: "emoji", start: 5, query: "roc" });
    });

    it("中文查询也收 —— emoji 表里带中文关键词", () => {
      expect(at(":火|")).toEqual({ kind: "emoji", start: 0, query: "火" });
    });

    it("URL 里的 : 不触发", () => {
      expect(at("http:|")).toBeNull();
    });

    it("时间里的 : 不触发", () => {
      expect(at("12:|")).toBeNull();
    });

    it("YAML 风格的 key: 不触发", () => {
      expect(at("status:|")).toBeNull();
    });
  });

  describe("边界", () => {
    it("越界的光标返回 null 而不是抛", () => {
      expect(detectTrigger("abc", -1)).toBeNull();
      expect(detectTrigger("abc", 99)).toBeNull();
    });

    it("空文档不触发", () => {
      expect(detectTrigger("", 0)).toBeNull();
    });

    it("普通文字不触发", () => {
      expect(at("just some text|")).toBeNull();
    });
  });
});
