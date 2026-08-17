import { useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  ExternalLink,
  FileCode2,
  FilePlus2,
  FileSearch,
  Globe,
  TerminalSquare,
} from "lucide-react";
import { useI18n } from "../i18n";
import type {
  DshDiffCallView,
  DshDiffResultView,
  DshFileDiff,
  DshGenericCallView,
  DshReadResultView,
  DshSearchMatchesResultView,
  DshSearchPathsResultView,
  DshTerminalCallView,
  DshTerminalResultView,
  DshToolEventView,
  DshWebFetchResultView,
  DshWebSearchResultView,
} from "../dshToolViews";

/**
 * Renders one DeepSeek Harness tool render-intent (`ToolEventView`) as the card
 * family the Harness Web UI uses: a terminal card, an inline diff, a grouped or
 * flat search result, a line-numbered read window, or a web citation list. An
 * arm this component does not recognize never reaches here — `parseDshToolEventView`
 * degrades it to `undefined` — and the caller then falls back to the raw event,
 * which is the documented behavior for a UI without the matching capability.
 */

/** Cap on rendered rows per card; the raw event stays available underneath. */
const ROW_BUDGET = 400;

function truncateRows<T>(rows: readonly T[]): { rows: T[]; hidden: number } {
  return rows.length <= ROW_BUDGET
    ? { rows: [...rows], hidden: 0 }
    : { rows: rows.slice(0, ROW_BUDGET), hidden: rows.length - ROW_BUDGET };
}

/**
 * Split a single-file change into line rows. `oldText: null` means a create or
 * an overwrite — there is no before-image to diff against, so every line reads
 * as an addition rather than being compared to a phantom empty file.
 */
function diffRows(diff: DshFileDiff): Array<{ sign: "+" | "-" | " "; text: string }> {
  const next = diff.newText.split("\n");
  if (diff.oldText === null) return next.map((text) => ({ sign: "+" as const, text }));
  const prev = diff.oldText.split("\n");
  // Common prefix/suffix trim keeps an edit's context tight without pulling in
  // a full LCS diff; the Harness already sends contextual hunks at result time.
  let head = 0;
  while (head < prev.length && head < next.length && prev[head] === next[head]) head += 1;
  let tail = 0;
  while (
    tail < prev.length - head &&
    tail < next.length - head &&
    prev[prev.length - 1 - tail] === next[next.length - 1 - tail]
  ) {
    tail += 1;
  }
  return [
    ...prev.slice(head, prev.length - tail).map((text) => ({ sign: "-" as const, text })),
    ...next.slice(head, next.length - tail).map((text) => ({ sign: "+" as const, text })),
  ];
}

function CardShell({
  icon,
  title,
  meta,
  children,
  kind,
}: {
  icon: React.ReactNode;
  title: string;
  meta?: React.ReactNode;
  children?: React.ReactNode;
  kind: string;
}) {
  return (
    <div className="dsh-tool-card" data-card={kind}>
      <header className="dsh-tool-card-head">
        {icon}
        <strong title={title}>{title}</strong>
        {meta}
      </header>
      {children}
    </div>
  );
}

function DiffBody({ diffs }: { diffs: readonly DshFileDiff[] }) {
  const { t } = useI18n();
  return (
    <>
      {diffs.map((diff) => {
        const { rows, hidden } = truncateRows(diffRows(diff));
        const added = rows.filter((row) => row.sign === "+").length;
        const removed = rows.filter((row) => row.sign === "-").length;
        return (
          <div className="dsh-tool-diff" key={diff.path}>
            <div className="dsh-tool-diff-path">
              <code title={diff.path}>{diff.path}</code>
              {diff.oldText === null && <em>{t("dsh.tool.newFile")}</em>}
              <span className="dsh-tool-diff-stat">
                <ins>+{added}</ins>
                <del>-{removed}</del>
              </span>
            </div>
            <pre className="dsh-tool-diff-body">
              {rows.map((row, index) => (
                <span key={index} data-sign={row.sign.trim() || "context"}>
                  {row.sign}
                  {row.text}
                  {"\n"}
                </span>
              ))}
            </pre>
            {hidden > 0 && (
              <div className="dsh-tool-card-more">{t("dsh.tool.moreLines", { count: hidden })}</div>
            )}
          </div>
        );
      })}
    </>
  );
}

