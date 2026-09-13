/* 导出文件名里的日期必须是**本地**日期。
 *
 * `new Date().toISOString().slice(0, 10)` 给的是 UTC 日期:UTC+8 在早上 08:00 之前,
 * 它报的是昨天,于是上午导出的 `aeroric-<slug>-<date>.md` 带着前一天的日期。这个坑在
 * 这个仓库里已经踩过两次(`noteTaskInbox.todayIso`、`weeklyReport.localYmd` 都为它留了
 * 整段注释),所以除了钉住行为,这里再加一条守卫禁止这个写法回到 `src/` 下。
 */
import fs from "node:fs";
import path from "node:path";
import { invoke } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n";
import { ToastProvider } from "../components/Toast";
import { RunningView } from "../components/RunningView";
import type { Task } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("../components/TerminalView", () => ({
  TerminalView: () => <div data-testid="terminal-view" />,
}));
vi.mock("../components/SessionView", () => ({
  SessionView: () => <div data-testid="session-view" />,
}));
vi.mock("../hooks/useUsageSnapshot", () => ({ useUsageSnapshot: () => ({ snapshot: null }) }));

const SESSION_PATH = "/Users/test/.codex/sessions/2026/08/28/rollout-2026-08-28T00-30-00.jsonl";

const exportableTask: Task = {
  id: "task-1",
  projectId: "project-1",
  prompt: "morning export",
  name: "morning export",
  agent: "codex",
  permissionMode: "ask",
  status: "done",
  createdAt: 1,
  codexSessionPath: SESSION_PATH,
};

function renderRunningView() {
  return render(
    <I18nProvider>
      <ToastProvider>
        <RunningView
          task={exportableTask}
          projectPath="/tmp/project"
          onCancel={vi.fn()}
          onResume={vi.fn()}
          onReconnect={vi.fn()}
          onMarkDone={vi.fn()}
          onInput={vi.fn()}
          onResize={vi.fn()}
          onRegisterTerminal={vi.fn(() => 1)}
          onTerminalReady={vi.fn()}
          onRename={vi.fn()}
          onGenerateName={vi.fn().mockResolvedValue(undefined)}
          themeVariant="light"
          terminalFontSize={11}
          monoFontFamily="JetBrains Mono"
        />
      </ToastProvider>
    </I18nProvider>,
  );
}

describe("导出文件名的日期", () => {
  /* 时区必须钉死,不能靠跑测试那台机器的 TZ:CI 容器一般是 UTC,而在 UTC 下
     `toISOString()` 与本地日期恰好相同 —— 那样这条测试在 CI 上永远是绿的,测不出东西。
     Node 会在 `process.env.TZ` 变化后重建时区缓存,所以这里改完再造 Date 即可。 */
  const originalTz = process.env.TZ;

  beforeAll(() => {
    process.env.TZ = "Asia/Shanghai";
  });

  afterAll(() => {
    process.env.TZ = originalTz;
  });

  beforeEach(() => {
    localStorage.setItem("aeroric:language", "en");
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue({});
    vi.mocked(saveDialog).mockReset();
    vi.mocked(saveDialog).mockResolvedValue(null);
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
  });

  it("UTC+8 早 8 点前导出,文件名带的是本地今天而不是 UTC 的昨天", async () => {
    /* 本地 2026-08-28 04:30(UTC+8)= UTC 2026-08-27T20:30Z。
       用 UTC 日期会写成 2026-08-27 —— 用户在 8 月 28 日上午导出,拿到一个写着 27 日的文件。
       只 fake Date:setTimeout / rAF 留给 userEvent 与 React 用,免得点击卡住。 */
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      vi.setSystemTime(new Date("2026-08-27T20:30:00.000Z"));
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
      renderRunningView();

      await user.click(screen.getByRole("button", { name: "Export as Markdown" }));

      const dialogArgs = vi.mocked(saveDialog).mock.calls[0][0];
      expect(dialogArgs?.defaultPath).toBe("aeroric-morning_export-2026-08-28.md");
    } finally {
      vi.useRealTimers();
    }
  });

  it("本地日期与 UTC 同一天时也是这一天", async () => {
    // 一条对照:下午导出时两种算法结果相同,证明上一条钉住的是时区差而不是随便一个偏移。
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      vi.setSystemTime(new Date("2026-08-28T06:00:00.000Z")); // 本地 14:00
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
      renderRunningView();

      await user.click(screen.getByRole("button", { name: "Export as Markdown" }));

      const dialogArgs = vi.mocked(saveDialog).mock.calls[0][0];
      expect(dialogArgs?.defaultPath).toBe("aeroric-morning_export-2026-08-28.md");
    } finally {
      vi.useRealTimers();
    }
  });
});

