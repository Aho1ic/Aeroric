import { describe, expect, it } from "vitest";
import type { Project } from "../types";
import { normalizeProjectOrder, sortProjectsForRail } from "../projectOrder";

function project(id: string, orderIndex?: number): Project {
  return {
    id,
    name: id,
    path: `/tmp/${id}`,
    lastOpenedAt: Number(id.replace(/\D/g, "")) || 1,
    ...(orderIndex === undefined ? null : { orderIndex }),
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
