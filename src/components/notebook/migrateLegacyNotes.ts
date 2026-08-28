/* localStorage → 磁盘的一次性迁移(前端侧编排)。
 *
 * 后端负责「备份 → 转换 → 落盘 → 失败回滚」;这里负责决定「要不要迁」和
 * 「迁完之后怎么标记」。
 *
 * 顺序是刻意的:
 * 1. 只有 legacy 键存在且非空时才动手
 * 2. 把原始字符串整体交给后端(不在前端解析 —— 备份要存真正的原文)
 * 3. **成功之后**才把键改名为 `…:migrated`
 * 4. 改名而不是删除:留一个版本周期的回退余地。用户升级后发现问题,
 *    数据还在原地
 *
 * 失败时什么都不动:legacy 键保持原样,下次启动会再试一次。这是安全的,
 * 因为后端迁移是幂等的(靠 frontmatter 里的 legacyId 识别)。
 */

import { migrateLegacyNotes as invokeMigration, type MigrationReport } from "./notebookApi";

/** 随手记在 localStorage 里的原始键。与 NotebookPanel 的 STORAGE_KEY 一致。 */
export const LEGACY_STORAGE_KEY = "aeroric:notebook:v1";
/** 迁移完成后的标记键。保留数据,只改名。 */
export const MIGRATED_STORAGE_KEY = "aeroric:notebook:v1:migrated";

export type MigrationOutcome =
  | { status: "skipped"; reason: "no-legacy-data" | "already-migrated" }
  | { status: "migrated"; report: MigrationReport }
  | { status: "failed"; message: string };

type StorageLike = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function readLegacyRaw(storage: StorageLike): string | null {
  let raw: string | null;
  try {
    raw = storage.getItem(LEGACY_STORAGE_KEY);
  } catch {
    // 隐私模式下 localStorage 可能直接抛。没有数据可迁,等价于跳过。
    return null;
  }
  if (!raw) return null;
  const trimmed = raw.trim();
  // 空数组不值得迁,但也不算失败 —— 直接当作没有数据。
  if (!trimmed || trimmed === "[]") return null;
  return raw;
}

/**
 * 执行迁移。
 *
 * `migrate` 参数只为测试注入;正常调用走真实的 Tauri 命令。
 */
export async function runLegacyMigration(options?: {
  storage?: StorageLike;
  migrate?: (rawJson: string) => Promise<MigrationReport>;
}): Promise<MigrationOutcome> {
  const storage: StorageLike | null = options?.storage ?? safeLocalStorage();
  if (!storage) return { status: "skipped", reason: "no-legacy-data" };

  const migrate = options?.migrate ?? invokeMigration;

  // 已经迁过就不再动。注意:这里检查的是标记键,不是 legacy 键 —— 因为
  // legacy 键在成功后被改名,两者不会同时存在。
  try {
    if (storage.getItem(MIGRATED_STORAGE_KEY) !== null) {
      return { status: "skipped", reason: "already-migrated" };
    }
  } catch {
    // 读不了标记键就当没迁过。后端幂等,重复迁一次不会产生副本。
  }

  const raw = readLegacyRaw(storage);
  if (raw === null) return { status: "skipped", reason: "no-legacy-data" };

  let report: MigrationReport;
  try {
    report = await migrate(raw);
  } catch (error) {
    // 不动 localStorage。下次启动重试,后端幂等所以安全。
    return {
      status: "failed",
      message: error instanceof Error ? error.message : String(error),
    };
  }

  // 只有后端确认成功后才改名。顺序很重要:先写新键,再删旧键 —— 反过来的话
  // 中间崩溃会同时丢掉数据和标记。
  try {
    storage.setItem(MIGRATED_STORAGE_KEY, raw);
    storage.removeItem(LEGACY_STORAGE_KEY);
  } catch (error) {
    // 磁盘上的迁移已经成功了,只是标记没写上。下次启动会再迁一次,但后端
    // 幂等会把它们全部识别成 skipped。数据是安全的,所以这里不算失败。
    console.warn("Quick notes migrated, but the legacy key could not be renamed", error);
  }

  return { status: "migrated", report };
}

function safeLocalStorage(): StorageLike | null {
  try {
    // 隐私模式 / 沙盒环境下访问 localStorage 本身就可能抛。
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}
