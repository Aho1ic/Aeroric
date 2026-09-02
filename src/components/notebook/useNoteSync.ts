/* 云盘同步面板的状态与动作。
 *
 * 抽成 hook 而不是写进 NotebookPanel(那个文件已经 3800 行)。这里有四件必须一处做对的事:
 *
 * 1. **状态是两路数据拼的。** 目标列表(含 `autoSync`、上次同步时间)在库里,调度状态(退避、
 *    下一轮还有多久)在进程内存里。后端故意分成两条命令,因为状态栏这一路要高频问而不该每次
 *    开库。合并的键是 `remoteId`。
 *
 * 2. **倒计时要本地推。** `nextRunInMs` 是取的那一刻的快照;照着它原样显示的话,数字会卡在
 *    一个值上直到下次轮询。这里记下取到的时刻,渲染前减掉已经过去的时间。
 *
 * 3. **守护线程跑完一轮之后,手里这份报告就过期了,但不能自动去重算。** 重算要调
 *    `notebook_sync_run`,那会写文件 —— 面板开着这件事本身不该让后台的一轮变成两轮写。所以
 *    收到事件只重查三样只读的东西(目标、调度状态、存着的决定),另外把报告标成 `stale`,让
 *    用户自己按刷新。
 *
 * 4. **`ask` 从不碰冲突文件。** 刷新走的是 `runSync(..., "ask")`:非冲突动作照做(那也正是
 *    守护线程刚做过的),冲突一律挂起。所以「刷新」在任何时候按都不会覆盖谁的内容。
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import {
  clearConflictResolution,
  listConflictResolutions,
  listSyncRemotes,
  resolveConflict,
  runSync,
  setSyncAuto,
  syncStatus,
  SYNC_EVENT,
  type StoredResolution,
  type SyncRemoteStatus,
  type SyncRemoteTarget,
  type SyncReport,
  type SyncResolution,
} from "./noteSync";

/** 状态轮询间隔。倒计时本身在本地推,所以这里不用一秒一次。 */
const POLL_MS = 2000;

/** 目标 + 调度状态合到一起的一行。 */
export type SyncRemoteView = {
  target: SyncRemoteTarget;
  /** 还没查到调度状态时是 `null`(比如刚打开面板那一瞬间)。 */
  status: SyncRemoteStatus | null;
};

export type NoteSyncState = {
  remotes: SyncRemoteView[];
  /** 当前看的那个远端的 id。`null` = 没有云盘远端。 */
  activeId: string | null;
  active: SyncRemoteView | null;
  /** 最近一轮的报告。守护线程自己跑的那些轮不产报告,只有这一路的才有。 */
  report: SyncReport | null;
  /**
   * 拿到这份报告之后,守护线程又跑过一轮 —— 里面的冲突清单可能已经不是现在的样子了。
   *
   * 这一档必须显示出来。守护线程执行掉用户的决定之后会把决定从库里删掉(见 Rust 侧 `engine`
   * 的 clear-on-Done),于是那一行在这份旧报告上会显示成「还没决定」,而它其实已经处理完了。
   */
  stale: boolean;
  decided: StoredResolution[];
  /** 正在跑一轮。 */
  running: boolean;
  error: string | null;
  /**
   * 把 `nextRunInMs` 拿到手的时刻(`Date.now()`)。渲染倒计时要减掉从那时到现在的时间,
   * 否则数字会一直卡在快照上。
   */
  statusAt: number;
};

export type NoteSyncApi = NoteSyncState & {
  selectRemote: (remoteId: string) => void;
  /** 跑一轮。`strategy` 省略时按 `ask` —— 冲突挂起,不动文件。 */
  sync: (strategy?: "ask" | "local" | "remote") => void;
  toggleAuto: (enabled: boolean) => void;
  decide: (path: string, resolution: SyncResolution) => void;
  undecide: (path: string) => void;
  refresh: () => void;
};

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * `enabled` 为真时开始查。关着的时候一次请求都不发 —— 绝大多数时候面板没打开,而这里有
 * 一路是两秒一次的轮询。
 */
