import { lazy, type ReactNode } from "react";
import type { IdeToolWithAvailability } from "../../plugins/ideToolRegistry";
import type { RightPanel } from "../../hooks/useProjectPanels";
import { renderIdeToolIcon } from "../RightToolbar";

export type ProjectPanel = Exclude<RightPanel, null>;

export function IdePanelShell({
  tools,
  activePanel,
  width,
  onSelectPanel,
  t,
  children,
}: {
  tools: IdeToolWithAvailability[];
  activePanel: string;
  width: number;
  onSelectPanel: (panel: IdeToolWithAvailability["panel"]) => void;
  t: (key: string) => string;
  children: ReactNode;
}) {
  const activeTool = tools.find((tool) => tool.panel === activePanel);

  return (
    <div
      style={{
        width,
        height: "100%",
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        background: "var(--bg-panel)",
      }}
    >
      <div
        style={{
          flexShrink: 0,
          minHeight: 40,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 8,
          padding: "0 8px",
          borderBottom: "1px solid var(--border-dim)",
          background: "color-mix(in srgb, var(--bg-sidebar) 94%, transparent)",
        }}
      >
        <div
          role="tablist"
          aria-label="IDE panels"
          style={{
            minWidth: 0,
            display: "flex",
            alignItems: "center",
            gap: 2,
            overflowX: "auto",
          }}
        >
          {tools.map((tool) => {
            const active = tool.panel === activePanel;
            return (
              <button
                key={tool.id}
                type="button"
                role="tab"
                aria-selected={active}
                title={t(tool.titleKey)}
                onClick={() => onSelectPanel(tool.panel)}
                style={{
                  height: 30,
                  minWidth: 34,
                  maxWidth: 136,
                  border: "1px solid transparent",
                  borderRadius: 6,
                  background: active ? "var(--bg-selected)" : "transparent",
                  color: active ? "var(--text-primary)" : "var(--text-muted)",
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  gap: 6,
                  padding: "0 8px",
                  fontSize: 11.5,
                  fontWeight: active ? 700 : 600,
                  cursor: "pointer",
                  whiteSpace: "nowrap",
                  flexShrink: 0,
                }}
              >
                {renderIdeToolIcon(tool.icon, 14)}
                <span
                  style={{
                    minWidth: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                  }}
                >
                  {t(tool.titleKey)}
                </span>
              </button>
            );
          })}
        </div>
        <div
          style={{
            flexShrink: 0,
            maxWidth: 126,
            color: "var(--text-hint)",
            fontSize: 11,
            fontWeight: 700,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {activeTool ? t(activeTool.titleKey) : ""}
        </div>
      </div>
      <div style={{ flex: 1, minHeight: 0, overflow: "hidden", display: "flex" }}>{children}</div>
    </div>
  );
}

const loadFileViewer = () =>
  import("../FileViewer").then((module) => ({ default: module.FileViewer }));
const loadFileSearchDialog = () =>
  import("../file-explorer/SearchPanel").then((module) => ({
    default: module.FileSearchDialog,
  }));
const loadGitChanges = () =>
  import("../GitChanges").then((module) => ({ default: module.GitChanges }));
const loadGitHistory = () =>
  import("../GitHistory").then((module) => ({ default: module.GitHistory }));
const loadGitAdvancedPanel = () =>
  import("../git-advanced/GitAdvancedPanel").then((module) => ({
    default: module.GitAdvancedPanel,
  }));
const loadGitDiffViewer = () =>
  import("../GitDiffViewer").then((module) => ({ default: module.GitDiffViewer }));
const loadSearchPanel = () =>
  import("../search/SearchPanel").then((module) => ({ default: module.SearchPanel }));
const loadProblemsPanel = () =>
  import("../problems/ProblemsPanel").then((module) => ({ default: module.ProblemsPanel }));
const loadTestExplorerPanel = () =>
  import("../tests/TestExplorerPanel").then((module) => ({
    default: module.TestExplorerPanel,
  }));
const loadRunConfigurationsPanel = () =>
  import("../run/RunConfigurationsPanel").then((module) => ({
    default: module.RunConfigurationsPanel,
  }));
const loadWebPreviewPanel = () =>
  import("../preview/WebPreviewPanel").then((module) => ({
    default: module.WebPreviewPanel,
  }));
const loadDebugPanel = () =>
  import("../debug/DebugPanel").then((module) => ({ default: module.DebugPanel }));
const loadSshWorkspace = () =>
  import("../ssh/SshWorkspace").then((module) => ({ default: module.SshWorkspace }));
const loadSftpPanel = () =>
  import("../sftp/SftpPanel").then((module) => ({ default: module.SftpPanel }));
const loadSftpPreview = () =>
  import("../sftp/SftpPreview").then((module) => ({ default: module.SftpPreview }));
const loadDockerServiceView = () =>
  import("../docker/DockerServiceView").then((module) => ({
    default: module.DockerServiceView,
  }));
const loadDatabaseView = () =>
  import("../database/DatabaseView").then((module) => ({ default: module.DatabaseView }));
const loadNotebookPanel = () =>
  import("../notebook/NotebookPanel").then((module) => ({ default: module.NotebookPanel }));

export const FileViewer = lazy(loadFileViewer);
export const FileSearchDialog = lazy(loadFileSearchDialog);
export const GitChanges = lazy(loadGitChanges);
export const GitHistory = lazy(loadGitHistory);
export const GitAdvancedPanel = lazy(loadGitAdvancedPanel);
export const GitDiffViewer = lazy(loadGitDiffViewer);
export const SearchPanel = lazy(loadSearchPanel);
export const ProblemsPanel = lazy(loadProblemsPanel);
export const TestExplorerPanel = lazy(loadTestExplorerPanel);
export const RunConfigurationsPanel = lazy(loadRunConfigurationsPanel);
export const WebPreviewPanel = lazy(loadWebPreviewPanel);
export const DebugPanel = lazy(loadDebugPanel);
export const SshWorkspace = lazy(loadSshWorkspace);
export const SftpPanel = lazy(loadSftpPanel);
export const SftpPreview = lazy(loadSftpPreview);
export const DockerServiceView = lazy(loadDockerServiceView);
export const DatabaseView = lazy(loadDatabaseView);
export const NotebookPanel = lazy(loadNotebookPanel);

export function projectPanelFeedbackLabel(panel: ProjectPanel, t: (key: string) => string): string {
  switch (panel) {
    case "files":
      return t("toolbar.fileExplorer");
    case "git-changes":
      return t("toolbar.gitChanges");
    case "git-history":
      return t("toolbar.gitHistory");
    case "git-advanced":
      return t("gitAdvanced.title");
    case "search":
      return t("toolbar.search");
    case "problems":
      return t("problems.title");
    case "tests":
      return t("tests.title");
    case "run":
      return t("run.title");
    case "debug":
      return t("debug.title");
    case "preview":
      return t("preview.title");
    case "ssh":
      return t("ssh.title");
    case "sftp":
      return t("sftp.title");
    case "database":
      return t("database.title");
    case "docker":
      return t("docker.title");
    case "notes":
      return t("notes.title");
  }
}

export function preloadProjectPanel(panel: ProjectPanel): void {
  if (import.meta.env.MODE === "test") return;
  switch (panel) {
    case "files":
      void loadFileViewer();
      break;
    case "git-changes":
      void loadGitChanges();
      break;
    case "git-history":
      void loadGitHistory();
      break;
    case "git-advanced":
      void loadGitAdvancedPanel();
      break;
    case "search":
      void loadSearchPanel();
      break;
    case "problems":
      void loadProblemsPanel();
      break;
    case "tests":
      void loadTestExplorerPanel();
      break;
    case "run":
      void loadRunConfigurationsPanel();
      break;
    case "preview":
      void loadWebPreviewPanel();
      break;
    case "debug":
      void loadDebugPanel();
      break;
    case "ssh":
      void loadSshWorkspace();
      break;
    case "sftp":
      void loadSftpPanel();
      break;
    case "docker":
      void loadDockerServiceView();
      break;
    case "database":
      void loadDatabaseView();
      break;
    case "notes":
      void loadNotebookPanel();
      break;
  }
}

export function preloadCommonProjectPanels(): void {
  (
    [
      "git-changes",
      "git-history",
      "git-advanced",
      "search",
      "problems",
      "tests",
      "run",
      "preview",
      "debug",
      "ssh",
      "sftp",
    ] as const
  ).forEach(preloadProjectPanel);
}

export function CenterSuspenseFallback({ label }: { label: string }) {
  return (
    <div
      style={{
        flex: 1,
        minHeight: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        color: "var(--text-muted)",
        fontSize: 12,
        background: "var(--bg-panel)",
      }}
    >
      {label}
    </div>
  );
}

export function DockSuspenseFallback({ width, label }: { width: number; label: string }) {
  return (
    <div
      style={{
        width,
        minHeight: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        borderLeft: "1px solid var(--border-dim)",
        background: "var(--bg-panel)",
        color: "var(--text-muted)",
        fontSize: 12,
      }}
    >
      {label}
    </div>
  );
}
