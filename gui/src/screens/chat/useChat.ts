import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import type { ChatBody, ChatMessage } from "../../api/types";
import { useConnection } from "../../app/connection";
import { useProjects } from "../../app/projects";
import { useSendChat, type SendResult } from "../../components/useSendChat";
import { chatMemoryFor, UNKNOWN_PROJECT, type ChatSnapshot, type OutboxEntry } from "./chatMemory";
import { startMessageFor, type LeadControl } from "./useLead";

export type { OutboxEntry };

/** The server's wording for a project without a run. */
const NO_RUN_TEXT = /\bno run\b/i;
import type { ApiError } from "../../api/errors";
import { isApiError } from "../../api/errors";

type Inner = SendResult | { ok: false; kind: "no_run"; text: string };

/**
 * First messages that started the Lead are kept on this machine: with no run the server has no
 * conversation to put them in. They come back after a reload, ahead of the stored messages.
 */
const KEPT_MAX = 50;
function readStarted(key: string): ChatMessage[] {
  try {
    const raw = window.localStorage.getItem(key);
    const list = raw ? (JSON.parse(raw) as unknown) : [];
    return Array.isArray(list) ? (list as ChatMessage[]).filter((m) => typeof m?.id === "number" && m.id < 0) : [];
  } catch {
    return [];
  }
}
function rememberStarted(key: string, m: ChatMessage): void {
  try {
    window.localStorage.setItem(key, JSON.stringify([...readStarted(key), m].slice(-KEPT_MAX)));
  } catch {
    // storage full or off: the message still shows for this visit
  }
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
  sendNow: (body: ChatBody) => Promise<SendResult>;
  sending: boolean;
  reload: () => void;
  /** Called when a stored message came back from a send. */
  adopt: (m: ChatMessage) => void;
  clearLeadProblem: () => void;
  /** What is typed in the message box and not sent yet (kept while Chat is not on screen). */
  draft: string;
  setDraft: (text: string) => void;
}

/**
 * The conversation with the lead: loaded once, then kept live by the stream's `chat` events
 * (each one is a stored message or a change of its delivery state). After the stream
 * reconnects the whole conversation is read again, so nothing missed while it was down stays
 * missing. A message the stream delivered while a read was in flight wins over that read.
 */
