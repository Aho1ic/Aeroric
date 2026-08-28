import { describe, expect, it } from "vitest";
import {
  NOTEBOOK_MATH_SELECTOR,
  NOTEBOOK_MERMAID_SELECTOR,
  renderNoteMarkdown,
} from "../components/notebook/noteRender";

/** 把 HTML 挂进一个游离容器,便于用选择器断言结构。 */
function mount(html: string): HTMLDivElement {
  const host = document.createElement("div");
  host.innerHTML = html;
  return host;
}

describe("renderNoteMarkdown — 基础 markdown", () => {
  it("渲染标题并带上锚点 id", () => {
    const host = mount(renderNoteMarkdown("# Release notes\n\ntext\n").html);
    const heading = host.querySelector("h1");
    expect(heading?.textContent).toBe("Release notes");
    expect(heading?.id).toBe("release-notes");
  });

  it("同名标题的 id 去重", () => {
    const host = mount(renderNoteMarkdown("# Same\n\n## Same\n\n### Same\n").html);
    const ids = Array.from(host.querySelectorAll("h1,h2,h3")).map((node) => node.id);
    expect(ids).toEqual(["same", "same-1", "same-2"]);
  });

  it("每次渲染都从干净的 id 表开始", () => {
    // 复用 marked 实例会让第二次渲染同一篇笔记时所有 id 都带 `-1` 后缀,
    // 大纲跳转随之失效。
    const first = mount(renderNoteMarkdown("# Title\n").html).querySelector("h1")?.id;
    const second = mount(renderNoteMarkdown("# Title\n").html).querySelector("h1")?.id;
    expect(first).toBe("title");
    expect(second).toBe("title");
  });

  it("支持 GFM 表格与删除线", () => {
    const host = mount(
      renderNoteMarkdown("| a | b |\n| --- | --- |\n| 1 | 2 |\n\n~~gone~~\n").html,
    );
    expect(host.querySelector("table")).not.toBeNull();
    expect(host.querySelectorAll("td")).toHaveLength(2);
    expect(host.querySelector("del")?.textContent).toBe("gone");
  });

  it("任务列表渲染成只读复选框", () => {
    const host = mount(renderNoteMarkdown("- [ ] todo\n- [x] done\n").html);
    const boxes = host.querySelectorAll<HTMLInputElement>("input[type=checkbox]");
    expect(boxes).toHaveLength(2);
    expect(boxes[0]?.hasAttribute("checked")).toBe(false);
    expect(boxes[1]?.hasAttribute("checked")).toBe(true);
    // 预览是只读的,勾选交互留给编辑器。
    expect(boxes[0]?.hasAttribute("disabled")).toBe(true);
  });

  it("渲染脚注", () => {
    const host = mount(renderNoteMarkdown("Text[^1]\n\n[^1]: The note\n").html);
    expect(host.textContent).toContain("The note");
    // 脚注插件会生成回跳链接。
    expect(host.querySelector("a[href^='#']")).not.toBeNull();
  });

  it("渲染 GitHub 风格提示块", () => {
    const host = mount(renderNoteMarkdown("> [!NOTE]\n> Heads up\n").html);
    expect(host.textContent).toContain("Heads up");
    // marked-alert 给容器加 `markdown-alert` 类。
    expect(host.querySelector("[class*=alert]")).not.toBeNull();
  });
});

describe("renderNoteMarkdown — 安全", () => {
  it("剥掉 script 标签", () => {
    const host = mount(renderNoteMarkdown("before\n\n<script>alert(1)</script>\n\nafter").html);
    expect(host.querySelector("script")).toBeNull();
    expect(host.textContent).toContain("before");
  });

  it("剥掉事件属性", () => {
    const host = mount(renderNoteMarkdown('<img src="x" onerror="alert(1)">').html);
    expect(host.querySelector("img")?.getAttribute("onerror")).toBeNull();
  });

  it("剥掉 javascript: 协议链接", () => {
    const host = mount(renderNoteMarkdown("[click](javascript:alert(1))").html);
    const href = host.querySelector("a")?.getAttribute("href") ?? "";
    expect(href.startsWith("javascript:")).toBe(false);
  });

  it("剥掉 iframe", () => {
    const host = mount(renderNoteMarkdown('<iframe src="https://evil.test"></iframe>').html);
    expect(host.querySelector("iframe")).toBeNull();
  });
});

