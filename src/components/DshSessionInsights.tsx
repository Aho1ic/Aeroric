import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  BarChart3,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Clock3,
  FileOutput,
  FolderOpen,
  GitBranch,
  Loader2,
  MessageSquareText,
  Search,
  Send,
  ThumbsDown,
  ThumbsUp,
  X,
} from "lucide-react";
import { useI18n } from "../i18n";
import type {
  DshLiveSessionState,
  DshSessionStatsProjection,
  DshTokenUsageProjection,
} from "../types";
import type { DshSessionEvent, DshStats } from "../dshSessionFeatures";
import { useDshSessionFeatures } from "../hooks/useDshSessionFeatures";

type InsightTab = "trajectory" | "stats" | "files" | "workflows" | "schedules" | "feedback";

const tabs: Array<{ id: InsightTab; icon: typeof Activity; labelKey: string }> = [
  { id: "trajectory", icon: Activity, labelKey: "dsh.insights.trajectory" },
  { id: "stats", icon: BarChart3, labelKey: "dsh.insights.stats" },
  { id: "files", icon: FileOutput, labelKey: "dsh.insights.files" },
  { id: "workflows", icon: GitBranch, labelKey: "dsh.insights.workflows" },
  { id: "schedules", icon: Clock3, labelKey: "dsh.insights.schedules" },
  { id: "feedback", icon: MessageSquareText, labelKey: "dsh.insights.feedback" },
];

function finite(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function statsProjection(value: unknown): DshSessionStatsProjection | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const item = value as Partial<DshSessionStatsProjection>;
  return finite(item.turns) && finite(item.steps) && finite(item.llmMs) && finite(item.toolMs)
    && finite(item.ttftMs) && finite(item.ttftSteps) && finite(item.decodeMs) && finite(item.decodeTokens)
    ? item as DshSessionStatsProjection
    : undefined;
}

function tokenProjection(value: unknown): DshTokenUsageProjection | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const item = value as Partial<DshTokenUsageProjection>;
  return finite(item.uncachedInputTokens) && finite(item.outputTokens)
    && finite(item.cacheReadTokens) && finite(item.cacheWriteTokens)
    ? item as DshTokenUsageProjection
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

