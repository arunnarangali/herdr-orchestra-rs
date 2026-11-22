import { useCallback, useEffect, useRef, useState } from "react";
import { isApiError } from "../../api/errors";
import type { ChatBody, LeadState, LeadView } from "../../api/types";
import { useConnection } from "../../app/connection";
import { useProjects } from "../../app/projects";
import { savedLeadAgent } from "./defaultAgent";

/** What the Lead is doing as the page sees it. `unknown`: the server could not say (the old chat flow applies). */
export type LeadPhase = "loading" | "unknown" | LeadState;

/** An agent kind that is installed here and can take the Lead role. */
export interface LeadAgent {
  kind: string;
  name: string;
}

export type StartResult = { ok: true } | { ok: false; text: string };

export interface LeadControl {
  phase: LeadPhase;
  view: LeadView | null;
  /** Installed agents that can lead; null while that is being read. */
  agents: LeadAgent[] | null;
  /** The project's saved default, installed or not. */
  savedAgent: string | null;
  /** The saved default is set but not installed here. */
  savedMissing: boolean;
  /** The developer's pick when no installed default exists. */
  choice: string;
  setChoice: (kind: string) => void;
  /** The agent a start would use now, or null when the developer must choose first. */
  agentToUse: LeadAgent | null;
  /** Start has been asked and the state has not changed yet (also true while the request is in flight). */
  starting: boolean;
  startError: string | null;
  /** The Lead's state could not be read for a reason worth showing (not "no such route"). */
  readError: string | null;
  /** A message typed now has to start the Lead (there is none, or it went away). */
  mustStart: boolean;
  /** Ask for the Lead to start, once: a second call while one is open does nothing. */
  start: (message?: string | null) => Promise<StartResult>;
  reload: () => void;
  /** Answer the prompt the Lead is blocked at with one of its options; rejects with the server's error. */
  answerPrompt: (option: string) => Promise<LeadView>;
  /** Read where the Lead stands now and show it (e.g. whether its prompt went); rejects when it could not be read. */
  recheck: () => Promise<LeadView>;
}

/** Plain words for a start that cannot be asked yet. */
export const CHOOSE_AGENT_TEXT = "Choose which agent starts the Lead first.";
export const NO_AGENT_TEXT = "No agent CLI that can lead is installed on this machine, so the Lead cannot start. See Settings.";

const FIELDS = [
  ["Feature", "feature"],
  ["Why", "why"],
  ["Must", "must"],
  ["Must not", "must_not"],
  ["Already decided", "decided"],
  ["Done when", "done_when"],
  ["Mode", "mode"],
] as const;

/** The words the Lead is given for a message: the text as written, or a feature request's fields. */
export function startMessageFor(body: ChatBody): string {
  if (body.request) {
    const lines = FIELDS.filter(([, k]) => (body.request?.[k] ?? "").trim() !== "").map(([label, k]) => `${label}: ${body.request?.[k]}`);
    return ["New feature request", ...lines].join("\n");
  }
  return body.text;
}

/**
 * The project's Lead: where it stands (`GET /lead`, kept live by the stream's `lead` event),
 * which agent a start would use, and starting it. The page only asks; the server starts the
 * agent, and only an agent that is installed here.
 */
