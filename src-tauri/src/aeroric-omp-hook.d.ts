/**
 * `aeroric-omp-hook.js` 的类型声明。
 *
 * 那个脚本是随 Rust 二进制 include_str! 打包、由 Bun 在 omp agent 进程里加载的
 * 运行时资产,不参与前端构建;这里只为 `src/test/omp-hook.test.ts` 提供类型,
 * 好让它直接跑真脚本而不是复刻件。
 */

/** omp 的 ExtensionContext,hook 只读 hasUI(交互式 TUI 为 true,子代理为 false)。 */
export interface OmpExtensionContext {
  hasUI?: boolean;
}

/** omp 的 ExtensionAPI,hook 只用到 on。 */
export interface OmpExtensionApi {
  on(
    event: string,
    handler: (event: unknown, ctx: OmpExtensionContext | undefined) => Promise<void>,
  ): void;
}

export default function aeroricOmpHook(pi: OmpExtensionApi): void;
