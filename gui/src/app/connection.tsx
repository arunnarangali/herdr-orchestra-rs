import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { createApiClient, normalizeUrl, type ApiClient, type Credentials } from "../api/client";
import { isApiError } from "../api/errors";
import { useEventStream, type EventStream } from "../api/stream";
import type { Health } from "../api/types";
import { ProjectsProvider } from "./projects";
import { detectShell, type Shell } from "./shell";

/**
 * What the app knows about its server, in the words the frame shows.
 * - `unconfigured`: no project chosen yet (no address and token).
 * - `checking`: asked, no answer yet.
 * - `no-server`: nothing answers at the address, or the live stream dropped.
 * - `bad-token`: the server answered 401.
 * - `no-run`: the server is fine but the project has no run yet.
 * - `connected`: everything works.
 */
export type ConnectionKind =
  | "unconfigured"
  | "checking"
  | "no-server"
  | "bad-token"
  | "no-run"
  | "connected";

type Reach = "unknown" | "ok" | "no-server" | "bad-token";

export interface Connection {
  kind: ConnectionKind;
  /** The address and token, in memory only. */
  credentials: Credentials | null;
  /** The one typed client; null until a project is chosen. */
  client: ApiClient | null;
  /** What `/health` said last (kept while the server is unreachable). */
  health: Health | null;
  /** The one live stream the whole app shares. */
  stream: EventStream;
  /** The desktop shell, or null in a plain browser. */
  shell: Shell | null;
  /** Use this address and token. Kept in memory only. */
  connect: (credentials: Credentials) => void;
  /** Forget the address and token (the shell also stops the server it started). */
  disconnect: () => void;
  /** Ask the server again now. */
  recheck: () => void;
}

const Ctx = createContext<Connection | null>(null);

export interface ConnectionProviderProps {
  children: ReactNode;
  /** Defaults to the desktop shell when the page runs in one. */
  shell?: Shell | null;
  /** Start connected (tests). */
  initial?: Credentials;
  /** For tests. */
  fetch?: typeof fetch;
  retryDelaysMs?: readonly number[];
  /** How often to ask again while not connected; default 3000. */
  pollMs?: number;
  /** Pick up a server the shell already started (a reloaded window); off when projects are managed. */
  pickup?: boolean;
  /** The project this connection is for: `disconnect` stops only that project's server. */
  projectPath?: string;
}

/**
 * The one place that owns the connection: the address and token, the typed client, the
 * health check and the live stream. Screens read them through `useConnection`, `useApi` and
 * `useStream`; none opens its own.
 */
function SingleConnection({
  children,
  shell: shellProp,
  initial,
  fetch: fetchImpl,
  retryDelaysMs,
  pollMs = 3000,
  pickup = true,
  projectPath,
}: ConnectionProviderProps) {
  const shell = useMemo(() => (shellProp === undefined ? detectShell() : shellProp), [shellProp]);
  const [credentials, setCredentials] = useState<Credentials | null>(initial ?? null);
  const [reach, setReach] = useState<Reach>("unknown");
  const [health, setHealth] = useState<Health | null>(null);
  const [tick, setTick] = useState(0);

  // A server the shell already started (the window was reloaded): pick it up.
  useEffect(() => {
    if (!shell || initial || !pickup) return;
    let cancelled = false;
    shell
      .getConnection()
      .then((c) => {
        if (!cancelled && c) setCredentials(c);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [shell, initial, pickup]);

  const client = useMemo(
    () => (credentials ? createApiClient({ ...credentials, fetch: fetchImpl }) : null),
    [credentials, fetchImpl],
  );

  // Ask `/health`; keep asking while the answer is not "connected".
  const reachRef = useRef(reach);
  reachRef.current = reach;
  const streamStatusRef = useRef<EventStream["status"]>("idle");
  // A different server starts from nothing; asking again does not (the stream must keep
  // its place so it can resume with Last-Event-ID).
  useEffect(() => {
    setReach("unknown");
    setHealth(null);
  }, [client]);
  useEffect(() => {
    if (!client) return;
    const abort = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const ask = async () => {
      try {
        const h = await client.health({ signal: abort.signal });
        setHealth(h);
        setReach("ok");
      } catch (e) {
        if (abort.signal.aborted) return;
        setReach(isApiError(e) && e.isUnauthorized ? "bad-token" : "no-server");
      }
      if (abort.signal.aborted) return;
      // Once connected the stream reports a drop; until then, ask again.
      const settled = reachRef.current === "ok" && streamStatusRef.current === "connected";
      const refused = reachRef.current === "bad-token";
      if (!settled && !refused) timer = setTimeout(() => void ask(), pollMs);
    };
    void ask();
    return () => {
      abort.abort();
      clearTimeout(timer);
    };
  }, [client, pollMs, tick]);

  const streamCredentials = credentials && health?.run ? credentials : null;
  const stream = useEventStream(streamCredentials, { fetch: fetchImpl, retryDelaysMs });
  streamStatusRef.current = stream.status;

  // A dropped stream means the server may be gone: start asking again.
  useEffect(() => {
    if (stream.status === "reconnecting" || stream.status === "refused") setTick((t) => t + 1);
  }, [stream.status]);

  const kind: ConnectionKind = !credentials
    ? "unconfigured"
    : reach === "unknown"
      ? "checking"
      : reach === "no-server"
        ? "no-server"
        : reach === "bad-token" || stream.status === "refused"
          ? "bad-token"
          : !health?.run
            ? "no-run"
            : stream.status === "reconnecting"
              ? "no-server"
              : "connected";

  const connect = useCallback((c: Credentials) => {
    setCredentials({ url: normalizeUrl(c.url), token: c.token.trim() });
  }, []);
  const disconnect = useCallback(() => {
    setCredentials(null);
    void shell?.stopServer(projectPath).catch(() => {});
  }, [shell, projectPath]);
  const recheck = useCallback(() => setTick((t) => t + 1), []);

  const value = useMemo<Connection>(
    () => ({ kind, credentials, client, health, stream, shell, connect, disconnect, recheck }),
    [kind, credentials, client, health, stream, shell, connect, disconnect, recheck],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

/**
 * The connection of the app. With one server (a browser, or fixed credentials) it is that
 * server's. In the desktop app with several open projects it is the connection of the project
 * on screen, rebuilt from nothing (new client, new health check, new stream) whenever another
 * project is selected or that project's server restarts: nothing of one project's connection
 * can outlive the switch.
 */
export function ConnectionProvider(props: ConnectionProviderProps) {
  const { children, ...rest } = props;
  return (
    <ProjectsProvider initial={props.initial} fetch={props.fetch} retryDelaysMs={props.retryDelaysMs} pollMs={props.pollMs}>
      {({ projectKey, credentials, projectPath }) => {
        const multi = projectPath !== undefined || projectKey !== "single";
        return (
          <SingleConnection
            key={projectKey}
            {...rest}
            initial={multi ? credentials : props.initial}
            pickup={!multi}
            projectPath={projectPath}
          >
            {children}
          </SingleConnection>
        );
      }}
    </ProjectsProvider>
  );
}

export function useConnection(): Connection {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error("useConnection needs a ConnectionProvider");
  return ctx;
}

/** The typed client. Screens are only shown once a project is chosen, so it exists. */
export function useApi(): ApiClient {
  const { client } = useConnection();
  if (!client) throw new Error("useApi: no project chosen yet");
  return client;
}

/** The app's one live stream. */
export function useStream(): EventStream {
  return useConnection().stream;
}
