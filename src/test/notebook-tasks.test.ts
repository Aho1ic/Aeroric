/* `noteTasks` / `toggleTaskLine`:任务项的定位与勾选。
 *
 * 这一层最要紧的性质是「行号必须真的对着源码那一行」—— 错位的表现是勾错别人那一行,
 * 一种静默的数据损坏,所以下面的断言都盯着具体行号,不只数个数。
 */

import { describe, expect, it } from "vitest";
import { noteTasks, toggleTaskLine } from "../components/notebook/noteTasks";

describe("noteTasks", () => {
  it("空正文没有任务", () => {
    expect(noteTasks("")).toEqual([]);
    expect(noteTasks("没有任何任务的一段话")).toEqual([]);
  });

  it("按文档顺序给出行号与勾选状态", () => {
    const source = ["# 标题", "", "- [ ] 一", "- [x] 二", "- [ ] 三"].join("\n");
    expect(noteTasks(source)).toEqual([
      { line: 3, checked: false, text: "一" },
      { line: 4, checked: true, text: "二" },
      { line: 5, checked: false, text: "三" },
    ]);
  });

  it("大写的 X 也算勾上了", () => {
    expect(noteTasks("- [X] 大写")).toEqual([{ line: 1, checked: true, text: "大写" }]);
  });

  it("普通列表项不是任务", () => {
    expect(noteTasks("- 普通\n- [ ] 任务")).toEqual([{ line: 2, checked: false, text: "任务" }]);
  });

  it("围栏里的 `- [ ]` 不算 —— 渲染器也不会给它产复选框", () => {
    const source = ["- [ ] 真的", "", "```md", "- [ ] 假的", "```", "", "- [x] 也真的"].join("\n");
    expect(noteTasks(source).map((task) => task.text)).toEqual(["真的", "也真的"]);
    expect(noteTasks(source).map((task) => task.line)).toEqual([1, 7]);
  });

  it("有序列表里的任务算", () => {
    const source = ["1. [ ] 第一", "2. [x] 第二"].join("\n");
    expect(noteTasks(source)).toEqual([
      { line: 1, checked: false, text: "第一" },
      { line: 2, checked: true, text: "第二" },
    ]);
  });

  it("blockquote 里的任务算,行号是它在源码里的那一行", () => {
    const source = ["前言", "", "> - [ ] 引用里的", "", "- [ ] 外面的"].join("\n");
    expect(noteTasks(source)).toEqual([
      { line: 3, checked: false, text: "引用里的" },
      { line: 5, checked: false, text: "外面的" },
    ]);
  });

  /* 多行的引用块是 `at < 0` 那条分支唯一真正会走到的地方:blockquote 会把每行的 `> `
     剥掉,于是里层 list 的 `raw`(`- [ ] 甲\n- [ ] 乙`)不是源码的字面子串 —— 定位不到。
     那时候必须不动游标继续往孙子层走,不能整片跳过,否则引用里的任务全丢。 */
  it("多行引用块里的任务都能找到,行号各自对应", () => {
    expect(noteTasks("> - [ ] 甲\n> - [x] 乙\n")).toEqual([
      { line: 1, checked: false, text: "甲" },
      { line: 2, checked: true, text: "乙" },
    ]);
  });

  it("引用块里有说明文字时,行号跳过那几行", () => {
    expect(noteTasks("> 说明\n>\n> - [ ] 甲\n> - [ ] 乙\n")).toEqual([
      { line: 3, checked: false, text: "甲" },
      { line: 4, checked: false, text: "乙" },
    ]);
  });

  it("引用块之后的任务接着往下数", () => {
    expect(noteTasks("> - [ ] 甲\n> - [ ] 乙\n\n- [ ] 丙\n")).toEqual([
      { line: 1, checked: false, text: "甲" },
      { line: 2, checked: false, text: "乙" },
      { line: 4, checked: false, text: "丙" },
    ]);
  });

  it("嵌套引用里的任务也能找到", () => {
    expect(noteTasks("> > - [ ] 深\n")).toEqual([{ line: 1, checked: false, text: "深" }]);
  });

  it("嵌套任务:外层在前、内层在后,各自行号正确", () => {
    const source = ["- [ ] 外层", "  - [x] 内层", "- [ ] 后面"].join("\n");
    expect(noteTasks(source)).toEqual([
      { line: 1, checked: false, text: "外层" },
      { line: 2, checked: true, text: "内层" },
      { line: 3, checked: false, text: "后面" },
    ]);
  });

  it("多行任务项只取第一行当文本,行号指向带复选框的那一行", () => {
    const source = ["- [ ] 第一行", "  接着写的第二行", "- [x] 下一条"].join("\n");
    expect(noteTasks(source)).toEqual([
      { line: 1, checked: false, text: "第一行" },
      { line: 3, checked: true, text: "下一条" },
    ]);
  });

  it("数学块之后的行号不受影响 —— 渲染前会把多行 `$$` 压成一行", () => {
    const source = ["$$", "a = 1", "b = 2", "c = 3", "$$", "", "- [ ] 公式后面"].join("\n");
    expect(noteTasks(source)).toEqual([{ line: 7, checked: false, text: "公式后面" }]);
  });

  it("任务文本里的行内标记原样留着(是 markdown 原文)", () => {
    expect(noteTasks("- [ ] 看 [[某篇]] 和 `代码`")).toEqual([
      { line: 1, checked: false, text: "看 [[某篇]] 和 `代码`" },
    ]);
  });

  it("重复的任务文本不会互相干扰(游标只前进)", () => {
    const source = ["- [ ] 同一句", "- [ ] 同一句", "- [x] 同一句"].join("\n");
    expect(noteTasks(source)).toEqual([
      { line: 1, checked: false, text: "同一句" },
      { line: 2, checked: false, text: "同一句" },
      { line: 3, checked: true, text: "同一句" },
    ]);
  });

  /* 没有内容的 `- [ ]` 不是任务项 —— marked 把它当普通列表项(渲染出来是字面的
     "[ ]",没有复选框)。这里跟着 tokenizer 走,不自己定规矩。 */
  it("没有内容的 `- [ ]` 不算任务", () => {
    expect(noteTasks("- [ ] ")).toEqual([]);
    expect(noteTasks("- [ ]")).toEqual([]);
  });

  /* 手写正则最容易在这里错位:按"第 N 个 `- [ ]`"数的话,真任务会被当成第 1 个,
     于是勾选写到第 1 行 —— 而第 1 行根本不是任务。 */
  it("混着没内容的空壳时,行号仍然指向真任务", () => {
    expect(noteTasks("- [ ]\n- [ ] 真的")).toEqual([{ line: 2, checked: false, text: "真的" }]);
  });
});

