import { beforeEach, describe, expect, it } from "vitest";
import type { RedisValue } from "../types";
import { mergeRedisValuePage } from "../hooks/useRedisBrowser";
import {
  clampRedisHashFieldWidth,
  clampRedisZsetScoreWidth,
  escapeRedisArg,
  loadRedisJsonWordWrap,
  redisInsertStatement,
  redisJsonChildNodes,
  redisJsonText,
  redisJsonValue,
  redisKeySizeLabel,
  redisMemberRows,
  redisStreamMemberGroups,
  redisValueMemberKind,
  saveRedisJsonWordWrap,
} from "../components/database/redisBrowserState";

function value(keyType: string, data: unknown, overrides: Partial<RedisValue> = {}): RedisValue {
  return {
    key_display: "sample",
    key_raw: "sample",
    key_type: keyType,
    ttl: -1,
    value_is_binary: false,
    value: data,
    ...overrides,
  };
}

describe("redisBrowserState", () => {
  it("merges typed collection pages without replacing first-page metadata", () => {
    const current = value("hash", [{ field: "name", value: "Ada" }], {
      ttl: 60,
      total: 2,
      scan_cursor: 12,
    });

    expect(
      mergeRedisValuePage(current, {
        kind: "hash",
        items: [{ field: "role", value: "admin" }],
        scan_cursor: null,
      }),
    ).toEqual({
      ...current,
      value: [
        { field: "name", value: "Ada" },
        { field: "role", value: "admin" },
      ],
      scan_cursor: null,
    });
  });

  it("formats, parses, and expands JSON values", () => {
    expect(redisJsonText('{"enabled":true}', true)).toBe('{\n  "enabled": true\n}');
    expect(redisJsonText("{bad", true)).toBeNull();
    expect(redisJsonValue("[1,2]")).toEqual({ value: [1, 2] });
    expect(redisJsonValue("nope")).toBeNull();

    expect(
      redisJsonChildNodes({
        key: "$",
        label: "$",
        value: { items: [1] },
        path: "$",
        depth: 0,
        parentKind: "root",
      }),
    ).toEqual([
      {
        key: "items",
        label: "items",
        value: [1],
        path: "$.items",
        depth: 1,
        parentKind: "object",
      },
    ]);
  });

  it("derives editable member rows for collection values", () => {
    expect(redisMemberRows(value("list", ["a"]))[0]).toMatchObject({
      id: "list:0",
      cells: ["#0", "a"],
      deleteAction: { kind: "list", index: 0 },
    });
    expect(redisMemberRows(value("hash", { name: '{"first":"Ada"}' }))[0]).toMatchObject({
      kind: "hash",
      title: "name",
      format: "json",
      editAction: { kind: "hash", field: "name" },
    });
    expect(redisMemberRows(value("zset", [{ member: "one", score: 2 }]))[0]).toMatchObject({
      cells: ["2", "one"],
      editAction: { kind: "zset", member: "one", score: 2 },
    });
    expect(redisValueMemberKind(value("stream", []))).toBe("stream");
    expect(redisMemberRows(value("list", [], { value_is_binary: true }))).toEqual([]);
  });

  it("groups stream fields by entry id", () => {
    const rows = redisMemberRows(
      value("stream", [
        { id: "1-0", fields: { event: "created", user: "ada" } },
        { id: "2-0", fields: { event: "updated" } },
      ]),
    );
    const groups = redisStreamMemberGroups(rows);
    expect(groups.map((group) => [group.entryId, group.rows.length])).toEqual([
      ["1-0", 2],
      ["2-0", 1],
    ]);
  });

  it("builds reproducible Redis insert statements and display metadata", () => {
    expect(escapeRedisArg('a"b\\c')).toBe('"a\\"b\\\\c"');
    expect(redisInsertStatement(value("string", "hello", { ttl: 60 }))).toBe(
      'SET "sample" "hello"\nEXPIRE "sample" 60',
    );
    expect(redisInsertStatement(value("hash", { name: "Ada" }))).toBe('HSET "sample" "name" "Ada"');
    expect(redisInsertStatement(value("json", { enabled: true }))).toBe(
      'JSON.SET "sample" $ "{\\"enabled\\":true}"',
    );
    expect(redisKeySizeLabel("string", 2048)).toBe("2.0 KB");
    expect(redisKeySizeLabel("hash", 3)).toBe("3");
    expect(clampRedisHashFieldWidth(10)).toBe(120);
    expect(clampRedisZsetScoreWidth(999)).toBe(260);
  });

  /* 自动换行偏好的键名从 `dbx-redis-json-word-wrap` 改成
     `aeroric:database:redis-json-word-wrap`(localStorage 一律带 `aeroric:` 前缀,否则清理
     本应用存储时会被漏掉)。直接换字符串会让老用户关掉的换行又变回开着。 */
  describe("loadRedisJsonWordWrap 的键名迁移", () => {
    beforeEach(() => {
      window.localStorage.clear();
    });

    it("旧键里的 false 仍然生效,并被搬到新键、删掉旧键", () => {
      window.localStorage.setItem("dbx-redis-json-word-wrap", "false");

      expect(loadRedisJsonWordWrap()).toBe(false);
      // 只回退读不写回的话,下一次「保存」只写新键,旧键成了永远读不到的僵尸值。
      expect(window.localStorage.getItem("aeroric:database:redis-json-word-wrap")).toBe("false");
      expect(window.localStorage.getItem("dbx-redis-json-word-wrap")).toBeNull();
    });

    it("新键有值时不看旧键", () => {
      // 迁移只在新键缺失时发生;否则用户改过的新值会被一个陈旧的旧键顶掉。
      window.localStorage.setItem("aeroric:database:redis-json-word-wrap", "true");
      window.localStorage.setItem("dbx-redis-json-word-wrap", "false");

      expect(loadRedisJsonWordWrap()).toBe(true);
      expect(window.localStorage.getItem("dbx-redis-json-word-wrap")).toBe("false");
    });

    it("两个键都没有时默认开启", () => {
      expect(loadRedisJsonWordWrap()).toBe(true);
    });

    it("保存只写新键", () => {
      saveRedisJsonWordWrap(false);

      expect(window.localStorage.getItem("aeroric:database:redis-json-word-wrap")).toBe("false");
      expect(window.localStorage.getItem("dbx-redis-json-word-wrap")).toBeNull();
    });
  });
});
