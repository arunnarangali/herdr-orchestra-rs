import type { StatusResponse } from "../api/types";
import { runStateLabel } from "../screens/projects/format";
import { Dot } from "../ui/Badge";
import { useStream, type ConnectionKind } from "./connection";
import { connectionLabel } from "./ConnectionStatus";

export interface StatusBarProps {
  kind: ConnectionKind;
  /** The run's status, or null while it is unknown (no run, loading, error). */
  status: StatusResponse | null;
  /** Items waiting for the developer, across the open projects. */
  waiting: number;
}

const LIVE = {
  idle: "off",
  connecting: "connecting…",
  connected: "on",
  reconnecting: "lost, retrying…",
  refused: "refused",
} as const;

/** The bottom line: connection, live updates, run state and counts, all from real data. */
export function StatusBar({ kind, status, waiting }: StatusBarProps) {
  const { status: live } = useStream();
  const merged = status?.tasks.filter((t) => t.state === "merged").length ?? 0;
  return (
    <footer id="statusbar" className="statusbar">
      <span className="status__item">
        <Dot tone={kind === "connected" ? "emerald" : kind === "checking" ? "idle" : "coral"} />
        {connectionLabel(kind)}
      </span>
      <span className="status__item status__optional">
        <Dot tone={live === "connected" ? "emerald" : "idle"} pulse={live === "connected"} />
        Stream: {LIVE[live]}
      </span>
      {status && (
        <>
          <span className="status__item">Run: {runStateLabel(status.run_state)}</span>
          <span className="status__item">
            milestones {merged}/{status.tasks.length} merged
          </span>
        </>
      )}
      <span className="status__spacer" />
      <span className="status__item">{waiting === 0 ? "nothing needs you" : `${waiting} need${waiting === 1 ? "s" : ""} attention`}</span>
      {status && <span className="status__item status__optional">lead: {status.lead_mode}</span>}
    </footer>
  );
}
