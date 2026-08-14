import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { AgentType } from "./types";

export interface DshWebUiState {
  status: "starting" | "running" | "stopped" | "error";
  url: string | null;
  error?: string;
}

type StartDshWebUi = (agent: AgentType) => Promise<DshWebUiState>;
type OpenWebUi = (url: string) => Promise<void>;

export async function launchDshWebUi(
  agent: AgentType,
  start: StartDshWebUi = (selectedAgent) =>
    invoke<DshWebUiState>("start_dsh_webui", { agent: selectedAgent }),
  open: OpenWebUi = openUrl,
): Promise<void> {
  const state = await start(agent);
  if (state.status !== "running" || !state.url) {
    throw new Error(state.error || "DeepSeek Harness Web UI did not become ready");
  }
  await open(state.url);
}