export function useChat(lead?: LeadControl): Chat {
  const { client, stream } = useConnection();
  const { send: rawSend, pending } = useSendChat();
  // The conversation lives in the project's chat memory, not in this screen: see chatMemory.ts.
  const { selected } = useProjects();
  const { health } = useConnection();
  const projectKey = (selected ?? health?.project ?? "") || UNKNOWN_PROJECT;
  const memory = useMemo(() => chatMemoryFor(projectKey), [projectKey]);
  const snap = useSyncExternalStore(memory.subscribe, memory.get);
  const { byId, state, error, outbox, leadProblem, noRun } = snap;
  const setters = useMemo(() => {
    const patch =
      <K extends keyof ChatSnapshot>(key: K) =>
      (v: ChatSnapshot[K] | ((prev: ChatSnapshot[K]) => ChatSnapshot[K])) =>
        memory.update((s) => ({ [key]: typeof v === "function" ? (v as (p: ChatSnapshot[K]) => ChatSnapshot[K])(s[key]) : v }) as Partial<ChatSnapshot>);
    return {
      setById: patch("byId"),
      setState: patch("state"),
      setError: patch("error"),
      setOutbox: patch("outbox"),
      setLeadProblem: patch("leadProblem"),
      setNoRun: patch("noRun"),
      setDraft: patch("draft"),
    };
  }, [memory]);
  const { setById, setState, setError, setOutbox, setLeadProblem, setNoRun, setDraft } = setters;
  const noRunRef = useRef(false);
  noRunRef.current = noRun;
  // the stored conversation was read and a run exists (so the chat routes take messages)
  const runExistsRef = useRef(false);
  runExistsRef.current = state === "ready" && !noRun;

  // Without a run the server takes no chat messages (404 no_run) and its answer names a
  // terminal command. Say it in plain words instead; the message was not stored.
  const storeKey = `orch.chat.started:${projectKey}`;
  const storeKeyRef = useRef(storeKey);
  storeKeyRef.current = storeKey;
  const leadRef = useRef(lead);
  leadRef.current = lead;
  const send = useCallback(
    async (body: ChatBody): Promise<Inner> => {
      const l = leadRef.current;
      if (l?.mustStart) {
        // No Lead: the message goes in the start request (the chat routes need a run) and is
        // kept on this machine, shown as sent to the Lead it started.
        if (body.kind === "plan_approval") {
          return { ok: false, kind: "lead_unavailable", text: "The Lead is not running, so there is nobody to approve the plan for." };
        }
        if (runExistsRef.current) {
          // A run exists, so the server has a conversation: start the Lead on its own, and post
          // the message to the chat route so it is stored there (what the Lead reads with
          // `orch chat`, shared with every screen) and delivered once the Lead is up.
          const started = await l.start(null);
          if (!started.ok) return { ok: false, kind: "error", text: started.text };
          return rawSend(body);
        }
        const r = await l.start(startMessageFor(body));
        if (!r.ok) return { ok: false, kind: "error", text: r.text };
        const message: ChatMessage = {
          // negative and rising with time: ahead of every stored message, in the order they were sent
          id: Date.now() - 2e13,
          ts: new Date().toISOString(),
          sender: "developer",
          kind: body.kind,
          text: body.text,
          request: body.request ?? null,
          about: null,
          delivery: "sent",
          delivery_note: "Passed to the Lead as it started.",
        };
        rememberStarted(storeKeyRef.current, message);
        return { ok: true, message };
      }
      if (l?.starting && !runExistsRef.current) {
        return { ok: false, kind: "error", text: "The Lead is starting. Send it again once it is running." };
      }
      const r = await rawSend(body);
      if (!r.ok && r.kind === "error" && (noRunRef.current || NO_RUN_TEXT.test(r.text))) {
        setNoRun(true);
        return { ok: false, kind: "no_run", text: r.text };
      }
      return r;
    },
    [rawSend, setNoRun],
  );
  // for the form and the approval, which show the result themselves
  const sendNow = useCallback(
    async (body: ChatBody): Promise<SendResult> => {
      const r = await send(body);
      if (!r.ok && r.kind === "no_run") {
        return { ok: false, kind: "error", text: "The Lead is still setting up this project's run. Send this again in a moment." };
      }
      return r;
    },
    [send],
  );
  const [nonce, setNonce] = useState(0);
  const { kind } = useConnection();
  const streamSeq = useRef(0);
  const streamedAt = useRef(new Map<number, number>());
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

  useEffect(() => {
    const kept = readStarted(storeKey);
    if (kept.length > 0) setById((prev) => new Map([...prev, ...kept.map((m) => [m.id, m] as const)]));
  }, [storeKey]);

  const merge = useCallback((list: ChatMessage[], skipStreamedAfter?: number) => {
    setById((prev) => {
      const next = new Map(prev);
      for (const m of list) {
        if (skipStreamedAfter !== undefined && (streamedAt.current.get(m.id) ?? 0) > skipStreamedAfter) continue;
        next.set(m.id, m);
      }
      return next;
    });
  }, [setById]);

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
  }, [client, nonce, merge, setError, setNoRun, setState]);

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
      if (r.kind === "no_run") {
        // No run yet, so no chat route: hold it and send it by itself once the run exists.
        setOutbox((o) => o.map((e) => (e.key === key ? { ...e, state: "waiting", error: null } : e)));
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
    [send, merge, setOutbox, setLeadProblem],
  );

  // a held message goes out as soon as the run exists
  useEffect(() => {
    if (noRun) return;
    const held = outbox.filter((e) => e.state === "waiting");
    if (held.length === 0) return;
    setOutbox((o) => o.map((e) => (e.state === "waiting" ? { ...e, state: "sending" } : e)));
    for (const e of held) void attempt(e.key, e.body);
  }, [noRun, outbox, attempt, setOutbox]);

  const sendQueued = useCallback(
    (body: ChatBody) => {
      const key = memory.nextKey();
      setOutbox((o) => [...o, { key, body, state: "sending", error: null }]);
      return attempt(key, body);
    },
    [attempt, memory],
  );

  const retryOutbox = useCallback(
    (key: number) => {
      const entry = outbox.find((e) => e.key === key);
      if (!entry) return;
      setOutbox((o) => o.map((e) => (e.key === key ? { ...e, state: "sending", error: null } : e)));
      void attempt(key, entry.body);
    },
    [outbox, attempt, setOutbox],
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
    sendNow,
    sending: pending,
    reload: () => setNonce((n) => n + 1),
    adopt: (m) => merge([m]),
    clearLeadProblem: () => setLeadProblem(null),
    draft: snap.draft,
    setDraft,
  };
}
