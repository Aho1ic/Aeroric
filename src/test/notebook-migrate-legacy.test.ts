import { describe, expect, it, vi } from "vitest";
import {
  LEGACY_STORAGE_KEY,
  MIGRATED_STORAGE_KEY,
  runLegacyMigration,
} from "../components/notebook/migrateLegacyNotes";
import type { MigrationReport } from "../components/notebook/notebookApi";

function fakeStorage(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    data,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
  };
}

function report(overrides: Partial<MigrationReport> = {}): MigrationReport {
  return {
    vault: "/home/u/.aeroric/notebook",
    backupPath: "/home/u/.aeroric/notebook/.notebook/legacy-backup-x.json",
    migrated: [],
    skipped: [],
    totalInput: 0,
    ...overrides,
  };
}

const ONE_NOTE = JSON.stringify([
  { id: "n1", title: "Deploy", body: "# hi", format: "markdown", updatedAt: 1 },
]);

describe("runLegacyMigration", () => {
  it("migrates legacy notes and renames the key instead of deleting it", async () => {
    const storage = fakeStorage({ [LEGACY_STORAGE_KEY]: ONE_NOTE });
    const migrate = vi.fn(async () => report({ totalInput: 1 }));

    const outcome = await runLegacyMigration({ storage, migrate });

    expect(outcome.status).toBe("migrated");
    expect(migrate).toHaveBeenCalledWith(ONE_NOTE);
    // 旧键让位,但数据必须还在 —— 留一个版本周期的回退余地。
    expect(storage.getItem(LEGACY_STORAGE_KEY)).toBeNull();
    expect(storage.getItem(MIGRATED_STORAGE_KEY)).toBe(ONE_NOTE);
  });

  it("passes the raw string through untouched so the backup keeps the original", async () => {
    // 原文里有后端不认识的字段,备份必须原样保留它们。
    const raw = '[{"id":"n1","title":"T","body":"b","format":"markdown","futureField":42}]';
    const storage = fakeStorage({ [LEGACY_STORAGE_KEY]: raw });
    const migrate = vi.fn(async () => report({ totalInput: 1 }));

    await runLegacyMigration({ storage, migrate });

    expect(migrate).toHaveBeenCalledWith(raw);
  });

  it("leaves localStorage untouched when the backend fails", async () => {
    const storage = fakeStorage({ [LEGACY_STORAGE_KEY]: ONE_NOTE });
    const migrate = vi.fn(async () => {
      throw new Error("backup failed");
    });

    const outcome = await runLegacyMigration({ storage, migrate });

    expect(outcome).toEqual({ status: "failed", message: "backup failed" });
    // 失败后数据必须原封不动 —— 下次启动还要靠它重试。
    expect(storage.getItem(LEGACY_STORAGE_KEY)).toBe(ONE_NOTE);
    expect(storage.getItem(MIGRATED_STORAGE_KEY)).toBeNull();
  });

  it("does not run twice once the migrated marker exists", async () => {
    const storage = fakeStorage({ [MIGRATED_STORAGE_KEY]: ONE_NOTE });
    const migrate = vi.fn(async () => report());

    const outcome = await runLegacyMigration({ storage, migrate });

    expect(outcome).toEqual({ status: "skipped", reason: "already-migrated" });
    expect(migrate).not.toHaveBeenCalled();
  });

  it("skips when there is no legacy data at all", async () => {
    const storage = fakeStorage();
    const migrate = vi.fn(async () => report());

    const outcome = await runLegacyMigration({ storage, migrate });

    expect(outcome).toEqual({ status: "skipped", reason: "no-legacy-data" });
    expect(migrate).not.toHaveBeenCalled();
  });

  it("treats an empty array as nothing to migrate", async () => {
    const storage = fakeStorage({ [LEGACY_STORAGE_KEY]: "[]" });
    const migrate = vi.fn(async () => report());

    const outcome = await runLegacyMigration({ storage, migrate });

    expect(outcome).toEqual({ status: "skipped", reason: "no-legacy-data" });
    expect(migrate).not.toHaveBeenCalled();
  });

  it("reports success even if renaming the key fails, because the files are on disk", async () => {
    const storage = fakeStorage({ [LEGACY_STORAGE_KEY]: ONE_NOTE });
    // 配额耗尽:磁盘上的迁移已经成功,只是标记写不上。
    storage.setItem = () => {
      throw new Error("quota exceeded");
    };
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const migrate = vi.fn(async () => report({ totalInput: 1 }));

    const outcome = await runLegacyMigration({ storage, migrate });

    // 数据在磁盘上了,不能报失败 —— 报失败会让调用方以为笔记没迁过去。
    expect(outcome.status).toBe("migrated");
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });

  it("retries on the next launch after a failure, relying on backend idempotency", async () => {
    const storage = fakeStorage({ [LEGACY_STORAGE_KEY]: ONE_NOTE });
    const migrate = vi
      .fn<(raw: string) => Promise<MigrationReport>>()
      .mockRejectedValueOnce(new Error("transient"))
      .mockResolvedValueOnce(report({ totalInput: 1 }));

    const first = await runLegacyMigration({ storage, migrate });
    expect(first.status).toBe("failed");

    const second = await runLegacyMigration({ storage, migrate });
    expect(second.status).toBe("migrated");
    expect(migrate).toHaveBeenCalledTimes(2);
    expect(storage.getItem(MIGRATED_STORAGE_KEY)).toBe(ONE_NOTE);
  });

  it("surfaces non-Error rejections as readable messages", async () => {
    const storage = fakeStorage({ [LEGACY_STORAGE_KEY]: ONE_NOTE });
    // Tauri 的 invoke 拒绝时给的是字符串,不是 Error。
    const migrate = vi.fn(async () => {
      throw "ALREADY_EXISTS:/tmp/x.md";
    });

    const outcome = await runLegacyMigration({ storage, migrate });

    expect(outcome).toEqual({
      status: "failed",
      message: "ALREADY_EXISTS:/tmp/x.md",
    });
  });
});
