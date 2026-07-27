import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AGENT_OPTIONS, agentOptionsFromProfiles, type AgentOption } from "../agents";
import { APP_SETTINGS_CHANGED_EVENT, type AppSettings } from "../components/app-settings/types";

let cachedSettings: AppSettings | null = null;
let cachedOptions: AgentOption[] = AGENT_OPTIONS;
let loadPromise: Promise<AppSettings> | null = null;
const subscribers = new Set<() => void>();

function emitChange() {
  for (const subscriber of subscribers) subscriber();
}

function updateCache(settings: AppSettings) {
  cachedSettings = settings;
  cachedOptions = agentOptionsFromProfiles(
    settings.custom_agents ?? [],
    settings.agent_label_overrides ?? {},
  );
  emitChange();
  return settings;
}

function loadAgentSettings(force = false): Promise<AppSettings> {
  if (!force && cachedSettings) return Promise.resolve(cachedSettings);
  if (loadPromise) return loadPromise;

  loadPromise = invoke<AppSettings>("load_app_settings")
    .then(updateCache)
    .finally(() => {
      loadPromise = null;
    });
  return loadPromise;
}

function handleSettingsChanged() {
  void loadAgentSettings(true).catch(() => {});
}

function subscribe(subscriber: () => void) {
  if (subscribers.size === 0 && typeof window !== "undefined") {
    window.addEventListener(APP_SETTINGS_CHANGED_EVENT, handleSettingsChanged);
  }
  subscribers.add(subscriber);
  return () => {
    subscribers.delete(subscriber);
    if (subscribers.size === 0 && typeof window !== "undefined") {
      window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, handleSettingsChanged);
    }
  };
}

function useEnsureAgentSettingsLoaded() {
  useEffect(() => {
    void loadAgentSettings().catch(() => {});
  }, []);
}

export function useAgentOptions(): AgentOption[] {
  const options = useSyncExternalStore(
    subscribe,
    () => cachedOptions,
    () => AGENT_OPTIONS,
  );
  useEnsureAgentSettingsLoaded();
  return options;
}

export function useAgentSettings(): AppSettings | null {
  const settings = useSyncExternalStore(
    subscribe,
    () => cachedSettings,
    () => null,
  );
  useEnsureAgentSettingsLoaded();
  return settings;
}

export function invalidateAgentSettingsCache() {
  cachedSettings = null;
  cachedOptions = AGENT_OPTIONS;
  emitChange();
}
