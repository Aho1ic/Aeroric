import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface PlatformRuntimeInfo {
  os: string;
  arch: string;
  shellKind: string;
  shellLabel: string;
  pathSeparator: string;
  canRunShellScripts: boolean;
  shellScriptUnavailableReason: string;
}

const fallback: PlatformRuntimeInfo = {
  os: "",
  arch: "",
  shellKind: "",
  shellLabel: "Shell",
  pathSeparator: "/",
  canRunShellScripts: true,
  shellScriptUnavailableReason: "",
};

let cached: PlatformRuntimeInfo | null = null;

export function usePlatformRuntimeInfo(): PlatformRuntimeInfo {
  const [runtime, setRuntime] = useState<PlatformRuntimeInfo>(() => cached ?? fallback);

  useEffect(() => {
    if (cached) {
      setRuntime(cached);
      return;
    }
    void invoke<PlatformRuntimeInfo>("get_platform_runtime_info")
      .then((value) => {
        if (!value || typeof value.shellLabel !== "string" || value.shellLabel.length === 0) {
          return;
        }
        cached = value;
        setRuntime(value);
      })
      .catch(() => {});
  }, []);

  return runtime;
}
