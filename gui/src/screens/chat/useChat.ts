import { useCallback, useEffect, useRef, useState } from "react";
import type { ChatBody, ChatMessage } from "../../api/types";
import { useConnection } from "../../app/connection";
import { useSendChat, type SendResult } from "../../components/useSendChat";

/** The server's wording for a project without a run. */
const NO_RUN_TEXT = /\bno run\b/i;
/** What a send says when the project has no run yet. */
export const NO_RUN_SEND =
  "Not sent: this project has no run yet, so there is no lead to take messages here. Once the lead is running for this project, send it again.";
import type { ApiError } from "../../api/errors";
import { isApiError } from "../../api/errors";

/** A message of the developer's that has not been stored yet, or could not be sent. */
export interface OutboxEntry {
  key: number;
  body: ChatBody;
  state: "sending" | "failed";
  error: string | null;
}

export interface Chat {
  state: "loading" | "ready" | "error";
  error: ApiError | null;
  /** The project has no run yet: a normal state, shown as an empty conversation, not an error. */
  noRun: boolean;
  /** The stored conversation, oldest first. */
  messages: ChatMessage[];
  outbox: OutboxEntry[];
  /** Set when the last send found no lead to take the message; cleared by the next good send. */
  leadProblem: string | null;
  /** Send from the composer: shown at once as "sending", kept as "failed" with a retry. */
  sendQueued: (body: ChatBody) => Promise<boolean>;
  retryOutbox: (key: number) => void;
  /** Send a stored message's content again (a `failed` one). */
  resend: (m: ChatMessage) => void;
  /** Send without the outbox (the form, the approval): the caller shows the result. */
  sendNow: ReturnType<typeof useSendChat>["send"];
  sending: boolean;
  reload: () => void;
  /** Called when a stored message came back from a send. */
  adopt: (m: ChatMessage) => void;
  clearLeadProblem: () => void;
}

/**
 * The conversation with the lead: loaded once, then kept live by the stream's `chat` events
 * (each one is a stored message or a change of its delivery state). After the stream
 * reconnects the whole conversation is read again, so nothing missed while it was down stays
 * missing. A message the stream delivered while a read was in flight wins over that read.
 */
export function useChat(): Chat {
  const { client, stream } = useConnection();
  const { send: rawSend, pending } = useSendChat();
  const [byId, setById] = useState<Map<number, ChatMessage>>(new Map());
  const [state, setState] = useState<Chat["state"]>("loading");
  const [error, setError] = useState<ApiError | null>(null);
  const [outbox, setOutbox] = useState<OutboxEntry[]>([]);
  const [leadProblem, setLeadProblem] = useState<string | null>(null);
  const [noRun, setNoRun] = useState(false);
  const noRunRef = useRef(false);
  noRunRef.current = noRun;

  // Without a run the server takes no chat messages (404 no_run) and its answer names a
  // terminal command. Say it in plain words instead; the message was not stored.
  const send = useCallback(
    async (body: ChatBody): Promise<SendResult> => {
      const r = await rawSend(body);
      if (!r.ok && r.kind === "error" && (noRunRef.current || NO_RUN_TEXT.test(r.text))) {
        setNoRun(true);
        return { ok: false, kind: "error", text: NO_RUN_SEND };
      }
      return r;
    },
    [rawSend],
  );
  const [nonce, setNonce] = useState(0);
  const { kind } = useConnection();
  const streamSeq = useRef(0);
  const streamedAt = useRef(new Map<number, number>());
  const keys = useRef(0);
  const subscribe = stream.subscribe;
  const status = stream.status;

  // The connection check keeps polling a project with no run; read again once a run exists.
  const wasNoRun = useRef(false);
  useEffect(() => {
    if (kind === "no-run") wasNoRun.current = true;
    else if (kind === "connected" && wasNoRun.current) {
      wasNoRun.current = false;
      setNonce((n) => n + 1);
    }
  }, [kind]);

  const merge = useCallback((list: ChatMessage[], skipStreamedAfter?: number) => {
    setById((prev) => {
      const next = new Map(prev);
      for (const m of list) {
        if (skipStreamedAfter !== undefined && (streamedAt.current.get(m.id) ?? 0) > skipStreamedAfter) continue;
        next.set(m.id, m);
      }
      return next;
    });
  }, []);

  // read the conversation: on mount, after a reconnect, and on `reload`
  const wasConnected = useRef(false);
  useEffect(() => {
    if (status === "connected") {
      if (wasConnected.current) setNonce((n) => n + 1);
      wasConnected.current = true;
    }
  }, [status]);
  useEffect(() => {
    if (!client) return;
    const abort = new AbortController();
    const started = streamSeq.current;
    client
      .chat({}, { signal: abort.signal })
      .then((r) => {
        if (abort.signal.aborted) return;
        merge(r.messages, started);
        setError(null);
        setNoRun(false);
        setState("ready");
      })
      .catch((e: unknown) => {
        if (abort.signal.aborted) return;
        if (isApiError(e) && e.isNoRun) {
          // Before the first run there is nothing stored: an empty conversation, not a failure.
          setError(null);
          setNoRun(true);
          setState("ready");
          return;
        }
        if (isApiError(e)) setError(e);
        setState((s) => (s === "ready" ? s : "error"));
      });
    return () => abort.abort();
  }, [client, nonce, merge]);

  useEffect(
    () =>
      subscribe((m) => {
        if (m.event !== "chat") return;
        streamSeq.current += 1;
        streamedAt.current.set(m.data.id, streamSeq.current);
        merge([m.data]);
      }),
    [subscribe, merge],
  );

  const attempt = useCallback(
    async (key: number, body: ChatBody): Promise<boolean> => {
      const r = await send(body);
      if (r.ok) {
        setOutbox((o) => o.filter((e) => e.key !== key));
        merge([r.message]);
        setLeadProblem(null);
        return true;
      }
      if (r.kind === "lead_unavailable") {
        // stored all the same: it shows up (queued) through the stream or the next read
        setOutbox((o) => o.filter((e) => e.key !== key));
        setLeadProblem(r.text);
        setNonce((n) => n + 1);
        return false;
      }
      setOutbox((o) => o.map((e) => (e.key === key ? { ...e, state: "failed", error: r.text } : e)));
      return false;
    },
    [send, merge],
  );

  const sendQueued = useCallback(
    (body: ChatBody) => {
      const key = ++keys.current;
      setOutbox((o) => [...o, { key, body, state: "sending", error: null }]);
      return attempt(key, body);
    },
    [attempt],
  );

  const retryOutbox = useCallback(
    (key: number) => {
      const entry = outbox.find((e) => e.key === key);
      if (!entry) return;
      setOutbox((o) => o.map((e) => (e.key === key ? { ...e, state: "sending", error: null } : e)));
      void attempt(key, entry.body);
    },
    [outbox, attempt],
  );

  const resend = useCallback(
    (m: ChatMessage) => {
      void sendQueued({ kind: m.kind, text: m.text, request: m.request });
    },
    [sendQueued],
  );

  const messages = [...byId.values()].sort((a, b) => a.id - b.id);
  return {
    state,
    error,
    noRun,
    messages,
    outbox,
    leadProblem,
    sendQueued,
    retryOutbox,
    resend,
    sendNow: send,
    sending: pending,
    reload: () => setNonce((n) => n + 1),
    adopt: (m) => merge([m]),
    clearLeadProblem: () => setLeadProblem(null),
  };
}
