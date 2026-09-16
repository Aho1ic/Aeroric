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
  // 只依赖字段，避免 value 对象字面量每帧新建导致重渲染。
  const memo = useMemo(
    () => ({
      sshConnections: value.sshConnections,
      onSshConnectionsChange: value.onSshConnectionsChange,
      onDeleteSshConnection: value.onDeleteSshConnection,
      condaEnvironments: value.condaEnvironments,
      selectedCondaEnvPath: value.selectedCondaEnvPath,
      onSelectedCondaEnvPathChange: value.onSelectedCondaEnvPathChange,
    }),
    [
      value.sshConnections,
      value.onSshConnectionsChange,
      value.onDeleteSshConnection,
      value.condaEnvironments,
      value.selectedCondaEnvPath,
      value.onSelectedCondaEnvPathChange,
    ],
  );
  return <ConnectionsContext.Provider value={memo}>{children}</ConnectionsContext.Provider>;
}

const defaultConnections: ConnectionsState = {
  sshConnections: [],
  onSshConnectionsChange: () => {},
  condaEnvironments: [],
  selectedCondaEnvPath: null,
  onSelectedCondaEnvPathChange: () => {},
};

export function useConnections(): ConnectionsState {
  return useContext(ConnectionsContext) ?? defaultConnections;
}
