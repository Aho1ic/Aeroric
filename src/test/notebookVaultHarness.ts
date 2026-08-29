/* 随手记面板测试用的内存 vault。
 *
 * 面板落盘后每次新建 / 保存 / 删除都要过一次 Tauri 命令。这里实现一个内存版
 * 后端,让面板测试仍然验证**行为**而不是验证 mock 被调用过 —— 写进去的内容
 * 真的能读回来,冲突检测真的会触发。
 *
 * 刻意与 Rust 侧保持同样的语义:
 * - 保存要比对基线指纹,不一致就报 conflict
 * - 新建不覆盖,重名自动加序号
 * - 标题存 frontmatter,文件名只在新建时定一次
 */

export type HarnessSig = { mtimeMs: number; hash: string };

type HarnessFile = {
  content: string;
  mtimeMs: number;
};

const VAULT = "/vault";

/** 与 Rust 侧同一套 FNV-1a,保证测试里的冲突判定和真实行为一致。 */
function hash64(text: string): string {
  let hash = 0xcbf29ce484222325n;
  const bytes = new TextEncoder().encode(text);
  for (const byte of bytes) {
    hash ^= BigInt(byte);
    hash = (hash * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return hash.toString();
}

export class NotebookVaultHarness {
  private files = new Map<string, HarnessFile>();
  /** 单调递增的假时钟。真实 mtime 精度在某些文件系统上只到秒,测试里不能靠
   *  Date.now() —— 同一个 tick 内的两次写会拿到同样的 mtime。 */
  private clock = 1_000;
  /** 迁移过的原始 JSON,供断言检查备份行为。 */
  migratedRaw: string | null = null;
  /** 手工排序(文件名列表),对应 vault 里的 `.notebook/order.json`。 */
  order: string[] = [];
  /** 收尾迁移被调用过几次。用来确认它真的接进了启动流程。 */
  richtextConversions = 0;
  /** 「在文件管理器里揭示」收到的参数。要断言的不只是被调用过,还有 allowlist
   *  根传的是 vault —— 传错了这个入口就变成任意路径揭示器。 */
  revealCalls: { path: string; projectPath: string }[] = [];
  /** 落盘次数。「⌘S 在没有改动时不写盘」只能靠它看出来 —— 内容不变的写从
   *  `read()` 上看不出区别。 */
  saveCalls = 0;
  /** 让下一次保存直接失败(磁盘满、权限、IPC 断)。保存失败态是「关 tab 要确认」
   *  的唯一入口,没有它那条分支进不去。冲突**不**走这里 —— 冲突是正常分支。 */
  failNextSave = false;

  /** 直接往 vault 里放一个文件,模拟「磁盘上已经有笔记」。 */
  seed(fileName: string, content: string): string {
    const path = `${VAULT}/${fileName}`;
    this.files.set(path, { content, mtimeMs: (this.clock += 10) });
    return path;
  }

  /** 绕过面板直接改磁盘,模拟外部编辑器。用来触发冲突。 */
  externalWrite(path: string, content: string): void {
    this.files.set(path, { content, mtimeMs: (this.clock += 10) });
  }

  read(path: string): string | undefined {
    return this.files.get(path)?.content;
  }

  paths(): string[] {
    return [...this.files.keys()].sort();
  }

  private sigOf(path: string): HarnessSig {
    const file = this.files.get(path);
    if (!file) throw new Error(`no such file: ${path}`);
    return { mtimeMs: file.mtimeMs, hash: hash64(file.content) };
  }

  /** 接管 `invoke`。未知命令直接抛,避免悄悄吞掉真实调用。 */
  handle = (command: string, args: Record<string, unknown> = {}): unknown => {
    switch (command) {
      case "notebook_ensure_default_vault":
        return VAULT;

      case "notebook_read_tree":
        return [...this.files.entries()]
          .filter(([path]) => path.endsWith(".md"))
          .map(([path, file]) => ({
            name: path.slice(VAULT.length + 1),
            path,
            isDir: false,
            size: file.content.length,
            modifiedMs: file.mtimeMs,
            children: null,
            truncated: false,
          }));

      case "notebook_open_note": {
        const path = String(args.path);
        const file = this.files.get(path);
        if (!file) throw new Error(`no such file: ${path}`);
        return { content: file.content, sig: this.sigOf(path) };
      }

      case "notebook_close_note":
        return undefined;

      // 通用 fs 命令,不是 notebook_* 的。列表右键菜单的「在系统文件夹中打开」
      // 借了它,所以这里也要认。
      case "open_in_system_file_manager":
        this.revealCalls.push({
          path: String(args.path),
          projectPath: String(args.projectPath),
        });
        return undefined;

      case "notebook_read_order":
        return [...this.order];

      case "notebook_write_order":
        this.order = (args.names as string[]) ?? [];
        return undefined;

      case "notebook_save_note": {
        this.saveCalls += 1;
        if (this.failNextSave) {
          this.failNextSave = false;
          throw new Error("disk is on fire");
        }
        const path = String(args.path);
        const content = String(args.content);
        const expected = args.expected as HarnessSig | null;
        const force = Boolean(args.force);
        const existing = this.files.get(path);

        if (existing && !force) {
          const disk = this.sigOf(path);
          // 与 Rust 侧同一条判据:两个维度都不一致才算冲突。
          const stale = expected
            ? disk.hash !== expected.hash && disk.mtimeMs !== expected.mtimeMs
            : true;
          if (stale) return { status: "conflict", disk };
        }

        this.files.set(path, { content, mtimeMs: (this.clock += 10) });
        return { status: "saved", sig: this.sigOf(path) };
      }

      case "notebook_create_note_in_vault": {
        const title = String(args.title ?? "");
        const content = String(args.content ?? "");
        const path = this.allocate(title);
        this.files.set(path, { content, mtimeMs: (this.clock += 10) });
        return { path, sig: this.sigOf(path) };
      }

      case "notebook_delete_note": {
        this.files.delete(String(args.path));
        return undefined;
      }

      case "notebook_rename_to_title": {
        const path = String(args.path);
        const title = String(args.title ?? "");
        const file = this.files.get(path);
        if (!file) throw new Error(`no such file: ${path}`);
        const target = this.allocate(title);
        if (target === path) return path;
        this.files.delete(path);
        this.files.set(target, file);
        return target;
      }

      case "notebook_rename_note": {
        const from = String(args.from);
        const to = String(args.to);
        const file = this.files.get(from);
        if (!file) throw new Error(`no such file: ${from}`);
        if (this.files.has(to)) throw new Error(`ALREADY_EXISTS:${to}`);
        this.files.delete(from);
        this.files.set(to, file);
        return undefined;
      }

      case "notebook_convert_richtext": {
        // P1 收尾迁移。把 `editor: richtext` 的笔记正文换成一个标记,便于断言
        // 「转过了」;真正的 HTML → Markdown 语义由 Rust 侧测试覆盖。
        const converted: { path: string; title: string }[] = [];
        for (const [path, file] of this.files) {
          if (!path.endsWith(".md")) continue;
          if (!/^---\n(?:.*\n)*?editor: richtext\n/m.test(file.content)) continue;
          const next = file.content
            .split("\n")
            .filter((line) => !line.startsWith("editor:"))
            .join("\n");
          this.files.set(path, { content: next, mtimeMs: (this.clock += 10) });
          converted.push({ path, title: path.slice(VAULT.length + 1) });
        }
        this.richtextConversions += 1;
        return {
          vault: VAULT,
          backupDir: `${VAULT}/.notebook/richtext-backup-test`,
          converted,
          skipped: 0,
        };
      }

      case "notebook_migrate_legacy": {
        // 面板只关心「迁移成功了」,详细的迁移语义由 Rust 侧测试覆盖。
        this.migratedRaw = String(args.rawJson);
        return {
          vault: VAULT,
          backupPath: `${VAULT}/.notebook/legacy-backup-test.json`,
          migrated: [],
          skipped: [],
          totalInput: 0,
        };
      }

      default:
        throw new Error(`unexpected notebook command: ${command}`);
    }
  };

  /** 与后端 `allocate_note_path` 同样的 slug + 去重规则。 */
  private allocate(title: string): string {
    const stem = slugify(title);
    let name = `${stem}.md`;
    let suffix = 2;
    const taken = new Set([...this.files.keys()].map((path) => path.toLowerCase()));
    while (taken.has(`${VAULT}/${name}`.toLowerCase())) {
      name = `${stem}-${suffix}.md`;
      suffix += 1;
    }
    return `${VAULT}/${name}`;
  }
}

function slugify(title: string): string {
  let out = "";
  let lastDash = false;
  for (const ch of title) {
    const mapped = /[/\\:*?"<>|\s]/.test(ch) || ch.charCodeAt(0) < 0x20 ? "-" : ch;
    if (mapped === "-") {
      if (!lastDash && out.length > 0) {
        out += "-";
        lastDash = true;
      }
      continue;
    }
    out += mapped;
    lastDash = false;
  }
  const trimmed = out.replace(/^[-.\s]+|[-.\s]+$/g, "");
  return trimmed || "untitled";
}

export const HARNESS_VAULT = VAULT;
