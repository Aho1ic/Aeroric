/* jsdom 没有 ResizeObserver 的替身。
 *
 * 单独一个模块,不写进 setup.ts —— 测试要 import `triggerResize`,而 import
 * setup.ts 会把里面的 `beforeEach` / `afterEach` 再注册一遍(Vitest 已经把它
 * 当 setupFile 跑过一次),重复的钩子会把套件挂死。
 *
 * 替身把回调记下来,由 `triggerResize` **同步**派发。真实实现是异步的,但测试
 * 里同步更好控 —— 不用 waitFor 去等一个本来就该立刻发生的布局判定。
 */

type StubEntry = { target: Element; callback: ResizeObserverCallback };

const entries = new Set<StubEntry>();

/** 手动派发一次 resize。元素尺寸自己用 getBoundingClientRect 桩子给。 */
export function triggerResize(): void {
  for (const entry of entries) {
    entry.callback([], entry.target as unknown as ResizeObserver);
  }
}

/** 装上替身。真实环境里已经有就不动 —— 别把真实实现换掉。 */
export function installResizeObserverStub(): void {
  if (typeof globalThis.ResizeObserver !== "undefined") return;
  globalThis.ResizeObserver = class implements ResizeObserver {
    private callback: ResizeObserverCallback;
    constructor(callback: ResizeObserverCallback) {
      this.callback = callback;
    }
    observe(target: Element): void {
      entries.add({ target, callback: this.callback });
    }
    unobserve(target: Element): void {
      for (const entry of entries) {
        if (entry.target === target && entry.callback === this.callback) entries.delete(entry);
      }
    }
    disconnect(): void {
      for (const entry of entries) {
        if (entry.callback === this.callback) entries.delete(entry);
      }
    }
  };
}
