import { useEffect, useState } from "react";
import type { StatusResponse, TaskInfo } from "../../api/types";
import { RunControls } from "./RunControls";
import { ExecIcon } from "./icons";
import { budgetSeconds, clock } from "./time";

export interface MonitorState {
  key: string;
  label: string;
  tone: "emerald" | "amber" | "coral" | "cyan" | "neutral";
}

/** One state for the header badge: the stream's control state (paused, stopping) wins over the read. */
export function monitorState(runState: string | null, control: string | null): MonitorState {
  if (control === "paused") return { key: "paused", label: "Paused", tone: "amber" };
  if (control === "stopping") return { key: "stopping", label: "Stopping", tone: "coral" };
  if (control === "stopped") return { key: "stopped", label: "Stopped", tone: "coral" };
  switch (runState) {
    case "running":
      return { key: "live", label: "Live", tone: "emerald" };
    case "idle":
      return { key: "idle", label: "Idle", tone: "neutral" };
    case "all_done":
      return { key: "done", label: "All done", tone: "emerald" };
    case "stopped":
      return { key: "stopped", label: "Stopped", tone: "coral" };
    case "broken_branch":
      return { key: "broken", label: "Broken branch", tone: "coral" };
    default:
      return { key: "unknown", label: control === "running" ? "Live" : "State unknown", tone: control === "running" ? "emerald" : "neutral" };
  }
}

/** The run's timing as `/report` gives it: when it started and, once over, how long it took. */
export interface RunTiming {
  started: number | null;
  finished: number | null;
  elapsed_s: number | null;
}

/** Elapsed time, ticking once a second while the run is live; only this text re-renders. */
function Elapsed({ timing, ticking, budget }: { timing: RunTiming | null; ticking: boolean; budget: number | null }) {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    if (!ticking) return;
    const t = setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => clearInterval(t);
  }, [ticking]);

  let secs: number | null = null;
  if (timing?.started != null && timing.finished == null && ticking) secs = now - timing.started;
  else if (timing?.elapsed_s != null) secs = timing.elapsed_s;
  else if (timing?.started != null) secs = (timing.finished ?? now) - timing.started;

  const over = secs !== null && budget !== null && secs > budget;
  return (
    <strong className={`mono${over ? " over-budget" : ""}`} id="elapsed">
      {secs === null ? "–" : clock(secs)}
      {over && <span className="visually-hidden"> (over budget)</span>}
    </strong>
  );
}

/** The run header, the progress card and the run's controls (prototype screen 03, top). */
export function RunProgress({
  status,
  tasks,
  budget,
  timing,
  control,
  onControl,
  onChange,
}: {
  status: StatusResponse | null;
  tasks: TaskInfo[];
  budget: string | null;
  timing: RunTiming | null;
  control: string | null;
  onControl: (state: string) => void;
  onChange: () => void;
}) {
  const st = monitorState(status?.run_state ?? null, control);
  const live = st.key === "live";
  const total = tasks.length;
  const merged = tasks.filter((t) => t.state === "merged").length;
  const active = tasks.filter((t) => t.state === "coding" || t.state === "verifying").length;
  const percent = total > 0 ? Math.round((merged / total) * 100) : 0;
  const barTone = st.key === "paused" ? " bar--paused" : st.tone === "coral" ? " bar--stopped" : "";
  const budgetSecs = budgetSeconds(budget);

  return (
    <section className="mon__top" aria-label="Run controls">
      <header className="mon__head">
        <span className="mon__icon">
          <ExecIcon name="cube" size={34} />
        </span>
        <div>
          <h2 className="mon__title">
            <span>Execution Monitor{status?.title ? ` — ${status.title}` : ""}</span>{" "}
            <span className={`badge badge--pill badge--${st.tone}`} id="mon-live" data-run-state={st.key}>
              <span className={`dot dot--${st.tone === "neutral" ? "idle" : st.tone}${live ? " dot--pulse" : ""}`} aria-hidden="true" />
              {st.label}
            </span>
          </h2>
          {status && (
            <p className="mon__sub">
              Branch: <span className="mono">{status.branch}</span>
              {status.base ? (
                <>
                  {" "}
                  · Base: <span className="mono">{status.base}</span>
                </>
              ) : null}
              {" · "}Isolation: <span className="mono">{status.isolation}</span>
              {" · "}Event manager: {status.event_manager.running ? "running" : "not running"}
            </p>
          )}
        </div>
      </header>

      <div className="card progress">
        <div className="progress__label">
          <ExecIcon name={live ? "spinner" : "checkcircle"} size={26} className={live ? "spin" : ""} />
          <strong>Run Progress</strong>
        </div>

        <div className="progress__bar">
          <span id="prog-text">
            {percent}% ({merged}/{total} Merged, {active} Coding)
          </span>
          <div
            className={`bar${barTone}`}
            role="progressbar"
            aria-label="Run progress"
            aria-valuenow={percent}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuetext={`${merged} of ${total} milestones merged`}
          >
            <i style={{ width: `${percent}%` }} />
          </div>
        </div>

        <div className="progress__time">
          <ExecIcon name="clock" size={18} />
          <span>
            Elapsed: <Elapsed timing={timing} ticking={live} budget={budgetSecs} /> / Budget:{" "}
            <strong className="mono" id="budget">
              {budget ? (budgetSecs !== null ? clock(budgetSecs) : budget) : "not set"}
            </strong>
          </span>
        </div>

        <RunControls control={control} onControl={onControl} onChange={onChange} />
      </div>
    </section>
  );
}
