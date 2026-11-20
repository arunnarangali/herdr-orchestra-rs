import { useConnection, type ConnectionKind } from "./connection";
import { useProjects } from "./projects";

const LABEL: Record<ConnectionKind, string> = {
  unconfigured: "Not connected",
  checking: "Connecting…",
  "no-server": "No server",
  "bad-token": "Bad token",
  "no-run": "No run",
  connected: "Connected",
};

export function connectionLabel(kind: ConnectionKind): string {
  return LABEL[kind];
}

/** What each state means and what to do about it. */
export function advice(kind: ConnectionKind, url: string | null): { title: string; hint: string } | null {
  switch (kind) {
    case "unconfigured":
      return {
        title: "No project chosen yet.",
        hint: "Choose a project folder, or enter the address and token of a running `orch serve`.",
      };
    case "no-server":
      return {
        title: `No server answers${url ? ` at ${url}` : ""}.`,
        hint: "Start `orch serve` in the project, or choose the folder again. This page keeps trying.",
      };
    case "bad-token":
      return {
        title: "The server refused the token.",
        hint: "The token changed or is wrong. Enter the current one (see ~/.config/herdr-orchestra/api-token) or choose the folder again.",
      };
    case "no-run":
      return {
        title: "The server works, but this project has no run yet.",
        hint: "Write .orchestrate/plan.md and run `orch sync`. This page keeps checking.",
      };
    default:
      return null;
  }
}

/** The small label in the top bar. */
export function ConnectionPill() {
  const { kind, health } = useConnection();
  return (
    <span
      className={`chip chip--status pill--${kind}`}
      data-connected={kind === "connected" ? "true" : "false"}
      role="status"
      aria-label={`Connection: ${LABEL[kind]}`}
      title={kind === "connected" && health ? health.project : undefined}
    >
      <span className="pill-dot" aria-hidden="true" />
      <span>{LABEL[kind]}</span>
    </span>
  );
}

/** Explains a problem with the connection under the top bar. */
export function ConnectionBanner() {
  const { kind, credentials, recheck, disconnect } = useConnection();
  const { multi, selected, retry } = useProjects();
  const info = advice(kind, credentials?.url ?? null);
  if (!info || kind === "unconfigured") return null;
  return (
    <div className={`banner banner--${kind}`} role="alert">
      <p className="banner-text">
        <strong>{info.title}</strong> {info.hint}
      </p>
      <div className="banner-actions">
        <button type="button" className="button" onClick={recheck}>
          Check again
        </button>
        {multi && selected ? (
          <button type="button" className="button" onClick={() => void retry(selected)}>
            Restart this project's server
          </button>
        ) : (
          <button type="button" className="button" onClick={disconnect}>
            Change connection
          </button>
        )}
      </div>
    </div>
  );
}
