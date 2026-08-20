/**
 * The trajectory panel's row list.
 *
 * Rows come from `deriveDshLedger`, so the list and the timing overview draw the
 * same operations: locating from the overview always lands on a row that exists.
 *
 * Folding is a baseline plus a set of per-group overrides rather than state per
 * group, because the toolbar's `Turns` / `Calls` buttons flip every group at once
 * and a session that has not been scrolled to yet has no state to flip. A group
 * the user toggled stays deviated from the baseline until the baseline moves.
 */

import { useEffect, useRef } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useI18n } from "../i18n";
import { dshDisplayTime } from "../dshTrajectoryFormat";
import { dshLedgerCategory } from "../dshTrajectoryLedger";
import type { DshLedgerGroup, DshLedgerRow } from "../dshTrajectoryLedger";
import type { DshImageLoader } from "../hooks/useDshImageLoader";
import { DshImageGallery } from "./DshImageGallery";
import { DshMentionProse } from "./DshMentionProse";
import { DshToolCard } from "./DshToolCard";

/** A fold: what the toolbar set, and which groups the user moved off it. */
export interface DshLedgerFold {
  collapsed: boolean;
  /** Anchor seqs of the groups toggled away from `collapsed`. */
  overrides: ReadonlySet<number>;
}

/** Folded when the baseline says so and the user has not deviated, or vice versa. */
export function dshLedgerFolded(fold: DshLedgerFold, anchor: number): boolean {
  return fold.collapsed !== fold.overrides.has(anchor);
}

/** Move one group off whatever the baseline currently says for it. */
export function dshLedgerToggle(fold: DshLedgerFold, anchor: number): DshLedgerFold {
  const overrides = new Set(fold.overrides);
  if (overrides.has(anchor)) overrides.delete(anchor);
  else overrides.add(anchor);
  return { ...fold, overrides };
}

/** Unfold one group, whichever side of the baseline it is currently on. */
export function dshLedgerReveal(fold: DshLedgerFold, anchor: number): DshLedgerFold {
  return dshLedgerFolded(fold, anchor) ? dshLedgerToggle(fold, anchor) : fold;
}

/** Set the baseline for every group, dropping the deviations it overrides. */
export function dshLedgerFoldAll(collapsed: boolean): DshLedgerFold {
  return { collapsed, overrides: new Set() };
}

/** The seq a turn group is folded by: its first row, which never renumbers. */
export function dshLedgerGroupAnchor(group: DshLedgerGroup): number {
  return group.rows[0]?.seq ?? 0;
}

