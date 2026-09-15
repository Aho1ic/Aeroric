import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { FileIcon } from "../components/file-explorer/FileIcon";

describe("FileIcon", () => {
  it("uses the shared folder icon token for folders without a dedicated icon", () => {
    const { container } = render(<FileIcon name="zzz-unknown" isDir />);

    expect(container.firstElementChild).toHaveStyle({ color: "var(--icon-folder)" });
    expect(container.firstElementChild).toHaveAttribute("data-kind", "folder");
  });

  it("uses the shared folder token for expanded folders too", () => {
    const { container } = render(<FileIcon name="zzz-unknown" isDir expanded />);

    expect(container.firstElementChild).toHaveStyle({ color: "var(--icon-folder)" });
  });

  it("gives well-known folders their own kind", () => {
    const { container } = render(<FileIcon name="src" isDir />);

    expect(container.firstElementChild).toHaveAttribute("data-kind", "folder-code");
  });

  it("renders symlinks as links even when they resolve to a directory", () => {
    // fs.rs 对指向目录的链接同时给出 is_dir 与 is_symlink;链接必须赢,
    // 否则「链接到 src」和「真的 src」画成同一个图标。
    const { container } = render(<FileIcon name="src" isDir isSymlink />);

    expect(container.firstElementChild).toHaveAttribute("data-kind", "symlink");
  });

  it("keeps the glyph but dims the colour for gitignored entries", () => {
    const { container } = render(<FileIcon name="main.rs" ext="rs" isDir={false} isGitignored />);

    expect(container.firstElementChild).toHaveAttribute("data-kind", "code");
    expect(container.firstElementChild).toHaveStyle({ color: "var(--icon-file-ignored)" });
  });
});
