export const MAX_TASK_IMAGE_COUNT = 8;
export const MAX_TASK_IMAGE_BYTES = 10 * 1024 * 1024;
export const MAX_TASK_IMAGE_TOTAL_BYTES = 32 * 1024 * 1024;

const TASK_IMAGE_MIME_TYPES = new Set(["image/png", "image/jpeg", "image/gif", "image/webp"]);

export interface TaskImageAttachment {
  id: string;
  dataUrl: string;
  byteSize?: number;
  mimeType?: string;
}

export type TaskImageRejection = "unsupported" | "too-large" | "too-many" | "total-too-large";

export interface TaskImageCandidate {
  size: number;
  type: string;
}

export function estimatedDataUrlBytes(dataUrl: string): number {
  const comma = dataUrl.indexOf(",");
  if (comma < 0) return dataUrl.length;
  const payloadLength = dataUrl.length - comma - 1;
  return Math.floor((payloadLength * 3) / 4);
}

export function attachmentBytes(image: TaskImageAttachment): number {
  return image.byteSize ?? estimatedDataUrlBytes(image.dataUrl);
}

export function selectTaskImageCandidates<T extends TaskImageCandidate>(
  existing: TaskImageAttachment[],
  candidates: T[],
): { accepted: T[]; rejections: Set<TaskImageRejection> } {
  const accepted: T[] = [];
  const rejections = new Set<TaskImageRejection>();
  let count = existing.length;
  let totalBytes = existing.reduce((sum, image) => sum + attachmentBytes(image), 0);

  for (const candidate of candidates) {
    if (!TASK_IMAGE_MIME_TYPES.has(candidate.type.toLowerCase())) {
      rejections.add("unsupported");
      continue;
    }
    if (candidate.size > MAX_TASK_IMAGE_BYTES) {
      rejections.add("too-large");
      continue;
    }
    if (count >= MAX_TASK_IMAGE_COUNT) {
      rejections.add("too-many");
      continue;
    }
    if (totalBytes + candidate.size > MAX_TASK_IMAGE_TOTAL_BYTES) {
      rejections.add("total-too-large");
      continue;
    }
    accepted.push(candidate);
    count += 1;
    totalBytes += candidate.size;
  }

  return { accepted, rejections };
}
