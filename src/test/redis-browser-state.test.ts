import { describe, expect, it } from "vitest";
import type { RedisValue } from "../types";
import {
  clampRedisHashFieldWidth,
  clampRedisZsetScoreWidth,
  escapeRedisArg,
  redisInsertStatement,
  redisJsonChildNodes,
  redisJsonText,
  redisJsonValue,
  redisKeySizeLabel,
  redisMemberRows,
  redisStreamMemberGroups,
  redisValueMemberKind,
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
});
