import type { AppStateStatus } from "react-native";

/** The narrow AppState surface the connection provider needs. */
export interface AppStateChangeSource {
  currentState: AppStateStatus | null;
  addEventListener(
    type: "change",
    listener: (nextState: AppStateStatus) => void,
  ): { remove(): void };
}

/**
 * Pass every return-to-active edge to the connection layer.
 *
 * `inactive → active` does not always include an observable `background` event
 * (for example, after an iOS interruption). Duplicate `active` notifications
 * are intentionally ignored here: a delayed duplicate could otherwise restart
 * a healthy but slow dial after its stale-dial threshold.
 */
export function subscribeForegroundConnectionRecovery(
  appState: AppStateChangeSource,
  notifyForeground: () => void,
): () => void {
  let previousState = appState.currentState;
  const subscription = appState.addEventListener("change", (nextState) => {
    const becameActive = nextState === "active" && previousState !== "active";
    previousState = nextState;
    if (becameActive) notifyForeground();
  });
  return () => subscription.remove();
}
