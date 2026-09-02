import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BranchBar } from "../components/task-panel/BranchBar";
import { I18nProvider } from "../i18n";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

type Branch = { name: string; current: boolean; remote: string | null };

const local = (name: string, current = false): Branch => ({ name, current, remote: null });
const remote = (name: string, origin = "origin"): Branch => ({
  name,
  current: false,
  remote: origin,
});

/** 只回 `git_list_branches`,其余命令按需在用例里另行接管。 */
function mockBranches(branches: Branch[]) {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "git_list_branches") return Promise.resolve(branches);
    return Promise.resolve(undefined);
  });
}

function renderBar(props: { projectPath?: string; active?: boolean } = {}) {
  return render(
    <I18nProvider>
      <BranchBar projectPath={props.projectPath ?? "/tmp/repo"} active={props.active} />
    </I18nProvider>,
  );
}

function callsTo(command: string) {
  return vi.mocked(invoke).mock.calls.filter(([c]) => c === command);
}

/** 分支名渲染在触发器里,等它出现即等到首次 `git_list_branches` 落地。 */
function waitForBranch(name: string) {
  return waitFor(() => expect(screen.getByText(name)).toBeInTheDocument());
}

/**
 * 轮询相关的用例必须**先**装假时钟再 render:`setInterval` 在 effect 里建立,
 * render 之后才 `useFakeTimers()` 的话拿到的是真时钟上的 timer,
 * `advanceTimersByTime` 永远推不动它 —— 于是"没有多打 IPC"这类断言会无条件通过。
 */
async function renderWithFakeTimers(props: { active?: boolean } = {}) {
  vi.useFakeTimers();
  const result = renderBar(props);
  // 冲掉首次 fetch 的 microtask,让 inflightRef 归位。
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  return result;
}

async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

/** 加载完成 → 点开分支选择器,回来时搜索框已在 DOM 里。 */
async function openPicker(branches: Branch[]) {
  mockBranches(branches);
  renderBar();
  const current = branches.find((b) => b.current);
  await waitForBranch(current ? current.name : "detached HEAD");
  fireEvent.click(screen.getByTitle("Switch branch"));
  return screen.findByPlaceholderText("Switch to branch…");
}

