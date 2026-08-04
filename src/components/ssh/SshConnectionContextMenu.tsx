import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { Check, ChevronRight, Clipboard, Link, Plug, Terminal } from "lucide-react";
import { createPortal } from "react-dom";
import type { SshConnection } from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { sshConnectionCommand, sshConnectionUrl } from "./sshConnectionActions";

export type SshConnectionProtocol = "ssh" | "sftp";

interface Props {
  connection: SshConnection;
  x: number;
  y: number;
  onClose: () => void;
  onConnect: (connection: SshConnection, protocol: SshConnectionProtocol) => void;
}

const MENU_WIDTH = 214;
const MENU_HEIGHT = 142;

function menuPosition(x: number, y: number): CSSProperties {
  return {
    left: Math.max(6, Math.min(x, window.innerWidth - MENU_WIDTH - 8)),
    top: Math.max(6, Math.min(y, window.innerHeight - MENU_HEIGHT - 8)),
  };
}

function menuItemStyle(active = false): CSSProperties {
  return {
    ...s.toolbarMenuItem,
    width: "100%",
    boxSizing: "border-box",
    border: "none",
    background: active ? "var(--control-hover-bg)" : "transparent",
    textAlign: "left",
  };
}

export function SshConnectionContextMenu({ connection, x, y, onClose, onConnect }: Props) {
  const { t } = useI18n();
  const [connectMenuOpen, setConnectMenuOpen] = useState(false);
  const [copied, setCopied] = useState<"link" | "command" | null>(null);
  const link = useMemo(() => sshConnectionUrl(connection), [connection]);
  const command = useMemo(() => sshConnectionCommand(connection), [connection]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [onClose]);

  async function copy(value: string, kind: "link" | "command") {
    if (!navigator.clipboard?.writeText) return;
    try {
      await navigator.clipboard.writeText(value);
      setCopied(kind);
      window.setTimeout(() => setCopied(null), 900);
    } catch {
      // Clipboard permissions can be denied by the host; keep the menu usable.
    }
  }

  const content = (
    <>
      <div
        aria-hidden="true"
        style={{ position: "fixed", inset: 0, zIndex: 9998 }}
        onPointerDown={onClose}
        onContextMenu={(event) => {
          event.preventDefault();
          onClose();
        }}
      />
      <div
        role="menu"
        aria-label={`${connection.name} ${t("ssh.connectionMenu")}`}
        style={{
          ...s.toolbarMenuContent,
          ...menuPosition(x, y),
          position: "fixed",
          minWidth: MENU_WIDTH,
          padding: 5,
          zIndex: 9999,
        }}
        onPointerDown={(event) => event.stopPropagation()}
        onContextMenu={(event) => {
          event.preventDefault();
          event.stopPropagation();
        }}
      >
        <div style={{ position: "relative" }} onMouseEnter={() => setConnectMenuOpen(true)}>
          <button
            type="button"
            role="menuitem"
            aria-haspopup="menu"
            aria-expanded={connectMenuOpen}
            style={menuItemStyle(connectMenuOpen)}
            onFocus={() => setConnectMenuOpen(true)}
            onClick={() => setConnectMenuOpen(true)}
          >
            <Plug style={s.toolbarMenuItemIcon} />
            <span style={{ flex: 1 }}>{t("ssh.connect")}</span>
            <ChevronRight size={14} style={{ color: "var(--text-hint)" }} />
          </button>
          {connectMenuOpen && (
            <div
              role="menu"
              aria-label={t("ssh.connect")}
              style={{
                ...s.toolbarMenuContent,
                position: "absolute",
                left: "calc(100% + 4px)",
                top: -5,
                minWidth: 132,
                padding: 5,
                zIndex: 1,
              }}
              onMouseEnter={() => setConnectMenuOpen(true)}
              onPointerDown={(event) => event.stopPropagation()}
            >
              <button
                type="button"
                role="menuitem"
                style={menuItemStyle()}
                onClick={() => {
                  onConnect(connection, "ssh");
                  onClose();
                }}
              >
                <Terminal style={s.toolbarMenuItemIcon} />
                {t("ssh.connectSsh")}
              </button>
              <button
                type="button"
                role="menuitem"
                style={menuItemStyle()}
                onClick={() => {
                  onConnect(connection, "sftp");
                  onClose();
                }}
              >
                <Link style={s.toolbarMenuItemIcon} />
                {t("ssh.connectSftp")}
              </button>
            </div>
          )}
        </div>

        <button
          type="button"
          role="menuitem"
          style={menuItemStyle()}
          onClick={() => void copy(link, "link")}
        >
          {copied === "link" ? (
            <Check style={{ ...s.toolbarMenuItemIcon, color: "var(--success)" }} />
          ) : (
            <Link style={s.toolbarMenuItemIcon} />
          )}
          {copied === "link" ? t("ssh.copied") : t("ssh.copyLink")}
        </button>
        <button
          type="button"
          role="menuitem"
          style={menuItemStyle()}
          onClick={() => void copy(command, "command")}
        >
          {copied === "command" ? (
            <Check style={{ ...s.toolbarMenuItemIcon, color: "var(--success)" }} />
          ) : (
            <Clipboard style={s.toolbarMenuItemIcon} />
          )}
          {copied === "command" ? t("ssh.copied") : t("ssh.copyCommand")}
        </button>
      </div>
    </>
  );

  return createPortal(content, document.body);
}
