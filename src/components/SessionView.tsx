import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Brain,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  FileText,
  Paperclip,
  Wrench,
} from "lucide-react";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { useI18n } from "../i18n";

export interface SessionContent {
  type: "text" | "tool_use" | "tool_result" | "thinking" | "attachment";
  text?: string;
  id?: string;
  name?: string;
  input?: string;
  output?: string;
  thinking?: string;
  mediaType?: string;
  source?: string;
}

export interface SessionMessage {
  role: "user" | "assistant";
  content: SessionContent[];
}

interface SessionMessagePage {
  messages: SessionMessage[];
  nextCursor: number | null;
  hasMore: boolean;
}

export function mergeSessionMessagePages(
  earlier: SessionMessage[],
  later: SessionMessage[],
  isCodex: boolean,
): SessionMessage[] {
  if (!isCodex || earlier.length === 0 || later.length === 0) {
    return [...earlier, ...later];
  }

  const mergedEarlier = [...earlier];
  const mergedLater = [...later];
  const earlierBoundary = mergedEarlier[mergedEarlier.length - 1];
  const laterBoundary = mergedLater[0];
  if (earlierBoundary.role !== laterBoundary.role) return [...mergedEarlier, ...mergedLater];

  if (earlierBoundary.role === "assistant") {
    mergedEarlier[mergedEarlier.length - 1] = {
      ...earlierBoundary,
      content: [...earlierBoundary.content, ...laterBoundary.content],
    };
    mergedLater.shift();
    return [...mergedEarlier, ...mergedLater];
  }

  const earlierText = new Set(
    earlierBoundary.content
      .filter((content) => content.type === "text")
      .map((content) => content.text ?? ""),
  );
  const repeatsCodexEvent = laterBoundary.content.some(
    (content) => content.type === "text" && earlierText.has(content.text ?? ""),
  );
  if (!repeatsCodexEvent) return [...mergedEarlier, ...mergedLater];

  mergedEarlier[mergedEarlier.length - 1] = {
    ...earlierBoundary,
    content: [
      ...earlierBoundary.content,
      ...laterBoundary.content.filter(
        (content) => content.type !== "text" || !earlierText.has(content.text ?? ""),
      ),
    ],
  };
  mergedLater.shift();
  return [...mergedEarlier, ...mergedLater];
}

export function renderSessionMarkdown(text: string): string {
  return DOMPurify.sanitize(marked(text, { async: false }) as string);
}

function ExpandableCard({
  title,
  content,
  icon,
}: {
  title: string;
  content: string;
  icon: ReactNode;
}) {
  const [expanded, setExpanded] = useState(false);
  return (
    <div
      style={{
        margin: "6px 0",
        border: "1px solid var(--border-dim)",
        borderRadius: 8,
        overflow: "hidden",
        fontSize: 12,
        background: "var(--bg-panel)",
      }}
    >
      <button
        type="button"
        onClick={() => setExpanded((value) => !value)}
        style={{
          width: "100%",
          display: "flex",
          alignItems: "center",
          gap: 7,
          padding: "7px 10px",
          background: "var(--bg-input)",
          border: "none",
          cursor: "pointer",
          textAlign: "left",
          color: "var(--text-secondary)",
        }}
      >
        {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        {icon}
        <span style={{ fontFamily: "var(--font-mono)", fontWeight: 650 }}>{title}</span>
      </button>
      {expanded && (
        <pre
          style={{
            margin: 0,
            padding: "10px 12px",
            fontSize: 11,
            fontFamily: "var(--font-mono)",
            color: "var(--text-secondary)",
            background: "var(--bg-root)",
            overflowX: "auto",
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
          }}
        >
          {content}
        </pre>
      )}
    </div>
  );
}

function ThinkingBlock({ thinking }: { thinking: string }) {
  const { t } = useI18n();
  return (
    <ExpandableCard
      title={t("session.thinking")}
      content={thinking}
      icon={<Brain size={12} aria-hidden="true" />}
    />
  );
}

function AttachmentCard({ content }: { content: SessionContent }) {
  const source = content.source ?? "";
  const mediaType = content.mediaType ?? "application/octet-stream";
  const name = content.name || "attachment";
  const canPreviewImage = mediaType.startsWith("image/") && /^(data:|https?:\/\/)/.test(source);

  return (
    <div style={{ margin: "7px 0" }}>
      {canPreviewImage && (
        <img
          src={source}
          alt={name}
          style={{
            display: "block",
            maxWidth: "100%",
            maxHeight: 420,
            marginBottom: 6,
            borderRadius: 8,
            border: "1px solid var(--border-dim)",
            objectFit: "contain",
          }}
        />
      )}
      <ExpandableCard
        title={`${name} · ${mediaType}`}
        content={source}
        icon={<Paperclip size={12} aria-hidden="true" />}
      />
    </div>
  );
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const handleCopy = () => {
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    });
  };
  return (
    <button
      type="button"
      className="copy-btn"
      aria-label="Copy message"
      onClick={handleCopy}
      style={{
        position: "absolute",
        top: 7,
        right: 8,
        opacity: 0,
        transition: "opacity 0.15s",
        background: "none",
        border: "none",
        cursor: "pointer",
        padding: 2,
        color: "var(--text-muted)",
        display: "flex",
        alignItems: "center",
      }}
    >
      {copied ? <Check size={13} /> : <Copy size={13} />}
    </button>
  );
}

