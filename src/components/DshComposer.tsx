import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ImagePlus, Send, X } from "lucide-react";
import { useI18n } from "../i18n";
import { DshSlashPalette } from "./DshSlashPalette";
import { Button } from "./ui/Button";

interface DshComposerImage {
  id: string;
  dataUrl: string;
}

export function DshComposer({
  taskId,
  sessionId,
}: {
  taskId: string;
  sessionId?: string;
}) {
  const { t } = useI18n();
  const [text, setText] = useState("");
  const [mode, setMode] = useState<"queue" | "steer">("queue");
  const [images, setImages] = useState<DshComposerImage[]>([]);
  const [slashOpen, setSlashOpen] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const textRef = useRef<HTMLTextAreaElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  if (!sessionId) return null;

  function insertSlash(command: string): boolean {
    const input = textRef.current;
    const start = input?.selectionStart ?? text.length;
    const end = input?.selectionEnd ?? start;
    const insertion = `/${command} `;
    const next = `${text.slice(0, start)}${insertion}${text.slice(end)}`;
    setText(next);
    setSlashOpen(false);
    requestAnimationFrame(() => {
      input?.focus();
      input?.setSelectionRange(start + insertion.length, start + insertion.length);
    });
    return true;
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
    setSending(true);
    setError(null);
    try {
      await invoke("prompt_dsh_task", {
        taskId,
        prompt: text,
        promptMode: mode,
        images: images.map((image) => image.dataUrl),
        clientTimeZone: Intl.DateTimeFormat().resolvedOptions().timeZone,
      });
      setText("");
      setImages([]);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSending(false);
    }
  }

  return (
    <div
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
      {slashOpen && (
        <DshSlashPalette
          editorInsert={insertSlash}
          onDismiss={() => setSlashOpen(false)}
          sessionId={sessionId}
        />
      )}
      {images.length > 0 && (
        <div style={{ display: "flex", gap: 6, flexWrap: "wrap", marginBottom: 6 }}>
          {images.map((image) => (
            <div key={image.id} style={{ position: "relative", width: 42, height: 42 }}>
              <img
                src={image.dataUrl}
                alt=""
                style={{ width: "100%", height: "100%", objectFit: "cover", borderRadius: 4 }}
              />
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
      <div style={{ display: "flex", alignItems: "flex-end", gap: 8 }}>
        <textarea
          ref={textRef}
          value={text}
          disabled={sending}
          rows={2}
          placeholder={t("dsh.composer.placeholder")}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "/" && !text) setSlashOpen(true);
            if (event.key === "Escape" && slashOpen) {
              event.preventDefault();
              setSlashOpen(false);
            } else if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              void submit();
            }
          }}
          style={{
            flex: 1,
            minWidth: 0,
            minHeight: 44,
            maxHeight: 140,
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
        <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
          <div style={{ display: "flex", gap: 4 }}>
            <Button
              type="button"
              variant={mode === "queue" ? "secondary" : "ghost"}
              size="icon-sm"
              title={t("dsh.composer.queue")}
              aria-label={t("dsh.composer.queue")}
              onClick={() => setMode("queue")}
            >
              Q
            </Button>
            <Button
              type="button"
              variant={mode === "steer" ? "secondary" : "ghost"}
              size="icon-sm"
              title={t("dsh.composer.steer")}
              aria-label={t("dsh.composer.steer")}
              onClick={() => setMode("steer")}
            >
              S
            </Button>
          </div>
          <div style={{ display: "flex", gap: 4 }}>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              title={t("dsh.composer.attach")}
              aria-label={t("dsh.composer.attach")}
              onClick={() => fileRef.current?.click()}
            >
              <ImagePlus size={15} />
            </Button>
            <Button
              type="button"
              variant="default"
              size="icon-sm"
              title={t("dsh.composer.send")}
              aria-label={t("dsh.composer.send")}
              disabled={sending || (!text.trim() && images.length === 0)}
              onClick={() => void submit()}
            >
              <Send size={15} />
            </Button>
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
