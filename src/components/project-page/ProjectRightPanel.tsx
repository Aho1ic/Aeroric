import { Suspense, type ComponentProps, type MouseEvent, type ReactNode } from "react";

import type { IdeToolWithAvailability } from "../../plugins/ideToolRegistry";
import type { RightPanel } from "../../hooks/useProjectPanels";
import { ErrorBoundary } from "../ErrorBoundary";
import { FileExplorer } from "../FileExplorer";
import {
  DebugPanel,
  DockSuspenseFallback,
  GitAdvancedPanel,
  GitChanges,
  GitHistory,
  ProblemsPanel,
  ProjectSkillsPanel,
  RunConfigurationsPanel,
  SearchPanel,
  TestExplorerPanel,
  WebPreviewPanel,
} from "./ProjectPanelInfrastructure";

/**
 * 每个 prop 的类型都从对应面板的 props 里取,而不是在这里重新写一遍:
 * dock 里三种路径口径、两种 remote 上下文、十几个回调,手抄一遍必然抄出偏差,
 * 而偏差处会被 `any` 或结构兼容悄悄吃掉。用 `ComponentProps` 取则是面板改签名
 * 这里就编译不过。
 */
type FileExplorerProps = ComponentProps<typeof FileExplorer>;
type GitChangesProps = ComponentProps<typeof GitChanges>;
type GitHistoryProps = ComponentProps<typeof GitHistory>;
type GitAdvancedProps = ComponentProps<typeof GitAdvancedPanel>;
type SearchPanelProps = ComponentProps<typeof SearchPanel>;
type ProblemsPanelProps = ComponentProps<typeof ProblemsPanel>;
type TestExplorerProps = ComponentProps<typeof TestExplorerPanel>;
type RunPanelProps = ComponentProps<typeof RunConfigurationsPanel>;
type PreviewPanelProps = ComponentProps<typeof WebPreviewPanel>;
type DebugPanelProps = ComponentProps<typeof DebugPanel>;

interface ProjectRightPanelProps {
  /** `visibleDockPanel()` 的返回值:已经把 sftp/docker/ssh/database/notes 折成 null。 */
  visibleRightPanel: Exclude<RightPanel, "sftp" | "docker" | "ssh" | "database" | "notes">;
  effectiveRightPanelWidth: number;
  gitContextPath: string;
  fileRootPath: string;
  projectPath: string;
  projectName: string;
  currentTaskCreatedAt: GitChangesProps["currentTaskCreatedAt"];
  visible: boolean;
  activeFilePath: GitAdvancedProps["activeFilePath"];
  themeVariant: FileExplorerProps["themeVariant"];
  supportedFileContext: FileExplorerProps["remote"];
  remoteFileContext: SearchPanelProps["remote"];
  editorDebugBreakpoints: NonNullable<RunPanelProps["editorBreakpoints"]>;
  launchedRunProcess: PreviewPanelProps["runProcessTarget"];
  launchedDebugSession: DebugPanelProps["launchedSession"];
  editorTestDebugError: DebugPanelProps["externalError"];
  testRunRequest: TestExplorerProps["runRequest"];
  runDraftRequest: RunPanelProps["draftRequest"];
  debugDraftRequest: DebugPanelProps["draftRequest"];
  t: (key: string) => string;
  showActionFailure: (panel: RightPanel & string, label: string, error: unknown) => void;
  handleRightResizeStart: (event: MouseEvent) => void;
  renderTopRightIdePanelShell: (
    panel: IdeToolWithAvailability["panel"],
    children: ReactNode,
  ) => ReactNode;
  handleFileSelectWithShellMinimize: FileExplorerProps["onFileSelect"];
  handleDiffFileSelectWithCollapse: GitChangesProps["onFileSelect"];
  handleCommitSelectWithCollapse: GitHistoryProps["onCommitSelect"];
  handleCommitFileClickWithCollapse: GitHistoryProps["onFileClick"];
  openFileAtLocation: NonNullable<GitAdvancedProps["onOpenFile"]>;
  handleTextSearchMatchOpen: SearchPanelProps["onOpenMatch"];
  setFilePreviewTarget: NonNullable<FileExplorerProps["onPreviewRequest"]>;
  handleOpenDatabaseFile: NonNullable<FileExplorerProps["onOpenDatabaseFile"]>;
  handleDiagnosticOpen: ProblemsPanelProps["onOpenDiagnostic"];
  handleCreateProblemsAgentTask: ProblemsPanelProps["onCreateAgentTask"];
  setEditorDiagnostics: NonNullable<ProblemsPanelProps["onDiagnosticsChange"]>;
  handleTestFailureOpen: TestExplorerProps["onOpenFailure"];
  handleTestRunResult: TestExplorerProps["onTestRunResult"];
  handleRunDebugStarted: RunPanelProps["onDebugStarted"];
  handleRunProcessChanged: RunPanelProps["onRunProcessChanged"];
}

