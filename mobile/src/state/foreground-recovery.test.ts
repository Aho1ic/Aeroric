import { describe, expect, it, vi } from "vitest";
import type { AppStateStatus } from "react-native";
import {
  subscribeForegroundConnectionRecovery,
  type AppStateChangeSource,
} from "./foreground-recovery";

class FakeAppState implements AppStateChangeSource {
  currentState: AppStateStatus | null = "active";
  private listener: ((nextState: AppStateStatus) => void) | null = null;
  readonly remove = vi.fn(() => {
    this.listener = null;
  });

  addEventListener(
    type: "change",
    listener: (nextState: AppStateStatus) => void,
  ): { remove(): void } {
    expect(type).toBe("change");
    this.listener = listener;
    return { remove: this.remove };
  }

  emit(nextState: AppStateStatus): void {
    this.currentState = nextState;
    this.listener?.(nextState);
  }
}

describe("ConnectionProvider foreground recovery subscription", () => {
  it("forwards the initial unknown-to-active transition", () => {
    const appState = new FakeAppState();
    appState.currentState = "unknown";
    const notifyForeground = vi.fn();
    const dispose = subscribeForegroundConnectionRecovery(appState, notifyForeground);

    appState.emit("active");

    expect(notifyForeground).toHaveBeenCalledTimes(1);
    dispose();
  });

  it("forwards every active edge, including inactive-to-active", () => {
    const appState = new FakeAppState();
    const notifyForeground = vi.fn();
    const dispose = subscribeForegroundConnectionRecovery(appState, notifyForeground);

    appState.emit("inactive");
    appState.emit("active");
    appState.emit("active");
    appState.emit("background");
    appState.emit("active");

    // A duplicate active event must not reset a healthy but slow dial. Each
    // actual return-to-active edge still reaches RemoteConnection.
    expect(notifyForeground).toHaveBeenCalledTimes(2);
    dispose();
  });

  it("removes the AppState listener when the provider unmounts", () => {
    const appState = new FakeAppState();
    const notifyForeground = vi.fn();
    const dispose = subscribeForegroundConnectionRecovery(appState, notifyForeground);

    dispose();
    appState.emit("active");

    expect(appState.remove).toHaveBeenCalledTimes(1);
    expect(notifyForeground).not.toHaveBeenCalled();
  });
});
