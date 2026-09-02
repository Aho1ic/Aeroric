import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { act, render } from "@testing-library/react";
import { useEffect } from "react";

import type { ShellSession, ShellTerminalPanelHandle } from "../components/ShellTerminalPanel";
import { useLocalShellSession, type LocalShellSessionState } from "../hooks/useLocalShellSession";

/**
 * `useLocalShellSession` 的收尾约束。
 *
 * 这簇状态的价值全在「一起清」:`resetShellSession()` 漏掉任何一项都不会让页面报错,
 * 而是留下一个矛盾状态(标签条列着已销毁的会话、命令投给已卸载的句柄、下一轮突然
 * 冒出上一轮的命令)。所以每条用例都盯住某一项单独残留时才会露出的后果,而不是只
 * 断言 `mounted === false` —— 那样的话删掉另外四行也照样全绿。
 */

// 真实的 handle 类型,而不是随手写个对象:字段名写错了要在编译期就挡住,
// 不能等到「mock 交付了后端产不出的字段」再发现。
function fakeShellHandle(): ShellTerminalPanelHandle & { sent: string[] } {
  const sent: string[] = [];
  return {
    sent,
    sendCommand: (cmd: string) => sent.push(cmd),
    addShell: () => {},
    closeShell: () => {},
    activateShell: () => {},
  };
}

function sessions(...ids: string[]): ShellSession[] {
  return ids.map((id) => ({ id, title: id }));
}

/** 把 hook 的返回值抬到测试作用域,顺带记录渲染次数。 */
function mountHook() {
  let api: LocalShellSessionState | null = null;
  function Probe() {
    api = useLocalShellSession();
    return null;
  }
  const view = render(<Probe />);
  return {
    get api() {
      if (!api) throw new Error("hook 尚未挂载");
      return api;
    },
    unmount: view.unmount,
  };
}

// jsdom 会把 effect / 事件回调里抛的异常吞掉,渲染结果看起来只是「没更新」。
// 每个文件都挂一个哨兵,否则「状态没变」和「读 null 崩了」在断言上无法区分。
const errors: unknown[] = [];
const onError = (event: ErrorEvent) => errors.push(event.error ?? event.message);

beforeEach(() => {
  errors.length = 0;
  window.addEventListener("error", onError);
});

afterEach(() => {
  window.removeEventListener("error", onError);
  expect(errors).toEqual([]);
});

