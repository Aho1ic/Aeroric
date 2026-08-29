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
  /**
   * 单调递增的假时钟。真实 mtime 精度在某些文件系统上只到秒,测试里不能靠
   * Date.now() —— 同一个 tick 内的两次写会拿到同样的 mtime。
   *
   * 基准取一个真实的 epoch 毫秒(2026-01-01)而不是 0 附近:属性面板会把 mtime
   * 格式化出来给人看,从 1000 起算的话面板上是一个 1970-01-01,而那正是这个面板
   * 用来表示"时间戳没读到"的样子。
   */
  private clock = 1_767_225_600_000;
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
  /** 存下来的附件,旧的在前。 */
  attachments: {
    path: string;
    name: string;
    relativePath: string;
    size: number;
    modifiedMs: number;
    kind: string;
  }[] = [];
  /** 被读过字节的附件路径。图片解析层"只读一次"靠它验。 */
  attachmentReads: string[] = [];
  /** 让接下来几次附件保存失败。多张图里只有一张失败时的降级路径要用。 */
  private failingAttachmentSaves = 0;
  /** 让附件列表失败。分区的错误态只能从这里进。 */
  failAttachmentList = false;
  /** 让读附件字节失败(文件正被写、权限变了)。图片的坏图标记只能从这里进。 */
  failAttachmentReads = false;
  /** 让读文件元数据失败。属性面板的错误态只能从这里进。 */
  failNoteStat = false;

  /** 让接下来 `count` 次附件保存抛错。 */
  failAttachmentSaves(count = 1): void {
    this.failingAttachmentSaves = count;
  }
  /**
   * 版本历史快照,按路径分组,新的在前。
   *
   * 和 Rust 侧的差别:这里**不限流**,每次成功保存都留一条。真实后端两条快照之间
   * 至少隔三分钟,但那个窗口按真实时钟算,而这里的时钟是假的 —— 照搬会让所有
   * 保存都落在同一个窗口里,历史永远只有一条,面板测试也就没东西可看了。限流本身
   * 由 Rust 侧的 `rapid_autosaves_share_one_snapshot` 覆盖。
   */
  private snapshots = new Map<string, { id: string; content: string; createdAtMs: number }[]>();
  /** 让下一次快照相关的调用失败,用来验历史面板的错误态。 */
  failNextSnapshotCall = false;
  /**
   * 挂起中的 `notebook_read_snapshot`,按调用顺序排。`holdSnapshotReads()` 之后
   * 每次读快照都停在这里,要测试手工放行。
   *
   * 为什么需要它:面板里"回来的不是当前选中的那条就丢掉"这条守卫,只有在两个
   * 请求**乱序**返回时才看得出来。默认 harness 是同步返回的,两个请求永远按发起
   * 顺序完成,那条分支进不去 —— 于是守卫在测试里等于不存在。
   */
  private heldSnapshotReads: (() => void)[] | null = null;
  /**
   * 软删的笔记,按回收站 id 存 —— 对应 vault 的 `.notebook/trash/`。
   *
   * 删除**不是**从这个 map 里消失就完了:笔记先离开 `files` 进这里,恢复时再回去。
   * 让删除直接丢掉内容的话「恢复」在测试里就永远拿不到正文,回收站那一整套行为
   * 也就无从验证。
   */
  private trashed = new Map<
    string,
    { name: string; relativePath: string; deletedAtMs: number; content: string }
  >();
  /**
   * 还要让接下来几次回收站调用失败。用来验回收站面板的错误态。
   *
   * 是计数而不是布尔:面板在「清空失败」之后会自己再拉一次列表来纠正清单,那条
   * 补救路径本身也可能失败。只能注入一次失败的话,这两级降级里的第二级就没法验。
   */
  private failingTrashCalls = 0;

  /** 让接下来 `count` 次回收站调用抛错。 */
  failTrashCalls(count = 1): void {
    this.failingTrashCalls = count;
  }

  /** 这一次回收站调用该不该失败。为真时顺带把额度扣掉。 */
  private shouldFailTrashCall(): boolean {
    if (this.failingTrashCalls <= 0) return false;
    this.failingTrashCalls -= 1;
    return true;
  }

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

      case "notebook_note_stat": {
        const path = String(args.path);
        if (this.failNoteStat) throw new Error("reading file info failed");
        const file = this.files.get(path);
        if (!file) throw new Error(`no such file: ${path}`);
        return {
          // 字节数按 UTF-8 算,和真实后端的 `meta.len()` 一致 —— 用 `length` 的话
          // 一篇中文笔记会报成三分之一大。
          size: new TextEncoder().encode(file.content).length,
          modifiedMs: file.mtimeMs,
          // 假时钟没有"创建时间"的概念。给 null 正好覆盖"文件系统不记它"那条路径。
          createdMs: null,
        };
      }

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

        // 快照记在冲突判定之后、写盘之前,和 Rust 侧同序 —— 报冲突的那次保存
        // 不留快照,存的是被覆盖掉的那一版。
        if (existing && existing.content !== content) this.pushSnapshot(path, existing.content);
        this.files.set(path, { content, mtimeMs: (this.clock += 10) });
        return { status: "saved", sig: this.sigOf(path) };
      }

      case "notebook_list_snapshots": {
        if (this.failNextSnapshotCall) {
          this.failNextSnapshotCall = false;
          throw new Error("history is unavailable");
        }
        const path = String(args.path);
        return (this.snapshots.get(path) ?? []).map((snapshot) => ({
          id: snapshot.id,
          filePath: path,
          relativePath: path.slice(VAULT.length + 1),
          createdAtMs: snapshot.createdAtMs,
          size: snapshot.content.length,
        }));
      }

      case "notebook_read_snapshot": {
        if (this.failNextSnapshotCall) {
          this.failNextSnapshotCall = false;
          throw new Error("snapshot is unreadable");
        }
        const path = String(args.path);
        const entryId = String(args.entryId);
        const found = (this.snapshots.get(path) ?? []).find((snapshot) => snapshot.id === entryId);
        if (!found) throw new Error(`no such snapshot: ${entryId}`);
        const payload = {
          entry: {
            id: found.id,
            filePath: path,
            relativePath: path.slice(VAULT.length + 1),
            createdAtMs: found.createdAtMs,
            size: found.content.length,
          },
          content: found.content,
        };
        const held = this.heldSnapshotReads;
        if (!held) return payload;
        // 挂住:`invoke` 的 mock 会 await 这个 promise,于是这次读要等测试放行。
        return new Promise((resolve) => {
          held.push(() => resolve(payload));
        });
      }

      case "notebook_restore_snapshot": {
        if (this.failNextSnapshotCall) {
          this.failNextSnapshotCall = false;
          throw new Error("rollback failed");
        }
        const path = String(args.path);
        const entryId = String(args.entryId);
        const found = (this.snapshots.get(path) ?? []).find((snapshot) => snapshot.id === entryId);
        if (!found) throw new Error(`no such snapshot: ${entryId}`);
        const existing = this.files.get(path);
        // 兜底快照:被回滚覆盖掉的那一版要留下来,否则回滚不可撤销。
        if (existing) this.pushSnapshot(path, existing.content);
        this.files.set(path, { content: found.content, mtimeMs: (this.clock += 10) });
        return {
          content: found.content,
          sig: this.sigOf(path),
          entry: {
            id: found.id,
            filePath: path,
            relativePath: path.slice(VAULT.length + 1),
            createdAtMs: found.createdAtMs,
            size: found.content.length,
          },
        };
      }

      case "notebook_create_note_in_vault": {
        const title = String(args.title ?? "");
        const content = String(args.content ?? "");
        const path = this.allocate(title);
        this.files.set(path, { content, mtimeMs: (this.clock += 10) });
        return { path, sig: this.sigOf(path) };
      }

      // 删除是**软删**:笔记搬进回收站,不是消失。和 Rust 侧同语义。
      case "notebook_delete_note": {
        const path = String(args.path);
        const file = this.files.get(path);
        if (!file) throw new Error(`no such file: ${path}`);
        this.files.delete(path);
        const deletedAtMs = (this.clock += 10);
        const relativePath = path.slice(VAULT.length + 1);
        const name = relativePath.split("/").pop() ?? relativePath;
        this.trashed.set(String(deletedAtMs), {
          name,
          relativePath,
          deletedAtMs,
          content: file.content,
        });
        return {
          id: String(deletedAtMs),
          name,
          relativePath,
          deletedAtMs,
          size: file.content.length,
          isDir: false,
        };
      }

      case "notebook_trash_list": {
        if (this.shouldFailTrashCall()) {
          throw new Error("trash is unavailable");
        }
        return (
          [...this.trashed.entries()]
            .map(([id, item]) => ({
              id,
              name: item.name,
              relativePath: item.relativePath,
              deletedAtMs: item.deletedAtMs,
              size: item.content.length,
              isDir: false,
            }))
            // 新删的在前,和后端一致。
            .sort((left, right) => right.deletedAtMs - left.deletedAtMs)
        );
      }

      case "notebook_trash_restore": {
        if (this.shouldFailTrashCall()) {
          throw new Error("restore failed");
        }
        const id = String(args.id);
        const item = this.trashed.get(id);
        if (!item) throw new Error(`no such trash item: ${id}`);
        const path = `${VAULT}/${item.relativePath}`;
        // 原路径被占用时报 ALREADY_EXISTS，和新建 / 改名同一个前缀。
        if (this.files.has(path)) throw new Error(`ALREADY_EXISTS:${path}`);
        this.files.set(path, { content: item.content, mtimeMs: (this.clock += 10) });
        this.trashed.delete(id);
        return { path, isDir: false };
      }

      case "notebook_trash_purge": {
        if (this.shouldFailTrashCall()) {
          throw new Error("purge failed");
        }
        const id = String(args.id);
        const item = this.trashed.get(id);
        if (!item) throw new Error(`no such trash item: ${id}`);
        this.trashed.delete(id);
        // 和 Rust 侧一致:彻底删除把这条的历史快照一起清掉。
        this.snapshots.delete(`${VAULT}/${item.relativePath}`);
        return undefined;
      }

      case "notebook_trash_purge_all": {
        if (this.shouldFailTrashCall()) {
          throw new Error("emptying the trash failed");
        }
        const count = this.trashed.size;
        for (const item of this.trashed.values()) {
          this.snapshots.delete(`${VAULT}/${item.relativePath}`);
        }
        this.trashed.clear();
        return count;
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

      case "notebook_attachment_save": {
        const note = String(args.note);
        if (!this.files.has(note)) throw new Error(`no such file: ${note}`);
        if (this.failingAttachmentSaves > 0) {
          this.failingAttachmentSaves -= 1;
          throw new Error("saving the attachment failed");
        }
        const mime = String(args.mime);
        const given = args.fileName == null ? null : String(args.fileName);
        // 和 Rust 侧同一套命名:笔记名 + 毫秒戳 + 扩展名。扩展名优先取文件名。
        const ext =
          given?.includes(".") === true
            ? given.split(".").pop()!.toLowerCase()
            : (mime.split("/")[1]?.replace("+xml", "") ?? "bin");
        const stem = note.slice(VAULT.length + 1).replace(/\.md$/, "");
        const name = `${stem}-${(this.clock += 10)}.${ext}`;
        return this.storeAttachment(note, name, given ?? name, String(args.dataBase64).length);
      }

      case "notebook_attachment_save_from_path": {
        const note = String(args.note);
        if (!this.files.has(note)) throw new Error(`no such file: ${note}`);
        if (this.failingAttachmentSaves > 0) {
          this.failingAttachmentSaves -= 1;
          throw new Error("saving the attachment failed");
        }
        const src = String(args.src);
        const base = src.split(/[\\/]/).pop() ?? "file";
        const ext = base.includes(".") ? base.split(".").pop()!.toLowerCase() : "bin";
        const stem = note.slice(VAULT.length + 1).replace(/\.md$/, "");
        return this.storeAttachment(note, `${stem}-${(this.clock += 10)}.${ext}`, base, 32);
      }

      case "notebook_attachment_list":
        if (this.failAttachmentList) throw new Error("listing attachments failed");
        // 新的在前,和后端一致。
        return [...this.attachments].reverse();

      case "notebook_attachment_read": {
        const path = String(args.path);
        if (this.failAttachmentReads) throw new Error("reading the attachment failed");
        const found = this.attachments.find((item) => item.path === path);
        if (!found) throw new Error(`no such attachment: ${path}`);
        this.attachmentReads.push(path);
        return new Uint8Array([1, 2, 3]).buffer;
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

  /**
   * 存一个附件并算出插进正文的 markdown。
   *
   * 链接相对**笔记所在目录**,和 Rust 侧 `finish` 一致 —— 子目录里的笔记要爬回
   * vault 根。写错这一条的话面板测试会对着一条断链断言成功。
   */
  private storeAttachment(note: string, name: string, alt: string, size: number) {
    const noteDir = note.slice(0, note.lastIndexOf("/"));
    const depth = noteDir.slice(VAULT.length).split("/").filter(Boolean).length;
    const relativePath = `attachments/${name}`;
    const link = `${"../".repeat(depth)}${relativePath}`;
    const kind = /\.(png|jpe?g|gif|webp|bmp|avif|ico|tiff?|heic)$/i.test(name)
      ? "image"
      : /\.svg$/i.test(name)
        ? "svg"
        : "pdf";
    const path = `${VAULT}/${relativePath}`;
    const stem = alt.replace(/\.[^.]+$/, "").replace(/[[\]]/g, "-");
    this.attachments.push({
      path,
      name,
      relativePath,
      size,
      modifiedMs: this.clock,
      kind,
    });
    return {
      path,
      name,
      link,
      markdown: kind === "pdf" ? `[${name}](${link})` : `![${stem}](${link})`,
      size,
    };
  }

  /** 记一条快照。新的在前,和后端 `list` 的顺序一致。 */
  private pushSnapshot(path: string, content: string): void {
    const list = this.snapshots.get(path) ?? [];
    const createdAtMs = (this.clock += 10);
    list.unshift({ id: String(createdAtMs), content, createdAtMs });
    this.snapshots.set(path, list);
  }

  /** 直接塞一条快照,免得测试为了造历史先保存好几次。 */
  seedSnapshot(path: string, content: string): string {
    this.pushSnapshot(path, content);
    return this.snapshots.get(path)![0].id;
  }

  snapshotCount(path: string): number {
    return this.snapshots.get(path)?.length ?? 0;
  }

  /** 快照内容,新的在前。 */
  snapshotContents(path: string): string[] {
    return (this.snapshots.get(path) ?? []).map((snapshot) => snapshot.content);
  }

  /** 回收站里的文件名,新删的在前。 */
  trashedNames(): string[] {
    return [...this.trashed.values()]
      .sort((left, right) => right.deletedAtMs - left.deletedAtMs)
      .map((item) => item.name);
  }

  /** 从现在起,读快照都停住不返回。 */
  holdSnapshotReads(): void {
    this.heldSnapshotReads = [];
  }

  /** 挂起的读快照有几个。 */
  heldSnapshotReadCount(): number {
    return this.heldSnapshotReads?.length ?? 0;
  }

  /** 放行第 `index` 个挂起的读快照(0 是最早发起的那个)。 */
  releaseSnapshotRead(index: number): void {
    const held = this.heldSnapshotReads;
    if (!held) throw new Error("snapshot reads are not held");
    const release = held[index];
    if (!release) throw new Error(`no held snapshot read at ${index}`);
    held[index] = () => {};
    release();
  }

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
