#!/usr/bin/env node
// P3-03 的终端测试替身:往 PTY 里写**任意字节**,并且故意在两种地方使坏——
//   1. 把一个多字节序列劈成两次 write(中间留时间差,让内核真的分帧投递);
//   2. 输出非法 UTF-8 与 NUL;
//   3. 在一个字符写一半时立刻退出。
// 它是"原始字节优先"这条验收的对方证人:测试把读到的字节自己记一份 oracle,
// 再和 core 字节流里的内容比 hash。
//
// 模式与旋钮都由环境变量给(后续切片复用同一替身):
//   FAKE_MODE           fidelity(默认)/ nasty / exit / signal / eof / child / orphan / interactive
//   FAKE_STRESS_MIB     fidelity 的输出量(MiB)
//   FAKE_EXIT_CODE      exit / eof / interactive 的退出码(默认 0)
//   FAKE_SIGNAL         signal 模式给自己发的信号名(默认 SIGTERM)
//   FAKE_LIVE_MS        eof 模式关掉 tty fd 之后还要活多久(默认 700)
//
// 不用 process.stdout 的字符串层:那会先把非法字节替换掉,替身就废了。

import { spawn } from 'node:child_process';
import { closeSync } from 'node:fs';
import { createInterface } from 'node:readline';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const envInt = (name, fallback) => {
  const raw = process.env[name];
  if (raw === undefined || raw === '') return fallback;
  const value = Number.parseInt(raw, 10);
  if (!Number.isInteger(value)) throw new Error(`${name} 需要整数,实际 ${raw}`);
  return value;
};

/// 关掉 tty 之后不能再走 process.exit:它会同步 flush stdout,而 fd 已经不在了。
const reallyExit = (code) => process.reallyExit(code);

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

// ── 退出结算切片(§4-3)用的模式 ──────────────────────────────────────────

/// 正常退出 / 指定码退出:替身以 FAKE_EXIT_CODE 结束,退出码是测试要看的事实。
async function exitMode() {
  const code = envInt('FAKE_EXIT_CODE', 0);
  await write(`[fake-agent] exiting code=${code}\n`);
  reallyExit(code);
}

/// 信号致死:默认动作直接终止进程,**不经过** process.exit,所以父侧看到的
/// 状态与"非零退出"不是一回事(这就是 SIGTERM 那条来源要单独有用例的理由)。
async function signalMode() {
  const name = process.env.FAKE_SIGNAL || 'SIGTERM';
  await write(`[fake-agent] signalling self ${name}\n`);
  process.kill(process.pid, name);
  await sleep(5000);
  // 走到这里说明信号没带走进程(名字不可信号或被处理器吃掉了)。
  await write(`[fake-agent] survived ${name}\n`);
  reallyExit(0);
}

/// EOF 早于进程结束:把子进程手里的 tty fd 全关掉,PTY 主端就此读到 EOF,
/// 而进程还活着。用来钉住"reader 收尾不是结算来源"。
async function eofMode() {
  const live = envInt('FAKE_LIVE_MS', 700);
  const code = envInt('FAKE_EXIT_CODE', 0);
  await write(`[fake-agent] before-eof live=${live}ms code=${code}\n`);
  await sleep(150); // 让上面这行确实被读到,别和关闭动作抢顺序
  closeSync(0);
  closeSync(1);
  closeSync(2);
  await sleep(live);
  reallyExit(code);
}

/// 带后代的运行:后代继承同一个进程组且不持有这个 tty(stdio 全 ignore),
/// 所以"杀掉正确的进程组"只有真的把后代一起带走才算数。
async function childMode() {
  const descendant = spawn(
    process.execPath,
    ['-e', 'setInterval(() => {}, 1000)'],
    { stdio: 'ignore' },
  );
  await write(`[fake-agent] self=${process.pid} descendant=${descendant.pid}\n`);
  setInterval(() => {}, 1000);
}

/// 组长先退出、后代**继承**这个 tty 并继续活着:reader 的 EOF 由此被推到很久之后
/// (要等整个组被带走),而退出结算在 EOF 之前就必须已经发生且只有一次。
async function orphanMode() {
  const holder = spawn(
    process.execPath,
    ['-e', 'setInterval(() => {}, 1000)'],
    { stdio: 'inherit' },
  );
  await write(`[fake-agent] leader=${process.pid} holder=${holder.pid}\n`);
  await sleep(envInt('FAKE_LIVE_MS', 150));
  reallyExit(envInt('FAKE_EXIT_CODE', 0));
}

/// 报告尺寸 + 等待输入(§4-4 的 resize/input 两片都要它):每次 SIGWINCH 都重新
/// 报一次,输入按行回显 ack,`quit` 结束。
async function interactiveMode() {
  const size = () => `${process.stdout.rows}x${process.stdout.columns}`;
  await write(`[fake-agent] size=${size()}\n`);
  process.stdout.on('resize', () => {
    void write(`[fake-agent] resized=${size()}\n`);
  });
  const lines = createInterface({ input: process.stdin });
  for await (const line of lines) {
    if (line === 'quit') break;
    await write(`[fake-agent] ack=${line}\n`);
  }
  reallyExit(envInt('FAKE_EXIT_CODE', 0));
}

const mode = process.env.FAKE_MODE || 'fidelity';
if (mode === 'nasty') {
  await nasty();
} else if (mode === 'exit') {
  await exitMode();
} else if (mode === 'signal') {
  await signalMode();
} else if (mode === 'eof') {
  await eofMode();
} else if (mode === 'child') {
  await childMode();
} else if (mode === 'orphan') {
  await orphanMode();
} else if (mode === 'interactive') {
  await interactiveMode();
} else {
  const mib = Number.parseInt(process.env.FAKE_STRESS_MIB || '4', 10);
  const total = await fidelity(mib);
  // 让父进程能从输出里核对量级:每 16 MiB 打一行进度(也计入字节流)。
  await write(`[fake-agent] fidelity wrote ${total} bytes\n`);
}
