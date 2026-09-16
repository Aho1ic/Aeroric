import { createContext, useContext, useMemo, type ReactNode } from "react";
import type { CondaEnvironment, SshConnection } from "../../types";

/**
 * SSH / Conda 连接面。App 仍是权威源（持久化与 toast 在 App），
 * 这里只提供只读快照与 setter 引用，避免 ProjectPage 再穿 props。
 */
export type ConnectionsState = {
  sshConnections: SshConnection[];
  onSshConnectionsChange: (connections: SshConnection[]) => void;
  onDeleteSshConnection?: (connectionId: string) => void | Promise<void>;
  condaEnvironments: CondaEnvironment[];
  selectedCondaEnvPath: string | null;
  onSelectedCondaEnvPathChange: (path: string | null) => void;
};

const ConnectionsContext = createContext<ConnectionsState | null>(null);
ConnectionsContext.displayName = "ConnectionsContext";

export function ConnectionsProvider({
  children,
  value,
}: {
  children: ReactNode;
  value: ConnectionsState;
}) {
  const memo = useMemo(() => value, [
    value.sshConnections,
    value.onSshConnectionsChange,
    value.onDeleteSshConnection,
    value.condaEnvironments,
    value.selectedCondaEnvPath,
    value.onSelectedCondaEnvPathChange,
    // value 对象本身每帧新建，只依赖字段。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  ]);
  return <ConnectionsContext.Provider value={memo}>{children}</ConnectionsContext.Provider>;
}

export function useConnections(): ConnectionsState {
  const ctx = useContext(ConnectionsContext);
  if (!ctx) throw new Error("useConnections must be used within ConnectionsProvider");
  return ctx;
}
