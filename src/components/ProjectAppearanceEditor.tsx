import { useState } from "react";
import { useI18n } from "../i18n";
import {
  AVATAR_PALETTE,
  AVATAR_PALETTE_KEYS,
  normalizeAvatarOverride,
  resolveProjectAvatar,
} from "../projectAvatar";
import type { ProjectAvatarOverride } from "../types";
import { ProjectAvatar } from "./ProjectAvatar";
import { AnimatedSelectionGroup } from "./ui/AnimatedSelection";

/** 头像内容的三态。互斥语义 —— 走 AnimatedSelectionGroup。 */
type AvatarContentMode = "auto" | "initials" | "emoji";

function initialMode(override?: ProjectAvatarOverride): AvatarContentMode {
  if (override?.emoji) return "emoji";
  if (override?.label) return "initials";
  return "auto";
}

/**
 * 头像定制面板:颜色 + 内容(自动 / 首字母 / emoji)。
 *
 * 自己不落盘,把收敛好的定制项交给 `onSave`(全空时给 `undefined` = 清除定制),
 * 持久化沿用项目那条既有链路。
 */
export function ProjectAppearanceEditor({
  name,
  avatar,
  onSave,
  onCancel,
}: {
  name: string;
  avatar?: ProjectAvatarOverride;
  onSave: (next: ProjectAvatarOverride | undefined) => void;
  onCancel: () => void;
}) {
  const { t } = useI18n();
  const [mode, setMode] = useState<AvatarContentMode>(() => initialMode(avatar));
  // 颜色空串 = 跟随名字自动取色。
  const [colorKey, setColorKey] = useState(() => avatar?.color ?? "");
  const [label, setLabel] = useState(() => avatar?.label ?? "");
  const [emoji, setEmoji] = useState(() => avatar?.emoji ?? "");

  const draft = normalizeAvatarOverride({
    color: colorKey,
    // 未选中的那一档不参与,切回自动就等于没定制过内容。
    label: mode === "initials" ? label : "",
    emoji: mode === "emoji" ? emoji : "",
  });
  const preview = resolveProjectAvatar(name, draft);

  const colorOptions = [
    {
      value: "",
      ariaLabel: t("projectAvatar.colorAuto"),
      title: t("projectAvatar.colorAuto"),
      label: <Swatch gradient={undefined} />,
    },
    ...AVATAR_PALETTE_KEYS.map((key) => ({
      value: key,
      ariaLabel: t(`projectAvatar.color.${key}`),
      title: t(`projectAvatar.color.${key}`),
      label: <Swatch gradient={AVATAR_PALETTE[key]} />,
    })),
  ];

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <ProjectAvatar name={name} avatar={draft} size={44} />
        <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
          {mode === "auto" ? t("projectAvatar.autoHint") : name}
        </span>
      </div>

      <Field label={t("projectAvatar.colorLabel")}>
        <AnimatedSelectionGroup
          value={colorKey}
          options={colorOptions}
          onChange={setColorKey}
          ariaLabel={t("projectAvatar.colorGroup")}
          style={{ flexWrap: "wrap" }}
        />
      </Field>

      <Field label={t("projectAvatar.contentLabel")}>
        <AnimatedSelectionGroup
          value={mode}
          options={[
            { value: "auto", label: t("projectAvatar.modeAuto") },
            { value: "initials", label: t("projectAvatar.modeInitials") },
            { value: "emoji", label: t("projectAvatar.modeEmoji") },
          ]}
          onChange={setMode}
          ariaLabel={t("projectAvatar.contentGroup")}
          equalWidth
        />
      </Field>

      {mode === "initials" && (
        <input
          aria-label={t("projectAvatar.initialsInput")}
          placeholder={t("projectAvatar.initialsPlaceholder")}
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          style={inputStyle}
        />
      )}
      {mode === "emoji" && (
        <input
          aria-label={t("projectAvatar.emojiInput")}
          placeholder={t("projectAvatar.emojiPlaceholder")}
          value={emoji}
          onChange={(e) => setEmoji(e.target.value)}
          style={inputStyle}
        />
      )}

      <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
        <button
          type="button"
          onClick={() => {
            setMode("auto");
            setColorKey("");
            setLabel("");
            setEmoji("");
          }}
        >
          {t("projectAvatar.reset")}
        </button>
        <button type="button" onClick={onCancel}>
          {t("projectAvatar.cancel")}
        </button>
        <button type="button" onClick={() => onSave(draft)} data-preview-label={preview.label}>
          {t("projectAvatar.save")}
        </button>
      </div>
    </div>
  );
}

const inputStyle = {
  padding: "6px 8px",
  borderRadius: 6,
  border: "1px solid var(--border)",
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  fontSize: 13,
} as const;

/**
 * 分组标题。刻意用 `div` 而不是 `label`:`label` 包一组按钮时,标题文字会被算进
 * 第一个按钮的 accessible name(「Auto」变成「Content Avatar content」),
 * 读屏用户听到的是拼接后的噪声。分组本身的名字由 `AnimatedSelectionGroup` 的
 * `ariaLabel` 提供,这里只做视觉标题。
 */
function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <span
        aria-hidden="true"
        style={{ fontSize: 11, color: "var(--text-muted)", letterSpacing: 0.4 }}
      >
        {label}
      </span>
      {children}
    </div>
  );
}

/** 一个色块。`gradient` 缺席 = 自动档,画成中性底。 */
function Swatch({ gradient }: { gradient?: readonly [string, string] }) {
  return (
    <span
      aria-hidden="true"
      style={{
        display: "block",
        width: 16,
        height: 16,
        borderRadius: 5,
        background: gradient
          ? `linear-gradient(135deg, ${gradient[0]}, ${gradient[1]})`
          : "var(--bg-subtle)",
        border: gradient ? "none" : "1px dashed var(--border)",
      }}
    />
  );
}
