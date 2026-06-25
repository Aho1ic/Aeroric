import { invoke } from "@tauri-apps/api/core";
import { confirm } from "@tauri-apps/plugin-dialog";
import { Check, GitBranch, Loader2, RefreshCw, Search, X } from "lucide-react";
import type React from "react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useI18n } from "../../i18n";
import type {
  GitBlameResult,
  GitBranchGraphResult,
  GitConflictFile,
  GitConflictPreview,
  GitConflictResolution,
  GitStashDiff,
  GitStashEntry,
} from "../../types";
import { branchGraphSummary, projectRelativeGitPath, stashDisplayTitle } from "./gitAdvancedState";

export function GitAdvancedPanel({
  projectPath,
  activeFilePath,
  width,
  onOpenFile,
}: {
  projectPath: string;
  activeFilePath: string | null;
  width: number;
  onOpenFile: (path: string, name: string, selection?: { line: number; column?: number }) => void;
}) {
  const { t } = useI18n();
  const activeRelativePath = useMemo(
    () => projectRelativeGitPath(projectPath, activeFilePath),
    [activeFilePath, projectPath],
  );
  const [blamePath, setBlamePath] = useState(activeRelativePath);
  const [blame, setBlame] = useState<GitBlameResult | null>(null);
  const [branchGraph, setBranchGraph] = useState<GitBranchGraphResult | null>(null);
  const [stashes, setStashes] = useState<GitStashEntry[]>([]);
  const [stashDiff, setStashDiff] = useState<GitStashDiff | null>(null);
  const [conflicts, setConflicts] = useState<GitConflictFile[]>([]);
  const [conflictPreview, setConflictPreview] = useState<GitConflictPreview | null>(null);
  const [stashMessage, setStashMessage] = useState("");
  const [includeUntracked, setIncludeUntracked] = useState(true);
  const [loadingBlame, setLoadingBlame] = useState(false);
  const [loadingBranchGraph, setLoadingBranchGraph] = useState(false);
  const [loadingStashes, setLoadingStashes] = useState(false);
  const [loadingStashDiff, setLoadingStashDiff] = useState<string | null>(null);
  const [loadingConflicts, setLoadingConflicts] = useState(false);
  const [loadingConflictPreview, setLoadingConflictPreview] = useState<string | null>(null);
  const [working, setWorking] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (activeRelativePath && !blamePath) {
      setBlamePath(activeRelativePath);
    }
  }, [activeRelativePath, blamePath]);

  const graphSummary = useMemo(
    () => (branchGraph ? branchGraphSummary(branchGraph) : null),
    [branchGraph],
  );

  const loadBlame = useCallback(
    async (path = blamePath) => {
      const filePath = path.trim();
      if (!filePath) return;
      setLoadingBlame(true);
      setError(null);
      try {
        const result = await invoke<GitBlameResult>("git_blame_file", { projectPath, filePath });
        setBlame(result);
        setBlamePath(filePath);
      } catch (err) {
        setError(String(err));
      } finally {
        setLoadingBlame(false);
      }
    },
    [blamePath, projectPath],
  );

  const refreshStashes = useCallback(async () => {
    setLoadingStashes(true);
    setError(null);
    try {
      setStashes(await invoke<GitStashEntry[]>("git_stash_list", { projectPath }));
    } catch (err) {
      setError(String(err));
    } finally {
      setLoadingStashes(false);
    }
  }, [projectPath]);

  const refreshBranchGraph = useCallback(async () => {
    setLoadingBranchGraph(true);
    setError(null);
    try {
      setBranchGraph(
        await invoke<GitBranchGraphResult>("git_branch_graph", { projectPath, limit: 80 }),
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setLoadingBranchGraph(false);
    }
  }, [projectPath]);

  const refreshConflicts = useCallback(async () => {
    setLoadingConflicts(true);
    setError(null);
    try {
      const nextConflicts = await invoke<GitConflictFile[]>("git_conflict_files", { projectPath });
      setConflicts(nextConflicts);
      setConflictPreview((preview) =>
        preview && !nextConflicts.some((file) => file.path === preview.filePath) ? null : preview,
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setLoadingConflicts(false);
    }
  }, [projectPath]);

  useEffect(() => {
    void refreshBranchGraph();
    void refreshStashes();
    void refreshConflicts();
  }, [refreshBranchGraph, refreshConflicts, refreshStashes]);

  const pushStash = async () => {
    setWorking("stash-push");
    setError(null);
    try {
      await invoke<string>("git_stash_push", {
        projectPath,
        message: stashMessage.trim() || null,
        includeUntracked,
      });
      setStashMessage("");
      await refreshStashes();
    } catch (err) {
      setError(String(err));
    } finally {
      setWorking(null);
    }
  };

  const applyStash = async (entry: GitStashEntry) => {
    const ok = await confirm(t("gitAdvanced.confirmApplyStash", { name: entry.name }), {
      title: t("gitAdvanced.applyStash"),
      kind: "warning",
      okLabel: t("gitAdvanced.apply"),
    });
    if (!ok) return;
    setWorking(`apply:${entry.name}`);
    setError(null);
    try {
      await invoke<string>("git_stash_apply", { projectPath, stashRef: entry.name });
      await Promise.all([refreshStashes(), refreshConflicts()]);
    } catch (err) {
      setError(String(err));
    } finally {
      setWorking(null);
    }
  };

  const dropStash = async (entry: GitStashEntry) => {
    const ok = await confirm(t("gitAdvanced.confirmDropStash", { name: entry.name }), {
      title: t("gitAdvanced.dropStash"),
      kind: "warning",
      okLabel: t("common.delete"),
    });
    if (!ok) return;
    setWorking(`drop:${entry.name}`);
    setError(null);
    try {
      await invoke<string>("git_stash_drop", { projectPath, stashRef: entry.name });
      await refreshStashes();
    } catch (err) {
      setError(String(err));
    } finally {
      setWorking(null);
    }
  };

  const loadStashDiff = async (entry: GitStashEntry) => {
    setLoadingStashDiff(entry.name);
    setError(null);
    try {
      setStashDiff(
        await invoke<GitStashDiff>("git_stash_diff", { projectPath, stashRef: entry.name }),
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setLoadingStashDiff(null);
    }
  };

  const loadConflictPreview = async (file: GitConflictFile) => {
    setLoadingConflictPreview(file.path);
    setError(null);
    try {
      setConflictPreview(
        await invoke<GitConflictPreview>("git_conflict_preview", {
          projectPath,
          filePath: file.path,
        }),
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setLoadingConflictPreview(null);
    }
  };

  const resolveConflict = async (file: GitConflictFile, resolution: GitConflictResolution) => {
    const ok = await confirm(t(`gitAdvanced.confirmResolve.${resolution}`, { name: file.path }), {
      title: t("gitAdvanced.resolveConflict"),
      kind: "warning",
      okLabel: t(`gitAdvanced.resolution.${resolution}`),
    });
    if (!ok) return;
    setWorking(`resolve:${file.path}:${resolution}`);
    setError(null);
    try {
      await invoke("git_resolve_conflict", { projectPath, filePath: file.path, resolution });
      if (conflictPreview?.filePath === file.path) {
        setConflictPreview(null);
      }
      await refreshConflicts();
    } catch (err) {
      setError(String(err));
    } finally {
      setWorking(null);
    }
  };

  const openProjectFile = (filePath: string, line = 1) => {
    const path = `${projectPath.replace(/\/+$/, "")}/${filePath}`;
    const name = filePath.split("/").pop() || filePath;
    onOpenFile(path, name, { line, column: 1 });
  };

  return (
    <div style={rootStyle(width)}>
      <div style={headerStyle}>
        <GitBranch size={14} />
        <span>{t("gitAdvanced.title")}</span>
        <button
          type="button"
          style={iconButtonStyle}
          onClick={() => {
            void refreshBranchGraph();
            void refreshStashes();
            void refreshConflicts();
          }}
          title={t("common.refresh")}
        >
          <RefreshCw size={13} />
        </button>
      </div>

      {error && <div style={errorStyle}>{error}</div>}

      <section style={sectionStyle}>
        <div style={sectionHeaderStyle}>
          <span>{t("gitAdvanced.branchGraph")}</span>
          <button type="button" style={smallButtonStyle} onClick={() => void refreshBranchGraph()}>
            {loadingBranchGraph ? t("common.loading") : t("common.refresh")}
          </button>
        </div>
        {graphSummary && (
          <div style={graphSummaryStyle}>
            <span>{graphSummary.totalCommits}</span>
            {graphSummary.currentBranch && (
              <span>{t("gitAdvanced.currentBranch", { branch: graphSummary.currentBranch })}</span>
            )}
            {branchGraph?.truncated && (
              <span style={graphTruncatedStyle}>{t("gitAdvanced.graphTruncated")}</span>
            )}
          </div>
        )}
        <div style={graphListStyle}>
          {branchGraph?.commits.length ? (
            branchGraph.commits.map((commit) => (
              <div key={commit.hash} style={graphRowStyle}>
                <div style={graphRailStyle} />
                <div style={graphCommitStyle}>
                  <div style={stashTitleStyle}>{commit.subject || commit.shortHash}</div>
                  <div style={graphMetaStyle}>
                    {commit.shortHash} - {commit.author} - {commit.relativeTime}
                  </div>
                  {commit.refs.length ? (
                    <div style={refListStyle}>
                      {commit.refs.map((ref) => (
                        <span key={`${commit.hash}:${ref}`} style={refChipStyle}>
                          {ref.startsWith("HEAD -> ")
                            ? ref.slice("HEAD -> ".length)
                            : ref.replace(/^tag: /, "")}
                        </span>
                      ))}
                    </div>
                  ) : null}
                </div>
              </div>
            ))
          ) : (
            <div style={emptyStyle}>{t("gitAdvanced.noBranchGraph")}</div>
          )}
        </div>
      </section>

      <section style={sectionStyle}>
        <div style={sectionHeaderStyle}>
          <span>{t("gitAdvanced.blame")}</span>
          {activeRelativePath && (
            <button
              type="button"
              style={smallButtonStyle}
              onClick={() => {
                setBlamePath(activeRelativePath);
                void loadBlame(activeRelativePath);
              }}
            >
              {t("gitAdvanced.useActiveFile")}
            </button>
          )}
        </div>
        <div style={inputRowStyle}>
          <Search size={13} />
          <input
            value={blamePath}
            onChange={(event) => setBlamePath(event.target.value)}
            placeholder="src/index.ts"
            style={inputStyle}
          />
          <button
            type="button"
            style={primaryButtonStyle}
            disabled={!blamePath.trim() || loadingBlame}
            onClick={() => void loadBlame()}
          >
            {loadingBlame ? <Loader2 size={13} className="spin" /> : <Check size={13} />}
            {t("gitAdvanced.loadBlame")}
          </button>
        </div>
        <div style={blameListStyle}>
          {blame?.lines.length ? (
            blame.lines.slice(0, 200).map((line) => (
              <button
                key={`${line.commit}:${line.line}`}
                type="button"
                style={blameRowStyle}
                onClick={() => openProjectFile(blame.filePath, line.line)}
                title={`${line.shortCommit} ${line.author} ${line.summary}`}
              >
                <span style={lineNumberStyle}>{line.line}</span>
                <span style={blameMetaStyle}>{line.author}</span>
                <span style={blameContentStyle}>{line.content}</span>
              </button>
            ))
          ) : (
            <div style={emptyStyle}>{t("gitAdvanced.noBlame")}</div>
          )}
        </div>
      </section>

      <section style={sectionStyle}>
        <div style={sectionHeaderStyle}>
          <span>{t("gitAdvanced.stashes")}</span>
          <button type="button" style={smallButtonStyle} onClick={() => void refreshStashes()}>
            {loadingStashes ? t("common.loading") : t("common.refresh")}
          </button>
        </div>
        <input
          value={stashMessage}
          onChange={(event) => setStashMessage(event.target.value)}
          placeholder={t("gitAdvanced.stashMessage")}
          style={wideInputStyle}
        />
        <label style={checkboxLabelStyle}>
          <input
            type="checkbox"
            checked={includeUntracked}
            onChange={(event) => setIncludeUntracked(event.target.checked)}
          />
          {t("gitAdvanced.includeUntracked")}
        </label>
        <button
          type="button"
          style={primaryButtonStyle}
          disabled={working === "stash-push"}
          onClick={() => void pushStash()}
        >
          {working === "stash-push" ? <Loader2 size={13} className="spin" /> : <Check size={13} />}
          {t("gitAdvanced.createStash")}
        </button>
        <div style={listStyle}>
          {stashes.length ? (
            stashes.map((entry) => (
              <div key={entry.name} style={stashRowStyle}>
                <div style={stashTextStyle}>
                  <div style={stashTitleStyle}>{stashDisplayTitle(entry)}</div>
                  <div style={stashMetaStyle}>
                    {entry.date} - {entry.commit.slice(0, 8)}
                  </div>
                </div>
                <button
                  type="button"
                  style={smallButtonStyle}
                  disabled={loadingStashDiff === entry.name}
                  onClick={() => void loadStashDiff(entry)}
                >
                  {loadingStashDiff === entry.name
                    ? t("common.loading")
                    : t("gitAdvanced.stashDiff")}
                </button>
                <button
                  type="button"
                  style={smallButtonStyle}
                  disabled={working === `apply:${entry.name}`}
                  onClick={() => void applyStash(entry)}
                >
                  {t("gitAdvanced.apply")}
                </button>
                <button
                  type="button"
                  style={dangerButtonStyle}
                  disabled={working === `drop:${entry.name}`}
                  onClick={() => void dropStash(entry)}
                >
                  <X size={12} />
                </button>
              </div>
            ))
          ) : (
            <div style={emptyStyle}>{t("gitAdvanced.noStashes")}</div>
          )}
        </div>
        <div style={stashDiffBlockStyle}>
          <div style={stashDiffHeaderStyle}>
            <span>{t("gitAdvanced.stashDiff")}</span>
            {stashDiff?.truncated && (
              <span style={stashDiffTruncatedStyle}>{t("gitAdvanced.diffTruncated")}</span>
            )}
          </div>
          <pre style={stashDiffPreviewStyle}>
            {stashDiff?.diff?.trim() || t("gitAdvanced.noStashDiff")}
          </pre>
        </div>
      </section>

      <section style={sectionStyle}>
        <div style={sectionHeaderStyle}>
          <span>{t("gitAdvanced.conflicts")}</span>
          <button type="button" style={smallButtonStyle} onClick={() => void refreshConflicts()}>
            {loadingConflicts ? t("common.loading") : t("common.refresh")}
          </button>
        </div>
        <div style={listStyle}>
          {conflicts.length ? (
            conflicts.map((file) => (
              <div key={file.path} style={conflictRowStyle}>
                <button
                  type="button"
                  style={filePathButtonStyle}
                  onClick={() => openProjectFile(file.path)}
                >
                  {file.path}
                </button>
                <div style={conflictActionsStyle}>
                  <button
                    type="button"
                    style={smallButtonStyle}
                    disabled={loadingConflictPreview === file.path}
                    onClick={() => void loadConflictPreview(file)}
                  >
                    {loadingConflictPreview === file.path
                      ? t("common.loading")
                      : t("gitAdvanced.previewConflict")}
                  </button>
                  {(["ours", "theirs", "both"] as const).map((resolution) => (
                    <button
                      key={resolution}
                      type="button"
                      style={smallButtonStyle}
                      disabled={working === `resolve:${file.path}:${resolution}`}
                      onClick={() => void resolveConflict(file, resolution)}
                    >
                      {t(`gitAdvanced.resolution.${resolution}`)}
                    </button>
                  ))}
                </div>
              </div>
            ))
          ) : (
            <div style={emptyStyle}>{t("gitAdvanced.noConflicts")}</div>
          )}
        </div>
        <div style={conflictPreviewBlockStyle}>
          <div style={conflictPreviewHeaderStyle}>
            <span>{t("gitAdvanced.conflictPreview")}</span>
            {conflictPreview && (
              <span style={conflictPreviewPathStyle}>{conflictPreview.filePath}</span>
            )}
          </div>
          {conflictPreview?.hunks.length ? (
            <div style={conflictHunksStyle}>
              {conflictPreview.hunks.map((hunk) => (
                <div key={hunk.index} style={conflictHunkStyle}>
                  <div style={conflictHunkHeaderStyle}>
                    {t("gitAdvanced.conflictHunk", { index: String(hunk.index) })}
                  </div>
                  <div style={conflictColumnsStyle}>
                    <div style={conflictColumnStyle}>
                      <div style={conflictColumnHeaderStyle}>
                        {t("gitAdvanced.resolution.ours")}
                      </div>
                      <pre style={conflictColumnBodyStyle}>
                        {hunk.ours || t("gitAdvanced.emptySide")}
                      </pre>
                    </div>
                    <div style={conflictColumnStyle}>
                      <div style={conflictColumnHeaderStyle}>
                        {t("gitAdvanced.resolution.base")}
                      </div>
                      <pre style={conflictColumnBodyStyle}>
                        {hunk.base || t("gitAdvanced.noBaseSide")}
                      </pre>
                    </div>
                    <div style={conflictColumnStyle}>
                      <div style={conflictColumnHeaderStyle}>
                        {t("gitAdvanced.resolution.theirs")}
                      </div>
                      <pre style={conflictColumnBodyStyle}>
                        {hunk.theirs || t("gitAdvanced.emptySide")}
                      </pre>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div style={emptyStyle}>{t("gitAdvanced.noConflictPreview")}</div>
          )}
        </div>
      </section>
    </div>
  );
}

function rootStyle(width: number): React.CSSProperties {
  return {
    width,
    flexShrink: 0,
    minHeight: 0,
    display: "flex",
    flexDirection: "column",
    borderLeft: "1px solid var(--border-dim)",
    background: "var(--bg-panel)",
    overflow: "hidden",
  };
}

const headerStyle: React.CSSProperties = {
  height: 38,
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "0 10px",
  borderBottom: "1px solid var(--border-dim)",
  fontSize: 12,
  fontWeight: 650,
};

const sectionStyle: React.CSSProperties = {
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  gap: 7,
  padding: 10,
  borderBottom: "1px solid var(--border-dim)",
};

const sectionHeaderStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  color: "var(--text-primary)",
  fontSize: 11.5,
  fontWeight: 700,
};

const inputRowStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 6,
  minWidth: 0,
};

const inputStyle: React.CSSProperties = {
  flex: 1,
  minWidth: 0,
  height: 26,
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
  background: "var(--bg-card)",
  color: "var(--text-primary)",
  fontSize: 11,
  padding: "0 7px",
};

const wideInputStyle: React.CSSProperties = {
  ...inputStyle,
  flex: "0 0 auto",
  width: "100%",
  boxSizing: "border-box",
};

const iconButtonStyle: React.CSSProperties = {
  marginLeft: "auto",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  width: 24,
  height: 24,
  border: "none",
  borderRadius: 5,
  background: "transparent",
  color: "var(--text-muted)",
  cursor: "pointer",
};

const smallButtonStyle: React.CSSProperties = {
  height: 24,
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 4,
  border: "1px solid var(--border-dim)",
  borderRadius: 5,
  background: "transparent",
  color: "var(--text-muted)",
  fontSize: 10.5,
  fontWeight: 650,
  cursor: "pointer",
  padding: "0 7px",
};

const primaryButtonStyle: React.CSSProperties = {
  ...smallButtonStyle,
  background: "var(--primary-action-bg)",
  borderColor: "var(--primary-action-bg)",
  color: "var(--primary-action-fg)",
};

const dangerButtonStyle: React.CSSProperties = {
  ...smallButtonStyle,
  color: "var(--danger)",
};

const errorStyle: React.CSSProperties = {
  padding: "7px 10px",
  color: "var(--danger)",
  fontSize: 11,
  borderBottom: "1px solid var(--border-dim)",
};

const emptyStyle: React.CSSProperties = {
  padding: "12px 6px",
  color: "var(--text-muted)",
  textAlign: "center",
  fontSize: 11.5,
};

const blameListStyle: React.CSSProperties = {
  maxHeight: 220,
  minHeight: 72,
  overflow: "auto",
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
};

const blameRowStyle: React.CSSProperties = {
  width: "100%",
  display: "grid",
  gridTemplateColumns: "34px minmax(58px, 0.55fr) minmax(0, 1fr)",
  gap: 6,
  alignItems: "center",
  padding: "4px 6px",
  border: "none",
  borderBottom: "1px solid var(--border-dim)",
  background: "transparent",
  color: "var(--text-primary)",
  textAlign: "left",
  cursor: "pointer",
};

const lineNumberStyle: React.CSSProperties = {
  color: "var(--text-hint)",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
};

const blameMetaStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-muted)",
  fontSize: 10.5,
};

const blameContentStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
};

const checkboxLabelStyle: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 6,
  color: "var(--text-muted)",
  fontSize: 11,
};

