import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useI18n } from "../../i18n";
import s from "../../styles";
import appLogo from "../../assets/app-logo.png";

const GITHUB_URL = "https://github.com/Aho1ic/Aeroric.git";

export function AboutPanel() {
  const { t } = useI18n();
  const [appVersion, setAppVersion] = useState("");

  useEffect(() => {
    getVersion()
      .then((version) => setAppVersion(version))
      .catch(() => setAppVersion(t("common.unknown")));
  }, [t]);

  return (
    <div
      style={{
        ...s.settingsBody,
        display: "flex",
        flexDirection: "column",
        padding: "20px",
      }}
    >
      <div
        style={{
          display: "flex",
          gap: 16,
          padding: "18px",
          borderRadius: 12,
          border: "1px solid var(--border-dim)",
          background: "var(--bg-subtle)",
        }}
      >
        <img
          src={appLogo}
          alt="Aeroric logo"
          style={{
            width: 64,
            height: 64,
            borderRadius: 16,
            flexShrink: 0,
            objectFit: "cover",
          }}
        />

        <div style={{ minWidth: 0, display: "flex", flexDirection: "column", gap: 14 }}>
          <div>
            <div style={{ fontSize: 16, fontWeight: 700, color: "var(--text-primary)" }}>Aeroric</div>
            <div style={{ fontSize: 12.5, color: "var(--text-secondary)", marginTop: 4 }}>
              {t("appSettings.description")}
            </div>
          </div>

          <div style={{ display: "grid", gap: 12 }}>
            <div>
              <div style={{ fontSize: 11, color: "var(--text-hint)", marginBottom: 4 }}>
                {t("appSettings.version")}
              </div>
              <div
                style={{
                  fontSize: 12.5,
                  color: "var(--text-primary)",
                  fontFamily: "var(--font-mono)",
                }}
              >
                {appVersion || t("common.loading")}
              </div>
            </div>
            <div>
              <div style={{ fontSize: 11, color: "var(--text-hint)", marginBottom: 4 }}>
                GitHub
              </div>
              <button
                type="button"
                onClick={() => void openUrl(GITHUB_URL)}
                style={{
                  padding: 0,
                  border: "none",
                  background: "transparent",
                  color: "var(--accent)",
                  fontSize: 12.5,
                  fontFamily: "var(--font-mono)",
                  cursor: "pointer",
                  textAlign: "left",
                }}
              >
                {GITHUB_URL}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