function MessageContent({ content }: { content: SessionContent }) {
  switch (content.type) {
    case "text":
      return (
        <div
          className="session-prose"
          dangerouslySetInnerHTML={{ __html: renderSessionMarkdown(content.text ?? "") }}
        />
      );
    case "thinking":
      return <ThinkingBlock thinking={content.thinking ?? ""} />;
    case "tool_use":
      return (
        <ExpandableCard
          title={content.name || "tool"}
          content={content.input ?? ""}
          icon={<Wrench size={12} aria-hidden="true" />}
        />
      );
    case "tool_result":
      return (
        <ExpandableCard
          title={content.id ? `result · ${content.id}` : "tool result"}
          content={content.output ?? ""}
          icon={<FileText size={12} aria-hidden="true" />}
        />
      );
    case "attachment":
      return <AttachmentCard content={content} />;
  }
}

function MessageBubble({ message }: { message: SessionMessage }) {
  const isUser = message.role === "user";
  const copyText = message.content
    .map((content) =>
      content.type === "text"
        ? (content.text ?? "")
        : content.type === "thinking"
          ? (content.thinking ?? "")
          : content.type === "tool_use"
            ? (content.input ?? "")
            : content.type === "tool_result"
              ? (content.output ?? "")
              : (content.source ?? ""),
    )
    .filter(Boolean)
    .join("\n");

  if (message.content.length === 0) return null;
  return (
    <div
      data-session-role={message.role}
      style={{
        marginBottom: 14,
        display: "flex",
        justifyContent: isUser ? "flex-end" : "flex-start",
      }}
    >
      <div
        className={isUser ? "user-message-bubble" : "assistant-message-bubble"}
        style={{
          width: "fit-content",
          maxWidth: isUser ? "78%" : "88%",
          minWidth: 0,
          position: "relative",
          padding: "10px 16px",
          borderRadius: 14,
          border: isUser
            ? "1px solid color-mix(in srgb, var(--accent) 25%, var(--border-dim))"
            : "1px solid var(--border-dim)",
          background: isUser
            ? "color-mix(in srgb, var(--accent) 12%, var(--bg-panel))"
            : "color-mix(in srgb, var(--bg-card) 90%, var(--bg-panel))",
          color: "var(--text-primary)",
          overflow: "hidden",
        }}
        onMouseEnter={(event) => {
          const button = event.currentTarget.querySelector<HTMLElement>(".copy-btn");
          if (button) button.style.opacity = "1";
        }}
        onMouseLeave={(event) => {
          const button = event.currentTarget.querySelector<HTMLElement>(".copy-btn");
          if (button) button.style.opacity = "0";
        }}
      >
        {copyText && <CopyButton text={copyText} />}
        {message.content.map((content, index) => (
          <MessageContent key={`${content.type}-${content.id ?? index}`} content={content} />
        ))}
      </div>
    </div>
  );
}

