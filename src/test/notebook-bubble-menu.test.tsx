import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";

import { NoteBubbleMenu, type BubbleAction } from "../components/notebook/NoteBubbleMenu";
import { en } from "../i18n/en";

/* 用真实英文文案而不是回显 key:按钮之间只靠 aria-label 区分,而 label 是 t() 出来的。
   拿 key 当名字的测试在两个按钮误用同一个 key 时也是绿的。 */
const t = (key: string) => en[key] ?? key;

/* 与源码常量对齐。写死数字而不是从组件导入 —— 组件没导出它们,而这些值是布局契约:
   改了就该有一条测试红,而不是跟着改。 */
const WIDTH = 336;
const HEIGHT = 34;
const GAP = 8;
const EDGE = 8;

/** 按源码 GROUPS 展开后的顺序。既当顺序断言的期望值,也当点击用例的数据源。 */
const ITEMS: [label: string, action: BubbleAction][] = [
  ["Bold", "bold"],
  ["Italic", "italic"],
  ["Underline", "underline"],
  ["Strikethrough", "strike"],
  ["Highlight", "highlight"],
  ["Inline code", "inlineCode"],
  ["Link", "link"],
  ["Quote", "quote"],
  ["Bullet list", "bullet"],
  ["Code block", "codeBlock"],
];

function renderMenu(over: Partial<React.ComponentProps<typeof NoteBubbleMenu>> = {}) {
  const props = {
    // 默认锚点落在视口中间且离顶部够远,这样定位相关的分支都不会被意外触发。
    anchor: { left: 400, right: 500, top: 300, bottom: 320 },
    onAction: vi.fn(),
    onDismiss: vi.fn(),
    t,
    ...over,
  };
  const utils = render(<NoteBubbleMenu {...props} />);
  return { ...utils, props };
}

const toolbar = () => screen.getByRole("toolbar");
const buttons = () => screen.getAllByRole("button");

