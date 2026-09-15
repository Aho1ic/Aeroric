import type React from "react";
import { resolveProjectAvatar } from "../projectAvatar";
import type { ProjectAvatarOverride } from "../types";

/**
 * 项目头像。`avatar` 缺省时按名字自动取色取首字母,
 * 定制项逐字段覆盖 —— 合并规则全在 `resolveProjectAvatar` 里。
 */
export function ProjectAvatar({
  name,
  avatar,
  size = 28,
  style: extraStyle,
}: {
  name: string;
  avatar?: ProjectAvatarOverride;
  size?: number;
  style?: React.CSSProperties;
}) {
  const { gradient, emoji, label, colorKey } = resolveProjectAvatar(name, avatar);
  const [from, to] = gradient;
  return (
    <div
      data-avatar-color={colorKey}
      data-avatar-mode={emoji ? "emoji" : "label"}
      style={{
        width: size,
        height: size,
        borderRadius: Math.round(size * 0.28),
        flexShrink: 0,
        background: `linear-gradient(135deg, ${from}, ${to})`,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        // emoji 自带字形高度,按字母那档缩放会显小。
        fontSize: emoji ? size * 0.52 : size * 0.38,
        fontWeight: emoji ? 400 : 700,
        color: "var(--fg-on-accent)",
        letterSpacing: emoji ? 0 : 0.3,
        lineHeight: 1,
        boxShadow: `0 2px 5px ${from}55`,
        userSelect: "none",
        ...extraStyle,
      }}
    >
      {emoji || label}
    </div>
  );
}
