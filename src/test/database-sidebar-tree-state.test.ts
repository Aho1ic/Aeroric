import { describe, expect, it } from "vitest";
import type { AeroricDbConnectionConfig, DbxObjectInfo } from "../types";
import {
  connectionBadgeColor,
  connectionBadgeText,
  dbxChildObjectBelongsToTable,
  dbxObjectNodeKey,
  dbxTableChildObjectType,
  getDefaultDatabase,
  groupDbxObjects,
  matchesSearch,
  mongoDocumentPreview,
  orderPinnedFirst,
  sortDbxConnections,
} from "../components/database/databaseSidebarTreeState";

function connection(
  id: string,
  name: string,
  overrides: Partial<AeroricDbConnectionConfig> = {},
): AeroricDbConnectionConfig {
  return {
    id,
    name,
    dbType: "postgres",
    readOnly: false,
    createdAt: 1,
    lastOpenedAt: 1,
    ...overrides,
  };
}

describe("databaseSidebarTreeState", () => {
  it("builds compact connection badges with deterministic or configured colors", () => {
    expect(connectionBadgeText("localhost")).toBe("IP");
    expect(connectionBadgeText("Production Database")).toBe("PD");
    expect(connectionBadgeText("主数据库")).toBe("主数");
    expect(connectionBadgeText("")).toBe("DB");

    const item = connection("one", "Production");
    expect(connectionBadgeColor(item)).toBe(connectionBadgeColor(item));
    expect(connectionBadgeColor(connection("two", "Custom", { dbx: { color: " #123456 " } }))).toBe(
      "#123456",
    );
  });

  it("groups, de-duplicates, and naturally sorts supported DBX objects", () => {
    const objects: DbxObjectInfo[] = [
      { name: "table10", object_type: "TABLE", schema: "public" },
      { name: "table2", object_type: "table", schema: "public" },
      { name: "table2", object_type: "TABLE", schema: "PUBLIC" },
      { name: "view1", object_type: "VIEW", schema: "public" },
      { name: "ignored", object_type: "TYPE", schema: "public" },
    ];

    const groups = groupDbxObjects(objects);
    expect(groups.map((group) => group.key)).toEqual(["tables", "views"]);
    expect(groups[0].objects.map((object) => object.name)).toEqual(["table2", "table10"]);
    expect(dbxObjectNodeKey(objects[0])).toBe("dbx-object:TABLE:public:table10");
  });

  it("classifies and associates table child objects", () => {
    const table: DbxObjectInfo = { name: "users", object_type: "TABLE", schema: "public" };
    const index: DbxObjectInfo = {
      name: "users_name_idx",
      object_type: "INDEX",
      parent_name: "USERS",
      parent_schema: "PUBLIC",
    };
    expect(dbxTableChildObjectType(index)).toBe("INDEX");
    expect(dbxChildObjectBelongsToTable(index, table)).toBe(true);
    expect(dbxChildObjectBelongsToTable({ ...index, parent_name: "accounts" }, table)).toBe(false);
  });

  it("derives document previews, defaults, search matching, and pinned ordering", () => {
    expect(mongoDocumentPreview({ _id: "a", name: "Ada", active: true }, 0)).toBe(
      "a name: Ada, active: true",
    );
    expect(getDefaultDatabase(connection("one", "One", { dbx: { database: " app " } }))).toBe(
      "app",
    );
    expect(matchesSearch("Production", "duct")).toBe(true);
    expect(orderPinnedFirst(["a", "b", "c"], (item) => item === "b")).toEqual(["b", "a", "c"]);

    const pinned = connection("pinned", "Zeta", { pinned: true });
    const regular = connection("regular", "Alpha");
    expect([regular, pinned].sort(sortDbxConnections)).toEqual([pinned, regular]);
  });
});