describe("NoteBubbleMenu 语义", () => {
  it("根节点是横向 toolbar,名字取自 i18n", () => {
    /* toolbar 而不是一堆裸 button:读屏软件靠这个角色把十个图标按钮当成一组来播报,
       否则用户听到的是十个孤立的按钮。 */
    renderMenu();
    expect(toolbar()).toHaveAccessibleName("Formatting");
    expect(toolbar()).toHaveAttribute("aria-orientation", "horizontal");
  });

  it("十个按钮,顺序和可访问名与 GROUPS 展开后一致", () => {
    /* 整体比较而不是逐个 getByLabelText:后者查不出顺序,而顺序就是这个组件的设计
       (加粗在最左边),分组边界错了也照样绿。 */
    renderMenu();
    expect(buttons().map((node) => node.getAttribute("aria-label"))).toEqual(
      ITEMS.map(([label]) => label),
    );
  });

  it("图标不进 a11y 树,名字只来自 aria-label", () => {
    /* 图标进了 a11y 树,读屏会在按钮名后面多念一段。
       注意这一条**杀不掉「组件里去掉 aria-hidden」这个变异**:lucide-react 在没有
       children、也没有传任何 a11y 属性时会自己补上(`lucide-react@1.7.0`
       `dist/esm/Icon.js:92` 的 `...!children && !hasA11yProp(rest) && { "aria-hidden": "true" }`)。
       断言仍然留着 —— 要守的契约是「图标不在 a11y 树里」,不管这个属性由谁写上;
       lucide 哪天改了默认值,这条会红。 */
    renderMenu();
    for (const button of buttons()) {
      expect(button.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    }
  });
});

describe("NoteBubbleMenu 分隔符", () => {
  it("组间三条分隔符,第一组前面没有", () => {
    // 四组之间只有三个缝。多画一条会在最左边留一道悬空竖线。
    renderMenu();
    expect(toolbar().querySelectorAll('span[aria-hidden="true"]')).toHaveLength(3);
  });

  it("分隔符是纯装饰:不是 button,也不在 a11y 树里", () => {
    /* 分隔符若被当成可聚焦控件,Tab 会停在一条竖线上。按 role 数量断言,顺带守住
       "只有十个按钮"这条 —— 装饰节点一旦漏了 aria-hidden 就会被算进来。 */
    renderMenu();
    const separators = Array.from(toolbar().querySelectorAll('span[aria-hidden="true"]'));
    for (const separator of separators) {
      expect(separator.tagName).toBe("SPAN");
    }
    expect(buttons()).toHaveLength(ITEMS.length);
    expect(screen.getAllByRole("toolbar")).toHaveLength(1);
  });
});

describe("NoteBubbleMenu 触发", () => {
  it.each(ITEMS)("%s 的 mouseDown 发出 %s 并挡掉默认行为", (label, action) => {
    /* 必须挡掉 mousedown 的默认行为:它会把焦点从编辑器抢走,而失焦时选区就没了 ——
       命令随后落在一个空选区上,什么都不做。 */
    const { props } = renderMenu();
    const event = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    fireEvent(screen.getByRole("button", { name: label }), event);
    expect(props.onAction).toHaveBeenCalledTimes(1);
    expect(props.onAction).toHaveBeenCalledWith(action);
    expect(event.defaultPrevented).toBe(true);
  });

  it("只有 click 不算触发", () => {
    /* 组件挂的是 onMouseDown。这条守的是"别改回 onClick":改回去在手动点击下看着
       一样能用(真实点击 mousedown 在前),但选区已经在 click 到达前丢了。 */
    const { props } = renderMenu();
    fireEvent.click(screen.getByRole("button", { name: "Bold" }));
    expect(props.onAction).not.toHaveBeenCalled();
  });
});

describe("NoteBubbleMenu 定位", () => {
  it("水平居中在选区上", () => {
    const { props } = renderMenu();
    const center = (props.anchor.left + props.anchor.right) / 2;
    expect(toolbar().style.left).toBe(`${center - WIDTH / 2}px`);
  });

  it("选区贴左边时夹到边距,而不是让气泡溢出屏幕", () => {
    // 居中算出来是负数;不夹的话最左边几个按钮(含加粗)直接点不到。
    renderMenu({ anchor: { left: 0, right: 20, top: 300, bottom: 320 } });
    expect(toolbar().style.left).toBe(`${EDGE}px`);
  });

  it("选区贴右边时按右边距反推左坐标", () => {
    /* 夹的是左边缘而不是中心:只夹中心时靠右的选区上会有一半气泡在屏幕外。
       jsdom 的 window.innerWidth 是 1024,这里就用默认值。 */
    const right = window.innerWidth;
    renderMenu({ anchor: { left: right - 20, right, top: 300, bottom: 320 } });
    expect(toolbar().style.left).toBe(`${right - WIDTH - EDGE}px`);
  });

  it("默认贴在选区上方", () => {
    const { props } = renderMenu();
    expect(toolbar().style.top).toBe(`${props.anchor.top - HEIGHT - GAP}px`);
  });

  it("上方放不下就翻到选区下方", () => {
    /* 选区在第一行时 top - HEIGHT - GAP 是负数,被切掉的正好是按钮那一截。
       top=10 时上方只剩 -32,必须翻。 */
    const anchor = { left: 400, right: 500, top: 10, bottom: 30 };
    renderMenu({ anchor });
    expect(toolbar().style.top).toBe(`${anchor.bottom + GAP}px`);
  });

  it("上方刚好够 EDGE 时不翻", () => {
    // 边界:above === EDGE 属于放得下。写成 `<= EDGE` 会让这条红。
    const anchor = { left: 400, right: 500, top: HEIGHT + GAP + EDGE, bottom: 320 };
    renderMenu({ anchor });
    expect(toolbar().style.top).toBe(`${EDGE}px`);
  });

  it("上方放得下但顶不住边距时也翻", () => {
    /* 另一侧的边界:above 落在 [0, EDGE) 之间 —— 数值是正的,气泡不会被窗口切掉,
       但会贴在最上沿甚至压住工具栏。阈值写成 `above < 0` 时这条红,而
       "top=10 必须翻" 那条(above 是负数)两种写法都绿,分不出来。 */
    const anchor = { left: 400, right: 500, top: HEIGHT + GAP + 4, bottom: 320 };
    renderMenu({ anchor });
    expect(toolbar().style.top).toBe(`${anchor.bottom + GAP}px`);
  });
});

describe("NoteBubbleMenu 收起", () => {
  it("点气泡外面收起", () => {
    const { props } = renderMenu();
    fireEvent.pointerDown(document.body);
    expect(props.onDismiss).toHaveBeenCalled();
  });

  it("点气泡里面不收起", () => {
    // 点到按钮上时 onAction 已经处理了,再收一次会让面板在命令生效前拆掉气泡。
    const { props } = renderMenu();
    fireEvent.pointerDown(screen.getByRole("button", { name: "Bold" }));
    expect(props.onDismiss).not.toHaveBeenCalled();
  });

  it("卸载后不再监听 pointerdown", () => {
    const { props, unmount } = renderMenu();
    unmount();
    fireEvent.pointerDown(document.body);
    expect(props.onDismiss).not.toHaveBeenCalled();
  });

  it("监听在捕获阶段,外层 stopPropagation 拦不住", () => {
    /* 编辑器上的 mousedown/pointerdown 会先改选区,冒泡阶段再判就晚了;而任何一层
       祖先调 stopPropagation 都会让冒泡阶段的 document 监听彻底收不到事件。
       这里用一个自己吃掉冒泡的容器来区分两种装法。 */
    const outer = document.createElement("div");
    outer.addEventListener("pointerdown", (event) => event.stopPropagation());
    document.body.appendChild(outer);
    const inner = document.createElement("button");
    outer.appendChild(inner);

    const { props } = renderMenu();
    fireEvent.pointerDown(inner);
    expect(props.onDismiss).toHaveBeenCalled();

    outer.remove();
  });
});