function GenericCallCard({ view }: { view: DshGenericCallView }) {
  const { t } = useI18n();
  const raw =
    view.rawInput === undefined
      ? undefined
      : typeof view.rawInput === "string"
        ? view.rawInput
        : JSON.stringify(view.rawInput, null, 2);
  return (
    <CardShell
      kind="generic"
      icon={<FileCode2 size={13} aria-hidden="true" />}
      title={view.title}
      meta={
        view.kind && <span className="dsh-tool-card-kind">{t(`dsh.tool.kind.${view.kind}`)}</span>
      }
    >
      {raw !== undefined && raw !== "" && <pre className="dsh-tool-card-raw">{raw}</pre>}
      {view.locations && view.locations.length > 0 && (
        <div className="dsh-tool-card-locations">
          {view.locations.map((location) => (
            <code key={`${location.path}:${location.line ?? ""}`}>
              {location.path}
              {location.line !== undefined && `:${location.line}`}
            </code>
          ))}
        </div>
      )}
    </CardShell>
  );
}

function TerminalCard({ view }: { view: DshTerminalCallView | DshTerminalResultView }) {
  const { t } = useI18n();
  const isCall = view.card === "terminal" && "cwd" in view;
  const call = isCall ? (view as DshTerminalCallView) : undefined;
  const result = isCall ? undefined : (view as DshTerminalResultView);
  const title = view.title ?? t("dsh.tool.command");
  const status =
    result?.signal !== undefined
      ? result.signal
      : result?.exitCode !== undefined
        ? t("dsh.tool.exitCode", { code: result.exitCode })
        : undefined;
  const failed =
    result?.signal !== undefined || (result?.exitCode !== undefined && result.exitCode !== 0);
  return (
    <>
      {call?.description && <div className="dsh-tool-card-lead">{call.description}</div>}
      <CardShell
        kind="terminal"
        icon={<TerminalSquare size={13} aria-hidden="true" />}
        title={title}
        meta={
          <>
            {call?.cwd && <code className="dsh-tool-card-cwd">{call.cwd}</code>}
            {status !== undefined && (
              <span className="dsh-tool-card-exit" data-failed={failed}>
                {status}
              </span>
            )}
          </>
        }
      >
        {result?.output !== undefined && result.output !== "" && (
          <pre className="dsh-tool-card-console">{result.output}</pre>
        )}
      </CardShell>
    </>
  );
}

function DiffCard({ view }: { view: DshDiffCallView | DshDiffResultView }) {
  const { t } = useI18n();
  return (
    <CardShell
      kind="diff"
      icon={<FilePlus2 size={13} aria-hidden="true" />}
      title={view.title ?? t("dsh.tool.change")}
    >
      <DiffBody diffs={view.diffs} />
    </CardShell>
  );
}

