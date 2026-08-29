/**
 * 把粘贴 / 拖入的图片存成附件并在光标处插入 markdown。
 *
 * 两条来路,给的东西不一样:
 *
 * - **HTML5 paste / drop** —— 从网页、聊天软件、截图工具来的。拿到的是 `File`
 *   blob,没有磁盘路径,只能编码成 base64 穿过 IPC(`saveAttachment`)。
 * - **Tauri 原生 drag-drop** —— 从系统文件管理器拖进来的。OS 不会触发 WebView 的
 *   HTML5 drop,事件被 Tauri 截走,给的是**绝对路径**(`saveAttachmentFromPath`)。
 *   这条路不编码:一张 8MB 的图 base64 之后是 11MB 的字符串,来回穿 IPC 纯浪费。
 *
 * 两条都汇到同一个插入逻辑:等所有附件写完,再把 markdown 一次性插进当前选区。
 * 一张一插会在撤销栈里留下 N 步,用户按一次 ⌘Z 只退回一张图。
 */

import { useCallback, useEffect, useRef } from "react";
import { saveAttachment, saveAttachmentFromPath, type SavedAttachment } from "./notebookApi";

/** 一次最多插几张。截图工具有时会把整个剪贴板历史都塞进来。 */
const MAX_FILES_PER_DROP = 10;

/** 认得出的图片扩展名。原生拖入只有路径,没有 MIME。 */
const IMAGE_EXTENSIONS = new Set([
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
  "bmp",
  "avif",
  "svg",
  "ico",
  "tiff",
  "tif",
  "heic",
]);

function isImagePath(path: string): boolean {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return IMAGE_EXTENSIONS.has(ext);
}

/**
 * `ArrayBuffer` → base64。
 *
 * 不用 `FileReader.readAsDataURL`:它给的是 data URL,还要再切前缀,而且它是
 * 事件式的,包一层 promise 反而更长。分块转是因为 `String.fromCharCode(...bytes)`
 * 在几 MB 的图上会把参数栈撑爆(RangeError)。
 */
function toBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  const CHUNK = 0x8000;
  let binary = "";
  for (let index = 0; index < bytes.length; index += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(index, index + CHUNK));
  }
  return btoa(binary);
}

export type NoteAttachmentDropOptions = {
  /** 当前笔记的绝对路径。空串表示没有打开的笔记,这时候不接受拖入。 */
  notePath: string;
  /** 把插入点定成一个空选区。异步保存期间由 CodeMirror 跟着后续编辑一起映射。 */
  setInsertPoint: (at: number) => void;
  /** 用文本替换当前选区。 */
  insert: (markdown: string) => void;
  /**
   * 视口坐标 → 文档偏移。点不在编辑器上时返回 null。
   *
   * 系统拖入必须过这一道:那个事件是整个窗口的,不判落点的话把文件拖到笔记列表
   * 甚至别的面板上都会往正文里插图。
   */
  posAtClientPoint: (x: number, y: number) => number | null;
  /** 附件写完了(面板要刷新附件列表)。 */
  onSaved?: () => void;
  onError: (message: string) => void;
  /** 没有打开笔记时的提示文案。 */
  noNoteMessage: string;
  /** 一次拖太多时的提示文案,`{count}` 是被跳过的张数。 */
  tooManyMessage: string;
};

export type NoteAttachmentDrop = {
  /** 交给 `NoteSourceEditor` 的 `onDropFiles`。 */
  handleFiles: (files: File[], at: number) => boolean;
};

export function useNoteAttachmentDrop(options: NoteAttachmentDropOptions): NoteAttachmentDrop {
  // 回调进了 CodeMirror 的 extension 数组,身份必须稳定 —— 每次渲染换一个新函数
  // 会让编辑器重建 view,光标和撤销栈全丢。所以选项走 ref。
  const optionsRef = useRef(options);
  optionsRef.current = options;

  const insertSaved = useCallback((saved: SavedAttachment[]) => {
    if (saved.length === 0) return;
    // 多张图之间空一行:紧挨着写的话 markdown 会把它们渲染进同一个段落,
    // 变成一行里挤好几张图。
    optionsRef.current.insert(saved.map((item) => item.markdown).join("\n\n"));
    optionsRef.current.onSaved?.();
  }, []);

  const handleFiles = useCallback(
    (files: File[], at: number): boolean => {
      const current = optionsRef.current;
      if (!current.notePath) {
        current.onError(current.noNoteMessage);
        // 仍然算"接手了":不拦的话浏览器会把图片文件名当纯文本插进笔记。
        return true;
      }
      const accepted = files.slice(0, MAX_FILES_PER_DROP);
      const skipped = files.length - accepted.length;
      current.setInsertPoint(at);
      void (async () => {
        const saved: SavedAttachment[] = [];
        try {
          for (const file of accepted) {
            const base64 = toBase64(await file.arrayBuffer());
            saved.push(
              await saveAttachment(current.notePath, base64, file.type, file.name || undefined),
            );
          }
        } catch (error) {
          // 已经存下来的那些照样插进去:让用户丢掉三张里成功的两张没有道理。
          current.onError(error instanceof Error ? error.message : String(error));
        }
        insertSaved(saved);
        if (skipped > 0) {
          current.onError(current.tooManyMessage.replace("{count}", String(skipped)));
        }
      })();
      return true;
    },
    [insertSaved],
  );

  // 系统文件管理器拖入。OS 把事件给了窗口而不是 WebView,所以只能听 Tauri 的。
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let mounted = true;
    void (async () => {
      try {
        const { getCurrentWebview } = await import("@tauri-apps/api/webview");
        unlisten = await getCurrentWebview().onDragDropEvent((event) => {
          if (!mounted || event.payload.type !== "drop") return;
          const current = optionsRef.current;
          // 没有打开笔记时静默忽略:这个事件是**整个窗口**的,别的面板里的拖放
          // 不该弹随手记的错误。
          if (!current.notePath) return;
          const paths = event.payload.paths.filter(isImagePath);
          if (paths.length === 0) return;
          // 事件给的是**物理像素**,而 DOM 的 getBoundingClientRect 是 CSS 像素。
          // 在缩放屏(macOS Retina 是 2x)上不换算的话落点会偏到两倍远的地方。
          const ratio = window.devicePixelRatio || 1;
          const at = current.posAtClientPoint(
            event.payload.position.x / ratio,
            event.payload.position.y / ratio,
          );
          // 落点不在编辑器上:这次拖放不是给正文的。
          if (at === null) return;
          current.setInsertPoint(at);
          void (async () => {
            const saved: SavedAttachment[] = [];
            try {
              for (const path of paths.slice(0, MAX_FILES_PER_DROP)) {
                saved.push(await saveAttachmentFromPath(current.notePath, path));
              }
            } catch (error) {
              current.onError(error instanceof Error ? error.message : String(error));
            }
            insertSaved(saved);
          })();
        });
      } catch {
        // 不在 Tauri 里(测试 / 纯浏览器)就只剩 HTML5 那条路,不是错误。
      }
    })();
    return () => {
      mounted = false;
      unlisten?.();
    };
  }, [insertSaved]);

  return { handleFiles };
}