function LedgerRow({
  row,
  selected,
  onSelect,
  fold,
  loadImage,
  mentionPaths,
  register,
}: {
  row: DshLedgerRow;
  selected: boolean;
  onSelect: (seq: number) => void;
  /** Present only for a reply that ordered calls, which is what folds. */
  fold?: { collapsed: boolean; onToggle: () => void };
  loadImage: DshImageLoader;
  mentionPaths: ReadonlyMap<number, readonly string[]>;
  register: (seq: number, node: HTMLDivElement | null) => void;
}) {
  const { t } = useI18n();
  const entry = row.entry;
  const mentions = mentionPaths.get(row.seq);
  return (
    <div
      ref={(node) => register(row.seq, node)}
      className="dsh-trajectory-entry dsh-ledger-row"
      data-category={dshLedgerCategory(row.tag)}
      data-tag={row.tag}
      data-depth={row.depth}
      data-status={row.status}
      data-selected={selected}
    >
      <div className="dsh-ledger-summary">
        {fold ? (
          <button
            type="button"
            className="dsh-ledger-fold"
            aria-expanded={!fold.collapsed}
            title={t(
              fold.collapsed ? "dsh.trajectory.expandRowCalls" : "dsh.trajectory.collapseRowCalls",
            )}
            aria-label={t(
              fold.collapsed ? "dsh.trajectory.expandRowCalls" : "dsh.trajectory.collapseRowCalls",
            )}
            onClick={fold.onToggle}
          >
            {fold.collapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
          </button>
        ) : (
          <span className="dsh-ledger-fold" aria-hidden="true" />
        )}
        <button
          type="button"
          className="dsh-ledger-open"
          aria-pressed={selected}
          title={t("dsh.trajectory.openDetails")}
          onClick={() => onSelect(row.seq)}
        >
          <span className="dsh-ledger-tag" data-tag={row.tag}>
            {row.tag}
          </span>
          <time>{dshDisplayTime(row.startedAt)}</time>
          <code>#{row.seq}</code>
          <strong>{row.title}</strong>
          {(row.turn !== undefined || row.step !== undefined) && (
            <span className="dsh-ledger-marker">
              T{row.turn ?? "-"} / S{row.step ?? "-"}
            </span>
          )}
        </button>
      </div>
      {entry.images && (
        <DshImageGallery
          images={entry.images}
          load={loadImage}
          align={entry.type.startsWith("user/") ? "end" : "start"}
        />
      )}
      {entry.view ? (
        <DshToolCard intent={entry.view} />
      ) : (
        entry.detail &&
        (mentions ? (
          // The closing prose carries the produced-file vocabulary: a token
          // naming one of this session's files opens it.
          <DshMentionProse
            className="dsh-trajectory-detail"
            prose={entry.detail}
            paths={mentions}
          />
        ) : (
          <div className="dsh-trajectory-detail">{entry.detail}</div>
        ))
      )}
    </div>
  );
}

export function DshTrajectoryLedger({
  groups,
  visible,
  selectedSeq,
  onSelect,
  turns,
  onToggleTurn,
  calls,
  onToggleCall,
  loadImage,
  mentionPaths,
}: {
  groups: readonly DshLedgerGroup[];
  /** The seqs the search, category, and overview filters left; null means all. */
  visible: ReadonlySet<number> | null;
  selectedSeq: number | null;
  onSelect: (seq: number) => void;
  turns: DshLedgerFold;
  onToggleTurn: (anchor: number) => void;
  calls: DshLedgerFold;
  onToggleCall: (anchor: number) => void;
  loadImage: DshImageLoader;
  mentionPaths: ReadonlyMap<number, readonly string[]>;
}) {
  const { t } = useI18n();
  const rowNodes = useRef(new Map<number, HTMLDivElement>());
  const register = (seq: number, node: HTMLDivElement | null) => {
    if (node === null) rowNodes.current.delete(seq);
    else rowNodes.current.set(seq, node);
  };

  // Locating from the overview only moves the selection, so bringing the row
  // into view is what makes the click readable. `nearest` leaves a row that is
  // already visible where it is, which is the case for a click on a row.
  useEffect(() => {
    if (selectedSeq === null) return;
    rowNodes.current.get(selectedSeq)?.scrollIntoView({ block: "nearest" });
  }, [selectedSeq]);

  // The filters decide which rows a group has; the folds only decide which of
  // them are drawn. So a group with a matching row keeps its heading even when
  // the fold hides the row, and a group with none is dropped rather than drawn
  // empty — a search reads as a shorter list, not a list of empty headings.
  const shown = groups
    .map((group) => {
      const parents = new Set(
        group.rows.map((row) => row.parentSeq).filter((seq): seq is number => seq !== undefined),
      );
      const matching =
        visible === null ? group.rows : group.rows.filter((row) => visible.has(row.seq));
      return {
        group,
        anchor: dshLedgerGroupAnchor(group),
        parents,
        matching,
        rows: matching.filter(
          (row) => row.parentSeq === undefined || !dshLedgerFolded(calls, row.parentSeq),
        ),
      };
    })
    .filter((item) => item.matching.length > 0);

  if (shown.length === 0) {
    return (
      <div className="dsh-trajectory-list">
        <div className="dsh-insight-empty">{t("dsh.insights.noEvents")}</div>
      </div>
    );
  }

  return (
    <div className="dsh-trajectory-list" role="list" aria-label={t("dsh.trajectory.rows")}>
      {shown.map(({ group, anchor, parents, matching, rows }) => {
        const folded = dshLedgerFolded(turns, anchor);
        return (
          <section key={anchor} className="dsh-ledger-group" role="listitem">
            <button
              type="button"
              className="dsh-ledger-group-head"
              aria-expanded={!folded}
              title={t(folded ? "dsh.trajectory.expandGroup" : "dsh.trajectory.collapseGroup")}
              onClick={() => onToggleTurn(anchor)}
            >
              {folded ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
              <strong>
                {group.turn === undefined
                  ? t("dsh.trajectory.looseGroup")
                  : t("dsh.trajectory.turnGroup", { turn: String(group.turn) })}
              </strong>
              <small>{t("dsh.trajectory.groupRows", { count: String(matching.length) })}</small>
            </button>
            {!folded &&
              rows.map((row) => (
                <LedgerRow
                  key={row.seq}
                  row={row}
                  selected={row.seq === selectedSeq}
                  onSelect={onSelect}
                  {...(parents.has(row.seq)
                    ? {
                        fold: {
                          collapsed: dshLedgerFolded(calls, row.seq),
                          onToggle: () => onToggleCall(row.seq),
                        },
                      }
                    : {})}
                  loadImage={loadImage}
                  mentionPaths={mentionPaths}
                  register={register}
                />
              ))}
          </section>
        );
      })}
    </div>
  );
}
