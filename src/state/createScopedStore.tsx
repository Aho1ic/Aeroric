import { createContext, createElement, useContext, useRef, type ReactNode } from "react";
import { useStore } from "zustand";
import type { StoreApi } from "zustand/vanilla";

export function createScopedStore<State>(displayName: string, createStore: () => StoreApi<State>) {
  const Context = createContext<StoreApi<State> | null>(null);
  Context.displayName = `${displayName}Context`;

  function Provider({ children }: { children: ReactNode }) {
    const storeRef = useRef<StoreApi<State> | null>(null);
    if (!storeRef.current) storeRef.current = createStore();
    return createElement(Context.Provider, { value: storeRef.current }, children);
  }

  function useScopedStore<Selected>(selector: (state: State) => Selected): Selected {
    const store = useContext(Context);
    if (!store)
      throw new Error(`use${displayName}Store must be used within ${displayName}Provider`);
    return useStore(store, selector);
  }

  return { Provider, useScopedStore, Context };
}