/* 守卫:`toISOString()` 取日期段的写法不许再出现在 `src/` 下。
 *
 * 形状照 `src-tauri/src/command_registration_tests.rs`:读源码做静态比对。为什么值得机制化
 * —— 这不是编译期错误、也不是任何单测会碰到的分支,它只在「本地日期与 UTC 日期不同的那
 * 几个小时」里错,而 CI 通常跑在 UTC 上,天然看不见。已经踩过两次了。
 */
const SRC_DIR = path.resolve(process.cwd(), "src");

/**
 * 允许的例外。键是相对 `src/` 的路径,值是理由。
 *
 * 真的需要 UTC 日期时(比如要发给一个按 UTC 归档的外部系统)把文件加进来并写清理由,
 * 而不是把这条守卫删掉。当前为空:仓库里所有「取今天」都该是本地日期。
 */
const UTC_DATE_ALLOWLIST: Record<string, string> = {};

/** 去掉块注释与行注释:讲这个坑的注释本身会命中正则(`noteTaskInbox.ts` 就有整段)。 */
function withoutComments(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/[^\n]*/g, "");
}

function collectSourceFiles(dir: string, acc: string[] = []): string[] {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    // 跳过 test:守卫测试自己就要写出这个模式来说明禁的是什么。
    if (entry.name === "test" || entry.name === "assets") continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) collectSourceFiles(full, acc);
    else if (/\.tsx?$/.test(entry.name)) acc.push(full);
  }
  return acc;
}

describe("UTC 取日期的守卫", () => {
  it("`src/` 下没有 toISOString() 截日期段的写法", () => {
    /* 三种等价写法都拦:`.slice(0, 10)` / `.substring(0, 10)` / `.split("T")[0]`。
       允许 `toISOString()` 单独出现 —— 完整时间戳里它是对的(`dshSessionFeatures.ts` 存
       `scheduledAt` 就是),错的只是从它身上截出「哪一天」。 */
    const offenders: string[] = [];
    const pattern =
      /toISOString\(\)\s*\.\s*(?:slice|substring)\(\s*0\s*,\s*10\s*\)|toISOString\(\)\s*\.\s*split\(\s*["'`]T["'`]\s*\)/;

    for (const file of collectSourceFiles(SRC_DIR)) {
      const relative = path.relative(SRC_DIR, file);
      if (relative in UTC_DATE_ALLOWLIST) continue;
      if (pattern.test(withoutComments(fs.readFileSync(file, "utf8")))) offenders.push(relative);
    }

    expect(offenders).toEqual([]);
  });

  it("守卫的正则确实能认出这些写法", () => {
    // 守卫本身要有测试:正则写错(比如被注释剥离顺手吃掉)会让上一条永远绿。
    const pattern =
      /toISOString\(\)\s*\.\s*(?:slice|substring)\(\s*0\s*,\s*10\s*\)|toISOString\(\)\s*\.\s*split\(\s*["'`]T["'`]\s*\)/;

    expect(pattern.test("const d = new Date().toISOString().slice(0, 10);")).toBe(true);
    expect(pattern.test("const d = now.toISOString().substring(0, 10);")).toBe(true);
    expect(pattern.test('const d = now.toISOString().split("T")[0];')).toBe(true);
    // 完整时间戳不该被拦。
    expect(pattern.test("current.scheduledAt = new Date(next).toISOString();")).toBe(false);
    // 注释里的示例会被 withoutComments 先剥掉。
    expect(withoutComments("// 不要用 toISOString().slice(0, 10)\ncode();")).not.toMatch(pattern);
  });
});
