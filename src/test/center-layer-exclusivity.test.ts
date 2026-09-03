import { describe, expect, it } from "vitest";
import {
  centerLayerVisibility,
  centerWorkspaceMode,
  type CenterWorkspaceMode,
} from "../components/project-page/viewMode";

/**
 * 中央工作区的覆盖层互斥。
 *
 * 这些面板改成常挂 + `display` 切换之后,它们同时在 DOM 里,而每层都是
 * `position:absolute; inset:0`。两层同时可见不会报错,只会静默相互遮盖 —— 这类
 * 故障在 JSX 里看不出来,只能靠这条不变量守住。
 */

const ALL_MODES: CenterWorkspaceMode[] = [
  null,
  "sftp",
  "shell",
  "docker",
  "ssh",
  "database",
  "notes",
];

describe("centerLayerVisibility", () => {
  it("任何模式下至多一个专属层可见", () => {
    for (const mode of ALL_MODES) {
      const layers = centerLayerVisibility(mode);
      const exclusive = [layers.sftp, layers.database, layers.docker, layers.notes];
      const visible = exclusive.filter(Boolean).length;
      expect(visible, `mode=${String(mode)} 有 ${visible} 个专属层可见`).toBeLessThanOrEqual(1);
    }
  });

  it("任何模式下都恰好有一层可见(专属层或主链)", () => {
    for (const mode of ALL_MODES) {
      const layers = centerLayerVisibility(mode);
      const all = [layers.sftp, layers.database, layers.docker, layers.notes, layers.primary];
      expect(all.filter(Boolean).length, `mode=${String(mode)}`).toBe(1);
    }
  });

  it("专属层可见时主链一定关掉", () => {
    for (const mode of ["sftp", "database", "docker", "notes"] as const) {
      expect(centerLayerVisibility(mode).primary, `mode=${mode}`).toBe(false);
    }
  });

  // shell 是叠在主链上面的另一层,SSH 分屏时左半边仍是主链 —— 两者都不能把主链关掉。
  it("shell 与 ssh 模式下主链仍然渲染", () => {
    expect(centerLayerVisibility("shell").primary).toBe(true);
    expect(centerLayerVisibility("ssh").primary).toBe(true);
  });

  it("无模式时只有主链", () => {
    const layers = centerLayerVisibility(null);
    expect(layers).toEqual({
      sftp: false,
      database: false,
      docker: false,
      notes: false,
      primary: true,
    });
  });

  it.each(["sftp", "database", "docker", "notes"] as const)("%s 模式只点亮自己", (mode) => {
    const layers = centerLayerVisibility(mode);
    expect(layers[mode]).toBe(true);
  });
});

describe("centerWorkspaceMode 与可见性联动", () => {
  /** 每个 rightPanel 值经过 centerWorkspaceMode 后都必须落到一个自洽的可见性组合。 */
  it.each([
    ["sftp", "sftp"],
    ["ssh", "ssh"],
    ["database", "database"],
    ["notes", "notes"],
    ["docker", "docker"],
  ] as const)("rightPanel=%s → mode=%s", (panel, expected) => {
    const mode = centerWorkspaceMode(panel);
    expect(mode).toBe(expected);
    const layers = centerLayerVisibility(mode);
    const all = [layers.sftp, layers.database, layers.docker, layers.notes, layers.primary];
    expect(all.filter(Boolean).length).toBe(1);
  });

  it("shell 覆盖 docker:同时给时 shell 优先,但主链仍在", () => {
    const mode = centerWorkspaceMode("docker", true);
    expect(mode).toBe("shell");
    expect(centerLayerVisibility(mode).docker).toBe(false);
    expect(centerLayerVisibility(mode).primary).toBe(true);
  });

  it("dock 类面板不占中央区,主链照常", () => {
    for (const panel of ["files", "search", "problems", "git-changes", null] as const) {
      const mode = centerWorkspaceMode(panel);
      expect(mode, `panel=${String(panel)}`).toBeNull();
      expect(centerLayerVisibility(mode).primary).toBe(true);
    }
  });
});
