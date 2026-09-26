#!/usr/bin/env node
// P3-03 的终端测试替身:往 PTY 里写**任意字节**,并且故意在两种地方使坏——
//   1. 把一个多字节序列劈成两次 write(中间留时间差,让内核真的分帧投递);
//   2. 输出非法 UTF-8 与 NUL;
//   3. 在一个字符写一半时立刻退出。
// 它是"原始字节优先"这条验收的对方证人:测试把读到的字节自己记一份 oracle,
// 再和 core 字节流里的内容比 hash。
//
// 模式由环境变量给(留给后续切片复用同一替身):
//   FAKE_MODE      fidelity(默认)/ nasty
//   FAKE_STRESS_MIB fidelity 的输出量(MiB)
//
// 不用 process.stdout 的字符串层:那会先把非法字节替换掉,替身就废了。

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function write(chunk) {
  const buf = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
  if (!process.stdout.write(buf)) {
    await new Promise((resolve) => process.stdout.once('drain', resolve));
  }
}

async function fidelity(mib) {
  // 一个可重算的循环图案。块长取**图案周期的整数倍**,于是拼起来的字节流永远
  // 是合法 UTF-8;而内核的 tty 输出队列会把投递切成远小于块长的帧(且不按周期
  // 对齐),跨帧劈开多字节序列因此是必然发生而不是靠运气。
  const pattern = Buffer.from('A中文😀b测试B🚀\u0007', 'utf8'); // 24 字节
  const blockLen = pattern.length * 683; // 16392,非 2 的幂:帧边界必然落在字符中间
  const block = Buffer.allocUnsafe(blockLen);
  for (let i = 0; i < blockLen; i += 1) {
    block[i] = pattern[i % pattern.length];
  }
  const target = mib * 1024 * 1024;
  let written = 0;
  while (written < target) {
    await write(block);
    written += blockLen;
  }
  return written;
}

async function nasty() {
  const emoji = Buffer.from('😀', 'utf8'); // F0 9F 98 80
  await write('\u001b[31m中文');
  await sleep(120);
  await write(emoji.subarray(0, 2));
  await sleep(120);
  await write(emoji.subarray(2, 4));
  await write(Buffer.from([0x00, 0x01, 0x00]));
  await write(Buffer.from([0xc0, 0x80, 0xff, 0xfe]));
  await write('\u001b[0m');
  await sleep(120);
  // 半个字符 + 立刻退出:旧实现会把这 3 个字节静默丢掉。
  await write(emoji.subarray(0, 3));
  process.exit(0);
}

const mode = process.env.FAKE_MODE || 'fidelity';
if (mode === 'nasty') {
  await nasty();
} else {
  const mib = Number.parseInt(process.env.FAKE_STRESS_MIB || '4', 10);
  const total = await fidelity(mib);
  // 让父进程能从输出里核对量级:每 16 MiB 打一行进度(也计入字节流)。
  await write(`[fake-agent] fidelity wrote ${total} bytes\n`);
}
