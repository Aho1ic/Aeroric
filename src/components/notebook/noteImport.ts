/* 从第三方笔记应用导入:provider 清单 + 一次导入的编排。
 *
 * 和 UI 分开的理由和 `noteExport.ts` 一样 —— 「选什么源、调哪个命令、报告怎么读成
 * 一句话」全都能在没有 DOM 的情况下测,组件只负责画。
 *
 * 两处容易写错的地方,都在类型里钉死了:
 *
 * 1. **源的形态每个 provider 不同**,不是「都选一个文件」。Obsidian / Logseq 要的是
 *    目录,Notion / Bear / Roam 要 zip,Evernote 要 `.enex`,而 Apple Notes **没有源
 *    路径** —— 它走 osascript 问 Notes.app。所以 `source` 是个 union,不是
 *    `string | null` 加一个布尔标记。
 *
 * 2. **报告的计数不构成划分**。`resourceLost` / `degraded` 是跨状态的「受影响条目数」,
 *    一条 `imported` 也可以算在里面。加到总数里会得出一个比真实条目数还大的数字。
 */

import { APP_PLATFORM } from "../../platform";
import {
  importAppleNotes,
  importBear,
  importEvernote,
  importLogseq,
  importNotion,
  importObsidian,
  importRoam,
  type ImportReport,
} from "./notebookApi";

export type ImportProviderId =
  | "obsidian"
  | "logseq"
  | "notion"
  | "bear"
  | "roam"
  | "evernote"
  | "appleNotes";

/** 源的形态。决定点下去先弹什么(选目录 / 选文件 / 什么都不弹)。 */
export type ImportSource =
  | { kind: "directory" }
  /** `extensions` 直接进文件对话框的 filter。 */
  | { kind: "file"; extensions: readonly string[] }
  /** 没有源路径 —— 从系统应用里读(Apple Notes)。 */
  | { kind: "system" };

export type ImportProvider = {
  id: ImportProviderId;
  /** i18n key,不是文案本身。 */
  labelKey: string;
  hintKey: string;
  source: ImportSource;
  /** 只在这些平台可用。undefined = 全平台。 */
  platforms?: readonly ("windows" | "macos" | "other")[];
};

/** 顺序即 UI 里的顺序:目录型在前(最常见),系统型垫底。 */
export const IMPORT_PROVIDERS: readonly ImportProvider[] = [
  {
    id: "obsidian",
    labelKey: "notebook.importObsidian",
    hintKey: "notebook.importObsidianHint",
    source: { kind: "directory" },
  },
  {
    id: "logseq",
    labelKey: "notebook.importLogseq",
    hintKey: "notebook.importLogseqHint",
    source: { kind: "directory" },
  },
  {
    id: "notion",
    labelKey: "notebook.importNotion",
    hintKey: "notebook.importNotionHint",
    source: { kind: "file", extensions: ["zip"] },
  },
  {
    id: "bear",
    labelKey: "notebook.importBear",
    hintKey: "notebook.importBearHint",
    source: { kind: "file", extensions: ["zip"] },
  },
  {
    id: "roam",
    labelKey: "notebook.importRoam",
    hintKey: "notebook.importRoamHint",
    // zip,不是 json:Roam 的两种导出都打在 zip 里,后端在归档内按扩展名分路。
    source: { kind: "file", extensions: ["zip"] },
  },
  {
    id: "evernote",
    labelKey: "notebook.importEvernote",
    hintKey: "notebook.importEvernoteHint",
    source: { kind: "file", extensions: ["enex"] },
  },
  {
    id: "appleNotes",
    labelKey: "notebook.importAppleNotes",
    hintKey: "notebook.importAppleNotesHint",
    source: { kind: "system" },
    // 后端在非 macOS 上直接返回错误。这里也挡一道,免得画出一个必然失败的按钮。
    platforms: ["macos"],
  },
];

export function isProviderAvailable(
  provider: ImportProvider,
  platform: string = APP_PLATFORM,
): boolean {
  if (!provider.platforms) return true;
  return provider.platforms.some((candidate) => candidate === platform);
}

/** 当前平台可用的 provider。UI 用这个,不要自己过滤。 */
export function availableImportProviders(
  platform: string = APP_PLATFORM,
): readonly ImportProvider[] {
  return IMPORT_PROVIDERS.filter((provider) => isProviderAvailable(provider, platform));
}

/** 一次导入的结果。`cancelled` 是用户在文件对话框里取消 —— 既不是成功也不是失败。 */
export type ImportOutcome =
  | { status: "done"; report: ImportReport }
  | { status: "cancelled" }
  | { status: "failed"; message: string };

/** 可注入的依赖。测试传假的,正常调用走真实对话框和真实命令。 */
export type ImportDeps = {
  pickDirectory: (title: string) => Promise<string | null>;
  pickFile: (title: string, extensions: readonly string[]) => Promise<string | null>;
  run: (id: ImportProviderId, vault: string, sourcePath: string | null) => Promise<ImportReport>;
};

export function defaultImportDeps(): ImportDeps {
  return {
    pickDirectory: async (title) => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const target = await open({ directory: true, multiple: false, title });
      // multiple: false 下返回单个路径,但类型里仍带着数组分支。
      return typeof target === "string" ? target : null;
    },
    pickFile: async (title, extensions) => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const target = await open({
        directory: false,
        multiple: false,
        title,
        filters: [{ name: extensions.join("/").toUpperCase(), extensions: [...extensions] }],
      });
      return typeof target === "string" ? target : null;
    },
    run: (id, vault, sourcePath) => runProviderCommand(id, vault, sourcePath),
  };
}

