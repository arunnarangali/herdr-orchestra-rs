import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterEach } from "vitest";
import { resetChatMemoryForTests } from "../screens/chat/chatMemory";

// The suite runs many files in parallel workers; under that load the app's async settles
// (health checks, fetches, stream connects) can outlast the 1s default. A wider window
// keeps the same assertions honest on a busy machine.
configure({ asyncUtilTimeout: 5000 });

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
