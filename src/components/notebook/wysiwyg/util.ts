/**
 * 跨子模块共享的 DOM / 类型工具。范围严格限定在 wysiwyg/ 内部使用。
 */

/** widget destroy 时统一拆除挂在 DOM 上的 listener。多个 widget 类各自的
 *  install 函数返回此类型，存到 widget 实例上，destroy(dom) 时调用。 */
export type Cleanup = () => void;

/** event.target 是 Node | null，类型上不一定是 HTMLElement（可能是 Text 节点）。
 *  table / mousedown handler 都需要先收窄到 HTMLElement 才能 .closest。 */
export function eventElementTarget(event: Event): HTMLElement | null {
  const target = event.target;
  if (target instanceof HTMLElement) return target;
  if (target instanceof Node) return target.parentElement;
  return null;
}
