/* 云盘同步面板:一轮的结果 + 逐个冲突做决定。
 *
 * 这是 P8e 里唯一能把 `Resolution` 造出来的地方。在它之前,冲突只有 vault 级的一律处理
 * (`ConflictStrategy`)—— 对一个文件选「用对面那份」会把这一轮**所有**冲突文件的本地改动
 * 一起丢掉。所以这个面板不是锦上添花,它是那条路的唯一安全出口。
 *
 * 四件必须做对的事:
 *
 * 1. **三档状态要分开显示。** 「还没决定」/「已决定,等下一轮」/「决定过但文件之后又变了」。
 *    第三档混进前两档任何一档都会骗人:混进「已决定」用户会等一个永远不执行的决定,混进
 *    「还没决定」他会以为上次没点上。判定口径在 `conflictRowState` 里,和后端一致。
 * 2. **决定不当场生效。** 它只是入库,下一轮同步才执行。每一行都写着这句话。
 * 3. **fork 的落点要让用户看见再确认。** 点「两份都留」先把路径摊开(默认 `x.conflict.md`),
 *    确认才提交 —— 那是要新建一个文件,不该点一下就发生。
 * 4. **挂起和失败也要列。** 「文件太大不同步」这类结果只在 outcomes 里,不列出来的话用户会
 *    一直等一个永远不会同步的文件。
 */

import { useMemo, useState, type CSSProperties } from "react";
import { AlertTriangle, RefreshCw, X } from "lucide-react";

import {
  noteSheetHeaderStyle,
  noteSheetIconButtonStyle,
  noteSheetOverlayStyle,
  useNoteSheetDismiss,
} from "./noteSheetChrome";

import {
  conflictRowState,
  defaultForkPath,
  pendingConflicts,
  resolutionLabelKey,
  syncPendingKey,
  syncReasonKey,
  type StoredResolution,
  type SyncActionOutcome,
  type SyncOutcomeStatus,
  type SyncPlannedAction,
  type SyncReport,
  type SyncResolution,
} from "./noteSync";
import type { SyncRemoteView } from "./useNoteSync";

export type NoteSyncSheetProps = {
  remotes: SyncRemoteView[];
  activeId: string | null;
  report: SyncReport | null;
  stale: boolean;
  decided: StoredResolution[];
  running: boolean;
  error: string | null;
  onSelectRemote: (remoteId: string) => void;
  onToggleAuto: (enabled: boolean) => void;
  onSync: () => void;
  onDecide: (path: string, resolution: SyncResolution) => void;
  onUndecide: (path: string) => void;
  onClose: () => void;
  t: (key: string, vars?: Record<string, string>) => string;
};

const headerStyle = noteSheetHeaderStyle(6);

const hintStyle: CSSProperties = {
  padding: 10,
  color: "var(--text-hint)",
  fontSize: 11.5,
};

const sectionTitleStyle: CSSProperties = {
  padding: "3px 6px",
  color: "var(--text-muted)",
  fontSize: 10.5,
  textTransform: "uppercase",
  letterSpacing: 0.4,
};

const rowStyle: CSSProperties = {
  padding: "5px 6px",
  borderBottom: "1px solid var(--border-dim)",
  fontSize: 11.5,
};

const pathStyle: CSSProperties = {
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-primary)",
};

const choiceStyle: CSSProperties = {
  height: 20,
  padding: "0 7px",
  border: "1px solid var(--border-medium)",
  borderRadius: 4,
  background: "var(--bg-card)",
  color: "var(--text-primary)",
  cursor: "pointer",
  fontSize: 10.5,
  whiteSpace: "nowrap",
};

const inputStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  height: 20,
  padding: "0 6px",
  border: "1px solid var(--border)",
  borderRadius: 4,
  background: "var(--bg-input, transparent)",
  color: "var(--text-primary)",
  fontSize: 11,
};

const KEEP: SyncResolution[] = [{ kind: "keepLocal" }, { kind: "keepRemote" }];