describe("toggleTaskLine", () => {
  it("未勾 → 勾上", () => {
    expect(toggleTaskLine("- [ ] 一\n- [ ] 二", 1)).toBe("- [x] 一\n- [ ] 二");
  });

  it("勾上 → 未勾", () => {
    expect(toggleTaskLine("- [x] 一\n- [ ] 二", 1)).toBe("- [ ] 一\n- [ ] 二");
  });

  it("只动目标那一行", () => {
    expect(toggleTaskLine("- [ ] 一\n- [ ] 二\n- [ ] 三", 2)).toBe("- [ ] 一\n- [x] 二\n- [ ] 三");
  });

  it("保留缩进与行尾内容", () => {
    expect(toggleTaskLine("  - [ ] 缩进的 **粗体** 尾巴", 1)).toBe("  - [x] 缩进的 **粗体** 尾巴");
  });

  it("有序列表与 blockquote 的标记原样保留", () => {
    expect(toggleTaskLine("1. [ ] 有序", 1)).toBe("1. [x] 有序");
    expect(toggleTaskLine("> - [ ] 引用", 1)).toBe("> - [x] 引用");
  });

  it("大写 X 也认,翻回来用空格", () => {
    expect(toggleTaskLine("- [X] 大写", 1)).toBe("- [ ] 大写");
  });

  it("行号越界返回 null", () => {
    expect(toggleTaskLine("- [ ] 一", 0)).toBeNull();
    expect(toggleTaskLine("- [ ] 一", 2)).toBeNull();
    expect(toggleTaskLine("- [ ] 一", -1)).toBeNull();
  });

  it("那一行不是任务项就返回 null,正文一个字不动", () => {
    expect(toggleTaskLine("普通一行\n- [ ] 任务", 1)).toBeNull();
    expect(toggleTaskLine("- 普通列表", 1)).toBeNull();
    expect(toggleTaskLine("", 1)).toBeNull();
  });

  it("`[ ]` 里不是空格或 x 的不算任务项", () => {
    expect(toggleTaskLine("- [-] 不是", 1)).toBeNull();
    expect(toggleTaskLine("- [] 也不是", 1)).toBeNull();
  });

  it("乐观锁:期望状态与当前一致才写", () => {
    expect(toggleTaskLine("- [ ] 一", 1, false)).toBe("- [x] 一");
    expect(toggleTaskLine("- [x] 一", 1, true)).toBe("- [ ] 一");
  });

  it("乐观锁:期望状态与当前不符就拒绝 —— 正文已经被别处改过了", () => {
    expect(toggleTaskLine("- [x] 一", 1, false)).toBeNull();
    expect(toggleTaskLine("- [ ] 一", 1, true)).toBeNull();
  });

  it("不传期望状态就不校验(纯翻转)", () => {
    expect(toggleTaskLine("- [x] 一", 1, undefined)).toBe("- [ ] 一");
  });

  it("`\\r\\n` 行尾不会被吞掉", () => {
    // split("\n") 会把 `\r` 留在行尾,它在勾选框之后,所以照样保住。
    expect(toggleTaskLine("- [ ] 一\r\n- [ ] 二", 1)).toBe("- [x] 一\r\n- [ ] 二");
  });

  it("翻转是可逆的", () => {
    const source = "- [ ] 一\n- [x] 二";
    const once = toggleTaskLine(source, 2);
    expect(once).not.toBeNull();
    expect(toggleTaskLine(once!, 2)).toBe(source);
  });
});

describe("两侧配合", () => {
  it("noteTasks 给的行号能直接喂给 toggleTaskLine", () => {
    const source = [
      "# 周计划",
      "",
      "> - [ ] 引用里的",
      "",
      "```md",
      "- [ ] 围栏里的",
      "```",
      "",
      "1. [x] 有序的",
      "  - [ ] 嵌套的",
    ].join("\n");
    const tasks = noteTasks(source);
    for (const task of tasks) {
      const next = toggleTaskLine(source, task.line, task.checked);
      // 每一条都必须写得进去:行号对不上或状态读错都会让它变成 null。
      expect(next).not.toBeNull();
      expect(next).not.toBe(source);
    }
    expect(tasks.map((task) => task.line)).toEqual([3, 9, 10]);
  });
});
