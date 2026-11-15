import { useCallback, useEffect, useRef, useState, type DependencyList } from "react";
import type { ApiClient } from "../api/client";
import { ApiError, isApiError } from "../api/errors";
import type { StreamEventName } from "../api/types";
import { useConnection } from "../app/connection";

export type ResourceState = "loading" | "ready" | "empty" | "error";

export interface Resource<T> {
  state: ResourceState;
  data: T | null;
  error: ApiError | null;
  /** A reload is in flight (the previous data stays on screen meanwhile). */
  refreshing: boolean;
  reload: () => void;
}

export interface ResourceOptions<T> {
  /** When this says the data is empty the state is `empty`, not `ready`. */
  isEmpty?: (data: T) => boolean;
  /** Reload (once the burst settles) whenever the live stream delivers one of these. */
  refreshOn?: readonly StreamEventName[];
  /** Milliseconds to wait for a burst of messages to settle; default 300. */
  refreshDelayMs?: number;
}

function asApiError(e: unknown): ApiError {
  return isApiError(e) ? e : new ApiError("bad_response", e instanceof Error ? e.message : String(e), 0);
}

/**
 * Load data through the app's client with loading, empty and error states, and optionally
 * keep it fresh from the live stream. `load` runs again when `deps` change or `reload()` is
 * called; a request that is no longer wanted is aborted.
 */
export function useResource<T>(
  load: (client: ApiClient, signal: AbortSignal) => Promise<T>,
  deps: DependencyList,
  options: ResourceOptions<T> = {},
): Resource<T> {
  const { client, stream } = useConnection();
  const subscribe = stream.subscribe; // stable: the stream object itself changes on every message
  const [state, setState] = useState<ResourceState>("loading");
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [nonce, setNonce] = useState(0);
  const loadRef = useRef(load);
  loadRef.current = load;
  const optionsRef = useRef(options);
  optionsRef.current = options;
  const hasData = useRef(false);

  useEffect(() => {
    if (!client) return;
    const abort = new AbortController();
    setRefreshing(true);
    loadRef
      .current(client, abort.signal)
      .then((d) => {
        if (abort.signal.aborted) return;
        hasData.current = true;
        setData(d);
        setError(null);
        setState(optionsRef.current.isEmpty?.(d) ? "empty" : "ready");
      })
      .catch((e: unknown) => {
        if (abort.signal.aborted) return;
        setError(asApiError(e));
        setState("error");
      })
      .finally(() => {
        if (!abort.signal.aborted) setRefreshing(false);
      });
    return () => abort.abort();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the caller's deps are the contract
  }, [client, nonce, ...deps]);

  const refreshOn = options.refreshOn;
  const delay = options.refreshDelayMs ?? 300;
  const names = refreshOn?.join(",") ?? "";
  useEffect(() => {
    if (!names) return;
    const wanted = names.split(",");
    let timer: ReturnType<typeof setTimeout> | undefined;
    const off = subscribe((m) => {
      if (!wanted.includes(m.event)) return;
      clearTimeout(timer);
      timer = setTimeout(() => setNonce((n) => n + 1), delay);
    });
    return () => {
      off();
      clearTimeout(timer);
    };
  }, [subscribe, names, delay]);

  const reload = useCallback(() => setNonce((n) => n + 1), []);
  return { state, data, error, refreshing, reload };
}
