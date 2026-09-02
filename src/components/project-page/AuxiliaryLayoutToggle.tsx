import { Columns2, Maximize2 } from "lucide-react";

import { useI18n } from "../../i18n";
import type { AuxiliaryWorkspaceLayout } from "./viewMode";

/**
 * 附属工作区右上角那个「分屏 ⇄ 全屏」的切换按钮。
 *
 * 从 `ProjectPage.tsx` 模块层整块搬出来,一行没改。它本来就是个独立组件,
 * 只是被写在了 3000 行文件的顶部。
 *
 * 图标与 `aria-label` 都表示**点下去会切到的那个状态**:当前是 `full` 时显示分栏图标、
 * 读作「分屏视图」。
 */
export function AuxiliaryLayoutToggle({
  layout,
  onChange,
}: {
  layout: AuxiliaryWorkspaceLayout;
  onChange: (layout: AuxiliaryWorkspaceLayout) => void;
}) {
  const { t } = useI18n();
  return (
    <button
      type="button"
      className="ssh-workspace-icon-btn auxiliary-layout-toggle"
      aria-label={layout === "full" ? t("ssh.splitView") : t("ssh.fullView")}
      title={layout === "full" ? t("ssh.splitView") : t("ssh.fullView")}
      onClick={() => onChange(layout === "full" ? "split" : "full")}
    >
      {layout === "full" ? <Columns2 size={15} /> : <Maximize2 size={15} />}
    </button>
  );
}
