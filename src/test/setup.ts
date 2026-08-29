import "@testing-library/jest-dom";
import { configure } from "@testing-library/react";
import { vi } from "vitest";
import { installResizeObserverStub } from "./resizeObserverStub";

// Testing Library 默认给 waitFor / findBy* 只留 1000ms。本仓库有多处真实防抖
// (侧边栏搜索 180ms、任务落盘 350ms),叠上 v8 coverage 插桩与 CI 上的多进程争抢,
// 1000ms 会被吃满 —— 这类失败与被测逻辑无关,只反映当时机器有多忙。
// 放宽到 3000ms:通过路径不会因此变慢(waitFor 一满足就返回),只有真失败时多等 2s。
configure({ asyncUtilTimeout: 3000 });

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

class MemoryStorage implements Storage {
  private readonly data = new Map<string, string>();

  get length() {
    return this.data.size;
  }

  clear() {
    this.data.clear();
  }

  getItem(key: string) {
    return this.data.get(key) ?? null;
  }

  key(index: number) {
    return Array.from(this.data.keys())[index] ?? null;
  }

  removeItem(key: string) {
    this.data.delete(key);
  }

  setItem(key: string, value: string) {
    this.data.set(key, value);
  }
}

if (typeof localStorage.clear !== "function") {
  Object.defineProperty(globalThis, "localStorage", {
    value: new MemoryStorage(),
    configurable: true,
  });
}

// jsdom 未实现 window.matchMedia()，主题状态在初始化时就会调用它。
if (typeof window !== "undefined" && typeof window.matchMedia !== "function") {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    }),
  });
}

// jsdom 未实现 Range.getClientRects() / getBoundingClientRect()。CodeMirror 6 在
// requestAnimationFrame 里做文字测量时会调用它们，而 rAF 回调往往在测试结束、
// DOM 已拆掉之后才执行 —— 报出来是 unhandled error，和被测逻辑无关。
// 返回空矩形集合：jsdom 本来就没有布局，CodeMirror 拿到 0 尺寸会走它自己的降级路径。
if (typeof Range !== "undefined") {
  const emptyRect = () => ({
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    bottom: 0,
    right: 0,
    width: 0,
    height: 0,
    toJSON: () => ({}),
  });
  if (typeof Range.prototype.getClientRects !== "function") {
    Range.prototype.getClientRects = function getClientRects() {
      const list: DOMRect[] = [];
      return Object.assign(list, {
        item: (index: number) => list[index] ?? null,
      }) as unknown as DOMRectList;
    };
  }
  if (typeof Range.prototype.getBoundingClientRect !== "function") {
    Range.prototype.getBoundingClientRect = emptyRect as unknown as () => DOMRect;
  }
}

// jsdom 未实现 HTMLCanvasElement.getContext()，xterm 等组件在渲染时会调用它。
// 提供一个最小的 2D context stub，消除测试日志中的 "Not implemented" 噪声。
if (typeof HTMLCanvasElement !== "undefined") {
  const noop = () => {};
  const stubContext = {
    canvas: null as unknown,
    fillRect: noop,
    clearRect: noop,
    getImageData: (_x: number, _y: number, w: number, h: number) => ({
      data: new Uint8ClampedArray(Math.max(0, w) * Math.max(0, h) * 4),
      width: w,
      height: h,
    }),
    putImageData: noop,
    createImageData: (w: number, h: number) => ({
      data: new Uint8ClampedArray(Math.max(0, w) * Math.max(0, h) * 4),
      width: w,
      height: h,
    }),
    setTransform: noop,
    drawImage: noop,
    save: noop,
    fillText: noop,
    restore: noop,
    beginPath: noop,
    moveTo: noop,
    lineTo: noop,
    closePath: noop,
    stroke: noop,
    translate: noop,
    scale: noop,
    rotate: noop,
    arc: noop,
    fill: noop,
    measureText: (text: string) => ({ width: text.length * 6 }),
    transform: noop,
    rect: noop,
    clip: noop,
  };
  HTMLCanvasElement.prototype.getContext = vi.fn(
    () => stubContext,
  ) as unknown as typeof HTMLCanvasElement.prototype.getContext;
}

// jsdom 没有 ResizeObserver,面板的三档布局要靠它测。
installResizeObserverStub();
