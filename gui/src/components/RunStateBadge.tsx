import "./components.css";

const RUN_STATES: Record<string, { label: string; tone: string }> = {
  running: { label: "Running", tone: "info" },
  paused: { label: "Paused", tone: "warning" },
  idle: { label: "Idle", tone: "neutral" },
  all_done: { label: "All done", tone: "success" },
  stopped: { label: "Stopped", tone: "warning" },
  stopping: { label: "Stopping", tone: "warning" },
  broken_branch: { label: "Broken branch", tone: "danger" },
};

/** The run's state in words. `control` (paused/stopping) from the live stream wins while the run is running. */
export function RunStateBadge({ runState, control }: { runState: string; control?: string | null }) {
  const key = (control === "paused" || control === "stopping") && runState === "running" ? control : runState;
  const s = RUN_STATES[key] ?? { label: key, tone: "neutral" };
  return (
    <span className={`badge badge--${s.tone} run-state`} data-run-state={key}>
      {s.label}
    </span>
  );
}