export function SessionView({
  sessionPath,
  projectPath,
  isCodex,
  fallback,
}: {
  sessionPath: string;
  projectPath: string;
  isCodex: boolean;
  fallback?: ReactNode;
}) {
  const { t } = useI18n();
  const [messages, setMessages] = useState<SessionMessage[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadingEarlier, setLoadingEarlier] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const pendingScrollRestoreRef = useRef<{ height: number; top: number } | null>(null);

  useLayoutEffect(() => {
    const restore = pendingScrollRestoreRef.current;
    const element = scrollRef.current;
    if (!restore || !element) return;
    element.scrollTop = restore.top + (element.scrollHeight - restore.height);
    pendingScrollRestoreRef.current = null;
  }, [messages]);

  useEffect(() => {
    let cancelled = false;
    setMessages([]);
    setLoading(true);
    setLoadingEarlier(false);
    setError(null);

    const load = async () => {
      let cursor: number | null = null;
      let displayedFirstPage = false;
      try {
        for (;;) {
          const page: SessionMessagePage = await invoke<SessionMessagePage>(
            "read_session_message_page",
            {
              sessionPath,
              projectPath,
              isCodex,
              cursor,
            },
          );
          if (cancelled) return;

          if (page.messages.length > 0) {
            if (!displayedFirstPage) {
              displayedFirstPage = true;
              setMessages(page.messages);
              setLoading(false);
              setLoadingEarlier(page.hasMore);
              window.requestAnimationFrame(() => {
                const element = scrollRef.current;
                if (element) element.scrollTop = element.scrollHeight;
              });
            } else {
              const element = scrollRef.current;
              if (element) {
                pendingScrollRestoreRef.current = {
                  height: element.scrollHeight,
                  top: element.scrollTop,
                };
              }
              setMessages((current) => mergeSessionMessagePages(page.messages, current, isCodex));
            }
          }

          if (!page.hasMore || page.nextCursor == null) {
            setLoading(false);
            setLoadingEarlier(false);
            return;
          }
          if (cursor !== null && page.nextCursor >= cursor) {
            throw new Error("Invalid session history cursor");
          }
          cursor = page.nextCursor;
        }
      } catch (caught) {
        if (cancelled) return;
        setError(String(caught));
        setLoading(false);
        setLoadingEarlier(false);
      }
    };

    void load();
    return () => {
      cancelled = true;
    };
  }, [sessionPath, projectPath, isCodex]);

  if (!loading && fallback && (error || messages.length === 0)) {
    return (
      <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
        <div
          role="alert"
          style={{
            flexShrink: 0,
            padding: "8px 14px",
            borderBottom: "1px solid var(--border-dim)",
            background: "color-mix(in srgb, var(--warning) 10%, var(--bg-panel))",
            color: "var(--text-secondary)",
            fontSize: 12,
          }}
        >
          {t("session.terminalFallback", { error: error ?? t("session.noMessages") })}
        </div>
        {fallback}
      </div>
    );
  }

  return (
    <div
      className="terminal-record-pane"
      ref={scrollRef}
      style={{
        flex: 1,
        minHeight: 0,
        overflowY: "auto",
        margin: "14px 16px 16px",
        padding: "20px 26px 32px",
        borderRadius: 22,
        border: "1px solid color-mix(in srgb, var(--border-medium) 72%, #ffffff 28%)",
        background:
          "linear-gradient(180deg, color-mix(in srgb, var(--bg-card) 72%, var(--bg-subtle)), color-mix(in srgb, var(--bg-panel) 78%, var(--bg-subtle)))",
        boxShadow:
          "inset 0 1px 0 color-mix(in srgb, #ffffff 58%, transparent), 0 22px 54px color-mix(in srgb, #111827 13%, transparent), 0 2px 8px color-mix(in srgb, #111827 8%, transparent)",
        backdropFilter: "blur(22px) saturate(1.38)",
        WebkitBackdropFilter: "blur(22px) saturate(1.38)",
      }}
    >
      {(loading || loadingEarlier) && (
        <div style={{ color: "var(--text-hint)", fontSize: 12, padding: "6px 0 12px" }}>
          {loading ? t("session.loading") : t("session.loadingEarlier")}
        </div>
      )}
      {error && !fallback && (
        <div style={{ color: "var(--text-muted)", fontSize: 13, padding: "12px 0" }}>
          {t("session.unableToLoad", { error })}
        </div>
      )}
      {!loading && !error && messages.length === 0 && (
        <div style={{ color: "var(--text-hint)", fontSize: 13, padding: "12px 0" }}>
          {t("session.noMessages")}
        </div>
      )}
      {messages.map((message, index) => (
        <MessageBubble key={index} message={message} />
      ))}
    </div>
  );
}