/**
 * provider id → 后端命令。
 *
 * `sourcePath` 为 null 只有 `appleNotes` 合法。别的 provider 走到这里还是 null,说明
 * 上游漏了取消分支 —— 那时候宁可抛,也不要拿空字符串去调后端:后端会把它当成一个
 * 相对路径去解析,报出来的错和真正的原因无关。
 *
 * 导出只为测试。`runImport` 的取消分支已经在它前面拦住了 null,所以这个 throw 通过
 * UI 那条路走不到 —— 不导出的话它就是一段谁都观察不到的防御代码(变异掉它测试全绿,
 * 验过)。导出之后这条契约有了直接的断言,将来有人把 `deps.run` 接到别处也仍然成立。
 */
export function runProviderCommand(
  id: ImportProviderId,
  vault: string,
  sourcePath: string | null,
): Promise<ImportReport> {
  if (id === "appleNotes") return importAppleNotes(vault);
  if (sourcePath === null) {
    throw new Error(`import provider ${id} requires a source path`);
  }
  switch (id) {
    case "obsidian":
      return importObsidian(vault, sourcePath);
    case "logseq":
      return importLogseq(vault, sourcePath);
    case "notion":
      return importNotion(vault, sourcePath);
    case "bear":
      return importBear(vault, sourcePath);
    case "roam":
      return importRoam(vault, sourcePath);
    case "evernote":
      return importEvernote(vault, sourcePath);
  }
}

/** 跑一次导入:按 provider 的源形态弹对话框,然后调后端。 */
export async function runImport(
  provider: ImportProvider,
  vault: string,
  deps: ImportDeps,
  t: (key: string, vars?: Record<string, string>) => string,
): Promise<ImportOutcome> {
  let sourcePath: string | null = null;
  const pickTitle = t("notebook.importPick", { provider: t(provider.labelKey) });

  try {
    if (provider.source.kind === "directory") {
      sourcePath = await deps.pickDirectory(pickTitle);
      if (sourcePath === null) return { status: "cancelled" };
    } else if (provider.source.kind === "file") {
      sourcePath = await deps.pickFile(pickTitle, provider.source.extensions);
      if (sourcePath === null) return { status: "cancelled" };
    }
  } catch (error) {
    return { status: "failed", message: errorText(error) };
  }

  try {
    const report = await deps.run(provider.id, vault, sourcePath);
    return { status: "done", report };
  } catch (error) {
    return { status: "failed", message: errorText(error) };
  }
}

/**
 * 报告的一句话摘要。
 *
 * 只在这里拼,因为「哪些数字能相加」是个反复会写错的点:`imported` / `skipped` /
 * `failed` 是划分,可以加;`resourceLost` / `degraded` 是跨状态计数,**不能**加进去。
 * 后两个只在非零时各自追一句。
 */
export function importSummary(
  report: ImportReport,
  t: (key: string, vars?: Record<string, string>) => string,
): string {
  const parts = [
    t("notebook.importSummaryCounts", {
      imported: String(report.imported),
      skipped: String(report.skipped),
      failed: String(report.failed),
    }),
  ];
  if (report.resourceLost > 0) {
    parts.push(t("notebook.importSummaryResourceLost", { count: String(report.resourceLost) }));
  }
  if (report.degraded > 0) {
    parts.push(t("notebook.importSummaryDegraded", { count: String(report.degraded) }));
  }
  if (report.truncated > 0) {
    parts.push(t("notebook.importSummaryTruncated", { count: String(report.truncated) }));
  }
  return parts.join(" · ");
}

/** 明细里一条的状态文案。 */
export function importStatusText(
  status: ImportReport["items"][number]["status"],
  t: (key: string, vars?: Record<string, string>) => string,
): string {
  switch (status.kind) {
    case "imported":
      return t("notebook.importStatusImported");
    case "failed":
      return t("notebook.importStatusFailed", { detail: status.detail });
    case "skipped":
      return t("notebook.importStatusSkipped", { reason: skipReasonText(status.reason, t) });
  }
}

function skipReasonText(
  reason: Extract<ImportReport["items"][number]["status"], { kind: "skipped" }>["reason"],
  t: (key: string, vars?: Record<string, string>) => string,
): string {
  switch (reason.kind) {
    case "alreadyImported":
      return t("notebook.importSkipAlreadyImported");
    case "unsupported":
      return t("notebook.importSkipUnsupported", { extension: reason.extension });
    case "tooLarge":
      return t("notebook.importSkipTooLarge", { bytes: String(reason.bytes) });
    case "limitReached":
      return t("notebook.importSkipLimitReached", { limit: reason.limit });
    case "unreadable":
      return t("notebook.importSkipUnreadable", { detail: reason.detail });
    case "symlink":
      return t("notebook.importSkipSymlink");
  }
}

/** 明细里一条的 issue 文案。 */
export function importIssueText(
  issue: NonNullable<ImportReport["items"][number]["issues"]>[number],
  t: (key: string, vars?: Record<string, string>) => string,
): string {
  switch (issue.kind) {
    case "resourceLost":
      return t("notebook.importIssueResourceLost", {
        target: issue.target,
        detail: issue.detail,
      });
    case "degraded":
      return t("notebook.importIssueDegraded", { detail: issue.detail });
  }
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
