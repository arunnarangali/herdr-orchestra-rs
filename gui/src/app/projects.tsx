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
import { createApiClient, type ApiClient, type Credentials } from "../api/client";
import { useEventStream, type EventStream, type StreamStatus } from "../api/stream";
import { useAttentionStore, type AttentionSource, type AttentionStore } from "../components/attentionStore";
import { getBackend, type AppBackend, type RestoredProject } from "../screens/projects/backend";

/** One open project: its own server, and where starting it stands. */
export interface OpenProject {
  /** The project's folder: its identity everywhere in the app. */
  path: string;
  name: string;
  status: "starting" | "ready" | "failed";
  /** Where its server listens and its token: only when `ready`. */
  credentials: Credentials | null;
  /** Why the server did not start, when `failed`. */
  error: string | null;
  /** Grows each time the project's server is (re)started, so nothing of the old one survives. */
  generation: number;
}

/** What the app watches of each open project in the background (for the switcher and attention). */
export interface Runtime {
  client: ApiClient;
  subscribe: EventStream["subscribe"];
  streamStatus: StreamStatus;
  /** `null` until `/health` answered. */
  hasRun: boolean | null;
  /** The run's state from `/status` (`running`, `all_done`, …), when there is a run. */
  runState: string | null;
  /** The server stopped answering. */
  unreachable: boolean;
}

export interface ProjectsValue {
  /** Several projects can be open (the desktop app); false in a browser or with fixed credentials. */
  multi: boolean;
  /** The open projects, in the order they were opened. */
  projects: OpenProject[];
  /** The project on screen. */
  selected: string | null;
  /** Reading the projects that were open last time. */
  restoring: boolean;
  runtimes: Record<string, Runtime>;
  /** Attention items of every open project (multi only). */
  attention: AttentionStore | null;
  select: (path: string) => void;
  /** Add a folder to the list (it must be a git repository) and open it. */
  openFolder: (path: string) => Promise<void>;
  /** Open a project that is in the list. */
  openListed: (path: string) => Promise<void>;
  close: (path: string) => Promise<void>;
  /** Start a failed project's server again. */
  retry: (path: string) => Promise<void>;
  /** Make a new project in a folder and open it. Resolves with what was done. */
  create: (path: string) => Promise<string>;
  /** What the last open, close or create did. Kept here because the Projects screen is rebuilt when the project on screen changes. */
  notice: { kind: "ok" | "error"; text: string } | null;
  clearNotice: () => void;
}

const none: ProjectsValue = {
  multi: false,
  projects: [],
  selected: null,
  restoring: false,
  runtimes: {},
  attention: null,
  select: () => {},
  openFolder: async () => {},
  openListed: async () => {},
  close: async () => {},
  retry: async () => {},
  create: async () => "",
  notice: null,
  clearNotice: () => {},
};

const Ctx = createContext<ProjectsValue>(none);

/** The open projects and which one is on screen. Outside a provider: no projects, not multi. */
export function useProjects(): ProjectsValue {
  return useContext(Ctx);
}

const nameOf = (path: string) => path.replace(/\/+$/, "").split("/").pop() || path;
const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export interface ProjectsProviderProps {
  children: (props: { projectKey: string; credentials: Credentials | undefined; projectPath: string | undefined }) => ReactNode;
  /** Fixed credentials (tests, a browser): the app then has one server and no project list. */
  initial?: Credentials;
  fetch?: typeof fetch;
  retryDelaysMs?: readonly number[];
  pollMs?: number;
}

/**
 * Owns the set of open projects when the desktop shell can run one server per project. Each
 * open project gets a background watcher (its own client and stream); the project on screen
 * additionally gets a connection that is rebuilt from nothing whenever the selection or the
 * project's server changes, so no request, stream or state of one project can show in another.
 */
