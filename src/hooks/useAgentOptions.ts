import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AGENT_OPTIONS, agentOptionsFromProfiles, type AgentOption } from "../agents";
import { APP_SETTINGS_CHANGED_EVENT, type AppSettings } from "../components/app-settings/types";

export function useAgentOptions(): AgentOption[] {
  const [options, setOptions] = useState<AgentOption[]>(AGENT_OPTIONS);

  useEffect(() => {
    let cancelled = false;

    const load = () => {
      invoke<AppSettings>("load_app_settings")
        .then((settings) => {
          if (!cancelled) {
            setOptions(agentOptionsFromProfiles(settings.custom_agents ?? []));
          }
        })
        .catch(() => {
          if (!cancelled) setOptions(AGENT_OPTIONS);
        });
    };

    load();
    window.addEventListener(APP_SETTINGS_CHANGED_EVENT, load);
    return () => {
      cancelled = true;
      window.removeEventListener(APP_SETTINGS_CHANGED_EVENT, load);
    };
  }, []);

  return options;
}
