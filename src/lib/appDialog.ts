/* 应用内提示弹窗的调用入口 —— 替代 OS 原生的 confirm / prompt。
 *
 * 背景:plugin-dialog 的 `confirm` 走 OS 原生 MessageBox,`window.prompt` 走
 * WebView2 自带的输入框。两者在 Windows 上都表现为系统提示框——标题栏是系统
 * 的、配色不随主题、拿不到设计 token,与应用其余部分割裂。
 *
 * `confirm` 刻意保持与 plugin-dialog 完全相同的签名,所以迁移时调用点一行都
 * 不用动,只换 import 来源。`prompt` 无法同签名:`window.prompt` 是同步的,
 * 应用内实现必然返回 Promise,调用点要加 await。
 *
 * 为什么是模块级单例而不是 React Context:`databaseProductionSafety.ts` 是纯
 * TS 模块(非组件),Context 方案会迫使它变成 hook,并沿调用链改十几个组件
 * 签名。`AppDialogHost` 挂载时把自己注册进来,非 React 调用方照样能弹窗。
 */

export type AppConfirmKind = "info" | "warning" | "error";

/** 与 plugin-dialog `ConfirmDialogOptions` 对齐的子集(仓库里实际用到的那些)。 */
export type AppConfirmOptions = {
  title?: string;
  kind?: AppConfirmKind;
  okLabel?: string;
  cancelLabel?: string;
};

export type AppPromptOptions = {
  title?: string;
  /** 输入框初值,对应 `window.prompt` 的第二个参数。 */
  defaultValue?: string;
  placeholder?: string;
  okLabel?: string;
  cancelLabel?: string;
  /**
   * 允许提交空串。默认 false —— 空输入折成 null(取消),因为多数调用点是
   * `if (!name) return;`。但少数调用点(如把连接移出分组)靠"提交空值"表达
   * 清空语义,`window.prompt` 时代它们靠 `null` 与 `""` 的区别实现,迁移后
   * 必须显式打开这个开关才能保住这层语义。
   */
  allowEmpty?: boolean;
};

export type AppConfirmRequest = {
  kind: "confirm";
  message: string;
  options: AppConfirmOptions;
};

export type AppPromptRequest = {
  kind: "prompt";
  message: string;
  options: AppPromptOptions;
};

export type AppDialogRequest = AppConfirmRequest | AppPromptRequest;

/** confirm 用 boolean 兑现;prompt 用字符串兑现,取消是 null。 */
export type AppDialogResult = boolean | string | null;

export type AppDialogHandler = (request: AppDialogRequest) => Promise<AppDialogResult>;

let handler: AppDialogHandler | null = null;

/** 由 `AppDialogHost` 在挂载时调用;返回的函数用于卸载时注销。 */
export function registerAppDialogHandler(next: AppDialogHandler): () => void {
  handler = next;
  return () => {
    // 只有仍是自己注册的那个才注销:StrictMode 下 effect 会跑两遍,
    // 后挂载的 host 已经覆盖了 handler,旧 host 的清理不该把它抹掉。
    if (handler === next) handler = null;
  };
}

/** 测试用:清掉已注册的 handler,避免用例之间互相污染。 */
export function resetAppDialogHandlerForTests(): void {
  handler = null;
}

/**
 * 弹出应用内确认框,确认返回 true,取消返回 false。
 *
 * host 未挂载时返回 false 而不是抛错:拒绝是安全默认值,不能让删除这类
 * 操作在弹窗缺失时静默放行。
 */
export function confirm(message: string, options: AppConfirmOptions = {}): Promise<boolean> {
  if (!handler) {
    console.warn("[appDialog] confirm() called before AppDialogHost mounted; treating as cancel.");
    return Promise.resolve(false);
  }
  return handler({ kind: "confirm", message, options }) as Promise<boolean>;
}

/**
 * 弹出应用内输入框,确认返回去空白后的字符串,取消返回 null。
 *
 * 与 `window.prompt` 的差异:这里返回 Promise;且空输入默认按取消处理(返回
 * null),因为多数调用点都是 `if (!name) return;` 这个形状。需要区分"取消"与
 * "提交空值"的调用点传 `allowEmpty: true`。
 */
export function prompt(message: string, options: AppPromptOptions = {}): Promise<string | null> {
  if (!handler) {
    console.warn("[appDialog] prompt() called before AppDialogHost mounted; treating as cancel.");
    return Promise.resolve(null);
  }
  return handler({ kind: "prompt", message, options }) as Promise<string | null>;
}
