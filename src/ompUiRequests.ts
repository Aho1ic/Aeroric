import { invoke } from "@tauri-apps/api/core";

/** omp `extension_ui_request` 的前端载荷(由 omp_rpc.rs emit 的 omp-ui-request)。 */
export interface OmpUiRequest {
  taskId: string;
  requestId: string;
  method: "select" | "confirm" | "input" | "editor";
  title?: string | null;
  message?: string | null;
  /** select 方法的选项 label 数组,与 optionDetails 按下标对应。 */
  options?: string[] | null;
  optionDetails?: Array<{ description?: string }> | null;
  placeholder?: string | null;
  prefill?: string | null;
}

/** 回应 omp 的 UI 请求;response 形如 {value}/{confirmed}/{cancelled}。 */
export async function respondOmpUiRequest(
  request: OmpUiRequest,
  response: Record<string, unknown>,
): Promise<void> {
  await invoke("respond_omp_server_request", {
    taskId: request.taskId,
    requestId: request.requestId,
    response,
  });
}

/** 请求方取消(关弹窗/点击遮罩)时回的取消帧。 */
export function cancelledOmpUiResponse(): Record<string, unknown> {
  return { cancelled: true };
}
