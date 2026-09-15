import type { ReactNode } from "react";
import { AppOpsProvider } from "./AppOpsProvider";

/**
 * 应用级 zustand store 的组合 Provider。
 * stores 本身是模块级单例（desktop 单窗口），这里只挂 ops context。
 */
export function AppProviders({ children }: { children: ReactNode }) {
  return <AppOpsProvider>{children}</AppOpsProvider>;
}
