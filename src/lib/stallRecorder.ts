/**
 * 卡顿归因记录器 —— 回答「刚才那几秒卡在哪」。
 *
 * 起因:用户报「有时候点一下按钮要等好几秒」。这类间歇卡顿静态分析找不出来 ——
 * 下面每一条都实测过,全都比症状小三到四个数量级,**全部排除**:
 *
 * | 机制 | 实测 |
 * | --- | --- |
 * | `~/.aeroric` 上的存档读写(`load_projects` 等) | < 0.3 ms |
 * | 8 MB 终端历史回放进 xterm | 95 ms(且 xterm 内部分片,不是一个长任务) |
 * | `emit` 洪水(32 KiB/条,255 条积压) | 0.127 ms/条,积压后一次点击只等 9 ms |
 * | `projectDshSessionEvents`(4 万事件) | 13.4 ms |
 * | 用量库 30 天查询(70 MB 库,15.6 万行) | 20 ms |
 * | `lsp_change_document` 整文件往返(142 KB) | 0.53 ms/键 |
 * | 用量索引器扫盘(1958 个 jsonl) | 42 ms |
 * | 隐藏子树合成 | 任务面板同时只挂 1 个,量级不够 |
 * | 双写者 SQLite(同一个 70 MB 库) | 尾部 1.2–1.9 s,但两侧都不在主线程 |
 * | 超过 WebGL 配额后退化的 DOM 渲染器 | 4000 行 33 ms(只渲可见行) |
 * | 整机内存压力下重新触碰冷页 | 0.75 us/页,12288 页共 10 ms |
 *
 * 也就是说:**猜是猜不到的**,得在真的卡住那一下抓现场。这个模块干这件事,代价
 * 接近零:`longtask` 观察器由浏览器被动上报(不卡就没有回调),invoke 计时只是每
 * 次调用多两次 `performance.now()`,输入探针是被动监听器。
 *
 * 判读方式见 `stallReport()` 的注释。
 */

/** 超过这个时长的主线程任务才记 —— 与 Long Tasks API 的门槛一致。 */
const LONG_TASK_THRESHOLD_MS = 50;
/** 超过这个时长的 invoke 才记。比一帧多一截,正常命令都在这之下。 */
const SLOW_INVOKE_THRESHOLD_MS = 100;
/**
 * 超过这个延迟的输入事件才记。
 *
 * 这一档是专门为「点了一下,什么都没发生,过几秒才动」准备的:那种卡顿里既没有长
 * 任务(JS 根本还没跑到),也没有慢命令(invoke 还没发出去),前两个桶都是空的。
 * 100 ms 已经是人能明确感到「不跟手」的门槛。
 */
const SLOW_INPUT_THRESHOLD_MS = 100;
/** 每类各留多少条。有界,免得自己变成泄漏源。 */
const MAX_SAMPLES = 50;
/**
 * 输入延迟的合理上界。超过就当作时间基准不可比而丢弃 —— `event.timeStamp` 在个别
 * 引擎上曾经是 epoch 毫秒而不是 time-origin 相对值,那种值减出来是天文数字。
 */
const MAX_PLAUSIBLE_INPUT_DELAY_MS = 60_000;

export interface StallSample {
  /** 长任务:`longtask`;慢命令:被调用的 command 名。 */
  label: string;
  durationMs: number;
  /** 距进程启动的毫秒数,用来和用户「刚才卡了」对时。 */
  atMs: number;
}

/** 一次「不跟手」的输入。两段时间分开记,因为它们指向完全不同的原因。 */
export interface InputSample {
  /** 事件类型,`pointerdown` / `click`。 */
  label: string;
  /**
   * 从系统生成事件到 JS 处理器真正跑起来。
   *
   * 大 = 主线程在别的事情上没让出来,或者事件在 JS 之前就被拖住了(合成、
   * 输入管线、进程被换出)。这一段是前两个桶都看不见的部分。
   */
  toHandlerMs: number;
  /** 从处理器跑起来到下一帧画完 —— 「点上了但画面没动」的那一段。 */
  toFrameMs: number;
  atMs: number;
}

export interface InvokeStat {
  command: string;
  calls: number;
  totalMs: number;
  maxMs: number;
}

