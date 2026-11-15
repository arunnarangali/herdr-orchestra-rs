import { ApiError, errorFromResponse } from "./errors";
import type {
  ActionResponse,
  AgentsResponse,
  AnswerBody,
  ChatBody,
  ChatListResponse,
  ChatMessage,
  DispatchBody,
  EscalateBody,
  EventsResponse,
  FixBody,
  Health,
  LogsResponse,
  ModelsResponse,
  OpenApiDocument,
  PlanResponse,
  ReportResponse,
  RunControlResponse,
  SetStatusBody,
  StatusResponse,
  TaskDetailResponse,
  TasksResponse,
} from "./types";

export const API_PREFIX = "/api/v1";

export interface Credentials {
  /** Where `orch serve` listens, e.g. `http://127.0.0.1:43211`. */
  url: string;
  token: string;
}

export interface ClientOptions extends Credentials {
  /** For tests; defaults to the global `fetch`. */
  fetch?: typeof fetch;
}

type Query = Record<string, string | number | undefined>;

export interface RequestOptions {
  signal?: AbortSignal;
}

/** Normalise what the user typed: no trailing slash, and `http://` when no scheme was given. */
export function normalizeUrl(input: string): string {
  const trimmed = input.trim().replace(/\/+$/, "");
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`;
}

export function createApiClient(options: ClientOptions) {
  const base = normalizeUrl(options.url);
  const token = options.token;
  const doFetch = options.fetch ?? ((...a: Parameters<typeof fetch>) => fetch(...a));

  async function request<T>(
    method: "GET" | "POST",
    path: string,
    { query, body, signal }: { query?: Query; body?: unknown; signal?: AbortSignal } = {},
  ): Promise<T> {
    const search = new URLSearchParams();
    for (const [k, v] of Object.entries(query ?? {})) {
      if (v !== undefined) search.set(k, String(v));
    }
    const qs = search.size > 0 ? `?${search}` : "";
    const headers: Record<string, string> = {
      Authorization: `Bearer ${token}`,
      Accept: "application/json",
    };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    let response: Response;
    try {
      response = await doFetch(`${base}${API_PREFIX}${path}${qs}`, {
        method,
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
        signal,
      });
    } catch (e) {
      if (signal?.aborted) throw e;
      throw new ApiError(
        "network",
        `cannot reach the server at ${base}: is \`orch serve\` running?`,
        0,
      );
    }
    if (!response.ok) throw await errorFromResponse(response);
    try {
      return (await response.json()) as T;
    } catch {
      throw new ApiError("bad_response", "the server's answer was not JSON", response.status);
    }
  }

  const get = <T>(path: string, o?: RequestOptions, query?: Query) =>
    request<T>("GET", path, { query, signal: o?.signal });
  const post = <T>(path: string, body: unknown, o?: RequestOptions) =>
    request<T>("POST", path, { body: body ?? {}, signal: o?.signal });
  const task = (id: string) => `/tasks/${encodeURIComponent(id)}`;

  return {
    baseUrl: base,

    // -- system
    health: (o?: RequestOptions) => get<Health>("/health", o),
    openapi: (o?: RequestOptions) => get<OpenApiDocument>("/openapi.json", o),

    // -- reads
    status: (o?: RequestOptions) => get<StatusResponse>("/status", o),
    plan: (o?: RequestOptions) => get<PlanResponse>("/plan", o),
    tasks: (o?: RequestOptions) => get<TasksResponse>("/tasks", o),
    task: (id: string, o?: RequestOptions) => get<TaskDetailResponse>(task(id), o),
    events: (q: { since?: number; limit?: number } = {}, o?: RequestOptions) =>
      get<EventsResponse>("/events", o, q),
    report: (o?: RequestOptions) => get<ReportResponse>("/report", o),
    agents: (o?: RequestOptions) => get<AgentsResponse>("/agents", o),
    models: (o?: RequestOptions) => get<ModelsResponse>("/models", o),
    logs: (q: { lines?: number; milestone?: string; agent?: string } = {}, o?: RequestOptions) =>
      get<LogsResponse>("/logs", o, q),

    // -- chat with the lead (stored by the server; the event manager delivers it)
    chat: (q: { after?: number } = {}, o?: RequestOptions) => get<ChatListResponse>("/chat", o, q),
    sendChat: (body: ChatBody, o?: RequestOptions) => post<ChatMessage>("/chat", body, o),

    // -- actions (the event manager carries them out; the answer says whether it is running)
    dispatch: (body: DispatchBody, o?: RequestOptions) =>
      post<ActionResponse>("/dispatch", body, o),
    answer: (id: string, body: AnswerBody, o?: RequestOptions) =>
      post<ActionResponse>(`${task(id)}/answer`, body, o),
    approve: (id: string, o?: RequestOptions) => post<ActionResponse>(`${task(id)}/approve`, {}, o),
    escalate: (id: string, body: EscalateBody = {}, o?: RequestOptions) =>
      post<ActionResponse>(`${task(id)}/escalate`, body, o),
    fix: (id: string, body: FixBody, o?: RequestOptions) =>
      post<ActionResponse>(`${task(id)}/fix`, body, o),
    setStatus: (id: string, body: SetStatusBody, o?: RequestOptions) =>
      post<ActionResponse>(`${task(id)}/status`, body, o),
    pauseRun: (o?: RequestOptions) => post<RunControlResponse>("/run/pause", {}, o),
    resumeRun: (o?: RequestOptions) => post<RunControlResponse>("/run/resume", {}, o),
    stopRun: (o?: RequestOptions) => post<RunControlResponse>("/run/stop", {}, o),
  };
}

export type ApiClient = ReturnType<typeof createApiClient>;
