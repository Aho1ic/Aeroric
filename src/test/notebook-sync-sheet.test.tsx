/* 冲突面板。
 *
 * 这是唯一能逐个文件做决定的地方 —— 在它之前只有 vault 级的一律处理,对一个文件选「用对面
 * 那份」会把这一轮**所有**冲突文件的本地改动一起丢掉。所以这里盯的是"会丢内容"那一类错:
 *
 *   1. 三档状态(还没决定 / 已决定 / 决定过但文件又变了)必须分开显示。
 *   2. fork 要先把落点摊开让人看见,确认才提交 —— 它是要新建一个文件。
 *   3. 挂起和失败也要列出来,否则"文件太大不同步"这类结果用户永远看不到。
 *
 * 文案用 `key|var=value`,断言钉的是 key 和变量值 —— 改文案不该让这些测试变红。
 */

import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { NoteSyncSheet } from "../components/notebook/NoteSyncSheet";
import type { StoredResolution, SyncReport } from "../components/notebook/noteSync";
import type { SyncRemoteView } from "../components/notebook/useNoteSync";

const t = (key: string, vars?: Record<string, string>) =>
  vars
    ? `${key}|${Object.entries(vars)
        .map(([name, value]) => `${name}=${value}`)
        .join(",")}`
    : key;

const NOW = Date.parse("2026-09-01T12:00:00.000Z");

const remote = (over: Partial<SyncRemoteView["target"]> = {}): SyncRemoteView => ({
  target: {
    id: "r1",
    kind: "cloud",
    root: "/dav/notes",
    connectionId: "c1",
    lastSyncAt: NOW - 60_000,
    seq: 3,
    autoSync: true,
    ...over,
  },
  status: {
    remoteId: over.id ?? "r1",
    autoSync: over.autoSync ?? true,
    failures: 0,
    dirty: false,
    lastAttemptMs: NOW - 60_000,
    nextRunInMs: 30_000,
  },
});

const report = (): SyncReport => ({
  plan: {
    actions: [
      {
        path: "notes/a.md",
        reason: "both_modified",
        action: { kind: "conflict", resolution: null, localHash: "111", remoteHash: "222" },
      },
    ],
    summary: { upload: 1, download: 2, deleteRemote: 0, deleteLocal: 0, conflict: 1 },
  },
  outcomes: [
    {
      path: "notes/a.md",
      reason: "both_modified",
      status: { kind: "pending", detail: "awaiting_user" },
    },
  ],
  tombstonesWritten: 0,
  seq: null,
});

const decision = (over: Partial<StoredResolution> = {}): StoredResolution => ({
  path: "notes/a.md",
  resolution: { kind: "keepLocal" },
  localHash: "111",
  remoteHash: "222",
  decidedAt: NOW,
  ...over,
});

const renderSheet = (props: Partial<React.ComponentProps<typeof NoteSyncSheet>> = {}) => {
  const handlers = {
    onSelectRemote: vi.fn(),
    onToggleAuto: vi.fn(),
    onSync: vi.fn(),
    onDecide: vi.fn(),
    onUndecide: vi.fn(),
    onClose: vi.fn(),
  };
  render(
    <NoteSyncSheet
      remotes={[remote()]}
      activeId="r1"
      report={report()}
      stale={false}
      decided={[]}
      running={false}
      error={null}
      {...handlers}
      t={t}
      {...props}
    />,
  );
  return handlers;
};

const row = () => screen.getByTestId("note-sync-conflict-row");

describe("没有远端", () => {
  it("说这个库还没绑云盘,而不是显示一张空清单", () => {
    renderSheet({ remotes: [], activeId: null, report: null });
    expect(screen.getByText("notebook.sync.noRemote")).toBeInTheDocument();
    expect(screen.queryByTestId("note-sync-conflict-row")).toBeNull();
  });

  it("没有远端时不显示自动同步开关", () => {
    renderSheet({ remotes: [], activeId: null, report: null });
    expect(screen.queryByRole("checkbox")).toBeNull();
  });
});

describe("还没跑过", () => {
  it("提示去刷新,而不是说「没有冲突」", () => {
    // 「没有冲突」是一个结论,而这时候什么都还没查过 —— 那句话是在替用户下一个没有依据的判断。
    renderSheet({ report: null });
    expect(screen.getByText("notebook.sync.noReport")).toBeInTheDocument();
    expect(screen.queryByText("notebook.sync.conflictsEmpty")).toBeNull();
  });
});

