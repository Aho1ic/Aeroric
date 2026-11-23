import { test } from "node:test";
import assert from "node:assert/strict";
import { diffSnapshots, fields, loadSnapshot, methodTable, snapshotNames } from "./codex-contract-diff.mjs";

const request = (method, params) => ({
  properties: { method: { enum: [method] }, ...(params ? { params } : {}) },
});
const file = (entries, definitions = {}) => ({ oneOf: entries, definitions });
const snapshot = (client, server = file([]), notifications = file([])) => ({
  "ClientRequest.json": client,
  "ServerRequest.json": server,
  "ServerNotification.json": notifications,
});

test("the committed snapshots are listed oldest first", () => {
  const names = snapshotNames();
  assert.ok(names.length >= 2, names.join(","));
  assert.equal(names[0], "app-server-0.162.1");
  const newest = loadSnapshot(names.at(-1));
  assert.ok(methodTable(newest["ClientRequest.json"]).has("turn/start"));
});

test("fields follow $ref, mark required and keep enums", () => {
  const root = {
    definitions: {
      Params: { type: "object", required: ["threadId"], properties: { threadId: { type: "string" }, mode: { $ref: "#/definitions/Mode" } } },
      Mode: { type: "string", enum: ["a", "b"] },
    },
  };
  const out = fields(root, { $ref: "#/definitions/Params" });
  assert.equal(out.get("params.threadId"), "string");
  assert.equal(out.get("params.threadId (required)"), "required");
  assert.equal(out.get("params.mode"), 'string ["a","b"]');
});

test("tagged branches get their own paths; untagged ones merge", () => {
  const tagged = fields({}, {
    oneOf: [
      { type: "object", properties: { type: { enum: ["text"] }, text: { type: "string" } } },
      { type: "object", properties: { type: { enum: ["image"] }, url: { type: "string" } } },
    ],
  });
  assert.equal(tagged.get("params<text>.text"), "string");
  assert.equal(tagged.get("params<image>.url"), "string");

  const merged = fields({}, {
    anyOf: [
      { type: "object", required: ["url"], properties: { url: { type: "string" } } },
      { type: "object", required: ["fileId"], properties: { fileId: { type: "string" } } },
      { type: "null" },
    ],
  });
  assert.equal(merged.get("params.url"), "string");
  assert.equal(merged.get("params.fileId"), "string");
  assert.equal(merged.has("params.url (required)"), false, "required by only one branch");

  const beside = fields({}, {
    type: "object",
    required: ["type"],
    properties: { type: { type: "string", enum: ["audio"] } },
    anyOf: [
      { type: "object", required: ["url"], properties: { url: { type: "string" } } },
      { type: "object", required: ["fileId"], properties: { fileId: { type: "string" } } },
    ],
  });
  assert.equal(beside.get("params.type"), 'string ["audio"]', "a field beside the branches is kept");
  assert.equal(beside.get("params.type (required)"), "required");
  assert.equal(beside.get("params.fileId"), "string");
});

test("a used method that disappears breaks the diff; an unused one does not", () => {
  const older = snapshot(file([request("turn/start"), request("old/unused"), request("thread/gone")]));
  const newer = snapshot(file([request("turn/start"), request("feedback/save")]));
  const used = new Set(["turn/start", "thread/gone"]);
  const { lines, broken } = diffSnapshots(older, newer, used);
  assert.equal(broken, true);
  assert.ok(lines.includes("  + feedback/save"));
  assert.ok(lines.includes("  - thread/gone   ← 驱动在用"));
  assert.ok(lines.includes("  - old/unused"));
  assert.equal(diffSnapshots(older, newer, new Set(["turn/start"])).broken, false);
});

test("a used method's new required field and narrowed enum are reported", () => {
  const params = (extra) => ({ type: "object", ...extra });
  const older = snapshot(file([request("review/start", params({ properties: { delivery: { type: "string", enum: ["inline", "detached"] } } }))]));
  const newer = snapshot(file([request("review/start", params({
    required: ["target"],
    properties: { delivery: { type: "string", enum: ["inline"] }, target: { type: "object" } },
  }))]));
  const { lines, broken } = diffSnapshots(older, newer, new Set(["review/start"]));
  assert.equal(broken, false);
  assert.ok(lines.includes("  review/start（驱动在用）"), lines.join("\n"));
  assert.ok(lines.includes('    ~ params.delivery: string ["inline","detached"] → string ["inline"]'));
  assert.ok(lines.includes("    + params.target: object"));
  assert.ok(lines.includes("    + params.target (required)"));
});
