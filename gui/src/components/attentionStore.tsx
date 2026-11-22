import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { ApiClient } from "../api/client";
import type { EventStream } from "../api/stream";
import type { AttentionItem } from "../api/types";
import { getBackend } from "../screens/projects/backend";

/** Where attention items come from: one server (a project's). */
export interface AttentionSource {
  /** The project's folder; empty for the single server of a browser page. */
  key: string;
  /** The project's name, shown on each of its items; null when there is only one server. */
  name: string | null;
  client: ApiClient;
  subscribe: EventStream["subscribe"];
}

/** An item together with the project it belongs to. Acting on it uses that project's client. */
export type TaggedItem = AttentionItem & {
  /** Unique across projects (two servers may both have an item called `item-1`). */
  uid: string;
  sourceKey: string;
  projectName: string | null;
};

/** An item that no longer waits. The API only returns open items, so the app keeps this short history. */
export type ResolvedItem = TaggedItem & {
  /** When the app saw it resolved. */
  resolvedAt: string;
  /** What was done, in the words the developer saw ("Approved", "Answered", "Resolved"). */
  resolution: string;
};

/**
 * The stream says when an item appears or goes, not when a blocked worker's prompt goes or
 * changes (its item stays open while the milestone carries on). While any item of a source
 * shows a prompt, that source's items are read again every `promptPollMs`. Tests shorten it.
 */
export const attentionTiming = { promptPollMs: 5_000 };

const tag = (s: AttentionSource, item: AttentionItem): TaggedItem => ({
  ...item,
  uid: `${s.key}\u0000${item.id}`,
  sourceKey: s.key,
  projectName: s.name,
});

async function notifyDesktop(item: TaggedItem) {
  const backend = getBackend();
  let enabled = false;
  if (backend) {
    try {
      const { value } = await backend.getSettings();
      enabled = value.notifications;
    } catch {
      enabled = false;
    }
  }
  if (!enabled) return;
  if (typeof Notification === "undefined" || Notification.permission !== "granted") return;
  const body = item.prompt?.text || item.detail || item.title;
  new Notification(item.projectName ? `${item.projectName}: ${item.title}` : item.title, { body });
}

export interface AttentionStore {
  /** Items that still need the developer, across every source. */
  waitingItems: TaggedItem[];
  /** Items this app saw resolved, newest first. */
  resolvedItems: ResolvedItem[];
  /** Replace one project's open items with what its server just answered (e.g. after answering a prompt). */
  applyItems: (sourceKey: string, items: AttentionItem[]) => void;
  /** Read one project's open items again and show them (e.g. to see whether a prompt went); rejects when that failed. */
  recheckSource: (sourceKey: string) => Promise<void>;
  /** Take an item out of the waiting list (after its action succeeded) and keep it in the history. */
  resolveItem: (item: TaggedItem, resolution?: string) => void;
  currentItem: TaggedItem | null;
  dismissCurrent: () => void;
  openItem: (item: TaggedItem) => void;
  isListOpen: boolean;
  openList: () => void;
  closeList: () => void;
  /** Waiting items per project (by source key). */
  counts: Record<string, number>;
  /** The client of the project an item belongs to: the only client that may act on it. */
  clientFor: (sourceKey: string) => ApiClient | null;
  /** Render this once: it keeps every source's items current. */
  watchers: ReactNode;
}

