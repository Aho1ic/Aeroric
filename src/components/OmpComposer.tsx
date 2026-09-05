import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ImagePlus, Send, X } from "lucide-react";
import { useI18n } from "../i18n";
import { Button } from "./ui/Button";

interface OmpComposerImage {
  id: string;
  dataUrl: string;
}

/**
 * omp(rpc-ui) 会话的输入框:提交即 `prompt_omp_task`(`prompt` RPC 命令)。
 * 与 DshComposer 不同,omp 不需要 promptMode/触发菜单;完成与否由
 * `agent_end.isTerminal` 决定,这里只负责投递输入。
 */
export function OmpComposer({ taskId }: { taskId: string }) {
  const { t } = useI18n();
  const [text, setText] = useState("");
  const [images, setImages] = useState<OmpComposerImage[]>([]);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

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
      await invoke("prompt_omp_task", {
        taskId,
        prompt: text,
        images: images.map((image) => image.dataUrl),
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
      data-composer-card
      style={{
        position: "relative",
        flexShrink: 0,
        borderTop: "1px solid var(--border-dim)",
        padding: "8px 12px 10px",
        background: "var(--bg-card)",
      }}
    >
      {images.length > 0 && (
        <div style={{ display: "flex", gap: 6, flexWrap: "wrap", padding: "4px 2px 8px" }}>
          {images.map((image) => (
            <span
              key={image.id}
              style={{
                position: "relative",
                display: "inline-flex",
                alignItems: "center",
                gap: 4,
                padding: "2px 22px 2px 6px",
                borderRadius: 6,
                background: "var(--bg-hover)",
                fontSize: 11,
                color: "var(--text-secondary)",
              }}
            >
              <img
                src={image.dataUrl}
                style={{ width: 22, height: 22, borderRadius: 4, objectFit: "cover" }}
              />
              {t("omp.composer.imageAttached")}
              <button
                type="button"
                onClick={() => setImages((prev) => prev.filter((item) => item.id !== image.id))}
                style={{
                  position: "absolute",
                  right: 4,
                  top: "50%",
                  transform: "translateY(-50%)",
                  cursor: "pointer",
                  background: "none",
                  border: "none",
                  color: "var(--text-hint)",
                  display: "inline-flex",
                }}
                aria-label={t("omp.composer.removeImage")}
              >
                <X size={11} />
              </button>
            </span>
          ))}
        </div>
      )}
      <div style={{ display: "flex", gap: 8, alignItems: "flex-end" }}>
        <textarea
          value={text}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault();
              void submit();
            }
          }}
          placeholder={t("omp.composer.placeholder")}
          rows={2}
          style={{
            flex: 1,
            resize: "none",
            border: "1px solid var(--border-dim)",
            borderRadius: 8,
            background: "var(--bg-input, transparent)",
            color: "var(--text-primary)",
            padding: "8px 10px",
            fontSize: 12.5,
            lineHeight: 1.45,
            fontFamily: "inherit",
          }}
        />
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <input
            ref={fileRef}
            type="file"
            accept="image/*"
            multiple
            hidden
            onChange={(event) => {
              if (event.target.files) readFiles(event.target.files);
              event.target.value = "";
            }}
          />
          <Button variant="outline" size="sm" onClick={() => fileRef.current?.click()}>
            <ImagePlus size={13} />
          </Button>
          <Button
            variant="default"
            size="sm"
            onClick={() => void submit()}
            disabled={sending || (!text.trim() && images.length === 0)}
          >
            <Send size={13} />
          </Button>
        </div>
      </div>
      {error && <div style={{ marginTop: 6, fontSize: 12, color: "var(--danger)" }}>{error}</div>}
    </div>
  );
}
