import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ReadOnlyStorageBadge } from "../components/ReadOnlyStorageBadge";
import { I18nProvider } from "../i18n";
import type { StartupDegradation } from "../types";

vi.mock("../lib/api/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "../lib/api/invoke";

function rows(...entries: StartupDegradation[]) {
  return vi.mocked(invoke).mockResolvedValue(entries);
}

/**
 * Let the component's own read settle. Asserting the DOM right after `invoke` was called reads
 * a state that has not arrived yet — which is how an earlier version of the two negative cases
 * passed even when the filter was deliberately broken. Flushing the microtask chain (promise
 * resolution -> `.then` -> `setState`) makes the absence mean something.
 */
async function settleComponentReads() {
  await act(async () => {
    for (let hop = 0; hop < 5; hop += 1) {
      await Promise.resolve();
    }
  });
}

function renderBadge() {
  return render(
    <I18nProvider>
      <ReadOnlyStorageBadge />
    </I18nProvider>,
  );
}

describe("ReadOnlyStorageBadge — ADR-027 decision 4's persistent half", () => {
  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    vi.mocked(invoke).mockReset();
  });

  it("shows while a refused writer lease is on record, and names the directory", async () => {
    rows({
      component: "storage-lease",
      reason: "already held by another instance",
      fallback: "/home/u/.aeroric",
    });
    renderBadge();
    const badge = await screen.findByTestId("read-only-storage-badge");
    expect(badge.getAttribute("title")).toContain("/home/u/.aeroric");
  });

  it("stays up past a toast's lifetime, which is the whole point of the decision", async () => {
    rows({ component: "storage-lease", reason: "held", fallback: "/home/u/.aeroric" });
    renderBadge();
    const badge = await screen.findByTestId("read-only-storage-badge");
    vi.useFakeTimers();
    // The toast path this reuses expires on its own timer; a session-long state may not.
    vi.advanceTimersByTime(60_000);
    vi.useRealTimers();
    expect(screen.getByTestId("read-only-storage-badge")).toBe(badge);
  });

  it("stays out of the way for the instance that actually holds the lease", async () => {
    rows();
    renderBadge();
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("list_startup_degradations");
    await settleComponentReads();
    expect(screen.queryByTestId("read-only-storage-badge")).toBeNull();
  });

  it("does not fire for a degraded writer, only for a refused one", async () => {
    // `storage-location` means "this directory is cloud-synced / not ideal", and writes still
    // land. Saying 只读 there would tell the user their work is unsaved when it is being saved.
    rows({
      component: "storage-location",
      reason: "cloud-synced root",
      fallback: "/Users/x/Library/CloudStorage/Dropbox/.aeroric",
    });
    renderBadge();
    await settleComponentReads();
    expect(screen.queryByTestId("read-only-storage-badge")).toBeNull();
  });

  it("asks the same question the toasts ask, so the two cannot disagree", async () => {
    rows({ component: "storage-lease", reason: "held", fallback: "/d" });
    renderBadge();
    await screen.findByTestId("read-only-storage-badge");
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("list_startup_degradations");
  });
});