export function useNoteSync(vault: string | null, enabled: boolean): NoteSyncApi {
  const [targets, setTargets] = useState<SyncRemoteTarget[]>([]);
  const [statuses, setStatuses] = useState<SyncRemoteStatus[]>([]);
  const [statusAt, setStatusAt] = useState(() => Date.now());
  const [activeId, setActiveId] = useState<string | null>(null);
  const [report, setReport] = useState<SyncReport | null>(null);
  const [decided, setDecided] = useState<StoredResolution[]>([]);
  const [running, setRunning] = useState(false);
  const [stale, setStale] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reloadToken, setReloadToken] = useState(0);

  /* 只留云盘那些。`git` 走 `notebook_git_*` 那一套命令,`p2p` 还没接 —— 混在一起的话
     点「立即同步」会拿一个 `notebook_sync_run` 处理不了的目标去调。 */
  const cloud = useMemo(() => targets.filter((item) => item.kind === "cloud"), [targets]);

  const remotes = useMemo<SyncRemoteView[]>(
    () =>
      cloud.map((target) => ({
        target,
        status: statuses.find((s) => s.remoteId === target.id) ?? null,
      })),
    [cloud, statuses],
  );

  /* 选中的那个。选中的 id 不在列表里(远端被解绑了)时落回第一个,而不是显示空白。 */
  const active = useMemo(
    () => remotes.find((item) => item.target.id === activeId) ?? remotes[0] ?? null,
    [activeId, remotes],
  );

  /* `active` 已经做了落回,但 `activeId` 本身还指着一个不存在的 id —— 不同步回去的话,
     之后每次比较都要靠那个落回,而「当前选中」这件事在两处有两个答案。 */
  useEffect(() => {
    const resolved = active?.target.id ?? null;
    setActiveId((current) => (current === resolved ? current : resolved));
  }, [active]);

  /* 目标列表。开库读表,所以只在打开时和显式刷新时读,不进轮询。 */
  useEffect(() => {
    if (!vault || !enabled) return;
    let cancelled = false;
    void (async () => {
      try {
        const next = await listSyncRemotes(vault);
        if (!cancelled) setTargets(next);
      } catch (failure: unknown) {
        if (!cancelled) setError(messageOf(failure));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [enabled, reloadToken, vault]);

  /* 调度状态的轮询。它不开库(见 `daemon::status_for`),所以两秒一次是可以的。
     进依赖的是一个**串**而不是数组:数组每次渲染都是新的,会让这个 effect 每次都重挂,
     于是每次渲染都发一次请求。

     串用 JSON 而不是拿某个分隔符拼 —— 远端 id 是后端给的不透明串,挑任何一个字符当分隔符
     都得先论证它不会出现在 id 里。JSON 不用论证。 */
  const idsKey = JSON.stringify(cloud.map((item) => item.id));
  useEffect(() => {
    if (!vault || !enabled) return;
    const ids: string[] = JSON.parse(idsKey);
    if (ids.length === 0) {
      setStatuses([]);
      return;
    }
    let cancelled = false;
    const tick = async () => {
      try {
        const next = await syncStatus(vault, ids);
        if (cancelled) return;
        setStatuses(next);
        // 和状态同一刻记下时间戳。分开记的话倒计时的基准会偏掉一个请求的往返时间。
        setStatusAt(Date.now());
      } catch (failure: unknown) {
        /* 轮询失败不写 `error`:那个位置是给用户动作的失败留的,被一次瞬时的查询失败
           占住之后,真正的失败原因会被后面的轮询覆盖掉。 */
        if (!cancelled) console.warn("notebook sync status poll failed", failure);
      }
    };
    void tick();
    const timer = window.setInterval(() => void tick(), POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [enabled, idsKey, vault]);

  /* 存着的决定。面板重开之后要靠它把「已决定,等下一轮」那些行恢复出来。 */
  const remoteId = active?.target.id ?? null;
  useEffect(() => {
    if (!vault || !enabled || !remoteId) {
      setDecided([]);
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const next = await listConflictResolutions(vault, remoteId);
        if (!cancelled) setDecided(next);
      } catch (failure: unknown) {
        if (!cancelled) setError(messageOf(failure));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [enabled, reloadToken, remoteId, vault]);

  /* 换远端时把上一轮的报告丢掉。那份报告讲的是另一个远端的冲突,留着会让用户对着 A 的
     冲突清单给 B 做决定。`stale` 跟着清 —— 它讲的是刚被丢掉的那份报告。 */
  useEffect(() => {
    setReport(null);
    setStale(false);
  }, [remoteId]);

  const refresh = useCallback(() => setReloadToken((current) => current + 1), []);

  const sync = useCallback(
    (strategy?: "ask" | "local" | "remote") => {
      if (!vault || !remoteId) return;
      setRunning(true);
      setError(null);
      void (async () => {
        try {
          const next = await runSync(vault, remoteId, strategy ?? "ask");
          setReport(next);
          // 新报告落地,「可能过期」这件事就说完了。
          setStale(false);
        } catch (failure: unknown) {
          setError(messageOf(failure));
        } finally {
          /* 无论成败都重读一次:成功时决定可能被执行掉了(后端会删),失败时目标上的
             `lastSyncAt` 也可能已经动过。 */
          setRunning(false);
          refresh();
        }
      })();
    },
    [refresh, remoteId, vault],
  );

  /* 守护线程跑完一轮。载荷里没有 vault(见 `SYNC_EVENT` 的注释),所以收到就重查自己这个。
     **只重查,不重算。** 重算要调 `notebook_sync_run`,那会写文件 —— 面板开着不该让后台的
     一轮变成两轮写。报告标成 `stale`,由用户按刷新(理由见模块头第 3 条)。

     `hasReport` 进 ref 而不是进依赖:它一变这个 effect 就重挂,而重挂意味着退订再订阅,
     那个窗口里的事件会漏掉。 */
  const hasReportRef = useRef(false);
  hasReportRef.current = report !== null;
  useEffect(() => {
    if (!vault || !enabled) return;
    const pending = listen(SYNC_EVENT, () => {
      // 没有报告时不必标 —— 那时候界面上没有任何「这一轮的结果」可过期。
      if (hasReportRef.current) setStale(true);
      setReloadToken((current) => current + 1);
    });
    /* 无条件退订:`listen` 返回 promise,卸载时它可能还没落地 —— 挂在 `.then` 上而不是
       "落地了才退订",否则快开快关会留下一个收不掉的监听。 */
    return () => {
      void pending.then((unlisten) => unlisten());
    };
  }, [enabled, vault]);

  const toggleAuto = useCallback(
    (value: boolean) => {
      if (!vault || !remoteId) return;
      setError(null);
      void (async () => {
        try {
          await setSyncAuto(vault, remoteId, value);
        } catch (failure: unknown) {
          setError(messageOf(failure));
        } finally {
          // 重读目标列表 —— `autoSync` 存在库里,乐观更新会在失败时留下一个假的开关状态。
          refresh();
        }
      })();
    },
    [refresh, remoteId, vault],
  );

  /**
   * 记下一条决定。
   *
   * 那两个 hash 从**本轮报告**的冲突动作上原样取。取不到就不提交 —— 后端拿空串当「那一侧
   * 当时不存在」,凭空传空串会让一条本该作废的决定通过闸门。
   */
  const decide = useCallback(
    (path: string, resolution: SyncResolution) => {
      if (!vault || !remoteId) return;
      const action = report?.plan.actions.find((item) => item.path === path);
      if (!action || action.action.kind !== "conflict") return;
      const { localHash, remoteHash } = action.action;
      setError(null);
      void (async () => {
        try {
          await resolveConflict({ vault, remoteId, path, resolution, localHash, remoteHash });
          /* 本地也记一份,不等重查 —— 点完到重查落地之间那一段,行上会一直显示「还没决定」,
             用户会以为没点上然后再点一次。 */
          setDecided((current) => [
            ...current.filter((item) => item.path !== path),
            { path, resolution, localHash, remoteHash, decidedAt: Date.now() },
          ]);
        } catch (failure: unknown) {
          // 失败要重查:乐观那一份得被库里的真实情况顶掉。
          setError(messageOf(failure));
          refresh();
        }
      })();
    },
    [refresh, remoteId, report, vault],
  );

  const undecide = useCallback(
    (path: string) => {
      if (!vault || !remoteId) return;
      setError(null);
      void (async () => {
        try {
          await clearConflictResolution(vault, remoteId, path);
          setDecided((current) => current.filter((item) => item.path !== path));
        } catch (failure: unknown) {
          setError(messageOf(failure));
          refresh();
        }
      })();
    },
    [refresh, remoteId, vault],
  );

  return {
    remotes,
    activeId,
    active,
    report,
    stale,
    decided,
    running,
    error,
    statusAt,
    selectRemote: setActiveId,
    sync,
    toggleAuto,
    decide,
    undecide,
    refresh,
  };
}
