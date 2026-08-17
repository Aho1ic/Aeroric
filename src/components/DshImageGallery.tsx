import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { useI18n } from "../i18n";
import { dshImageFit, type DshImageAttachmentRef } from "../dshImageAttachments";
import type { DshImageLoader } from "../hooks/useDshImageLoader";

/**
 * Document-level original-image preview opened by clicking a thumbnail.
 *
 * Closes on Escape, backdrop press, or the close control, and restores focus to
 * the opener on unmount. Rendered through a body portal: an opener inside a
 * transformed or filtered ancestor — or inside the insights dialog — would
 * otherwise trap the fixed backdrop in that ancestor's box instead of covering
 * the viewport.
 */
export function DshImageLightbox({
  src,
  alt,
  onClose,
}: {
  src: string;
  alt: string;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const closeRef = useRef<HTMLButtonElement | null>(null);
  const restoreRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    restoreRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    closeRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      restoreRef.current?.focus();
    };
  }, [onClose]);

  return createPortal(
    <div
      className="dsh-image-backdrop"
      role="dialog"
      aria-modal="true"
      aria-label={t("dsh.image.preview")}
    >
      {/* A separate mask layer, not a background on the backdrop: a
          backdrop-filter there would blur the previewed image too. */}
      <div className="dsh-image-mask" aria-hidden="true" onMouseDown={onClose} />
      <img className="dsh-image-original" src={src} alt={alt} />
      <button
        ref={closeRef}
        type="button"
        className="dsh-image-close"
        aria-label={t("dsh.image.closePreview")}
        onClick={onClose}
      >
        <X size={16} />
      </button>
    </div>,
    document.body,
  );
}

/**
 * Compact history renderer with retryable loading and click-to-open original
 * preview. A lone image renders at its `dshImageFit` size; an image among
 * several renders as a fixed 64px square tile.
 */
export function DshMessageImage({
  attachment,
  load,
  variant,
}: {
  attachment: DshImageAttachmentRef;
  load: DshImageLoader;
  variant: "single" | "tile";
}) {
  const { t } = useI18n();
  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState(false);
  const [open, setOpen] = useState(false);
  // Retry re-arms the one load effect below, so every attempt — first load or
  // retry — runs under the same liveness guard and the same reset.
  const [attempt, setAttempt] = useState(0);
  const close = useCallback(() => setOpen(false), []);
  const fit = useMemo(
    () => (variant === "single" ? dshImageFit(attachment) : undefined),
    [attachment, variant],
  );

  useEffect(() => {
    let live = true;
    setError(false);
    setSrc(null);
    void load(attachment).then(
      (url) => {
        if (live) setSrc(url);
      },
      () => {
        if (live) setError(true);
      },
    );
    return () => {
      live = false;
    };
  }, [attachment, load, attempt]);

  const label = attachment.name ?? t("dsh.image.label");
  if (error) {
    return (
      <button
        type="button"
        className="dsh-image-error"
        data-variant={variant}
        onClick={() => setAttempt((value) => value + 1)}
      >
        {t("dsh.image.loadFailed")}
      </button>
    );
  }
  return (
    <>
      <button
        type="button"
        className="dsh-image-frame"
        data-variant={variant}
        style={fit === undefined ? undefined : { width: fit.width, height: fit.height }}
        title={t("dsh.image.openOriginal")}
        aria-label={t("dsh.image.openOriginalLabel", { label })}
        onClick={() => {
          if (src !== null) setOpen(true);
        }}
      >
        {src === null ? (
          <span className="dsh-image-loading">{t("dsh.image.loading")}</span>
        ) : (
          <img
            src={src}
            alt={label}
            style={fit === undefined ? undefined : { objectPosition: fit.objectPosition }}
          />
        )}
      </button>
      {open && src !== null && <DshImageLightbox src={src} alt={label} onClose={close} />}
    </>
  );
}

/**
 * Wrapping image group shared by every history role: a lone image renders
 * large, several render as 64px square tiles (the Harness' DeepSeek Chat rule).
 */
export function DshImageGallery({
  images,
  load,
  align,
}: {
  images: readonly DshImageAttachmentRef[];
  load: DshImageLoader;
  align: "start" | "end";
}) {
  if (images.length === 0) return null;
  const variant = images.length === 1 ? "single" : "tile";
  return (
    <div className="dsh-image-gallery" data-align={align}>
      {images.map((attachment, index) => (
        <DshMessageImage
          key={`${attachment.attachmentId}:${index}`}
          attachment={attachment}
          load={load}
          variant={variant}
        />
      ))}
    </div>
  );
}
