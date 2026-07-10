import "@testing-library/jest-dom";
import { vi } from "vitest";

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