describe("renderNoteMarkdown — 数学占位", () => {
  it("行内公式产出未渲染的占位元素", () => {
    const host = mount(renderNoteMarkdown("mass $E=mc^2$ energy").html);
    const nodes = host.querySelectorAll(NOTEBOOK_MATH_SELECTOR);
    expect(nodes).toHaveLength(1);
    // 占位里放原始 TeX:渲染失败时用户至少看得到公式源码。
    expect(nodes[0]?.textContent).toBe("E=mc^2");
    expect(nodes[0]?.classList.contains("notebook-math-inline")).toBe(true);
  });

  it("块级公式用 div 且标为 display", () => {
    const host = mount(renderNoteMarkdown("$$\n\\int_0^1 x dx\n$$\n").html);
    const node = host.querySelector(NOTEBOOK_MATH_SELECTOR);
    expect(node?.tagName).toBe("DIV");
    expect(node?.classList.contains("notebook-math-display")).toBe(true);
    expect(node?.textContent).toContain("\\int_0^1 x dx");
  });

  it("公式里的下划线与星号不被 markdown 吃掉", () => {
    // 这是「数学必须在 marked 之前抽走」的理由:`_i_` 会变成 <em>i</em>,
    // `\\` 会被当转义,拿到 KaTeX 手上就不是原式了。
    const host = mount(renderNoteMarkdown("$a_i * b_j \\\\ c$").html);
    const node = host.querySelector(NOTEBOOK_MATH_SELECTOR);
    expect(node?.textContent).toBe("a_i * b_j \\\\ c");
    expect(node?.querySelector("em")).toBeNull();
  });

  it("多个行内公式各自独立", () => {
    const host = mount(renderNoteMarkdown("$a$ and $b$ and $c$").html);
    const nodes = Array.from(host.querySelectorAll(NOTEBOOK_MATH_SELECTOR));
    expect(nodes.map((node) => node.textContent)).toEqual(["a", "b", "c"]);
  });

  it("单行 $$…$$ 不算块级公式", () => {
    // 块级要求 `$$` 独占一行(与 Markio 一致)。单行形式两个 `$` 都被跳过,
    // 不会退化成把中间内容当行内公式。
    const host = mount(renderNoteMarkdown("text $$c$$ tail").html);
    expect(host.querySelectorAll(NOTEBOOK_MATH_SELECTOR)).toHaveLength(0);
  });

  it("代码块里的 $ 不当公式", () => {
    const host = mount(renderNoteMarkdown("```sh\necho $HOME\n```\n").html);
    expect(host.querySelectorAll(NOTEBOOK_MATH_SELECTOR)).toHaveLength(0);
    expect(host.querySelector("code")?.textContent).toContain("$HOME");
  });

  it("金额不被当成公式", () => {
    // 随手记里写价格很常见。Markio 原版会把 `$5 and $9` 认成一个公式
    // (内容 "5 and"),预览里渲染成一坨数学 —— 移植时补了 pandoc 的
    // 「闭定界符前不能是空白」判据修掉它。
    const host = mount(renderNoteMarkdown("plain text, costs $5 and $9").html);
    expect(host.querySelectorAll(NOTEBOOK_MATH_SELECTOR)).toHaveLength(0);
    expect(host.textContent).toContain("$5 and $9");
  });

  it("开定界符后紧跟空白时不算公式", () => {
    const host = mount(renderNoteMarkdown("give $ 5 to $ 9 people").html);
    expect(host.querySelectorAll(NOTEBOOK_MATH_SELECTOR)).toHaveLength(0);
  });
});

describe("renderNoteMarkdown — Mermaid 占位", () => {
  it("mermaid 围栏产出带编码源码的占位块", () => {
    const source = "graph LR\nA-->B";
    const host = mount(renderNoteMarkdown(`\`\`\`mermaid\n${source}\n\`\`\`\n`).html);
    const node = host.querySelector(NOTEBOOK_MERMAID_SELECTOR);
    expect(node).not.toBeNull();
    // 源码 URI 编码进属性:里面有引号、尖括号和换行,裸塞会破坏 HTML 结构。
    expect(decodeURIComponent(node?.getAttribute("data-mermaid") ?? "")).toBe(source);
  });

  it("其它语言的围栏保持普通代码块", () => {
    const host = mount(renderNoteMarkdown("```rust\nfn main() {}\n```\n").html);
    expect(host.querySelectorAll(NOTEBOOK_MERMAID_SELECTOR)).toHaveLength(0);
    expect(host.querySelector("pre")?.getAttribute("data-language")).toBe("rust");
  });

  it("mermaid 源码里的尖括号不破坏结构", () => {
    const host = mount(renderNoteMarkdown('```mermaid\ngraph LR\nA["<b>x</b>"]-->B\n```\n').html);
    const node = host.querySelector(NOTEBOOK_MERMAID_SELECTOR);
    expect(node).not.toBeNull();
    // 属性里是编码后的,不该出现真的标签。
    expect(node?.querySelector("b")).toBeNull();
    expect(decodeURIComponent(node?.getAttribute("data-mermaid") ?? "")).toContain("<b>x</b>");
  });

  it("语言标注带额外参数时仍识别", () => {
    const host = mount(renderNoteMarkdown("```mermaid showLineNumbers\ngraph LR\n```\n").html);
    expect(host.querySelector(NOTEBOOK_MERMAID_SELECTOR)).not.toBeNull();
  });
});

describe("renderNoteMarkdown — 边界", () => {
  it("空输入不抛", () => {
    expect(renderNoteMarkdown("").html).toBe("");
    // 面板在笔记还没读入时会拿到 undefined。
    expect(() => renderNoteMarkdown(undefined as unknown as string)).not.toThrow();
  });

  it("未闭合的围栏不吞掉后续内容", () => {
    const host = mount(renderNoteMarkdown("```\nunclosed\n").html);
    expect(host.textContent).toContain("unclosed");
  });
});
