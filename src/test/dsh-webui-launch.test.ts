import { describe, expect, it, vi } from "vitest";
import { launchDshWebUi } from "../dshWebUi";

describe("DSH Web UI launch", () => {
  it("opens the URL returned by a running Web UI process", async () => {
    const start = vi.fn().mockResolvedValue({
      status: "running",
      url: "http://127.0.0.1:15800",
    });
    const open = vi.fn().mockResolvedValue(undefined);

    await launchDshWebUi("dsh", start, open);

    expect(start).toHaveBeenCalledWith("dsh");
    expect(open).toHaveBeenCalledWith("http://127.0.0.1:15800");
  });

  it("does not open a URL when the Web UI process reports an error", async () => {
    const start = vi.fn().mockResolvedValue({
      status: "error",
      url: null,
      error: "health check failed",
    });
    const open = vi.fn().mockResolvedValue(undefined);

    await expect(launchDshWebUi("dsh", start, open)).rejects.toThrow("health check failed");
    expect(open).not.toHaveBeenCalled();
  });
});
