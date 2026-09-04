import type React from "react";

/**
 * 隐藏一棵「要保活的」子树:必须用 `display:none`,不能用 `visibility:hidden`。
 *
 * 这类子树(ProjectPage / 任务面板 / shell 标签)都要留着挂载来保住终端与编辑器
 * 状态,所以不能卸载。但两种藏法的代价差得很远:
 *
 * - `visibility:hidden` 只是不画自己,元素**仍在布局里**,也**仍在动画时间线上**。
 * - `display:none` 把元素从布局和动画时间线上一起摘掉。
 *
 * 实测(真实浏览器,同一个 `filter` 无限动画,0.1s 一轮,量 2 秒):
 *
 * | 藏法 | animationiteration 次数 | getAnimations().length |
 * | --- | --- | --- |
 * | 可见 | 20 | 1 |
 * | `visibility:hidden` | **20** | **1** |
 * | `display:none` | **0** | **0** |
 *
 * 也就是说 `visibility:hidden` 的子树和完全可见的子树一样在跑动画 —— 每一轮都要
 * 重绘 + 走一遍 filter 渲染。本仓库里这类无限动画不少(`ui-spinner-rotate` 0.75s、
 * `spin` 1s 挂在运行中任务的状态图标上、`aeroric-terminal-tab-cursor` 1.8s、
 * `model-options-fast-flicker` 2.4s 带 `brightness` + `drop-shadow`),藏着的子树
 * 靠 `visibility:hidden` 是关不掉的,合成器也就永远不进 idle。
 *
 * `activeDisplay` 给的是「可见时」用哪种 display:默认 `flex`;xterm 的挂载容器要
 * 传 `block` —— 那层的子节点是 xterm 自己建的 DOM,不能变成 flex item。
 */
export function mountedSubtreeVisibilityStyle(
  visible: boolean,
  activeDisplay: "flex" | "block" = "flex",
): React.CSSProperties {
  return visible
    ? { display: activeDisplay, pointerEvents: "auto", zIndex: 1 }
    : { display: "none", pointerEvents: "none", zIndex: 0 };
}
