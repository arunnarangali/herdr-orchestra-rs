import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { API_PREFIX, normalizeUrl, type Credentials } from "./client";
import { SseParser } from "./sse";
import {
  STREAM_EVENT_NAMES,
  type StreamEventName,
  type StreamMessage,
  type StreamPayloads,
} from "./types";

/**
 * `idle`: not started. `connecting`: first attempt. `connected`: the server accepted the
 * stream. `reconnecting`: it dropped or cannot be reached; trying again (with the last event
 * id, so nothing is missed). `refused`: the server answered and said no (wrong token): it
 * does not retry, because the same request would be refused again.
 */
export type StreamStatus = "idle" | "connecting" | "connected" | "reconnecting" | "refused";

export interface StreamRefusal {
  status: number;
}

export interface StreamOptions extends Credentials {
  onMessage: (message: StreamMessage) => void;
  onStatus: (status: StreamStatus, refusal: StreamRefusal | null) => void;
  /** For tests; defaults to the global `fetch`. */
  fetch?: typeof fetch;
  /** Waits between reconnect attempts; the last value repeats. */
  retryDelaysMs?: readonly number[];
}

const DEFAULT_RETRY_MS = [500, 1000, 2000, 4000, 8000];

function isEventName(name: string): name is StreamEventName {
  return (STREAM_EVENT_NAMES as readonly string[]).includes(name);
}

/** The stream URL: the token is the `token` query parameter (an event source sets no headers). */
export function streamUrl({ url, token }: Credentials): string {
  return `${normalizeUrl(url)}${API_PREFIX}/stream?token=${encodeURIComponent(token)}`;
}

function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) return resolve();
    const timer = setTimeout(done, ms);
    function done() {
      clearTimeout(timer);
      signal.removeEventListener("abort", done);
      resolve();
    }
    signal.addEventListener("abort", done);
  });
}

/**
 * Open `/api/v1/stream` and keep it open: after a drop it reconnects with `Last-Event-ID`
 * set to the last id it saw. Returns a function that closes it for good.
 */
export function openEventStream(options: StreamOptions): () => void {
  const doFetch = options.fetch ?? ((...a: Parameters<typeof fetch>) => fetch(...a));
  const delays = options.retryDelaysMs?.length ? options.retryDelaysMs : DEFAULT_RETRY_MS;
  const stop = new AbortController();
  let lastId: string | null = null;
  let status: StreamStatus = "idle";

  const set = (next: StreamStatus, refusal: StreamRefusal | null = null) => {
    if (stop.signal.aborted) return;
    status = next;
    options.onStatus(next, refusal);
  };

  async function readBody(body: ReadableStream<Uint8Array>): Promise<void> {
    const reader = body.getReader();
    const decoder = new TextDecoder();
    const parser = new SseParser();
    stop.signal.addEventListener("abort", () => void reader.cancel().catch(() => {}), {
      once: true,
    });
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return;
      for (const ev of parser.push(decoder.decode(value, { stream: true }))) {
        if (ev.id !== null) lastId = ev.id;
        if (!isEventName(ev.event)) continue;
        let data: unknown;
        try {
          data = JSON.parse(ev.data);
        } catch {
          continue;
        }
        if (stop.signal.aborted) return;
        options.onMessage({
          id: ev.id ?? String(lastId ?? ""),
          event: ev.event,
          data: data as StreamPayloads[typeof ev.event],
        } as StreamMessage);
      }
    }
  }

  async function run(): Promise<void> {
    set("connecting");
    for (let attempt = 0; !stop.signal.aborted; attempt++) {
      try {
        const headers: Record<string, string> = { Accept: "text/event-stream" };
        if (lastId !== null) headers["Last-Event-ID"] = lastId;
        const response = await doFetch(streamUrl(options), {
          headers,
          cache: "no-store",
          signal: stop.signal,
        });
        if (response.status >= 400 && response.status < 500 && response.status !== 429) {
          // The server answered and said no (401 wrong token, 403 not allowed): retrying
          // the same request cannot help.
          void response.body?.cancel().catch(() => {});
          set("refused", { status: response.status });
          return;
        }
        if (response.ok && response.body) {
          attempt = -1; // connected: the next drop starts the back-off again
          set("connected");
          await readBody(response.body);
        } else {
          void response.body?.cancel().catch(() => {});
        }
      } catch {
        // unreachable, or the connection broke mid-stream: reconnect below
      }
      if (stop.signal.aborted) return;
      if (status !== "reconnecting") set("reconnecting");
      await sleep(delays[Math.min(attempt < 0 ? 0 : attempt, delays.length - 1)] ?? 500, stop.signal);
    }
  }

  void run();
  return () => stop.abort();
}

// ------------------------------------------------------------------------------- the hook

/** How many messages of one event name the hook keeps (the newest). */
export const MESSAGES_KEPT = 200;

export type MessagesByEvent = {
  [K in StreamEventName]: Array<Extract<StreamMessage, { event: K }>>;
};

function emptyMessages(): MessagesByEvent {
  return { "task-state": [], "lead-event": [], lead: [], "run-state": [], chat: [], log: [], attention: [] };
}

export interface EventStream {
  status: StreamStatus;
  refusal: StreamRefusal | null;
  /** Messages received since the stream was opened, oldest first, by event name. */
  messages: MessagesByEvent;
  /** Call `listener` for every new message (for code that reloads data on an event). */
  subscribe: (listener: (message: StreamMessage) => void) => () => void;
}

export interface UseEventStreamOptions {
  fetch?: typeof fetch;
  retryDelaysMs?: readonly number[];
}

/**
 * Connect to the live stream while `credentials` is set. The app owns one of these (see
 * `app/connection.tsx`); screens read it through `useStream()` instead of opening their own.
 */
export function useEventStream(
  credentials: Credentials | null,
  options: UseEventStreamOptions = {},
): EventStream {
  const [status, setStatus] = useState<StreamStatus>("idle");
  const [refusal, setRefusal] = useState<StreamRefusal | null>(null);
  const [messages, setMessages] = useState<MessagesByEvent>(emptyMessages);
  const listeners = useRef(new Set<(m: StreamMessage) => void>());
  // Options are read when the stream opens; a new array or function each render must not
  // reopen it.
  const optionsRef = useRef(options);
  optionsRef.current = options;
  const url = credentials?.url;
  const token = credentials?.token;

  useEffect(() => {
    setMessages(emptyMessages());
    setRefusal(null);
    if (url === undefined || token === undefined) {
      setStatus("idle");
      return;
    }
    return openEventStream({
      url,
      token,
      fetch: optionsRef.current.fetch,
      retryDelaysMs: optionsRef.current.retryDelaysMs,
      onStatus: (s, r) => {
        setStatus(s);
        setRefusal(r);
      },
      onMessage: (m) => {
        setMessages((prev) => {
          const list = [...prev[m.event], m].slice(-MESSAGES_KEPT);
          return { ...prev, [m.event]: list } as MessagesByEvent;
        });
        for (const l of listeners.current) l(m);
      },
    });
  }, [url, token]);

  const subscribe = useCallback((listener: (m: StreamMessage) => void) => {
    listeners.current.add(listener);
    return () => {
      listeners.current.delete(listener);
    };
  }, []);

  return useMemo(
    () => ({ status, refusal, messages, subscribe }),
    [status, refusal, messages, subscribe],
  );
}
