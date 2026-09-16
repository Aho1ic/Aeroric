import type { CondaEnvironment, SshConnection } from "../../types";
import type { ReactNode } from "react";
import {
  AppOpsProvider,
  type ProjectOps,
  type TaskActions,
  type TaskOps,
} from "./AppOpsProvider";
import { ConnectionsProvider } from "./ConnectionsProvider";

/**
 * 应用级 zustand store 的组合 Provider。
 * stores 本身是模块级单例（desktop 单窗口），这里只挂 ops context。
 */
export function AppProviders({
  children,
  projectOps,
  taskOps,
  taskActions,
  connections,
}: {
  children: ReactNode;
  projectOps?: ProjectOps;
  taskOps?: TaskOps;
  taskActions?: TaskActions;
  connections?: {
    sshConnections: SshConnection[];
    onSshConnectionsChange: (connections: SshConnection[]) => void;
    onDeleteSshConnection?: (connectionId: string) => void | Promise<void>;
    condaEnvironments: CondaEnvironment[];
    selectedCondaEnvPath: string | null;
    onSelectedCondaEnvPathChange: (path: string | null) => void;
  };
}) {
  const inner = (
    <AppOpsProvider projectOps={projectOps} taskOps={taskOps} taskActions={taskActions}>
      {children}
    </AppOpsProvider>
  );
  if (!connections) return inner;
  return <ConnectionsProvider value={connections}>{inner}</ConnectionsProvider>;
}
