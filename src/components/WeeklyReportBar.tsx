/* 周报生成入口。
 *
 * 挂在首页 timeline 视图上方:那里已经有跨项目全量 `tasks` 与 `allProjects`,是唯一
 * 不需要额外取数的位置。
 *
 * 组件自己 `invoke("load_app_settings")` 读区间与输出目录 —— 走 prop drilling 要串
 * 五层组件,而设置本来就在 Rust 侧。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { FileText, Loader2 } from "lucide-react";
import type { Project, ProtocolFamily, Task } from "../types";
import { useI18n } from "../i18n";
import { useToast } from "./Toast";
import { taskTitle } from "./TimelineView";
import { resolveTaskSessionOwner, getTaskSessionFieldsByFamily } from "../taskSession";
import { pickExportDir } from "./notebook/noteExport";
import { APP_SETTINGS_CHANGED_EVENT, normalizeWeeklyReportSettings } from "./app-settings/types";
import type { AppSettings, WeeklyReportSettings } from "./app-settings/types";
import {
  tasksInWindow,
  weekLabel,
  weekWindow,
  weeklyReportFileName,
  type WeekWindow,
} from "../weeklyReport";

const PROGRESS_EVENT = "weekly-report-progress";

interface WeeklyReportProgress {
  scope: string;
  completed: number;
  total: number;
  currentTitle: string;
}

/** 一条任务在 Rust 侧的载荷形状。字段名 camelCase,与 `WeeklyReportTask` 的 serde 一致。 */
interface WeeklyReportTaskPayload {
  projectName: string;
  projectPath: string;
  title: string;
  agent: string;
  status: string;
  createdAt: number;
  completedAt?: number;
  failureReason?: string;
  sessionPath?: string;
  /** 会话协议族。Rust 侧用它选路径校验的布局根 —— 布尔会把 dsh/omp 一起压成 claude。 */
  sessionFamily: ProtocolFamily;
  additions?: number;
  deletions?: number;
}

function buildTaskPayload(task: Task, projects: Project[]): WeeklyReportTaskPayload {
  const project = projects.find((item) => item.id === task.projectId);
  const owner = resolveTaskSessionOwner(task);
  const fields = getTaskSessionFieldsByFamily(task, owner.family);
  return {
    projectName: project?.name ?? task.projectId,
    // worktree 任务的会话属于 worktree 目录,不是项目根;传错 Rust 侧的路径校验会拒读。
    projectPath: task.worktreePath ?? project?.path ?? "",
    title: taskTitle(task),
    agent: task.agent,
    status: task.status,
    createdAt: task.createdAt,
    completedAt: task.completedAt,
    failureReason: task.failureReason,
    sessionPath: fields.sessionPath,
    sessionFamily: owner.family,
    additions: task.additions,
    deletions: task.deletions,
  };
}

export function WeeklyReportBar({ tasks, projects }: { tasks: Task[]; projects: Project[] }) {
  const { language, t } = useI18n();
  const { showToast } = useToast();
  const [settings, setSettings] = useState<WeeklyReportSettings | null>(null);
  const [progress, setProgress] = useState<WeeklyReportProgress | null>(null);
  const [running, setRunning] = useState(false);
  // 生成中途组件被卸载(用户切走视图)后不该再 setState。
  const activeRef = useRef(true);

  useEffect(() => {
    activeRef.current = true;
    return () => {
      activeRef.current = false;
    };
  }, []);

  const loadSettings = useCallback(() => {
    void invoke<AppSettings>("load_app_settings")
      .then((value) => {
        if (activeRef.current)
          setSettings(normalizeWeeklyReportSettings(value.weekly_report_settings));
      })
      .catch(() => {
        // 读不到就用默认区间:周报比"因为读设置失败而没有按钮"有用。
        if (activeRef.current) setSettings(normalizeWeeklyReportSettings(undefined));
      });
  }, []);

  useEffect(() => {
    loadSettings();
    window.addEventListener(APP_SETTINGS_CHANGED_EVENT, loadSettings);
    return () => window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, loadSettings);
  }, [loadSettings]);

  useEffect(() => {
    const subscription = listen<WeeklyReportProgress>(PROGRESS_EVENT, (event) => {
      if (activeRef.current) setProgress(event.payload);
    });
    return () => {
      void subscription.then((unlisten) => unlisten());
    };
  }, []);

  const generate = useCallback(async () => {
    if (running || !settings) return;
    const window_: WeekWindow = weekWindow(
      Date.now(),
      settings.week_start_day,
      settings.week_end_day,
    );
    const inWindow = tasksInWindow(tasks, window_);
    if (inWindow.length === 0) {
      showToast(t("report.empty"), "warning");
      return;
    }

    let outputDir = settings.output_dir;
    if (!outputDir) {
      const picked = await pickExportDir(t("report.pickOutputDir"));
      if (!picked) return;
      outputDir = picked;
    }

    setRunning(true);
    setProgress(null);
    try {
      const savedPath = await invoke<string>("generate_weekly_report", {
        request: {
          fromMs: window_.from,
          toMs: window_.to,
          weekLabel: weekLabel(window_),
          locale: language,
          outputPath: `${outputDir}/${weeklyReportFileName(window_)}`,
          tasks: inWindow.map((task) => buildTaskPayload(task, projects)),
        },
      });
      showToast(t("report.success", { path: savedPath }), "success");
    } catch (error: unknown) {
      showToast(t("report.failed", { error: String(error) }), "error");
    } finally {
      if (activeRef.current) {
        setRunning(false);
        setProgress(null);
      }
    }
  }, [language, projects, running, settings, showToast, t, tasks]);

  const label = running
    ? progress
      ? t("report.generating", { completed: progress.completed, total: progress.total })
      : t("report.generating", { completed: 0, total: 0 })
    : t("report.generate");

  return (
    <div
      style={{
        display: "flex",
        justifyContent: "flex-end",
        alignItems: "center",
        padding: "0 4px 8px",
      }}
    >
      <button
        type="button"
        disabled={running || !settings}
        title={t("report.generate")}
        onClick={() => void generate()}
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: 6,
          height: 28,
          padding: "0 12px",
          border: "1px solid var(--border-medium)",
          borderRadius: 6,
          background: "var(--bg-card)",
          color: "var(--text-primary)",
          fontFamily: "var(--font-ui)",
          fontSize: 11.5,
          fontWeight: 600,
          cursor: running ? "default" : "pointer",
          opacity: running ? 0.7 : 1,
        }}
      >
        {running ? (
          <Loader2 size={12} strokeWidth={2.2} className="spin" />
        ) : (
          <FileText size={12} strokeWidth={2.2} />
        )}
        <span>{label}</span>
      </button>
    </div>
  );
}
