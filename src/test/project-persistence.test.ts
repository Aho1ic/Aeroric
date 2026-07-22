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
});
