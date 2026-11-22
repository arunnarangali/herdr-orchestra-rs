import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterEach } from "vitest";
import { resetChatMemoryForTests } from "../screens/chat/chatMemory";

// The suite runs many files in parallel workers, often next to other agents' builds; under
// that load the app's async settles (health checks, fetches, stream connects) can take
// several seconds (a check run measured 3-25x the usual times). Each wait gets most of the
// test's own 20s budget (vite.config.ts) instead of giving up at 5s: the assertions are the
// same, a passing wait returns as soon as it holds, and only a slow machine waits longer.
configure({ asyncUtilTimeout: 15_000 });

// Node 25 ships its own `localStorage` global that is unusable without a file; use a plain
// in-memory one so tests behave the same on every Node version.
const store = new Map<string, string>();
const memoryStorage: Storage = {
  get length() {
    return store.size;
  },
  clear: () => store.clear(),
  getItem: (k) => store.get(k) ?? null,
  key: (i) => [...store.keys()][i] ?? null,
  removeItem: (k) => void store.delete(k),
  setItem: (k, v) => void store.set(k, String(v)),
};
Object.defineProperty(window, "localStorage", { value: memoryStorage, configurable: true });

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  resetChatMemoryForTests();
});

// jsdom has no matchMedia; tests choose the system theme through `setSystemDark`.
let systemDark = false;
const listeners = new Set<(e: MediaQueryListEvent) => void>();

Object.defineProperty(window, "matchMedia", {
  writable: true,
  value: (query: string): MediaQueryList =>
    ({
      matches: query.includes("dark") ? systemDark : false,
      media: query,
      onchange: null,
      addEventListener: (_: string, l: (e: MediaQueryListEvent) => void) => listeners.add(l),
      removeEventListener: (_: string, l: (e: MediaQueryListEvent) => void) => listeners.delete(l),
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    }) as unknown as MediaQueryList,
});

export function setSystemDark(dark: boolean): void {
  systemDark = dark;
  for (const l of listeners) l({ matches: dark } as MediaQueryListEvent);
}
