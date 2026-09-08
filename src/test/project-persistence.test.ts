import { describe, expect, it, vi } from "vitest";
import { createProjectPersister } from "../projectPersistence";
import type { Project } from "../types";

function project(name: string): Project {
  return {
    id: "p1",
    name,
    path: "/project",
    lastOpenedAt: 1,
  };
}

describe("createProjectPersister", () => {
  it("serializes snapshots so an older write cannot finish after a newer one", async () => {
    let releaseFirst!: () => void;
    const first = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const save = vi
      .fn()
      .mockImplementationOnce(() => first)
      .mockImplementationOnce(() => Promise.resolve());
    const persist = createProjectPersister(save);

    persist([project("old")]);
    persist([project("latest")]);
    await Promise.resolve();

    expect(save).toHaveBeenCalledTimes(1);
    releaseFirst();
    await first;
    await Promise.resolve();
    await Promise.resolve();

    expect(save).toHaveBeenCalledTimes(2);
    expect(save).toHaveBeenLastCalledWith([project("latest")]);
  });

  it("continues the queue after a failed write", async () => {
    const onError = vi.fn();
    const save = vi
      .fn()
      .mockRejectedValueOnce(new Error("disk full"))
      .mockResolvedValueOnce(undefined);
    const persist = createProjectPersister(save);

    persist([project("first")], { onError });
    persist([project("second")], { onError });
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(onError).toHaveBeenCalledWith("Error: disk full");
    expect(save).toHaveBeenCalledTimes(2);
    expect(save).toHaveBeenLastCalledWith([project("second")]);
  });

  it("flush 等到队列排空,包含它让出那一拍里新排进来的快照", async () => {
    /* 退出前最后一次编辑正好排在 `await` 让出的那一拍里。只 await 一次的话它会被漏掉 ——
       表现是「刚改的项目名退出后没了」。 */
    let releaseFirst!: () => void;
    const first = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const saved: string[] = [];
    const save = vi.fn(async (projects: Project[]) => {
      if (projects[0]?.name === "old") await first;
      saved.push(String(projects[0]?.name));
    });
    const persist = createProjectPersister(save);

    persist([project("old")]);
    const flushed = persist.flush();
    persist([project("new")]);
    releaseFirst();
    await flushed;

    expect(saved).toEqual(["old", "new"]);
  });

  it("flush 把未被更晚成功取代的失败抛出来", async () => {
    const onError = vi.fn();
    const save = vi.fn().mockRejectedValueOnce(new Error("disk full"));
    const persist = createProjectPersister(save);

    persist([project("only")], { onError });

    await expect(persist.flush()).rejects.toThrow("disk full");
    // 后台那一路仍走既有的 onError 提示,不产生未捕获 rejection。
    expect(onError).toHaveBeenCalledWith("Error: disk full");
  });

  it("更晚的成功取代更早的失败 —— 项目文件是整份覆盖写", async () => {
    const save = vi
      .fn()
      .mockRejectedValueOnce(new Error("disk full"))
      .mockResolvedValueOnce(undefined);
    const persist = createProjectPersister(save);

    persist([project("first")]);
    persist([project("second")]);

    await expect(persist.flush()).resolves.toBeUndefined();
  });

  it("队列空着时 flush 直接落地", async () => {
    const persist = createProjectPersister(vi.fn().mockResolvedValue(undefined));
    await expect(persist.flush()).resolves.toBeUndefined();
  });
});
