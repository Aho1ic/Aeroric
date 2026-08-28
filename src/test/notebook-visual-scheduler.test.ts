// 视口优先懒渲染调度器的测试。用一个合成 renderer 记录调用顺序并模拟单块耗时;
// 不在这里跑真的 katex/mermaid —— 那需要真实浏览器。
//
// 移植自 Markio(`src/lib/visualScheduler.test.ts`)。原文件用环境指令切到
// happy-dom;这里跟随 Aeroric 的 jsdom 默认环境,不为一个测试文件引入第二个
// DOM 实现。注意:那条环境指令即使写在注释里也会被 vitest 的文本扫描认出来,
// 所以这里刻意不复述它的字面写法。

import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { scheduleVisualBlocks } from "../components/notebook/visualScheduler";

interface FakeEntry {
  target: Element;
  isIntersecting: boolean;
}
type IOCallback = (entries: FakeEntry[]) => void;

class FakeIntersectionObserver {
  static instances: FakeIntersectionObserver[] = [];
  observed: Element[] = [];
  disconnected = false;
  constructor(public readonly callback: IOCallback) {
    FakeIntersectionObserver.instances.push(this);
  }
  observe(el: Element) {
    if (!this.disconnected) this.observed.push(el);
  }
  unobserve(el: Element) {
    this.observed = this.observed.filter((x) => x !== el);
  }
  disconnect() {
    this.disconnected = true;
    this.observed = [];
  }
  fireAll() {
    this.callback(this.observed.map((target) => ({ target, isIntersecting: true })));
  }
  fireFirst(n: number) {
    const fired = this.observed.slice(0, n);
    this.callback(fired.map((target) => ({ target, isIntersecting: true })));
  }
}

beforeEach(() => {
  FakeIntersectionObserver.instances = [];
  (
    globalThis as unknown as { IntersectionObserver: typeof FakeIntersectionObserver }
  ).IntersectionObserver = FakeIntersectionObserver;
});

afterEach(() => {
  delete (globalThis as unknown as Record<string, unknown>).IntersectionObserver;
});

function makeBlocks(count: number, className = "viz-block"): HTMLElement {
  const root = document.createElement("div");
  for (let i = 0; i < count; i++) {
    const el = document.createElement("div");
    el.className = className;
    el.dataset.idx = String(i);
    root.appendChild(el);
  }
  document.body.appendChild(root);
  return root;
}

function stubLayout(root: HTMLElement, heightPerBlock = 100) {
  Array.from(root.querySelectorAll<HTMLElement>("div.viz-block")).forEach((bq, i) => {
    const top = i * heightPerBlock;
    bq.getBoundingClientRect = () => ({
      top,
      bottom: top + 80,
      left: 0,
      right: 100,
      width: 100,
      height: 80,
      x: 0,
      y: top,
      toJSON: () => ({}),
    });
  });
}

const instantYield = () => Promise.resolve();

