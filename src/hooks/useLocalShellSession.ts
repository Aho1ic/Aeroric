import { useCallback, useRef, useState } from "react";

import type { ShellSession, ShellTerminalPanelHandle } from "../components/ShellTerminalPanel";

/**
 * 本地 shell 终端的会话状态与待发命令。
 *
 * 之所以值得聚成一簇:这五件东西必须一起收尾。终端关闭时若只清 `mounted` 而留下
 * `sessions`,工作区标签条会继续列出已经不存在的终端(它按 `shellSessions` 渲染);
 * 若留下 `ready`,下一次 `sendOrQueue` 会以为面板还活着,把命令直接投给一个已卸载的
 * 句柄,命令静默消失;若留下 `pendingCmd`,那条上一轮没来得及发的命令会在下次挂载
 * 就绪时突然执行 —— 用户看到的是「我没敲过的命令自己跑了」。
 *
 * 分界:这里只管「本地 shell 这一簇」。终端的可见性(`showShellTerminal`)、右侧面板
 * 归属都是页面级导航,页面里另有十几个读写点,不搬进来;`mountShell()` 只负责挂载,
 * 可见性由调用方在页面里自己置。
 */
export interface LocalShellSessionState {
  /** 面板句柄。页面要把它交给 `ProjectTerminals`,所以直接暴露 ref 而不是包一层。 */
  shellRef: React.RefObject<ShellTerminalPanelHandle | null>;
  /** 是否已挂载过面板。挂载后不随可见性变化卸载,保留终端里的会话。 */
  shellTerminalMounted: boolean;
  /** 面板报上来的会话列表,工作区标签条按它渲染。 */
  shellSessions: ShellSession[];
  activeShellId: string | null;
  /** 挂载面板(不改可见性)。 */
  mountShell: () => void;
  /** 面板就绪:补发上一轮排队的命令。 */
  handleShellReady: () => void;
  handleShellSessionsChange: (sessions: ShellSession[], nextActiveShellId: string | null) => void;
  /**
   * 关闭终端:把这一簇一次清干净。
   *
   * 页面侧还要另置可见性与右侧面板 —— 那两件是导航,不在这簇里。
   */
  resetShellSession: () => void;
  /**
   * 面板就绪就直接发,否则排队等 `handleShellReady`。
   *
   * 顺带挂载面板 —— 命令要有终端可发。可见性由调用方置。
   */
  sendOrQueueLocalCommand: (cmd: string) => void;
}

export function useLocalShellSession(): LocalShellSessionState {
  const shellRef = useRef<ShellTerminalPanelHandle>(null);
  const shellReadyRef = useRef(false);
  const pendingCmdRef = useRef<string | null>(null);
  const [shellTerminalMounted, setShellTerminalMounted] = useState(false);
  const [shellSessions, setShellSessions] = useState<ShellSession[]>([]);
  const [activeShellId, setActiveShellId] = useState<string | null>(null);

  const mountShell = useCallback(() => {
    setShellTerminalMounted(true);
  }, []);

  const handleShellReady = useCallback(() => {
    shellReadyRef.current = true;
    if (!pendingCmdRef.current || !shellRef.current) return;
    shellRef.current.sendCommand(pendingCmdRef.current);
    pendingCmdRef.current = null;
  }, []);

  const handleShellSessionsChange = useCallback(
    (sessions: ShellSession[], nextActiveShellId: string | null) => {
      setShellSessions(sessions);
      setActiveShellId(nextActiveShellId);
    },
    [],
  );

  const resetShellSession = useCallback(() => {
    setShellTerminalMounted(false);
    setShellSessions([]);
    setActiveShellId(null);
    shellReadyRef.current = false;
    pendingCmdRef.current = null;
  }, []);

  const sendOrQueueLocalCommand = useCallback((cmd: string) => {
    setShellTerminalMounted(true);
    if (shellReadyRef.current && shellRef.current) {
      shellRef.current.sendCommand(cmd);
      return;
    }
    pendingCmdRef.current = cmd;
  }, []);

  return {
    shellRef,
    shellTerminalMounted,
    shellSessions,
    activeShellId,
    mountShell,
    handleShellReady,
    handleShellSessionsChange,
    resetShellSession,
    sendOrQueueLocalCommand,
  };
}