function SearchMatchesCard({ view }: { view: DshSearchMatchesResultView }) {
  const { t } = useI18n();
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  return (
    <CardShell
      kind="search"
      icon={<FileSearch size={13} aria-hidden="true" />}
      title={view.title ?? t("dsh.tool.searchMatches")}
      meta={
        <span className="dsh-tool-card-count">
          {view.truncated
            ? t("dsh.tool.matchesTruncated", { count: view.total })
            : t("dsh.tool.matches", { count: view.total })}
        </span>
      }
    >
      {view.files.map((file) => {
        const open = collapsed[file.path] !== true;
        return (
          <div className="dsh-tool-search-group" key={file.path}>
            <button
              type="button"
              onClick={() => setCollapsed((current) => ({ ...current, [file.path]: open }))}
              aria-expanded={open}
            >
              {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              <code title={file.path}>{file.path}</code>
              <small>{file.matches.length}</small>
            </button>
            {open && (
              <pre className="dsh-tool-search-lines">
                {truncateRows(file.matches).rows.map((match) => (
                  <span key={match.lineNumber}>
                    <i>{match.lineNumber}</i>
                    {match.line}
                    {"\n"}
                  </span>
                ))}
              </pre>
            )}
          </div>
        );
      })}
      {view.files.length === 0 && (
        <div className="dsh-tool-card-empty">{t("dsh.tool.noMatches")}</div>
      )}
    </CardShell>
  );
}

function SearchPathsCard({ view }: { view: DshSearchPathsResultView }) {
  const { t } = useI18n();
  const { rows, hidden } = truncateRows(view.paths);
  return (
    <CardShell
      kind="search"
      icon={<FileSearch size={13} aria-hidden="true" />}
      title={view.title ?? t("dsh.tool.searchPaths")}
      meta={
        <span className="dsh-tool-card-count">
          {view.truncated
            ? t("dsh.tool.pathsTruncated", { count: view.total })
            : t("dsh.tool.paths", { count: view.total })}
        </span>
      }
    >
      {rows.length > 0 ? (
        <div className="dsh-tool-path-list">
          {rows.map((path) => (
            <code key={path} title={path}>
              {path}
            </code>
          ))}
        </div>
      ) : (
        <div className="dsh-tool-card-empty">{t("dsh.tool.noPaths")}</div>
      )}
      {hidden > 0 && (
        <div className="dsh-tool-card-more">{t("dsh.tool.morePaths", { count: hidden })}</div>
      )}
    </CardShell>
  );
}

function ReadCard({ view }: { view: DshReadResultView }) {
  const { t } = useI18n();
  const { rows, hidden } = truncateRows(view.lines);
  return (
    <CardShell
      kind="read"
      icon={<FileCode2 size={13} aria-hidden="true" />}
      title={view.title ?? view.path}
      meta={
        <>
          {view.lang && <span className="dsh-tool-card-lang">{view.lang}</span>}
          <span className="dsh-tool-card-count">
            {t("dsh.tool.showingLines", {
              shown: view.lines.length,
              total: view.totalLines,
              offset: view.offset,
            })}
          </span>
        </>
      }
    >
      {rows.length > 0 ? (
        <pre className="dsh-tool-read-body" data-lang={view.lang ?? "text"}>
          {rows.map((line) => (
            <span key={line.number}>
              <i>{line.number}</i>
              {line.text}
              {"\n"}
            </span>
          ))}
        </pre>
      ) : (
        <div className="dsh-tool-card-empty">{t("dsh.tool.emptyWindow")}</div>
      )}
      {hidden > 0 && (
        <div className="dsh-tool-card-more">{t("dsh.tool.moreLines", { count: hidden })}</div>
      )}
    </CardShell>
  );
}

function WebSearchCard({ view }: { view: DshWebSearchResultView }) {
  const { t } = useI18n();
  return (
    <CardShell
      kind="web"
      icon={<Globe size={13} aria-hidden="true" />}
      title={view.title ?? t("dsh.tool.webSearch")}
      meta={
        <span className="dsh-tool-card-count">
          {view.truncated
            ? t("dsh.tool.sourcesTruncated", { count: view.sources.length })
            : t("dsh.tool.sources", { count: view.sources.length })}
        </span>
      }
    >
      {view.answer && <div className="dsh-tool-web-answer">{view.answer}</div>}
      <ol className="dsh-tool-web-sources">
        {view.sources.map((source) => (
          <li key={source.url}>
            <a href={source.url} target="_blank" rel="noreferrer noopener">
              <span>{source.title ?? source.url}</span>
              <ExternalLink size={11} aria-hidden="true" />
            </a>
            {source.snippet && <p>{source.snippet}</p>}
            <footer>
              <code>{source.url}</code>
              {source.publishedAt && <time>{source.publishedAt}</time>}
            </footer>
          </li>
        ))}
      </ol>
      {view.sources.length === 0 && (
        <div className="dsh-tool-card-empty">{t("dsh.tool.noSources")}</div>
      )}
    </CardShell>
  );
}

function WebFetchCard({ view }: { view: DshWebFetchResultView }) {
  const { t } = useI18n();
  const failed = view.statusCode < 200 || view.statusCode >= 400;
  return (
    <CardShell
      kind="web"
      icon={<Globe size={13} aria-hidden="true" />}
      title={view.title ?? t("dsh.tool.webFetch")}
      meta={
        <>
          <span className="dsh-tool-card-exit" data-failed={failed}>
            {view.statusCode}
          </span>
          {view.truncated && (
            <span className="dsh-tool-card-count">{t("dsh.tool.bodyTruncated")}</span>
          )}
        </>
      }
    >
      <div className="dsh-tool-card-locations">
        <a href={view.url} target="_blank" rel="noreferrer noopener">
          {view.url}
          <ExternalLink size={11} aria-hidden="true" />
        </a>
      </div>
    </CardShell>
  );
}

/** Dispatch one render intent to its card. */
export function DshToolCard({ intent }: { intent: DshToolEventView }) {
  if (intent.for === "call") {
    const view = intent.view;
    if (view.card === "terminal") return <TerminalCard view={view} />;
    if (view.card === "diff") return <DiffCard view={view} />;
    return <GenericCallCard view={view} />;
  }
  const view = intent.view;
  switch (view.card) {
    case "terminal":
      return <TerminalCard view={view} />;
    case "diff":
      return <DiffCard view={view} />;
    case "search":
      return view.shape === "paths" ? (
        <SearchPathsCard view={view} />
      ) : (
        <SearchMatchesCard view={view} />
      );
    case "read":
      return <ReadCard view={view} />;
    case "web":
      return view.kind === "fetch" ? <WebFetchCard view={view} /> : <WebSearchCard view={view} />;
    case "generic":
      // A generic result view carries only a replacement title; without one it
      // adds nothing over the raw event the caller already renders.
      return view.title === undefined ? null : (
        <CardShell
          kind="generic"
          icon={<FileCode2 size={13} aria-hidden="true" />}
          title={view.title}
        />
      );
  }
}
