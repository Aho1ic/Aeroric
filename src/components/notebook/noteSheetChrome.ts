import { useEffect, useRef, type CSSProperties, type KeyboardEvent } from "react";

/**
 * 随手记侧栏 sheet 的共用外壳零件。
 *
 * 七个 sheet(回收站/字段/关系图/历史/属性/任务收集箱/搜索)原先各自复制了同一套
 * 壳:同样的 overlay 样式、同样的 header 样式(只有 gap 差 6 与 8)、同样的
 * `role="dialog"` + `aria-modal`、同样的挂载后聚焦关闭按钮、以及同样那段
 * "Esc 必须 stopPropagation"的处理 —— 连注释都互相引用("和另外三个 sheet 一致
 * (理由见 NoteFieldsSheet 里那段)")。
 *
 * 这里只收**逐字节相同**的那部分。壳本身**不能**做成一个包裹组件:
 *   - Fields / History 的 overlay 是 row 方向(左侧列表 + 右侧详情的主从分栏),
 *     header 在右栏里面,不在 overlay 的第一层;
 *   - 各 sheet 在关闭按钮之前还有自己的控件(清空 / 深度选择 / 刷新 / 恢复),
 *     `marginLeft: "auto"` 挂在谁身上各不相同 —— 由壳统一渲染关闭按钮会让这些
 *     控件的位置全部改变;
 *   - Search 的 header(padding "7px 9px" + `--bg-sidebar` 背景)和关闭按钮
 *     (24×24、`--text-muted`)与其余六个都不同,且它聚焦的是搜索框而不是关闭按钮。
 * 所以这里导出常量与 hook,由各 sheet 自己拼 JSX。
 */

/** 铺满面板的浮层,自上而下排。Trash / Graph / Properties / TaskInbox / Search 用。 */
export const noteSheetOverlayStyle: CSSProperties = {
  position: "absolute",
  inset: 0,
  zIndex: 30,
  display: "flex",
  flexDirection: "column",
  background: "var(--bg-panel)",
};

/**
 * 同上,但左右排 —— Fields / History 是「左列表 + 右详情」的主从分栏,
 * 它们的 header 在右栏内部。不要给这两个加 `flexDirection: "column"`。
 */
export const noteSheetSplitOverlayStyle: CSSProperties = {
  position: "absolute",
  inset: 0,
  zIndex: 30,
  display: "flex",
  background: "var(--bg-panel)",
};

/**
 * sheet 顶栏。`gap` 各 sheet 不同(6 或 8),所以要显式传 —— 给默认值会让改动
 * 悄悄改掉某几个 sheet 的间距。
 */
export function noteSheetHeaderStyle(gap: number): CSSProperties {
  return {
    minHeight: 32,
    display: "flex",
    alignItems: "center",
    gap,
    padding: "0 8px",
    borderBottom: "1px solid var(--border-dim)",
    color: "var(--text-muted)",
    fontSize: 11.5,
  };
}

/**
 * 顶栏里的图标按钮(关闭 / 刷新 / 图谱重算)。六个 sheet 算出来的样式完全一致,
 * 只是写法各异:有的本地叫 `actionStyle` 再覆盖 `color`,有的叫 `iconButtonStyle`,
 * 有的直接内联。照抄那个结果,不要"顺手美化",否则六处外观会一起变。
 * (Search 的关闭按钮是另一套尺寸,不用这个。)
 */
export const noteSheetIconButtonStyle: CSSProperties = {
  display: "flex",
  padding: 3,
  border: "none",
  borderRadius: 4,
  background: "transparent",
  color: "var(--text-hint)",
  cursor: "pointer",
};

export type NoteSheetDismiss = {
  /** 挂到关闭按钮上 —— 打开时焦点会落在它身上。 */
  closeRef: React.RefObject<HTMLButtonElement | null>;
  /** 摊到 overlay 那个 div 上:`role` / `aria-modal` / `aria-label` / `onKeyDown`。 */
  overlayProps: {
    role: "dialog";
    "aria-modal": true;
    "aria-label": string;
    onKeyDown: (event: KeyboardEvent<HTMLDivElement>) => void;
  };
};

/**
 * sheet 的浮层语义 + Esc 关闭 + 打开即聚焦。
 *
 * 聚焦是必需的:不挪焦点的话焦点还留在编辑器上,Esc 会被编辑器的按键处理先吃掉
 * (事件在编辑器那棵子树里冒泡,不经过 overlay 这个 div),而 Tab 会从被遮住的
 * 元素开始走。
 *
 * Esc 的 `stopPropagation`:**目前**没有可碰撞的对象 —— 面板自己没有 Esc 处理,
 * 宿主那个 window 监听要按住修饰键才进,所以去掉它测试照样绿(验过)。留着是因为
 * 它是"浮层拦掉自己的 Esc"的正确写法,而给面板加一层 Esc 是随时会发生的改动 ——
 * 那时候少了它就变成一次按键关两层。测不出来,所以在这里写清。
 */
export function useNoteSheetDismiss(ariaLabel: string, onClose: () => void): NoteSheetDismiss {
  const closeRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    closeRef.current?.focus();
  }, []);

  return {
    closeRef,
    overlayProps: {
      role: "dialog",
      "aria-modal": true,
      "aria-label": ariaLabel,
      onKeyDown: (event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        onClose();
      },
    },
  };
}
