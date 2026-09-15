/**
 * 首页连接横幅的文案派生:把「状态 + 授权错误 + 主机 typed 关闭原因」映射成
 * 一条可显示的横幅。纯函数、无 RN 依赖,vitest 直测;状态机本身仍在
 * transport/remote-connection.ts 里,这里只负责说人话。
 */

import { t } from "../i18n";
import type { ConnectionStatus, HostCloseReason } from "../transport/remote-connection";
import { theme } from "./theme";

export interface ConnectionBannerInput {
  status: ConnectionStatus;
  authError: string | null;
  /** 主机最近一次 close frame 上的 typed 原因;未收到或已退休时为 null。 */
  hostCloseReason: HostCloseReason | null;
}

export interface ConnectionBannerMeta {
  text: string;
  color: string;
  /** 是否在横幅右侧给出「重新配对」入口(仅授权失效时;登出会自愈,配对没坏)。 */
  showRePair: boolean;
}

/**
 * 返回 null 表示不显示横幅(idle / stopped:没有正在进行的连接可交代)。
 *
 * signed-out 只在断线态覆盖文案:自动重连仍在跑,但停摆的原因在主机那头,
 * 用户在手机上做任何操作都没用,必须直说「电脑端已退出登录」。
 */
export function connectionBannerMeta({
  status,
  authError,
  hostCloseReason,
}: ConnectionBannerInput): ConnectionBannerMeta | null {
  switch (status) {
    case "online":
      return { text: t("home.online"), color: theme.success, showRePair: false };
    case "connecting":
    case "authenticating":
      return { text: t("home.connecting"), color: theme.accent, showRePair: false };
    case "reconnecting":
      if (hostCloseReason === "signed-out") {
        return { text: t("home.hostSignedOut"), color: theme.warning, showRePair: false };
      }
      return { text: t("home.reconnecting"), color: theme.warning, showRePair: false };
    case "unauthorized":
      return { text: authError ?? t("home.authExpired"), color: theme.danger, showRePair: true };
    default:
      return null;
  }
}