/** 项目右侧 dock 的 11 个面板分支。 */
export function ProjectRightPanel({
  visibleRightPanel,
  effectiveRightPanelWidth,
  gitContextPath,
  fileRootPath,
  projectPath,
  projectName,
  currentTaskCreatedAt,
  visible,
  activeFilePath,
  themeVariant,
  supportedFileContext,
  remoteFileContext,
  editorDebugBreakpoints,
  launchedRunProcess,
  launchedDebugSession,
  editorTestDebugError,
  testRunRequest,
  runDraftRequest,
  debugDraftRequest,
  t,
  showActionFailure,
  handleRightResizeStart,
  renderTopRightIdePanelShell,
  handleFileSelectWithShellMinimize,
  handleDiffFileSelectWithCollapse,
  handleCommitSelectWithCollapse,
  handleCommitFileClickWithCollapse,
  openFileAtLocation,
  handleTextSearchMatchOpen,
  setFilePreviewTarget,
  handleOpenDatabaseFile,
  handleDiagnosticOpen,
  handleCreateProblemsAgentTask,
  setEditorDiagnostics,
  handleTestFailureOpen,
  handleTestRunResult,
  handleRunDebugStarted,
  handleRunProcessChanged,
}: ProjectRightPanelProps) {
  return (
    <>
      {visibleRightPanel && (
        <div
          style={{
            position: "relative",
            display: visibleRightPanel ? "flex" : "none",
            flexShrink: 0,
          }}
        >
          <div
            onMouseDown={handleRightResizeStart}
            style={{
              position: "absolute",
              left: 0,
              top: 0,
              bottom: 0,
              width: 5,
              cursor: "col-resize",
              zIndex: 10,
            }}
          />
          <Suspense
            fallback={
              <DockSuspenseFallback width={effectiveRightPanelWidth} label={t("common.loading")} />
            }
          >
            {visibleRightPanel === "files" && (
              <ErrorBoundary
                label="文件浏览器"
                onError={(error) => showActionFailure("files", t("toolbar.fileExplorer"), error)}
              >
                <FileExplorer
                  projectPath={fileRootPath}
                  projectName={projectName}
                  onFileSelect={handleFileSelectWithShellMinimize}
                  active={visible}
                  width={effectiveRightPanelWidth}
                  remote={supportedFileContext}
                  themeVariant={themeVariant}
                  onPreviewRequest={setFilePreviewTarget}
                  onOpenDatabaseFile={handleOpenDatabaseFile}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "git-changes" && (
              <ErrorBoundary
                label="Git 变更"
                onError={(error) =>
                  showActionFailure("git-changes", t("toolbar.gitChanges"), error)
                }
              >
                <GitChanges
                  projectPath={gitContextPath}
                  currentTaskCreatedAt={currentTaskCreatedAt}
                  onFileSelect={handleDiffFileSelectWithCollapse}
                  width={effectiveRightPanelWidth}
                  remote={supportedFileContext}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "git-history" && (
              <ErrorBoundary
                label="Git 历史"
                onError={(error) =>
                  showActionFailure("git-history", t("toolbar.gitHistory"), error)
                }
              >
                <GitHistory
                  projectPath={gitContextPath}
                  onCommitSelect={handleCommitSelectWithCollapse}
                  onFileClick={handleCommitFileClickWithCollapse}
                  width={effectiveRightPanelWidth}
                  remote={supportedFileContext}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "git-advanced" && (
              <ErrorBoundary
                label="Git Advanced"
                onError={(error) =>
                  showActionFailure("git-advanced", t("gitAdvanced.title"), error)
                }
              >
                <GitAdvancedPanel
                  projectPath={gitContextPath}
                  activeFilePath={activeFilePath}
                  width={effectiveRightPanelWidth}
                  onOpenFile={openFileAtLocation}
                  remote={supportedFileContext}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "search" && (
              <ErrorBoundary
                label="搜索"
                onError={(error) => showActionFailure("search", t("toolbar.search"), error)}
              >
                <SearchPanel
                  projectPath={fileRootPath}
                  width={effectiveRightPanelWidth}
                  onOpenMatch={handleTextSearchMatchOpen}
                  remote={remoteFileContext}
                />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "skills" && (
              <ErrorBoundary
                label="Skills"
                onError={(error) => showActionFailure("skills", t("skills.installedSkills"), error)}
              >
                <ProjectSkillsPanel projectPath={fileRootPath} width={effectiveRightPanelWidth} />
              </ErrorBoundary>
            )}
            {visibleRightPanel === "problems" && (
              <>
                {renderTopRightIdePanelShell(
                  "problems",
                  <ErrorBoundary
                    label="Problems"
                    onError={(error) => showActionFailure("problems", t("problems.title"), error)}
                  >
                    <ProblemsPanel
                      projectPath={projectPath}
                      width={effectiveRightPanelWidth}
                      onOpenDiagnostic={handleDiagnosticOpen}
                      onCreateAgentTask={handleCreateProblemsAgentTask}
                      onDiagnosticsChange={remoteFileContext ? undefined : setEditorDiagnostics}
                      remote={remoteFileContext}
                    />
                  </ErrorBoundary>,
                )}
              </>
            )}
            {visibleRightPanel === "tests" && (
              <>
                {renderTopRightIdePanelShell(
                  "tests",
                  <ErrorBoundary
                    label="Tests"
                    onError={(error) => showActionFailure("tests", t("tests.title"), error)}
                  >
                    <TestExplorerPanel
                      projectPath={projectPath}
                      width={effectiveRightPanelWidth}
                      onOpenFailure={handleTestFailureOpen}
                      onCreateAgentTask={handleCreateProblemsAgentTask}
                      onTestRunResult={handleTestRunResult}
                      runRequest={testRunRequest}
                      remote={remoteFileContext}
                    />
                  </ErrorBoundary>,
                )}
              </>
            )}
            {visibleRightPanel === "run" && (
              <>
                {renderTopRightIdePanelShell(
                  "run",
                  <ErrorBoundary
                    label="Run"
                    onError={(error) => showActionFailure("run", t("run.title"), error)}
                  >
                    <RunConfigurationsPanel
                      projectPath={fileRootPath}
                      width={effectiveRightPanelWidth}
                      editorBreakpoints={remoteFileContext ? [] : editorDebugBreakpoints}
                      onDebugStarted={handleRunDebugStarted}
                      onRunProcessChanged={handleRunProcessChanged}
                      draftRequest={runDraftRequest}
                      remote={remoteFileContext}
                    />
                  </ErrorBoundary>,
                )}
              </>
            )}
            {visibleRightPanel === "preview" && (
              <>
                {renderTopRightIdePanelShell(
                  "preview",
                  <ErrorBoundary
                    label="Preview"
                    onError={(error) => showActionFailure("preview", t("preview.title"), error)}
                  >
                    <WebPreviewPanel
                      projectPath={fileRootPath}
                      width={effectiveRightPanelWidth}
                      runProcessTarget={launchedRunProcess}
                      remote={remoteFileContext}
                    />
                  </ErrorBoundary>,
                )}
              </>
            )}
            {visibleRightPanel === "debug" && (
              <>
                {renderTopRightIdePanelShell(
                  "debug",
                  <ErrorBoundary
                    label="Debug"
                    onError={(error) => showActionFailure("debug", t("debug.title"), error)}
                  >
                    <DebugPanel
                      projectPath={fileRootPath}
                      width={effectiveRightPanelWidth}
                      onOpenLocation={openFileAtLocation}
                      launchedSession={launchedDebugSession}
                      editorBreakpoints={remoteFileContext ? [] : editorDebugBreakpoints}
                      externalError={editorTestDebugError}
                      draftRequest={debugDraftRequest}
                      remote={remoteFileContext}
                    />
                  </ErrorBoundary>,
                )}
              </>
            )}
          </Suspense>
        </div>
      )}
    </>
  );
}
