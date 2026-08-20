/**
 * Shared state for the DeepSeek Harness trajectory surface.
 *
 * The trigger sits above the terminal and the panel sits inside it, so the two
 * are sibling subtrees that cannot pass props to each other. This provider wraps
 * both and owns what they share: the open flag, the session's event fold, and
 * the attachment cache.
 *
 * The fold and the image cache live here rather than in the panel because the
 * fold is one `get_dsh_session_history` call plus one `dsh-session-event`
 * listener and the cache mints object URLs it must revoke: mounting either with
 * the panel would resubscribe and refetch every time the panel is opened.
 */

import { createContext, useContext, useState, type ReactNode } from "react";
import type { DshLiveSessionState } from "../types";
import { useDshSessionFeatures } from "../hooks/useDshSessionFeatures";
import { useDshImageLoader, type DshImageLoader } from "../hooks/useDshImageLoader";

export interface DshTrajectoryContextValue {
  sessionId: string;
  /** The live push frame, when the session is still attached. */
  live?: DshLiveSessionState;
  history: ReturnType<typeof useDshSessionFeatures>;
  loadImage: DshImageLoader;
  open: boolean;
  setOpen: (open: boolean) => void;
}

const DshTrajectoryContext = createContext<DshTrajectoryContextValue | null>(null);

export function DshTrajectoryHost({
  sessionId,
  live,
  children,
}: {
  sessionId: string;
  live?: DshLiveSessionState;
  children: ReactNode;
}) {
  const history = useDshSessionFeatures(sessionId);
  const loadImage = useDshImageLoader(sessionId);
  const [open, setOpen] = useState(false);
  // Rebuilt on every render on purpose: the fold is a fresh projection whenever
  // an event arrives, which is exactly when the trigger's count and the panel's
  // rows both have to change.
  const value: DshTrajectoryContextValue = {
    sessionId,
    ...(live === undefined ? {} : { live }),
    history,
    loadImage,
    open,
    setOpen,
  };
  return <DshTrajectoryContext.Provider value={value}>{children}</DshTrajectoryContext.Provider>;
}

/** Throws outside the provider, so a mis-wired subtree fails loudly. */
export function useDshTrajectory(): DshTrajectoryContextValue {
  const value = useContext(DshTrajectoryContext);
  if (value === null) throw new Error("useDshTrajectory must be used within DshTrajectoryHost");
  return value;
}
