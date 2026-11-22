import type { ApiError } from "../../api/errors";
import type { ChatBody, ChatMessage } from "../../api/types";

/** A message of the developer's that has not been stored yet, or could not be sent. */
export interface OutboxEntry {
  key: number;
  body: ChatBody;
  /** `waiting`: no run exists yet, so it is held and sent by itself once the run is there. */
  state: "sending" | "failed" | "waiting";
  error: string | null;
}

/** Everything Chat shows for one project, kept while the developer is on another page. */
export interface ChatSnapshot {
  byId: Map<number, ChatMessage>;
  state: "loading" | "ready" | "error";
  error: ApiError | null;
  noRun: boolean;
  outbox: OutboxEntry[];
  leadProblem: string | null;
  /** What is typed in the message box and not sent yet. */
  draft: string;
}

/**
 * One project's chat, outside any screen. The Chat page comes and goes with the route; this
 * lives as long as the app, so leaving Chat and coming back shows the same conversation, with
 * the same message states and the unsent text. A send that is still in flight when the page
 * is left finishes into it, so its result is there on return.
 */
export interface ChatMemory {
  get(): ChatSnapshot;
  update(fn: (s: ChatSnapshot) => Partial<ChatSnapshot>): void;
  subscribe(listener: () => void): () => void;
  /** A new outbox key, unique for the life of the app. */
  nextKey(): number;
}

function createMemory(): ChatMemory {
  let snap: ChatSnapshot = {
    byId: new Map(),
    state: "loading",
    error: null,
    noRun: false,
    outbox: [],
    leadProblem: null,
    draft: "",
  };
  let keys = 0;
  const listeners = new Set<() => void>();
  return {
    get: () => snap,
    update(fn) {
      const patch = fn(snap);
      // nothing changed: do not wake the screen
      if (Object.entries(patch).every(([k, v]) => Object.is(snap[k as keyof ChatSnapshot], v))) return;
      snap = { ...snap, ...patch };
      for (const l of [...listeners]) l();
    },
    subscribe(l) {
      listeners.add(l);
      return () => void listeners.delete(l);
    },
    nextKey: () => ++keys,
  };
}

const memories = new Map<string, ChatMemory>();

/** The name used before the connection says which project it is. */
export const UNKNOWN_PROJECT = "project";

/**
 * The chat memory of a project (made on first use). The first time a known project asks, it
 * takes over what was gathered under `UNKNOWN_PROJECT` while the connection was still saying
 * which project it is, so nothing flickers away when the name arrives.
 */
export function chatMemoryFor(project: string): ChatMemory {
  let m = memories.get(project);
  if (!m) {
    m = createMemory();
    memories.set(project, m);
    const early = project === UNKNOWN_PROJECT ? undefined : memories.get(UNKNOWN_PROJECT);
    if (early) {
      m.update(() => early.get());
      memories.delete(UNKNOWN_PROJECT);
    }
  }
  return m;
}

/** For tests: forget every project's chat. */
export function resetChatMemoryForTests(): void {
  memories.clear();
}