describe("BranchBar", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe("加载与渲染", () => {
    it("显示当前分支名", async () => {
      mockBranches([local("main", true), local("dev")]);
      renderBar();
      await waitForBranch("main");
    });

    it("没有任何分支时整条不渲染(不是渲染一个空壳)", async () => {
      mockBranches([]);
      const { container } = renderBar();
      await waitFor(() => expect(callsTo("git_list_branches")).toHaveLength(1));
      expect(container).toBeEmptyDOMElement();
    });

    it("git 报错时不渲染,也不把异常抛出去", async () => {
      // 不是 git 仓库 / 没装 git 时后端会 reject。组件吞掉它,分支列表留空。
      vi.mocked(invoke).mockRejectedValue(new Error("not a git repository"));
      const { container } = renderBar();
      await waitFor(() => expect(callsTo("git_list_branches")).toHaveLength(1));
      expect(container).toBeEmptyDOMElement();
    });

    it("有分支但没有一个是 current 时显示 detached HEAD", async () => {
      // 真实状态:checkout 到某个 commit 之后,git 报的分支列表里没有 current。
      mockBranches([local("main"), local("dev")]);
      renderBar();
      await waitForBranch("detached HEAD");
    });

    it("active=false 时不拉分支(后台项目不该打 IPC)", async () => {
      mockBranches([local("main", true)]);
      renderBar({ active: false });
      // 给 effect 一次机会跑完再断言"没发生"。
      await act(async () => {
        await Promise.resolve();
      });
      expect(callsTo("git_list_branches")).toHaveLength(0);
    });

    it("传的 projectPath 会带进请求", async () => {
      mockBranches([local("main", true)]);
      renderBar({ projectPath: "/somewhere/else" });
      await waitForBranch("main");
      expect(callsTo("git_list_branches")[0]?.[1]).toEqual({ projectPath: "/somewhere/else" });
    });
  });

  describe("刷新触发源", () => {
    it("窗口获焦时刷新(外部改过分支要跟上)", async () => {
      mockBranches([local("main", true)]);
      renderBar();
      await waitForBranch("main");
      const before = callsTo("git_list_branches").length;

      mockBranches([local("main"), local("feature", true)]);
      await act(async () => {
        window.dispatchEvent(new Event("focus"));
        await Promise.resolve();
      });

      expect(callsTo("git_list_branches").length).toBeGreaterThan(before);
      await waitForBranch("feature");
    });

    it("10 秒轮询兜底", async () => {
      mockBranches([local("main", true)]);
      await renderWithFakeTimers();
      expect(callsTo("git_list_branches")).toHaveLength(1);

      // 不到点不该动。
      await advance(9_000);
      expect(callsTo("git_list_branches")).toHaveLength(1);

      await advance(1_000);
      expect(callsTo("git_list_branches")).toHaveLength(2);

      // 是 interval 不是 timeout,下一轮还会来。
      await advance(10_000);
      expect(callsTo("git_list_branches")).toHaveLength(3);
    });

    it("active=false 时既不轮询也不听 focus", async () => {
      mockBranches([local("main", true)]);
      await renderWithFakeTimers({ active: false });
      await advance(60_000);
      await act(async () => {
        window.dispatchEvent(new Event("focus"));
        await Promise.resolve();
      });
      expect(callsTo("git_list_branches")).toHaveLength(0);
    });

    it("卸载后不再轮询,也不再听 focus", async () => {
      mockBranches([local("main", true)]);
      const { unmount } = await renderWithFakeTimers();
      // 先确认这个时钟真能推动轮询,否则下面的"没有增加"是空断言。
      await advance(10_000);
      expect(callsTo("git_list_branches")).toHaveLength(2);

      unmount();
      await advance(30_000);
      await act(async () => {
        window.dispatchEvent(new Event("focus"));
        await Promise.resolve();
      });

      expect(callsTo("git_list_branches")).toHaveLength(2);
    });

    it("多个触发源同时来只打一次 IPC(inflight 复用)", async () => {
      // 这是组件里 inflightRef 的目的:focus + 轮询 + 切换同时到,后端 git 命令
      // 并发会堵住 Tokio worker。
      let release: ((value: Branch[]) => void) | undefined;
      vi.mocked(invoke).mockImplementation((command) => {
        if (command === "git_list_branches") {
          return new Promise<Branch[]>((resolve) => {
            release = resolve;
          });
        }
        return Promise.resolve(undefined);
      });

      renderBar();
      await waitFor(() => expect(callsTo("git_list_branches")).toHaveLength(1));

      // 首个请求还没落地就连发 3 次 focus。
      await act(async () => {
        window.dispatchEvent(new Event("focus"));
        window.dispatchEvent(new Event("focus"));
        window.dispatchEvent(new Event("focus"));
        await Promise.resolve();
      });
      expect(callsTo("git_list_branches")).toHaveLength(1);

      // 落地之后 inflight 清空,后续触发能再打出去。
      await act(async () => {
        release?.([local("main", true)]);
        await Promise.resolve();
      });
      await waitForBranch("main");
      await act(async () => {
        window.dispatchEvent(new Event("focus"));
        await Promise.resolve();
      });
      expect(callsTo("git_list_branches").length).toBeGreaterThan(1);
    });
  });

  describe("切换分支", () => {
    it("点一个分支会 checkout,并带上 isRemote=false", async () => {
      await openPicker([local("main", true), local("dev")]);
      fireEvent.click(await screen.findByText("dev"));

      await waitFor(() => expect(callsTo("git_checkout_branch")).toHaveLength(1));
      expect(callsTo("git_checkout_branch")[0]?.[1]).toEqual({
        projectPath: "/tmp/repo",
        branchName: "dev",
        isRemote: false,
      });
    });

    it("远程分支带 isRemote=true(后端要据此建跟踪分支)", async () => {
      await openPicker([local("main", true), remote("origin/staging")]);
      fireEvent.click(await screen.findByText("origin/staging"));

      await waitFor(() => expect(callsTo("git_checkout_branch")).toHaveLength(1));
      expect(callsTo("git_checkout_branch")[0]?.[1]).toMatchObject({
        branchName: "origin/staging",
        isRemote: true,
      });
    });

    it("远程分支按 remote 名分组", async () => {
      await openPicker([
        local("main", true),
        remote("origin/dev"),
        remote("upstream/dev", "upstream"),
      ]);
      await waitFor(() => expect(screen.getByText("origin")).toBeInTheDocument());
      expect(screen.getByText("upstream")).toBeInTheDocument();
      expect(screen.getByText("Local")).toBeInTheDocument();
    });

    it("点当前分支不发请求", async () => {
      await openPicker([local("main", true), local("dev")]);
      // 当前分支在列表里也渲染一行,点它应当无副作用。
      const rows = await screen.findAllByText("main");
      fireEvent.click(rows[rows.length - 1]);
      await act(async () => {
        await Promise.resolve();
      });
      expect(callsTo("git_checkout_branch")).toHaveLength(0);
    });

    it("切换进行中时其余分支不可点,只有目标行显示进度", async () => {
      let finish: (() => void) | undefined;
      vi.mocked(invoke).mockImplementation((command) => {
        if (command === "git_list_branches") {
          return Promise.resolve([local("main", true), local("dev"), local("release")]);
        }
        if (command === "git_checkout_branch") {
          return new Promise<void>((resolve) => {
            finish = resolve;
          });
        }
        return Promise.resolve(undefined);
      });
      renderBar();
      await waitForBranch("main");
      fireEvent.click(screen.getByTitle("Switch branch"));

      const dev = await screen.findByText("dev");
      fireEvent.click(dev);
      await waitFor(() => expect(callsTo("git_checkout_branch")).toHaveLength(1));

      const release = screen.getByText("release");
      // 拦第二次切换的是 `disabled={!!switching}`;`handleSwitch` 里的 `|| switching`
      // 是同一件事的第二道闸门,从 UI 点不到(按钮已 disabled,click 不派发)。
      // 变异测试确认过:摘掉那半个条件本文件全绿。保留它是有意的兜底,不是可测行为。
      expect(release.closest("button")).toBeDisabled();
      expect(dev.closest("button")).toBeDisabled();
      // 进度指示只挂在正在切的那一行。
      expect(dev.closest("button")).toHaveTextContent("…");
      expect(release.closest("button")).not.toHaveTextContent("…");

      // 再点别的分支不会打出第二次 checkout。
      fireEvent.click(release);
      await act(async () => {
        await Promise.resolve();
      });
      expect(callsTo("git_checkout_branch")).toHaveLength(1);

      await act(async () => {
        finish?.();
        await Promise.resolve();
      });
    });

    it("checkout 失败时把错误显示出来,且不关掉面板", async () => {
      vi.mocked(invoke).mockImplementation((command) => {
        if (command === "git_list_branches") {
          return Promise.resolve([local("main", true), local("dev")]);
        }
        if (command === "git_checkout_branch") {
          return Promise.reject(new Error("local changes would be overwritten"));
        }
        return Promise.resolve(undefined);
      });
      renderBar();
      await waitForBranch("main");
      fireEvent.click(screen.getByTitle("Switch branch"));
      fireEvent.click(await screen.findByText("dev"));

      // 报错留在面板上:关掉的话用户看不到为什么没切过去。
      await waitFor(() =>
        expect(screen.getByText(/local changes would be overwritten/)).toBeInTheDocument(),
      );
      expect(screen.getByPlaceholderText("Switch to branch…")).toBeInTheDocument();
    });

    it("切换成功后重新拉分支列表", async () => {
      await openPicker([local("main", true), local("dev")]);
      const before = callsTo("git_list_branches").length;
      fireEvent.click(await screen.findByText("dev"));

      await waitFor(() => expect(callsTo("git_checkout_branch")).toHaveLength(1));
      // 不重拉的话触发器上仍显示旧分支名。
      await waitFor(() => expect(callsTo("git_list_branches").length).toBeGreaterThan(before));
    });
  });

  describe("搜索过滤", () => {
    it("按子串过滤", async () => {
      const input = await openPicker([
        local("main", true),
        local("feature/login"),
        local("hotfix/crash"),
      ]);
      fireEvent.change(input, { target: { value: "login" } });

      await waitFor(() => expect(screen.getByText("feature/login")).toBeInTheDocument());
      expect(screen.queryByText("hotfix/crash")).not.toBeInTheDocument();
    });

    it("大小写两侧都归一(分支名带大写也搜得到)", async () => {
      // 只用小写分支名 + 大写搜索词是测不出来的:那种情况下 `q` 已经小写,
      // 少一次 name.toLowerCase() 照样命中。分支名必须带大写,比如 JIRA-42 这种。
      const input = await openPicker([local("main", true), local("Feature/JIRA-42")]);
      fireEvent.change(input, { target: { value: "jira-42" } });
      await waitFor(() => expect(screen.getByText("Feature/JIRA-42")).toBeInTheDocument());

      fireEvent.change(input, { target: { value: "FEATURE" } });
      await waitFor(() => expect(screen.getByText("Feature/JIRA-42")).toBeInTheDocument());
    });

    it("过滤同时作用于本地与远程", async () => {
      const input = await openPicker([
        local("main", true),
        local("feature/login"),
        remote("origin/feature/login"),
        remote("origin/other"),
      ]);
      fireEvent.change(input, { target: { value: "feature" } });

      await waitFor(() => expect(screen.getByText("feature/login")).toBeInTheDocument());
      expect(screen.getByText("origin/feature/login")).toBeInTheDocument();
      expect(screen.queryByText("origin/other")).not.toBeInTheDocument();
    });

    it("一个都没命中时给空状态提示", async () => {
      const input = await openPicker([local("main", true), local("dev")]);
      fireEvent.change(input, { target: { value: "no-such-branch" } });
      await waitFor(() => expect(screen.getByText("No branches found")).toBeInTheDocument());
      expect(screen.queryByText("Local")).not.toBeInTheDocument();
    });

    it("清空按钮把搜索词清掉,全部分支回来", async () => {
      const input = await openPicker([local("main", true), local("dev"), local("release")]);
      fireEvent.change(input, { target: { value: "dev" } });
      await waitFor(() => expect(screen.queryByText("release")).not.toBeInTheDocument());

      fireEvent.change(input, { target: { value: "" } });
      await waitFor(() => expect(screen.getByText("release")).toBeInTheDocument());
    });
  });

  describe("新建分支", () => {
    /** 打开选择器 → 点底部"New branch…" → 拿到弹窗里的分支名输入框。 */
    async function openDialog(branches: Branch[] = [local("main", true), local("dev")]) {
      await openPicker(branches);
      fireEvent.click(await screen.findByText("New branch…"));
      return screen.findByPlaceholderText("feature/my-branch");
    }

    it("默认以当前分支为基线", async () => {
      await openDialog();
      expect(screen.getByText("Create Branch")).toBeInTheDocument();
      // "Based on" 触发器上显示 currentBranch。
      await waitFor(() => expect(screen.getAllByText("main").length).toBeGreaterThan(0));
    });

    it("名字为空时创建按钮禁用,敲回车也不发请求", async () => {
      const input = await openDialog();
      // 空名有三道闸门:按钮 disabled、handleKeyDown 的 branchName.trim()、
      // handleCreate 里的 `if (!name) return`。前两道就足以让第三道从 UI 点不到
      // (变异测试:删掉 handleCreate 那句本文件仍全绿),这里断言的是前两道。
      expect(screen.getByText("Create & switch").closest("button")).toBeDisabled();

      fireEvent.keyDown(input, { key: "Enter" });
      await act(async () => {
        await Promise.resolve();
      });
      expect(callsTo("git_create_branch")).toHaveLength(0);
    });

    it("只有空白字符也算空", async () => {
      const input = await openDialog();
      fireEvent.change(input, { target: { value: "   " } });
      await waitFor(() =>
        expect(screen.getByText("Create & switch").closest("button")).toBeDisabled(),
      );
      fireEvent.keyDown(input, { key: "Enter" });
      await act(async () => {
        await Promise.resolve();
      });
      expect(callsTo("git_create_branch")).toHaveLength(0);
    });

    it("回车 = 创建并切换(checkout: true),名字两端空白被裁掉", async () => {
      const input = await openDialog();
      fireEvent.change(input, { target: { value: "  feature/x  " } });
      fireEvent.keyDown(input, { key: "Enter" });

      await waitFor(() => expect(callsTo("git_create_branch")).toHaveLength(1));
      expect(callsTo("git_create_branch")[0]?.[1]).toEqual({
        projectPath: "/tmp/repo",
        branchName: "feature/x",
        fromBranch: "main",
        checkout: true,
      });
    });

    it("创建过程中连按回车不会重复发请求", async () => {
      // git 慢的时候用户会连敲回车,第二次必须被 loading 挡住,否则重名报错。
      let finish: (() => void) | undefined;
      vi.mocked(invoke).mockImplementation((command) => {
        if (command === "git_list_branches") {
          return Promise.resolve([local("main", true), local("dev")]);
        }
        if (command === "git_create_branch") {
          return new Promise<void>((resolve) => {
            finish = resolve;
          });
        }
        return Promise.resolve(undefined);
      });
      renderBar();
      await waitForBranch("main");
      fireEvent.click(screen.getByTitle("Switch branch"));
      fireEvent.click(await screen.findByText("New branch…"));
      const input = await screen.findByPlaceholderText("feature/my-branch");

      fireEvent.change(input, { target: { value: "feature/slow" } });
      fireEvent.keyDown(input, { key: "Enter" });
      await waitFor(() => expect(callsTo("git_create_branch")).toHaveLength(1));
      // 按钮文案切成进行中。
      expect(screen.getByText("Creating…")).toBeInTheDocument();

      fireEvent.keyDown(input, { key: "Enter" });
      fireEvent.keyDown(input, { key: "Enter" });
      await act(async () => {
        await Promise.resolve();
      });
      expect(callsTo("git_create_branch")).toHaveLength(1);

      await act(async () => {
        finish?.();
        await Promise.resolve();
      });
    });

    it("Create only 走 checkout: false", async () => {
      const input = await openDialog();
      fireEvent.change(input, { target: { value: "feature/y" } });
      // 拆分按钮:主按钮旁的下拉里才有 "Create only"。
      fireEvent.click(screen.getByText("Create & switch"));
      fireEvent.click(await screen.findByText("Create only"));

      await waitFor(() => expect(callsTo("git_create_branch")).toHaveLength(1));
      expect(callsTo("git_create_branch")[0]?.[1]).toMatchObject({
        branchName: "feature/y",
        checkout: false,
      });
    });

    it("可以改基线分支", async () => {
      const input = await openDialog([local("main", true), local("DEV")]);
      fireEvent.change(input, { target: { value: "feature/z" } });

      // 基线下拉的触发器上显示的是 fromBranch(默认当前分支),按文案取会撞到
      // 触发器/列表里的同名项,所以按 class 取。
      const fromTrigger = document.querySelector<HTMLElement>(".radix-select-trigger");
      expect(fromTrigger).toHaveTextContent("main");
      fireEvent.click(fromTrigger!);

      const search = await screen.findByPlaceholderText("Search branches…");
      // 大写分支名 + 小写搜索词:弹窗里的过滤也要两侧归一。
      fireEvent.change(search, { target: { value: "dev" } });
      fireEvent.click(await screen.findByText("DEV"));

      fireEvent.keyDown(input, { key: "Enter" });
      await waitFor(() => expect(callsTo("git_create_branch")).toHaveLength(1));
      expect(callsTo("git_create_branch")[0]?.[1]).toMatchObject({ fromBranch: "DEV" });
    });

    it("创建成功后关闭弹窗并重新拉列表", async () => {
      const input = await openDialog();
      const before = callsTo("git_list_branches").length;
      fireEvent.change(input, { target: { value: "feature/ok" } });
      fireEvent.keyDown(input, { key: "Enter" });

      await waitFor(() => expect(screen.queryByText("Create Branch")).not.toBeInTheDocument());
      expect(callsTo("git_list_branches").length).toBeGreaterThan(before);
    });

    it("创建失败时弹窗留着并显示错误", async () => {
      vi.mocked(invoke).mockImplementation((command) => {
        if (command === "git_list_branches") {
          return Promise.resolve([local("main", true), local("dev")]);
        }
        if (command === "git_create_branch") {
          return Promise.reject(new Error("branch already exists"));
        }
        return Promise.resolve(undefined);
      });
      renderBar();
      await waitForBranch("main");
      fireEvent.click(screen.getByTitle("Switch branch"));
      fireEvent.click(await screen.findByText("New branch…"));
      const input = await screen.findByPlaceholderText("feature/my-branch");
      fireEvent.change(input, { target: { value: "dev" } });
      fireEvent.keyDown(input, { key: "Enter" });

      await waitFor(() => expect(screen.getByText(/branch already exists/)).toBeInTheDocument());
      expect(screen.getByText("Create Branch")).toBeInTheDocument();
    });

    it("Escape 关闭弹窗,不发请求", async () => {
      const input = await openDialog();
      fireEvent.change(input, { target: { value: "feature/never" } });
      fireEvent.keyDown(input, { key: "Escape" });

      await waitFor(() => expect(screen.queryByText("Create Branch")).not.toBeInTheDocument());
      expect(callsTo("git_create_branch")).toHaveLength(0);
    });

    it("Cancel 关闭弹窗,不发请求", async () => {
      const input = await openDialog();
      fireEvent.change(input, { target: { value: "feature/never" } });
      fireEvent.click(screen.getByText("Cancel"));

      await waitFor(() => expect(screen.queryByText("Create Branch")).not.toBeInTheDocument());
      expect(callsTo("git_create_branch")).toHaveLength(0);
    });
  });
});
