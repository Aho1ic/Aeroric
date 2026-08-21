/**
 * The DeepSeek Harness trajectory panel.
 *
 * Mounted as the last child of the terminal's own box and drawn at `inset: 0`,
 * so it covers exactly the terminal and nothing else: the session it describes
 * stays framed by the task it belongs to instead of floating over the whole app.
 *
 * Everything it reads comes from `DshTrajectoryHost`, because the trigger that
 * opens it lives above the terminal and cannot reach it by props.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  CheckCircle2,
  Clock3,
  FolderOpen,
  GitBranch,
  Loader2,
  Search,
  Send,
  ThumbsDown,
  ThumbsUp,
  X,
} from "lucide-react";
import { useI18n } from "../i18n";
import type { DshSessionStatsProjection, DshTokenUsageProjection } from "../types";
import type { DshStats, DshTimelineRecord } from "../dshSessionFeatures";
import { isDshSessionMissingError } from "../dshSessionFeatures";
import { dshMentionVocabulary } from "../dshDeliverables";
import { deriveDshLedger, dshLedgerCategory, dshLedgerRows } from "../dshTrajectoryLedger";
import type { DshLedgerCategory } from "../dshTrajectoryLedger";
import { dshTimelineFocus, deriveDshTimeline } from "../dshTrajectoryTimeline";
import type { DshTimelineMode, DshTimelineRange } from "../dshTrajectoryTimeline";
import type { useDshSessionFeatures } from "../hooks/useDshSessionFeatures";
import type { DshImageLoader } from "../hooks/useDshImageLoader";
import { DshTrajectoryDetails } from "./DshTrajectoryDetails";
import {
  DshTrajectoryLedger,
  dshLedgerFoldAll,
  dshLedgerGroupAnchor,
  dshLedgerReveal,
  dshLedgerToggle,
  type DshLedgerFold,
} from "./DshTrajectoryLedger";
import { DshTrajectoryTimeline } from "./DshTrajectoryTimeline";
import { useDshTrajectory } from "./DshTrajectoryHost";

function finite(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function statsProjection(value: unknown): DshSessionStatsProjection | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const item = value as Partial<DshSessionStatsProjection>;
  return finite(item.turns) &&
    finite(item.steps) &&
    finite(item.llmMs) &&
    finite(item.toolMs) &&
    finite(item.ttftMs) &&
    finite(item.ttftSteps) &&
    finite(item.decodeMs) &&
    finite(item.decodeTokens)
    ? (item as DshSessionStatsProjection)
    : undefined;
}

function tokenProjection(value: unknown): DshTokenUsageProjection | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const item = value as Partial<DshTokenUsageProjection>;
  return finite(item.uncachedInputTokens) &&
    finite(item.outputTokens) &&
    finite(item.cacheReadTokens) &&
    finite(item.cacheWriteTokens)
    ? (item as DshTokenUsageProjection)
    : undefined;
}

function formatDuration(ms: number): string {
  if (ms < 1_000) return `${Math.round(ms)} ms`;
  const seconds = ms / 1_000;
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 1 : 0)} s`;
  return `${Math.floor(seconds / 60)}m ${Math.round(seconds % 60)}s`;
}

function formatTokens(value: number): string {
  if (value < 1_000) return String(Math.round(value));
  if (value < 1_000_000) return `${(value / 1_000).toFixed(value < 10_000 ? 1 : 0)}K`;
  return `${(value / 1_000_000).toFixed(1)}M`;
}

function ProjectionMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="dsh-insight-metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function StatsPanel({ stats, tokens }: { stats: DshStats; tokens?: DshTokenUsageProjection }) {
  const { t } = useI18n();
  const input = tokens
    ? tokens.uncachedInputTokens + tokens.cacheReadTokens + tokens.cacheWriteTokens
    : stats.inputTokens + stats.cacheReadTokens + stats.cacheWriteTokens;
  const output = tokens?.outputTokens ?? stats.outputTokens;
  const cacheRead = tokens?.cacheReadTokens ?? stats.cacheReadTokens;
  const cacheTotal = input;
  const cachePercent = cacheTotal > 0 ? Math.round((cacheRead / cacheTotal) * 100) : 0;
  const throughput = stats.decodeMs > 0 ? stats.decodeTokens / (stats.decodeMs / 1_000) : 0;
  return (
    <div className="dsh-insight-metrics">
      <ProjectionMetric label={t("dsh.insights.turns")} value={String(stats.turns)} />
      <ProjectionMetric label={t("dsh.insights.steps")} value={String(stats.steps)} />
      <ProjectionMetric label={t("dsh.insights.llmTime")} value={formatDuration(stats.llmMs)} />
      <ProjectionMetric label={t("dsh.insights.toolTime")} value={formatDuration(stats.toolMs)} />
      <ProjectionMetric
        label={t("dsh.insights.ttft")}
        value={stats.ttftSteps ? formatDuration(stats.ttftMs / stats.ttftSteps) : "-"}
      />
      <ProjectionMetric
        label={t("dsh.insights.throughput")}
        value={throughput ? `${throughput.toFixed(1)} tok/s` : "-"}
      />
      <ProjectionMetric label={t("dsh.insights.inputTokens")} value={formatTokens(input)} />
      <ProjectionMetric label={t("dsh.insights.outputTokens")} value={formatTokens(output)} />
      <ProjectionMetric label={t("dsh.insights.cacheHit")} value={`${cachePercent}%`} />
    </div>
  );
}

function TrajectoryPanel({
  entries,
  produced,
  timeline,
  hasMore,
  loadingOlder,
  loadOlder,
  loadImage,
}: {
  entries: ReturnType<typeof useDshSessionFeatures>["features"]["trajectory"];
  produced: ReturnType<typeof useDshSessionFeatures>["features"]["producedFiles"];
  timeline: readonly DshTimelineRecord[];
  hasMore: boolean;
  loadingOlder: boolean;
  loadOlder: () => Promise<void>;
  loadImage: DshImageLoader;
}) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<"all" | DshLedgerCategory>("all");
  const [mode, setMode] = useState<DshTimelineMode>({ actualDuration: false, actualTime: false });
  const [focus, setFocus] = useState<DshTimelineRange | null>(null);
  const [selectedSeq, setSelectedSeq] = useState<number | null>(null);
  const [turns, setTurns] = useState<DshLedgerFold>(() => dshLedgerFoldAll(false));
  const [calls, setCalls] = useState<DshLedgerFold>(() => dshLedgerFoldAll(false));
  const groups = useMemo(() => deriveDshLedger(entries), [entries]);
  const rows = useMemo(() => dshLedgerRows(groups), [groups]);
  // The overview's selection is an interval of the projection, so the rows it
  // covers are resolved in the same projection the user dragged in.
  const focused = useMemo(
    () => (focus === null ? null : dshTimelineFocus(deriveDshTimeline(timeline, mode), focus)),
    [focus, mode, timeline],
  );
  // A selection is an interval of one projection, so re-scaling the domain would
  // leave it pointing somewhere arbitrary: switching projections drops it.
  const changeMode = (next: DshTimelineMode) => {
    setMode(next);
    setFocus(null);
  };
  // `null` rather than a set of every seq, so an unfiltered ledger costs nothing
  // to draw. A row matches on any of the events folded into it, which is what
  // keeps a tool result findable now that it is no longer a row of its own.
  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (needle === "" && category === "all" && focused === null) return null;
    const seqs = new Set<number>();
    for (const row of rows) {
      if (focused !== null && !row.seqs.some((seq) => focused.has(seq))) continue;
      if (category !== "all" && dshLedgerCategory(row.tag) !== category) continue;
      const haystack = `${row.entry.type} ${row.title} ${row.entry.detail ?? ""} ${row.result ?? ""}`;
      if (needle !== "" && !haystack.toLowerCase().includes(needle)) continue;
      seqs.add(row.seq);
    }
    return seqs as ReadonlySet<number>;
  }, [category, focused, query, rows]);
  // One vocabulary per assistant message, keyed by the seq it was delivered at,
  // so a message can only reference files produced before it and each prose
  // keeps a stable path list across renders.
  const mentionPaths = useMemo(() => {
    const paths = new Map<number, readonly string[]>();
    for (const entry of entries) {
      if (entry.type !== "assistant/message") continue;
      paths.set(entry.seq, dshMentionVocabulary(produced, entry.seq));
    }
    return paths;
  }, [entries, produced]);
  const selected = selectedSeq === null ? undefined : rows.find((row) => row.seq === selectedSeq);
  // A located row has to be on screen for the selection to read, so the folds
  // that would have hidden it give way to the click.
  const locate = (seq: number) => {
    const row = rows.find((candidate) => candidate.seq === seq);
    if (row === undefined) return;
    const group = groups.find((candidate) => candidate.rows.includes(row));
    if (group !== undefined)
      setTurns((current) => dshLedgerReveal(current, dshLedgerGroupAnchor(group)));
    if (row.parentSeq !== undefined) {
      const parentSeq = row.parentSeq;
      setCalls((current) => dshLedgerReveal(current, parentSeq));
    }
    setSelectedSeq(seq);
  };
  return (
    <div className="dsh-trajectory-work" data-detail={selected !== undefined}>
      <div className="dsh-trajectory-main">
        <DshTrajectoryTimeline
          records={timeline}
          mode={mode}
          onModeChange={changeMode}
          focus={focus}
          onFocusChange={setFocus}
          onLocate={locate}
          turnsCollapsed={turns.collapsed}
          onToggleTurns={() => setTurns((current) => dshLedgerFoldAll(!current.collapsed))}
          callsCollapsed={calls.collapsed}
          onToggleCalls={() => setCalls((current) => dshLedgerFoldAll(!current.collapsed))}
          // Search and the category filter ride in the timeline's own control
          // row: both are one-line controls that were costing a full row each.
          controls={
            <div className="dsh-insight-toolbar">
              <label className="dsh-insight-search">
                <Search size={13} aria-hidden="true" />
                <input
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  placeholder={t("dsh.insights.search")}
                />
              </label>
              <div
                className="dsh-insight-segments"
                role="group"
                aria-label={t("dsh.insights.eventFilter")}
              >
                {(["all", "message", "tool", "lifecycle", "system"] as const).map((value) => (
                  <button
                    key={value}
                    type="button"
                    data-active={category === value}
                    onClick={() => setCategory(value)}
                  >
                    {t(`dsh.insights.filter.${value}`)}
                  </button>
                ))}
              </div>
            </div>
          }
        />
        {hasMore && (
          <button
            type="button"
            className="dsh-insight-load"
            disabled={loadingOlder}
            onClick={() => void loadOlder()}
          >
            {loadingOlder && <Loader2 size={12} className="spin" />}
            {t("dsh.insights.loadEarlier")}
          </button>
        )}
        <DshTrajectoryLedger
          groups={groups}
          visible={visible}
          selectedSeq={selectedSeq}
          onSelect={setSelectedSeq}
          turns={turns}
          onToggleTurn={(anchor) => setTurns((current) => dshLedgerToggle(current, anchor))}
          calls={calls}
          onToggleCall={(anchor) => setCalls((current) => dshLedgerToggle(current, anchor))}
          loadImage={loadImage}
          mentionPaths={mentionPaths}
        />
      </div>
      {selected && (
        <DshTrajectoryDetails
          row={selected}
          rows={rows}
          onSelect={setSelectedSeq}
          onClose={() => setSelectedSeq(null)}
        />
      )}
    </div>
  );
}

function FilesPanel({
  files,
}: {
  files: ReturnType<typeof useDshSessionFeatures>["features"]["producedFiles"];
}) {
  const { t } = useI18n();
  const [busy, setBusy] = useState<string | null>(null);
  if (files.length === 0)
    return <div className="dsh-insight-empty">{t("dsh.insights.noFiles")}</div>;
  return (
    <div className="dsh-produced-files">
      {files.map((file) => (
        <button
          type="button"
          key={file.path}
          title={file.path}
          disabled={busy === file.path}
          onClick={() => {
            setBusy(file.path);
            void invoke("open_dsh_host_path", { path: file.path }).finally(() => setBusy(null));
          }}
        >
          {busy === file.path ? <Loader2 size={14} className="spin" /> : <FolderOpen size={14} />}
          <span>{file.path}</span>
          {file.turn !== undefined && <small>T{file.turn}</small>}
        </button>
      ))}
    </div>
  );
}

function WorkflowsPanel({
  workflows,
}: {
  workflows: ReturnType<typeof useDshSessionFeatures>["features"]["workflows"];
}) {
  const { t } = useI18n();
  if (workflows.length === 0)
    return <div className="dsh-insight-empty">{t("dsh.insights.noWorkflows")}</div>;
  return (
    <div className="dsh-workflow-list">
      {workflows.map((run) => (
        <section key={run.runId} data-status={run.status}>
          <header>
            <GitBranch size={14} />
            <strong>{run.name}</strong>
            <span>{t(`dsh.insights.status.${run.status}`)}</span>
          </header>
          {Object.entries(run.phases).map(([key, phase]) => (
            <div key={key} className="dsh-workflow-phase">
              <h4>{phase.phase || t("dsh.insights.defaultPhase")}</h4>
              {phase.members.map((member) => (
                <div key={member.seq} className="dsh-workflow-member" data-status={member.status}>
                  {member.status === "running" ? (
                    <Loader2 size={12} className="spin" />
                  ) : (
                    <CheckCircle2 size={12} />
                  )}
                  <span>{member.label}</span>
                  <code>{member.childId}</code>
                  <small>{t(`dsh.insights.status.${member.status}`)}</small>
                </div>
              ))}
            </div>
          ))}
        </section>
      ))}
    </div>
  );
}

function SchedulesPanel({
  schedules,
}: {
  schedules: ReturnType<typeof useDshSessionFeatures>["features"]["schedules"];
}) {
  const { t } = useI18n();
  if (schedules.length === 0)
    return <div className="dsh-insight-empty">{t("dsh.insights.noSchedules")}</div>;
  return (
    <div className="dsh-schedule-list">
      {schedules.map((schedule) => (
        <div key={schedule.id} data-state={schedule.state}>
          <Clock3 size={14} />
          <div>
            <strong>{schedule.prompt}</strong>
            <span>{new Date(schedule.scheduledAt).toLocaleString()}</span>
          </div>
          <code>
            {schedule.kind}
            {schedule.everySeconds ? ` · ${schedule.everySeconds}s` : ""}
          </code>
          <small>{t(`dsh.insights.schedule.${schedule.state}`)}</small>
        </div>
      ))}
    </div>
  );
}

function FeedbackPanel({ sessionId }: { sessionId: string }) {
  const { t } = useI18n();
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const submit = async () => {
    const normalized = value.trim();
    if (!normalized || busy) return;
    setBusy(true);
    setResult(null);
    try {
      const response = await invoke<{ text?: string }>("execute_dsh_command", {
        sessionId,
        line: `/feedback ${normalized}`,
      });
      setResult(response?.text ?? t("dsh.insights.feedbackRecorded"));
      setValue("");
    } catch (caught) {
      setResult(String(caught));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="dsh-feedback-panel">
      <div className="dsh-feedback-rating-hint">
        <ThumbsUp size={14} />
        <ThumbsDown size={14} />
        <span>{t("dsh.insights.messageFeedbackHint")}</span>
      </div>
      <textarea
        value={value}
        onChange={(event) => setValue(event.target.value)}
        placeholder={t("dsh.insights.feedbackPlaceholder")}
      />
      <button type="button" disabled={!value.trim() || busy} onClick={() => void submit()}>
        {busy ? <Loader2 size={13} className="spin" /> : <Send size={13} />}
        {t("dsh.insights.sendFeedback")}
      </button>
      {result && <pre>{result}</pre>}
    </div>
  );
}

export function DshTrajectoryOverlay() {
  const { t } = useI18n();
  // The view selector moved to the terminal header, so the tab it picked lives
  // in the host rather than here.
  const { sessionId, live, history, loadImage, open, setOpen, tab } = useDshTrajectory();
  const panelRef = useRef<HTMLElement>(null);

  // The panel covers the terminal without covering the app, so xterm keeps the
  // keyboard unless focus is moved: taking it here is what makes Escape close
  // the panel instead of reaching the agent as a keystroke.
  useEffect(() => {
    if (open) panelRef.current?.focus();
  }, [open]);

  const sessionStats = statsProjection(
    live?.projections?.sessionStats ?? history.projections.sessionStats,
  );
  const tokenUsage = tokenProjection(
    live?.projections?.tokenUsage ?? history.projections.tokenUsage,
  );
  const stats: DshStats = {
    ...history.features.stats,
    ...(sessionStats ?? {}),
    inputTokens: history.features.stats.inputTokens,
    outputTokens: history.features.stats.outputTokens,
    cacheReadTokens: history.features.stats.cacheReadTokens,
    cacheWriteTokens: history.features.stats.cacheWriteTokens,
  };
  if (!open) return null;
  return (
    <div className="dsh-trajectory-overlay">
      <div
        className="dsh-trajectory-scrim"
        role="presentation"
        onMouseDown={() => setOpen(false)}
      />
      <section
        ref={panelRef}
        className="dsh-trajectory-panel"
        role="dialog"
        // Not `aria-modal`: the panel is contained in the terminal's box, so the
        // rest of the app stays reachable behind it.
        aria-label={t("dsh.insights.title")}
        tabIndex={-1}
        onKeyDown={(event) => {
          if (event.key !== "Escape") return;
          event.stopPropagation();
          setOpen(false);
        }}
      >
        <header className="dsh-insights-header">
          <div>
            <Activity size={16} />
            <strong>{t(`dsh.insights.${tab === "trajectory" ? "title" : tab}`)}</strong>
            <code>{sessionId}</code>
          </div>
          <button
            type="button"
            title={t("common.close")}
            aria-label={t("common.close")}
            onClick={() => setOpen(false)}
          >
            <X size={16} />
          </button>
        </header>
        <div className="dsh-insights-body" data-tab={tab}>
          {history.loading ? (
            <div className="dsh-insight-empty">
              <Loader2 size={16} className="spin" />
              {t("session.loading")}
            </div>
          ) : (
            <>
              {tab === "trajectory" && (
                <TrajectoryPanel
                  entries={history.features.trajectory}
                  produced={history.features.producedFiles}
                  timeline={history.features.timeline}
                  hasMore={history.hasMore}
                  loadingOlder={history.loadingOlder}
                  loadOlder={history.loadOlder}
                  loadImage={loadImage}
                />
              )}
              {tab === "stats" && <StatsPanel stats={stats} tokens={tokenUsage} />}
              {tab === "files" && <FilesPanel files={history.features.producedFiles} />}
              {tab === "workflows" && <WorkflowsPanel workflows={history.features.workflows} />}
              {tab === "schedules" && <SchedulesPanel schedules={history.features.schedules} />}
              {tab === "feedback" && <FeedbackPanel sessionId={sessionId} />}
            </>
          )}
          {history.error && (
            <div className="dsh-insights-error">
              {isDshSessionMissingError(history.error)
                ? t("dsh.insights.sessionMissing")
                : history.error}
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
