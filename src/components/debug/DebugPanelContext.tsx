import { createScopedStore } from "../../state/createScopedStore";
import { createDebugPanelStore, type DebugPanelUiState } from "./debugPanelStore";

const scoped = createScopedStore<DebugPanelUiState>("DebugPanel", createDebugPanelStore);
export const DebugPanelProvider = scoped.Provider;
export const useDebugPanelStore = scoped.useScopedStore;
