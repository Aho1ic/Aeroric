// Shared scheduler for "visual" preview blocks (math, mermaid, diagrams).
// These renderers are CPU-heavy per block (katex compile, mermaid SVG layout,
// viz.js Graphviz). Doing them all up-front on a chart-heavy doc freezes the
// main thread for seconds.
//
// Strategy (mirrors enhanceCalloutsLazy / enhanceWikiLinksLazy):
//   1. Pick the blocks currently in (or near) the viewport.
//   2. Render that visible set first, serially, yielding to the browser
//      between blocks via requestIdleCallback so user input stays responsive.
//   3. Observe the rest with IntersectionObserver; on scroll-in, queue them
//      and process the queue with the same yield-between policy.
//
// Concurrency: serial. These renderers are CPU-bound on the main thread —
// running them in parallel via Promise.all only forces the scheduler to
// interleave them, raising peak latency without finishing any sooner.

export interface VisualBlockHandle {
  /** Stop scheduling further work; in-flight render of the current block runs to completion. */
  disconnect(): void;
  /** Render every remaining block immediately (used for export / tests). */
  flushAll(): Promise<void>;
}

export interface VisualSchedulerOptions {
  rootMargin?: string;
  viewportHeight?: number;
  visibilityMargin?: number;
  /** Yield strategy between blocks. Default: requestIdleCallback. */
  yieldFn?: () => Promise<void>;
}

const noop = () => undefined;
const noopAsync = async () => undefined;

function defaultYield(): Promise<void> {
  return new Promise<void>((resolve) => {
    if (typeof window !== "undefined" && typeof window.requestIdleCallback === "function") {
      window.requestIdleCallback(() => resolve(), { timeout: 100 });
    } else if (typeof queueMicrotask === "function") {
      queueMicrotask(() => resolve());
    } else {
      setTimeout(resolve, 0);
    }
  });
}

export function scheduleVisualBlocks<T extends HTMLElement>(
  root: HTMLElement,
  selector: string,
  renderOne: (block: T) => Promise<void> | void,
  options: VisualSchedulerOptions = {},
): VisualBlockHandle {
  const all = Array.from(root.querySelectorAll<T>(selector));
  if (all.length === 0) {
    return { disconnect: noop, flushAll: noopAsync };
  }

  const viewportH =
    options.viewportHeight ??
    (typeof window !== "undefined" && typeof window.innerHeight === "number"
      ? window.innerHeight
      : 0);
  const margin = options.visibilityMargin ?? 200;
  const yieldFn = options.yieldFn ?? defaultYield;

  const visible: T[] = [];
  const pending: T[] = [];
  for (const block of all) {
    const rect = block.getBoundingClientRect();
    const isVisible = rect.bottom >= -margin && rect.top <= viewportH + margin;
    if (isVisible) visible.push(block);
    else pending.push(block);
  }

  let cancelled = false;

  // Single-flight queue processor so IO bursts don't spawn concurrent loops.
  const queue: T[] = [];
  let processing = false;

  const runOne = async (block: T) => {
    try {
      await renderOne(block);
    } catch {
      /* renderer is responsible for its own error UI */
    }
  };

  const drain = async () => {
    if (processing) return;
    processing = true;
    while (queue.length > 0 && !cancelled) {
      const block = queue.shift()!;
      await runOne(block);
      if (cancelled) break;
      await yieldFn();
    }
    processing = false;
  };

  // Stage 1: visible set, in order.
  const visibleDone = (async () => {
    for (const block of visible) {
      if (cancelled) return;
      await runOne(block);
      if (cancelled) return;
      await yieldFn();
    }
  })();

  if (pending.length === 0) {
    return {
      disconnect: () => {
        cancelled = true;
      },
      flushAll: async () => {
        await visibleDone;
      },
    };
  }

  const IO =
    typeof globalThis !== "undefined" && "IntersectionObserver" in globalThis
      ? (globalThis as typeof window).IntersectionObserver
      : null;

  if (!IO) {
    // No IO available: chain all remaining onto visibleDone, still yielding.
    const allDone = visibleDone.then(async () => {
      for (const block of pending) {
        if (cancelled) return;
        await runOne(block);
        if (cancelled) return;
        await yieldFn();
      }
    });
    return {
      disconnect: () => {
        cancelled = true;
      },
      flushAll: async () => {
        await allDone;
      },
    };
  }

  const observer = new IO(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        queue.push(entry.target as T);
        observer.unobserve(entry.target);
      }
      void drain();
    },
    { rootMargin: options.rootMargin ?? "200px 0px" },
  );

  for (const block of pending) observer.observe(block);

  return {
    disconnect: () => {
      cancelled = true;
      observer.disconnect();
    },
    flushAll: async () => {
      observer.disconnect();
      queue.length = 0;
      await visibleDone;
      for (const block of pending) {
        if (cancelled) return;
        await runOne(block);
        if (cancelled) return;
        await yieldFn();
      }
    },
  };
}
