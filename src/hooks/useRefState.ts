/**
 * useState + 同步 ref 的合并:返回与 useState 完全同形的 `[value, setValue]`,
 * 外加一个每次写入都同步刷新的 ref。value 必须经过 setValue 来更新 —— 绕过
 * setValue 直接改 ref 是 bug。
 *
 * App.tsx 的 projects / tasks / sshConnections 原本各抄一份这个样板
 * (useState + useRef + 手写 setter),用于事件回调里免依赖数组地读最新列表。
 */
import { useCallback, useRef, useState, type SetStateAction } from "react";

export function useRefState<S>(initial: S | (() => S)) {
  const [value, setValueState] = useState<S>(initial);
  const valueRef = useRef(value);
  const setValue = useCallback((update: SetStateAction<S>) => {
    const previous = valueRef.current;
    const next = typeof update === "function" ? (update as (prev: S) => S)(previous) : update;
    valueRef.current = next;
    setValueState(next);
  }, []);
  return [value, setValue, valueRef] as const;
}
