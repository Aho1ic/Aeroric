import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FolderOpen, RefreshCw, X } from "lucide-react";
import type { WslDistribution, WslDistributionProbe, WslSettings } from "../../types";
import { useI18n } from "../../i18n";
import { AnimatedSelectionTrack } from "../ui/AnimatedSelection";
import s from "../../styles";

const inputStyle: React.CSSProperties = {
  width: "100%",
  boxSizing: "border-box",
  padding: "8px 10px",
  border: "1px solid var(--border-medium)",
  borderRadius: 7,
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  fontFamily: "var(--font-ui)",
  outline: "none",
};

export interface WslProjectInput {
  name: string;
  distribution: string;
  linuxPath: string;
}

function basename(path: string): string {
  return path.replace(/\/+$/, "").split("/").filter(Boolean).pop() ?? path;
}

export function WslProjectDialog({
  open,
  onClose,
  onOpen,
}: {
  open: boolean;
  onClose: () => void;
  onOpen: (input: WslProjectInput) => void;
}) {
  const { t } = useI18n();
  const [distributions, setDistributions] = useState<WslDistribution[]>([]);
  const [selected, setSelected] = useState("");
  const [linuxPath, setLinuxPath] = useState("");
  const [probe, setProbe] = useState<WslDistributionProbe | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selectedDistribution = useMemo(
    () => distributions.find((item) => item.name === selected) ?? null,
    [distributions, selected],
  );

  const refresh = async () => {
    setLoading(true);
    setError(null);
    try {
      const [list, settings] = await Promise.all([
        invoke<WslDistribution[]>("list_wsl_distributions"),
        invoke<WslSettings>("load_wsl_settings"),
      ]);
      setDistributions(list);
      const preferred =
        settings.defaultDistribution ??
        list.find((item) => item.isDefault)?.name ??
        list[0]?.name ??
        "";
      setSelected((current) =>
        current && list.some((item) => item.name === current) ? current : preferred,
      );
    } catch (nextError) {
      setError(String(nextError));
      setDistributions([]);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (!open) return;
    void refresh();
  }, [open]);

  useEffect(() => {
    if (!open || !selected) {
      setProbe(null);
      return;
    }
    let cancelled = false;
    invoke<WslDistributionProbe>("probe_wsl_distribution", { distribution: selected })
      .then((result) => {
        if (cancelled) return;
        setProbe(result);
        setLinuxPath((current) => current || result.home);
      })
      .catch((nextError) => {
        if (!cancelled) setError(String(nextError));
      });
    return () => {
      cancelled = true;
    };
  }, [open, selected]);

  if (!open) return null;

  const submit = async () => {
    const path = linuxPath.trim();
    if (!selected || !path.startsWith("/")) {
      setError(t("wsl.projectPathRequired"));
      return;
    }
    setLoading(true);
    setError(null);
    try {
      await invoke("validate_wsl_project_path", {
        distribution: selected,
        linuxPath: path,
      });
      onOpen({
        name: basename(path),
        distribution: selected,
        linuxPath: path === "/" ? "/" : path.replace(/\/+$/, ""),
      });
      onClose();
    } catch (nextError) {
      setError(String(nextError));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div
      style={s.modalOverlay}
      onMouseDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t("wsl.openProject")}
        style={{
          ...s.modalBox,
          width: 620,
          maxWidth: "calc(100vw - 40px)",
          height: "auto",
          aspectRatio: "auto",
          flexDirection: "column",
        }}
      >
        <div style={s.settingsContentHeader}>
          <div style={s.settingsContentTitle}>{t("wsl.openProject")}</div>
          <button style={s.modalCloseBtn} onClick={onClose} aria-label={t("common.close")}>
            <X size={16} />
          </button>
        </div>
        <div style={{ padding: 20, display: "flex", flexDirection: "column", gap: 16 }}>
          <div style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
            <div>
              <div style={{ fontSize: 12, fontWeight: 650 }}>{t("wsl.distribution")}</div>
              <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 3 }}>
                {selectedDistribution
                  ? `WSL${selectedDistribution.version ?? "?"} · ${selectedDistribution.state}`
                  : t("wsl.noDistributions")}
              </div>
            </div>
            <button style={s.secondaryActionBtn} onClick={() => void refresh()} disabled={loading}>
              <RefreshCw size={14} />
              {t("common.refresh")}
            </button>
          </div>
          {distributions.length > 0 && (
            <AnimatedSelectionTrack
              value={selected}
              ariaLabel={t("wsl.distribution")}
              style={{ display: "flex", flexWrap: "wrap", gap: 6, padding: 4 }}
            >
              {distributions.map((distribution) => (
                <button
                  key={distribution.name}
                  type="button"
                  data-animated-selection-item
                  data-selection-value={distribution.name}
                  aria-pressed={selected === distribution.name}
                  onClick={() => {
                    setSelected(distribution.name);
                    setLinuxPath("");
                    setError(null);
                  }}
                  style={{
                    position: "relative",
                    zIndex: 1,
                    border: "none",
                    background: "transparent",
                    color:
                      selected === distribution.name
                        ? "var(--control-active-fg)"
                        : "var(--text-secondary)",
                    padding: "7px 10px",
                    borderRadius: 7,
                    cursor: "pointer",
                  }}
                >
                  {distribution.name}
                </button>
              ))}
            </AnimatedSelectionTrack>
          )}
          <label style={{ display: "flex", flexDirection: "column", gap: 7 }}>
            <span style={{ fontSize: 12, fontWeight: 650 }}>{t("wsl.linuxPath")}</span>
            <input
              value={linuxPath}
              onChange={(event) => setLinuxPath(event.target.value)}
              placeholder={probe?.home ?? "/home/user/project"}
              style={inputStyle}
            />
            <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
              {probe ? `${probe.user} · ${probe.shell} · HOME ${probe.home}` : t("wsl.pathHint")}
            </span>
          </label>
          {error && <div style={{ color: "var(--danger)", fontSize: 12 }}>{error}</div>}
        </div>
        <div style={s.settingsFooter}>
          <button style={s.secondaryActionBtn} onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button
            style={s.primaryActionBtn}
            onClick={() => void submit()}
            disabled={loading || !selected || !linuxPath.trim()}
          >
            <FolderOpen size={14} />
            {t("wsl.openProject")}
          </button>
        </div>
      </div>
    </div>
  );
}