function displayTime(time: number): string {
  if (!time) return "--:--:--";
  return new Date(time).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function eventCategory(event: DshSessionEvent): "message" | "tool" | "lifecycle" | "system" {
  if (event.type.startsWith("user/") || event.type.startsWith("assistant/")) return "message";
  if (event.type.startsWith("tool/") || event.type.startsWith("tool-workflow/")) return "tool";
  if (event.type.startsWith("turn/") || event.type.startsWith("step/")) return "lifecycle";
  return "system";
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
  const cachePercent = cacheTotal > 0 ? Math.round(cacheRead / cacheTotal * 100) : 0;
  const throughput = stats.decodeMs > 0 ? stats.decodeTokens / (stats.decodeMs / 1_000) : 0;
  return (
    <div className="dsh-insight-metrics">
      <ProjectionMetric label={t("dsh.insights.turns")} value={String(stats.turns)} />
      <ProjectionMetric label={t("dsh.insights.steps")} value={String(stats.steps)} />
      <ProjectionMetric label={t("dsh.insights.llmTime")} value={formatDuration(stats.llmMs)} />
      <ProjectionMetric label={t("dsh.insights.toolTime")} value={formatDuration(stats.toolMs)} />
      <ProjectionMetric label={t("dsh.insights.ttft")} value={stats.ttftSteps ? formatDuration(stats.ttftMs / stats.ttftSteps) : "-"} />
      <ProjectionMetric label={t("dsh.insights.throughput")} value={throughput ? `${throughput.toFixed(1)} tok/s` : "-"} />
      <ProjectionMetric label={t("dsh.insights.inputTokens")} value={formatTokens(input)} />
      <ProjectionMetric label={t("dsh.insights.outputTokens")} value={formatTokens(output)} />
      <ProjectionMetric label={t("dsh.insights.cacheHit")} value={`${cachePercent}%`} />
    </div>
  );
}

function TrajectoryPanel({
  entries,
  hasMore,
  loadingOlder,
  loadOlder,
}: {
  entries: ReturnType<typeof useDshSessionFeatures>["features"]["trajectory"];
  hasMore: boolean;
  loadingOlder: boolean;
  loadOlder: () => Promise<void>;
}) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<"all" | ReturnType<typeof eventCategory>>("all");
  const [expanded, setExpanded] = useState<number | null>(null);
  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return entries.filter((entry) => {
      if (category !== "all" && eventCategory(entry.event) !== category) return false;
      return !needle || `${entry.type} ${entry.title} ${entry.detail ?? ""}`.toLowerCase().includes(needle);
    });
  }, [category, entries, query]);
  return (
    <div className="dsh-trajectory-panel">
      <div className="dsh-insight-toolbar">
        <label className="dsh-insight-search">
          <Search size={13} aria-hidden="true" />
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("dsh.insights.search")} />
        </label>
        <div className="dsh-insight-segments" role="group" aria-label={t("dsh.insights.eventFilter")}>
          {(["all", "message", "tool", "lifecycle", "system"] as const).map((value) => (
            <button key={value} type="button" data-active={category === value} onClick={() => setCategory(value)}>
              {t(`dsh.insights.filter.${value}`)}
            </button>
          ))}
        </div>
      </div>
      {hasMore && (
        <button type="button" className="dsh-insight-load" disabled={loadingOlder} onClick={() => void loadOlder()}>
          {loadingOlder && <Loader2 size={12} className="spin" />}
          {t("dsh.insights.loadEarlier")}
        </button>
      )}
      <div className="dsh-trajectory-list">
        {filtered.map((entry) => (
          <div key={`${entry.seq}:${entry.type}`} className="dsh-trajectory-entry" data-category={eventCategory(entry.event)}>
            <button type="button" className="dsh-trajectory-summary" onClick={() => setExpanded(expanded === entry.seq ? null : entry.seq)}>
              {expanded === entry.seq ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
              <time>{displayTime(entry.time)}</time>
              <code>#{entry.seq}</code>
              <strong>{entry.title}</strong>
              {(entry.turn !== undefined || entry.step !== undefined) && (
                <span>T{entry.turn ?? "-"} / S{entry.step ?? "-"}</span>
              )}
            </button>
            {entry.detail && <div className="dsh-trajectory-detail">{entry.detail}</div>}
            {expanded === entry.seq && <pre>{JSON.stringify(entry.event, null, 2)}</pre>}
          </div>
        ))}
        {filtered.length === 0 && <div className="dsh-insight-empty">{t("dsh.insights.noEvents")}</div>}
      </div>
    </div>
  );
}

