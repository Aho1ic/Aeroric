import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import type { Project } from "../types";
import { resolveProjectLocation, sshProjectPath, wslProjectPath } from "../types";
import { deriveProjectName, normalizeRemotePath, normalizeWslProjectPath, upsertWslProject } from "../appProjectState";
import { applyProjectOrder, normalizeProjectOrder, sortProjectsForRail } from "../projectOrder";
import {
  groupProjectsForRail,
  mergeProjectGroupNames,
  normalizeProjectGroupNames,
  projectGroupForProject,
} from "../projectGroups";
import {
  assignProjectGroup,
  clearProjectGroupFromList,
  renameProjectGroupInList,
  renameProjectInList,
  setProjectAvatarInList,
  toggleProjectHiddenInList,
  toggleProjectPinnedInList,
  upsertLocalProject,
  upsertSshProject,
} from "../state/app/projectMutations";
import { touchProjectInList } from "../state/app/projectMutations";

/**
 * This file is the *source of truth* for `contracts/native/v1/fixtures/projects.json`.
 *
 * Every case is run through the shipping client's own TypeScript function — never a
 * re-implementation — and the result is written out. The Rust core is then tested against
 * that artifact, and this file asserts the artifact still matches what the TypeScript
 * produces. If a later change to either side moves a result, one of the two assertions
 * here fails; nobody has to remember to re-derive a table by hand.
 *
 * `AERORIC_WRITE_PROJECT_EXPECTATIONS=1` re-emits the expectations (used once, when the
 * fixture is created or when a deliberate behaviour change is being reviewed).
 */

type Case = {
  name: string;
  operation: string;
  input: Record<string, unknown>;
};

const P = (fields: Partial<Project> & { id: string }): Project => ({
  name: fields.name ?? fields.id,
  path: fields.path ?? `/repo/${fields.id}`,
  lastOpenedAt: fields.lastOpenedAt ?? 1_700_000_000_000,
  ...fields,
});