/** 没落定的那些结果 —— 只有这两档有话要显示(`done` 既没有 `detail` 也没有 `error`)。 */
type UnsettledOutcome = SyncActionOutcome & {
  status: Exclude<SyncOutcomeStatus, { kind: "done" }>;
};

/**
 * 谓词而不是直接 `filter(o => o.status.kind !== "done")`:后者留下的类型仍然是完整的联合,
 * 于是渲染那里读 `detail` 编译不过,只能靠断言把「我筛过了」这件事重说一遍。写成谓词的话那个
 * 不变式跟着类型走。
 */
function isUnsettled(outcome: SyncActionOutcome): outcome is UnsettledOutcome {
  return outcome.status.kind !== "done";
}

/** 一行冲突。 */
function ConflictRow({
  action,
  decided,
  onDecide,
  onUndecide,
  t,
}: {
  action: SyncPlannedAction;
  decided: StoredResolution[];
  onDecide: (path: string, resolution: SyncResolution) => void;
  onUndecide: (path: string) => void;
  t: NoteSyncSheetProps["t"];
}) {
  const state = conflictRowState(action, decided);
  /* fork 的路径草稿。`null` = 还没摊开。摊开的默认值是 `x.conflict.md`(拼在扩展名之前,
     否则那份内容不会被当成笔记 —— 见 `defaultForkPath`)。 */
  const [fork, setFork] = useState<string | null>(null);

  const choiceLabel = (resolution: SyncResolution) => t(resolutionLabelKey(resolution));

  return (
    <div style={rowStyle} data-testid="note-sync-conflict-row">
      <div style={pathStyle} title={action.path}>
        {action.path}
      </div>
      <div style={{ marginTop: 1, color: "var(--text-hint)", fontSize: 10.5 }}>
        {t(syncReasonKey(action.reason))}
      </div>

      {/* 已决定 / 已过期那两档:显示决定 + 一个撤回,不再摆三个按钮 —— 那会让人以为
          上次没点上。过期那一档要重新选,所以按钮还得给。 */}
      {state.kind !== "undecided" ? (
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            marginTop: 3,
            flexWrap: "wrap",
            color: state.kind === "stale" ? "var(--warning, #eab308)" : "var(--text-muted)",
            fontSize: 10.5,
          }}
        >
          <span>
            {t(state.kind === "stale" ? "notebook.sync.stale" : "notebook.sync.decided", {
              choice: choiceLabel(state.resolution),
            })}
          </span>
          <button
            type="button"
            onClick={() => onUndecide(action.path)}
            style={{ ...choiceStyle, height: 18 }}
          >
            {t("notebook.sync.undo")}
          </button>
        </div>
      ) : null}

      {state.kind !== "decided" ? (
        <div style={{ display: "flex", gap: 5, marginTop: 4, flexWrap: "wrap" }}>
          {KEEP.map((resolution) => (
            <button
              key={resolution.kind}
              type="button"
              onClick={() => onDecide(action.path, resolution)}
              style={choiceStyle}
            >
              {choiceLabel(resolution)}
            </button>
          ))}
          <button
            type="button"
            aria-expanded={fork !== null}
            onClick={() =>
              setFork((current) => (current === null ? defaultForkPath(action.path) : null))
            }
            style={choiceStyle}
          >
            {t("notebook.sync.fork")}
          </button>
        </div>
      ) : null}

      {/* fork 要新建一个文件,所以先把落点摊开让用户看见,确认才提交。 */}
      {fork !== null && state.kind !== "decided" ? (
        <div style={{ display: "flex", alignItems: "center", gap: 5, marginTop: 4 }}>
          <input
            value={fork}
            onChange={(event) => setFork(event.target.value)}
            aria-label={t("notebook.sync.forkPath")}
            placeholder={t("notebook.sync.forkPath")}
            style={inputStyle}
          />
          <button
            type="button"
            /* 空路径后端会拒(`Fork path cannot be empty`),但那要等到提交之后才报。
               这里直接禁掉 —— 一个点了没反应的按钮比一句报错更让人困惑。

               `disabled` 是**唯一**那道闸门。回调里原先还有一句 `if (length === 0) return`,
               两道一起在的时候把任何一道拿掉测试都照样绿(disabled 挡住了点击,回调根本不
               会跑),于是"哪一道在起作用"这件事没有任何测试说得清。收成一道。 */
            disabled={fork.trim().length === 0}
            onClick={() => {
              onDecide(action.path, { kind: "fork", forkPath: fork.trim() });
              setFork(null);
            }}
            style={{ ...choiceStyle, opacity: fork.trim().length === 0 ? 0.45 : 1 }}
          >
            {t("notebook.sync.forkConfirm")}
          </button>
        </div>
      ) : null}
    </div>
  );
}

