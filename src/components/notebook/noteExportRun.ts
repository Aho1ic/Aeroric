/**
 * 导出动作 → 结果文案。
 *
 * 把「跑哪条导出」和「跑完之后说什么」收在一处,面板只管存 state 和画 UI。这样文案
 * 分支(取消 / 部分失败 / 有图没内联)能被单测直接盖住,不用挂一整个面板。
 */

import {
  copyAsHtml,
  copyAsMarkdown,
  exportAsHtml,
  exportAsMarkdown,
  exportAsPdf,
  type ExportDeps,
  type ExportOutcome,
  type ExportSource,
} from "./noteExport";
import { runSiteExport, type SiteExportDeps, type SiteExportProgress } from "./noteSiteExportRun";

/** 导出面板能发起的动作。 */
export type ExportAction = "pdf" | "html" | "markdown" | "copyHtml" | "copyMarkdown" | "site";

/** 单篇导出的五项。`site` 不在里面 —— 它和当前笔记无关。 */
export const SINGLE_ACTIONS: readonly ExportAction[] = [
  "pdf",
  "html",
  "markdown",
  "copyHtml",
  "copyMarkdown",
];

export type Translate = (key: string, vars?: Record<string, string>) => string;

/** 一次导出跑完之后要显示什么。两个都为 null = 什么都不说(用户取消了)。 */
export type ExportRunOutcome = {
  notice: string | null;
  error: string | null;
};

const NOTHING: ExportRunOutcome = { notice: null, error: null };

/**
 * 把成功文案和「有图没内联」的提醒拼起来。
 *
 * 拼成两行而不是丢掉后半句:图片没内联的导出物看起来是好的,只有换台机器打开才会发现
 * 图裂了 —— 这件事必须当场说。
 */
function withSkipped(base: string | null, skipped: number, t: Translate): string | null {
  if (skipped <= 0) return base;
  const warning = t("notebook.exportImagesSkipped", { count: String(skipped) });
  return base ? `${base}\n${warning}` : warning;
}

/** 跑一条单篇导出。`source` 为 null 时报「没有可导出的笔记」。 */
export async function runSingleExport(
  action: ExportAction,
  source: ExportSource | null,
  deps: ExportDeps,
  t: Translate,
): Promise<ExportRunOutcome> {
  if (!source) return { notice: null, error: t("notebook.exportNoNote") };
  try {
    const outcome = await runOne(action, source, deps);
    // 用户在保存对话框里取消:不是失败,也不需要报「已导出」。
    if (outcome.cancelled) return withImagesOnly(outcome, t);
    if (outcome.path) {
      return {
        notice: withSkipped(
          t("notebook.exportDone", { path: outcome.path }),
          outcome.images.skipped,
          t,
        ),
        error: null,
      };
    }
    if (action === "pdf") {
      /* PDF 没有「已导出到…」可说 —— 落盘发生在系统打印对话框里,路径由用户在那边定,
         我们拿不到。打印对话框本身就是反馈,所以这里只在有图没内联时才出声。 */
      return withImagesOnly(outcome, t);
    }
    return {
      notice: withSkipped(t("notebook.exportCopied"), outcome.images.skipped, t),
      error: null,
    };
  } catch (error) {
    return { notice: null, error: t("notebook.exportFailed", { message: messageOf(error) }) };
  }
}

/** 取消 / PDF 这两条路径上,只有「图没内联」值得说。 */
function withImagesOnly(outcome: ExportOutcome, t: Translate): ExportRunOutcome {
  const notice = withSkipped(null, outcome.images.skipped, t);
  return notice ? { notice, error: null } : NOTHING;
}

function runOne(
  action: ExportAction,
  source: ExportSource,
  deps: ExportDeps,
): Promise<ExportOutcome> {
  switch (action) {
    case "pdf":
      return exportAsPdf(source, deps);
    case "html":
      return exportAsHtml(source, deps);
    case "markdown":
      return exportAsMarkdown(source, deps);
    case "copyHtml":
      return copyAsHtml(source, deps);
    case "copyMarkdown":
      return copyAsMarkdown(source, deps);
    case "site":
      // 不该走到这里:`site` 由 `runSiteExportAction` 处理。
      return Promise.reject(new Error("site export is not a single-note action"));
  }
}

/**
 * 站点首页的标题:vault 的目录名。
 *
 * 先去掉尾部分隔符 —— `/a/b/` 直接 split 会得到一个空的末段,首页标题就变成空白。
 * 只剩分隔符(`/`)或者取不到名字时回落到原串,总比空标题好。
 */
export function vaultSiteTitle(vault: string): string {
  const trimmed = vault.replace(/[/\\]+$/, "");
  const name = trimmed.split(/[/\\]/).pop() ?? "";
  return name || vault;
}

export type SiteExportContext = {
  vault: string;
  siteTitle: string;
  notes: readonly { path: string; title: string }[];
  /** 选目录。返回 null 表示用户取消。 */
  pickDir: () => Promise<string | null>;
  deps: SiteExportDeps;
};

/**
 * 跑一次整库导出并给出结果文案。
 *
 * 取消和「部分失败」是两种不同的结局,文案也分开 —— 用户点了取消却看到「导出完成」
 * 会以为取消没生效。
 */
export async function runSiteExportAction(
  ctx: SiteExportContext,
  t: Translate,
  onProgress?: (progress: SiteExportProgress) => void,
  signal?: AbortSignal,
): Promise<ExportRunOutcome> {
  try {
    const outDir = await ctx.pickDir();
    if (!outDir) return NOTHING;
    const result = await runSiteExport(
      { vault: ctx.vault, siteTitle: ctx.siteTitle, notes: ctx.notes, outDir },
      ctx.deps,
      onProgress,
      signal,
    );
    if (signal?.aborted) {
      return {
        notice: t("notebook.exportSiteCancelled", { written: String(result.written) }),
        error: null,
      };
    }
    if (result.failed > 0) {
      /* 部分失败当 notice 而不是 error:多数页面已经写出去了,那是个可用的产物。
         失败数如实带上,用户自己判断要不要重跑。 */
      return {
        notice: t("notebook.exportSiteDoneWithFailures", {
          written: String(result.written),
          failed: String(result.failed),
        }),
        error: null,
      };
    }
    return {
      notice: t("notebook.exportSiteDone", { written: String(result.written) }),
      error: null,
    };
  } catch (error) {
    return { notice: null, error: t("notebook.exportFailed", { message: messageOf(error) }) };
  }
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