const CASES: Case[] = [
  {
    name: "rail order keeps pinned first and missing indexes last",
    operation: "sortProjectsForRail",
    input: {
      projects: [
        P({ id: "a", orderIndex: 2 }),
        P({ id: "b" }),
        P({ id: "c", pinned: true, orderIndex: 9 }),
        P({ id: "d", orderIndex: 0 }),
      ],
    },
  },
  {
    name: "equal order indexes keep their original order",
    operation: "sortProjectsForRail",
    input: {
      projects: [P({ id: "x", orderIndex: 1 }), P({ id: "y", orderIndex: 1 }), P({ id: "z", orderIndex: 1 })],
    },
  },
  {
    name: "a non-numeric order index is treated as missing",
    operation: "normalizeProjectOrder",
    input: { projects: [P({ id: "s", orderIndex: "3" as unknown as number }), P({ id: "t", orderIndex: 0 })] },
  },
  {
    name: "a fractional order index survives untouched",
    operation: "normalizeProjectOrder",
    input: { projects: [P({ id: "half", orderIndex: 1.5 as number })] },
  },
  {
    name: "reordering by ids drops unknown ids and re-indexes the rest",
    operation: "applyProjectOrder",
    input: {
      projects: [P({ id: "a", orderIndex: 0 }), P({ id: "b", orderIndex: 1 }), P({ id: "c", orderIndex: 2 })],
      orderedProjectIds: ["c", "ghost", "a"],
    },
  },
  {
    name: "a duplicated id in the order list means one record, after the ADR-013 fix",
    operation: "applyProjectOrder",
    input: {
      projects: [P({ id: "a", orderIndex: 0 }), P({ id: "b", orderIndex: 1 })],
      orderedProjectIds: ["a", "a", "b"],
    },
  },
  {
    name: "an empty id list leaves rail order",
    operation: "applyProjectOrder",
    input: {
      projects: [P({ id: "a", orderIndex: 5 }), P({ id: "b", pinned: true, orderIndex: 7 })],
      orderedProjectIds: [],
    },
  },
  {
    name: "groups are configured order first, then discovered, ungrouped last",
    operation: "groupProjectsForRail",
    input: {
      projects: [
        P({ id: "a", group: "Work" }),
        P({ id: "b" }),
        P({ id: "c", group: "  Padded  " }),
        P({ id: "d", group: "Work" }),
        P({ id: "e", group: "   " }),
      ],
      configuredNames: ["Later", "Work", "Later", "  "],
    },
  },
  {
    name: "no ungrouped section is emitted when every project has a group",
    operation: "groupProjectsForRail",
    input: { projects: [P({ id: "a", group: "Work" })], configuredNames: [] },
  },
  {
    name: "merge reports each group once",
    operation: "mergeProjectGroupNames",
    input: {
      projects: [P({ id: "a", group: "Work" }), P({ id: "b", group: "Play" }), P({ id: "c", group: "Work" })],
      configuredNames: ["Play"],
    },
  },
  {
    name: "a zero-width-no-break name normalises away",
    operation: "normalizeProjectGroupNames",
    input: { value: ["\u{FEFF}Bom", "", "  ", 7, "Bom", "\u{FEFF}"] },
  },
  {
    name: "projectGroupForProject falls back to the sentinel",
    operation: "projectGroupForProject",
    input: { project: P({ id: "u", group: null as unknown as string | undefined }) },
  },
  {
    name: "opening a new local project prepends and re-indexes",
    operation: "upsertLocalProject",
    input: {
      projects: [P({ id: "1", path: "/repo/one", orderIndex: 0 }), P({ id: "2", path: "/repo/two", orderIndex: 1 })],
      path: "/repo/three",
      now: 1_700_000_009_999,
    },
  },
  {
    name: "reopening the same path touches it and keeps its place",
    operation: "upsertLocalProject",
    input: {
      projects: [P({ id: "1", path: "/repo/one", orderIndex: 0 }), P({ id: "2", path: "/repo/two", orderIndex: 1 })],
      path: "/repo/two",
      now: 1_700_000_010_000,
    },
  },
  {
    name: "a differently spelled same path is a second project, as today",
    operation: "upsertLocalProject",
    input: {
      projects: [P({ id: "1", path: "/repo/one/", orderIndex: 0 })],
      path: "/repo/one",
      now: 1_700_000_011_000,
    },
  },
  {
    name: "a chinese and spaced path keeps its name and byte order",
    operation: "upsertLocalProject",
    input: { projects: [], path: "/home/dev/同步 空间/项目 一", now: 1_700_000_012_000 },
  },
  {
    name: "case-differing paths stay distinct",
    operation: "upsertLocalProject",
    input: { projects: [P({ id: "1", path: "/Repo/One", orderIndex: 0 })], path: "/repo/one", now: 1_700_000_013_000 },
  },
  {
    name: "ssh upsert matches on connection and path, not on the ssh:// string",
    operation: "upsertSshProject",
    input: {
      projects: [
        {
          ...P({ id: "s1", path: "ssh://conn-a/srv/app", orderIndex: 0 }),
          location: { kind: "ssh", connectionId: "conn-a", remotePath: "/srv/app" },
        },
      ],
      input: { name: "renamed?", connectionId: "conn-a", remotePath: "srv/app" },
      now: 1_700_000_014_000,
    },
  },
  {
    name: "the same remote path under another connection is a new project",
    operation: "upsertSshProject",
    input: {
      projects: [
        {
          ...P({ id: "s1", path: "ssh://conn-a/srv/app", orderIndex: 0 }),
          location: { kind: "ssh", connectionId: "conn-a", remotePath: "/srv/app" },
        },
      ],
      input: { name: "other host", connectionId: "conn-b", remotePath: "/srv/app" },
      now: 1_700_000_015_000,
    },
  },
  {
    name: "wsl normalises the root path and encodes the distribution",
    operation: "upsertWslProject",
    input: {
      projects: [],
      input: { name: "wsl", distribution: "Ubuntu-24.04 LTS", linuxPath: "/home/dev/app/" },
      now: 1_700_000_016_000,
    },
  },
  {
    name: "a wsl root path stays a root",
    operation: "normalizeWslProjectPath",
    input: { linuxPath: " / " },
  },
  {
    name: "touch leaves the order alone",
    operation: "touchProjectInList",
    input: {
      projects: [P({ id: "a", orderIndex: 0, lastOpenedAt: 1 }), P({ id: "b", orderIndex: 1, lastOpenedAt: 2 })],
      projectId: "b",
      now: 1_700_000_017_000,
    },
  },
  {
    name: "assigning no group removes the key",
    operation: "assignProjectGroup",
    input: { projects: [P({ id: "a", group: "Work" })], projectId: "a", groupName: "   " },
  },
  {
    name: "renaming a group moves every member and reports the change",
    operation: "renameProjectGroupInList",
    input: {
      projects: [P({ id: "a", group: "Work" }), P({ id: "b", group: "Play" }), P({ id: "c", group: "Work" })],
      oldName: "Work",
      nextName: "  Work 2026  ",
    },
  },
  {
    name: "renaming to the same name is not a change",
    operation: "renameProjectGroupInList",
    input: { projects: [P({ id: "a", group: "Work" })], oldName: "Work", nextName: "Work" },
  },
  {
    name: "renaming to blank is not a change",
    operation: "renameProjectGroupInList",
    input: { projects: [P({ id: "a", group: "Work" })], oldName: "Work", nextName: " " },
  },
  {
    name: "clearing a group empties its members",
    operation: "clearProjectGroupFromList",
    input: { projects: [P({ id: "a", group: "Work" }), P({ id: "b", group: "Play" })], groupName: "Work" },
  },
  {
    name: "renaming a project keeps every other field",
    operation: "renameProjectInList",
    input: {
      projects: [P({ id: "a", name: "old", branch: "main", group: "Work", orderIndex: 3 })],
      projectId: "a",
      name: "新名字",
    },
  },
  {
    name: "hiding toggles the flag only",
    operation: "toggleProjectHiddenInList",
    input: { projects: [P({ id: "a", hiddenFromRail: true })], projectId: "a" },
  },
  {
    name: "pinning toggles without touching orderIndex",
    operation: "toggleProjectPinnedInList",
    input: { projects: [P({ id: "a", orderIndex: 4 })], projectId: "a" },
  },
  {
    name: "pinning an unknown id changes nothing",
    operation: "toggleProjectPinnedInList",
    input: { projects: [P({ id: "a" })], projectId: "ghost" },
  },
  {
    name: "removing an avatar clears the key",
    operation: "setProjectAvatarInList",
    input: { projects: [P({ id: "a", avatar: { emoji: "x" } })], projectId: "a", avatar: undefined },
  },
  { name: "deriveProjectName takes the last segment", operation: "deriveProjectName", input: { path: "/repo/app/" } },
  { name: "deriveProjectName handles windows separators", operation: "deriveProjectName", input: { path: "C:\\repo\\app\\" } },
  { name: "deriveProjectName keeps a bare separator as-is", operation: "deriveProjectName", input: { path: "/" } },
  { name: "deriveProjectName keeps an empty path", operation: "deriveProjectName", input: { path: "" } },
  { name: "normalizeRemotePath makes a path absolute", operation: "normalizeRemotePath", input: { path: " srv/app " } },
  { name: "sshProjectPath prefixes the connection id", operation: "sshProjectPath", input: { connectionId: "c-1", remotePath: "/a/b" } },
  { name: "wslProjectPath encodes the distribution", operation: "wslProjectPath", input: { distribution: "a b", linuxPath: "/x" } },
  {
    name: "resolveProjectLocation defaults to local",
    operation: "resolveProjectLocation",
    input: { project: P({ id: "a", path: "/repo/a" }) },
  },
  {
    name: "resolveProjectLocation passes an unknown kind through",
    operation: "resolveProjectLocation",
    input: {
      project: {
        ...P({ id: "o", path: "/repo/o" }),
        location: { kind: "carrier-pigeon", remotePath: "/srv/app" },
      },
    },
  },
  {
    name: "resolveProjectLocation reads an ssh location",
    operation: "resolveProjectLocation",
    input: { project: { ...P({ id: "s" }), location: { kind: "ssh", connectionId: "c", remotePath: "/r" } } },
  },
];

