import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

/**
 * WebGL 上下文的全局配额。
 *
 * 浏览器超出上限时不报错,而是静默丢弃最老的上下文 —— 被顶掉的那个终端画面变空白,
 * 自己毫不知情。这类故障没法在运行时观测,只能靠这里守住「不超发、且归还」。
 */

const addonState = vi.hoisted(() => ({
  created: 0,
  /** 每个 addon 实例的 onContextLoss 回调。 */
  contextLossHandlers: [] as Array<() => void>,
  /** 下一次 new WebglAddon() 抛出。 */
  throwOnNext: false,
}));

vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: class {
    dispose = vi.fn();
    constructor() {
      if (addonState.throwOnNext) {
        addonState.throwOnNext = false;
        throw new Error("no webgl");
      }
      addonState.created += 1;
    }
    onContextLoss(handler: () => void) {
      addonState.contextLossHandlers.push(handler);
    }
  },
}));

vi.mock("@xterm/xterm", () => ({ Terminal: class {} }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class {} }));
vi.mock("@xterm/addon-unicode11", () => ({ Unicode11Addon: class {} }));

const { liveWebglContextCount, loadWebglAddon, resetWebglContextBudget } =
  await import("../components/terminalShared");

/**
 * 只实现 loadWebglAddon 用到的:loadAddon(挂 addon)、dispose(触发 addon 的 dispose)。
 * 真实 xterm 在 dispose() 时会连带 dispose 所有 loadAddon 进去的 addon。
 */
function fakeTerm() {
  const addons: Array<{ activate?: () => void; dispose?: () => void }> = [];
  return {
    loadAddon: vi.fn((addon: { activate?: () => void; dispose?: () => void }) => {
      addons.push(addon);
      addon.activate?.();
    }),
    /** 模拟终端销毁。xterm 的实现是 dispose 所有 addon。 */
    dispose: () => {
      for (const addon of addons) addon.dispose?.();
    },
  };
}

beforeEach(() => {
  resetWebglContextBudget();
  addonState.created = 0;
  addonState.contextLossHandlers = [];
  addonState.throwOnNext = false;
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("WebGL 配额", () => {
  it("配额内的终端都拿到 addon", () => {
    for (let i = 0; i < 6; i += 1) {
      const term = fakeTerm();
      loadWebglAddon(term as never);
      // loadAddon 调两次:WebGL addon + 销毁哨兵 addon。
      expect(term.loadAddon).toHaveBeenCalledTimes(2);
    }
    expect(liveWebglContextCount()).toBe(6);
  });

  // 超出上限时建了就已经把别人顶掉了,所以是"不建",不是"建了再看"。
  it("超出配额的终端根本不建上下文", () => {
    const terms = Array.from({ length: 8 }, () => fakeTerm());
    for (const term of terms) loadWebglAddon(term as never);

    expect(addonState.created).toBe(6);
    expect(liveWebglContextCount()).toBe(6);
    expect(terms[6].loadAddon).not.toHaveBeenCalled();
    expect(terms[7].loadAddon).not.toHaveBeenCalled();
  });

  it("终端销毁时归还配额,后来者能拿到", () => {
    const terms = Array.from({ length: 6 }, () => fakeTerm());
    for (const term of terms) loadWebglAddon(term as never);
    expect(liveWebglContextCount()).toBe(6);

    terms[0].dispose();
    expect(liveWebglContextCount()).toBe(5);

    const late = fakeTerm();
    loadWebglAddon(late as never);
    expect(late.loadAddon).toHaveBeenCalledTimes(2);
  });

  /**
   * 只挂 onContextLoss 不够。正常关终端不触发它,配额就只减不增 —— 开关几次之后所有
   * 新终端都被挤到 DOM 渲染器上,而 WebGL 明明是空的。
   */
  it("反复开关不会让配额漏损", () => {
    for (let round = 0; round < 20; round += 1) {
      const term = fakeTerm();
      loadWebglAddon(term as never);
      term.dispose();
    }
    expect(liveWebglContextCount()).toBe(0);

    const term = fakeTerm();
    loadWebglAddon(term as never);
    expect(term.loadAddon).toHaveBeenCalledTimes(2);
  });

  it("重复 dispose 不会把配额减成负数", () => {
    const term = fakeTerm();
    loadWebglAddon(term as never);
    term.dispose();
    term.dispose();
    expect(liveWebglContextCount()).toBe(0);
  });

  it("上下文丢失时归还配额", () => {
    const term = fakeTerm();
    loadWebglAddon(term as never);
    expect(liveWebglContextCount()).toBe(1);

    addonState.contextLossHandlers[0]?.();
    expect(liveWebglContextCount()).toBe(0);
  });

  it("上下文丢失后再 dispose 不会重复归还", () => {
    const term = fakeTerm();
    loadWebglAddon(term as never);
    addonState.contextLossHandlers[0]?.();
    term.dispose();
    expect(liveWebglContextCount()).toBe(0);
  });

  // 构造抛出时两个回调都没挂上,配额得就地还,否则永久漏损。
  it("addon 构造失败时不占配额", () => {
    addonState.throwOnNext = true;
    const term = fakeTerm();
    loadWebglAddon(term as never);

    expect(liveWebglContextCount()).toBe(0);
    expect(term.loadAddon).not.toHaveBeenCalled();
  });

  it("构造失败之后配额仍然可用", () => {
    addonState.throwOnNext = true;
    loadWebglAddon(fakeTerm() as never);

    const term = fakeTerm();
    loadWebglAddon(term as never);
    // loadWebglAddon 挂两个 addon:WebGL 和销毁哨兵,所以成功时是 2。
    expect(term.loadAddon).toHaveBeenCalledTimes(2);
    expect(liveWebglContextCount()).toBe(1);
  });
});