describe("冲突行的三档", () => {
  it("还没决定:三个选择都在,没有撤回", () => {
    renderSheet();
    const scope = within(row());
    expect(scope.getByText("notebook.sync.keepLocal")).toBeInTheDocument();
    expect(scope.getByText("notebook.sync.keepRemote")).toBeInTheDocument();
    expect(scope.getByText("notebook.sync.fork")).toBeInTheDocument();
    expect(scope.queryByText("notebook.sync.undo")).toBeNull();
  });

  it("已决定:显示决定和撤回,不再摆三个按钮", () => {
    renderSheet({ decided: [decision()] });
    const scope = within(row());
    expect(
      scope.getByText("notebook.sync.decided|choice=notebook.sync.keepLocal"),
    ).toBeInTheDocument();
    expect(scope.getByText("notebook.sync.undo")).toBeInTheDocument();
    // 摆着三个按钮会让人以为上次没点上。
    expect(scope.queryByText("notebook.sync.keepRemote")).toBeNull();
  });

  it("决定过但文件又变了:说清楚不再适用,并且还得能重新选", () => {
    // 两侧 hash 只要有一侧对不上就算过期 —— 口径和后端 `diff::decided_for` 一致。
    renderSheet({ decided: [decision({ remoteHash: "999" })] });
    const scope = within(row());
    expect(
      scope.getByText("notebook.sync.stale|choice=notebook.sync.keepLocal"),
    ).toBeInTheDocument();
    /* 这一档必须还能选:混进"已决定"的话用户会一直等一个永远不执行的决定。 */
    expect(scope.getByText("notebook.sync.keepLocal")).toBeInTheDocument();
    expect(scope.getByText("notebook.sync.keepRemote")).toBeInTheDocument();
  });

  it("本地那侧变了也算过期", () => {
    renderSheet({ decided: [decision({ localHash: "888" })] });
    expect(
      within(row()).getByText("notebook.sync.stale|choice=notebook.sync.keepLocal"),
    ).toBeInTheDocument();
  });

  it("别的路径的决定不影响这一行", () => {
    renderSheet({ decided: [decision({ path: "notes/other.md" })] });
    const scope = within(row());
    expect(scope.queryByText(/notebook\.sync\.decided/)).toBeNull();
    expect(scope.getByText("notebook.sync.keepLocal")).toBeInTheDocument();
  });

  it("显示路径和冲突原因", () => {
    renderSheet();
    const scope = within(row());
    expect(scope.getByText("notes/a.md")).toBeInTheDocument();
    expect(scope.getByText("notebook.sync.reason.bothModified")).toBeInTheDocument();
  });
});

describe("做决定", () => {
  it("留我这份", async () => {
    const handlers = renderSheet();
    await userEvent.click(within(row()).getByText("notebook.sync.keepLocal"));
    expect(handlers.onDecide).toHaveBeenCalledWith("notes/a.md", { kind: "keepLocal" });
  });

  it("用对面那份", async () => {
    const handlers = renderSheet();
    await userEvent.click(within(row()).getByText("notebook.sync.keepRemote"));
    expect(handlers.onDecide).toHaveBeenCalledWith("notes/a.md", { kind: "keepRemote" });
  });

  it("撤回", async () => {
    const handlers = renderSheet({ decided: [decision()] });
    await userEvent.click(within(row()).getByText("notebook.sync.undo"));
    expect(handlers.onUndecide).toHaveBeenCalledWith("notes/a.md");
  });
});

describe("两份都留", () => {
  it("点一下只是摊开落点,不提交", async () => {
    const handlers = renderSheet();
    await userEvent.click(within(row()).getByText("notebook.sync.fork"));
    /* fork 要新建一个文件。点一下就发生的话用户没有机会看见它落在哪。 */
    expect(handlers.onDecide).not.toHaveBeenCalled();
    expect(screen.getByLabelText("notebook.sync.forkPath")).toBeInTheDocument();
  });

  it("默认落点拼在扩展名之前", async () => {
    renderSheet();
    await userEvent.click(within(row()).getByText("notebook.sync.fork"));
    /* 拼成 `a.md.conflict` 的话那份内容在列表、搜索、反链里全都不出现 —— 随手记只索引 `.md`,
       而用户选 fork 的意思正是"这份我要留着看"。 */
    expect(screen.getByLabelText("notebook.sync.forkPath")).toHaveValue("notes/a.conflict.md");
  });

  it("确认之后才提交", async () => {
    const handlers = renderSheet();
    await userEvent.click(within(row()).getByText("notebook.sync.fork"));
    await userEvent.click(screen.getByText("notebook.sync.forkConfirm"));
    expect(handlers.onDecide).toHaveBeenCalledWith("notes/a.md", {
      kind: "fork",
      forkPath: "notes/a.conflict.md",
    });
  });

  it("改过的落点按改后的提交", async () => {
    const handlers = renderSheet();
    await userEvent.click(within(row()).getByText("notebook.sync.fork"));
    const input = screen.getByLabelText("notebook.sync.forkPath");
    await userEvent.clear(input);
    await userEvent.type(input, "keep/theirs.md");
    await userEvent.click(screen.getByText("notebook.sync.forkConfirm"));
    expect(handlers.onDecide).toHaveBeenCalledWith("notes/a.md", {
      kind: "fork",
      forkPath: "keep/theirs.md",
    });
  });

  it("空路径时确认按钮是禁的", async () => {
    const handlers = renderSheet();
    await userEvent.click(within(row()).getByText("notebook.sync.fork"));
    await userEvent.clear(screen.getByLabelText("notebook.sync.forkPath"));
    /* 后端会拒(`Fork path cannot be empty`),但那要等提交之后才报。一个点了没反应的按钮
       比一句报错更让人困惑。 */
    expect(screen.getByText("notebook.sync.forkConfirm")).toBeDisabled();
    await userEvent.click(screen.getByText("notebook.sync.forkConfirm"));
    expect(handlers.onDecide).not.toHaveBeenCalled();
  });

  it("只有空白也算空", async () => {
    renderSheet();
    await userEvent.click(within(row()).getByText("notebook.sync.fork"));
    const input = screen.getByLabelText("notebook.sync.forkPath");
    await userEvent.clear(input);
    await userEvent.type(input, "   ");
    expect(screen.getByText("notebook.sync.forkConfirm")).toBeDisabled();
  });

  it("再点一下收起来", async () => {
    renderSheet();
    const fork = within(row()).getByText("notebook.sync.fork");
    await userEvent.click(fork);
    expect(screen.getByLabelText("notebook.sync.forkPath")).toBeInTheDocument();
    await userEvent.click(fork);
    expect(screen.queryByLabelText("notebook.sync.forkPath")).toBeNull();
  });
});