/**
 * Where the new core deliberately does something different from the shipping client
 * (ADR-013). `expected` always stays what the TypeScript produces — that is the point of
 * deriving the table from the client — and `coreExpected` states the intended new result
 * together with the reason. The Rust suite compares against `coreExpected` when present and
 * fails if a divergence is declared without one, so "we changed it" can never be a silent
 * edit of an expectation.
 */
const CORE_DIVERGENCE: Record<string, { coreExpected: unknown; reason: string }> = {
  "a differently spelled same path is a second project, as today": {
    // The existing record wins and keeps every field it had — including `name`, which the
    // `P()` helper defaulted to the id — and the result is the client's own
    // `{ project, projects }` envelope. Writing this by hand once already produced a wrong
    // expectation (name "one"), which is why the Rust side refuses a coreExpected that does
    // not differ from the client result *and* pins the exact value.
    coreExpected: {
      project: {
        id: "1",
        name: "1",
        path: "/repo/one/",
        lastOpenedAt: 1700000011000,
        orderIndex: 0,
      },
      projects: [
        {
          id: "1",
          name: "1",
          path: "/repo/one/",
          lastOpenedAt: 1700000011000,
          orderIndex: 0,
        },
      ],
    },
    reason:
      "ADR-013: local identity is compared after lossless normalisation (trailing separators), so this touches the existing record instead of adding a second one.",
  },
  "resolveProjectLocation passes an unknown kind through": {
    coreExpected: { kind: "local", path: "/repo/o" },
    reason:
      "ADR-013 Q32: the core cannot name this kind, so it reads the record as listed-but-unaddressable (identity falls back to the legacy path field) while `is_addressable` refuses any operation that needs the location.",
  },
};