export function useLead(): LeadControl {
  const { client, stream, health } = useConnection();
  const { selected } = useProjects();
  const projectPath = (selected ?? health?.project ?? null) || null;
  const { subscribe, status } = stream;

  const [view, setView] = useState<LeadView | null>(null);
  const [phase0, setPhase0] = useState<"loading" | "unknown" | "known">("loading");
  const [readError, setReadError] = useState<string | null>(null);
  const [agents, setAgents] = useState<LeadAgent[] | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [choice, setChoice] = useState("");
  const [pending, setPending] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);
  const open = useRef(false);

  // where the Lead stands: on mount, after a reconnect, on `reload`
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
    client.lead({ signal: abort.signal }).then(
      (v) => {
        if (abort.signal.aborted) return;
        setView(v);
        setPhase0("known");
        setReadError(null);
      },
      (e: unknown) => {
        if (abort.signal.aborted) return;
        // a server without the Lead routes (404) is not a problem to show: chat works as before
        setPhase0("unknown");
        setReadError(isApiError(e) && e.status === 404 ? null : isApiError(e) ? e.message : String(e));
      },
    );
    return () => abort.abort();
  }, [client, nonce]);

  useEffect(
    () =>
      subscribe((m) => {
        if (m.event !== "lead") return;
        setView(m.data);
        setPhase0("known");
        setReadError(null);
        if (m.data.state === "starting" || m.data.state === "running") setStartError(null);
      }),
    [subscribe],
  );

  // which agents are installed, and which one the project saved
  useEffect(() => {
    if (!client) return;
    const abort = new AbortController();
    client.agents({ signal: abort.signal }).then(
      (r) => {
        if (abort.signal.aborted) return;
        setAgents(
          r.agents
            .filter((a) => a.installed && a.roles.includes("lead"))
            .map((a) => ({ kind: a.kind, name: a.display_name?.trim() || a.kind })),
        );
      },
      () => {
        if (!abort.signal.aborted) setAgents([]);
      },
    );
    return () => abort.abort();
  }, [client, nonce]);
  useEffect(() => {
    let live = true;
    void savedLeadAgent(projectPath).then((a) => {
      if (live) setSaved(a);
    });
    return () => {
      live = false;
    };
  }, [projectPath]);

  const savedAgent = agents?.find((a) => a.kind === saved) ?? null;
  const picked = agents?.find((a) => a.kind === choice) ?? null;
  const agentToUse = savedAgent ?? picked;
  const agentRef = useRef<LeadAgent | null>(null);
  agentRef.current = agentToUse;

  const state = view?.state;
  const phase: LeadPhase =
    phase0 === "loading"
      ? "loading"
      : phase0 === "unknown" || !view
        ? "unknown"
        : state === "none" || state === "starting" || state === "running" || state === "gone"
          ? state
          : "unknown";
  const starting = pending || phase === "starting";

  const start = useCallback(
    async (message?: string | null): Promise<StartResult> => {
      if (open.current) return { ok: false, text: "The Lead is already starting." };
      const agent = agentRef.current;
      if (!client) return { ok: false, text: "Not connected to a project." };
      if (!agent) return { ok: false, text: CHOOSE_AGENT_TEXT };
      open.current = true;
      setPending(true);
      setStartError(null);
      try {
        const v = await client.startLead({ agent: agent.kind, message: message ?? null });
        setView(v);
        setPhase0("known");
        return { ok: true };
      } catch (e) {
        const text = isApiError(e) ? e.message : String(e);
        setStartError(text);
        if (isApiError(e) && e.code === "lead_running") setNonce((n) => n + 1);
        return { ok: false, text };
      } finally {
        open.current = false;
        setPending(false);
      }
    },
    [client],
  );

  const answerPrompt = useCallback(
    async (option: string): Promise<LeadView> => {
      if (!client) throw new Error("Not connected to a project.");
      const v = await client.answerLeadPrompt({ option });
      setView(v);
      setPhase0("known");
      return v;
    },
    [client],
  );

  const recheck = useCallback(async (): Promise<LeadView> => {
    if (!client) throw new Error("Not connected to a project.");
    const v = await client.lead();
    setView(v);
    setPhase0("known");
    return v;
  }, [client]);

  return {
    answerPrompt,
    recheck,
    phase,
    view,
    agents,
    savedAgent: saved,
    savedMissing: Boolean(saved) && agents !== null && !savedAgent,
    choice,
    setChoice,
    agentToUse,
    starting,
    startError,
    readError,
    mustStart: phase === "none" || phase === "gone",
    start,
    reload: () => setNonce((n) => n + 1),
  };
}
