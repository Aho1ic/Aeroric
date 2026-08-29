import { describe, expect, it } from "vitest";
import {
  TIER_COMPACT_MAX,
  TIER_HYSTERESIS,
  TIER_WIDE_MIN,
  tierForWidth,
} from "../components/notebook/useNoteLayoutTier";

describe("tierForWidth — 分档", () => {
  it("按阈值分三档", () => {
    expect(tierForWidth(400, null)).toBe("compact");
    expect(tierForWidth(700, null)).toBe("standard");
    expect(tierForWidth(1200, null)).toBe("wide");
  });

  it("阈值本身归上面那一档", () => {
    expect(tierForWidth(TIER_COMPACT_MAX, null)).toBe("standard");
    expect(tierForWidth(TIER_WIDE_MIN, null)).toBe("wide");
  });

  it("阈值下一像素还在下面那一档", () => {
    expect(tierForWidth(TIER_COMPACT_MAX - 1, null)).toBe("compact");
    expect(tierForWidth(TIER_WIDE_MIN - 1, null)).toBe("standard");
  });
});

describe("tierForWidth — 回差", () => {
  it("离开 compact 要多走一个回差", () => {
    // 拖分隔条停在 560 附近时,没有回差就会 compact ⇄ standard 反复横跳,
    // 每跳一次整个面板重排一次。
    expect(tierForWidth(TIER_COMPACT_MAX + 1, "compact")).toBe("compact");
    expect(tierForWidth(TIER_COMPACT_MAX + TIER_HYSTERESIS, "compact")).toBe("standard");
  });

  it("离开 wide 要多走一个回差", () => {
    expect(tierForWidth(TIER_WIDE_MIN - 1, "wide")).toBe("wide");
    expect(tierForWidth(TIER_WIDE_MIN - TIER_HYSTERESIS - 1, "wide")).toBe("standard");
  });

  it("进入某一档不加回差", () => {
    // 回差只在离开当前档时生效 —— 两头都加的话档位边界会跟着来的方向漂。
    expect(tierForWidth(TIER_COMPACT_MAX - 1, "standard")).toBe("compact");
    expect(tierForWidth(TIER_WIDE_MIN, "standard")).toBe("wide");
  });
});

describe("tierForWidth — 量不到宽度", () => {
  it("首帧 0 宽退回 standard", () => {
    // 首帧和 display:none 都会量到 0。猜 standard 是现状的布局,猜错代价最小。
    expect(tierForWidth(0, null)).toBe("standard");
  });

  it("量不到时保持当前档,不要跳回 standard", () => {
    // 面板被折叠(量到 0)再展开,不该顺手把用户所在的档位重置掉。
    expect(tierForWidth(0, "compact")).toBe("compact");
    expect(tierForWidth(Number.NaN, "wide")).toBe("wide");
  });

  it("负宽也当量不到", () => {
    expect(tierForWidth(-10, "compact")).toBe("compact");
  });
});