/** Reads one source: its items now, then every `attention` event of its stream. */
function SourceWatcher({
  source,
  onLoaded,
  onEvent,
  hasPrompt,
}: {
  source: AttentionSource;
  onLoaded: (s: AttentionSource, items: AttentionItem[], first: boolean) => void;
  onEvent: (s: AttentionSource, op: string, item: AttentionItem) => void;
  /** An item of this source shows a prompt: read the items again now and then (see `attentionTiming`). */
  hasPrompt: boolean;
}) {
  const { client, subscribe } = source;
  const ref = useRef(source);
  ref.current = source;
  useEffect(() => {
    if (!hasPrompt) return;
    let live = true;
    const t = setInterval(() => {
      client.attention().then(
        (res) => {
          if (live) onLoaded(ref.current, res.items, false);
        },
        () => {
          // not readable now: the next read tries again
        },
      );
    }, attentionTiming.promptPollMs);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [hasPrompt, client, onLoaded]);
  useEffect(() => {
    let cancelled = false;
    // What the stream reported while the first read was in flight is newer than that read.
    let loaded = false;
    const added = new Map<string, AttentionItem>();
    const removed = new Set<string>();
    // events first, so one that arrives while the first read is in flight is not lost
    const off = subscribe((m) => {
      if (m.event !== "attention") return;
      if (!loaded) {
        if (m.data.op === "add") {
          added.set(m.data.item.id, m.data.item);
          removed.delete(m.data.item.id);
        } else if (m.data.op === "remove") {
          added.delete(m.data.item.id);
          removed.add(m.data.item.id);
        }
      }
      onEvent(ref.current, m.data.op, m.data.item);
    });
    client.attention().then(
      (res) => {
        if (cancelled) return;
        loaded = true;
        const items = res.items.filter((i) => !removed.has(i.id));
        for (const [id, item] of added) if (!items.some((i) => i.id === id)) items.push(item);
        onLoaded(ref.current, items, true);
      },
      () => {
        // the run may not exist yet, or the route is missing: nothing waits
      },
    );
    return () => {
      cancelled = true;
      off();
    };
  }, [client, subscribe, onLoaded, onEvent]);
  return null;
}

/**
 * The attention items of every source, the popup queue (each new item shows once) and the
 * counts per project. Items of different projects never mix: each carries its project, and
 * only its own project's client acts on it.
 */
export function useAttentionStore(sources: AttentionSource[]): AttentionStore {
  const [items, setItems] = useState<TaggedItem[]>([]);
  const [resolved, setResolved] = useState<ResolvedItem[]>([]);
  const [queue, setQueue] = useState<TaggedItem[]>([]);
  const [isListOpen, setIsListOpen] = useState(false);
  const seen = useRef(new Set<string>());
  const previousFocus = useRef<HTMLElement | null>(null);
  const sourcesRef = useRef(sources);
  sourcesRef.current = sources;
  // The event and reload callbacks need the items as they are now, not as their closure saw them.
  const itemsRef = useRef<TaggedItem[]>(items);
  itemsRef.current = items;

  const addResolved = useCallback((item: TaggedItem, resolution: string) => {
    setResolved((prev) =>
      prev.some((x) => x.uid === item.uid)
        ? prev
        : [{ ...item, resolvedAt: new Date().toISOString(), resolution }, ...prev],
    );
  }, []);

  const resolveItem = useCallback(
    (item: TaggedItem, resolution = "Resolved") => {
      setItems((prev) => prev.filter((x) => x.uid !== item.uid));
      setQueue((q) => q.filter((x) => x.uid !== item.uid));
      addResolved(item, resolution);
    },
    [addResolved],
  );

  /**
   * One source's open items as its server listed them. Only the `first` read of a source marks
   * what it lists as seen (already waiting when the app opened: no popup). A later re-read can
   * find a new item before the feed publishes its `add` event; that event must still alert, so
   * a re-read shows the item without marking it seen.
   */
  const onLoaded = useCallback(
    (s: AttentionSource, loaded: AttentionItem[], first: boolean) => {
      const tagged = loaded.map((i) => tag(s, i));
      if (first) for (const t of tagged) seen.current.add(t.uid);
      const fresh = new Map(tagged.map((t) => [t.uid, t]));
      // An item of this source that disappeared was resolved elsewhere (the API only lists open items).
      const still = new Set(tagged.map((t) => t.uid));
      for (const gone of itemsRef.current.filter((x) => x.sourceKey === s.key && !still.has(x.uid))) {
        addResolved(gone, "Resolved");
      }
      setResolved((prev) => prev.filter((x) => !still.has(x.uid)));
      // replace this source's items (an item already shown by an event stays one item)
      setItems((prev) => [...prev.filter((x) => x.sourceKey !== s.key), ...tagged]);
      // the popup's queue shows the same items as they are now: a prompt that went or changed
      // is not left on screen, and an item that is no longer open leaves it
      setQueue((q) => q.flatMap((x) => (x.sourceKey !== s.key ? [x] : fresh.has(x.uid) ? [fresh.get(x.uid)!] : [])));
    },
    [addResolved],
  );

  const onEvent = useCallback(
    (s: AttentionSource, op: string, item: AttentionItem) => {
      const t = tag(s, item);
      if (op === "add") {
        // an item already shown is replaced in place: its prompt may have changed (or gone)
        setItems((prev) => (prev.some((x) => x.uid === t.uid) ? prev.map((x) => (x.uid === t.uid ? t : x)) : [...prev, t]));
        setQueue((q) => q.map((x) => (x.uid === t.uid ? t : x)));
        setResolved((prev) => prev.filter((x) => x.uid !== t.uid));
        if (!seen.current.has(t.uid)) {
          seen.current.add(t.uid);
          if (document.activeElement instanceof HTMLElement && !previousFocus.current) {
            previousFocus.current = document.activeElement;
          }
          setQueue((q) => (q.some((x) => x.uid === t.uid) ? q : [...q, t]));
          void notifyDesktop(t);
        }
      } else if (op === "remove") {
        const gone = itemsRef.current.find((x) => x.uid === t.uid);
        setItems((prev) => prev.filter((x) => x.uid !== t.uid));
        setQueue((q) => q.filter((x) => x.uid !== t.uid));
        if (gone) addResolved(gone, "Resolved");
      }
    },
    [addResolved],
  );

  // A project that is no longer a source (closed) takes its items with it.
  const keys = sources.map((s) => s.key).join("\u0001");
  useEffect(() => {
    const live = new Set(keys.split("\u0001"));
    setItems((prev) => (prev.every((x) => live.has(x.sourceKey)) ? prev : prev.filter((x) => live.has(x.sourceKey))));
    setResolved((prev) => (prev.every((x) => live.has(x.sourceKey)) ? prev : prev.filter((x) => live.has(x.sourceKey))));
    setQueue((prev) => (prev.every((x) => live.has(x.sourceKey)) ? prev : prev.filter((x) => live.has(x.sourceKey))));
    for (const uid of [...seen.current]) if (!live.has(uid.split("\u0000")[0] ?? "")) seen.current.delete(uid);
  }, [keys]);

  // When the queue empties, give focus back to where it was.
  const prevLength = useRef(queue.length);
  useEffect(() => {
    if (prevLength.current > 0 && queue.length === 0 && previousFocus.current) {
      previousFocus.current.focus();
      previousFocus.current = null;
    }
    prevLength.current = queue.length;
  }, [queue.length]);

  const dismissCurrent = useCallback(() => setQueue((q) => q.slice(1)), []);
  const openItem = useCallback((item: TaggedItem) => {
    if (document.activeElement instanceof HTMLElement && !previousFocus.current) {
      previousFocus.current = document.activeElement;
    }
    setQueue((q) => [item, ...q.filter((x) => x.uid !== item.uid)]);
    setIsListOpen(false);
  }, []);
  const applyItems = useCallback(
    (key: string, fresh: AttentionItem[]) => {
      const s = sourcesRef.current.find((x) => x.key === key);
      if (s) onLoaded(s, fresh, false);
    },
    [onLoaded],
  );
  const clientFor = useCallback(
    (key: string) => sourcesRef.current.find((s) => s.key === key)?.client ?? null,
    [],
  );
  const recheckSource = useCallback(
    async (key: string) => {
      const s = sourcesRef.current.find((x) => x.key === key);
      if (!s) throw new Error("This project is no longer open.");
      const res = await s.client.attention();
      // the project may have closed while it was read
      const now = sourcesRef.current.find((x) => x.key === key);
      if (now) onLoaded(now, res.items, false);
    },
    [onLoaded],
  );

  const counts = useMemo(() => {
    const c: Record<string, number> = {};
    for (const i of items) c[i.sourceKey] = (c[i.sourceKey] ?? 0) + 1;
    return c;
  }, [items]);

  const watchers = (
    <>
      {sources.map((s) => (
        <SourceWatcher
          key={s.key}
          source={s}
          onLoaded={onLoaded}
          onEvent={onEvent}
          hasPrompt={items.some((i) => i.sourceKey === s.key && i.prompt)}
        />
      ))}
    </>
  );

  return {
    waitingItems: items,
    resolvedItems: resolved,
    resolveItem,
    applyItems,
    recheckSource,
    currentItem: queue[0] ?? null,
    dismissCurrent,
    openItem,
    isListOpen,
    openList: () => setIsListOpen(true),
    closeList: () => setIsListOpen(false),
    counts,
    clientFor,
    watchers,
  };
}
