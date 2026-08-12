import { describe, expect, it } from "vitest";
import { REPEAT_DELAY_MS, REPEAT_INTERVAL_MS, TERMINAL_ACCESSORY_KEYS } from "./accessory-keys";

const byId = new Map(TERMINAL_ACCESSORY_KEYS.map((k) => [k.id, k]));

describe("TERMINAL_ACCESSORY_KEYS", () => {
  it("id 唯一且 label/bytes 非空", () => {
    expect(byId.size).toBe(TERMINAL_ACCESSORY_KEYS.length);
    for (const key of TERMINAL_ACCESSORY_KEYS) {
      expect(key.label.length).toBeGreaterThan(0);
      expect(key.bytes.length).toBeGreaterThan(0);
    }
  });

  it("控制字符字节正确", () => {
    expect(byId.get("escape")?.bytes).toBe("\x1b");
    expect(byId.get("tab")?.bytes).toBe("\t");
    expect(byId.get("enter")?.bytes).toBe("\r");
    expect(byId.get("space")?.bytes).toBe(" ");
    expect(byId.get("backspace")?.bytes).toBe("\x7f");
  });

  it("转义序列字节正确", () => {
    expect(byId.get("shiftTab")?.bytes).toBe("\x1b[Z");
    expect(byId.get("delete")?.bytes).toBe("\x1b[3~");
    expect(byId.get("arrowUp")?.bytes).toBe("\x1b[A");
    expect(byId.get("arrowDown")?.bytes).toBe("\x1b[B");
    expect(byId.get("arrowLeft")?.bytes).toBe("\x1b[D");
    expect(byId.get("arrowRight")?.bytes).toBe("\x1b[C");
  });

  it("Ctrl 组合键落在 0x01-0x1f 且等于字母序号", () => {
    const ctrl: Array<[string, string]> = [
      ["ctrlA", "a"],
      ["ctrlC", "c"],
      ["ctrlD", "d"],
      ["ctrlE", "e"],
      ["ctrlL", "l"],
      ["ctrlR", "r"],
      ["ctrlU", "u"],
      ["ctrlW", "w"],
      ["ctrlZ", "z"],
    ];
    for (const [id, letter] of ctrl) {
      const bytes = byId.get(id)?.bytes;
      expect(bytes, id).toHaveLength(1);
      const code = bytes!.charCodeAt(0);
      expect(code, id).toBe(letter.charCodeAt(0) & 0x1f);
      expect(code).toBeGreaterThanOrEqual(0x01);
      expect(code).toBeLessThanOrEqual(0x1f);
    }
  });

  it("只有退格/删除/方向键可长按连发", () => {
    const repeatable = TERMINAL_ACCESSORY_KEYS.filter((k) => k.repeatable).map((k) => k.id);
    expect(repeatable.sort()).toEqual(
      ["arrowDown", "arrowLeft", "arrowRight", "arrowUp", "backspace", "delete"].sort(),
    );
  });
});

describe("连发节奏", () => {
  it("首次延迟长于连发间隔", () => {
    expect(REPEAT_DELAY_MS).toBeGreaterThan(REPEAT_INTERVAL_MS);
    expect(REPEAT_INTERVAL_MS).toBeGreaterThan(0);
  });
});