describe("报告过期", () => {
  it("说清楚下面这张清单可能已经不是现在的情况", () => {
    renderSheet({ stale: true });
    expect(screen.getByText("notebook.sync.staleReport")).toBeInTheDocument();
  });

  it("给一个重新检查的入口", async () => {
    const handlers = renderSheet({ stale: true });
    await userEvent.click(screen.getByText("notebook.sync.recheck"));
    expect(handlers.onSync).toHaveBeenCalledTimes(1);
  });

  it("不过期就不出现", () => {
    renderSheet();
    expect(screen.queryByText("notebook.sync.staleReport")).toBeNull();
  });
});

describe("挂起和失败", () => {
  it("非冲突的挂起也要列出来", () => {
    const withOversize = report();
    withOversize.outcomes.push({
      path: "big.bin",
      reason: "upload",
      status: { kind: "pending", detail: "oversize_not_hashable" },
    });
    renderSheet({ report: withOversize });
    /* 不列的话用户会一直等一个永远不会同步的文件,而界面上没有任何线索。 */
    expect(screen.getByText("big.bin")).toBeInTheDocument();
    expect(screen.getByText("notebook.sync.pending.oversizeNotHashable")).toBeInTheDocument();
  });

  it("失败显示后端那句原文", () => {
    const withFailure = report();
    withFailure.outcomes.push({
      path: "b.md",
      reason: "upload",
      status: { kind: "failed", error: "remote refused: 507" },
    });
    renderSheet({ report: withFailure });
    // 后端的错误原文不该被一句通用文案盖掉 —— 那是排查时唯一有信息的部分。
    expect(screen.getByText("remote refused: 507")).toBeInTheDocument();
  });

  it("冲突不会在「其他结果」里重复出现一遍", () => {
    renderSheet();
    expect(screen.queryByText("notebook.sync.otherOutcomes")).toBeNull();
  });

  it("落定的动作不列", () => {
    const withDone = report();
    withDone.outcomes.push({ path: "done.md", reason: "upload", status: { kind: "done" } });
    renderSheet({ report: withDone });
    expect(screen.queryByText("done.md")).toBeNull();
  });
});

describe("头部", () => {
  it("一个远端时不给选择器", () => {
    renderSheet();
    expect(screen.queryByRole("combobox")).toBeNull();
  });

  it("多个远端时给选择器", () => {
    renderSheet({
      remotes: [remote(), remote({ id: "r2", root: "/dav/other" })],
    });
    expect(screen.getByRole("combobox", { name: "notebook.sync.remote" })).toBeInTheDocument();
  });

  it("自动同步开关反映目标上的值", async () => {
    const handlers = renderSheet({ remotes: [remote({ autoSync: false })] });
    const box = screen.getByRole("checkbox");
    expect(box).not.toBeChecked();
    await userEvent.click(box);
    expect(handlers.onToggleAuto).toHaveBeenCalledWith(true);
  });

  it("计划摘要把五个数字都摊出来", () => {
    renderSheet();
    expect(
      screen.getByText(
        "notebook.sync.summary|upload=1,download=2,deleteRemote=0,deleteLocal=0,conflict=1",
      ),
    ).toBeInTheDocument();
  });

  it("正在跑时禁掉刷新", () => {
    renderSheet({ running: true });
    expect(screen.getByRole("button", { name: "notebook.sync.syncNow" })).toBeDisabled();
  });

  it("报错显示出来", () => {
    renderSheet({ error: "cannot open remote" });
    expect(screen.getByText("cannot open remote")).toBeInTheDocument();
  });

  it("关闭", async () => {
    const handlers = renderSheet();
    await userEvent.click(screen.getByRole("button", { name: "notebook.sync.close" }));
    expect(handlers.onClose).toHaveBeenCalledTimes(1);
  });

  it("Esc 关闭", async () => {
    const handlers = renderSheet();
    await userEvent.keyboard("{Escape}");
    expect(handlers.onClose).toHaveBeenCalledTimes(1);
  });
});