export function ProjectsProvider({ children, initial, fetch: fetchImpl, retryDelaysMs, pollMs = 3000 }: ProjectsProviderProps) {
  const backend = useMemo<AppBackend | null>(() => (initial ? null : getBackend()), [initial]);
  const multi = backend !== null && typeof backend.openProject === "function";

  const [projects, setProjects] = useState<OpenProject[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [restoring, setRestoring] = useState(multi);
  const [runtimes, setRuntimes] = useState<Record<string, Runtime>>({});
  const generation = useRef(0);
  const [notice, setNotice] = useState<ProjectsValue["notice"]>(null);
  const clearNotice = useCallback(() => setNotice(null), []);

  const patch = useCallback((path: string, change: Partial<OpenProject>) => {
    setProjects((ps) => ps.map((p) => (p.path === path ? { ...p, ...change } : p)));
  }, []);

  const fromRestored = useCallback(
    (r: RestoredProject): OpenProject => ({
      path: r.path,
      name: r.name,
      status: r.connection ? "ready" : "failed",
      credentials: r.connection,
      error: r.error,
      generation: ++generation.current,
    }),
    [],
  );

  // Reopen what was open last time. Each project reports on its own.
  useEffect(() => {
    if (!multi || !backend) return;
    let live = true;
    Promise.all([backend.restoreProjects(), backend.listProjects().catch(() => null)]).then(
      ([restored, list]) => {
        if (!live) return;
        const open = restored.map(fromRestored);
        setProjects(open);
        const wanted = list?.active && open.some((p) => p.path === list.active) ? list.active : null;
        setSelected(wanted ?? open.find((p) => p.status === "ready")?.path ?? open[0]?.path ?? null);
        setRestoring(false);
      },
      () => {
        if (live) setRestoring(false);
      },
    );
    return () => {
      live = false;
    };
  }, [multi, backend, fromRestored]);

  const select = useCallback(
    (path: string) => {
      setSelected(path);
      void backend?.switchProject(path).catch(() => {});
    },
    [backend],
  );

  const start = useCallback(
    async (path: string, call: (b: AppBackend) => Promise<Credentials>) => {
      if (!backend) return;
      setProjects((ps) =>
        ps.some((p) => p.path === path)
          ? ps.map((p) => (p.path === path ? { ...p, status: "starting", error: null, credentials: null } : p))
          : [...ps, { path, name: nameOf(path), status: "starting", credentials: null, error: null, generation: ++generation.current }],
      );
      try {
        const credentials = await call(backend);
        patch(path, { status: "ready", credentials, error: null, generation: ++generation.current });
      } catch (e) {
        patch(path, { status: "failed", credentials: null, error: message(e), generation: ++generation.current });
      }
    },
    [backend, patch],
  );

  const openListed = useCallback(
    async (path: string) => {
      if (projects.some((p) => p.path === path && p.status === "ready")) return setSelected(path);
      setSelected(path);
      await start(path, (b) => b.openProject(path));
    },
    [projects, start],
  );

  const openFolder = useCallback(
    async (path: string) => {
      if (!backend) return;
      await backend.addProject(path); // refuses a folder that is not a git repository
      await openListed(path);
      setNotice({ kind: "ok", text: `Added and opened ${path}.` });
    },
    [backend, openListed],
  );

  const retry = useCallback(async (path: string) => start(path, (b) => b.retryProject(path)), [start]);

  const close = useCallback(
    async (path: string) => {
      if (!backend) return;
      await backend.closeProject(path);
      setNotice({ kind: "ok", text: `Closed ${nameOf(path)}: its server was stopped. The project stays in the list.` });
      setProjects((ps) => ps.filter((p) => p.path !== path));
      setSelected((cur) => {
        if (cur !== path) return cur;
        const rest = projects.filter((p) => p.path !== path);
        return rest[0]?.path ?? null;
      });
    },
    [backend, projects],
  );

  const create = useCallback(
    async (path: string) => {
      if (!backend) return "";
      const r = await backend.createProject(path);
      const p = r.created.path;
      setProjects((ps) => [
        ...ps.filter((x) => x.path !== p),
        { path: p, name: nameOf(p), status: "ready", credentials: r.connection, error: null, generation: ++generation.current },
      ]);
      setSelected(p);
      void backend.switchProject(p).catch(() => {});
      const bits = [
        r.created.made_folder ? "created the folder" : null,
        r.created.git_initialised ? "set up a git repository" : null,
        "prepared it for a run",
      ].filter(Boolean);
      const text = `Created ${nameOf(p)}: ${bits.join(", ")}, and opened it.`;
      setNotice({ kind: "ok", text });
      return text;
    },
    [backend],
  );

  // Every open project's watcher reports here.
  const report = useCallback((path: string, runtime: Runtime | null) => {
    setRuntimes((prev) => {
      if (runtime === null) {
        if (!(path in prev)) return prev;
        const { [path]: _gone, ...rest } = prev;
        return rest;
      }
      const old = prev[path];
      if (
        old &&
        old.client === runtime.client &&
        old.subscribe === runtime.subscribe &&
        old.streamStatus === runtime.streamStatus &&
        old.hasRun === runtime.hasRun &&
        old.runState === runtime.runState &&
        old.unreachable === runtime.unreachable
      ) {
        return prev;
      }
      return { ...prev, [path]: runtime };
    });
  }, []);

  const sources = useMemo<AttentionSource[]>(
    () =>
      projects
        .filter((p) => runtimes[p.path]?.hasRun)
        .map((p) => ({ key: p.path, name: p.name, client: runtimes[p.path]!.client, subscribe: runtimes[p.path]!.subscribe })),
    [projects, runtimes],
  );
  const attention = useAttentionStore(multi ? sources : []);

  const value = useMemo<ProjectsValue>(
    () =>
      multi
        ? { multi, projects, selected, restoring, runtimes, attention, select, openFolder, openListed, close, retry, create, notice, clearNotice }
        : none,
    [multi, projects, selected, restoring, runtimes, attention, select, openFolder, openListed, close, retry, create, notice, clearNotice],
  );

  const current = projects.find((p) => p.path === selected) ?? null;
  const ready = current?.status === "ready" ? current : null;
  return (
    <Ctx.Provider value={value}>
      {multi && projects.filter((p) => p.status === "ready").map((p) => (
        <RuntimeHost
          key={`${p.path}#${p.generation}`}
          project={p}
          fetchImpl={fetchImpl}
          retryDelaysMs={retryDelaysMs}
          pollMs={pollMs}
          report={report}
        />
      ))}
      {multi && attention.watchers}
      {/* The project on screen: rebuilt from nothing when the selection or its server changes. */}
      {children({
        projectKey: multi ? `${current?.path ?? "-"}#${ready?.generation ?? 0}` : "single",
        credentials: ready?.credentials ?? undefined,
        projectPath: multi ? (current?.path ?? undefined) : undefined,
      })}
    </Ctx.Provider>
  );
}

const RUN_REFRESH_MS = 300;

/** Watches one open project in the background: does it have a run, what state, is it reachable. */
function RuntimeHost({
  project,
  fetchImpl,
  retryDelaysMs,
  pollMs,
  report,
}: {
  project: OpenProject;
  fetchImpl?: typeof fetch;
  retryDelaysMs?: readonly number[];
  pollMs: number;
  report: (path: string, runtime: Runtime | null) => void;
}) {
  const url = project.credentials?.url ?? "";
  const token = project.credentials?.token ?? "";
  const credentials = useMemo(() => ({ url, token }), [url, token]);
  const client = useMemo(() => createApiClient({ ...credentials, fetch: fetchImpl }), [credentials, fetchImpl]);
  const [hasRun, setHasRun] = useState<boolean | null>(null);
  const [unreachable, setUnreachable] = useState(false);
  const [runState, setRunState] = useState<string | null>(null);
  const stream = useEventStream(hasRun ? credentials : null, { fetch: fetchImpl, retryDelaysMs });
  const subscribe = stream.subscribe;

  // `/health` until there is a run (the stream only exists then); keep trying while it is down.
  useEffect(() => {
    const abort = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const ask = async () => {
      try {
        const h = await client.health({ signal: abort.signal });
        setUnreachable(false);
        setHasRun(h.run);
        if (h.run) return;
      } catch {
        if (abort.signal.aborted) return;
        setUnreachable(true);
      }
      if (!abort.signal.aborted) timer = setTimeout(() => void ask(), pollMs);
    };
    void ask();
    return () => {
      abort.abort();
      clearTimeout(timer);
    };
  }, [client, pollMs]);

  // The run's state: now, and after every change the stream reports.
  useEffect(() => {
    if (!hasRun) return;
    const abort = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const read = () =>
      client.status({ signal: abort.signal }).then(
        (s) => {
          if (!abort.signal.aborted) setRunState(s.run_state);
        },
        () => {},
      );
    void read();
    const off = subscribe((m) => {
      if (m.event !== "task-state" && m.event !== "run-state") return;
      clearTimeout(timer);
      timer = setTimeout(() => void read(), RUN_REFRESH_MS);
    });
    return () => {
      abort.abort();
      clearTimeout(timer);
      off();
    };
  }, [client, hasRun, subscribe]);

  const streamStatus = stream.status;
  const unreachableNow = unreachable || streamStatus === "reconnecting";
  useEffect(() => {
    report(project.path, { client, subscribe, streamStatus, hasRun, runState, unreachable: unreachableNow });
  }, [report, project.path, client, subscribe, streamStatus, hasRun, runState, unreachableNow]);
  useEffect(() => () => report(project.path, null), [report, project.path]);
  return null;
}