export interface StallReport {
  longTasks: StallSample[];
  slowInvokes: StallSample[];
  /** 不跟手的输入事件 —— 前两个桶都空但用户确实感到卡时,看这里。 */
  slowInputs: InputSample[];
  /** 按累计耗时排序的 command 统计 —— 找「单次不慢但次数极多」的那种。 */
  invokeTotals: InvokeStat[];
  /** 观察器是否真的装上了(Safari/WKWebView 不支持 longtask 时为 false)。 */
  longTaskObserverActive: boolean;
  /**
   * invoke 计时是否真的装上了。
   *
   * 这一位不是多余的:探针本身就曾经因为「以为能改的属性其实只读」而静默失效
   * (见 `installStallRecorder()` 的注释)。三个桶全空时,先看这里是不是 false ——
   * 否则会把「探针没装上」误读成「没有卡顿」。
   */
  invokeProbeActive: boolean;
}

const longTasks: StallSample[] = [];
const slowInvokes: StallSample[] = [];
const slowInputs: InputSample[] = [];
const invokeStats = new Map<string, InvokeStat>();
let longTaskObserverActive = false;
let longTaskObserver: PerformanceObserver | null = null;
let invokeProbeActive = false;
/** 包装前的 `fetch`,reset 时还原 —— 否则重复装载会套娃,一次调用记多份。 */
let originalFetchRef: typeof globalThis.fetch | null = null;
let installed = false;

function record(bucket: StallSample[], sample: StallSample): void {
  bucket.push(sample);
  if (bucket.length > MAX_SAMPLES) bucket.shift();
}

/**
 * 从 IPC 请求的 URL 里取出 command 名。
 *
 * `invoke` 最终由 `ipc-protocol.js` 发成一个 POST(tauri 2.11 `scripts/ipc-protocol.js`),
 * 目标 URL 由 `convertFileSrc(cmd, 'ipc')` 拼出,两种形状(见 `scripts/core.js`):
 *
 * - macOS / Linux:`ipc://localhost/<encodeURIComponent(cmd)>`
 * - Windows / Android:`<scheme>://ipc.localhost/<encodeURIComponent(cmd)>`
 *
 * 不是 IPC 就返回 null。先做廉价的字符串判断再解析 URL —— 这个函数在每个 fetch 上
 * 都要跑一遍,不能给每次调用都摊上一次 URL 构造。
 */
