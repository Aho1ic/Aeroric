import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import type {
  DebugBreakpoint,
  DebugSessionSnapshot,
  RunProcessSnapshot,
  SshConnection,
  TestCoverageSummary,
  TestRunResult,
} from "../types";
import {
  debugBreakpointFileForProject,
  toggleLineDebugBreakpoint,
} from "../components/debug/debugBreakpointState";
import type { DebugConfigDraft } from "../components/debug/debugState";
import type { EditorTestRunTarget } from "../components/file-viewer/testRunGutter";
import { extractRunPreviewCandidates } from "../components/preview/portPanelState";
import type { RunConfigDraft } from "../components/run/runConfigState";
import { buildVitestDebugConfig } from "../components/tests/testDebugState";
import type { TestRunPanelRequest } from "../components/tests/testExplorerState";
import type { RightPanel } from "./useProjectPanels";

/**
 * 编辑器侧 run / debug / test 这一簇状态。
 *
 * 这不是纯搬移:每个 handler 都同时改「这簇状态」和「导航」。导航是页面级的
 * (`openRightPanel` 与终端可见性在页面里另有十几个读写点),所以导航以参数注入,
 * 不搬进来;搬进来的是状态、三个自增 id、以及换项目时的重置。
 *
 * 之所以值得聚:这簇有真实约束 —— 换项目时断点/会话/覆盖率必须一起清,
 * 断点里带着旧项目的绝对路径,留到新项目就是指向不存在的文件。
 */
export interface EditorRunDebugNavigation {
  /** 打开右侧面板。页面级导航,注入而非搬入。 */
  openRightPanel: (panel: Exclude<RightPanel, null>) => void;
  /**
   * 收起本地 shell 终端。
   *
   * 必须是 identity 稳定的回调(页面侧用 `useCallback(…, [])` 包一层):
   * 它进了下面几个 handler 的依赖数组,每渲染换一个新函数会让 handler 的
   * identity 跟着变,把 memo 化的面板全部带着重渲染。
   */
  hideShellTerminal: () => void;
}

export interface EditorRunDebugOptions extends EditorRunDebugNavigation {
  /** 项目根路径。断点路径按它归一化,换它就重置整簇状态。 */
  projectPath: string;
  /** 文件树根路径:远端项目下与 projectPath 不同口径。 */
  fileRootPath: string;
  /** 远端上下文;非空时调远端命令、并按远端根路径构造调试配置。 */
  remoteFileContext: { connection: SshConnection; projectPath: string } | undefined;
}

export interface EditorRunDebugState {
  launchedDebugSession: DebugSessionSnapshot | null;
  launchedRunProcess: RunProcessSnapshot | null;
  editorDebugBreakpoints: DebugBreakpoint[];
  editorCoverage: TestCoverageSummary | null;
  testRunRequest: TestRunPanelRequest | null;
  runDraftRequest: { id: number; draft: RunConfigDraft } | null;
  debugDraftRequest: { id: number; draft: DebugConfigDraft } | null;
  editorTestDebugError: string | null;
  handleRunDebugStarted: (snapshot: DebugSessionSnapshot) => void;
  handleRunProcessChanged: (snapshot: RunProcessSnapshot) => void;
  handleToggleEditorDebugBreakpoint: (filePath: string, line: number) => void;
  handleRunEditorTestTarget: (target: EditorTestRunTarget) => void;
  handleTestRunResult: (result: TestRunResult) => void;
  handleDebugEditorTestTarget: (target: EditorTestRunTarget) => Promise<void>;
  /** 派一次测试运行;id 由这里自增,调用方只给请求内容。 */
  requestTestRun: (request: Omit<TestRunPanelRequest, "id">) => void;
  /** 派一次 run 配置草稿;竞态守卫在这里,调用方只给「怎么取草稿」。 */
  requestRunDraft: (load: () => Promise<RunConfigDraft | null>) => void;
  requestDebugDraft: (draft: DebugConfigDraft) => void;
}

