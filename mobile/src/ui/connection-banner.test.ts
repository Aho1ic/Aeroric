import { afterEach, describe, expect, it } from "vitest";
import { setLanguage, t } from "../i18n";
import type { HostCloseReason } from "../transport/remote-connection";
import { connectionBannerMeta } from "./connection-banner";
import { theme } from "./theme";

afterEach(() => setLanguage("zh"));

describe("connectionBannerMeta", () => {
  it("断线原因是 signed-out 时说明是电脑端登出,而不是笼统的自动重连", () => {
    const meta = connectionBannerMeta({
      status: "reconnecting",
      authError: null,
      hostCloseReason: "signed-out",
    });

    expect(meta).toEqual({
      text: "电脑端已退出登录,登录后会自动恢复连接",
      color: theme.warning,
      showRePair: false,
    });
    // 登出会在主机重新登录后自愈,配对本身没坏:不能引导用户去重新配对
    expect(meta?.text).not.toBe(t("home.reconnecting"));
  });

  it("英文字典下同样给出登出文案", () => {
    setLanguage("en");

    expect(
      connectionBannerMeta({
        status: "reconnecting",
        authError: null,
        hostCloseReason: "signed-out",
      })?.text,
    ).toBe("Signed out on the computer — reconnects once you sign back in");
  });

  it("没有原因时保持原来的自动重连兜底", () => {
    expect(
      connectionBannerMeta({ status: "reconnecting", authError: null, hostCloseReason: null }),
    ).toEqual({
      text: t("home.reconnecting"),
      color: theme.warning,
      showRePair: false,
    });
  });

  it("非 signed-out 的原因不改写兜底文案", () => {
    // 线上原因是对端可控文本;将来新增成员时,未识别的原因必须落回通用文案
    const unknown = "host-restarting" as HostCloseReason;

    expect(
      connectionBannerMeta({ status: "reconnecting", authError: null, hostCloseReason: unknown })
        ?.text,
    ).toBe(t("home.reconnecting"));
  });

  it("授权失效仍优先显示 authError 并给出重新配对入口", () => {
    expect(
      connectionBannerMeta({
        status: "unauthorized",
        authError: "device revoked",
        hostCloseReason: "signed-out",
      }),
    ).toEqual({ text: "device revoked", color: theme.danger, showRePair: true });

    expect(
      connectionBannerMeta({ status: "unauthorized", authError: null, hostCloseReason: null }),
    ).toEqual({ text: t("home.authExpired"), color: theme.danger, showRePair: true });
  });

  it("在线与连接中不被残留的登出原因污染", () => {
    expect(
      connectionBannerMeta({ status: "online", authError: null, hostCloseReason: "signed-out" }),
    ).toEqual({ text: t("home.online"), color: theme.success, showRePair: false });

    expect(
      connectionBannerMeta({
        status: "authenticating",
        authError: null,
        hostCloseReason: "signed-out",
      }),
    ).toEqual({ text: t("home.connecting"), color: theme.accent, showRePair: false });
  });

  it("idle 与 stopped 不显示横幅", () => {
    expect(
      connectionBannerMeta({ status: "idle", authError: null, hostCloseReason: null }),
    ).toBeNull();
    expect(
      connectionBannerMeta({ status: "stopped", authError: null, hostCloseReason: "signed-out" }),
    ).toBeNull();
  });
});
