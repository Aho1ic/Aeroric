import type { AuxiliaryWorkspaceLayout, AuxiliaryWorkspaceType } from "./viewMode";

/**
 * 附属工作区(SSH / 文件 / 终端)的「分屏 or 全屏」偏好,按项目存 localStorage。
 *
 * 从 `ProjectPage.tsx` 模块层整块搬出来,逻辑一行没改。它不依赖组件里的任何东西,
 * 放在这儿能被测试直接调到 —— 原来它夹在 3000 行组件的顶部,只能靠渲染整个页面间接覆盖。
 */

export const AUXILIARY_LAYOUT_STORAGE_PREFIX = "aeroric:auxiliary-layout:";

export type AuxiliaryLayouts = Record<AuxiliaryWorkspaceType, AuxiliaryWorkspaceLayout>;

/**
 * 读三个附属工作区各自的布局偏好。
 *
 * 三条兜底路径都回到全 `"split"`:没有 window(SSR / 测试环境)、`JSON.parse` 抛错、
 * 以及存进去的值不是 `"full"`。最后一条是**逐字段**判的,所以半坏的数据
 * (比如 `{"ssh":"full","file":123}`)只会让坏的那个字段回落,不会整份丢掉。
 */
export function readAuxiliaryLayouts(projectId: string): AuxiliaryLayouts {
  const defaults: AuxiliaryLayouts = { ssh: "split", file: "split", terminal: "split" };
  if (typeof window === "undefined") return defaults;
  try {
    const parsed = JSON.parse(
      window.localStorage.getItem(`${AUXILIARY_LAYOUT_STORAGE_PREFIX}${projectId}`) ?? "{}",
    ) as Partial<AuxiliaryLayouts>;
    return {
      ssh: parsed.ssh === "full" ? "full" : "split",
      file: parsed.file === "full" ? "full" : "split",
      terminal: parsed.terminal === "full" ? "full" : "split",
    };
  } catch {
    return defaults;
  }
}