export function useEditorRunDebugState({
  projectPath,
  fileRootPath,
  remoteFileContext,
  openRightPanel,
  hideShellTerminal,
}: EditorRunDebugOptions): EditorRunDebugState {
  const [launchedDebugSession, setLaunchedDebugSession] = useState<DebugSessionSnapshot | null>(
    null,
  );
  const [launchedRunProcess, setLaunchedRunProcess] = useState<RunProcessSnapshot | null>(null);
  const [editorDebugBreakpoints, setEditorDebugBreakpoints] = useState<DebugBreakpoint[]>([]);
  const [editorCoverage, setEditorCoverage] = useState<TestCoverageSummary | null>(null);
  const [testRunRequest, setTestRunRequest] = useState<TestRunPanelRequest | null>(null);
  const [runDraftRequest, setRunDraftRequest] = useState<{
    id: number;
    draft: RunConfigDraft;
  } | null>(null);
  const [debugDraftRequest, setDebugDraftRequest] = useState<{
    id: number;
    draft: DebugConfigDraft;
  } | null>(null);
  const [editorTestDebugError, setEditorTestDebugError] = useState<string | null>(null);

  /** 已经为哪个 runId 自动开过预览。是 ref 不是 state:它不参与渲染,只防重复。 */
  const previewOpenedForRunRef = useRef<string | null>(null);
  const testRunRequestIdRef = useRef(0);
  const runDraftRequestIdRef = useRef(0);
  const debugDraftRequestIdRef = useRef(0);

  useEffect(() => {
    setLaunchedDebugSession(null);
    setLaunchedRunProcess(null);
    previewOpenedForRunRef.current = null;
    setEditorDebugBreakpoints([]);
    setEditorCoverage(null);
  }, [projectPath]);

  const handleRunDebugStarted = useCallback(
    (snapshot: DebugSessionSnapshot) => {
      hideShellTerminal();
      setLaunchedDebugSession(snapshot);
      openRightPanel("debug");
    },
    [hideShellTerminal, openRightPanel],
  );

  const handleRunProcessChanged = useCallback(
    (snapshot: RunProcessSnapshot) => {
      setLaunchedRunProcess(snapshot);
      if (
        snapshot.status === "running" &&
        previewOpenedForRunRef.current !== snapshot.runId &&
        extractRunPreviewCandidates(snapshot).length > 0
      ) {
        previewOpenedForRunRef.current = snapshot.runId;
        hideShellTerminal();
        openRightPanel("preview");
      }
    },
    [hideShellTerminal, openRightPanel],
  );

  const handleToggleEditorDebugBreakpoint = useCallback(
    (filePath: string, line: number) => {
      setEditorDebugBreakpoints((prev) =>
        toggleLineDebugBreakpoint(prev, {
          file: debugBreakpointFileForProject(projectPath, filePath),
          line,
          column: 1,
        }),
      );
    },
    [projectPath],
  );

  const requestTestRun = useCallback((request: Omit<TestRunPanelRequest, "id">) => {
    testRunRequestIdRef.current += 1;
    setTestRunRequest({ ...request, id: testRunRequestIdRef.current });
  }, []);

  const requestRunDraft = useCallback((load: () => Promise<RunConfigDraft | null>) => {
    const requestId = runDraftRequestIdRef.current + 1;
    runDraftRequestIdRef.current = requestId;
    void load().then((draft) => {
      // 取草稿是异步的:等回来时可能已经派过更新的一次请求,那就丢掉这次结果。
      if (draft && runDraftRequestIdRef.current === requestId) {
        setRunDraftRequest({ id: requestId, draft });
      }
    });
  }, []);

  const requestDebugDraft = useCallback((draft: DebugConfigDraft) => {
    debugDraftRequestIdRef.current += 1;
    setDebugDraftRequest({ id: debugDraftRequestIdRef.current, draft });
  }, []);

  const handleRunEditorTestTarget = useCallback(
    (target: EditorTestRunTarget) => {
      setEditorTestDebugError(null);
      hideShellTerminal();
      requestTestRun({
        profile: "vitest",
        target: { filePath: target.filePath, testName: target.testName },
        coverage: false,
      });
      openRightPanel("tests");
    },
    [hideShellTerminal, openRightPanel, requestTestRun],
  );

  const handleTestRunResult = useCallback((result: TestRunResult) => {
    setEditorCoverage(result.coverage ?? null);
  }, []);

  const handleDebugEditorTestTarget = useCallback(
    async (target: EditorTestRunTarget) => {
      setEditorTestDebugError(null);
      hideShellTerminal();
      try {
        const commandArgs = remoteFileContext
          ? {
              connection: remoteFileContext.connection,
              remoteProjectPath: remoteFileContext.projectPath,
              projectPath: fileRootPath,
              config: buildVitestDebugConfig(fileRootPath, target),
            }
          : {
              projectPath,
              config: buildVitestDebugConfig(projectPath, target),
            };
        const snapshot = await invoke<DebugSessionSnapshot>(
          remoteFileContext ? "remote_start_debug_config" : "start_debug_config",
          commandArgs,
        );
        handleRunDebugStarted(snapshot);
      } catch (err) {
        setEditorTestDebugError(String(err));
        openRightPanel("debug");
      }
    },
    [
      fileRootPath,
      handleRunDebugStarted,
      hideShellTerminal,
      openRightPanel,
      projectPath,
      remoteFileContext,
    ],
  );

  return {
    launchedDebugSession,
    launchedRunProcess,
    editorDebugBreakpoints,
    editorCoverage,
    testRunRequest,
    runDraftRequest,
    debugDraftRequest,
    editorTestDebugError,
    handleRunDebugStarted,
    handleRunProcessChanged,
    handleToggleEditorDebugBreakpoint,
    handleRunEditorTestTarget,
    handleTestRunResult,
    handleDebugEditorTestTarget,
    requestTestRun,
    requestRunDraft,
    requestDebugDraft,
  };
}