function run(theCase: Case): unknown {
  const i = theCase.input as Record<string, never>;
  switch (theCase.operation) {
    case "sortProjectsForRail":
      return sortProjectsForRail(i.projects as Project[]);
    case "normalizeProjectOrder":
      return normalizeProjectOrder(i.projects as Project[]);
    case "applyProjectOrder":
      return applyProjectOrder(i.projects as Project[], i.orderedProjectIds as string[]);
    case "groupProjectsForRail":
      return groupProjectsForRail(i.projects as Project[], i.configuredNames as string[]);
    case "mergeProjectGroupNames":
      return mergeProjectGroupNames(i.projects as Project[], i.configuredNames as string[]);
    case "normalizeProjectGroupNames":
      return normalizeProjectGroupNames(i.value);
    case "projectGroupForProject":
      return projectGroupForProject(i.project as Project);
    case "upsertLocalProject": {
      const r = upsertLocalProject(i.projects as Project[], i.path as string, i.now as number);
      return { project: r.project, projects: r.projects };
    }
    case "upsertSshProject": {
      const r = upsertSshProject(i.projects as Project[], i.input as never, i.now as number);
      return { project: r.project, projects: r.projects };
    }
    case "upsertWslProject": {
      const r = upsertWslProject(i.projects as Project[], i.input as never, i.now as number);
      return { project: r.project, projects: r.projects, reused: r.reused };
    }
    case "touchProjectInList": {
      const r = touchProjectInList(i.projects as Project[], i.projectId as string, i.now as number);
      return { project: r.project, projects: r.projects };
    }
    case "assignProjectGroup":
      return assignProjectGroup(i.projects as Project[], i.projectId as string, i.groupName as string | null);
    case "renameProjectGroupInList":
      return renameProjectGroupInList(i.projects as Project[], i.oldName as string, i.nextName as string);
    case "clearProjectGroupFromList":
      return clearProjectGroupFromList(i.projects as Project[], i.groupName as string);
    case "renameProjectInList":
      return renameProjectInList(i.projects as Project[], i.projectId as string, i.name as string);
    case "toggleProjectHiddenInList":
      return toggleProjectHiddenInList(i.projects as Project[], i.projectId as string);
    case "toggleProjectPinnedInList":
      return toggleProjectPinnedInList(i.projects as Project[], i.projectId as string);
    case "setProjectAvatarInList":
      return setProjectAvatarInList(i.projects as Project[], i.projectId as string, i.avatar as never);
    case "deriveProjectName":
      return deriveProjectName(i.path as string);
    case "normalizeRemotePath":
      return normalizeRemotePath(i.path as string);
    case "normalizeWslProjectPath":
      return normalizeWslProjectPath(i.linuxPath as string);
    case "sshProjectPath":
      return sshProjectPath(i.connectionId as string, i.remotePath as string);
    case "wslProjectPath":
      return wslProjectPath(i.distribution as string, i.linuxPath as string);
    case "resolveProjectLocation":
      return resolveProjectLocation(i.project as Project);
    default:
      throw new Error(`unhandled operation ${theCase.operation}`);
  }
}

