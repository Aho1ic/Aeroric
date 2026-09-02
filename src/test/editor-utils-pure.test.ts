import { describe, expect, it } from "vitest";
import {
  diagnosticFilterOptions,
  diagnosticSeverityColor,
  sqliteEndpointForFile,
} from "../components/file-viewer/editorUtils";
import type { SshConnection } from "../types";

/**
 * `editorUtils` 里不需要挂 CodeMirror 就能测的部分。
 * 懒加载语言表和两个扩展工厂在 `editor-utils-extensions.test.ts`。
 */

const sshConnection = {
  id: "conn-1",
  name: "prod",
  host: "prod.example.com",
  port: 22,
  username: "deploy",
  createdAt: 1,
} as SshConnection;

describe("sqliteEndpointForFile", () => {
  it("没有 remote 时是本地端点", () => {
    expect(sqliteEndpointForFile("/tmp/app.db")).toEqual({ kind: "local", path: "/tmp/app.db" });
  });

  it("ssh 项目带上连接与项目路径", () => {
    expect(
      sqliteEndpointForFile("/srv/app.db", {
        kind: "ssh",
        connection: sshConnection,
        projectPath: "/srv/project",
      }),
    ).toEqual({
      kind: "ssh",
      connection: sshConnection,
      path: "/srv/app.db",
      projectPath: "/srv/project",
    });
  });

  it("文件路径原样透传,不做规范化", () => {
    // 后端按原样打开;这里若擅自 normalize,远端相对路径会解析到错的位置。
    const endpoint = sqliteEndpointForFile("./data/../data/app.sqlite");
    expect(endpoint).toEqual({ kind: "local", path: "./data/../data/app.sqlite" });
  });

  it("wsl 项目目前退化成本地端点", () => {
    // 现状记录,不是本次要修的:DbEndpoint(src/types/database.ts:15)只有
    // local / ssh 两支,没有 wsl。所以 WSL 项目里打开 .db 会按本地路径去开。
    // 要修得先扩类型再动后端,不属于「不影响功能」的范围。
    expect(
      sqliteEndpointForFile("/home/u/app.db", {
        kind: "wsl",
        distribution: "Ubuntu",
        projectPath: "/home/u/project",
      }),
    ).toEqual({ kind: "local", path: "/home/u/app.db" });
  });
});

describe("diagnosticSeverityColor", () => {
  it("error / warning 各有专色", () => {
    expect(diagnosticSeverityColor("error")).toBe("var(--danger-fg)");
    expect(diagnosticSeverityColor("warning")).toBe("var(--warning)");
  });

  it("info 走 accent", () => {
    // DiagnosticSeverity 就是 error|warning|info 三个闭集(src/types.ts:588),
    // 函数里最后那句 `return "var(--accent)"` 只有 info 能走到。
    expect(diagnosticSeverityColor("info")).toBe("var(--accent)");
  });

  it("三个颜色互不相同(否则筛选器视觉上分不开)", () => {
    const colors = new Set(
      (["error", "warning", "info"] as const).map((s) => diagnosticSeverityColor(s)),
    );
    expect(colors.size).toBe(3);
  });
});

describe("diagnosticFilterOptions", () => {
  it("顺序是 all → error → warning → info", () => {
    // 下拉里按这个顺序渲染,严重度从高到低。
    expect(diagnosticFilterOptions).toEqual(["all", "error", "warning", "info"]);
  });

  it("除 all 之外每一项都能拿到颜色", () => {
    for (const option of diagnosticFilterOptions) {
      if (option === "all") continue;
      expect(diagnosticSeverityColor(option), option).toMatch(/^var\(--/);
    }
  });
});
