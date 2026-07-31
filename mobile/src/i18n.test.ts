import { afterEach, describe, expect, it } from "vitest";
import { getLanguage, setLanguage, t } from "./i18n";

afterEach(() => setLanguage("zh"));

describe("i18n", () => {
  it("默认锁定中文,不跟随系统 locale", () => {
    expect(getLanguage()).toBe("zh");
    expect(t("common.retry")).toBe("重试");
  });

  it("setLanguage 可切到英文", () => {
    setLanguage("en");
    expect(getLanguage()).toBe("en");
    expect(t("common.retry")).toBe("Retry");
  });

  it("插值替换 {var}", () => {
    expect(t("notify.body", { name: "构建文档" })).toBe("构建文档");
    expect(t("home.newTaskFor", { name: "my-app" })).toContain("my-app");
  });

  it("缺失变量时保留原占位符", () => {
    expect(t("notify.body", {})).toBe("{name}");
  });

  it("权限文案与桌面端一致", () => {
    expect(t("perm.ask")).toBe("请求确认");
    expect(t("perm.auto_edit")).toBe("替我审批");
    expect(t("perm.full_access")).toBe("完全访问");
  });
});
