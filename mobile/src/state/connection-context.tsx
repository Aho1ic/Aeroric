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
import { AppState } from "react-native";
import { t } from "../i18n";
import { RemoteConnection, type ConnectionStatus } from "../transport/remote-connection";
import type { RpcCapability, RpcVersion } from "../transport/rpc-codec";
import { subscribeForegroundConnectionRecovery } from "./foreground-recovery";
import { useHosts } from "./hosts-context";

interface ConnectionContextValue {
  status: ConnectionStatus;
  authError: string | null;
  /** 当前主机实际协商的 RPC 版本与能力，离线时回到空能力。 */
  rpcVersion: RpcVersion;
  capabilities: readonly RpcCapability[];
  capabilitiesReady: boolean;
  hasCapability: (capability: RpcCapability | string) => boolean;
  request: <T = unknown>(method: string, params?: Record<string, unknown>) => Promise<T>;
  onPush: (listener: (push: string, data: unknown, seq?: number) => void) => () => void;
  /** 终端流:发送二进制帧(离线返回 false) */
  sendBinary: (data: Uint8Array) => boolean;
  /** 终端流:入站二进制帧订阅 */
  onBinary: (listener: (data: ArrayBuffer) => void) => () => void;
}

const ConnectionContext = createContext<ConnectionContextValue | null>(null);

export function ConnectionProvider({ children }: { children: ReactNode }) {
  const { activeHost, reconcileHostIdentity } = useHosts();
  const connRef = useRef<RemoteConnection | null>(null);
  const [status, setStatus] = useState<ConnectionStatus>("idle");
  const [authError, setAuthError] = useState<string | null>(null);
  const [rpcVersion, setRpcVersion] = useState<RpcVersion>(2);
  const [capabilities, setCapabilities] = useState<readonly RpcCapability[]>([]);
  const [capabilitiesReady, setCapabilitiesReady] = useState(false);
  // push 监听表挂在 provider 层,连接重建时自动重挂,消费方无感
  const pushListeners = useRef(new Set<(push: string, data: unknown, seq?: number) => void>());
  const binaryListeners = useRef(new Set<(data: ArrayBuffer) => void>());
  // 身份合并会改写 activeHost(端点/名称),但那不该触发重连:
  // 连接效果只认「换主机」这件事,主机内容变化走 ref + updateEndpoints。
  const hostRef = useRef(activeHost);
  hostRef.current = activeHost;
  const reconcileRef = useRef(reconcileHostIdentity);
  reconcileRef.current = reconcileHostIdentity;

  const hostKey = activeHost
    ? `${activeHost.id}|${activeHost.publicKey ?? ""}|${activeHost.deviceToken}|${activeHost.protocol ?? "aeroric"}`
    : null;
  const endpointsKey = activeHost ? activeHost.endpoints.join("\n") : "";

  // 移动系统可能在后台或短暂 inactive 期间静默丢弃 WebSocket。每次
  // 回到 `active` 都交给连接层；重复 active 事件在订阅层去重。
  useEffect(() => {
    return subscribeForegroundConnectionRecovery(AppState, () => {
      connRef.current?.notifyForeground();
    });
  }, []);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) {
      connRef.current?.stop();
      connRef.current = null;
      setStatus("idle");
      setAuthError(null);
      setRpcVersion(2);
      setCapabilities([]);
      setCapabilitiesReady(false);
      return;
    }
    // M1 时代的旧记录没有 pin 公钥,无法建立 E2EE 通道:引导重新配对
    if (!host.publicKey) {
      connRef.current?.stop();
      connRef.current = null;
      setStatus("unauthorized");
      setAuthError(t("home.rePair"));
      setRpcVersion(2);
      setCapabilities([]);
      setCapabilitiesReady(false);
      return;
    }
    const conn = new RemoteConnection({
      endpoints: host.endpoints,
      serverPublicKey: host.publicKey,
      authParams: () => ({ deviceToken: host.deviceToken }),
      protocol: host.protocol ?? "aeroric",
    });
    connRef.current = conn;
    const offStatus = conn.onStatusChange((next) => {
      setStatus(next);
      setAuthError(conn.authError);
      if (next !== "online" && next !== "authenticating") {
        setRpcVersion(2);
        setCapabilities([]);
        setCapabilitiesReady(false);
      }
    });
    const offAuth = conn.onAuthSuccess((auth) => {
      setRpcVersion(conn.negotiatedRpcVersion);
      setCapabilities(conn.negotiatedCapabilities);
      setCapabilitiesReady(Array.isArray(auth.capabilities));
    });
    const offPush = conn.onPush((push, data, seq) => {
      pushListeners.current.forEach((listener) => listener(push, data, seq));
    });
    const offBinary = conn.onBinary((data) => {
      binaryListeners.current.forEach((listener) => listener(data));
    });
    // 认证成功后桌面回传实时身份:补 hostId、合并跨网段产生的重复记录、刷新 LAN 地址
    const offIdentity = conn.onHostIdentity((identity, connectedEndpoint) => {
      setRpcVersion(conn.negotiatedRpcVersion);
      if (Array.isArray(identity.capabilities)) {
        setCapabilities(identity.capabilities as RpcCapability[]);
        setCapabilitiesReady(true);
      }
      void reconcileRef.current(host.id, identity, connectedEndpoint).catch(() => {
        // 合并失败(SecureStore 写入异常)不影响当前连接,下次认证成功还会再试
      });
    });
    conn.start();
    setStatus(conn.status);
    return () => {
      offStatus();
      offAuth();
      offPush();
      offBinary();
      offIdentity();
      conn.stop();
      setRpcVersion(2);
      setCapabilities([]);
      setCapabilitiesReady(false);
      if (connRef.current === conn) connRef.current = null;
    };
  }, [hostKey]);

  // 端点变化(身份合并刷新 / 用户手动编辑)就地生效,不重建连接
  useEffect(() => {
    if (!endpointsKey) return;
    connRef.current?.updateEndpoints(endpointsKey.split("\n"));
  }, [endpointsKey, hostKey]);

  const value = useMemo<ConnectionContextValue>(
    () => ({
      status,
      authError,
      rpcVersion,
      capabilities,
      capabilitiesReady,
      hasCapability: (capability) => capabilities.includes(capability as RpcCapability),
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
    [authError, capabilities, capabilitiesReady, rpcVersion, status],
  );

  return <ConnectionContext.Provider value={value}>{children}</ConnectionContext.Provider>;
}

export function useConnection(): ConnectionContextValue {
  const value = useContext(ConnectionContext);
  if (!value) throw new Error("useConnection must be used within ConnectionProvider");
  return value;
}
