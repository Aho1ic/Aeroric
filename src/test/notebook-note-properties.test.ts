import { describe, expect, it } from "vitest";
import {
  formatNoteSize,
  formatNoteTime,
  freshPropertiesState,
} from "../components/notebook/NotePropertiesSheet";

describe("formatNoteSize", () => {
  it("1KB 以下按字节报", () => {
    expect(formatNoteSize(0)).toBe("0 B");
    expect(formatNoteSize(6)).toBe("6 B");
    expect(formatNoteSize(1023)).toBe("1023 B");
  });

  it("1KB 起换 KB,保留一位", () => {
    expect(formatNoteSize(1024)).toBe("1.0 KB");
    expect(formatNoteSize(1536)).toBe("1.5 KB");
    // 1MB 的前一个字节还得留在 KB 档,不能提前跳。
    expect(formatNoteSize(1024 * 1024 - 1)).toBe("1024.0 KB");
  });

  it("1MB 起换 MB,保留两位", () => {
    expect(formatNoteSize(1024 * 1024)).toBe("1.00 MB");
    expect(formatNoteSize(1024 * 1024 * 3.5)).toBe("3.50 MB");
  });
});

describe("formatNoteTime", () => {
  it("补零到固定宽度", () => {
    // 本地时区构造,因为格式化用的是 getMonth/getHours 这一套本地取值。
    const ms = new Date(2026, 0, 5, 3, 7).getTime();
    expect(formatNoteTime(ms)).toBe("2026-01-05 03:07");
  });

  it("两位数的月日时分原样保留", () => {
    const ms = new Date(2026, 10, 23, 14, 38).getTime();
    expect(formatNoteTime(ms)).toBe("2026-11-23 14:38");
  });

  it("没有时间戳就没有时间", () => {
    // null 是「文件系统不记创建时间」,0 是后端取不到时的兜底值 —— 两者都不该
    // 显示成 1970-01-01。
    expect(formatNoteTime(null)).toBeNull();
    expect(formatNoteTime(0)).toBeNull();
  });

  it("坏时间戳报 null 而不是 Invalid Date", () => {
    expect(formatNoteTime(Number.NaN)).toBeNull();
    expect(formatNoteTime(8.64e15 + 1)).toBeNull();
  });
});

describe("freshPropertiesState", () => {
  it("开局是加载中,没有数据也没有错误", () => {
    // 初值给 loading:false 的话面板会先闪一帧「0 B」再跳成真实大小。
    expect(freshPropertiesState("/vault/a.md")).toEqual({
      noteId: "/vault/a.md",
      stat: null,
      loading: true,
      error: null,
    });
  });
});
