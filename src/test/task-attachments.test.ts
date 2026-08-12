import { describe, expect, it } from "vitest";
import {
  MAX_TASK_IMAGE_BYTES,
  MAX_TASK_IMAGE_COUNT,
  MAX_TASK_IMAGE_TOTAL_BYTES,
  selectTaskImageCandidates,
  type TaskImageAttachment,
} from "../taskAttachments";

function attachment(byteSize: number): TaskImageAttachment {
  return { id: `${byteSize}`, dataUrl: "data:image/png;base64,AA==", byteSize };
}

describe("selectTaskImageCandidates", () => {
  it("只接受后端支持的图片格式", () => {
    const result = selectTaskImageCandidates(
      [],
      [
        { type: "image/png", size: 10 },
        { type: "image/svg+xml", size: 10 },
      ],
    );
    expect(result.accepted).toEqual([{ type: "image/png", size: 10 }]);
    expect(result.rejections).toEqual(new Set(["unsupported"]));
  });

  it("同时限制单图、数量和累计大小", () => {
    const existing = Array.from({ length: MAX_TASK_IMAGE_COUNT - 1 }, () => attachment(1));
    const result = selectTaskImageCandidates(existing, [
      { type: "image/jpeg", size: MAX_TASK_IMAGE_BYTES + 1 },
      { type: "image/webp", size: 1 },
      { type: "image/gif", size: 1 },
    ]);
    expect(result.accepted).toHaveLength(1);
    expect(result.rejections).toEqual(new Set(["too-large", "too-many"]));

    const total = selectTaskImageCandidates(
      [attachment(MAX_TASK_IMAGE_TOTAL_BYTES)],
      [{ type: "image/png", size: 1 }],
    );
    expect(total.accepted).toEqual([]);
    expect(total.rejections).toEqual(new Set(["total-too-large"]));
  });
});
