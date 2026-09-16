/* 「清理与周报」设置面板。
 *
 * 这两组配置都在 Rust 侧的 `AppSettings` 里(后端要读它们),所以面板自己
 * `invoke("load_app_settings")` 取值、自己调专用 setter 写回 —— 走 `SettingsPanelProps`
 * 那条 prop 链要串 App → EventHost → Dialog → registry → Panel 五层,还要改一批测试的
 * props 字面量,而这里一个 prop 都不需要。
 *
 * `APP_SETTINGS_CHANGED_EVENT` 两头都要接:
 * - **写回后必须发。** 周报按钮与自动清理都监听它,不发的话用户改完配置得重启才生效。
 * - **也必须听。** 自己不是这段设置的唯一写入方,只在挂载时读一次的话,别处改完这里
 *   显示的还是打开那一刻的快照。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import type React from "react";
import { Check, ChevronDown, FolderOpen } from "lucide-react";
import * as Select from "@radix-ui/react-select";
import { invoke } from "../../lib/api/invoke";
import { useI18n } from "../../i18n";
import s from "../../styles";
import { pickExportDir } from "../notebook/noteExport";
import {
  APP_SETTINGS_CHANGED_EVENT,
  normalizeAutoCleanupSettings,
  normalizeWeeklyReportSettings,
} from "./types";
import type { AppSettings, AutoCleanupSettings, WeeklyReportSettings } from "./types";

const WEEKDAYS = [0, 1, 2, 3, 4, 5, 6] as const;
const HOURS = Array.from({ length: 24 }, (_, hour) => hour);
const INTERVAL_DAY_OPTIONS = [1, 3, 7, 14, 30, 90] as const;
const RETAIN_DAY_OPTIONS = [7, 14, 30, 60, 90, 180, 365] as const;

export function CleanupReportPanel() {
  const { t } = useI18n();
  const [cleanup, setCleanup] = useState<AutoCleanupSettings>(() =>
    normalizeAutoCleanupSettings(undefined),
  );
  const [report, setReport] = useState<WeeklyReportSettings>(() =>
    normalizeWeeklyReportSettings(undefined),
  );
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  /** 见下面那段注释:标记"接下来这一次广播是我自己发的,不用重读"。 */
  const skipNextChangeEventRef = useRef(false);

  const applyLoaded = useCallback((loaded: AppSettings) => {
    setCleanup(normalizeAutoCleanupSettings(loaded.auto_cleanup_settings));
    setReport(normalizeWeeklyReportSettings(loaded.weekly_report_settings));
  }, []);

  /* 挂载读一次,之后跟着 `APP_SETTINGS_CHANGED_EVENT` 重读。同一段设置还有别的入口在写
     (自动清理跑完会更新 `last_run_at`、周报按钮也写 `output_dir`),只读一次的话面板
     显示的是打开那一刻的快照,用户看到的是陈旧值却以为是当前配置。

     `skipNextChangeEventRef` 挡掉自己那一次:`persist` 已经把 Rust 返回的规范化结果写进
     state 了,再为自己的广播读一次盘纯属多余,且那次读盘的结果会覆盖掉刚落地的值(内容
     相同,但多一次 IPC 与一次重渲染)。 */
  useEffect(() => {
    let cancelled = false;
    const load = () => {
      invoke<AppSettings>("load_app_settings")
        .then((loaded) => {
          if (!cancelled) applyLoaded(loaded);
        })
        .catch((loadError: unknown) => {
          if (!cancelled) setError(String(loadError));
        });
    };
    load();
    const handleSettingsChanged = () => {
      if (skipNextChangeEventRef.current) {
        skipNextChangeEventRef.current = false;
        return;
      }
      load();
    };
    window.addEventListener(APP_SETTINGS_CHANGED_EVENT, handleSettingsChanged);
    return () => {
      cancelled = true;
      window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, handleSettingsChanged);
    };
  }, [applyLoaded]);

  /**
   * 乐观改本地 state,再把 Rust 返回的规范化结果覆盖回来。
   * 失败时重新读盘取真值 —— 直接回滚到改之前会掩盖"部分写成功"的情况。
   */
  const persist = useCallback(
    async (command: string, payload: Record<string, unknown>) => {
      setSaving(true);
      setError(null);
      try {
        applyLoaded(await invoke<AppSettings>(command, payload));
        skipNextChangeEventRef.current = true;
        window.dispatchEvent(new Event(APP_SETTINGS_CHANGED_EVENT));
      } catch (persistError: unknown) {
        setError(String(persistError));
        await invoke<AppSettings>("load_app_settings")
          .then(applyLoaded)
          .catch(() => {});
      } finally {
        setSaving(false);
      }
    },
    [applyLoaded],
  );

  const saveCleanup = useCallback(
    (patch: Partial<AutoCleanupSettings>) => {
      const next = normalizeAutoCleanupSettings({ ...cleanup, ...patch });
      setCleanup(next);
      void persist("update_auto_cleanup_settings", { autoCleanupSettings: next });
    },
    [cleanup, persist],
  );

  const saveReport = useCallback(
    (patch: Partial<WeeklyReportSettings>) => {
      const next = normalizeWeeklyReportSettings({ ...report, ...patch });
      setReport(next);
      void persist("update_weekly_report_settings", { weeklyReportSettings: next });
    },
    [persist, report],
  );

  const labelStyle: React.CSSProperties = {
    fontSize: 12,
    fontWeight: 600,
    color: "var(--text-secondary)",
    marginBottom: 5,
    display: "block",
  };
  const fieldStyle: React.CSSProperties = { display: "flex", flexDirection: "column", gap: 5 };
  const spacedFieldStyle: React.CSSProperties = { ...fieldStyle, marginTop: 18 };
  const hintStyle: React.CSSProperties = { fontSize: 11, color: "var(--text-hint)", marginTop: 3 };
  const selectTriggerStyle: React.CSSProperties = { ...s.settingsSelectTrigger, width: 220 };
  const sectionStyle: React.CSSProperties = {
    fontSize: 12.5,
    fontWeight: 700,
    color: "var(--text-primary)",
    marginBottom: 10,
  };

  function renderSelect<T extends string | number>(
    ariaLabel: string,
    value: T,
    options: ReadonlyArray<{ value: T; label: string }>,
    onChange: (value: T) => void,
    parse: (raw: string) => T,
  ) {
    const selectedLabel = options.find((option) => option.value === value)?.label ?? String(value);
    return (
      <Select.Root value={String(value)} onValueChange={(raw) => onChange(parse(raw))}>
        <Select.Trigger aria-label={ariaLabel} style={selectTriggerStyle} disabled={saving}>
          <Select.Value>{selectedLabel}</Select.Value>
          <Select.Icon>
            <ChevronDown size={13} strokeWidth={2.2} color="var(--text-hint)" />
          </Select.Icon>
        </Select.Trigger>
        <Select.Portal>
          <Select.Content position="popper" sideOffset={4} style={s.settingsSelectContent}>
            <Select.Viewport style={s.settingsSelectViewport}>
              {options.map((option) => (
                <Select.Item
                  key={String(option.value)}
                  value={String(option.value)}
                  className="radix-select-item"
                  style={
                    option.value === value ? s.settingsSelectOptionSelected : s.settingsSelectOption
                  }
                >
                  <Select.ItemText>{option.label}</Select.ItemText>
                  <Select.ItemIndicator style={s.settingsSelectIndicator}>
                    <Check size={13} style={s.settingsSelectCheck} />
                  </Select.ItemIndicator>
                </Select.Item>
              ))}
            </Select.Viewport>
          </Select.Content>
        </Select.Portal>
      </Select.Root>
    );
  }

  const weekdayOptions = WEEKDAYS.map((value) => ({
    value,
    label: t(`common.weekday.${value}`),
  }));
  const toInt = (raw: string) => Number.parseInt(raw, 10);

  return (
    <div
      style={{
        ...s.settingsBody,
        display: "flex",
        flexDirection: "column",
        gap: 0,
        padding: "20px",
      }}
    >
      <div style={sectionStyle}>{t("appSettings.autoCleanup")}</div>

      <div style={fieldStyle}>
        <button
          type="button"
          role="switch"
          aria-checked={cleanup.enabled}
          aria-label={t("appSettings.autoCleanupToggle")}
          onClick={() => saveCleanup({ enabled: !cleanup.enabled })}
          style={s.settingToggle}
        >
          <span style={s.settingToggleLabel}>{t("appSettings.autoCleanupToggle")}</span>
          <span
            style={{
              ...s.settingToggleTrack,
              background: cleanup.enabled ? "var(--primary-action-bg)" : "var(--border-medium)",
            }}
          >
            <span
              style={{
                ...s.settingToggleKnob,
                transform: cleanup.enabled ? "translateX(16px)" : "translateX(0)",
              }}
            />
          </span>
        </button>
        <span style={{ ...hintStyle, color: "var(--danger)" }}>
          {t("appSettings.autoCleanupHint")}
        </span>
      </div>

      <div style={spacedFieldStyle}>
        <label style={labelStyle}>{t("appSettings.autoCleanupMode")}</label>
        {renderSelect(
          t("appSettings.autoCleanupMode"),
          cleanup.mode,
          [
            { value: "weekly" as const, label: t("appSettings.autoCleanupModeWeekly") },
            { value: "interval" as const, label: t("appSettings.autoCleanupModeInterval") },
          ],
          (mode) => saveCleanup({ mode }),
          (raw) => (raw === "interval" ? "interval" : "weekly"),
        )}
      </div>

      {cleanup.mode === "weekly" ? (
        <>
          <div style={spacedFieldStyle}>
            <label style={labelStyle}>{t("appSettings.autoCleanupWeekday")}</label>
            {renderSelect(
              t("appSettings.autoCleanupWeekday"),
              cleanup.weekday,
              weekdayOptions,
              (weekday) => saveCleanup({ weekday }),
              toInt,
            )}
          </div>
          <div style={spacedFieldStyle}>
            <label style={labelStyle}>{t("appSettings.autoCleanupHour")}</label>
            {renderSelect(
              t("appSettings.autoCleanupHour"),
              cleanup.hour,
              HOURS.map((hour) => ({
                value: hour,
                label: t("appSettings.autoCleanupHourValue", {
                  hour: String(hour).padStart(2, "0"),
                }),
              })),
              (hour) => saveCleanup({ hour }),
              toInt,
            )}
          </div>
        </>
      ) : (
        <div style={spacedFieldStyle}>
          <label style={labelStyle}>{t("appSettings.autoCleanupIntervalDays")}</label>
          {renderSelect(
            t("appSettings.autoCleanupIntervalDays"),
            cleanup.interval_days,
            INTERVAL_DAY_OPTIONS.map((days) => ({
              value: days,
              label: t("appSettings.autoCleanupDays", { days }),
            })),
            (interval_days) => saveCleanup({ interval_days }),
            toInt,
          )}
        </div>
      )}

      <div style={spacedFieldStyle}>
        <label style={labelStyle}>{t("appSettings.autoCleanupRetainDays")}</label>
        {renderSelect(
          t("appSettings.autoCleanupRetainDays"),
          cleanup.retain_days,
          RETAIN_DAY_OPTIONS.map((days) => ({
            value: days,
            label: t("appSettings.autoCleanupDays", { days }),
          })),
          (retain_days) => saveCleanup({ retain_days }),
          toInt,
        )}
        <span style={hintStyle}>{t("appSettings.autoCleanupRetainHint")}</span>
      </div>

      <div style={{ ...sectionStyle, marginTop: 26 }}>{t("appSettings.weeklyReport")}</div>

      <div style={fieldStyle}>
        <label style={labelStyle}>{t("appSettings.weekStartDay")}</label>
        {renderSelect(
          t("appSettings.weekStartDay"),
          report.week_start_day,
          weekdayOptions,
          (week_start_day) => saveReport({ week_start_day }),
          toInt,
        )}
      </div>

      <div style={spacedFieldStyle}>
        <label style={labelStyle}>{t("appSettings.weekEndDay")}</label>
        {renderSelect(
          t("appSettings.weekEndDay"),
          report.week_end_day,
          weekdayOptions,
          (week_end_day) => saveReport({ week_end_day }),
          toInt,
        )}
        <span style={hintStyle}>{t("appSettings.weekRangeHint")}</span>
      </div>

      <div style={spacedFieldStyle}>
        <label style={labelStyle}>{t("appSettings.reportOutputDir")}</label>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <input
            type="text"
            value={report.output_dir}
            onChange={(event) => setReport({ ...report, output_dir: event.currentTarget.value })}
            onBlur={(event) => saveReport({ output_dir: event.currentTarget.value.trim() })}
            style={{ ...s.settingsSelectTrigger, width: "min(100%, 420px)", cursor: "text" }}
            spellCheck={false}
          />
          <button
            type="button"
            aria-label={t("appSettings.browse")}
            title={t("appSettings.browse")}
            onClick={() => {
              void pickExportDir(t("report.pickOutputDir")).then((picked) => {
                if (picked) saveReport({ output_dir: picked });
              });
            }}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 5,
              height: 28,
              padding: "0 10px",
              border: "1px solid var(--border-medium)",
              borderRadius: 6,
              background: "var(--bg-card)",
              color: "var(--text-primary)",
              fontFamily: "var(--font-ui)",
              fontSize: 11.5,
              cursor: "pointer",
            }}
          >
            <FolderOpen size={12} strokeWidth={2.2} />
            <span>{t("appSettings.browse")}</span>
          </button>
        </div>
        <span style={hintStyle}>{t("appSettings.reportOutputDirHint")}</span>
      </div>

      {error && (
        <span style={{ ...hintStyle, color: "var(--danger)", marginTop: 16 }}>{error}</span>
      )}
    </div>
  );
}
