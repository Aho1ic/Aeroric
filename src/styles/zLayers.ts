/* 全局 z-index 层级量表 —— 唯一来源。
 *
 * 背景:此前各处独立拍板 z-index(overlay 用过 1000/1100/1200/1600/3000,
 * 浮层用过 60/100/200/1200/2000/4000/9999),导致 Radix Portal 浮层与模态
 * 遮罩层同挂 document.body 时相互遮挡。典型故障:SSH 新建连接弹窗
 * (overlay 3000)里的分组下拉(content 2000)被遮罩完全盖住。
 *
 * 规则:
 * 1. 任何瞬态浮层(select/popover/menu/tooltip)必须高于任何模态遮罩层。
 *    浮层失焦即关,不会挡住后续操作;遮罩层则是长驻的。
 * 2. 新增层级必须写进本文件,不要在组件里写裸数字。
 * 3. 同层级内的相对顺序靠 DOM 顺序,不再细分数值。
 *
 * src/test/z-layers.test.ts 会断言浮层严格高于遮罩层。
 */

export const zLayers = {
  /** 面板内的普通层叠(卡片抬层、选中态描边等)。 */
  base: 1,
  /** 表格表头、工具栏等 sticky 元素。 */
  sticky: 20,
  /** 不走 Portal、跟随容器裁剪的内联下拉(mention/skill 补全等)。 */
  dropdownInline: 200,
  /** 模态遮罩层与对话框本体。 */
  overlay: 3000,
  /** 嵌套在另一个模态里的模态(如 skill 管理 → 安装)。 */
  overlayNested: 3100,
  /** 三层嵌套模态(skill 管理 → 安装 → 冲突确认)。 */
  overlayNestedDeep: 3200,
  /** Radix Select / Popover / DropdownMenu 等 Portal 浮层。 */
  popover: 4200,
  /** 从另一个浮层里展开的二级浮层(如 ModelOptionsMenu 的子菜单)。 */
  popoverNested: 4300,
  /** 右键菜单的全屏点击捕获层,须紧贴在菜单之下。 */
  contextMenuBackdrop: 4399,
  /** 右键菜单本体。 */
  contextMenu: 4400,
  /** 全局 Toast 通知。 */
  toast: 4600,
} as const;

export type ZLayer = keyof typeof zLayers;

/** 遮罩层层级集合 —— 浮层必须严格高于其中每一个。 */
export const OVERLAY_LAYERS: ZLayer[] = ["overlay", "overlayNested", "overlayNestedDeep"];

/** 浮层层级集合 —— 每一个都必须严格高于所有遮罩层。 */
export const FLOATING_LAYERS: ZLayer[] = [
  "popover",
  "popoverNested",
  "contextMenuBackdrop",
  "contextMenu",
  "toast",
];
