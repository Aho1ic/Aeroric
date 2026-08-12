import { describe, expect, it } from "vitest";
import { APP_SETTINGS_CHANGED_EVENT } from "../components/app-settings/types";
import { applyProjectPinnedChange, dispatchAppSettingsChanged } from "../appRemoteEvents";
import type { Project } from "../types";

function project(id: string, pinned = false): Project {
  return { id, name: id, path: `/${id}`, lastOpenedAt: 1, pinned };
}

describe("applyProjectPinnedChange", () => {
  it("只更新目标项目并保留桌面端尚未落盘的其他字段", () => {
    const projects = [{ ...project("a"), name: "local rename" }, project("b")];

    const next = applyProjectPinnedChange(projects, { projectId: "b", pinned: true });

    expect(next).toEqual([projects[0], { ...projects[1], pinned: true }]);
    expect(next[0]).toBe(projects[0]);
  });

  it("目标不存在或状态未变化时保留数组引用", () => {
    const projects = [project("a", true)];
    expect(applyProjectPinnedChange(projects, { projectId: "a", pinned: true })).toBe(projects);
    expect(applyProjectPinnedChange(projects, { projectId: "missing", pinned: false })).toBe(
      projects,
    );
  });
});

describe("dispatchAppSettingsChanged", () => {
  it("把后端设置变更桥接到 DOM 事件总线", () => {
    let received = false;
    window.addEventListener(
      APP_SETTINGS_CHANGED_EVENT,
      () => {
        received = true;
      },
      { once: true },
    );

    dispatchAppSettingsChanged(window);

    expect(received).toBe(true);
  });
});
