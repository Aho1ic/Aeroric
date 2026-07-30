/** 活跃主机的长连接:随 activeHost 切换重建,向下游暴露状态/请求/推送订阅。 */

import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { t } from "../i18n";
import { RemoteConnection, type ConnectionStatus } from "../transport/remote-connection";
import { useHosts } from "./hosts-context";

interface ConnectionContextValue {
  status: ConnectionStatus;
  authError: string | null;
  request: <T = unknown>(method: string, params?: Record<string, unknown>) => Promise<T>;
  onPush: (listener: (push: string, data: unknown, seq?: number) => void) => () => void;
  /** 终端流:发送二进制帧(离线返回 false) */
  sendBinary: (data: Uint8Array) => boolean;
  /** 终端流:入站二进制帧订阅 */
  onBinary: (listener: (data: ArrayBuffer) => void) => () => void;
}

const ConnectionContext = createContext<ConnectionContextValue | null>(null);

export function ConnectionProvider({ children }: { children: ReactNode }) {
  const { activeHost } = useHosts();
  const connRef = useRef<RemoteConnection | null>(null);
  const [status, setStatus] = useState<ConnectionStatus>("idle");
  const [authError, setAuthError] = useState<string | null>(null);
  // push 监听表挂在 provider 层,连接重建时自动重挂,消费方无感
  const pushListeners = useRef(new Set<(push: string, data: unknown, seq?: number) => void>());
  const binaryListeners = useRef(new Set<(data: ArrayBuffer) => void>());

  useEffect(() => {
    if (!activeHost) {
      connRef.current?.stop();
      connRef.current = null;
      setStatus("idle");
      setAuthError(null);
      return;
    }
    // M1 时代的旧记录没有 pin 公钥,无法建立 E2EE 通道:引导重新配对
    if (!activeHost.publicKey) {
      connRef.current?.stop();
      connRef.current = null;
      setStatus("unauthorized");
      setAuthError(t("home.rePair"));
      return;
    }
    const conn = new RemoteConnection({
      endpoints: activeHost.endpoints,
      serverPublicKey: activeHost.publicKey,
      authParams: () => ({ deviceToken: activeHost.deviceToken }),
    });
    connRef.current = conn;
    const offStatus = conn.onStatusChange((next) => {
      setStatus(next);
      setAuthError(conn.authError);
    });
    const offPush = conn.onPush((push, data, seq) => {
      pushListeners.current.forEach((listener) => listener(push, data, seq));
    });
    const offBinary = conn.onBinary((data) => {
      binaryListeners.current.forEach((listener) => listener(data));
    });
    conn.start();
    setStatus(conn.status);
    return () => {
      offStatus();
      offPush();
      offBinary();
      conn.stop();
      if (connRef.current === conn) connRef.current = null;
    };
  }, [activeHost]);

  const value = useMemo<ConnectionContextValue>(
    () => ({
      status,
      authError,
      request: (method, params) => {
        const conn = connRef.current;
        if (!conn) return Promise.reject(new Error("no active host"));
        return conn.request(method, params);
      },
      onPush: (listener) => {
        pushListeners.current.add(listener);
        return () => pushListeners.current.delete(listener);
      },
      sendBinary: (data) => connRef.current?.sendBinary(data) ?? false,
      onBinary: (listener) => {
        binaryListeners.current.add(listener);
        return () => binaryListeners.current.delete(listener);
      },
    }),
    [authError, status],
  );

  return <ConnectionContext.Provider value={value}>{children}</ConnectionContext.Provider>;
}

export function useConnection(): ConnectionContextValue {
  const value = useContext(ConnectionContext);
  if (!value) throw new Error("useConnection must be used within ConnectionProvider");
  return value;
}
