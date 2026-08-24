import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import { TextInput } from "../ui/Primitives";

export type ChatBridgePythonStatus = {
  usable: boolean;
  program: string;
  version: string;
  configured: boolean;
  failure: string;
  checked: string[];
};

type Props = {
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  /** 挂载即预检,让缺失在保存之前就暴露,而不是等到启动终端。 */
  autoProbe?: boolean;
};

/**
 * Chat Completions bridge 的解释器输入 + 预检。
 *
 * 预检必须走后端实跑解释器:Windows 预置的 Microsoft Store 别名桩存在、能被 `where`
 * 找到,但一运行就跳商店并以 9009 退出。前端校验路径字符串或后端只判断文件存在,
 * 都会把这种机器报成"可用"。
 */
export function BridgePythonField({ value, onChange, disabled, autoProbe }: Props) {
  const { t } = useI18n();
  const [status, setStatus] = useState<ChatBridgePythonStatus | null>(null);
  const [probing, setProbing] = useState(false);
  const [probeError, setProbeError] = useState<string | null>(null);
  // 只采用最后一次请求的结果:快速改路径时旧结果可能后到。
  const requestRef = useRef(0);

  const probe = useCallback(async (path: string) => {
    const requestId = requestRef.current + 1;
    requestRef.current = requestId;
    setProbing(true);
    setProbeError(null);
    try {
      const next = await invoke<ChatBridgePythonStatus>("probe_chat_bridge_python", {
        bridgePythonPath: path,
      });
      if (requestRef.current === requestId) setStatus(next);
    } catch (e) {
      if (requestRef.current === requestId) setProbeError(String(e));
    } finally {
      if (requestRef.current === requestId) setProbing(false);
    }
  }, []);

  useEffect(() => {
    if (!autoProbe || disabled) return;
    // 只在挂载时自动跑一次;之后由用户改动或点"检测"触发。
    void probe(value);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoProbe, disabled]);

  function statusLine(): { text: string; tone: "ok" | "warn" | "danger" } | null {
    if (probing) return { text: t("appSettings.bridgePythonChecking"), tone: "warn" };
    if (probeError) return { text: probeError, tone: "danger" };
    if (!status) return null;
    if (status.usable) {
      return {
        text: t("appSettings.bridgePythonOk", {
          version: status.version,
          program: status.program,
        }),
        tone: "ok",
      };
    }
    if (status.configured) {
      return {
        text: t("appSettings.bridgePythonUnusable", { failure: status.failure }),
        tone: "danger",
      };
    }
    const missing = t("appSettings.bridgePythonMissing");
    const checked = status.checked.length
      ? ` ${t("appSettings.bridgePythonChecked", { checked: status.checked.join("; ") })}`
      : "";
    return { text: `${missing}${checked}`, tone: "danger" };
  }

  const line = statusLine();
  const toneColor =
    line?.tone === "ok"
      ? "var(--text-secondary)"
      : line?.tone === "danger"
        ? "var(--danger, #d94f4f)"
        : "var(--text-hint)";

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
      <span style={{ fontSize: 12.5, fontWeight: 650, color: "var(--text-secondary)" }}>
        {t("appSettings.bridgePython")}
      </span>
      <div style={{ display: "flex", gap: 8, minWidth: 0 }}>
        <TextInput
          aria-label={t("appSettings.bridgePython")}
          placeholder={t("appSettings.bridgePythonPlaceholder")}
          value={value}
          disabled={disabled}
          style={{ flex: 1, minWidth: 0 }}
          onChange={(event) => {
            onChange(event.target.value);
            // 路径变了,旧结论立刻失效,避免显示过期的"可用"。
            setStatus(null);
            setProbeError(null);
          }}
        />
        <Button
          variant="default"
          size="sm"
          disabled={disabled || probing}
          onClick={() => void probe(value)}
        >
          {probing ? t("appSettings.bridgePythonChecking") : t("appSettings.bridgePythonCheck")}
        </Button>
      </div>
      <span style={{ fontSize: 11, color: "var(--text-hint)" }}>
        {t("appSettings.bridgePythonHint")}
      </span>
      {line && (
        <span role="status" style={{ fontSize: 11, color: toneColor }}>
          {line.text}
        </span>
      )}
    </div>
  );
}
