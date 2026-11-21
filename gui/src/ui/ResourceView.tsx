import type { ReactNode } from "react";
import { ApiError } from "../api/errors";
import type { Resource } from "./useResource";

/** What went wrong, in words, and what to do about it. */
export function describeError(error: ApiError): { title: string; hint: string } {
  if (error.isNetwork) {
    return {
      title: "Cannot reach the server",
      hint: "Start `orch serve` in the project (or choose the folder again), then try again.",
    };
  }
  if (error.isUnauthorized) {
    return {
      title: "The server refused the token",
      hint: "The token changed or is wrong: enter the current one (see ~/.config/herdr-orchestra/api-token) or choose the project folder again.",
    };
  }
  if (error.isNoRun) {
    return {
      title: "No run in this project yet",
      hint: "This page fills in by itself once the lead has planned a run.",
    };
  }
  return { title: error.message, hint: error.requestId ? `Request ${error.requestId}` : "" };
}

export function ErrorPanel({ error, onRetry }: { error: ApiError; onRetry?: () => void }) {
  const { title, hint } = describeError(error);
  // No run yet is a normal state, not a failure: no error styling and nothing to retry.
  if (error.isNoRun) return <NoRunPanel title={title} hint={hint} />;
  return (
    <div className="panel panel--error" role="alert">
      <p className="panel-title">{title}</p>
      {hint && <p className="muted">{hint}</p>}
      {onRetry && (
        <button type="button" className="button" onClick={onRetry}>
          Try again
        </button>
      )}
    </div>
  );
}

/** The calm panel for a project that has no run yet. */
export function NoRunPanel({ title = "No run in this project yet", hint }: { title?: string; hint?: string }) {
  return (
    <div className="panel" role="status">
      <p className="panel-title">{title}</p>
      {hint && <p className="muted">{hint}</p>}
    </div>
  );
}

export interface ResourceViewProps<T> {
  resource: Resource<T>;
  /** Shown when the data is empty. */
  empty?: ReactNode;
  /** Shown while the first load is in flight. */
  loading?: ReactNode;
  children: (data: T) => ReactNode;
}

/** Renders a `useResource` result: loading, error (with retry), empty, or the data. */
export function ResourceView<T>({ resource, empty, loading, children }: ResourceViewProps<T>) {
  switch (resource.state) {
    case "loading":
      return (
        <div role="status" className="muted">
          {loading ?? "Loading…"}
        </div>
      );
    case "error":
      return resource.error ? <ErrorPanel error={resource.error} onRetry={resource.reload} /> : null;
    case "empty":
      return <p className="muted">{empty ?? "Nothing to show yet."}</p>;
    case "ready":
      return resource.data === null ? null : <>{children(resource.data)}</>;
  }
}
