/**
 * The trajectory panel's detail column.
 *
 * One row at a time, in the five tabs the Harness' own trajectory view offers
 * (`ui-trajectory/src/client/TrajectoryTable.tsx:918`). `Payload` and `Result`
 * are offered only when the row carries one, so a tab is never an empty pane;
 * `Schema` and `Timing` always are, because "this row has no schema" and "this
 * row was never measured" are answers the user came here for.
 */

import { useState } from "react";
import { X } from "lucide-react";
import { useI18n } from "../i18n";
import { dshStartedAt } from "../dshTrajectoryFormat";
import { formatDshTimelineOffset } from "../dshTrajectoryTimeline";
import type { DshLedgerRow } from "../dshTrajectoryLedger";

type DetailTab = "summary" | "payload" | "result" | "schema" | "timing";

const TAB_LABELS: Record<DetailTab, string> = {
  summary: "dsh.trajectory.detail.summary",
  payload: "dsh.trajectory.detail.payload",
  result: "dsh.trajectory.detail.result",
  schema: "dsh.trajectory.detail.schema",
  timing: "dsh.trajectory.detail.timing",
};

function tabsFor(row: DshLedgerRow): readonly DetailTab[] {
  const list: DetailTab[] = ["summary"];
  if (row.payload !== undefined) list.push("payload");
  if (row.result !== undefined) list.push("result");
  list.push("schema", "timing");
  return list;
}

/**
 * The crumbs above a row, outermost first.
 *
 * `parentSeq` links a call to the reply that ordered it and never deeper, so the
 * turn boundary is looked up by turn number instead of being walked to.
 */
function ancestorsOf(row: DshLedgerRow, rows: readonly DshLedgerRow[]): readonly DshLedgerRow[] {
  const chain: DshLedgerRow[] = [];
  const seen = new Set<number>([row.seq]);
  let current: DshLedgerRow = row;
  while (current.parentSeq !== undefined) {
    const parentSeq = current.parentSeq;
    const parent = rows.find((candidate) => candidate.seq === parentSeq);
    if (parent === undefined || seen.has(parent.seq)) break;
    seen.add(parent.seq);
    chain.unshift(parent);
    current = parent;
  }
  const turn =
    row.turn === undefined
      ? undefined
      : rows.find((candidate) => candidate.tag === "TURN" && candidate.turn === row.turn);
  if (turn !== undefined && !seen.has(turn.seq)) chain.unshift(turn);
  return chain;
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </>
  );
}

function SummaryTab({
  row,
  rows,
  onSelect,
}: {
  row: DshLedgerRow;
  rows: readonly DshLedgerRow[];
  onSelect: (seq: number) => void;
}) {
  const { t } = useI18n();
  const ancestors = ancestorsOf(row, rows);
  return (
    <>
      <dl className="dsh-detail-fields">
        <Field label={t("dsh.trajectory.detail.hierarchy")}>
          {ancestors.length === 0 ? (
            <span className="dsh-detail-muted">{t("dsh.trajectory.detail.sessionRoot")}</span>
          ) : (
            <span className="dsh-detail-crumbs">
              {ancestors.map((ancestor) => (
                <button key={ancestor.seq} type="button" onClick={() => onSelect(ancestor.seq)}>
                  {ancestor.title}
                </button>
              ))}
            </span>
          )}
        </Field>
        <Field label={t("dsh.trajectory.detail.status")}>
          <span className="dsh-detail-status" data-status={row.status}>
            {t(`dsh.trajectory.status.${row.status}`)}
          </span>
        </Field>
        {(row.turn !== undefined || row.step !== undefined) && (
          <Field label={t("dsh.trajectory.detail.position")}>
            T{row.turn ?? "-"} / S{row.step ?? "-"}
          </Field>
        )}
        {row.toolName !== undefined && (
          <Field label={t("dsh.trajectory.detail.tool")}>
            <code>{row.toolName}</code>
          </Field>
        )}
        {row.callId !== undefined && (
          <Field label={t("dsh.trajectory.detail.callId")}>
            <code>{row.callId}</code>
          </Field>
        )}
        {row.usage && (
          <Field label={t("dsh.trajectory.detail.usage")}>
            {t("dsh.trajectory.detail.usageValue", {
              input: String(row.usage.inputTokens),
              output: String(row.usage.outputTokens),
              cached: String(row.usage.cacheReadTokens),
            })}
          </Field>
        )}
      </dl>
      {/* 上面的分栏是对事件的"解读",原始事件是事实来源,所以默认展开——查轨迹
          的人多半就是来看原始 payload 的,再点一下纯属多余。仍然是 <details>,
          用户可以自己收起来;不受控,收起状态不跨条目保留。 */}
      <details className="dsh-detail-raw" open>
        <summary>{t("dsh.trajectory.detail.rawEvent")}</summary>
        <pre>{JSON.stringify(row.entry.event, null, 2)}</pre>
      </details>
    </>
  );
}

