import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, ShieldQuestion, X } from "lucide-react";
import type { SshConnection, SshHostKey } from "../../types";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";

/**
 * 首次连接时的 host key 确认。
 *
 * 命令行 ssh 在这一步会问 `Are you sure you want to continue connecting?`;
 * App 走 PTY 没有这个出口,所以把同样的信息搬进来:展示服务端提供的指纹,
 * 用户核对后才写入 known_hosts。
 *
 * 只用于"未登记"这一种情况。key 变更(已有记录但对不上)不会走到这里 ——
 * 那是 MITM 信号,不提供任何"照样信任"的入口。
 */
export function SshHostKeyDialog({
  connection,
  target,
  keys,
  onTrusted,
  onCancel,
}: {
  connection: SshConnection;
  target: string;
  keys: SshHostKey[];
  onTrusted: () => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setSubmitting(false);
    setError(null);
  }, [connection.id, target]);

  async function handleTrust() {
    if (submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      await invoke("trust_ssh_host_key", {
        connection,
        // 只确认用户真正看到的这几个指纹,后端会重新扫描比对。
        approvedFingerprints: keys.map((key) => key.fingerprint),
      });
      onTrusted();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setSubmitting(false);
    }
  }

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        zIndex: "var(--z-overlay)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "color-mix(in srgb, var(--bg-panel) 16%, transparent)",
        backdropFilter: "blur(12px)",
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget && !submitting) onCancel();
      }}
    >
      <div
        role="dialog"
        aria-labelledby="ssh-hostkey-title"
        aria-describedby="ssh-hostkey-description"
        style={{
          width: "min(560px, calc(100vw - 32px))",
          maxHeight: "calc(100vh - 64px)",
          display: "flex",
          flexDirection: "column",
          background: "var(--bg-card)",
          border: "1px solid var(--border-medium)",
          borderRadius: "var(--radius-lg, 14px)",
          boxShadow: "var(--shadow-dialog, 0 16px 48px rgba(0,0,0,0.24))",
          overflow: "hidden",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
            padding: "18px 20px",
            borderBottom: "1px solid var(--border-dim)",
            background:
              "color-mix(in srgb, var(--warning-subtle, rgba(251, 191, 36, 0.1)) 60%, transparent)",
          }}
        >
          <ShieldQuestion size={20} color="var(--warning, #f59e0b)" aria-hidden />
          <h2
            id="ssh-hostkey-title"
            style={{
              flex: 1,
              margin: 0,
              fontSize: 15,
              fontWeight: 600,
              color: "var(--text-primary)",
            }}
          >
            {t("ssh.hostKey.title")}
          </h2>
        </div>

        <div
          id="ssh-hostkey-description"
          style={{ flex: 1, minHeight: 0, padding: "20px", overflow: "auto" }}
        >
          <p
            style={{
              margin: "0 0 16px",
              fontSize: 13,
              lineHeight: 1.6,
              color: "var(--text-primary)",
            }}
          >
            {t("ssh.hostKey.body", { target })}
          </p>

          <div style={{ marginBottom: 16 }}>
            <div style={{ fontSize: 12, color: "var(--text-secondary)", marginBottom: 6 }}>
              {t("ssh.hostKey.fingerprintLabel")}
            </div>
            <div
              style={{
                display: "flex",
                flexDirection: "column",
                gap: 6,
                padding: "10px 12px",
                background: "var(--bg-panel)",
                border: "1px solid var(--border-dim)",
                borderRadius: "var(--radius-sm)",
              }}
            >
              {keys.map((key) => (
                <div
                  key={key.fingerprint}
                  style={{
                    display: "flex",
                    alignItems: "baseline",
                    gap: 10,
                    fontFamily: "var(--font-mono)",
                    fontSize: 12,
                    color: "var(--text-primary)",
                  }}
                >
                  <span style={{ color: "var(--text-secondary)", flexShrink: 0 }}>
                    {key.keyType}
                  </span>
                  <span style={{ wordBreak: "break-all" }}>{key.fingerprint}</span>
                </div>
              ))}
            </div>
          </div>

          <p
            style={{
              margin: 0,
              fontSize: 12,
              lineHeight: 1.6,
              color: "var(--text-secondary)",
            }}
          >
            {t("ssh.hostKey.verifyHint")}
          </p>

          {error && (
            <div
              role="alert"
              style={{
                marginTop: 16,
                padding: "10px 12px",
                background: "var(--danger-subtle, rgba(239, 68, 68, 0.1))",
                border: "1px solid var(--danger, #ef4444)",
                borderRadius: "var(--radius-sm)",
                fontSize: 12,
                lineHeight: 1.5,
                color: "var(--danger, #ef4444)",
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
              }}
            >
              {error}
            </div>
          )}
        </div>

        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "flex-end",
            gap: 10,
            padding: "14px 20px",
            borderTop: "1px solid var(--border-dim)",
            background: "color-mix(in srgb, var(--bg-card) 94%, transparent)",
          }}
        >
          <Button variant="outline" size="sm" icon={X} disabled={submitting} onClick={onCancel}>
            {t("ssh.hostKey.cancel")}
          </Button>
          <Button
            variant="default"
            size="sm"
            icon={Check}
            disabled={submitting}
            onClick={() => void handleTrust()}
          >
            {submitting ? t("ssh.hostKey.trusting") : t("ssh.hostKey.trust")}
          </Button>
        </div>
      </div>
    </div>
  );
}
