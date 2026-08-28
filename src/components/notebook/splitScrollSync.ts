/* 分屏滚动同步:源码侧 ↔ 预览侧。
 *
 * 移植自 Markio(`src/lib/splitScrollSync.ts`),**简化为按比例同步**。
 *
 * Markio 的原版优先「按源码行号」对齐,比例只作兜底。那个方案更准(长代码块
 * 或长表格不会让两侧越滚越偏),但它依赖预览侧的 `data-line` 锚点 —— Markio 的
 * Rust 渲染器原生就打,位置信息是白拿的。我们走前端 marked,marked 不暴露 token
 * 的源码位置,靠累加 `raw` 长度重建会偏(lexer 会规范化空白),而给数学哨兵补
 * 换行凑行数又会产出额外的 `<p>`,破坏「顶层 token ↔ 顶层元素一一对应」。
 * 试过,不值当,所以这里只做比例同步。按行同步作为后续优化,前提是先给渲染器
 * 加上可靠的位置信息。
 *
 * 保留 Markio 的两个关键设计:
 *
 * 1. **纯命令式 DOM 操作,不走 React state。** 每次 scroll 都 setState 会让
 *    「scroll → 重渲染 → effect 比较 deps → 写 scrollTop」这条链路上任何一环
 *    (memo、prop 身份变化、render 时序)出问题就静默失效。
 * 2. **用定时锁防回声。** A 写 B 的 scrollTop 会触发 B 的 scroll 事件,B 又去写
 *    A —— 不加锁两侧会互相拉扯。锁的时长要略长于浏览器派发 scroll 的窗口。
 */

export type PaneRole = "source" | "preview";

export type PaneHandle = {
  /** 真正的滚动元素(CodeMirror 的 scrollDOM 或预览容器)。 */
  el: HTMLElement;
  getRatio: () => number;
  setRatio: (ratio: number) => void;
};

type Slot = {
  pane: PaneHandle;
  detach: () => void;
};

const slots: Record<PaneRole, Slot | null> = { source: null, preview: null };

let lock: PaneRole | null = null;
let lockTimer: ReturnType<typeof setTimeout> | null = null;

/** macOS WebKit 偶尔在下一帧之后才派发 scroll,180ms 是 Markio 实测的值。 */
const LOCK_MS = 180;
/** 贴边时直接对齐到 0 / 1,避免两侧因取整差异卡在「差一点到底」。 */
const EDGE_EPSILON = 0.006;

function setLock(role: PaneRole) {
  lock = role;
  if (lockTimer !== null) clearTimeout(lockTimer);
  lockTimer = setTimeout(() => {
    lock = null;
    lockTimer = null;
  }, LOCK_MS);
}

function other(role: PaneRole): PaneRole {
  return role === "source" ? "preview" : "source";
}

function syncFrom(origin: PaneRole) {
  // origin 刚刚被对端写过,这次 scroll 是回声 —— 丢掉。
  if (lock === origin) return;
  const source = slots[origin]?.pane;
  const target = slots[other(origin)]?.pane;
  if (!source || !target) return;

  // 写对端之前先锁住它,这样它的 scroll 事件回来时会被上面那行拦掉。
  setLock(other(origin));
  const ratio = source.getRatio();
  if (ratio <= EDGE_EPSILON) {
    target.setRatio(0);
    return;
  }
  if (ratio >= 1 - EDGE_EPSILON) {
    target.setRatio(1);
    return;
  }
  target.setRatio(ratio);
}

/**
 * 注册(或注销)一侧窗格。传 `null` 注销。
 *
 * 两侧都注册好之后会立刻从源码侧同步一次,让预览对齐当前位置。
 */
export function registerPane(role: PaneRole, pane: PaneHandle | null): void {
  const existing = slots[role];
  if (existing) {
    existing.detach();
    slots[role] = null;
  }
  if (!pane) return;

  const handler = () => syncFrom(role);
  pane.el.addEventListener("scroll", handler, { passive: true });
  slots[role] = {
    pane,
    detach: () => pane.el.removeEventListener("scroll", handler),
  };

  if (slots.source && slots.preview) {
    // 延到下一个 tick:注册发生在 effect 里,此时预览可能还没完成布局。
    setTimeout(() => syncFrom("source"), 0);
  }
}

/** 主动把预览对齐到源码当前位置。预览内容重渲染(公式/图渲染完)后调用。 */
export function syncPreviewToSource(): void {
  if (!slots.source || !slots.preview) return;
  setTimeout(() => syncFrom("source"), 0);
}

/** 清场。切出分屏视图或测试之间调用。 */
export function resetSplitScrollSync(): void {
  for (const role of ["source", "preview"] as PaneRole[]) {
    slots[role]?.detach();
    slots[role] = null;
  }
  lock = null;
  if (lockTimer !== null) {
    clearTimeout(lockTimer);
    lockTimer = null;
  }
}

/** 从一个滚动元素造 handle。两侧的比例计算是同一套。 */
export function paneFromElement(el: HTMLElement): PaneHandle {
  return {
    el,
    getRatio: () => {
      const max = el.scrollHeight - el.clientHeight;
      if (max <= 0) return 0;
      const ratio = el.scrollTop / max;
      // 布局未完成时 scrollHeight 可能是 0,算出 NaN / Infinity。
      return Number.isFinite(ratio) ? Math.min(1, Math.max(0, ratio)) : 0;
    },
    setRatio: (ratio) => {
      const max = el.scrollHeight - el.clientHeight;
      el.scrollTop = ratio * Math.max(0, max);
    },
  };
}
