/* 侧栏那种「按需扫全库」的取数:反链和标签共用。
 *
 * 三件事都不能省,而且两档必须一模一样:
 *
 * - **只在这一档可见时扫。** 扫描要读每个文件的全文,是整个面板里最贵的一次 IO,
 *   而绝大多数时候用户根本没打开侧栏。
 * - **报错就地显示,并留住上一次的结果。** 清空成"什么都没有"比留着旧结果更糟 ——
 *   那看起来像扫完了、确实没有。
 * - **换笔记不重扫。** 结果是全库的,换笔记只是换一个筛选条件。
 *
 * 抽成 hook 而不是在面板里写两遍:这三条里任何一条在两档之间漂移,表现都是"其中
 * 一档偶尔看起来是空的",而这种偏差没人会往取数逻辑上想。
 *
 * 第三条对**未链接提及**那一档不成立:它的扫描参数里就有当前笔记的名字,换笔记之后
 * 上一次的结果讲的是另一篇。那一档靠 `resetKey` 声明这件事,见它的文档注释 ——
 * 与其复制一份"几乎一样但有一处不同"的取数,不如把那一处差异写成参数。
 */

import { useCallback, useEffect, useRef, useState } from "react";

export type VaultScan<T> = {
  data: T[];
  loading: boolean;
  /** 扫描失败的文案。只读视图,失败不影响读写笔记,所以不占用面板那条错误条。 */
  error: string | null;
  /** 手工重扫。外部编辑改了别人的笔记时,只能靠重扫发现。 */
  refresh: () => void;
};

/**
 * 按需扫全库。
 *
 * `enabled` 转 true 时扫一次;`vault` 变了或手工刷新时重扫。转回 false 不清结果 ——
 * 用户来回切档时不该每次都等一遍。
 *
 * `scan` 由调用方用 `useCallback` 稳住(或者是模块级函数):它进依赖,每次渲染换
 * 一个新函数会变成扫描不停。
 *
 * `resetKey` 变了就把已有结果**清空**(而不只是重扫)。给"结果只对某个上下文成立"的
 * 那种扫描用 —— 未链接提及的扫描参数里有当前笔记的名字,换笔记之后旧结果讲的是另一
 * 篇,留着它会在新笔记的标题下面显示上一篇的提及,那比空列表糟得多。不传就是全库
 * 视图的语义(反链 / 标签 / 字段 / 任务),行为和以前一字不差。
 */
export function useVaultScan<T>(
  vault: string | null,
  enabled: boolean,
  scan: (vault: string) => Promise<T[]>,
  errorText: (error: unknown) => string,
  resetKey?: string | null,
): VaultScan<T> {
  const [data, setData] = useState<T[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [token, setToken] = useState(0);
  const lastKey = useRef(resetKey);

  useEffect(() => {
    /* 清空要在**发请求之前**做,而且不受 `enabled` 影响:这一档关着的时候换了笔记,
       下次打开时不该先看到上一篇的结果闪一下。 */
    if (lastKey.current !== resetKey) {
      lastKey.current = resetKey;
      setData([]);
      setError(null);
    }
    if (!vault || !enabled) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    void (async () => {
      try {
        const next = await scan(vault);
        if (cancelled) return;
        setData(next);
      } catch (failure: unknown) {
        if (cancelled) return;
        setError(errorText(failure));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [enabled, errorText, resetKey, scan, token, vault]);

  const refresh = useCallback(() => setToken((current) => current + 1), []);
  return { data, loading, error, refresh };
}
