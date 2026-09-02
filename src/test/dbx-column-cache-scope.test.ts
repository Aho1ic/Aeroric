import { describe, expect, it } from "vitest";
import {
  dbxObjectKey,
  omitDbxCacheEntriesForSchema,
} from "../components/database/databaseViewModel";
import type { DbxColumnInfo } from "../types";

function columns(name: string): DbxColumnInfo[] {
  return [{ name, data_type: "text", is_nullable: true, is_primary_key: false }];
}

describe("omitDbxCacheEntriesForSchema", () => {
  it("drops only the named schema's entries", () => {
    const cache = {
      "public.users": columns("public_col"),
      "public.teams": columns("public_col"),
      "audit.logs": columns("audit_col"),
    };

    expect(omitDbxCacheEntriesForSchema(cache, "public")).toEqual({
      "audit.logs": cache["audit.logs"],
    });
  });

  it("keeps a schema whose name merely prefixes the refreshed one", () => {
    // 少了那个点就会把 `public.*` 一起误伤 —— 这是这支函数唯一容易写错的地方。
    const cache = {
      "pub.events": columns("pub_col"),
      "public.users": columns("public_col"),
    };

    expect(Object.keys(omitDbxCacheEntriesForSchema(cache, "pub"))).toEqual(["public.users"]);
    expect(Object.keys(omitDbxCacheEntriesForSchema(cache, "public"))).toEqual(["pub.events"]);
  });

  it("keeps schemaless keys, which is what dbxObjectKey produces without a schema", () => {
    // MySQL 一类没有模式,`dbxObjectKey` 只返回表名,键里不含点。
    const key = dbxObjectKey({ name: "users", object_type: "table" });
    expect(key).toBe("users");
    const cache = { [key]: columns("mysql_col"), "public.users": columns("pg_col") };

    expect(Object.keys(omitDbxCacheEntriesForSchema(cache, "public"))).toEqual(["users"]);
  });

  it("returns an equal-but-new object when nothing matches", () => {
    const cache = { "audit.logs": columns("audit_col") };
    const next = omitDbxCacheEntriesForSchema(cache, "public");

    expect(next).toEqual(cache);
    expect(next).not.toBe(cache);
  });

  it("handles an empty cache", () => {
    expect(omitDbxCacheEntriesForSchema({}, "public")).toEqual({});
  });
});
