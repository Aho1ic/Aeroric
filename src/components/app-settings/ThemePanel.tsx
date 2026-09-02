import type React from "react";
import { Check, FolderOpen, Monitor, Trash2, Upload } from "lucide-react";
import type { ThemeMode } from "../../types";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { useCustomThemes } from "../../hooks/useCustomThemes";
import { APP_PLATFORM } from "../../platform";

interface ThemePanelProps {
  themeMode: ThemeMode;
  systemPrefersDark: boolean;
  onThemeModeChange: (mode: ThemeMode) => void;
}

/** 应急停用快捷键的展示文案。判定在 `customThemes.ts` 的 `isCustomThemePanicKey`。 */
const PANIC_KEYS = APP_PLATFORM === "macos" ? "⌘ + ⌥ + ⇧ + T" : "Ctrl + Alt + Shift + T";

export function ThemePanel({ themeMode, systemPrefersDark, onThemeModeChange }: ThemePanelProps) {
  const { t } = useI18n();
  const custom = useCustomThemes();

  async function handleCustomImport() {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({
      directory: false,
      multiple: false,
      title: t("theme.customDialogTitle"),
      filters: [{ name: "CSS", extensions: ["css"] }],
    });
    // multiple: false 下返回单个路径,但类型里仍带着数组分支。
    if (typeof picked !== "string") return;
    await custom.importFrom(picked);
  }

  async function handleOpenCustomDir() {
    const dir = await custom.openDir();
    const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
    await revealItemInDir(dir);
  }
  type ManualMode = Extract<ThemeMode, "dark" | "light" | "eyecare">;
  const manualThemeModes: ManualMode[] = ["dark", "light", "eyecare"];
  const currentModeLabel = systemPrefersDark ? t("theme.dark") : t("theme.light");
  const manualModeLabel =
    themeMode === "dark"
      ? t("theme.dark")
      : themeMode === "eyecare"
        ? t("theme.eyecare")
        : t("theme.light");
  const selectedLabel =
    themeMode === "system"
      ? t("theme.followingSystem", { mode: currentModeLabel })
      : t("theme.manual", { mode: manualModeLabel });

  function handleSystemThemeToggle() {
    onThemeModeChange(themeMode === "system" ? "light" : "system");
  }

  function handleManualThemeKeyDown(
    mode: ManualMode,
    event: React.KeyboardEvent<HTMLButtonElement>,
  ) {
    const currentIndex = manualThemeModes.indexOf(mode);
    if (currentIndex === -1) {
      return;
    }

    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      event.preventDefault();
      onThemeModeChange(manualThemeModes[(currentIndex + 1) % manualThemeModes.length]);
      return;
    }

    if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      event.preventDefault();
      onThemeModeChange(
        manualThemeModes[(currentIndex - 1 + manualThemeModes.length) % manualThemeModes.length],
      );
      return;
    }

    if (event.key === "Home") {
      event.preventDefault();
      onThemeModeChange(manualThemeModes[0]);
      return;
    }

    if (event.key === "End") {
      event.preventDefault();
      onThemeModeChange(manualThemeModes[manualThemeModes.length - 1]);
    }
  }

  function renderThemeOption({
    mode,
    title,
    description,
    previewBackground,
    previewBorder,
    previewAccent,
  }: {
    mode: ManualMode;
    title: string;
    description: string;
    previewBackground: string;
    previewBorder: string;
    previewAccent: string;
  }) {
    const selected = themeMode === mode;

    return (
      <button
        type="button"
        onClick={() => onThemeModeChange(mode)}
        onKeyDown={(event) => handleManualThemeKeyDown(mode, event)}
        role="radio"
        aria-checked={selected}
        aria-label={title}
        style={{
          display: "flex",
          flexDirection: "column",
          alignItems: "stretch",
          gap: 10,
          padding: 14,
          borderRadius: 12,
          border: `1px solid ${selected ? "var(--control-active-fg)" : "var(--border-medium)"}`,
          background: selected ? "var(--control-active-bg)" : "var(--bg-subtle)",
          cursor: "pointer",
          textAlign: "left",
          boxShadow: selected ? "0 0 0 1px var(--control-active-bg)" : "none",
          transition: "border-color 0.12s, background 0.12s, box-shadow 0.12s",
        }}
      >
        <div
          style={{
            width: "100%",
            height: 106,
            borderRadius: 10,
            border: `1px solid ${previewBorder}`,
            background: previewBackground,
            padding: 8,
            boxSizing: "border-box",
            display: "flex",
            flexDirection: "column",
            gap: 7,
            overflow: "hidden",
          }}
        >
          <div style={{ display: "flex", gap: 5 }}>
            <span
              style={{
                width: 7,
                height: 7,
                borderRadius: 999,
                background: previewAccent,
                opacity: 0.9,
              }}
            />
            <span
              style={{
                width: 7,
                height: 7,
                borderRadius: 999,
                background: previewAccent,
                opacity: 0.65,
              }}
            />
            <span
              style={{
                width: 7,
                height: 7,
                borderRadius: 999,
                background: previewAccent,
                opacity: 0.4,
              }}
            />
          </div>
          <div
            style={{
              flex: 1,
              display: "grid",
              gridTemplateColumns: mode === "dark" ? "28px 1fr" : "24px 1fr",
              gap: 7,
            }}
          >
            <div
              style={{
                borderRadius: 7,
                background: mode === "dark" ? "rgba(255,255,255,0.05)" : "rgba(23,27,36,0.06)",
                border:
                  mode === "dark"
                    ? "1px solid rgba(255,255,255,0.06)"
                    : "1px solid rgba(23,27,36,0.06)",
                display: "flex",
                flexDirection: "column",
                gap: 5,
                padding: "7px 5px",
              }}
            >
              <span
                style={{
                  height: 5,
                  borderRadius: 999,
                  background: previewAccent,
                  opacity: mode === "dark" ? 0.55 : 0.3,
                }}
              />
              <span
                style={{
                  height: 5,
                  borderRadius: 999,
                  background: previewAccent,
                  opacity: mode === "dark" ? 0.28 : 0.16,
                }}
              />
              <span
                style={{
                  height: 5,
                  borderRadius: 999,
                  background: previewAccent,
                  opacity: mode === "dark" ? 0.2 : 0.12,
                }}
              />
            </div>
            <div
              style={{
                borderRadius: 8,
                background:
                  mode === "dark"
                    ? "rgba(19,23,29,0.82)"
                    : "linear-gradient(180deg, rgba(23,27,36,0.1), rgba(23,27,36,0.04))",
                border:
                  mode === "dark"
                    ? "1px solid rgba(255,255,255,0.08)"
                    : "1px solid rgba(23,27,36,0.08)",
                padding: 8,
                boxSizing: "border-box",
                display: "flex",
                flexDirection: "column",
                gap: 6,
              }}
            >
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: 6,
                }}
              >
                <span
                  style={{
                    width: 34,
                    height: 6,
                    borderRadius: 999,
                    background: previewAccent,
                    opacity: mode === "dark" ? 0.75 : 0.2,
                  }}
                />
                <span
                  style={{
                    width: 12,
                    height: 12,
                    borderRadius: 4,
                    background: mode === "dark" ? "rgba(255,255,255,0.12)" : "#ffffff",
                    border:
                      mode === "dark"
                        ? "1px solid rgba(255,255,255,0.08)"
                        : "1px solid rgba(23,27,36,0.08)",
                  }}
                />
              </div>
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "1.15fr 0.85fr",
                  gap: 6,
                  flex: 1,
                }}
              >
                <div
                  style={{
                    borderRadius: 6,
                    background:
                      mode === "dark" ? "rgba(255,255,255,0.07)" : "rgba(255,255,255,0.9)",
                    border:
                      mode === "dark"
                        ? "1px solid rgba(255,255,255,0.06)"
                        : "1px solid rgba(23,27,36,0.06)",
                  }}
                />
                <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
                  <span
                    style={{
                      height: 18,
                      borderRadius: 6,
                      background:
                        mode === "dark" ? "rgba(255,255,255,0.09)" : "rgba(255,255,255,0.92)",
                      border:
                        mode === "dark"
                          ? "1px solid rgba(255,255,255,0.06)"
                          : "1px solid rgba(23,27,36,0.06)",
                    }}
                  />
                  <span
                    style={{
                      flex: 1,
                      borderRadius: 6,
                      background:
                        mode === "dark" ? "rgba(255,255,255,0.05)" : "rgba(255,255,255,0.82)",
                      border:
                        mode === "dark"
                          ? "1px solid rgba(255,255,255,0.05)"
                          : "1px solid rgba(23,27,36,0.05)",
                    }}
                  />
                </div>
              </div>
            </div>
          </div>
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              gap: 8,
            }}
          >
            <span style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
              {title}
            </span>
            {selected && <Check size={14} color="var(--accent)" />}
          </div>
          <span style={{ fontSize: 11.5, color: "var(--text-hint)", lineHeight: 1.45 }}>
            {description}
          </span>
        </div>
      </button>
    );
  }

  return (
    <div
      style={{
        ...s.settingsBody,
        display: "flex",
        flexDirection: "column",
        gap: 18,
        padding: "20px",
      }}
    >
      <button
        type="button"
        onClick={handleSystemThemeToggle}
        role="switch"
        aria-checked={themeMode === "system"}
        aria-label={t("theme.followSystemAria")}
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 14,
          padding: "16px 18px",
          borderRadius: 12,
          border: `1px solid ${themeMode === "system" ? "var(--control-active-fg)" : "var(--border-dim)"}`,
          background: themeMode === "system" ? "var(--control-active-bg)" : "var(--bg-subtle)",
          cursor: "pointer",
          textAlign: "left",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0 }}>
          <div
            style={{
              flexShrink: 0,
              width: 48,
              height: 28,
              borderRadius: 999,
              border: "none",
              padding: 3,
              background:
                themeMode === "system" ? "var(--primary-action-bg)" : "var(--border-medium)",
              boxShadow:
                themeMode === "system"
                  ? "0 0 0 4px var(--control-active-bg)"
                  : "inset 0 0 0 1px var(--border-dim)",
              transition: "background 0.12s, box-shadow 0.12s",
            }}
          >
            <div
              style={{
                width: 22,
                height: 22,
                borderRadius: 999,
                display: "grid",
                placeItems: "center",
                background: "var(--control-knob-bg)",
                color: themeMode === "system" ? "var(--accent)" : "var(--text-secondary)",
                transform: themeMode === "system" ? "translateX(20px)" : "translateX(0)",
                transition: "transform 0.12s ease",
              }}
            >
              <Monitor size={12} />
            </div>
          </div>
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 3,
              minWidth: 0,
              padding: 0,
              textAlign: "left",
            }}
          >
            <span style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
              {t("theme.followSystem")}
            </span>
          </div>
        </div>
        <div
          style={{
            flexShrink: 0,
            display: "inline-flex",
            alignItems: "center",
            gap: 6,
            padding: "6px 10px",
            borderRadius: 999,
            background: "var(--bg-card)",
            border: "1px solid var(--border-medium)",
            color: "var(--text-secondary)",
            fontSize: 11.5,
            fontWeight: 600,
          }}
        >
          {themeMode === "system" && <Check size={13} color="var(--accent)" />}
          {selectedLabel}
        </div>
      </button>

      <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={{ fontSize: 12, fontWeight: 600, color: "var(--text-secondary)" }}>
          {t("theme.manualTheme")}
        </div>
        <div
          style={{ display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 14 }}
          role="radiogroup"
          aria-label={t("theme.manualThemeAria")}
        >
          {renderThemeOption({
            mode: "dark",
            title: t("theme.dark"),
            description: t("theme.darkDescription"),
            previewBackground: "#050607",
            previewBorder: "rgba(171,178,191,0.18)",
            previewAccent: "#61afef",
          })}
          {renderThemeOption({
            mode: "light",
            title: t("theme.light"),
            description: t("theme.lightDescription"),
            previewBackground: "#f5f7fb",
            previewBorder: "rgba(23,27,36,0.08)",
            previewAccent: "#171b24",
          })}
          {renderThemeOption({
            mode: "eyecare",
            title: t("theme.eyecare"),
            description: t("theme.eyecareDescription"),
            previewBackground: "#f5ecd7",
            previewBorder: "rgba(101,84,51,0.16)",
            previewAccent: "#5a4a30",
          })}
        </div>
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={{ fontSize: 12, fontWeight: 600, color: "var(--text-secondary)" }}>
          {t("theme.customTitle")}
        </div>
        <div style={{ fontSize: 11.5, color: "var(--text-secondary)", lineHeight: 1.6 }}>
          {t("theme.customHint")}
        </div>
        <div style={{ fontSize: 11.5, color: "var(--text-hint)", lineHeight: 1.6 }}>
          {t("theme.customScopeNote")}
        </div>
        {/* 快捷键提示紧跟在导入按钮旁边:界面被自定义 CSS 藏掉之后,用户得先知道
            怎么出来。藏在文档里的逃生路等于没有。 */}
        <div style={{ fontSize: 11.5, color: "var(--text-hint)", lineHeight: 1.6 }}>
          {t("theme.customPanicHint", { keys: PANIC_KEYS })}
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
          <button
            type="button"
            onClick={() => void handleCustomImport()}
            disabled={custom.busy}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
              padding: "6px 12px",
              borderRadius: 8,
              border: "1px solid var(--border-medium)",
              background: "var(--bg-card)",
              color: "var(--text-primary)",
              fontSize: 12,
              fontWeight: 600,
              cursor: custom.busy ? "default" : "pointer",
              opacity: custom.busy ? 0.6 : 1,
            }}
          >
            <Upload size={13} />
            {custom.busy ? t("theme.customImporting") : t("theme.customImport")}
          </button>
          <button
            type="button"
            onClick={() => void handleOpenCustomDir()}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
              padding: "6px 12px",
              borderRadius: 8,
              border: "1px solid var(--border-dim)",
              background: "transparent",
              color: "var(--text-secondary)",
              fontSize: 12,
              cursor: "pointer",
            }}
          >
            <FolderOpen size={13} />
            {t("theme.customOpenDir")}
          </button>
          {custom.activeId !== null && (
            <button
              type="button"
              onClick={() => void custom.apply(null)}
              style={{
                padding: "6px 12px",
                borderRadius: 8,
                border: "1px solid var(--border-dim)",
                background: "transparent",
                color: "var(--text-secondary)",
                fontSize: 12,
                cursor: "pointer",
              }}
            >
              {t("theme.customClear")}
            </button>
          )}
        </div>

        {custom.error !== null && (
          <div role="alert" style={{ fontSize: 11.5, color: "var(--danger)", lineHeight: 1.6 }}>
            {custom.error}
          </div>
        )}

        {custom.themes.length === 0 ? (
          <div style={{ fontSize: 11.5, color: "var(--text-hint)" }}>{t("theme.customEmpty")}</div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            {custom.themes.map((theme) => {
              const applied = theme.id === custom.activeId;
              return (
                <div
                  key={theme.id}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 10,
                    padding: "8px 12px",
                    borderRadius: 8,
                    border: `1px solid ${applied ? "var(--control-active-fg)" : "var(--border-dim)"}`,
                    background: applied ? "var(--control-active-bg)" : "var(--bg-subtle)",
                  }}
                >
                  <div style={{ display: "flex", flexDirection: "column", gap: 2, minWidth: 0 }}>
                    <span
                      style={{
                        fontSize: 12.5,
                        fontWeight: 600,
                        color: "var(--text-primary)",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {theme.name}
                    </span>
                    <span style={{ fontSize: 11, color: "var(--text-hint)" }}>
                      {t("theme.customSize", {
                        kb: String(Math.max(1, Math.round(theme.size / 1024))),
                      })}
                    </span>
                  </div>
                  <div style={{ display: "flex", alignItems: "center", gap: 6, flexShrink: 0 }}>
                    {applied ? (
                      <span
                        style={{
                          display: "inline-flex",
                          alignItems: "center",
                          gap: 4,
                          fontSize: 11.5,
                          fontWeight: 600,
                          color: "var(--accent)",
                        }}
                      >
                        <Check size={13} />
                        {t("theme.customApplied")}
                      </span>
                    ) : (
                      <button
                        type="button"
                        onClick={() => void custom.apply(theme.id)}
                        disabled={custom.busy}
                        style={{
                          padding: "4px 10px",
                          borderRadius: 6,
                          border: "1px solid var(--border-medium)",
                          background: "var(--bg-card)",
                          color: "var(--text-primary)",
                          fontSize: 11.5,
                          cursor: custom.busy ? "default" : "pointer",
                          opacity: custom.busy ? 0.6 : 1,
                        }}
                      >
                        {t("theme.customApply")}
                      </button>
                    )}
                    <button
                      type="button"
                      onClick={() => void custom.remove(theme.id)}
                      disabled={custom.busy}
                      aria-label={t("theme.customDeleteAria", { name: theme.name })}
                      title={t("theme.customDelete")}
                      style={{
                        display: "inline-flex",
                        alignItems: "center",
                        padding: 5,
                        borderRadius: 6,
                        border: "1px solid var(--border-dim)",
                        background: "transparent",
                        color: "var(--text-secondary)",
                        cursor: custom.busy ? "default" : "pointer",
                        opacity: custom.busy ? 0.6 : 1,
                      }}
                    >
                      <Trash2 size={13} />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