describe("scheduleVisualBlocks — correctness", () => {
  it("renders only visible blocks first; pending wait for IO", async () => {
    const root = makeBlocks(20);
    stubLayout(root);
    const rendered: string[] = [];
    const handle = scheduleVisualBlocks<HTMLElement>(
      root,
      "div.viz-block",
      (block) => {
        rendered.push(block.dataset.idx!);
        block.dataset.rendered = "1";
      },
      { viewportHeight: 400, visibilityMargin: 0, yieldFn: instantYield },
    );
    // Let visible queue drain.
    await new Promise((r) => setTimeout(r, 10));
    // Blocks 0..4 visible (top <=400)
    expect(rendered.length).toBeGreaterThanOrEqual(4);
    expect(rendered.length).toBeLessThan(20);
    // Trigger IO for the rest:
    FakeIntersectionObserver.instances[0]!.fireAll();
    await new Promise((r) => setTimeout(r, 50));
    expect(rendered.length).toBe(20);
    handle.disconnect();
  });

  it("renderOne is serial (not concurrent) across visible + IO bursts", async () => {
    const root = makeBlocks(10);
    stubLayout(root);
    let inFlight = 0;
    let maxInFlight = 0;
    const handle = scheduleVisualBlocks<HTMLElement>(
      root,
      "div.viz-block",
      async (_block) => {
        inFlight++;
        maxInFlight = Math.max(maxInFlight, inFlight);
        await new Promise((r) => setTimeout(r, 1));
        inFlight--;
      },
      { viewportHeight: 50, visibilityMargin: 0, yieldFn: instantYield },
    );
    await new Promise((r) => setTimeout(r, 30));
    FakeIntersectionObserver.instances[0]!.fireAll();
    await new Promise((r) => setTimeout(r, 100));
    expect(maxInFlight).toBe(1);
    handle.disconnect();
  });

  it("flushAll() renders everything and resolves only when done", async () => {
    const root = makeBlocks(8);
    stubLayout(root);
    const order: number[] = [];
    const handle = scheduleVisualBlocks<HTMLElement>(
      root,
      "div.viz-block",
      async (block) => {
        await new Promise((r) => setTimeout(r, 1));
        order.push(Number(block.dataset.idx));
      },
      { viewportHeight: 50, visibilityMargin: 0, yieldFn: instantYield },
    );
    await handle.flushAll();
    expect(order).toHaveLength(8);
    expect(new Set(order).size).toBe(8);
  });

  it("disconnect() stops scheduler before in-queue blocks finish", async () => {
    const root = makeBlocks(20);
    stubLayout(root);
    const rendered: number[] = [];
    const handle = scheduleVisualBlocks<HTMLElement>(
      root,
      "div.viz-block",
      async (block) => {
        await new Promise((r) => setTimeout(r, 2));
        rendered.push(Number(block.dataset.idx));
      },
      { viewportHeight: 50, visibilityMargin: 0, yieldFn: instantYield },
    );
    // Visible set: just block 0 (top=0, bottom=80, viewport 50 + margin 0 → bottom>=0 && top<=50)
    FakeIntersectionObserver.instances[0]!.fireAll();
    handle.disconnect();
    await new Promise((r) => setTimeout(r, 80));
    // Some may have rendered before disconnect, but not all 20
    expect(rendered.length).toBeLessThan(20);
  });

  it("falls back to non-IO mode when IntersectionObserver missing", async () => {
    delete (globalThis as unknown as Record<string, unknown>).IntersectionObserver;
    const root = makeBlocks(5);
    stubLayout(root);
    const order: number[] = [];
    const handle = scheduleVisualBlocks<HTMLElement>(
      root,
      "div.viz-block",
      (block) => {
        order.push(Number(block.dataset.idx));
      },
      { viewportHeight: 50, visibilityMargin: 0, yieldFn: instantYield },
    );
    await handle.flushAll();
    expect(order).toEqual([0, 1, 2, 3, 4]);
  });

  it("idle yield is invoked between blocks", async () => {
    const root = makeBlocks(5);
    stubLayout(root);
    let yieldCount = 0;
    const handle = scheduleVisualBlocks<HTMLElement>(root, "div.viz-block", () => undefined, {
      viewportHeight: 10_000, // everything visible
      yieldFn: async () => {
        yieldCount++;
      },
    });
    await handle.flushAll();
    expect(yieldCount).toBeGreaterThanOrEqual(5);
  });

  it("returns a noop handle when there are no matching blocks", async () => {
    const root = document.createElement("div");
    document.body.appendChild(root);
    const handle = scheduleVisualBlocks<HTMLElement>(root, ".viz-block", () => undefined);
    expect(typeof handle.disconnect).toBe("function");
    await expect(handle.flushAll()).resolves.toBeUndefined();
  });

  it("renderer errors do not break the queue", async () => {
    const root = makeBlocks(5);
    stubLayout(root);
    const rendered: number[] = [];
    const handle = scheduleVisualBlocks<HTMLElement>(
      root,
      "div.viz-block",
      async (block) => {
        const i = Number(block.dataset.idx);
        if (i === 2) throw new Error("boom");
        rendered.push(i);
      },
      { viewportHeight: 10_000, yieldFn: instantYield },
    );
    await handle.flushAll();
    expect(rendered).toEqual([0, 1, 3, 4]);
  });
});

describe("scheduleVisualBlocks — first paint perf", () => {
  it("setup + visible render on 500-block doc with cheap renderer is fast", async () => {
    const root = makeBlocks(500);
    stubLayout(root);
    const t0 = performance.now();
    const handle = scheduleVisualBlocks<HTMLElement>(root, "div.viz-block", () => undefined, {
      viewportHeight: 1080,
      yieldFn: instantYield,
    });
    // Sync setup cost — visible queue starts but isn't awaited here.
    const setupMs = performance.now() - t0;
    // Aeroric 的 lint 只允许 console.warn / console.error。
    console.warn(`[bench] scheduleVisualBlocks setup (500 blocks): ${setupMs.toFixed(2)}ms`);
    expect(setupMs).toBeLessThan(100);
    handle.disconnect();
  });
});
