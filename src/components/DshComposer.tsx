import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ImagePlus, Send, X } from "lucide-react";
import { useI18n } from "../i18n";
import { requestsDshSessionLogExport } from "../dshSessionLogExport";
import { replaceDshTriggerToken, type DshTokenSpan } from "../dshInputTriggers";
import type { DshSlashCommand } from "../dshSlashCommands";
import { useDshTriggerMenu } from "../hooks/useDshTriggerMenu";
import { useDshTriggerSources } from "../hooks/useDshTriggerSources";
import { DshImageLightbox } from "./DshImageGallery";
import { DshSlashPicker } from "./DshSlashPalette";
import { DshTriggerMenu } from "./DshTriggerMenu";
import { useDshSessionLogExport } from "./DshSessionLogExport";
import { Button } from "./ui/Button";
import { Select } from "./ui/Primitives";

interface DshComposerImage {
  id: string;
  dataUrl: string;
}

export function DshComposer({ taskId, sessionId }: { taskId: string; sessionId?: string }) {
  const { t } = useI18n();
  const [text, setText] = useState("");
  const [mode, setMode] = useState<"queue" | "steer">("queue");
  const [images, setImages] = useState<DshComposerImage[]>([]);
  const [preview, setPreview] = useState<string | null>(null);
  const [picker, setPicker] = useState<DshSlashCommand | null>(null);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const textRef = useRef<HTMLTextAreaElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  // Every draft mutation bumps this; a pick whose span was taken against an
  // older revision is dropped instead of splicing over moved offsets.
  const draftRevRef = useRef(0);
  const { exportLog } = useDshSessionLogExport(sessionId ?? "");
  const sources = useDshTriggerSources(sessionId ?? "", setPicker);
  const menu = useDshTriggerMenu(sources, applyToken);
  const previewed = images.find((image) => image.id === preview);

  function commitDraft(next: string, caret?: number) {
    draftRevRef.current += 1;
    setText(next);
    if (caret === undefined) return;
    const input = textRef.current;
    // The caret is restored after React has written the new value, otherwise
    // the browser parks it at the end of the replaced text.
    requestAnimationFrame(() => {
      input?.focus();
      input?.setSelectionRange(caret, caret);
    });
  }

  /** Replace the trigger token under the caret with the picked literal. */
  function applyToken(replacement: string, span: DshTokenSpan) {
    const draft = textRef.current?.value ?? text;
    const next = replaceDshTriggerToken(draft, span, replacement, draftRevRef.current);
    if (next === null) return;
    commitDraft(next.text, next.caret);
  }

  /** Splice literal text at the caret (the popup command's chosen argument). */
  function insertAtCaret(insertion: string) {
    const input = textRef.current;
    const draft = input?.value ?? text;
    const start = input?.selectionStart ?? draft.length;
    const end = input?.selectionEnd ?? start;
    commitDraft(
      `${draft.slice(0, start)}${insertion}${draft.slice(end)}`,
      start + insertion.length,
    );
  }

  /**
   * Re-detect the trigger token under the caret after a draft change.
   *
   * Only draft mutations re-detect, as in the Harness: a bare caret gesture must
   * not resurrect a menu the user just dismissed with Escape.
   */
  function track(draft: string, caret: number | null) {
    // No `claimed` tier here: Aeroric's draft is free text end to end, so a line
    // that merely parses as a command must not silence the completion menu.
    menu.track(
      draft,
      caret ?? draft.length,
      { tier: sending ? "frozen" : "plain" },
      draftRevRef.current,
    );
  }

  function readFiles(files: FileList | File[]) {
    for (const file of Array.from(files)) {
      if (!file.type.startsWith("image/")) continue;
      const reader = new FileReader();
      const id = `${Date.now()}-${Math.random()}`;
      reader.onload = () => {
        const dataUrl = typeof reader.result === "string" ? reader.result : "";
        if (dataUrl) setImages((prev) => [...prev, { id, dataUrl }]);
      };
      reader.readAsDataURL(file);
    }
  }

  async function submit() {
    if (sending || (!text.trim() && images.length === 0)) return;
    const line = text;
    menu.dismiss();
    setPicker(null);
    setSending(true);
    setError(null);
    try {
      const result = await invoke<unknown>("prompt_dsh_task", {
        taskId,
        prompt: text,
        promptMode: mode,
        images: images.map((image) => image.dataUrl),
        clientTimeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      });
      commitDraft("");
      setImages([]);
      // `/export` is answered by the Harness with a success command result and
      // leaves the archive to the client, exactly as its Web half starts the
      // download on `command/executed`.
      if (requestsDshSessionLogExport(line, result)) await exportLog();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSending(false);
    }
  }

  if (!sessionId) return null;

  return (
    <div
      data-composer-card
      style={{
        position: "relative",
        flexShrink: 0,
        borderTop: "1px solid var(--border-dim)",
        padding: "8px 12px 10px",
        background: "var(--bg-card)",
      }}
      onPaste={(event) => {
        const files = Array.from(event.clipboardData.items)
          .map((item) => item.getAsFile())
          .filter((file): file is File => file !== null);
        if (files.length > 0) {
          event.preventDefault();
          readFiles(files);
        }
      }}
    >
      {picker ? (
        <DshSlashPicker
          command={picker}
          keyboardTargetRef={textRef}
          onPick={(arg) => {
            setPicker(null);
            // The command token already landed, so only the argument is spliced
            // in — at the caret the token replacement left it at.
            insertAtCaret(`${arg} `);
          }}
          onBack={() => setPicker(null)}
          onDismiss={() => setPicker(null)}
        />
      ) : (
        <DshTriggerMenu
          state={menu.state}
          sources={sources}
          onPick={menu.pick}
          onDismiss={menu.dismiss}
        />
      )}
      {images.length > 0 && (
        <div style={{ display: "flex", gap: 6, flexWrap: "wrap", marginBottom: 6 }}>
          {images.map((image) => (
            <div key={image.id} style={{ position: "relative", width: 42, height: 42 }}>
              <button
                type="button"
                title={t("dsh.image.openOriginal")}
                aria-label={t("dsh.image.openOriginalLabel", { label: t("dsh.image.label") })}
                onClick={() => setPreview(image.id)}
                style={{
                  display: "block",
                  width: "100%",
                  height: "100%",
                  padding: 0,
                  border: "none",
                  borderRadius: 4,
                  overflow: "hidden",
                  background: "transparent",
                  cursor: "zoom-in",
                }}
              >
                <img
                  src={image.dataUrl}
                  alt=""
                  style={{ width: "100%", height: "100%", objectFit: "cover" }}
                />
              </button>
              <button
                type="button"
                aria-label={t("dsh.composer.removeImage")}
                title={t("dsh.composer.removeImage")}
                onClick={() => setImages((prev) => prev.filter((item) => item.id !== image.id))}
                style={{
                  position: "absolute",
                  top: -5,
                  right: -5,
                  width: 16,
                  height: 16,
                  padding: 0,
                  border: "1px solid var(--border-medium)",
                  borderRadius: "50%",
                  background: "var(--bg-card)",
                  color: "var(--text-secondary)",
                  display: "grid",
                  placeItems: "center",
                  cursor: "pointer",
                }}
              >
                <X size={10} />
              </button>
            </div>
          ))}
        </div>
      )}
      {/* Deriving the preview from the live draft rather than copying the URL
          into state closes the lightbox by itself when its image is removed. */}
      {previewed && (
        <DshImageLightbox
          src={previewed.dataUrl}
          alt={t("dsh.image.original")}
          onClose={() => setPreview(null)}
        />
      )}
      <div style={{ display: "flex", alignItems: "flex-end", gap: 8 }}>
        <textarea
          ref={textRef}
          value={text}
          disabled={sending}
          rows={3}
          placeholder={t("dsh.composer.placeholder")}
          onChange={(event) => {
            commitDraft(event.target.value);
            track(event.target.value, event.target.selectionStart);
          }}
          onKeyDown={(event) => {
            // Shift+Enter is the native newline unconditionally, decided before
            // the IME guard so a composition-closing Shift+Enter still breaks.
            if (event.key === "Enter" && event.shiftKey) return;
            const composing = event.nativeEvent.isComposing;
            if (event.key === "ArrowUp" || event.key === "ArrowDown") {
              const key = event.key === "ArrowUp" ? "up" : "down";
              if (menu.arbitrate(key, composing) === "consumed") event.preventDefault();
              return;
            }
            if (event.key === "Escape") {
              if (picker !== null) {
                event.preventDefault();
                setPicker(null);
                return;
              }
              if (menu.arbitrate("escape", composing) === "consumed") event.preventDefault();
              return;
            }
            if (event.key !== "Enter" || composing) return;
            // An open menu with a highlight completes the token; a menu without
            // one passes Enter down to the send.
            const arbitrated = menu.arbitrate("enter", composing);
            event.preventDefault();
            if (arbitrated !== "pass") return;
            void submit();
          }}
          style={{
            flex: 1,
            minWidth: 0,
            // 三行起步:一条提示词很少是一行,输入框太扁会让每次输入都先滚动一次。
            minHeight: 72,
            maxHeight: 200,
            resize: "vertical",
            padding: "8px 10px",
            border: "1px solid var(--border-medium)",
            borderRadius: 6,
            background: "var(--bg-input)",
            color: "var(--text-primary)",
            font: "inherit",
            lineHeight: 1.4,
            outline: "none",
          }}
        />
        {/* Two stacked rows, ordered the way the submission reads: pick how it
            lands, then attach to it and send it. The mode is a select rather than
            two pressed buttons — it is one exclusive choice, and its label stays
            visible because whether a submission interrupts the turn is not
            readable from a glyph. It takes the full width of the stack so it is
            not the odd one out beside two square glyph buttons. `Button` drops
            children at every `icon-*` size, so those glyphs come from the `icon`
            prop; send is a size up because it is the one action of the row. */}
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <Select
            value={mode}
            disabled={sending}
            aria-label={t("dsh.composer.mode")}
            title={t(mode === "queue" ? "dsh.composer.queue" : "dsh.composer.steer")}
            onChange={(event) => setMode(event.target.value === "steer" ? "steer" : "queue")}
            style={{ width: "100%", minHeight: 30, padding: "0 6px", fontSize: 12 }}
          >
            <option value="queue">{t("dsh.composer.queueLabel")}</option>
            <option value="steer">{t("dsh.composer.steerLabel")}</option>
          </Select>
          <div
            style={{ display: "flex", alignItems: "center", justifyContent: "flex-end", gap: 6 }}
          >
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              icon={ImagePlus}
              title={t("dsh.composer.attach")}
              aria-label={t("dsh.composer.attach")}
              onClick={() => fileRef.current?.click()}
            />
            <Button
              type="button"
              variant="default"
              size="icon-lg"
              icon={Send}
              title={t("dsh.composer.send")}
              aria-label={t("dsh.composer.send")}
              disabled={sending || (!text.trim() && images.length === 0)}
              onClick={() => void submit()}
            />
          </div>
        </div>
      </div>
      <input
        ref={fileRef}
        type="file"
        accept="image/png,image/jpeg,image/webp,image/gif"
        multiple
        hidden
        onChange={(event) => {
          if (event.target.files) readFiles(event.target.files);
          event.currentTarget.value = "";
        }}
      />
      {error && (
        <div role="alert" style={{ marginTop: 5, fontSize: 11, color: "var(--danger)" }}>
          {error}
        </div>
      )}
    </div>
  );
}
