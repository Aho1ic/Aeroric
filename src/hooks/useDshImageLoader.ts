import { useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { parseDshImageAttachment, type DshImageAttachmentRef } from "../dshImageAttachments";

/** Resolves a durable image reference to a URL an `<img>` can render. */
export type DshImageLoader = (attachment: DshImageAttachmentRef) => Promise<string>;

interface DshImageScope {
  /** Bumped when the scope is released; a late load compares against it. */
  generation: number;
  /** One in-flight-or-settled URL promise per attachment id. */
  cache: Map<string, Promise<string>>;
  /** Object URLs this scope minted and therefore has to revoke. */
  urls: Set<string>;
}

function decodeBase64(data: string): Uint8Array<ArrayBuffer> {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

function releaseImageUrl(url: string): void {
  if (!url.startsWith("blob:")) return;
  if (typeof URL.revokeObjectURL === "function") URL.revokeObjectURL(url);
}

/**
 * Fetch one attachment's bytes and hand back a renderable URL.
 *
 * `session.attachment` answers `{ attachment, data }` with base64 `data`. An
 * object URL is preferred so the decoded bytes stay out of the DOM (a large
 * data URL is re-parsed on every render and shows up in every DOM snapshot);
 * a host without the factory falls back to the base64 the Harness already sent.
 */
async function loadDshImageUrl(
  sessionId: string,
  attachment: DshImageAttachmentRef,
): Promise<string> {
  const value = await invoke<unknown>("get_dsh_session_attachment", {
    sessionId,
    attachmentId: attachment.attachmentId,
  });
  const record =
    typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {};
  const data = typeof record.data === "string" ? record.data : "";
  if (!data) throw new Error(`session.attachment returned no bytes for ${attachment.attachmentId}`);
  // The stored reference wins over the block's copy: the block was written when
  // the message was sent, the response is what the attachment service holds now.
  const mediaType = parseDshImageAttachment(record.attachment)?.mediaType ?? attachment.mediaType;
  if (typeof URL.createObjectURL !== "function") return `data:${mediaType};base64,${data}`;
  return URL.createObjectURL(new Blob([decodeBase64(data)], { type: mediaType }));
}

/**
 * Session-scoped loader for historical images, cached per attachment.
 *
 * Every URL minted for a session is revoked when the session scope changes or
 * the owner unmounts, and a load that settles after the release revokes its own
 * URL instead of handing a dangling one to a thumbnail that outlived the scope.
 */
export function useDshImageLoader(sessionId: string): DshImageLoader {
  const scopeRef = useRef<DshImageScope>({ generation: 0, cache: new Map(), urls: new Set() });

  useEffect(() => {
    const scope = scopeRef.current;
    return () => {
      // Invalidate before revoking so an in-flight load cannot re-register its
      // URL into the set this cleanup just emptied.
      scope.generation += 1;
      scope.cache.clear();
      for (const url of scope.urls) releaseImageUrl(url);
      scope.urls.clear();
    };
  }, [sessionId]);

  return useCallback(
    (attachment: DshImageAttachmentRef) => {
      const scope = scopeRef.current;
      const cached = scope.cache.get(attachment.attachmentId);
      if (cached) return cached;
      const generation = scope.generation;
      const pending = loadDshImageUrl(sessionId, attachment).then((url) => {
        if (scope.generation !== generation) {
          releaseImageUrl(url);
          throw new Error("dsh image scope was released before the load completed");
        }
        scope.urls.add(url);
        return url;
      });
      // A failed load must not be cached, or the retry control could never get
      // past the first failure.
      pending.catch(() => {
        if (scope.cache.get(attachment.attachmentId) === pending) {
          scope.cache.delete(attachment.attachmentId);
        }
      });
      scope.cache.set(attachment.attachmentId, pending);
      return pending;
    },
    [sessionId],
  );
}
