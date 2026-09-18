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

/**
 * 清空已登记的观察目标,由 setup.ts 的 afterEach 在**每个用例之后**调用。
 *
 * 为什么需要:entries 是模块级可变单例,正常路径靠组件 unmount 时 disconnect() 自摘。
 * 但只要有组件漏了 disconnect,它的 entry 就会留到下一个用例 —— 那时 `triggerResize()`
 * 会去调**已卸载树**的回调(对已卸载组件 setState / 读已摘掉的 DOM),表现为「单跑绿、
 * 整文件跑红」的顺序相关偶发失败。notebook-layout-tier-hook.test.tsx 那条「卸载后断开
 * 观察」正是靠 measureCount 不涨来判定的,前面漏下的 entry 会让它误判。
 *
 * 另:替身对同一 target 重复 observe() 不去重(每次都 entries.add 一个新对象),真实
 * ResizeObserver 按 target 去重,语义上略有偏差 —— 每个用例开头清一次能把这条差异
 * 限制在单个用例内部。
 */
export function resetResizeObserverStub(): void {
  entries.clear();
}
