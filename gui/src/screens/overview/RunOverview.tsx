import "./overview.css";
import { Link } from "react-router-dom";
import type { EventsResponse, StatusResponse, TaskBrief } from "../../api/types";
import { EventList } from "../../components/EventList";
import { plural } from "../../components/format";
import { ProgressBar } from "../../components/ProgressBar";
import { RunStateBadge } from "../../components/RunStateBadge";
import { StatusBadge } from "../../ui/StatusBadge";

function MilestoneList({ title, tasks, empty }: { title: string; tasks: TaskBrief[]; empty: string }) {
  return (
    <section className="panel" aria-label={title}>
      <h2 className="panel-title">
        {title} ({tasks.length})
      </h2>
      {tasks.length === 0 ? (
        <p className="muted">{empty}</p>
      ) : (
        <ul className="plain-list">
          {tasks.map((t) => (
            <li key={t.id}>
              <Link to={`/execution?task=${encodeURIComponent(t.id)}`}>
                <strong>{t.id}</strong> {t.title}
              </Link>{" "}
              <StatusBadge state={t.state} />
              {t.reason && <div className="muted">{t.reason}</div>}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/** A pending milestone with a dependency that has not landed. */
export function isBlocked(t: TaskBrief, all: TaskBrief[]): boolean {
  if (t.state !== "pending") return false;
  return t.deps.some((d) => {
    const dep = all.find((x) => x.id === d);
    return !dep || (dep.state !== "merged" && dep.state !== "skipped");
  });
}

export interface RunOverviewProps {
  status: StatusResponse;
  events: EventsResponse | null;
  /** The run's control state from the live stream, when known. */
  control?: string | null;
}

/** The overview of one run, from a status answer. */
export function RunOverview({ status, events, control = null }: RunOverviewProps) {
  const total = status.progress.reduce((n, p) => n + p.count, 0);
  const merged = status.progress.find((p) => p.state === "merged")?.count ?? 0;
  const tasks = status.tasks;
  const running = status.workers.filter((w) => w.task !== null);
  const seen = new Set<number>();
  const recent = events
    ? [...events.new, ...events.still_open]
        .filter((e) => !seen.has(e.id) && seen.add(e.id))
        .sort((a, b) => b.ts - a.ts)
        .slice(0, 10)
    : [];

  return (
    <>
      <section className="panel summary" aria-label="Run summary">
        <div className="summary-row">
          <span>
            Branch <code>{status.branch}</code>
            {status.base && (
              <span className="muted">
                {" "}
                (from <code>{status.base}</code>)
              </span>
            )}
          </span>
          <RunStateBadge runState={status.run_state} control={control} />
          {status.stuck && <span className="badge badge--warning">Needs you</span>}
        </div>
        {status.broken_branch && (
          <p className="error-text" role="alert">
            The run branch is broken: {status.broken_branch.reason}
          </p>
        )}
        <dl className="facts">
          <div>
            <dt>Event manager</dt>
            <dd>{status.event_manager.running ? `running${status.event_manager.holder ? ` (${status.event_manager.holder})` : ""}` : "not running"}</dd>
          </div>
          <div>
            <dt>Lead's events</dt>
            <dd>
              {status.events.unread} unread, {status.events.open} open
            </dd>
          </div>
          <div>
            <dt>Mode</dt>
            <dd>
              {status.lead_mode}, {plural(status.max_parallel, "worker")} at a time
            </dd>
          </div>
        </dl>
      </section>

      <section className="panel" aria-label="Progress">
        <h2 className="panel-title">
          Progress: {merged} of {total} merged
        </h2>
        <ProgressBar value={merged} max={total} label="Milestones merged" />
        <ul className="state-counts">
          {status.progress
            .filter((p) => p.count > 0)
            .map((p) => (
              <li key={p.state}>
                <StatusBadge state={p.state} /> {p.count}
              </li>
            ))}
        </ul>
      </section>

      <section className="panel" aria-label="Running workers">
        <h2 className="panel-title">Running workers ({running.length})</h2>
        {running.length === 0 ? (
          <p className="muted">No worker is on a milestone right now.</p>
        ) : (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th scope="col">Worker</th>
                  <th scope="col">Model</th>
                  <th scope="col">Tier</th>
                  <th scope="col">Milestone</th>
                </tr>
              </thead>
              <tbody>
                {running.map((w) => (
                  <tr key={w.name}>
                    <th scope="row">{w.name}</th>
                    <td>{w.model ?? "–"}</td>
                    <td>{w.tier ?? "–"}</td>
                    <td>{w.task}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <div className="grid">
        <MilestoneList
          title="Waiting for an answer"
          tasks={tasks.filter((t) => t.state === "needs-answer" || t.state === "paused")}
          empty="No question is waiting."
        />
        <MilestoneList title="Held" tasks={tasks.filter((t) => t.state === "held")} empty="Nothing is held." />
        <MilestoneList title="Failed" tasks={tasks.filter((t) => t.state === "failed")} empty="Nothing has failed." />
        <MilestoneList title="Blocked" tasks={tasks.filter((t) => isBlocked(t, tasks))} empty="Nothing waits on a dependency." />
        <MilestoneList title="Completed" tasks={tasks.filter((t) => t.state === "merged")} empty="Nothing has merged yet." />
      </div>

      <section className="panel" aria-label="Recent events">
        <h2 className="panel-title">Recent events</h2>
        <EventList events={recent} empty="No events yet." />
      </section>
    </>
  );
}
