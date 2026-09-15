import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProjectAvatar } from "../components/ProjectAvatar";
import {
  AVATAR_PALETTE,
  AVATAR_PALETTE_KEYS,
  autoAvatarColorKey,
  autoAvatarLabel,
  normalizeAvatarOverride,
  resolveProjectAvatar,
} from "../projectAvatar";

describe("ProjectAvatar", () => {
  it("preserves project name casing in initials", () => {
    render(<ProjectAvatar name="aeroric" />);

    expect(screen.getByText("ae")).toBeInTheDocument();
    expect(screen.queryByText("AE")).not.toBeInTheDocument();
  });

  it("renders the custom emoji instead of initials", () => {
    render(<ProjectAvatar name="aeroric" avatar={{ emoji: "🚀" }} />);

    expect(screen.getByText("🚀")).toBeInTheDocument();
    expect(screen.queryByText("ae")).not.toBeInTheDocument();
  });

  it("renders custom initials over the derived ones", () => {
    render(<ProjectAvatar name="aeroric" avatar={{ label: "XY" }} />);

    expect(screen.getByText("XY")).toBeInTheDocument();
    expect(screen.queryByText("ae")).not.toBeInTheDocument();
  });

  it("paints the chosen palette colour, not the name-hashed one", () => {
    const { container } = render(<ProjectAvatar name="aeroric" avatar={{ color: "rose" }} />);
    const el = container.firstElementChild as HTMLElement;

    expect(el).toHaveAttribute("data-avatar-color", "rose");
    // jsdom 把 hex 规一化成 rgb(),所以按 rgb 比。
    const [from] = AVATAR_PALETTE.rose;
    const rgb = [1, 3, 5].map((i) => parseInt(from.slice(i, i + 2), 16)).join(", ");
    expect(el.style.background).toContain(`rgb(${rgb})`);
    expect(el.style.background).not.toContain(AVATAR_PALETTE.blue[0]);
  });
});

describe("autoAvatarColorKey", () => {
  it("always lands on a real palette key", () => {
    for (const name of ["aeroric", "", "项目", "a-very-long-project-name", "🚀"]) {
      expect(AVATAR_PALETTE_KEYS, name).toContain(autoAvatarColorKey(name));
    }
  });

  it("is stable for the same name", () => {
    expect(autoAvatarColorKey("aeroric")).toBe(autoAvatarColorKey("aeroric"));
  });

  it("separates names sharing a long prefix", () => {
    // 老的 hash*31 对同前缀名区分度差,这是换 FNV-1a 的原因。
    const keys = new Set(
      ["project-alpha", "project-beta", "project-gamma", "project-delta"].map(autoAvatarColorKey),
    );
    expect(keys.size).toBeGreaterThan(1);
  });
});

describe("autoAvatarLabel", () => {
  it("takes the letter after a separator for latin names", () => {
    expect(autoAvatarLabel("agent-config")).toBe("ac");
    expect(autoAvatarLabel("my_side_project")).toBe("ms");
    expect(autoAvatarLabel("deep space")).toBe("ds");
  });

  it("falls back to the first two letters without a separator", () => {
    expect(autoAvatarLabel("aeroric")).toBe("ae");
  });

  it("takes two characters for CJK names", () => {
    expect(autoAvatarLabel("语音助手")).toBe("语音");
  });

  it("returns empty for a blank name instead of throwing", () => {
    expect(autoAvatarLabel("")).toBe("");
    expect(autoAvatarLabel("   ")).toBe("");
  });
});

describe("resolveProjectAvatar", () => {
  it("falls back per field, not all-or-nothing", () => {
    // 只定了颜色:首字母仍走自动。
    const resolved = resolveProjectAvatar("agent-config", { color: "teal" });
    expect(resolved.colorKey).toBe("teal");
    expect(resolved.label).toBe("ac");
  });

  it("discards an unknown palette key and auto-colours instead", () => {
    const resolved = resolveProjectAvatar("aeroric", { color: "not-a-colour" });
    expect(resolved.colorKey).toBe(autoAvatarColorKey("aeroric"));
  });

  it("does not resolve prototype keys as palette colours", () => {
    // Object.hasOwn 而不是 `in`:constructor 不该算合法调色板键。
    expect(resolveProjectAvatar("aeroric", { color: "constructor" }).colorKey).toBe(
      autoAvatarColorKey("aeroric"),
    );
  });

  it("keeps the label resolved while an emoji is showing", () => {
    // 去掉 emoji 就该立刻回到首字母,所以 label 一直算着。
    const resolved = resolveProjectAvatar("agent-config", { emoji: "🚀" });
    expect(resolved.emoji).toBe("🚀");
    expect(resolved.label).toBe("ac");
  });

  it("clamps a pasted emoji run to a single grapheme", () => {
    expect(resolveProjectAvatar("x", { emoji: "🚀🔥✨" }).emoji).toBe("🚀");
  });

  it("keeps a ZWJ emoji whole", () => {
    // 👩‍💻 是 ZWJ 序列,按 code point 切会碎成两半。
    expect(resolveProjectAvatar("x", { emoji: "👩‍💻" }).emoji).toBe("👩‍💻");
  });

  it("clamps latin labels to 3 and CJK labels to 2", () => {
    expect(resolveProjectAvatar("x", { label: "ABCDEF" }).label).toBe("ABC");
    // CJK 按 1.5 宽算,两个字 = 3,第三个超限。
    expect(resolveProjectAvatar("x", { label: "语音助手" }).label).toBe("语音");
  });

  it("always yields a gradient matching its colour key", () => {
    const resolved = resolveProjectAvatar("aeroric", { color: "violet" });
    expect(resolved.gradient).toBe(AVATAR_PALETTE.violet);
  });
});

describe("normalizeAvatarOverride", () => {
  it("returns undefined when nothing is customised", () => {
    // 全空要给 undefined,否则 projects.json 里会长出 "avatar": {}。
    expect(normalizeAvatarOverride({})).toBeUndefined();
    expect(normalizeAvatarOverride({ color: "", emoji: "", label: "" })).toBeUndefined();
    expect(normalizeAvatarOverride({ label: "   " })).toBeUndefined();
    expect(normalizeAvatarOverride(undefined)).toBeUndefined();
    expect(normalizeAvatarOverride("nope")).toBeUndefined();
  });

  it("drops an invalid colour but keeps the rest", () => {
    expect(normalizeAvatarOverride({ color: "chartreuse", label: "AB" })).toEqual({ label: "AB" });
  });

  it("does not accept prototype keys as a colour", () => {
    // Object.hasOwn 而不是 `in`:constructor / toString 不是调色板键。
    expect(normalizeAvatarOverride({ color: "constructor" })).toBeUndefined();
    expect(normalizeAvatarOverride({ color: "toString" })).toBeUndefined();
    expect(normalizeAvatarOverride({ color: "hasOwnProperty", label: "AB" })).toEqual({
      label: "AB",
    });
  });

  it("keeps only the fields that survive", () => {
    expect(normalizeAvatarOverride({ color: "pink", emoji: "🚀", label: "AB" })).toEqual({
      color: "pink",
      emoji: "🚀",
      label: "AB",
    });
  });

  it("ignores non-string field types", () => {
    expect(normalizeAvatarOverride({ color: 3, emoji: [], label: {} })).toBeUndefined();
  });

  it("round-trips through resolve", () => {
    const stored = normalizeAvatarOverride({ color: "cyan", label: "  ZZ  " });
    expect(stored).toEqual({ color: "cyan", label: "ZZ" });
    expect(resolveProjectAvatar("aeroric", stored).label).toBe("ZZ");
  });
});