export function NoteSyncSheet({
  remotes,
  activeId,
  report,
  stale,
  decided,
  running,
  error,
  onSelectRemote,
  onToggleAuto,
  onSync,
  onDecide,
  onUndecide,
  onClose,
  t,
}: NoteSyncSheetProps) {
  const { closeRef, overlayProps } = useNoteSheetDismiss(t("notebook.sync.title"), onClose);
  const active = remotes.find((item) => item.target.id === activeId) ?? null;

  const conflicts = useMemo(() => pendingConflicts(report), [report]);

  /* 挂起和失败里**不是冲突**的那些。冲突已经在上面逐条列了,再列一遍是重复;而
     「文件太大不同步」这类只在 outcomes 里,不列的话用户会一直等它。 */
  const others = useMemo(() => {
    if (!report) return [];
    const conflictPaths = new Set(conflicts.map((item) => item.path));
    /* 两步走而不是一个 `&&`:谓词只有在它是**整个**回调时才收窄,和别的条件并在一起
       就退回完整联合,那时候读 `detail` 又得靠断言。 */
    return report.outcomes
      .filter((outcome) => !conflictPaths.has(outcome.path))
      .filter(isUnsettled);
  }, [conflicts, report]);

  const summary = report?.plan.summary ?? null;

  return (
    <div style={noteSheetOverlayStyle} {...overlayProps}>
      <div style={headerStyle}>
        <AlertTriangle size={12} aria-hidden />
        <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>
          {t("notebook.sync.title")}
        </span>

        {/* 多个云盘远端时才给选择器。只有一个的时候它是纯噪声。 */}
        {remotes.length > 1 ? (
          <select
            value={activeId ?? ""}
            onChange={(event) => onSelectRemote(event.target.value)}
            aria-label={t("notebook.sync.remote")}
            style={{
              height: 20,
              maxWidth: 160,
              border: "1px solid var(--border)",
              borderRadius: 4,
              background: "var(--bg-input, transparent)",
              color: "var(--text-primary)",
              fontSize: 11,
            }}
          >
            {remotes.map((item) => (
              <option key={item.target.id} value={item.target.id}>
                {item.target.root || item.target.id}
              </option>
            ))}
          </select>
        ) : null}

        <button
          type="button"
          aria-label={t("notebook.sync.syncNow")}
          title={t("notebook.sync.syncNow")}
          onClick={onSync}
          disabled={running || !active}
          style={{
            ...noteSheetIconButtonStyle,
            marginLeft: "auto",
            cursor: running ? "progress" : "pointer",
            opacity: running || !active ? 0.45 : 1,
          }}
        >
          <RefreshCw size={12} aria-hidden />
        </button>
        <button
          ref={closeRef}
          type="button"
          aria-label={t("notebook.sync.close")}
          onClick={onClose}
          style={noteSheetIconButtonStyle}
        >
          <X size={13} aria-hidden />
        </button>
      </div>

      {/* 自动同步的开关。放在头下面而不是状态栏里 —— 状态栏那 22px 放不下一句说明,而
          「停笔一会儿之后同步」这件事不说清楚,用户不知道自己在开什么。 */}
      {active ? (
        <div
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: 6,
            padding: "5px 6px",
            borderBottom: "1px solid var(--border-dim)",
          }}
        >
          <label
            style={{
              display: "flex",
              alignItems: "center",
              gap: 4,
              color: "var(--text-primary)",
              fontSize: 11.5,
              whiteSpace: "nowrap",
              cursor: "pointer",
            }}
          >
            <input
              type="checkbox"
              checked={active.target.autoSync}
              onChange={(event) => onToggleAuto(event.target.checked)}
            />
            {t("notebook.sync.auto")}
          </label>
          <span style={{ color: "var(--text-hint)", fontSize: 10.5 }}>
            {t("notebook.sync.autoHint")}
          </span>
        </div>
      ) : null}

      <div style={{ flex: 1, minHeight: 0, overflowY: "auto" }} data-testid="note-sync-body">
        {!active ? (
          <div style={hintStyle}>{t("notebook.sync.noRemote")}</div>
        ) : (
          <>
            {error ? (
              <div style={{ ...hintStyle, color: "var(--danger, #ff453a)" }}>{error}</div>
            ) : null}

            {/* 报告过期。这一条要显眼:它意味着下面那张清单可能已经不是现在的样子了。 */}
            {stale ? (
              <div
                role="status"
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                  padding: "5px 6px",
                  background: "var(--bg-card)",
                  borderBottom: "1px solid var(--border-dim)",
                  color: "var(--warning, #eab308)",
                  fontSize: 11,
                }}
              >
                <span style={{ flex: 1, minWidth: 0 }}>{t("notebook.sync.staleReport")}</span>
                <button type="button" onClick={onSync} disabled={running} style={choiceStyle}>
                  {t("notebook.sync.recheck")}
                </button>
              </div>
            ) : null}

            {summary ? (
              <div style={{ ...hintStyle, paddingBottom: 4 }}>
                {t("notebook.sync.summary", {
                  upload: String(summary.upload),
                  download: String(summary.download),
                  deleteRemote: String(summary.deleteRemote),
                  deleteLocal: String(summary.deleteLocal),
                  conflict: String(summary.conflict),
                })}
              </div>
            ) : null}

            {!report ? (
              <div style={hintStyle}>{t("notebook.sync.noReport")}</div>
            ) : conflicts.length === 0 && others.length === 0 ? (
              <div style={hintStyle}>{t("notebook.sync.conflictsEmpty")}</div>
            ) : null}

            {conflicts.length > 0 ? (
              <>
                <div style={sectionTitleStyle}>{t("notebook.sync.conflicts")}</div>
                <div style={{ ...hintStyle, paddingTop: 0, paddingBottom: 6 }}>
                  {t("notebook.sync.conflictHint")}
                </div>
                {conflicts.map((action) => (
                  <ConflictRow
                    key={action.path}
                    action={action}
                    decided={decided}
                    onDecide={onDecide}
                    onUndecide={onUndecide}
                    t={t}
                  />
                ))}
              </>
            ) : null}

            {others.length > 0 ? (
              <>
                <div style={sectionTitleStyle}>{t("notebook.sync.otherOutcomes")}</div>
                {others.map((outcome) => (
                  <div key={`${outcome.path}:${outcome.status.kind}`} style={rowStyle}>
                    <div style={pathStyle} title={outcome.path}>
                      {outcome.path}
                    </div>
                    <div
                      style={{
                        marginTop: 1,
                        color:
                          outcome.status.kind === "failed"
                            ? "var(--danger, #ff453a)"
                            : "var(--text-hint)",
                        fontSize: 10.5,
                      }}
                    >
                      {outcome.status.kind === "failed"
                        ? outcome.status.error
                        : t(syncPendingKey(outcome.status.detail))}
                    </div>
                  </div>
                ))}
              </>
            ) : null}
          </>
        )}
      </div>
    </div>
  );
}