describe("useLocalShellSession", () => {
  it("初始是未挂载、无会话", () => {
    const { api } = mountHook();

    expect(api.shellTerminalMounted).toBe(false);
    expect(api.shellSessions).toEqual([]);
    expect(api.activeShellId).toBe(null);
  });

  it("mountShell 只挂载,不动可见性以外的会话状态", () => {
    const hook = mountHook();

    act(() => hook.api.mountShell());

    expect(hook.api.shellTerminalMounted).toBe(true);
    // 挂载不代表已有会话 —— 会话要等面板自己报上来。
    expect(hook.api.shellSessions).toEqual([]);
    expect(hook.api.activeShellId).toBe(null);
  });

  it("面板报上来的会话列表与活动 id 都落到状态里", () => {
    const hook = mountHook();

    act(() => hook.api.handleShellSessionsChange(sessions("a", "b"), "b"));

    expect(hook.api.shellSessions).toEqual(sessions("a", "b"));
    expect(hook.api.activeShellId).toBe("b");
  });

  describe("待发命令", () => {
    it("面板未就绪时排队,就绪后补发", () => {
      const hook = mountHook();
      const handle = fakeShellHandle();
      hook.api.shellRef.current = handle;

      act(() => hook.api.sendOrQueueLocalCommand("pnpm dev\n"));
      // 还没 ready:必须排队,不能直接投出去 —— 面板此刻收不了。
      expect(handle.sent).toEqual([]);
      // 顺带挂载:命令总得有个终端可发。
      expect(hook.api.shellTerminalMounted).toBe(true);

      act(() => hook.api.handleShellReady());
      expect(handle.sent).toEqual(["pnpm dev\n"]);
    });

    it("已就绪时直接发,不进队列", () => {
      const hook = mountHook();
      const handle = fakeShellHandle();
      hook.api.shellRef.current = handle;
      act(() => hook.api.handleShellReady());

      act(() => hook.api.sendOrQueueLocalCommand("ls\n"));
      expect(handle.sent).toEqual(["ls\n"]);

      // 再 ready 一次不该把同一条命令重放 —— 发完队列就得清空。
      act(() => hook.api.handleShellReady());
      expect(handle.sent).toEqual(["ls\n"]);
    });

    it("排队多条时只保留最后一条", () => {
      const hook = mountHook();
      const handle = fakeShellHandle();
      hook.api.shellRef.current = handle;

      act(() => {
        hook.api.sendOrQueueLocalCommand("first\n");
        hook.api.sendOrQueueLocalCommand("second\n");
      });
      act(() => hook.api.handleShellReady());

      expect(handle.sent).toEqual(["second\n"]);
    });
  });

  describe("resetShellSession 必须把五件事一起清", () => {
    /** 走一遍「挂载 → 有会话 → 已就绪 → 有排队命令」,再关闭。 */
    function openThenReset() {
      const hook = mountHook();
      const handle = fakeShellHandle();
      hook.api.shellRef.current = handle;

      act(() => {
        hook.api.mountShell();
        hook.api.handleShellSessionsChange(sessions("a", "b"), "b");
      });
      act(() => hook.api.handleShellReady());
      act(() => hook.api.resetShellSession());
      return { hook, handle };
    }

    it("mounted 归 false", () => {
      const { hook } = openThenReset();
      expect(hook.api.shellTerminalMounted).toBe(false);
    });

    it("会话列表清空:留着会让工作区标签条继续列出已销毁的终端", () => {
      const { hook } = openThenReset();
      expect(hook.api.shellSessions).toEqual([]);
    });

    it("活动 id 归 null:留着会让标签条把高亮钉在一个不存在的会话上", () => {
      const { hook } = openThenReset();
      expect(hook.api.activeShellId).toBe(null);
    });

    it("ready 归 false:留着会把下一条命令直接投给已卸载的句柄", () => {
      const { hook, handle } = openThenReset();
      handle.sent.length = 0;

      // 关闭后再发命令。ready 若没清,这条会被当成「面板还活着」直接投出去;
      // 正确行为是重新排队,等新面板 ready 再发。
      act(() => hook.api.sendOrQueueLocalCommand("after-close\n"));
      expect(handle.sent).toEqual([]);

      act(() => hook.api.handleShellReady());
      expect(handle.sent).toEqual(["after-close\n"]);
    });

    it("待发命令清空:留着会在下次面板就绪时冒出用户没敲过的命令", () => {
      const hook = mountHook();
      const handle = fakeShellHandle();
      hook.api.shellRef.current = handle;

      // 排一条但始终没 ready,然后关闭。
      act(() => hook.api.sendOrQueueLocalCommand("stale\n"));
      act(() => hook.api.resetShellSession());

      // 新面板挂载就绪 —— 上一轮那条不该在这时候执行。
      act(() => hook.api.handleShellReady());
      expect(handle.sent).toEqual([]);
    });
  });

  it("handleShellReady 在没有面板句柄时不抛", () => {
    const hook = mountHook();
    hook.api.shellRef.current = null;

    act(() => hook.api.sendOrQueueLocalCommand("noop\n"));
    // 句柄为空:既不能抛,也不能把队列清掉 —— 命令要等真有面板时再发。
    expect(() => act(() => hook.api.handleShellReady())).not.toThrow();

    const handle = fakeShellHandle();
    hook.api.shellRef.current = handle;
    act(() => hook.api.handleShellReady());
    expect(handle.sent).toEqual(["noop\n"]);
  });

  it("回调的 identity 跨渲染稳定:它们进了页面侧 handler 的依赖数组", () => {
    const hook = mountHook();
    const first = {
      mountShell: hook.api.mountShell,
      handleShellReady: hook.api.handleShellReady,
      handleShellSessionsChange: hook.api.handleShellSessionsChange,
      resetShellSession: hook.api.resetShellSession,
      sendOrQueueLocalCommand: hook.api.sendOrQueueLocalCommand,
      shellRef: hook.api.shellRef,
    };

    // 触发一次真实的状态更新,逼出重渲染。
    act(() => hook.api.handleShellSessionsChange(sessions("a"), "a"));
    expect(hook.api.shellSessions).toEqual(sessions("a"));

    expect(hook.api.mountShell).toBe(first.mountShell);
    expect(hook.api.handleShellReady).toBe(first.handleShellReady);
    expect(hook.api.handleShellSessionsChange).toBe(first.handleShellSessionsChange);
    expect(hook.api.resetShellSession).toBe(first.resetShellSession);
    expect(hook.api.sendOrQueueLocalCommand).toBe(first.sendOrQueueLocalCommand);
    expect(hook.api.shellRef).toBe(first.shellRef);
  });
});

describe("useLocalShellSession 与副作用无关的守卫", () => {
  it("不注册任何订阅或定时器:这簇状态是纯内存的", () => {
    vi.useFakeTimers();
    // 假时钟基准取真实 epoch 毫秒:从 0/1000 起算会让读到时间的地方显示 1970,
    // 而 1970 正是「没读到」的哨兵值,两者在断言上就分不开了。
    vi.setSystemTime(new Date("2026-09-02T12:00:00Z"));
    const addSpy = vi.spyOn(window, "addEventListener");

    let effectRan = false;
    function Probe() {
      useLocalShellSession();
      useEffect(() => {
        effectRan = true;
      }, []);
      return null;
    }
    const view = render(<Probe />);

    expect(effectRan).toBe(true);
    expect(addSpy).not.toHaveBeenCalled();
    // 真推时钟:「没装轮询」必须靠推时钟证明,只 flush 微任务证不了。
    act(() => void vi.advanceTimersByTime(10_000));

    view.unmount();
    addSpy.mockRestore();
    vi.useRealTimers();
  });
});
