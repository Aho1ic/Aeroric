import { describe, expect, it } from "vitest";
import { availableTaskTabKeys } from "./task-tabs";

describe("availableTaskTabKeys", () => {
  it("keeps all tabs visible until the host capabilities are negotiated", () => {
    expect(availableTaskTabKeys(false, () => false)).toEqual([
      "session",
      "terminal",
      "files",
      "changes",
    ]);
  });

  it("filters optional tabs to the capabilities supported by the host", () => {
    expect(
      availableTaskTabKeys(true, (capability) =>
        ["terminal.stream", "git.read"].includes(capability),
      ),
    ).toEqual(["session", "terminal", "changes"]);
  });
});
