import type { DiagnosticItem } from "../../types";

/**
 * 编辑器里 LSP 诊断的合并规则。
 *
 * 从 `ProjectPage.tsx` 模块层整块搬出来,逻辑一行没改。纯函数,不碰 state。
 */

/** 后端 `lsp-diagnostics` 事件的载荷。 */
export type LspDiagnosticsEvent = {
  projectPath: string;
  filePath: string;
  diagnostics: DiagnosticItem[];
};

/**
 * 用一个文件的新诊断替换掉它的旧 LSP 诊断,别的文件和别的来源都不动。
 *
 * 过滤条件是**两个都要满足才丢弃**(同一个文件 **且** 来源以 `lsp:` 开头):
 * 语言服务器每次 publish 都是「这个文件当前的全部诊断」,所以同文件的旧 LSP 条目
 * 必须整批清掉,否则修好的问题会一直挂着。而 `lsp:` 前缀这一半保证同文件里由
 * 别的来源(eslint / tsc / cargo 那些跑批的诊断)产生的条目不被这次 publish 顺手清掉。
 */
export function mergeLspDiagnostics(
  current: DiagnosticItem[],
  filePath: string,
  diagnostics: DiagnosticItem[],
): DiagnosticItem[] {
  return [
    ...current.filter(
      (diagnostic) => diagnostic.file !== filePath || !diagnostic.source.startsWith("lsp:"),
    ),
    ...diagnostics,
  ];
}
