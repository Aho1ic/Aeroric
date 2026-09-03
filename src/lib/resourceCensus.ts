/**
 * 存活资源普查 —— 只登记「当前还活着几个」,不累计历史总量。
 *
 * 为什么要这个:长跑内存增长的静态分析只能指出「哪些路径可能泄漏」,指不出「实际上涨了多少」。
 * 增长曲线要拿真机跑几小时才看得出来。这个模块是让曲线可见的仪表:每类资源在 create 时 +1、
 * dispose 时 -1,任意时刻读快照就知道有没有该释放而没释放的。
 *
 * 判读方式:让应用空转(不开新面板、不发消息),隔几分钟读一次快照。
 * - 计数持平 → 这类资源没泄漏
 * - 计数只涨不跌 → 有 dispose 路径没走到
 * - 计数跟着操作涨、操作撤销后不跌 → 那条操作的 cleanup 漏了
 *
 * 配合外部 RSS 采样定位「涨的是不是这些」:
 *   while :; do ps -o rss=,%cpu= -p <pid>; sleep 60; done | tee rss.log
 * RSS 涨但这里的计数持平 → 泄漏不在这几类里(可能在 Rust 侧、或 DOM 节点、或本模块没覆盖的缓存)。
 *
 * 只在 dev 下接线。计数本身是无副作用的加减,留在 release 里也无害,但读取入口
 * (`installResourceCensusProbe`)会往 window 上挂东西 —— 那是调试面,不进 release。
 */

export interface ResourceCensusSnapshot {
  /** 挂载中的 xterm 实例数。每个终端面板一个,关面板应该减回去。 */
  liveTerminals: number;
  /** 持有中的 WebGL context 数。有全局配额(见 terminalShared 的 MAX_WEBGL_TERMINALS)。 */
  liveWebglContexts: number;
  /** SessionView 的 prose HTML 缓存:条数与字符数(双上界,见该文件)。 */
  proseCacheEntries: number;
  proseCacheChars: number;
  /** 注册中的轮询 timer 数。面板卸载后应归零。 */
  liveTimers: number;
}

/** 除 WebGL 和 prose cache 之外的计数,由本模块自己持有。 */
let liveTerminals = 0;
let liveTimers = 0;

/** xterm 实例创建。在 initTerminal 里调。 */
export function countTerminalCreated(): void {
  liveTerminals += 1;
}

/** xterm 实例销毁。挂在 term.onDispose 上,而不是散在各处的 cleanup 里 —— 后者容易漏。 */
export function countTerminalDisposed(): void {
  liveTerminals -= 1;
}

/** 轮询 timer 注册。 */
export function countTimerRegistered(): void {
  liveTimers += 1;
}

/** 轮询 timer 注销。 */
export function countTimerCleared(): void {
  liveTimers -= 1;
}

/**
 * 快照读取器。WebGL 计数和 prose cache 统计由各自模块持有,这里通过注入避免循环依赖:
 * terminalShared 和 SessionView 都要 import 本模块的计数函数,本模块再反过来 import 它们
 * 就成环了。
 */
type CensusProbe = () => number;

let readWebglContexts: CensusProbe = () => 0;
let readProseCacheEntries: CensusProbe = () => 0;
let readProseCacheChars: CensusProbe = () => 0;

export function registerWebglContextProbe(probe: CensusProbe): void {
  readWebglContexts = probe;
}

export function registerProseCacheProbe(entries: CensusProbe, chars: CensusProbe): void {
  readProseCacheEntries = entries;
  readProseCacheChars = chars;
}

export function resourceCensusSnapshot(): ResourceCensusSnapshot {
  return {
    liveTerminals,
    liveWebglContexts: readWebglContexts(),
    proseCacheEntries: readProseCacheEntries(),
    proseCacheChars: readProseCacheChars(),
    liveTimers,
  };
}

/** 测试用:把自持计数归零。不动注入的 probe。 */
export function resetResourceCensus(): void {
  liveTerminals = 0;
  liveTimers = 0;
}

/**
 * 把快照读取器挂到 window 上,方便在 devtools 控制台直接敲 `__aeroricCensus()`。
 *
 * 只在 dev 下调用。devtools 本身是 opt-in feature、只在 `tauri dev` 启用(见 Cargo.toml
 * 那段注释),所以 release 里既没有控制台也没有这个入口 —— 两道门。
 *
 * 采样节奏建议:空转时每 5 分钟一次,连续两小时。手动操作(开关面板、切标签)前后各读一次,
 * 用来定位「哪条操作的 cleanup 漏了」。
 */
export function installResourceCensusProbe(): void {
  if (!import.meta.env.DEV) return;
  (globalThis as { __aeroricCensus?: () => ResourceCensusSnapshot }).__aeroricCensus =
    resourceCensusSnapshot;
}