const knowledgePath = resolve(
  process.cwd(),
  "core/crates/aeroric-protocol/knowledge/project-parity-cases.json",
);

type Stored = {
  title: string;
  caseCount: number;
  cases: {
    name: string;
    operation: string;
    input: Record<string, unknown>;
    expected: unknown;
    coreExpected?: unknown;
    divergenceReason?: string;
  }[];
};

const stored = JSON.parse(readFileSync(knowledgePath, "utf8")) as Stored;

describe("project behaviour is derived from the shipping client", () => {
  it("reproduces every stored expectation with the real functions", () => {
    expect(stored.cases.length).toBe(stored.caseCount);
    // A dropped case is a lost guarantee, so the stored list must cover this file's list
    // and nothing here may shrink it silently.
    const storedNames = new Set(stored.cases.map((row) => row.name));
    for (const theCase of CASES) {
      expect(storedNames.has(theCase.name), `${theCase.name} is not in the table`).toBe(true);
    }
    expect(CASES.length).toBe(stored.cases.length);
    const declared = new Set(stored.cases.filter((row) => row.coreExpected !== undefined).map((row) => row.name));
    for (const name of Object.keys(CORE_DIVERGENCE)) {
      expect(declared.has(name), `${name} is declared as a divergence but not stored`).toBe(true);
    }
    expect(declared.size).toBe(Object.keys(CORE_DIVERGENCE).length);
    for (const row of stored.cases) {
      const actual = run({ name: row.name, operation: row.operation, input: row.input });
      expect(
        actual,
        `${row.name}: the TypeScript function no longer produces the published expectation`,
      ).toEqual(row.expected);
    }
  });

  it("can re-derive the table on purpose", () => {
    if (process.env.AERORIC_WRITE_PROJECT_EXPECTATIONS !== "1") return;
    const rows = CASES.map((theCase) => {
      const stored = { ...theCase, expected: run(theCase) } as Record<string, unknown>;
      const divergence = CORE_DIVERGENCE[theCase.name];
      if (divergence) {
        stored.coreExpected = divergence.coreExpected;
        stored.divergenceReason = divergence.reason;
      }
      return stored;
    });
    mkdirSync(resolve(knowledgePath, ".."), { recursive: true });
    writeFileSync(
      knowledgePath,
      `${JSON.stringify({ ...stored, caseCount: rows.length, cases: rows }, null, 2)}\n`,
      "utf8",
    );
  });
});
