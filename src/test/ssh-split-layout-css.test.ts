import { describe, expect, it } from "vitest";
import { SSH_SPLIT_GRID_TEMPLATE } from "../components/project-page/viewMode";

describe("SSH split layout CSS", () => {
  it("uses an exact one-pixel divider and shrinkable equal columns", () => {
    expect(SSH_SPLIT_GRID_TEMPLATE).toBe("minmax(0, 1fr) 1px minmax(0, 1fr)");
  });
});
