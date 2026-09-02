import type React from "react";

/**
 * 工具面板(Tests / WebPreview / RunConfigurations / GitAdvanced / Debug)共用的外壳样式。
 *
 * 这些面板此前各自内联了一份字面完全一致的 `headerStyle` / `errorStyle` / …。
 * 合并前用 `/tmp/style_audit.py` 逐个抽出常量体、去注释去空白后按属性排序比对过:
 * 只有**归一化后完全相同**的那些被搬到这里,字面有差异的(例如三种 `labelStyle`
 * 的 600/650 字重、mb 5/6、block/flex-column)一律留在原地 —— 合并它们会改外观。
 *
 * 刻意**不**并进 `styles/index.ts` 的扁平 `s` 命名空间:那里靠 spread 叠加,
 * `headerStyle` / `inputStyle` 这种通用名进去会静默覆盖别的模块(见 common.ts:95-98
 * 已经踩过的那次)。这里只做具名导出,用的地方显式 import。
 */
export const panelChrome = {
  /** 面板顶栏。x5: Tests / WebPreview / RunConfigurations / GitAdvanced / Debug。 */
  header: {
    height: 38,
    display: "flex",
    alignItems: "center",
    gap: 8,
    padding: "0 10px",
    borderBottom: "1px solid var(--border-dim)",
    fontSize: 12,
    fontWeight: 650,
  },
  /** 顶栏右侧的图标按钮(自带 marginLeft:auto)。x2: WebPreview / GitAdvanced。 */
  headerIconButton: {
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
  },
  /** 顶栏下方的错误条。x4: WebPreview / RunConfigurations / GitAdvanced / Debug。 */
  errorBar: {
    padding: "7px 10px",
    color: "var(--danger)",
    fontSize: 11,
    borderBottom: "1px solid var(--border-dim)",
  },
} satisfies Record<string, React.CSSProperties>;

/**
 * RunConfigurations 与 Debug 两个面板的表单外壳。
 *
 * 这两个文件是复制关系 —— 有 8 个常量字面一致。它们和上面的 `panelChrome`
 * 不是同一套视觉(这里 input 高 26 / fontSize 11,设置页那套是 padding 7px 10px /
 * fontSize 12.5),所以单独一组,不要互相引用。
 */
export const runDebugForm = {
  toolbar: {
    display: "flex",
    flexWrap: "wrap",
    gap: 6,
    padding: 10,
    borderBottom: "1px solid var(--border-dim)",
  },
  list: {
    minHeight: 0,
    overflowY: "auto",
    padding: 8,
    borderBottom: "1px solid var(--border-dim)",
  },
  label: {
    display: "flex",
    flexDirection: "column",
    gap: 4,
    color: "var(--text-muted)",
    fontSize: 11,
    fontWeight: 650,
  },
  input: {
    height: 26,
    minWidth: 0,
    border: "1px solid var(--border-dim)",
    borderRadius: 6,
    background: "var(--bg-card)",
    color: "var(--text-primary)",
    fontSize: 11,
    padding: "0 7px",
  },
  empty: {
    padding: "18px 8px",
    color: "var(--text-muted)",
    textAlign: "center",
    fontSize: 12,
  },
  status: {
    marginLeft: "auto",
    minWidth: 0,
    maxWidth: "100%",
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
    color: "var(--text-muted)",
    fontSize: 11,
    fontWeight: 650,
  },
} satisfies Record<string, React.CSSProperties>;

/**
 * 设置页表单(McpPanel / ProxyPanel / AgentPathSection / LocalRouterPanel)。
 *
 * `label` x4、`input` x3、`hint` x3 字面一致。注意 LocalRouterPanel 的 hint 与
 * 另外三处**不同**(多了别的属性),没有并进来。
 */
export const settingsForm = {
  label: {
    fontSize: 12,
    fontWeight: 600,
    color: "var(--text-secondary)",
    marginBottom: 5,
    display: "block",
  },
  input: {
    width: "100%",
    padding: "7px 10px",
    background: "var(--bg-input)",
    border: "1px solid var(--border-medium)",
    borderRadius: 7,
    color: "var(--text-primary)",
    fontSize: 12.5,
    fontFamily: "var(--font-mono)",
    outline: "none",
    boxSizing: "border-box",
  },
  hint: {
    fontSize: 11,
    color: "var(--text-hint)",
    marginTop: 3,
  },
} satisfies Record<string, React.CSSProperties>;

/**
 * Agent 表单的字段标签。x3: AgentDetailModal / AgentConfigPanel / AddAgentPanel。
 *
 * 和 `settingsForm.label` 差 fontWeight(650 vs 600)与 marginBottom(6 vs 5),
 * 是两套视觉,不要合。
 */
export const agentForm = {
  label: {
    display: "block",
    fontSize: 12,
    fontWeight: 650,
    color: "var(--text-secondary)",
    marginBottom: 6,
  },
} satisfies Record<string, React.CSSProperties>;

/**
 * WSL 的路径输入框。x2: WslProjectDialog / app-settings/WslPanel。
 * 与 `settingsForm.input` 差 padding(8px vs 7px)和字体(ui vs mono)。
 */
export const wslForm = {
  input: {
    width: "100%",
    boxSizing: "border-box",
    padding: "8px 10px",
    border: "1px solid var(--border-medium)",
    borderRadius: 7,
    background: "var(--bg-input)",
    color: "var(--text-primary)",
    fontFamily: "var(--font-ui)",
    outline: "none",
  },
} satisfies Record<string, React.CSSProperties>;