function ipcCommandFromUrl(url: string): string | null {
  if (!url.includes("ipc")) return null;
  const isIpc = url.startsWith("ipc://") || url.includes("://ipc.localhost/");
  if (!isIpc) return null;
  try {
    const path = new URL(url).pathname.replace(/^\//, "");
    if (!path) return null;
    return decodeURIComponent(path);
  } catch {
    return null;
  }
}

/** 把一次 fetch 的输入归一成 URL 字符串。`Request` 与 `URL` 都要认。 */
function requestUrl(input: unknown): string {
  if (typeof input === "string") return input;
  if (input instanceof URL) return input.href;
  if (typeof input === "object" && input !== null && "url" in input) {
    const url = (input as { url?: unknown }).url;
    if (typeof url === "string") return url;
  }
  return "";
}

/** 监听的输入事件。`pointerdown` 抓按下,`click` 抓完整一次点击。 */
const INPUT_EVENT_TYPES = ["pointerdown", "click"] as const;

/** 已挂上的监听器,重复装载时先摘掉旧的 —— 否则一次点击会被记多份。 */
let inputListener: ((event: Event) => void) | null = null;

function removeInputDelayProbe(): void {
  if (!inputListener) return;
  if (typeof window !== "undefined" && typeof window.removeEventListener === "function") {
    for (const type of INPUT_EVENT_TYPES) {
      window.removeEventListener(type, inputListener, { capture: true });
    }
  }
  inputListener = null;
}

/**
 * 量「点下去多久才轮到 JS」以及「JS 跑完多久才画出来」。
 *
 * 用捕获阶段挂在 window 上,尽量早于应用自己的处理器,量到的才是输入管线的延迟而
 * 不是应用逻辑的耗时;`passive` 保证不影响滚动与默认行为。
 */
function installInputDelayProbe(): void {
  if (typeof window === "undefined" || typeof window.addEventListener !== "function") return;
  // 先摘旧的:生产路径只装一次,但测试会 reset 后重装,残留监听器会让样本翻倍。
  removeInputDelayProbe();

  const onInput = (event: Event): void => {
    const handlerAt = performance.now();
    // event.timeStamp 与 performance.now() 同源(都相对 time origin),差值即排队时间。
    const toHandlerMs = handlerAt - event.timeStamp;
    if (!Number.isFinite(toHandlerMs)) return;
    if (toHandlerMs < 0 || toHandlerMs > MAX_PLAUSIBLE_INPUT_DELAY_MS) return;

    const emit = (toFrameMs: number): void => {
      // 装载被 reset 之后,上一轮挂出去的 rAF 不能再往新桶里塞。
      if (inputListener !== onInput) return;
      if (toHandlerMs < SLOW_INPUT_THRESHOLD_MS && toFrameMs < SLOW_INPUT_THRESHOLD_MS) return;
      slowInputs.push({
        label: event.type,
        toHandlerMs: Math.round(toHandlerMs),
        toFrameMs: Math.round(toFrameMs),
        atMs: Math.round(handlerAt),
      });
      if (slowInputs.length > MAX_SAMPLES) slowInputs.shift();
    };

    if (typeof requestAnimationFrame !== "function") {
      emit(0);
      return;
    }
    requestAnimationFrame(() => emit(performance.now() - handlerAt));
  };

  inputListener = onInput;
  for (const type of INPUT_EVENT_TYPES) {
    window.addEventListener(type, onInput, { capture: true, passive: true });
  }
}

/**
 * 装上记录器。幂等,重复调用无副作用。
 *
 * 三个探针:
 * - `longtask` 观察器:主线程被占住超过 50 ms 就上报。**它只告诉你卡了多久,不告诉
 *   你是谁** —— Long Tasks API 的归因字段在 WebKit 上基本是空的。所以要配合下面那个。
 * - `window.fetch` 包装:所有 `invoke` 最终都发成一个到 `ipc://` 的 POST,command 名
 *   在 URL 里。慢命令直接点名。
 * - 输入延迟探针:量「点下去多久才轮到 JS」。
 *
 * **为什么包 `fetch` 而不是 `__TAURI_INTERNALS__.invoke`** —— 后者包不了。tauri 用
 * `Object.defineProperty` 定义它且只给 `value`(`scripts/core.js:81`),于是
 * `writable` 和 `configurable` 都默认 false:属性既不可写也不可重定义。ESM 是严格
 * 模式,`internals.invoke = …` 会当场抛 `TypeError`;这里跑在 `createRoot` 之前,
 * 抛出去就是整个应用白屏。`window.__TAURI_INTERNALS__` 自身同样是只给 `value` 的
 * `defineProperty`(`src/manager/webview.rs:173`),所以连「换掉整个对象」也不行。
 *
 * 这个坑在两处验证里都是隐形的:standalone WKWebView 里没有 `__TAURI_INTERNALS__`,
 * 守卫直接短路;单测的替身是对象字面量,`invoke` 是普通可写属性。只有真实 webview
 * 才会抛。所以下面每个装载点都包在 try/catch 里,并且把「有没有真的装上」放进
 * `StallReport` —— 探针自己静默失效,比没有探针更糟。
 */
export function installStallRecorder(): void {
  if (installed) return;
  installed = true;

  installInputDelayProbe();

  if (typeof PerformanceObserver === "function") {
    // 同上:reset 后重装不能留下上一个观察器,否则一个长任务会被记两遍。
    longTaskObserver?.disconnect();
    try {
      const observer = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          if (entry.duration < LONG_TASK_THRESHOLD_MS) continue;
          record(longTasks, {
            label: entry.name || "longtask",
            durationMs: Math.round(entry.duration),
            atMs: Math.round(entry.startTime),
          });
        }
      });
      observer.observe({ entryTypes: ["longtask"] });
      longTaskObserver = observer;
      longTaskObserverActive = true;
    } catch {
      // WebKit 直到近期才支持 longtask;不支持就只靠 invoke 计时。
      longTaskObserverActive = false;
    }
  }

  // 见上:`internals.invoke` 不可写也不可重定义,只能包真实传输层。
  if (typeof globalThis.fetch === "function") {
    const originalFetch = globalThis.fetch;
    const wrapped = (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
      const command = ipcCommandFromUrl(requestUrl(input));
      if (command === null) return originalFetch.call(globalThis, input, init);

      const startedAt = performance.now();
      const settle = () => {
        const durationMs = performance.now() - startedAt;
        let stat = invokeStats.get(command);
        if (!stat) {
          stat = { command, calls: 0, totalMs: 0, maxMs: 0 };
          invokeStats.set(command, stat);
        }
        stat.calls += 1;
        stat.totalMs += durationMs;
        if (durationMs > stat.maxMs) stat.maxMs = durationMs;
        if (durationMs >= SLOW_INVOKE_THRESHOLD_MS) {
          record(slowInvokes, {
            label: command,
            durationMs: Math.round(durationMs),
            atMs: Math.round(startedAt),
          });
        }
      };
      // 成功和失败都要计时:一个报错但很慢的命令同样把点击拖住了。
      return originalFetch.call(globalThis, input, init).then(
        (value) => {
          settle();
          return value;
        },
        (reason) => {
          settle();
          throw reason;
        },
      );
    };
    try {
      globalThis.fetch = wrapped;
      invokeProbeActive = globalThis.fetch === wrapped;
      originalFetchRef = invokeProbeActive ? originalFetch : null;
    } catch {
      // 真发生就说明这个引擎连 fetch 都锁了。记下来,别把应用带崩。
      invokeProbeActive = false;
      originalFetchRef = null;
    }
  }
}

