/**
 * 终端原始输出的环形缓冲。
 *
 * 终端历史有两个消费者:`useTerminalManager` 里跨任务保存的缓存,以及
 * `TerminalView` 为主题重建与快照恢复保留的本地镜像。两者要在"能重放完整历史"
 * 与"不能无上限吃内存"之间取同一个折中,所以实现只留一份。
 *
 * 超过 `TERMINAL_BUFFER_MAX_BYTES` 就从头丢弃,并把丢掉的长度累计进
 * `droppedLen`,使调用方持有的**绝对**偏移在裁剪后依然可用 —— 否则每次裁剪都会
 * 让"已渲染到哪里"的记录整体错位,重放就会重复或漏掉一段输出。
 */

export const TERMINAL_BUFFER_MAX_BYTES = 10 * 1024 * 1024; // 每个终端 10MB 上限
export const TERMINAL_BUFFER_MAX_CHUNKS = 256; // 超过就合并,避免数组本身过长

export interface TerminalRingBuffer {
  chunks: string[];
  totalLen: number;
  droppedLen: number;
}

export function createTerminalRingBuffer(): TerminalRingBuffer {
  return { chunks: [], totalLen: 0, droppedLen: 0 };
}

export function pushTerminalChunk(buffer: TerminalRingBuffer, data: string): void {
  buffer.chunks.push(data);
  buffer.totalLen += data.length;
  // 保留至少一块:单块本身就超上限时(例如挂载时一次性灌入的整段历史),丢掉它会把
  // 缓冲清空,连"还能重放的部分"都没了。留着它,下一块到达时它就会被正常挤出去。
  while (buffer.totalLen > TERMINAL_BUFFER_MAX_BYTES && buffer.chunks.length > 1) {
    const dropped = buffer.chunks.shift()!;
    buffer.totalLen -= dropped.length;
    buffer.droppedLen += dropped.length;
  }
  if (buffer.chunks.length > TERMINAL_BUFFER_MAX_CHUNKS) {
    const merged = buffer.chunks.join("");
    buffer.chunks.length = 0;
    buffer.chunks.push(merged);
  }
}

/** 终端开启至今写入的总长度,含已被裁掉的部分。 */
export function terminalBufferAbsLength(buffer: TerminalRingBuffer): number {
  return buffer.totalLen + buffer.droppedLen;
}

export function joinTerminalBuffer(buffer: TerminalRingBuffer): string {
  return buffer.chunks.join("");
}

/**
 * 取绝对偏移 `absOffset` 之后的内容。偏移落在已被裁掉的区间时返回现存的全部内容
 * —— 那段历史已经不在内存里,只能从还留着的地方接着重放。
 */
export function joinTerminalBufferFrom(buffer: TerminalRingBuffer, absOffset: number): string {
  const relOffset = absOffset - buffer.droppedLen;
  if (relOffset <= 0) return joinTerminalBuffer(buffer);
  let cum = 0;
  for (let i = 0; i < buffer.chunks.length; i++) {
    const len = buffer.chunks[i].length;
    if (cum + len > relOffset) {
      const parts = buffer.chunks.slice(i);
      parts[0] = parts[0].slice(relOffset - cum);
      return parts.join("");
    }
    cum += len;
  }
  return "";
}
