import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { applyLspWorkspaceEdit, requestLspRename } from "../components/file-viewer/lspRename";
import type { LspDocumentRequest } from "../hooks/languageServerState";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const request: LspDocumentRequest = {
  projectPath: "/tmp/aeroric",
  filePath: "/tmp/aeroric/src/App.tsx",
  content: "const value = helper();\n",
  line: 0,
  character: 14,
};

const remote = {
  connection: {
    id: "ssh-1",
    name: "remote",
    host: "127.0.0.1",
    port: 22,
    username: "dev",
    createdAt: 1,
  },
  projectPath: "/srv/app",
};

describe("LSP rename state", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("requests a rename workspace edit", async () => {
    const edit = {
      files: [
        {
          uri: "file:///tmp/aeroric/src/App.tsx",
          path: "/tmp/aeroric/src/App.tsx",
          edits: [
            {
              range: {
                start: { line: 0, character: 14 },
                end: { line: 0, character: 20 },
              },
              newText: "renamed",
            },
          ],
        },
      ],
    };
    vi.mocked(invoke).mockResolvedValue(edit);

    const result = await requestLspRename(request, "renamed");

    expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_rename", {
      request,
      newName: "renamed",
    });
    expect(result).toEqual(edit);
  });

  it("applies a confirmed workspace edit", async () => {
    const edit = { files: [] };
    vi.mocked(invoke).mockResolvedValue({
      filesChanged: 1,
      editsApplied: 2,
      editsSkipped: 0,
    });

    const result = await applyLspWorkspaceEdit("/tmp/aeroric", edit);

    expect(vi.mocked(invoke)).toHaveBeenCalledWith("lsp_apply_workspace_edit", {
      projectPath: "/tmp/aeroric",
      edit,
    });
    expect(result).toEqual({
      filesChanged: 1,
      editsApplied: 2,
      editsSkipped: 0,
    });
  });

  it("uses remote LSP commands for remote rename and apply", async () => {
    const edit = { files: [] };
    vi.mocked(invoke)
      .mockResolvedValueOnce(edit)
      .mockResolvedValueOnce({ filesChanged: 1, editsApplied: 1, editsSkipped: 0 });

    await requestLspRename(request, "renamed", remote);
    await applyLspWorkspaceEdit("/srv/app", edit, remote);

    expect(vi.mocked(invoke)).toHaveBeenNthCalledWith(1, "remote_lsp_rename", {
      connection: remote.connection,
      remoteProjectPath: "/srv/app",
      request,
      newName: "renamed",
    });
    expect(vi.mocked(invoke)).toHaveBeenNthCalledWith(2, "remote_lsp_apply_workspace_edit", {
      connection: remote.connection,
      remoteProjectPath: "/srv/app",
      projectPath: "/srv/app",
      edit,
    });
  });
});