function SchemaTab({ row }: { row: DshLedgerRow }) {
  const { t } = useI18n();
  if (row.schema === undefined) {
    return <div className="dsh-insight-empty">{t("dsh.trajectory.detail.schemaUnavailable")}</div>;
  }
  const parameters = row.schema.parameters;
  return (
    <div className="dsh-detail-schema">
      <h4>{row.schema.name}</h4>
      {row.schema.description !== "" && <p>{row.schema.description}</p>}
      {parameters === undefined ? (
        <div className="dsh-insight-empty">{t("dsh.trajectory.detail.noParameters")}</div>
      ) : (
        <>
          <h5>{t("dsh.trajectory.detail.parameters")}</h5>
          <pre>{JSON.stringify(parameters, null, 2)}</pre>
        </>
      )}
    </div>
  );
}

function TimingTab({ row }: { row: DshLedgerRow }) {
  const { t } = useI18n();
  const source =
    row.durationMs !== undefined
      ? "dsh.trajectory.detail.timingSession"
      : row.status === "running"
        ? "dsh.trajectory.detail.timingRunning"
        : "dsh.trajectory.detail.timingUnavailable";
  return (
    <dl className="dsh-detail-fields">
      <Field label={t("dsh.trajectory.detail.started")}>{dshStartedAt(row.startedAt)}</Field>
      <Field label={t("dsh.trajectory.detail.duration")}>
        {row.durationMs === undefined ? (
          <span className="dsh-detail-muted">{t("dsh.trajectory.detail.notMeasured")}</span>
        ) : (
          formatDshTimelineOffset(row.durationMs)
        )}
      </Field>
      <Field label={t("dsh.trajectory.detail.timingSource")}>{t(source)}</Field>
      {row.ttftMs !== undefined && (
        <Field label={t("dsh.trajectory.detail.ttft")}>{formatDshTimelineOffset(row.ttftMs)}</Field>
      )}
      {row.decodeMs !== undefined && (
        <Field label={t("dsh.trajectory.detail.decode")}>
          {formatDshTimelineOffset(row.decodeMs)}
        </Field>
      )}
    </dl>
  );
}

export function DshTrajectoryDetails({
  row,
  rows,
  onSelect,
  onClose,
}: {
  row: DshLedgerRow;
  rows: readonly DshLedgerRow[];
  onSelect: (seq: number) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [tab, setTab] = useState<DetailTab>("summary");
  const available = tabsFor(row);
  // Clicking through rows must never land on a pane the new row cannot fill.
  const active = available.includes(tab) ? tab : "summary";
  return (
    <aside className="dsh-trajectory-details" aria-label={t("dsh.trajectory.detail.title")}>
      <header>
        <span className="dsh-ledger-tag" data-tag={row.tag}>
          {row.tag}
        </span>
        <strong title={row.title}>{row.title}</strong>
        <code>#{row.seq}</code>
        <button
          type="button"
          title={t("common.close")}
          aria-label={t("common.close")}
          onClick={onClose}
        >
          <X size={14} />
        </button>
      </header>
      <nav className="dsh-detail-tabs" aria-label={t("dsh.trajectory.detail.views")}>
        {available.map((item) => (
          <button
            key={item}
            type="button"
            data-active={active === item}
            onClick={() => setTab(item)}
          >
            {t(TAB_LABELS[item])}
          </button>
        ))}
      </nav>
      <div className="dsh-detail-body">
        {active === "summary" && <SummaryTab row={row} rows={rows} onSelect={onSelect} />}
        {active === "payload" && <pre className="dsh-detail-code">{row.payload}</pre>}
        {active === "result" && <pre className="dsh-detail-code">{row.result}</pre>}
        {active === "schema" && <SchemaTab row={row} />}
        {active === "timing" && <TimingTab row={row} />}
      </div>
    </aside>
  );
}
