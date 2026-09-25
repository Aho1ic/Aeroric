/**
 * 项目列表只有一个落盘点：`projects.json`，由 `save_projects` 整份覆盖写。
 * 于是"几个队列实例"不是风格问题而是正确性问题：`createProjectPersister` 只在**自己**那条
 * promise 链里排序（见 project-persistence.test.ts 的第一条用例），跨实例没有任何顺序；
 * 退出前的 flush 也只等它等得到的那一条链。
 *
 * 这里锁的是接线而不是机制：App 侧 `persistProjects` 与 store 侧的 ops 回退路径
 * （`AppOpsProvider` 把 rename/pin/reorder 直接接给 store 的 writer）必须共用同一个实例。
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createProjectPersister } from "../projectPersistence";
import { flushProjects, persistProjects } from "../appProjectState";
import { useProjectsStore } from "../state/app/projectsStore";
import { invoke } from "@tauri-apps/api/core";
import type { Project } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const project = (id: string, name: string): Project => ({
  id,
  name,
  path: `/p/${id}`,
  lastOpenedAt: 1,
});

/**
 * 让**store 发起的那一份**快照挂住，其它写立即完成。
 * 只有两条链其实是一条的时候，"等 App 的 flush"才会连带等到这份写；两个实例时
 * App 的队列是空的，flush 立刻返回 —— 这就是本文件用来区分两种架构的判据。
 */
function gateStoreSnapshots(storeMarker: string) {
  const written: Project[][] = [];
  let release!: () => void;
  invokeMock.mockImplementation(async (command: string, args?: unknown) => {
    if (command !== "save_projects") return undefined;
    const projects = (args as { projects?: Project[] } | undefined)?.projects;
    if (!Array.isArray(projects)) return undefined;
    if (projects[0]?.name === storeMarker) {
      await new Promise<void>((resolve) => {
        release = resolve;
      });
    }
    written.push(projects);
    return undefined;
  });
  return { written, release: () => release() };
}

/** 一个宏任务节拍：此时所有微任务续接都已跑完，仍未 resolve 的 promise 就不是"还没轮到"。 */
const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  invokeMock.mockReset();
  useProjectsStore.setState({
    projects: [],
    selectedProjectId: null,
    collapsedGroups: new Set(),
    railWidth: null,
  });
});

describe("project saves share one queue", () => {
  it("documents why two instances cannot be trusted on one whole-overwrite file", async () => {
    // 机制本身：两条链各自有序，彼此无序 —— 更晚的改动可能被更早的快照盖掉。
    const seen: string[] = [];
    let releaseFirst!: () => void;
    const blocking = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const queueA = createProjectPersister(async (projects) => {
      seen.push(`A:${projects[0].name}`);
      await blocking;
    });
    const queueB = createProjectPersister(async (projects) => {
      seen.push(`B:${projects[0].name}`);
    });

    queueA([project("p1", "older")]);
    await tick();
    queueB([project("p1", "newer")]);
    await tick();
    releaseFirst();
    await tick();

    // B 已经写完，A 才把更旧的快照落下去：这就是双实例的真实后果。
    expect(seen).toEqual(["A:older", "B:newer"]);
  });

  it("makes the host flush wait for a write queued through the store's ops path", async () => {
    const { release } = gateStoreSnapshots("Renamed");
    useProjectsStore.setState({ projects: [project("p1", "Renamed")] });

    // 走 AppOpsProvider 的回退路径：store 自己发起的落盘，会挂住。
    useProjectsStore.getState().renameProject("p1", "Renamed");
    await tick();

    let flushed = false;
    const pending = flushProjects().then(() => {
      flushed = true;
    });
    await tick();
    expect(
      flushed,
      "App 的 flush 立刻返回了 —— 说明 store 那份写排在另一个队列实例里，退出流程不会等它，" +
        "用户刚改的名字会在下一次整份覆盖时丢掉",
    ).toBe(false);

    release();
    await pending;
    expect(flushed).toBe(true);
  });

  it("issues no project write after the host flush has resolved", async () => {
    const { written, release } = gateStoreSnapshots("Renamed");
    useProjectsStore.setState({ projects: [project("p1", "Renamed")] });
    useProjectsStore.getState().renameProject("p1", "Renamed");
    await tick();

    // App 侧随后写一份更新的快照。
    persistProjects([project("p1", "Newest")], () => {}, String);

    const flushDone = flushProjects().then(() => written.length);
    await tick();
    release();
    const settledBeforeStoreWrite = await flushDone;
    await tick();

    expect(
      written.slice(settledBeforeStoreWrite).length,
      "flush 已 resolve 之后还落过盘 —— 两份快照不在同一条链上，退出时序无法保证",
    ).toBe(0);
    expect(written.map((snapshot) => snapshot[0].name)).toEqual(["Renamed", "Newest"]);
  });
});