function FilesPanel({ files }: { files: ReturnType<typeof useDshSessionFeatures>["features"]["producedFiles"] }) {
  const { t } = useI18n();
  const [busy, setBusy] = useState<string | null>(null);
  if (files.length === 0) return <div className="dsh-insight-empty">{t("dsh.insights.noFiles")}</div>;
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

function WorkflowsPanel({ workflows }: { workflows: ReturnType<typeof useDshSessionFeatures>["features"]["workflows"] }) {
  const { t } = useI18n();
  if (workflows.length === 0) return <div className="dsh-insight-empty">{t("dsh.insights.noWorkflows")}</div>;
  return (
    <div className="dsh-workflow-list">
      {workflows.map((run) => (
        <section key={run.runId} data-status={run.status}>
          <header><GitBranch size={14} /><strong>{run.name}</strong><span>{t(`dsh.insights.status.${run.status}`)}</span></header>
          {Object.entries(run.phases).map(([key, phase]) => (
            <div key={key} className="dsh-workflow-phase">
              <h4>{phase.phase || t("dsh.insights.defaultPhase")}</h4>
              {phase.members.map((member) => (
                <div key={member.seq} className="dsh-workflow-member" data-status={member.status}>
                  {member.status === "running" ? <Loader2 size={12} className="spin" /> : <CheckCircle2 size={12} />}
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

function SchedulesPanel({ schedules }: { schedules: ReturnType<typeof useDshSessionFeatures>["features"]["schedules"] }) {
  const { t } = useI18n();
  if (schedules.length === 0) return <div className="dsh-insight-empty">{t("dsh.insights.noSchedules")}</div>;
  return (
    <div className="dsh-schedule-list">
      {schedules.map((schedule) => (
        <div key={schedule.id} data-state={schedule.state}>
          <Clock3 size={14} />
          <div><strong>{schedule.prompt}</strong><span>{new Date(schedule.scheduledAt).toLocaleString()}</span></div>
          <code>{schedule.kind}{schedule.everySeconds ? ` · ${schedule.everySeconds}s` : ""}</code>
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
      <div className="dsh-feedback-rating-hint"><ThumbsUp size={14} /><ThumbsDown size={14} /><span>{t("dsh.insights.messageFeedbackHint")}</span></div>
      <textarea value={value} onChange={(event) => setValue(event.target.value)} placeholder={t("dsh.insights.feedbackPlaceholder")} />
      <button type="button" disabled={!value.trim() || busy} onClick={() => void submit()}>
        {busy ? <Loader2 size={13} className="spin" /> : <Send size={13} />}
        {t("dsh.insights.sendFeedback")}
      </button>
      {result && <pre>{result}</pre>}
    </div>
  );
}

export function DshSessionInsights({ sessionId, live }: { sessionId: string; live?: DshLiveSessionState }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<InsightTab>("trajectory");
  const history = useDshSessionFeatures(sessionId);
  const sessionStats = statsProjection(live?.projections?.sessionStats ?? history.projections.sessionStats);
  const tokenUsage = tokenProjection(live?.projections?.tokenUsage ?? history.projections.tokenUsage);
  const stats: DshStats = {
    ...history.features.stats,
    ...(sessionStats ?? {}),
    inputTokens: history.features.stats.inputTokens,
    outputTokens: history.features.stats.outputTokens,
    cacheReadTokens: history.features.stats.cacheReadTokens,
    cacheWriteTokens: history.features.stats.cacheWriteTokens,
  };
  return (
    <>
      <button type="button" className="dsh-insights-trigger" title={t("dsh.insights.open")} onClick={() => setOpen(true)}>
        <Activity size={13} />
        <span>{t("dsh.insights.open")}</span>
        {history.features.events.length > 0 && <small>{history.features.events.length}</small>}
      </button>
      {open && (
        <div className="dsh-insights-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) setOpen(false); }}>
          <section className="dsh-insights-dialog" role="dialog" aria-modal="true" aria-label={t("dsh.insights.title")}>
            <header className="dsh-insights-header">
              <div><Activity size={16} /><strong>{t("dsh.insights.title")}</strong><code>{sessionId}</code></div>
              <button type="button" title={t("common.close")} aria-label={t("common.close")} onClick={() => setOpen(false)}><X size={16} /></button>
            </header>
            <nav className="dsh-insights-tabs" aria-label={t("dsh.insights.views")}>
              {tabs.map((item) => {
                const Icon = item.icon;
                return <button key={item.id} type="button" data-active={tab === item.id} onClick={() => setTab(item.id)}><Icon size={14} /><span>{t(item.labelKey)}</span></button>;
              })}
            </nav>
            <div className="dsh-insights-body">
              {history.loading ? <div className="dsh-insight-empty"><Loader2 size={16} className="spin" />{t("session.loading")}</div> : (
                <>
                  {tab === "trajectory" && <TrajectoryPanel entries={history.features.trajectory} hasMore={history.hasMore} loadingOlder={history.loadingOlder} loadOlder={history.loadOlder} />}
                  {tab === "stats" && <StatsPanel stats={stats} tokens={tokenUsage} />}
                  {tab === "files" && <FilesPanel files={history.features.producedFiles} />}
                  {tab === "workflows" && <WorkflowsPanel workflows={history.features.workflows} />}
                  {tab === "schedules" && <SchedulesPanel schedules={history.features.schedules} />}
                  {tab === "feedback" && <FeedbackPanel sessionId={sessionId} />}
                </>
              )}
              {history.error && <div className="dsh-insights-error">{history.error}</div>}
            </div>
          </section>
        </div>
      )}
    </>
  );
}
