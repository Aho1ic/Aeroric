/**
 * Durable image attachments carried by DeepSeek Harness content blocks.
 *
 * The Harness stores message images once and puts only a reference in the
 * transcript (`{ type: "image", attachment }`), so history replay needs the
 * reference shape and the display rules here, and the bytes on demand from
 * `session.attachment`. Keeping the parsing and the fit math pure makes both
 * testable without a live session.
 */

/** Raster types the Harness accepts and stores (`imageMediaTypeSchema`). */
export const DSH_IMAGE_MEDIA_TYPES = [
  "image/png",
  "image/jpeg",
  "image/webp",
  "image/gif",
] as const;

export type DshImageMediaType = (typeof DSH_IMAGE_MEDIA_TYPES)[number];

/** Immutable bytes plus intrinsic display metadata owned by the Harness. */
export interface DshImageAttachmentRef {
  attachmentId: string;
  mediaType: DshImageMediaType;
  bytes: number;
  width: number;
  height: number;
  name?: string;
}

function positiveInt(value: unknown): number | undefined {
  return typeof value === "number" && Number.isInteger(value) && value > 0 ? value : undefined;
}

function mediaType(value: unknown): DshImageMediaType | undefined {
  return DSH_IMAGE_MEDIA_TYPES.find((candidate) => candidate === value);
}

/**
 * Read one durable image reference, mirroring `imageAttachmentRefSchema`.
 *
 * A reference missing its intrinsic size cannot be laid out without a reflow
 * once the bytes land, and one with an unknown media type cannot be decoded, so
 * both are rejected here instead of rendering a broken frame.
 */
export function parseDshImageAttachment(value: unknown): DshImageAttachmentRef | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const item = value as Record<string, unknown>;
  const attachmentId = typeof item.attachmentId === "string" ? item.attachmentId : "";
  const type = mediaType(item.mediaType);
  const bytes = positiveInt(item.bytes);
  const width = positiveInt(item.width);
  const height = positiveInt(item.height);
  if (!attachmentId || !type || !bytes || !width || !height) return undefined;
  const name = typeof item.name === "string" && item.name ? item.name : undefined;
  return { attachmentId, mediaType: type, bytes, width, height, ...(name ? { name } : {}) };
}

/**
 * Collect the image references of one message content array, in block order.
 *
 * `ImageBlock` is deliberately role-neutral in the Harness, so user prompts,
 * assistant messages, and the image-returning tools (`read_image`) all reach
 * this same walk.
 */
export function collectDshImageAttachments(content: unknown): DshImageAttachmentRef[] {
  if (!Array.isArray(content)) return [];
  const images: DshImageAttachmentRef[] = [];
  for (const block of content) {
    if (typeof block !== "object" || block === null) continue;
    const item = block as Record<string, unknown>;
    if (item.type !== "image") continue;
    const attachment = parseDshImageAttachment(item.attachment);
    if (attachment) images.push(attachment);
  }
  return images;
}

/** Bounded display box of a thumbnail plus its crop anchor. */
export interface DshImageFit {
  width: number;
  height: number;
  objectPosition: string;
}

/**
 * Display box for a lone image (the Harness' DeepSeek Chat rule): long edge
 * 240px with the rendered aspect ratio clamped to [0.25, 4] — the overflow is
 * cropped by `object-fit: cover` — and never upscaled past the image's natural
 * size. The crop anchor keeps the top of very tall images and the left of very
 * wide ones, where the informative content usually starts.
 */
export function dshImageFit(attachment: DshImageAttachmentRef): DshImageFit {
  const natural = attachment.width / attachment.height;
  const ratio = Math.min(4, Math.max(0.25, natural));
  const box =
    ratio >= 1 ? { width: 240, height: 240 / ratio } : { width: 240 * ratio, height: 240 };
  const scale = Math.min(1, attachment.width / box.width, attachment.height / box.height);
  return {
    width: Math.max(1, Math.round(box.width * scale)),
    height: Math.max(1, Math.round(box.height * scale)),
    objectPosition: natural < 0.25 ? "center top" : natural > 4 ? "left center" : "center",
  };
}