/**
 * 读一份快照。
 *
 * 判读:
 * - `slowInvokes` 里有条目 → 那次点击在等后端。看 command 名。
 * - `longTasks` 里有几百毫秒的条目而 `slowInvokes` 是空的 → 卡在前端 JS/渲染,
 *   后端是无辜的。
 * - `slowInputs` 里 `toHandlerMs` 很大而前两个桶都空 → 点击根本没及时轮到 JS。
 *   这是「点了没反应」最典型的形状:延迟发生在 JS 之前,所以长任务和慢命令都抓不到。
 * - `slowInputs` 里 `toFrameMs` 很大而 `toHandlerMs` 很小 → JS 收到得很及时,是画
 *   不出来:合成/GPU 那一侧。
 * - 三个桶全空但用户确实感到卡 → **先看 `invokeProbeActive` / `longTaskObserverActive`**。
 *   都为 true 才能推断「不在进程里」:那种情况是磁盘或 CPU 被别的进程吃满、或整机
 *   内存压力把进程换出,看 `top` / `vm_stat` / `fs_usage`。有 false 的先修探针 ——
 *   空报告可能只是没测到。
 * - `invokeTotals` 里某条 `calls` 极大而 `maxMs` 很小 → 高频小命令,单次不慢但
 *   把主线程磨没了。
 */
export function stallReport(): StallReport {
  return {
    longTasks: [...longTasks],
    slowInvokes: [...slowInvokes],
    slowInputs: [...slowInputs],
    invokeTotals: [...invokeStats.values()].sort((a, b) => b.totalMs - a.totalMs).slice(0, 20),
    longTaskObserverActive,
    invokeProbeActive,
  };
}

/**
 * 把读取器挂到 window,devtools 控制台里直接敲 `__aeroricStalls()`。
 *
 * 不像 `__aeroricCensus` 那样只在 dev 挂:间歇卡顿只在真实使用里出现,而这只是一个
 * 只读函数。release 里没有控制台入口,排查时用 `pnpm tauri dev` 跑一遍即可。
 */
export function installStallReportProbe(): void {
  (globalThis as { __aeroricStalls?: () => StallReport }).__aeroricStalls = stallReport;
}

/**
 * 清空已记录的样本,并允许重新装载。
 *
 * 重置 `installed` 是给测试用的:每个用例换一份传输层替身,幂等守卫会让第二次
 * `installStallRecorder()` 变成空操作,包装就挂不到新替身上。生产路径只在启动时
 * 装一次,不受影响。
 */
export function resetStallRecorder(): void {
  longTasks.length = 0;
  slowInvokes.length = 0;
  slowInputs.length = 0;
  invokeStats.clear();
  removeInputDelayProbe();
  // 观察器也要断开:否则 reset 后重装会有两个观察器,一个长任务被记两遍。
  longTaskObserver?.disconnect();
  longTaskObserver = null;
  longTaskObserverActive = false;
  // fetch 也要还原:不还原的话重装会把上一层包装当成 original 套进去,
  // 一次 IPC 被记两遍,而且套娃层数随 reset 次数线性增长。
  if (originalFetchRef) {
    try {
      globalThis.fetch = originalFetchRef;
    } catch {
      // 还原不了就只能留着,但至少别让 reset 本身抛出去。
    }
    originalFetchRef = null;
  }
  invokeProbeActive = false;
  installed = false;
}