const listStyle: React.CSSProperties = {
  minHeight: 44,
  maxHeight: 180,
  overflow: "auto",
};

const graphSummaryStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  flexWrap: "wrap",
  color: "var(--text-muted)",
  fontSize: 10.5,
};

const graphTruncatedStyle: React.CSSProperties = {
  color: "var(--warning)",
  fontWeight: 600,
};

const graphListStyle: React.CSSProperties = {
  minHeight: 56,
  maxHeight: 220,
  overflow: "auto",
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
};

const graphRowStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "18px minmax(0, 1fr)",
  gap: 6,
  padding: "7px 8px",
  borderBottom: "1px solid var(--border-dim)",
};

const graphRailStyle: React.CSSProperties = {
  width: 8,
  height: 8,
  marginTop: 3,
  borderRadius: 999,
  background: "var(--accent)",
  boxShadow: "0 0 0 3px var(--bg-card)",
};

const graphCommitStyle: React.CSSProperties = {
  minWidth: 0,
  display: "flex",
  flexDirection: "column",
  gap: 4,
};

const graphMetaStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-muted)",
  fontFamily: "var(--font-mono)",
  fontSize: 10,
};

const refListStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 4,
  flexWrap: "wrap",
  minWidth: 0,
};

const refChipStyle: React.CSSProperties = {
  maxWidth: "100%",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  border: "1px solid var(--border-dim)",
  borderRadius: 999,
  padding: "1px 6px",
  color: "var(--text-muted)",
  background: "var(--bg-card)",
  fontSize: 10,
  fontFamily: "var(--font-mono)",
};

const stashRowStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr) auto auto auto",
  gap: 6,
  alignItems: "center",
  padding: "6px 0",
  borderBottom: "1px solid var(--border-dim)",
};

const stashTextStyle: React.CSSProperties = {
  minWidth: 0,
};

const stashTitleStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-primary)",
  fontSize: 11,
  fontWeight: 650,
};

const stashMetaStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--text-muted)",
  fontSize: 10,
  fontFamily: "var(--font-mono)",
};

const stashDiffBlockStyle: React.CSSProperties = {
  marginTop: 8,
  minHeight: 0,
};

const stashDiffHeaderStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 8,
  marginBottom: 5,
  color: "var(--text-muted)",
  fontSize: 10.5,
  fontWeight: 650,
};

const stashDiffTruncatedStyle: React.CSSProperties = {
  color: "var(--warning)",
  fontWeight: 600,
};

const stashDiffPreviewStyle: React.CSSProperties = {
  maxHeight: 220,
  minHeight: 78,
  overflow: "auto",
  margin: 0,
  padding: 8,
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
  background: "var(--bg-card)",
  color: "var(--text-primary)",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
  lineHeight: 1.45,
  whiteSpace: "pre",
};

const conflictRowStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 6,
  padding: "7px 0",
  borderBottom: "1px solid var(--border-dim)",
};

const filePathButtonStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  border: "none",
  background: "transparent",
  color: "var(--text-primary)",
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  textAlign: "left",
  cursor: "pointer",
  padding: 0,
};

const conflictActionsStyle: React.CSSProperties = {
  display: "flex",
  gap: 5,
  flexWrap: "wrap",
};

const conflictPreviewBlockStyle: React.CSSProperties = {
  marginTop: 8,
  minHeight: 0,
};

const conflictPreviewHeaderStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 8,
  marginBottom: 5,
  color: "var(--text-muted)",
  fontSize: 10.5,
  fontWeight: 650,
};

const conflictPreviewPathStyle: React.CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontFamily: "var(--font-mono)",
  fontWeight: 500,
};

const conflictHunksStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 8,
};

const conflictHunkStyle: React.CSSProperties = {
  border: "1px solid var(--border-dim)",
  borderRadius: 6,
  overflow: "hidden",
  background: "var(--bg-card)",
};

const conflictHunkHeaderStyle: React.CSSProperties = {
  padding: "5px 7px",
  borderBottom: "1px solid var(--border-dim)",
  color: "var(--text-muted)",
  fontSize: 10.5,
  fontWeight: 650,
};

const conflictColumnsStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "repeat(3, minmax(160px, 1fr))",
  overflowX: "auto",
};

const conflictColumnStyle: React.CSSProperties = {
  minWidth: 160,
  borderRight: "1px solid var(--border-dim)",
};

const conflictColumnHeaderStyle: React.CSSProperties = {
  padding: "5px 7px",
  borderBottom: "1px solid var(--border-dim)",
  color: "var(--text-muted)",
  fontSize: 10.5,
  fontWeight: 650,
};

const conflictColumnBodyStyle: React.CSSProperties = {
  minHeight: 64,
  maxHeight: 180,
  overflow: "auto",
  margin: 0,
  padding: 7,
  color: "var(--text-primary)",
  fontFamily: "var(--font-mono)",
  fontSize: 10.5,
  lineHeight: 1.45,
  whiteSpace: "pre",
};
