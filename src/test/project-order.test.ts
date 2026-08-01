import { describe, expect, it } from "vitest";
import type { Project } from "../types";
import { applyProjectOrder, normalizeProjectOrder, sortProjectsForRail } from "../projectOrder";

function project(id: string, orderIndex?: number, pinned?: boolean): Project {
  return {
    id,
    name: id,
    path: `/tmp/${id}`,
    lastOpenedAt: Number(id.replace(/\D/g, "")) || 1,
    ...(orderIndex === undefined ? null : { orderIndex }),
    ...(pinned === undefined ? null : { pinned }),
  };
}

describe("project manual ordering", () => {
  it("assigns stable order indexes to legacy projects without changing existing order", () => {
    const normalized = normalizeProjectOrder([project("p3"), project("p1"), project("p2")]);

    expect(normalized.map((item) => [item.id, item.orderIndex])).toEqual([
      ["p3", 0],
      ["p1", 1],
      ["p2", 2],
    ]);
  });

  it("sorts the project rail by manual order while keeping unindexed projects at the end", () => {
    expect(
      sortProjectsForRail([project("p1", 20), project("p2"), project("p3", 10)]).map(
        (item) => item.id,
      ),
    ).toEqual(["p3", "p1", "p2"]);
  });
});

describe("project pinning", () => {
  it("lifts pinned projects above the rest while keeping manual order inside each bucket", () => {
    expect(
      sortProjectsForRail([
        project("p1", 0),
        project("p2", 1, true),
        project("p3", 2),
        project("p4", 3, true),
      ]).map((item) => item.id),
    ).toEqual(["p2", "p4", "p1", "p3"]);
  });

  it("treats pinned: false the same as an absent flag", () => {
    expect(
      sortProjectsForRail([project("p1", 0, false), project("p2", 1)]).map((item) => item.id),
    ).toEqual(["p1", "p2"]);
  });

  it("does not rewrite orderIndex when only the pin flag changes", () => {
    const projects = [project("p1", 0), project("p2", 1, true)];

    expect(sortProjectsForRail(projects).map((item) => item.orderIndex)).toEqual([1, 0]);
  });

  it("keeps a drag reorder authoritative and re-indexes from the pinned-first fallback", () => {
    const projects = [project("p1", 0), project("p2", 1, true), project("p3", 2)];

    expect(
      applyProjectOrder(projects, ["p3", "p1"]).map((item) => [item.id, item.orderIndex]),
    ).toEqual([
      ["p3", 0],
      ["p1", 1],
      ["p2", 2],
    ]);
  });
});
