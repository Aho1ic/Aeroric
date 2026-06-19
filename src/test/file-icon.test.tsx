import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { FileIcon } from "../components/file-explorer/FileIcon";

describe("FileIcon", () => {
  it("uses the shared folder icon token", () => {
    const { container } = render(<FileIcon name="src" isDir />);

    expect(container.firstElementChild).toHaveStyle({ color: "var(--icon-folder)" });
  });

  it("uses the shared folder token for expanded folders too", () => {
    const { container } = render(<FileIcon name="src" isDir expanded />);

    expect(container.firstElementChild).toHaveStyle({ color: "var(--icon-folder)" });
  });
});
