import { afterEach, describe, expect, it, vi } from "vitest";
import {
  paneFromElement,
  registerPane,
  resetSplitScrollSync,
  syncPreviewToSource,
} from "../components/notebook/splitScrollSync";

/** 造一个可滚动元素。jsdom 没有布局,scrollHeight/clientHeight 要自己定义。 */
function scrollable(scrollHeight: number, clientHeight: number): HTMLElement {
  const el = document.createElement("div");
  Object.defineProperty(el, "scrollHeight", { value: scrollHeight, configurable: true });
  Object.defineProperty(el, "clientHeight", { value: clientHeight, configurable: true });
  // jsdom 的 scrollTop 是普通属性,赋值不会自动派发 scroll —— 测试里手动派。
  el.scrollTop = 0;
  document.body.append(el);
  return el;
}

function scroll(el: HTMLElement, top: number) {
  el.scrollTop = top;
  el.dispatchEvent(new Event("scroll"));
}

afterEach(() => {
  resetSplitScrollSync();
  document.body.replaceChildren();
  vi.useRealTimers();
});

describe("splitScrollSync", () => {
  it("把源码侧的滚动比例同步到预览侧", async () => {
    vi.useFakeTimers();
    // 两侧最大滚动都是 900,比例相同则像素也相同。
    const source = scrollable(1000, 100);
    const preview = scrollable(1000, 100);
    registerPane("source", paneFromElement(source));
    registerPane("preview", paneFromElement(preview));
    // 注册后有一次延到下一 tick 的初始对齐,先把它跑掉。
    await vi.advanceTimersByTimeAsync(0);
    // 初始对齐会给预览侧上锁,等锁过期再开始测。
    await vi.advanceTimersByTimeAsync(200);

    scroll(source, 450);
    expect(preview.scrollTop).toBe(450);
  });

  it("两侧高度不同时按比例换算", async () => {
    vi.useFakeTimers();
    const source = scrollable(1000, 100); // max 900
    const preview = scrollable(2000, 100); // max 1900
    registerPane("source", paneFromElement(source));
    registerPane("preview", paneFromElement(preview));
    await vi.advanceTimersByTimeAsync(200);

    scroll(source, 450); // 比例 0.5
    expect(preview.scrollTop).toBe(950); // 0.5 × 1900
  });

  it("不互相回声拉扯", async () => {
    vi.useFakeTimers();
    const source = scrollable(1000, 100);
    const preview = scrollable(1000, 100);
    registerPane("source", paneFromElement(source));
    registerPane("preview", paneFromElement(preview));
    await vi.advanceTimersByTimeAsync(200);

    scroll(source, 450);
    expect(preview.scrollTop).toBe(450);
    // 写 preview 会触发它的 scroll 事件。没有锁的话它会反过来写 source,
    // 两侧互相拉扯 —— 表现是滚动发抖或卡住。
    preview.dispatchEvent(new Event("scroll"));
    expect(source.scrollTop).toBe(450);
  });

  it("锁过期后反向同步生效", async () => {
    vi.useFakeTimers();
    const source = scrollable(1000, 100);
    const preview = scrollable(1000, 100);
    registerPane("source", paneFromElement(source));
    registerPane("preview", paneFromElement(preview));
    await vi.advanceTimersByTimeAsync(200);

    scroll(source, 450);
    // 锁只防回声,不该永久禁掉某个方向。
    await vi.advanceTimersByTimeAsync(200);
    scroll(preview, 900);
    expect(source.scrollTop).toBe(900);
  });

  it("贴边时对齐到端点", async () => {
    vi.useFakeTimers();
    const source = scrollable(1000, 100);
    const preview = scrollable(3000, 100);
    registerPane("source", paneFromElement(source));
    registerPane("preview", paneFromElement(preview));
    await vi.advanceTimersByTimeAsync(200);

    // 滚到底:预览也要真的到底,不能因为取整差异停在「差一点」。
    scroll(source, 900);
    expect(preview.scrollTop).toBe(2900);

    await vi.advanceTimersByTimeAsync(200);
    scroll(source, 0);
    expect(preview.scrollTop).toBe(0);
  });

  it("只注册一侧时不动任何东西", () => {
    const source = scrollable(1000, 100);
    registerPane("source", paneFromElement(source));
    // 不该抛,也不该有副作用。
    expect(() => scroll(source, 450)).not.toThrow();
  });

  it("注销后不再同步", async () => {
    vi.useFakeTimers();
    const source = scrollable(1000, 100);
    const preview = scrollable(1000, 100);
    registerPane("source", paneFromElement(source));
    registerPane("preview", paneFromElement(preview));
    await vi.advanceTimersByTimeAsync(200);

    registerPane("preview", null);
    scroll(source, 450);
    expect(preview.scrollTop).toBe(0);
  });

  it("重复注册同一侧会换掉旧的监听", async () => {
    vi.useFakeTimers();
    const source = scrollable(1000, 100);
    const first = scrollable(1000, 100);
    const second = scrollable(1000, 100);
    registerPane("source", paneFromElement(source));
    registerPane("preview", paneFromElement(first));
    registerPane("preview", paneFromElement(second));
    await vi.advanceTimersByTimeAsync(200);

    scroll(source, 450);
    expect(second.scrollTop).toBe(450);
    // 旧的必须被解绑,否则会攒下一堆已经不在文档里的元素的监听。
    expect(first.scrollTop).toBe(0);
  });

  it("resetSplitScrollSync 之后彻底停止", async () => {
    vi.useFakeTimers();
    const source = scrollable(1000, 100);
    const preview = scrollable(1000, 100);
    registerPane("source", paneFromElement(source));
    registerPane("preview", paneFromElement(preview));
    await vi.advanceTimersByTimeAsync(200);

    resetSplitScrollSync();
    scroll(source, 450);
    expect(preview.scrollTop).toBe(0);
  });

  it("syncPreviewToSource 主动对齐一次", async () => {
    vi.useFakeTimers();
    const source = scrollable(1000, 100);
    const preview = scrollable(1000, 100);
    registerPane("source", paneFromElement(source));
    registerPane("preview", paneFromElement(preview));
    await vi.advanceTimersByTimeAsync(200);

    // 直接改 scrollTop 不派事件,模拟「预览内容重渲染后高度变了」。
    source.scrollTop = 450;
    syncPreviewToSource();
    await vi.advanceTimersByTimeAsync(0);
    expect(preview.scrollTop).toBe(450);
  });

  it("不可滚动的元素比例为 0,不抛", () => {
    // 内容比视口短时 scrollHeight - clientHeight <= 0,除法会得到 Infinity/NaN。
    const pane = paneFromElement(scrollable(100, 500));
    expect(pane.getRatio()).toBe(0);
    expect(() => pane.setRatio(0.5)).not.toThrow();
  });
});
