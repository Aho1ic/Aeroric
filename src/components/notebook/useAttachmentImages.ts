/**
 * 让笔记里的相对路径图片真的显示出来 —— 阅读态和编辑态两条路。
 *
 * 阅读态走 `dangerouslySetInnerHTML`,HTML 是一次性生成的字符串,没法在生成时就
 * 拿到 blob URL(解析是异步的)。所以这里在挂载后扫一遍 `<img>`,把需要解析的那些
 * 换掉。和公式 / Mermaid 的懒渲染是同一个形状。
 *
 * 编辑态由 CodeMirror 的图片 widget 自己调 `resolve`,这里只负责把 resolver 和
 * 当前笔记目录交给它(`attachmentContext`)。
 *
 * resolver 的生命周期跟着**笔记**:换笔记就把上一篇的 blob URL 全部回收,不然
 * 翻二十篇图多的笔记会攒下几百个 blob,而它们只有页面卸载才会被浏览器收走。
 */

import { useEffect, useMemo, useRef } from "react";
import { AttachmentUrlResolver, needsVaultResolve, noteDirOf } from "./attachmentUrls";
import type { AttachmentContext } from "./wysiwyg";

/**
 * 把容器里所有相对路径的 `<img>` 换成能显示的 URL。
 *
 * 读的是 `data-notebook-src` 而不是 `src`:HTML 一挂上去,浏览器就已经按相对路径
 * 发过一次注定失败的请求了。但 markdown 渲染出来的是 `src`,所以这里第一步先把
 * 待解析的地址搬到 data 属性上并清空 src —— 同一个节点被扫第二次时(公式渲染完
 * 会再触发一轮)就不会重复读。
 */
async function resolveImagesIn(
  host: HTMLElement,
  resolver: AttachmentUrlResolver,
  noteDir: string,
) {
  const images = Array.from(host.querySelectorAll<HTMLImageElement>("img"));
  await Promise.all(
    images.map(async (img) => {
      const pending = img.dataset.notebookSrc;
      // `getAttribute` 而不是 `img.src`:后者是解析过的绝对 URL,相对路径已经被
      // 拼成 `tauri://localhost/attachments/x.png`,原始写法拿不回来了。
      const raw = pending ?? img.getAttribute("src") ?? "";
      if (!raw || !needsVaultResolve(raw)) return;
      if (!pending) {
        img.dataset.notebookSrc = raw;
        img.removeAttribute("src");
      }
      try {
        img.src = await resolver.resolve(raw, noteDir);
        img.removeAttribute("data-notebook-error");
      } catch {
        // 标在节点上,样式表可以把它画成占位框;不弹全局错误 —— 一张图读不出来
        // 不该盖住整个面板。
        img.dataset.notebookError = "1";
      }
    }),
  );
}

/**
 * @param notePath 当前笔记的绝对路径。空串表示没有打开的笔记。
 * @param previewHost 阅读 / 分屏态承载渲染结果的容器。
 * @param renderKey 预览 DOM 的身份。变了就重扫一遍。
 *
 * `renderKey` 不能只用 HTML:切换阅读 ⇄ 分屏时 HTML 一模一样,但预览容器是另一个
 * 节点(两态的 JSX 位置不同),不重扫的话新容器里的图全是空的。所以调用方要把
 * 视图模式一起编进去。
 */
export function useAttachmentImages(
  notePath: string,
  previewHost: React.RefObject<HTMLElement | null>,
  renderKey: string,
): AttachmentContext {
  const noteDir = useMemo(() => (notePath ? noteDirOf(notePath) : ""), [notePath]);
  const resolverRef = useRef<AttachmentUrlResolver>(new AttachmentUrlResolver());
  // facet 的值必须**身份稳定**:它进了 CodeMirror 的 extension 数组,每次渲染换
  // 一个新对象会让 ReactCodeMirror 重建 view —— 光标和撤销栈全丢。所以 noteDir
  // 通过 ref 读,不进对象。
  const noteDirRef = useRef(noteDir);
  noteDirRef.current = noteDir;

  // 换笔记就回收上一篇的 blob。放在 effect 里而不是渲染中:渲染可能被丢弃,
  // 那时回收掉的 URL 属于还在显示的那一篇。
  //
  // 跟的是 `notePath` 而不是 `noteDir`:随手记基本都躺在同一个目录里,按目录算的话
  // 翻五十篇笔记会把五十篇的图全留在缓存里 —— 那正是这个模块要防的那种增长。代价是
  // 两篇笔记共用同一张图时换一次笔记要重读一次,而那是一次本地磁盘读。
  useEffect(() => {
    const resolver = resolverRef.current;
    return () => resolver.release();
  }, [notePath]);

  useEffect(() => {
    const host = previewHost.current;
    if (!host) return;
    void resolveImagesIn(host, resolverRef.current, noteDirRef.current);
  }, [previewHost, renderKey, notePath]);

  return useMemo<AttachmentContext>(
    () => ({
      // getter:facet 对象本身不换,但读到的目录始终是当前那一篇的。
      get noteDir() {
        return noteDirRef.current;
      },
      resolve: (url, dir) => resolverRef.current.resolve(url, dir),
    }),
    [],
  );
}
